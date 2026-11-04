"""The v2 net for the in-process Rust player (docs/ai-net-model.md, «v2
in-process (GPU)»): a float weights file the Rust side loads
(`crates/classic-rules/src/ai/net_e2e/v2/`), and recorded positions with
the PyTorch logits to check it against.

    python -m aurelia_nn.export_v2 weights CKPT --out W.v2
        every tensor of the checkpoint (f32), the input buffers, the config
        as named integers, the layout hash and a label
    python -m aurelia_nn.export_v2 fixture CKPT --shards DIR --out F.v2x
        recorded windows as the remote player's wire requests (BEGIN, then
        each decision's SCORE with the previous choice as its note), with
        the offline evaluation's logits (`remote.offline_choices`, as
        `v2_bench eval`) and the remote server's own (`remote.Player`) on
        the same device

Weights file (little-endian): b"AV2W", u32 version (1), u32 count, then per
tensor: u16 name length, the name (UTF-8), u8 dtype (0 f32, 1 i64, 2 u8),
u8 rank, u32 × rank dims, u64 byte offset of the data from the file's
start (64-aligned); the data follow the directory.

Fixture (little-endian): b"AV2X", u32 version (1), u32 windows; per window
u32 turns; per turn u32 length + the BEGIN payload (kind byte first), i32
the server's value (Q8), u32 decisions; per decision u32 length + the SCORE
payload, u32 k, f32 × k the offline logits, i32 × k the server's (Q8), i32
the recorded choice.
"""

from __future__ import annotations

import argparse
import dataclasses
import struct
import time
from pathlib import Path

import numpy as np
import torch

from .data import Layout
from .model import load_checkpoint
from .model_v2 import COMBAT_ENTITY

MAGIC_WEIGHTS = b"AV2W"
MAGIC_FIXTURE = b"AV2X"
VERSION = 1
# Config fields written as `cfg.NAME` (i64 scalars or vectors).
CONFIG_INTS = ("c_tile", "c_global", "c_entity", "c_candidate", "c_event", "d_tile", "d_model", "n_attn_heads", "n_conv", "n_attn", "d_ff",
               "d_head", "n_kinds", "n_relations", "n_decision_heads", "tag_buckets", "tag_dim", "param_buckets", "param_dim", "mem_player",
               "mem_nation", "mem_unit", "id_buckets", "id_dim", "trace", "trace_heads", "trace_shift", "events", "defer", "arch", "map_ratio",
               "combat", "drop_move_strike", "engram", "aim", "coarse")
CONFIG_VECTORS = ("map_dilations", "id_tile", "id_global", "id_entity", "id_candidate", "id_event")
# The entity columns the sum heads read (model_v2.NetV2.sum_fields), in this order.
SUM_COLUMNS = ("strength", "ranged_strength", "civilian", "kind_own_unit", "moves_left", "can_attack", "kind_foreign_unit", "relation_war",
               "relation_barbarian", "movement", "range")


