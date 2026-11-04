"""The remote player's server (docs/ai-net-model.md, «Remote MPS player»):
a float checkpoint of any preset (v2 above all, which has no integer twin)
serves the decisions of Rust net players over a Unix socket, on MPS.

    python -m aurelia_nn.remote serve CKPT --socket PATH [--device mps]
        serve until killed (SIGTERM / SIGINT print the counters)
    python -m aurelia_nn.remote check CKPT --shards DIR [--device mps]
        faithfulness: the server's choices on recorded turns against the
        offline evaluation (train_imitation.run_windows, as v2_bench eval)
    python -m aurelia_nn.remote stats --socket PATH [--reset]
        the counters of a running server (JSON)

The Rust side is `crates/classic-rules/src/ai/net_e2e/remote.rs`
(`net:remote:PATH` on the bench). Each Rust net player opens one
connection and asks one thing at a time; a player's sessions (one per
nation it plays: its memory, its turn, its turn trace) live here, keyed by
the connection and the session id, and die with the connection.

Wire format (little-endian). A frame is u32 length, then the payload. A
request's payload starts with a u8 kind; a reply's with a u8 status (0 ok,
1 error, then the message in UTF-8).

- HELLO (1): u32 protocol version → u32 version, u64 layout hash, u32
  decision heads, u8 defer, u8 coarse, u8 events, u32 c_tile, c_global,
  c_entity, c_candidate, c_event, u32 length + the label.
- BEGIN (2): u64 session, u64 map key, u32 tiles, u8 has map [neighbors
  i32 × tiles × 6, region i32 × tiles], tile rows i16 × tiles × c_tile,
  global row i16 × c_global, u32 entities, rows i16 × E × c_entity, meta
  i32 × E × 3 (kind, tile, relation), ids i32 × E, u32 events, rows i16 ×
  V × c_event, meta i32 × V × 4 → i32 value (Q8). The turn's start: the
  memory is read, the tokens made, the memory after the turn written (it
  depends on the start only, as `model.rs` `end_turn`), the trace zeroed.
- SCORE (3): u64 session, i32 note (the index of the candidate the
  previous decision of the session's turn took, −1 none: the turn trace),
  u32 head, i32 actor, u32 k, rows i16 × k × c_candidate, meta i32 × k × 4
  → i32 × k logits (Q8, round(logit × 256)).
- STATS (4): u8 reset → JSON counters.

Requests that arrive together (from many games) run as one batch: BEGINs
of one map through `collate` and the net's `encode`, SCOREs of one map
through one `score` call; the turn trace is the training's
(`Net.turn_trace`: W_tr · clamp(S / 2^shift, ±128), S the sum of the chosen
candidates' encodings clamped to ±128, waits and deferrals left out), the
memory the training's (`MemoryBank`: nations kept, units of the last turn).
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import os
import selectors
import signal
import socket
import struct
import sys
import time
from pathlib import Path

import numpy as np
import torch

from .common import ACT_BITS, DEFER_TAG, HEAD_UNIT, NATION_SLOT_KINDS, UNIT_SLOT_KINDS, WAIT_TAG, param_index, tag_index
from .data import Layout, Turn, Uploader, bucket, collate, head_groups
from .model import CLIP, load_checkpoint

VERSION = 1
HELLO, BEGIN, SCORE, STATS = 1, 2, 3, 4
OK, ERROR = 0, 1
Q8 = float(1 << ACT_BITS)


class Wire:
    """A cursor over a request's bytes."""

    def __init__(self, data: bytes | memoryview):
        self.data = memoryview(data)
        self.at = 0

    def take(self, n: int) -> memoryview:
        if self.at + n > len(self.data):
            raise ValueError(f"short message: {n} bytes wanted at {self.at} of {len(self.data)}")
        out = self.data[self.at : self.at + n]
        self.at += n
        return out

    def u8(self) -> int:
        return self.take(1)[0]

    def u32(self) -> int:
        return struct.unpack("<I", self.take(4))[0]

    def i32(self) -> int:
        return struct.unpack("<i", self.take(4))[0]

    def u64(self) -> int:
        return struct.unpack("<Q", self.take(8))[0]

    def array(self, dtype, count: int, shape) -> np.ndarray:
        dt = np.dtype(dtype).newbyteorder("<")
        return np.frombuffer(self.take(count * dt.itemsize), dtype=dt).reshape(shape).astype(np.dtype(dtype), copy=True)


@dataclasses.dataclass
class MapEntry:
    neighbors: np.ndarray  # i32 [T, 6]
    region: np.ndarray  # i32 [T]
    n_regions: int
    nb_dev: torch.Tensor  # long [T, 6] on the device


class Session:
    """One nation of one game: its memory and its turn."""

    def __init__(self, cfg, device):
        self.cfg = cfg
        z = lambda *s: torch.zeros(*s, device=device)
        self.player = z(cfg.mem_player)
        self.nation_ids: dict[int, int] = {}
        self.nations = z(0, cfg.mem_nation)
        self.unit_ids: dict[int, int] = {}
        self.units = z(0, cfg.mem_unit)
        self.turn: dict | None = None
        self.trace = z(cfg.d_model)
        self.last_enc: torch.Tensor | None = None  # the last decision's candidate encodings, clamped
        # --int-ref: the NumPy integer reference's memory, turn and last decision.
        self.int_mem = None
        self.int_st = None
        self.int_last = None