def tensors_of(ckpt: str) -> tuple[dict, str]:
    """name → array of everything the Rust forward reads."""
    ck = torch.load(ckpt, map_location="cpu", weights_only=False)
    model = load_checkpoint(ckpt)
    cfg = model.cfg
    if cfg.arch != 2:
        raise ValueError(f"{ckpt}: not a v2 checkpoint (arch {cfg.arch})")
    if cfg.engram or cfg.aim or cfg.coarse or not cfg.combat or model.combat is None:
        raise ValueError("the Rust v2 builds neither engram, aim nor coarse, and needs the combat module")
    layout = Layout.from_json(ck["layout"])
    out: dict[str, np.ndarray] = {}
    for name, t in model.state_dict().items():
        if name.startswith("cand_aux_head."):
            continue  # training only
        out[name] = t.detach().float().numpy()
    for name, t in model.named_buffers():
        if name.startswith(("num_", "idc_")) or name in ("cand_keep",) or name.startswith("combat."):
            out[name] = t.detach().numpy().astype(np.float32 if t.is_floating_point() else np.int64)
    for f in CONFIG_INTS:
        out[f"cfg.{f}"] = np.asarray(int(getattr(cfg, f)), np.int64)
    for f in CONFIG_VECTORS:
        out[f"cfg.{f}"] = np.asarray(tuple(getattr(cfg, f)), np.int64).reshape(-1)
    out["cfg.entity_in"] = np.asarray(cfg.entity_in, np.int64)
    out["cfg.candidate_in"] = np.asarray(cfg.candidate_in, np.int64)
    out["cfg.event_in"] = np.asarray(cfg.event_in, np.int64)
    for table in ("tile", "global", "entity", "candidate", "event"):
        out[f"cfg.row_in_{table}"] = np.asarray(cfg.row_in(table), np.int64)
    names = layout.names["entity"]
    out["sum.columns"] = np.asarray([names.index(n) for n in SUM_COLUMNS], np.int64)
    out["combat.entity_columns_named"] = np.asarray([names.index(n) for n in COMBAT_ENTITY], np.int64)
    h = layout.hash()
    out["layout_hash"] = np.frombuffer(struct.pack("<Q", h), np.int64).copy()
    label = f"{Path(ckpt).parent.name}/{Path(ckpt).name}"
    out["label"] = np.frombuffer(label.encode(), np.uint8).copy()
    return out, label


def write_weights(path: str, tensors: dict[str, np.ndarray]):
    dtypes = {np.dtype(np.float32): 0, np.dtype(np.int64): 1, np.dtype(np.uint8): 2}
    items = [(k, np.ascontiguousarray(v)) for k, v in tensors.items()]
    head = bytearray(MAGIC_WEIGHTS + struct.pack("<II", VERSION, len(items)))
    dir_size = sum(2 + len(k.encode()) + 2 + 4 * v.ndim + 8 for k, v in items)
    at = (len(head) + dir_size + 63) // 64 * 64
    blobs = []
    for k, v in items:
        name = k.encode()
        head += struct.pack("<H", len(name)) + name + struct.pack("<BB", dtypes[v.dtype], v.ndim) + b"".join(struct.pack("<I", d) for d in v.shape)
        head += struct.pack("<Q", at)
        data = v.astype(v.dtype.newbyteorder("<"), copy=False).tobytes()
        blobs.append((at, data))
        at = (at + len(data) + 63) // 64 * 64
    with open(path, "wb") as f:
        f.write(head)
        for off, data in blobs:
            f.write(b"\0" * (off - f.tell()))
            f.write(data)


def weights(a):
    t0 = time.time()
    tensors, label = tensors_of(a.checkpoint)
    write_weights(a.out, tensors)
    n = sum(v.size for k, v in tensors.items() if v.dtype == np.float32)
    print(f"{a.out}: {label}, {len(tensors)} tensors, {n} floats, {Path(a.out).stat().st_size / 2**20:.1f} MB, {time.time() - t0:.1f} s")


# ---------------------------------------------------------------- fixture


def begin_payload(session: int, turn, c_event: int) -> bytes:
    from .remote import BEGIN

    nb = np.ascontiguousarray(turn.neighbors, "<i4")
    region = np.ascontiguousarray(turn.region, "<i4")
    e = len(turn.entities)
    ev = turn.events if turn.events is not None else np.zeros((0, c_event), np.int16)
    evm = turn.event_meta if turn.event_meta is not None else np.zeros((0, 4), np.int32)
    parts = [struct.pack("<BQQIB", BEGIN, session, 0, len(region), 1), nb.tobytes(), region.tobytes(),
             np.ascontiguousarray(turn.tiles, "<i2").tobytes(), np.ascontiguousarray(turn.global_row, "<i2").tobytes(), struct.pack("<I", e),
             np.ascontiguousarray(turn.entities, "<i2").tobytes(), np.ascontiguousarray(turn.entity_meta[:, :3], "<i4").tobytes(),
             np.ascontiguousarray(turn.entity_ids if turn.entity_ids is not None else np.full(e, -1), "<i4").tobytes(),
             struct.pack("<I", len(evm)), np.ascontiguousarray(ev, "<i2").tobytes(), np.ascontiguousarray(evm, "<i4").tobytes()]
    return b"".join(parts)