class Conn:
    def __init__(self, sock: socket.socket):
        self.sock = sock
        self.buf = bytearray()
        self.maps: dict[int, MapEntry] = {}
        self.sessions: dict[int, Session] = {}
        self.pending = None  # (kind, Wire, received at)


class Player:
    """The checkpoint on its device and the batched forward of BEGINs and
    SCOREs (no sockets: the server and the tests drive it)."""

    def __init__(self, checkpoint: str, device: str = "mps", int_ref: bool = False):
        self.device = torch.device(device)
        ck = torch.load(checkpoint, map_location="cpu", weights_only=False)
        self.layout = Layout.from_json(ck["layout"])
        model = load_checkpoint(checkpoint)
        # Training-only parts off: the candidate aux head (its targets are read off the rows).
        model.cfg = dataclasses.replace(model.cfg, cand_aux=0)
        self.model = model.to(self.device).eval()
        self.cfg = model.cfg
        if self.cfg.engram or self.cfg.aim:
            raise ValueError("the remote player does not build engram or aim inputs")
        self.label = f"{Path(checkpoint).parent.name}/{Path(checkpoint).name}"
        # Copies queued, not waited for (a blocking copy to MPS waits for the device's queue);
        # the host arrays are kept until the reply's read-back, then `sync`.
        self.upload = Uploader(self.device, blocking=False)
        _cache_distance_buckets(self.device)
        self._enc = None
        self.model.cand.register_forward_hook(lambda _m, _i, out: setattr(self, "_enc", out))
        # The turn trace from the sums the server keeps (Net.turn_trace's teacher-forced
        # indices are training's): batch["trace_s"] [D, d_model] holds S per decision.
        shift = 1.0 / (1 << self.cfg.trace_shift)
        if self.cfg.trace:
            teacher_forced = self.model.turn_trace
            self.model.turn_trace = lambda batch, enc, n: (self.model.trace((batch["trace_s"] * shift).clamp(-CLIP, CLIP)) if "trace_s" in batch
                                                           else teacher_forced(batch, enc, n))
        self.trace_heads = self.cfg.trace_heads if self.cfg.trace else 0
        # --int-ref (a check of the first net's presets): the NumPy integer reference
        # (quantize.py, which the Rust int8 player equals bit for bit) on the same
        # requests; per head, how often its argmax is the float net's.
        self.qm = None
        self.int_counts: dict[int, list[int]] = {}
        if int_ref:
            from .quantize import quantize

            self.qm = quantize(load_checkpoint(checkpoint), self.layout)

    def hello(self) -> bytes:
        c = self.cfg
        label = self.label.encode()
        return struct.pack("<BIQIBBBIIIIII", OK, VERSION, self.layout.hash(), c.n_decision_heads, int(bool(c.defer)), int(bool(c.coarse)), int(bool(c.events)),
                           c.c_tile, c.c_global, c.c_entity, c.c_candidate, c.c_event if c.events else 0, len(label)) + label

    # ------------------------------------------------------------ turns

    def begin(self, items: list[tuple[Session, Turn, MapEntry]]) -> list[int]:
        """The turns' start for sessions of one map: tokens, value, the
        memory after the turn. Returns the values (Q8)."""
        cfg = self.cfg
        turns = [t for _, t, _ in items]
        batch = collate(turns, cfg, upload=self.upload, pad=True)
        bsz, e_max = batch["ent_id"].shape
        ids = batch["ent_id"]
        kinds = np.full((bsz, e_max), -1, np.int64)
        for i, t in enumerate(turns):
            kinds[i, : len(t.entity_meta)] = t.entity_meta[:, 0]
        is_n = np.isin(kinds, NATION_SLOT_KINDS) & (ids >= 0)
        is_u = np.isin(kinds, UNIT_SLOT_KINDS) & (ids >= 0)
        # The memory read: a pool of every session's slots and a zero row.
        sessions = [s for s, _, _ in items]
        mem_n = self._gather(sessions, ids, is_n, "nation", cfg.mem_nation)
        mem_u = self._gather(sessions, ids, is_u, "unit", cfg.mem_unit)
        batch["mem_player"] = torch.stack([s.player for s in sessions])
        batch["mem_nation"], batch["mem_unit"] = mem_n, mem_u
        tile_h, tokens, off = self.model.encode(batch)
        value = self.model.value(batch, tokens, off)
        new_p, new_n, new_u = self.model.update_memory(batch, tokens, off)
        for i, s in enumerate(sessions):
            s.player = new_p[i]
            # Nations: the turn's written (a repeated id: its last row), the others kept.
            rows_n = self._last_rows(ids[i], is_n[i])
            keep = [(k, r) for k, r in s.nation_ids.items() if k not in rows_n]
            parts = [s.nations.index_select(0, self.upload(np.asarray([r for _, r in keep], np.int64)))] if keep else []
            if rows_n:
                parts.append(new_n[i].index_select(0, self.upload(np.asarray(list(rows_n.values()), np.int64))))
            order = [k for k, _ in keep] + list(rows_n)
            s.nations = torch.cat(parts) if parts else s.nations[:0]
            s.nation_ids = {k: j for j, k in enumerate(order)}
            # Units: the turn's, the others dropped.
            rows_u = self._last_rows(ids[i], is_u[i])
            s.units = new_u[i].index_select(0, self.upload(np.asarray(list(rows_u.values()), np.int64))) if rows_u else s.units[:0]
            s.unit_ids = {k: j for j, k in enumerate(rows_u)}
            n_ent = len(turns[i].entities)
            s.turn = {
                "map": items[i][2],
                "n_ent": n_ent,
                "e_pad": e_max,
                "tile_h": tile_h[i : i + 1],
                "tokens": tokens[i : i + 1],
                "tiles": batch["tiles"][i : i + 1],
                "entities": batch["entities"][i : i + 1],
                "ent_kind": batch["ent_kind"][i : i + 1],
                "ent_tile": batch["ent_tile"][i : i + 1],
                "ent_mask": batch["ent_mask"][i : i + 1],
                "r": int(batch["n_regions"]),
            }
            s.trace = torch.zeros(cfg.d_model, device=self.device)
            s.last_enc = None
            if self.qm is not None:
                from .quantize import Memory, encode_turn, end_turn

                t = turns[i]
                st = encode_turn(self.qm, t.tiles, t.neighbors, t.region, t.global_row, t.entities, t.entity_meta, t.entity_ids,
                                 s.int_mem or Memory.empty(cfg), events=t.events, event_meta=t.event_meta)
                s.int_mem = end_turn(self.qm, st, s.int_mem or Memory.empty(cfg))
                s.int_st, s.int_last = st, None
        out = [int(round(v * Q8)) for v in value.float().cpu().tolist()]
        self.upload.sync()
        return out

    def _gather(self, sessions, ids, is_slot, kind: str, width: int) -> torch.Tensor:
        bsz, e_max = ids.shape
        tables = [getattr(s, kind + "s") for s in sessions]
        offsets = np.cumsum([0] + [len(t) for t in tables])
        zero = int(offsets[-1])
        idx = np.full((bsz, e_max), zero, np.int64)
        for b, s in enumerate(sessions):
            known = getattr(s, kind + "_ids")
            if not known:
                continue
            for e in np.nonzero(is_slot[b])[0]:
                j = known.get(int(ids[b, e]))
                if j is not None:
                    idx[b, e] = offsets[b] + j
        pool = torch.cat(tables + [torch.zeros(1, width, device=self.device)])
        return pool.index_select(0, self.upload(idx.reshape(-1))).view(bsz, e_max, width)

    @staticmethod
    def _last_rows(ids: np.ndarray, sel: np.ndarray) -> dict[int, int]:
        """id → its last entity row among `sel` (entity order)."""
        out: dict[int, int] = {}
        for e in np.nonzero(sel)[0]:
            out[int(ids[e])] = int(e)
        return out

    # ------------------------------------------------------------ decisions

    def note(self, s: Session, at: int):
        """The session's last decision took candidate `at`: the turn trace."""
        if self.qm is not None and s.int_last is not None and at >= 0:
            from .quantize import note_choice

            note_choice(self.qm, s.int_st, *s.int_last, at)
        if at < 0 or s.last_enc is None:
            return
        enc, head, tags = s.last_enc
        if at >= len(enc) or not (self.trace_heads >> head) & 1 or int(tags[at]) in (WAIT_TAG, DEFER_TAG):
            return
        s.trace = s.trace + enc[at]

    def score(self, items: list[tuple[Session, int, int, np.ndarray, np.ndarray]]) -> list[np.ndarray]:
        """One decision per session (sessions of one map): (session, head,
        actor, rows i16 [k, c_candidate], meta i32 [k, 4]) → logits Q8 i32 [k]."""
        cfg = self.cfg
        turns = [s.turn for s, *_ in items]
        mp = turns[0]["map"]
        n_tiles = len(mp.region)
        r = turns[0]["r"]
        e_max = max(t["e_pad"] for t in turns)
        d = cfg.d_model
        b = len(items)
        tok, tile_h, tiles, ents, kind, etile, emask = [], [], [], [], [], [], []
        for t in turns:
            e = t["e_pad"]
            pe = e_max - e
            x = t["tokens"]
            if pe:
                z = x.new_zeros(1, pe, d)
                off = cfg.offsets(r, 0, e)
                x = torch.cat([x[:, : off["global"]], z, x[:, off["global"] : off["slots"]], x[:, off["slots"] :], z], dim=1)
            tok.append(x)
            tile_h.append(t["tile_h"])
            tiles.append(t["tiles"])
            pad = lambda a, v: torch.cat([a, a.new_full((1, pe) + tuple(a.shape[2:]), v)], dim=1) if pe else a
            ents.append(pad(t["entities"], 0))
            kind.append(pad(t["ent_kind"], -1))
            etile.append(pad(t["ent_tile"], -1))
            emask.append(pad(t["ent_mask"], False))
        k = np.asarray([len(rows) for _, _, _, rows, _ in items], np.int64)
        n_cand = int(k.sum())
        heads = np.asarray([h for _, h, *_ in items], np.int64)
        actors = np.asarray([a if 0 <= a < t["n_ent"] else -1 for (_, _, a, _, _), t in zip(items, turns)], np.int64)
        rows = np.concatenate([x[3] for x in items]).astype(np.int16, copy=False) if n_cand else np.zeros((0, cfg.c_candidate), np.int16)
        meta = np.concatenate([x[4] for x in items]).astype(np.int64) if n_cand else np.zeros((0, 4), np.int64)
        cand_dec = np.repeat(np.arange(b, dtype=np.int64), k)
        starts = np.cumsum(k) - k
        cand_pos = np.arange(n_cand, dtype=np.int64) - starts[cand_dec]
        n_ent_c = np.asarray([t["n_ent"] for t in turns], np.int64)[cand_dec]
        ctile = np.where((meta[:, 1] >= 0) & (meta[:, 1] < n_tiles), meta[:, 1], -1)
        cent = np.where((meta[:, 2] >= 0) & (meta[:, 2] < n_ent_c), meta[:, 2], -1)
        heads_c = heads[cand_dec]
        # Padding: the candidate count and the longest list to bucket sizes (fewer MPS shapes);
        # padding candidates belong to no head and no decision's list.
        nh = cfg.n_decision_heads
        c_pad = bucket(n_cand) - n_cand
        k_max = bucket(int(k.max()), 8)
        more = lambda a, v: np.concatenate([a, np.full((c_pad,) + a.shape[1:], v, a.dtype)])
        rows_p = more(rows, 0)
        meta_p = more(meta, -1)
        heads_cp = more(heads_c, nh)
        pad_index = np.full((b, k_max), n_cand + c_pad, np.int64)
        pad_index[cand_dec, cand_pos] = np.arange(n_cand)
        # Every index array in one copy, the rows in another (each copy to MPS costs ~0.1 ms).
        emb_head = np.minimum(heads_cp, nh - 1)
        packer = Packer()
        dec_order, dec_spans, dec_inverse = head_groups(heads, nh, packer)
        cand_order, cand_spans, cand_inverse = head_groups(heads_cp, nh, packer)
        ints = {
            "dec_b": np.arange(b, dtype=np.int64), "dec_head": heads, "dec_actor": actors, "dec_k": k, "pad_index": pad_index,
            "cand_tag": tag_index(emb_head, meta_p[:, 0], cfg), "cand_param": param_index(emb_head, meta_p[:, 3], cfg),
            "cand_tile": more(ctile, -1), "cand_ent": more(cent, -1), "cand_dec": more(cand_dec, 0), "cand_pos": more(cand_pos, 0),
            "cand_defer": ((heads_cp == HEAD_UNIT) & (meta_p[:, 0] == DEFER_TAG)).astype(np.int64),
        }
        dev = packer.upload(ints, self.upload)
        batch = {
            "tiles": torch.cat(tiles),
            "neighbors": mp.nb_dev,
            "entities": torch.cat(ents),
            "ent_kind": torch.cat(kind),
            "ent_tile": torch.cat(etile),
            "ent_mask": torch.cat(emask),
            "k_max": k_max,
            "cand_rows": self.upload(rows_p, torch.float32),
            "dec_groups": (dev["#0"], dec_spans, dev["#1"]),
            "cand_groups": (dev["#2"], cand_spans, dev["#3"]),
        } | {key: dev[key] for key in ints}
        batch["cand_defer"] = batch["cand_defer"].bool()
        if cfg.trace:
            batch["trace_s"] = torch.stack([s.trace for s, *_ in items])
        tokens = torch.cat(tok)
        off = cfg.offsets(r, 0, e_max)
        off["e_max"] = e_max
        logits = self.model.score(batch, torch.cat(tile_h), tokens, off)
        enc = self._enc.clamp(-CLIP, CLIP)
        q8 = (logits[:n_cand].float() * Q8).round().clamp(-(2**31), 2**31 - 1).to(torch.int64).cpu().numpy()
        out = []
        for i, (s, h, _a, _rows, m) in enumerate(items):
            a, n = int(starts[i]), int(k[i])
            s.last_enc = (enc[a : a + n], int(h), m[:, 0].copy())
            out.append(q8[a : a + n].astype(np.int32))
            if self.qm is not None:
                from .quantize import score as int_score

                im = m.astype(np.int64).copy()
                im[:, 1] = ctile[a : a + n]
                im[:, 2] = cent[a : a + n]
                act = int(actors[i])
                il = int_score(self.qm, s.int_st, int(h), act, _rows, im)
                c = self.int_counts.setdefault(int(h), [0, 0])
                c[0] += int(np.argmax(il) == np.argmax(out[-1]))
                c[1] += 1
                s.int_last = (int(h), act, _rows, im)
        self.upload.sync()
        return out


class Packer:
    """Int64 arrays gathered for one copy to the device: as an `upload`
    (data.head_groups takes one) it stands for its array by a placeholder
    `#i`; `upload` copies them all at once and returns name → device slice."""

    def __init__(self):
        self.arrays: list[np.ndarray] = []

    def __call__(self, a, dtype=None):
        self.arrays.append(np.asarray(a, np.int64).reshape(-1))
        return f"#{len(self.arrays) - 1}"

    def upload(self, named: dict, upload) -> dict:
        parts = list(self.arrays) + [np.asarray(v, np.int64) for v in named.values()]
        flat = upload(np.concatenate([p.reshape(-1) for p in parts]) if parts else np.zeros(0, np.int64))
        out, at = {}, 0
        names = [f"#{i}" for i in range(len(self.arrays))] + list(named)
        for name, p in zip(names, parts):
            out[name] = flat[at : at + p.size].view(p.shape)
            at += p.size
        return out


def _cache_distance_buckets(device):
    """model_v2.distance_bucket with its table on the device once (it copies
    the table from the host every call: a wait on MPS)."""
    from . import model_v2

    if getattr(model_v2.distance_bucket, "cached", False):
        return
    table = torch.as_tensor(model_v2._BUCKET_OF, device=device)
    original = model_v2.distance_bucket

    def distance_bucket(d, ok):
        if d.device != table.device:
            return original(d, ok)
        return torch.where(ok, table[d.clamp(0, 63)], model_v2.NO_POSITION)

    distance_bucket.cached = True
    model_v2.distance_bucket = distance_bucket