def score_payload(session: int, note: int, head: int, actor: int, rows: np.ndarray, meta: np.ndarray) -> bytes:
    from .remote import SCORE

    return struct.pack("<BQiIiI", SCORE, session, note, head, actor, len(rows)) + np.ascontiguousarray(rows, "<i2").tobytes() + np.ascontiguousarray(meta, "<i4").tobytes()


def fixture(a):
    from .data import WindowIndex, load_shards
    from .remote import MapEntry, Player, Session, offline_choices

    shards = load_shards(a.shards)[: a.max_shards]
    player = Player(a.checkpoint, a.device)
    cfg = player.cfg
    idx = WindowIndex(shards, a.window)
    windows = [[shards[si].turn(int(t)) for t in rows] for items in idx.groups.values() for si, rows in items][: a.max_windows]
    t0 = time.time()
    with torch.inference_mode():
        ref = offline_choices(player.model, windows, cfg, player.device)
    print(f"offline: {len(windows)} windows, {time.time() - t0:.1f} s", flush=True)
    out = bytearray(MAGIC_FIXTURE + struct.pack("<II", VERSION, len(windows)))
    n_dec = 0
    with torch.inference_mode():
        for wi, (w, ref_w) in enumerate(zip(windows, ref)):
            sid = wi + 1
            session = Session(cfg, player.device)
            mp = MapEntry(np.asarray(w[0].neighbors, np.int32), np.asarray(w[0].region, np.int32), w[0].n_regions,
                          torch.as_tensor(np.asarray(w[0].neighbors, np.int64), device=player.device))
            out += struct.pack("<I", len(w))
            for t, ref_t in zip(w, ref_w):
                b = begin_payload(sid, t, cfg.c_event)
                value = player.begin([(session, t, mp)])[0]
                out += struct.pack("<I", len(b)) + b + struct.pack("<iI", value, len(t.decisions))
                prev = -1
                for d, lg in zip(t.decisions, ref_t):
                    rows = np.asarray(d.rows, np.int16)
                    meta = np.asarray(d.meta, np.int32)
                    s = score_payload(sid, prev, d.head, d.actor, rows, meta)
                    player.note(session, prev)
                    got = player.score([(session, d.head, d.actor, rows, meta)])[0]
                    k = len(rows)
                    out += struct.pack("<I", len(s)) + s + struct.pack("<I", k) + np.asarray(lg[:k], "<f4").tobytes() + np.asarray(got, "<i4").tobytes()
                    out += struct.pack("<i", int(d.choice))
                    prev = int(d.choice)
                    n_dec += 1
    Path(a.out).write_bytes(bytes(out))
    print(f"{a.out}: {len(windows)} windows, {sum(len(w) for w in windows)} turns, {n_dec} decisions, {len(out) / 2**20:.1f} MB, {time.time() - t0:.1f} s")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("weights")
    p.add_argument("checkpoint")
    p.add_argument("--out", required=True)
    p = sub.add_parser("fixture")
    p.add_argument("checkpoint")
    p.add_argument("--shards", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--device", default="cpu")
    p.add_argument("--window", type=int, default=32)
    p.add_argument("--max-shards", type=int, default=20)
    p.add_argument("--max-windows", type=int, default=40)
    p.add_argument("--threads", type=int, default=4)
    a = ap.parse_args(argv)
    torch.set_num_threads(getattr(a, "threads", 4))
    if a.cmd == "weights":
        weights(a)
    else:
        fixture(a)


if __name__ == "__main__":
    main()