def read_begin(w: Wire, cfg, conn: Conn, device) -> tuple[int, Turn, MapEntry]:
    session = w.u64()
    key = w.u64()
    n_tiles = w.u32()
    if w.u8():
        nb = w.array(np.int32, n_tiles * 6, (n_tiles, 6))
        region = w.array(np.int32, n_tiles, (n_tiles,))
        conn.maps[key] = MapEntry(nb, region, int(region.max()) + 1 if n_tiles else 0, torch.as_tensor(nb.astype(np.int64), device=device))
    mp = conn.maps.get(key)
    if mp is None or len(mp.region) != n_tiles:
        raise ValueError(f"map {key:#x} of {n_tiles} tiles not sent on this connection")
    tiles = w.array(np.int16, n_tiles * cfg.c_tile, (n_tiles, cfg.c_tile))
    glob = w.array(np.int16, cfg.c_global, (cfg.c_global,))
    e = w.u32()
    ents = w.array(np.int16, e * cfg.c_entity, (e, cfg.c_entity))
    emeta = w.array(np.int32, e * 3, (e, 3))
    ids = w.array(np.int32, e, (e,))
    v = w.u32()
    c_ev = cfg.c_event if cfg.events else 0
    ev = w.array(np.int16, v * c_ev, (v, c_ev))
    evm = w.array(np.int32, v * 4, (v, 4))
    turn = Turn(tiles=tiles, neighbors=mp.neighbors, region=mp.region, n_regions=mp.n_regions, global_row=glob, entities=ents, entity_meta=emeta,
                entity_ids=ids, decisions=[], events=ev if cfg.events else None, event_meta=evm if cfg.events else None)
    return session, turn, mp


def read_score(w: Wire, cfg):
    session = w.u64()
    note = w.i32()
    head = w.u32()
    actor = w.i32()
    k = w.u32()
    rows = w.array(np.int16, k * cfg.c_candidate, (k, cfg.c_candidate))
    meta = w.array(np.int32, k * 4, (k, 4))
    return session, note, head, actor, rows, meta


class Counters:
    def __init__(self):
        self.reset()

    def reset(self):
        self.t0 = time.time()
        self.n = {"begin": 0, "score": 0, "candidates": 0}
        self.batches = {"begin": 0, "score": 0}
        self.busy = {"begin": 0.0, "score": 0.0}
        self.latency = {"begin": [], "score": []}
        self.batch_sizes = {"begin": [], "score": []}

    def report(self, device) -> dict:
        wall = max(time.time() - self.t0, 1e-9)
        lat = lambda xs, p: float(np.percentile(xs, p) * 1000) if xs else None
        out = {"seconds": wall}
        for k in ("begin", "score"):
            out[k] = {"requests": self.n[k], "batches": self.batches[k], "per_second": self.n[k] / wall, "busy_share": self.busy[k] / wall,
                      "mean_batch": float(np.mean(self.batch_sizes[k])) if self.batch_sizes[k] else None,
                      "ms_per_batch": self.busy[k] / max(self.batches[k], 1) * 1000,
                      "latency_ms_p50": lat(self.latency[k], 50), "latency_ms_p90": lat(self.latency[k], 90), "latency_ms_p99": lat(self.latency[k], 99)}
        out["candidates"] = self.n["candidates"]
        if device.type == "mps":
            out["mps_driver_mb"] = torch.mps.driver_allocated_memory() / 2**20
            out["mps_current_mb"] = torch.mps.current_allocated_memory() / 2**20
        return out


def send(sock: socket.socket, payload: bytes):
    data = struct.pack("<I", len(payload)) + payload
    view = memoryview(data)
    while view:
        try:
            n = sock.send(view)
            view = view[n:]
        except BlockingIOError:
            selectors.DefaultSelector().select(0.01)


def error(msg: str) -> bytes:
    return bytes([ERROR]) + msg.encode()


class Server:
    def __init__(self, player: Player, path: str, gather_us: int = 300):
        self.player = player
        self.path = path
        self.gather = gather_us / 1e6
        self.sel = selectors.DefaultSelector()
        self.conns: dict[int, Conn] = {}
        self.counters = Counters()
        self.peak_mb = 0.0

    def serve(self):
        if os.path.exists(self.path):
            os.unlink(self.path)
        lst = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        lst.bind(self.path)
        lst.listen(512)
        lst.setblocking(False)
        self.sel.register(lst, selectors.EVENT_READ, None)
        print(f"serving {self.player.label} on {self.path} ({self.player.device})", flush=True)
        first = None
        while True:
            timeout = None if first is None else max(0.0, first + self.gather - time.perf_counter())
            for key, _ in self.sel.select(timeout):
                if key.data is None:
                    sock, _ = lst.accept()
                    sock.setblocking(False)
                    c = Conn(sock)
                    self.conns[sock.fileno()] = c
                    self.sel.register(sock, selectors.EVENT_READ, c)
                else:
                    self.read(key.data)
            waiting = [c for c in self.conns.values() if c.pending is not None]
            if waiting and first is None:
                first = time.perf_counter()
            # Run when the gather window is over or every connection waits.
            if waiting and (time.perf_counter() >= first + self.gather or len(waiting) == len(self.conns)):
                self.run(waiting)
                first = None

    def read(self, c: Conn):
        try:
            data = c.sock.recv(1 << 20)
        except (BlockingIOError, InterruptedError):
            return
        except OSError:
            data = b""
        if not data:
            self.close(c)
            return
        c.buf += data
        if len(c.buf) >= 4:
            n = struct.unpack_from("<I", c.buf)[0]
            if len(c.buf) >= 4 + n:
                if c.pending is not None:
                    send(c.sock, error("a request before the reply to the last one"))
                    self.close(c)
                    return
                payload = bytes(c.buf[4 : 4 + n])
                del c.buf[: 4 + n]
                c.pending = (payload[0], Wire(payload[1:]), time.perf_counter())

    def close(self, c: Conn):
        try:
            self.sel.unregister(c.sock)
        except (KeyError, ValueError):
            pass
        self.conns.pop(c.sock.fileno(), None)
        c.sock.close()

    def reply(self, c: Conn, payload: bytes, kind: str | None = None):
        if kind is not None:
            self.counters.latency[kind].append(time.perf_counter() - c.pending[2])
        c.pending = None
        try:
            send(c.sock, payload)
        except OSError:
            self.close(c)

    def run(self, waiting: list[Conn]):
        p = self.player
        begins: list = []
        scores: list = []
        for c in waiting:
            kind, w, _ = c.pending
            try:
                if kind == HELLO:
                    v = w.u32()
                    self.reply(c, p.hello() if v == VERSION else error(f"protocol {v}, the server speaks {VERSION}"))
                elif kind == STATS:
                    reset = w.u8()
                    rep = self.counters.report(p.device) | {"peak_mps_driver_mb": self.peak_mb, "connections": len(self.conns),
                                                             "sessions": sum(len(x.sessions) for x in self.conns.values())}
                    if p.qm is not None:
                        rep["int_ref_agree_of"] = {str(h): v for h, v in p.int_counts.items()}
                    if reset:
                        self.counters.reset()
                    self.reply(c, bytes([OK]) + json.dumps(rep).encode())
                elif kind == BEGIN:
                    sid, turn, mp = read_begin(w, p.cfg, c, p.device)
                    s = c.sessions.get(sid) or c.sessions.setdefault(sid, Session(p.cfg, p.device))
                    begins.append((c, s, turn, mp))
                elif kind == SCORE:
                    sid, note, head, actor, rows, meta = read_score(w, p.cfg)
                    s = c.sessions.get(sid)
                    if s is None or s.turn is None:
                        raise ValueError(f"session {sid}: SCORE before BEGIN")
                    p.note(s, note)
                    scores.append((c, s, head, actor, rows, meta))
                else:
                    raise ValueError(f"unknown request kind {kind}")
            except Exception as e:  # noqa: BLE001 - the client gets the message
                self.reply(c, error(f"{type(e).__name__}: {e}"))
        with torch.inference_mode():
            # BEGINs and SCOREs of one map (the same neighbour and region tables) together.
            for group in self._by_map(begins, lambda x: x[3]):
                t = time.perf_counter()
                try:
                    values = p.begin([(s, turn, mp) for _, s, turn, mp in group])
                    for (c, *_), v in zip(group, values):
                        self.reply(c, struct.pack("<Bi", OK, v), "begin")
                except Exception as e:  # noqa: BLE001
                    for c, *_ in group:
                        self.reply(c, error(f"{type(e).__name__}: {e}"))
                self.counters.busy["begin"] += time.perf_counter() - t
                self.counters.batches["begin"] += 1
                self.counters.n["begin"] += len(group)
                self.counters.batch_sizes["begin"].append(len(group))
            for group in self._by_map(scores, lambda x: x[1].turn["map"]):
                t = time.perf_counter()
                try:
                    outs = p.score([(s, h, a, rows, meta) for _, s, h, a, rows, meta in group])
                    for (c, *_), o in zip(group, outs):
                        self.reply(c, bytes([OK]) + o.astype("<i4").tobytes(), "score")
                    self.counters.n["candidates"] += sum(len(o) for o in outs)
                except Exception as e:  # noqa: BLE001
                    for c, *_ in group:
                        self.reply(c, error(f"{type(e).__name__}: {e}"))
                self.counters.busy["score"] += time.perf_counter() - t
                self.counters.batches["score"] += 1
                self.counters.n["score"] += len(group)
                self.counters.batch_sizes["score"].append(len(group))
        if p.device.type == "mps":
            self.peak_mb = max(self.peak_mb, torch.mps.driver_allocated_memory() / 2**20)

    @staticmethod
    def _by_map(items: list, map_of) -> list[list]:
        """The items grouped by their map's neighbour and region tables."""
        groups: list[tuple[MapEntry, list]] = []
        for x in items:
            mp = map_of(x)
            for ref, g in groups:
                if ref is mp or (len(ref.region) == len(mp.region) and np.array_equal(ref.neighbors, mp.neighbors) and np.array_equal(ref.region, mp.region)):
                    g.append(x)
                    break
            else:
                groups.append((mp, [x]))
        return [g for _, g in groups]


# ---------------------------------------------------------------- client


class Client:
    """The wire's other side in Python (tests, `check`, `stats`)."""

    def __init__(self, path: str):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(path)
        self.maps: set[int] = set()

    def call(self, payload: bytes) -> bytes:
        self.sock.sendall(struct.pack("<I", len(payload)) + payload)
        head = self._recv(4)
        body = self._recv(struct.unpack("<I", head)[0])
        if body[0] != OK:
            raise RuntimeError(body[1:].decode())
        return body[1:]

    def _recv(self, n: int) -> bytes:
        out = bytearray()
        while len(out) < n:
            chunk = self.sock.recv(n - len(out))
            if not chunk:
                raise ConnectionError("the server closed the connection")
            out += chunk
        return bytes(out)

    def hello(self) -> dict:
        r = Wire(self.call(struct.pack("<BI", HELLO, VERSION)))
        keys = ("version", "layout_hash", "heads", "defer", "coarse", "events", "c_tile", "c_global", "c_entity", "c_candidate", "c_event")
        vals = [r.u32(), r.u64(), r.u32(), r.u8(), r.u8(), r.u8(), r.u32(), r.u32(), r.u32(), r.u32(), r.u32()]
        out = dict(zip(keys, vals))
        out["label"] = bytes(r.take(r.u32())).decode()
        return out

    def begin(self, session: int, turn: Turn, c_event: int) -> int:
        nb = np.ascontiguousarray(turn.neighbors, "<i4")
        region = np.ascontiguousarray(turn.region, "<i4")
        key = hash((nb.tobytes(), region.tobytes())) & 0xFFFFFFFFFFFFFFFF
        has = key not in self.maps
        self.maps.add(key)
        e = len(turn.entities)
        ev = turn.events if turn.events is not None else np.zeros((0, c_event), np.int16)
        evm = turn.event_meta if turn.event_meta is not None else np.zeros((0, 4), np.int32)
        parts = [struct.pack("<BQQIB", BEGIN, session, key, len(region), int(has))]
        if has:
            parts += [nb.tobytes(), region.tobytes()]
        parts += [np.ascontiguousarray(turn.tiles, "<i2").tobytes(), np.ascontiguousarray(turn.global_row, "<i2").tobytes(), struct.pack("<I", e),
                  np.ascontiguousarray(turn.entities, "<i2").tobytes(), np.ascontiguousarray(turn.entity_meta[:, :3], "<i4").tobytes(),
                  np.ascontiguousarray(turn.entity_ids if turn.entity_ids is not None else np.full(e, -1), "<i4").tobytes(),
                  struct.pack("<I", len(evm)), np.ascontiguousarray(ev, "<i2").tobytes(), np.ascontiguousarray(evm, "<i4").tobytes()]
        return struct.unpack("<i", self.call(b"".join(parts)))[0]

    def score(self, session: int, note: int, head: int, actor: int, rows: np.ndarray, meta: np.ndarray) -> np.ndarray:
        payload = struct.pack("<BQiIiI", SCORE, session, note, head, actor, len(rows)) + np.ascontiguousarray(rows, "<i2").tobytes() + np.ascontiguousarray(meta, "<i4").tobytes()
        return np.frombuffer(self.call(payload), "<i4").astype(np.int32)

    def stats(self, reset: bool = False) -> dict:
        return json.loads(self.call(struct.pack("<BB", STATS, int(reset))))


# ---------------------------------------------------------------- faithfulness


def offline_choices(model, windows, cfg, device) -> list[list[np.ndarray]]:
    """Per window, per turn, per decision: the offline logits (float) as
    v2_bench eval computes them (train_imitation.run_windows, one window a
    batch: its turns' decisions in shard order)."""
    from .train_imitation import HeadStats, Prepared, run_windows

    out = []
    for w in windows:
        got = []

        class Catch(HeadStats):
            def add(self, batch, pad, ce):
                n = batch["np"]["n_dec"]
                got.extend(pad[:n].float().cpu().numpy())

        run_windows(model, Prepared([w], cfg, 8), cfg, device, Catch(model.heads))
        logits = got
        per_turn, at = [], 0
        for t in w:
            n = len(t.decisions)
            per_turn.append(logits[at : at + n])
            at += n
        out.append(per_turn)
    return out


def check(a):
    """The server's argmax on recorded windows against the offline
    evaluation's, the trace teacher-forced by the recorded choices; the
    server runs in-process here (the same `Player`) unless --socket."""
    from .data import WindowIndex, load_shards

    shards = load_shards(a.shards)[: a.max_shards]
    player = Player(a.checkpoint, a.device)
    cfg = player.cfg
    idx = WindowIndex(shards, a.window)
    windows = [[shards[si].turn(int(t)) for t in rows] for items in idx.groups.values() for si, rows in items][: a.max_windows]
    t0 = time.time()
    model = player.model
    with torch.inference_mode():
        ref = offline_choices(model, windows, cfg, player.device)
    t_off = time.time() - t0
    client = Client(a.socket) if a.socket else None
    if client:
        hello = client.hello()
        print(f"server: {hello['label']}", flush=True)
    stats = {"decisions": 0, "agree": 0, "near_ties": 0, "max_abs_logit_diff": 0.0, "by_head": {}}
    t0 = time.time()
    with torch.inference_mode():
        for wi, (w, ref_w) in enumerate(zip(windows, ref)):
            sid = wi + 1
            session = Session(cfg, player.device)
            mp = None
            for t, ref_t in zip(w, ref_w):
                if client:
                    client.begin(sid, t, cfg.c_event)
                else:
                    if mp is None:
                        mp = MapEntry(np.asarray(t.neighbors, np.int32), np.asarray(t.region, np.int32), t.n_regions, torch.as_tensor(np.asarray(t.neighbors, np.int64), device=player.device))
                    player.begin([(session, t, mp)])
                prev = -1
                for d, lg in zip(t.decisions, ref_t):
                    rows = np.asarray(d.rows, np.int16)
                    meta = np.asarray(d.meta, np.int32)
                    if client:
                        got = client.score(sid, prev, d.head, d.actor, rows, meta)
                    else:
                        player.note(session, prev)
                        got = player.score([(session, d.head, d.actor, rows, meta)])[0]
                    prev = d.choice
                    k = len(rows)
                    ref_l = lg[:k]
                    a_ref, a_got = int(np.argmax(ref_l)), int(np.argmax(got))
                    h = stats["by_head"].setdefault(str(d.head), [0, 0])
                    h[0] += a_ref == a_got
                    h[1] += 1
                    stats["decisions"] += 1
                    stats["agree"] += a_ref == a_got
                    srt = np.sort(ref_l)[::-1]
                    if k > 1 and srt[0] - srt[1] < 2.0 / Q8:
                        stats["near_ties"] += 1
                    stats["max_abs_logit_diff"] = max(stats["max_abs_logit_diff"], float(np.abs(got / Q8 - ref_l).max()))
    stats["agreement"] = stats["agree"] / max(stats["decisions"], 1)
    stats["windows"] = len(windows)
    stats["seconds_offline"] = t_off
    stats["seconds_server"] = time.time() - t0
    print(json.dumps(stats, indent=1))
    if a.out:
        Path(a.out).write_text(json.dumps(stats, indent=1))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("serve")
    p.add_argument("checkpoint")
    p.add_argument("--socket", required=True)
    p.add_argument("--device", default="mps")
    p.add_argument("--gather-us", type=int, default=300, help="how long the first waiting request waits for others (µs)")
    p.add_argument("--threads", type=int, default=2, help="torch CPU threads")
    p.add_argument("--int-ref", action="store_true", help="also run the NumPy integer reference on every request (a check; slow)")
    p = sub.add_parser("check")
    p.add_argument("checkpoint")
    p.add_argument("--shards", required=True)
    p.add_argument("--socket", help="ask a running server instead of the in-process player")
    p.add_argument("--device", default="mps")
    p.add_argument("--window", type=int, default=32)
    p.add_argument("--max-shards", type=int, default=20)
    p.add_argument("--max-windows", type=int, default=40)
    p.add_argument("--out")
    p = sub.add_parser("stats")
    p.add_argument("--socket", required=True)
    p.add_argument("--reset", action="store_true")
    a = ap.parse_args(argv)
    if a.cmd == "serve":
        torch.set_num_threads(a.threads)
        player = Player(a.checkpoint, a.device, a.int_ref)
        server = Server(player, a.socket, a.gather_us)

        def stop(*_):
            print(json.dumps(server.counters.report(player.device) | {"peak_mps_driver_mb": server.peak_mb}), flush=True)
            try:
                os.unlink(a.socket)
            except OSError:
                pass
            sys.exit(0)

        signal.signal(signal.SIGTERM, stop)
        signal.signal(signal.SIGINT, stop)
        server.serve()
    elif a.cmd == "check":
        check(a)
    elif a.cmd == "stats":
        print(json.dumps(Client(a.socket).stats(a.reset), indent=1))


if __name__ == "__main__":
    main()
