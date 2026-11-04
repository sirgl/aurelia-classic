"""Imitation training of the end-to-end net (stage E1 of
classic/docs/ai-neural-architecture.md): cross-entropy over each decision's
legal candidates, the expert's choice as the target (a wait weighs
`--wait-weight`); the value head learns the player's final points share
(small weight). The net has memory, so it trains on windows of consecutive
turns of one player, carrying the memory along and back-propagating through
`--bptt` turns at a time (truncated BPTT; the memory is carried on, detached,
to the next piece of the window). Held-out games, never turns, measure top-1
agreement per head.

Data: existing shard directories (`--data <root>`), replay logs
(`--logs <dir|list.txt>`) materialised on demand by the feature binary
(`--features-bin`, see materialize.py), or both: a mixture of the expert's
logs and DAgger shards, `--mix P` of the window batches from `--data`, each
source held out and reported on its own (expert-val, dagger-val). Three
sources: `--logs`, `--logs2` and `--data` with `--data-mix Q` (Q of the
batches from `--data`, of the rest `--mix` from `--logs2`; e.g. a training
ground's scenes beside the whole games, ai-net-data.md «Imitation on the
grounds»).

    python -m aurelia_nn.train_imitation --data /path/to/shards --out runs/e1
"""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import queue
import threading
import time
from dataclasses import fields
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F

from .common import ID_SUFFIX, PRESETS, Config
from .data import Deferred, Layout, Uploader, WindowIndex, collate, load_shards, split_by_game, upload_batch
from .materialize import LogMaterializer, default_binary, stream_shards
from .model import Net, WindowMemory, padded_logits, parameter_count, save_checkpoint, zero_defer_tag


def autocast_methods(model, device, dtype):
    """Runs the net's embed, attend and score under autocast (`--autocast`):
    matrix products in `dtype`, their outputs handed back as float32 (the
    memory's GRUs, the losses and the norms stay float32)."""
    def to_f32(x):
        if torch.is_tensor(x) and x.is_floating_point():
            return x.float()
        if isinstance(x, dict):
            return {k: to_f32(v) for k, v in x.items()}
        return x

    for name in ("embed", "attend", "score"):
        fn = getattr(model, name)

        def wrapped(*args, fn=fn, **kw):
            with torch.autocast(device.type, dtype=dtype):
                return to_f32(fn(*args, **kw))

        setattr(model, name, wrapped)


def pick_device(name: str) -> torch.device:
    if name == "auto":
        if torch.backends.mps.is_available():
            return torch.device("mps")
        if torch.cuda.is_available():
            return torch.device("cuda")
        return torch.device("cpu")
    return torch.device(name)


class HeadStats:
    """Top-1 agreement per decision head (named by meta.json `heads`), then
    unit decisions split by whether the expert waited."""

    def __init__(self, heads=("unit", "research", "production", "policy")):
        self.heads = tuple(heads)
        self.groups = self.heads + ("unit_no_wait", "unit_wait")
        self.right = np.zeros(len(self.groups), np.int64)
        self.total = np.zeros(len(self.groups), np.int64)
        self.loss = 0.0
        self.n = 0
        self._pending = []  # (right on the device, ce sum on the device, heads, wait)

    def add(self, batch: dict, pad: torch.Tensor, ce: torch.Tensor):
        """Counts a batch's decisions; the device values are read back at
        `flush` (or `report`), so adding does not wait for the device."""
        host = batch.get("np") or {}
        heads = host.get("dec_head")
        heads = batch["dec_head"].cpu().numpy() if heads is None else heads
        wait = host.get("dec_wait")
        wait = batch["dec_wait"].cpu().numpy() if wait is None else wait
        self._pending.append((pad.argmax(dim=1) == batch["dec_choice"], ce.sum(), heads, wait))

    def flush(self):
        if not self._pending:
            return
        right = torch.cat([p[0] for p in self._pending]).cpu().numpy()
        ce = torch.stack([p[1] for p in self._pending]).cpu().numpy().astype(np.float64)
        # A padded batch's rows past its decisions (collate pad) are left out.
        ends = np.cumsum([len(p[0]) for p in self._pending])
        right = np.concatenate([right[e - len(p[0]) : e - len(p[0]) + len(p[2])] for e, p in zip(ends, self._pending)])
        heads = np.concatenate([p[2] for p in self._pending])
        wait = np.concatenate([p[3] for p in self._pending])
        self._pending = []
        sels = [heads == h for h in range(len(self.heads))] + [(heads == 0) & ~wait, (heads == 0) & wait]
        for g, sel in enumerate(sels):
            self.right[g] += int(right[sel].sum())
            self.total[g] += int(sel.sum())
        self.loss += float(ce.sum())
        self.n += len(right)

    def report(self) -> dict:
        self.flush()
        out = {g: (float(self.right[i] / self.total[i]) if self.total[i] else None) for i, g in enumerate(self.groups)}
        out["decisions"] = {g: int(self.total[i]) for i, g in enumerate(self.groups)}
        out["all"] = float(self.right[: len(self.heads)].sum() / max(self.n, 1))
        out["ce"] = self.loss / max(self.n, 1)
        return out


def aux_loss(model: Net, pred, batch: dict):
    """Squared error (binary targets: cross-entropy) of the auxiliary heads
    over the targets present (training only)."""
    target = batch.get("aux")
    if pred is None or target is None:
        return None
    ok = ~torch.isnan(target)
    if not ok.any():
        return None
    binary = torch.as_tensor(model.aux_binary, device=pred.device)[None, :].expand_as(pred)
    t = torch.nan_to_num(target)
    mse = (pred - t) ** 2
    bce = F.binary_cross_entropy_with_logits(pred, t.clamp(0, 1), reduction="none")
    return torch.where(binary, bce, mse)[ok].sum()


def parse_decision_weights(specs: list[str], layout) -> list[tuple[int | None, int | None, float]]:
    """`--decision-weight KEY=W` rules as (head, tag, weight); a decision's
    loss weight is the product of the rules it matches. KEY is a head name
    (every decision of the head), a tag name of meta.json `tags` (decisions
    whose expert choice has that tag), `HEAD:TAG` (TAG a number or a tag
    name: that head's decisions with that choice), `HEAD_TAGNAME` when
    TAGNAME is a tag name (`religion_save`), or `column:NAME` (decisions whose
    expert choice has a nonzero candidate column NAME: `blow_captures`,
    format 8; that rule is (None, None, weight, column index))."""
    heads = {h: i for i, h in enumerate(layout.heads)}
    tags = dict(layout.tags)
    rules = []
    for spec in specs:
        key, _, w = spec.partition("=")
        if not _:
            raise SystemExit(f"--decision-weight {spec!r}: expected KEY=W")
        weight = float(w)
        if key.startswith("column:"):
            names = list(layout.names["candidate"])
            if key[7:] not in names:
                raise SystemExit(f"--decision-weight {spec!r}: no candidate column {key[7:]!r}")
            rules.append((None, None, weight, names.index(key[7:])))
            continue

        def tag_of(name: str) -> int:
            if name.lstrip("-").isdigit():
                return int(name)
            if name in tags:
                return tags[name]
            raise SystemExit(f"--decision-weight {spec!r}: no tag {name!r} (meta tags: {sorted(tags)})")

        if ":" in key:
            h, t = key.split(":", 1)
            if h not in heads:
                raise SystemExit(f"--decision-weight {spec!r}: no head {h!r} (heads: {list(heads)})")
            rules.append((heads[h], tag_of(t), weight))
        elif key in heads:
            rules.append((heads[key], None, weight))
        elif key in tags:
            rules.append((None, tags[key], weight))
        else:
            for h in heads:
                if key.startswith(h + "_") and key[len(h) + 1 :] in tags:
                    rules.append((heads[h], tags[key[len(h) + 1 :]], weight))
                    break
            else:
                raise SystemExit(f"--decision-weight {spec!r}: {key!r} is no head, tag or HEAD:TAG (heads {list(heads)}, tags {sorted(tags)})")
    return rules


def decision_weights(batch: dict, wait_weight: float, rules) -> torch.Tensor:
    """The loss weight of each decision of a batch (on the batch's device)."""
    return torch.from_numpy(host_decision_weights(batch, wait_weight, rules)).to(batch["dec_head"].device)


def host_decision_weights(batch: dict, wait_weight: float, rules) -> np.ndarray:
    """`decision_weights` as a float32 NumPy array, from the batch's host
    copies when it has them."""
    host = batch.get("np") or {}
    get = lambda k: host[k] if k in host else batch[k].cpu().numpy()
    wait, heads, tags = get("dec_wait"), get("dec_head"), get("dec_tag")
    w = np.where(wait, np.float32(wait_weight), np.float32(1.0)).astype(np.float32)
    for head, tag, weight, *column in rules or ():
        hit = np.ones(len(w), bool)
        if head is not None:
            hit &= heads == head
        if tag is not None:
            hit &= tags == tag
        if column:
            chosen = np.asarray(host["dec_row"])[:, column[0]] != 0
            hit &= np.concatenate([chosen, np.zeros(len(w) - len(chosen), bool)])
        w = np.where(hit, w * np.float32(weight), w).astype(np.float32)
    return w


class Prepared:
    """A batch of windows made ready on the host, for run_windows: the
    memory plan and, per piece of `bptt` turns, the collated batch (padded,
    collate `pad`) and the memory's index tensors, none of it on the device
    yet (built on prefetch's worker thread while the device runs the
    previous batch)."""

    def __init__(self, windows, cfg, bptt: int):
        self.bptt = bptt
        self.memory = WindowMemory(windows, cfg, pad=True)
        self.pieces = []
        steps = max(len(w) for w in windows)
        host = Deferred()
        for k0 in range(0, steps, bptt):
            plan, turns = [], []
            for k in range(k0, min(k0 + bptt, steps)):
                active = [i for i, w in enumerate(windows) if k < len(w)]
                plan.append((k, active))
                turns.extend(windows[i][k] for i in active)
            batch = collate(turns, cfg, upload=host, pad=True)
            self.pieces.append((batch, self.memory.segment(plan, batch["ent_id"].shape[1], host), len(turns)))


def run_windows(model: Net, windows, cfg, device, stats: HeadStats, value_weight=0.0, wait_weight=1.0, opt=None, bptt=8, aux_weight=0.0, rules=None):
    """Runs a batch of windows (lists of `data.Turn` of one map, or a
    `Prepared` of them) turn by turn with memory; with `opt`, trains with
    truncated BPTT. Returns the number of player-turns run.

    The loss, the gradients and the memory are those of stepping the
    windows' turn k together for k = 0, 1, … (memory_batch, Net.forward,
    store_memory; tests/test_pipeline.py keeps that loop as the reference),
    computed in pieces of `bptt` turns: the memory-free part of the net
    (tiles, regions, entity tokens) and the heads run once over all turns of
    a piece, only the attention and the memory update step turn by turn, and
    the memory lives in device tables (WindowMemory). Nothing waits for the
    device before the end of the batch: copies to it are queued."""
    prepared = windows if isinstance(windows, Prepared) else Prepared(windows, cfg, bptt)
    memory = prepared.memory
    memory.start(device)
    upload = Uploader(device, blocking=False)
    turns_run = 0
    for host_batch, host_seg, n_turns in prepared.pieces:
        batch = upload_batch(host_batch, upload)
        emb = model.embed(batch)
        off = emb["off"]
        tokens = []
        for turn in memory.views(upload_batch(host_seg, upload)):
            mem = memory.read(turn)
            tok = model.attend(Net.rows(emb, turn["start"], turn["stop"]), *mem)
            memory.write(model, turn, tok, off, mem)
            tokens.append(tok)
        tokens = torch.cat(tokens)
        logits = model.score(batch, emb["tile_h"], tokens, off)
        turns_run += n_turns
        if cfg.cand_aux and "cand_aux" in batch and getattr(model, "cand_aux_sink", None) is not None:
            model.cand_aux_sink.add(model.cand_aux_out, batch["cand_aux"])
        loss_sum, weight_sum = 0.0, 0.0
        host = batch["np"]
        if host["n_dec"]:
            pad = padded_logits(batch, logits)
            ce = F.cross_entropy(pad, batch["dec_choice"], reduction="none")  # 0 on padding rows
            stats.add(batch, pad.detach(), ce.detach())
            if opt is not None:
                w = host_decision_weights(batch, wait_weight, rules)
                loss_sum = (ce * upload(np.pad(w, (0, len(ce) - len(w))), ce.dtype)).sum()
                weight_sum = float(w.sum(dtype=np.float64))
        if opt is not None:
            ok = ~np.isnan(host["value_target"])
            if value_weight > 0 and ok.any():
                # Σ over turns with a target of (value − target)², as a masked sum (one shape).
                err = model.value(batch, tokens, off) - batch["value_target"].nan_to_num()
                loss_sum = loss_sum + value_weight * (err * err * upload(ok.astype(np.float32))).sum()
            al = aux_loss(model, model.aux(tokens, off), batch) if aux_weight > 0 else None
            if al is not None:
                loss_sum = loss_sum + aux_weight * al
            if cfg.cand_aux and "cand_aux" in batch and getattr(model, "cand_aux_weight", 0.0) > 0:
                from .cand_aux import loss as cand_aux_loss

                loss_sum = loss_sum + model.cand_aux_weight * cand_aux_loss(model.cand_aux_out, batch["cand_aux"], batch["cand_dec"], batch["dec_k"])
            if getattr(model, "combat_loss", None) is not None and getattr(model, "combat_weight", 0.0) > 0:
                # v2's combat module keeps its preview loss (model_v2.NetV2.preview_loss).
                loss_sum = loss_sum + model.combat_weight * model.combat_loss
            if torch.is_tensor(loss_sum):
                loss = loss_sum / max(weight_sum, 1.0)
                opt.zero_grad(set_to_none=True)
                loss.backward()
                torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
                opt.step()
            memory.detach()
    stats.flush()
    upload.sync()
    return turns_run


def prefetch(items, fn, depth: int = 1):
    """`fn(item)` for each of `items`, computed up to `depth` ahead on a
    worker thread (the host side of the next batch overlaps the device work
    of this one). Exceptions surface here; leaving the loop early stops the
    worker."""
    q: queue.Queue = queue.Queue(maxsize=depth)
    stop = threading.Event()
    done = object()

    def put(x) -> bool:
        while not stop.is_set():
            try:
                q.put(x, timeout=0.1)
                return True
            except queue.Full:
                pass
        return False

    def work():
        try:
            for item in items:
                if not put((fn(item), None)):
                    return
            put((done, None))
        except BaseException as e:  # noqa: BLE001 - handed to the consumer
            put((None, e))

    thread = threading.Thread(target=work, daemon=True)
    thread.start()
    try:
        while True:
            value, error = q.get()
            if error is not None:
                raise error
            if value is done:
                return
            yield value
    finally:
        stop.set()
        thread.join()


def window_turns(batch):
    """[(shard, rows)] → lists of `data.Turn`."""
    return [[shard.turn(int(t)) for t in rows] for shard, rows in batch]


@torch.no_grad()
def evaluate(model: Net, batches, cfg, device, bptt: int = 8) -> dict:
    model.eval()
    stats = HeadStats(model.heads)
    for prepared in prefetch(batches, lambda b: Prepared(window_turns(b), cfg, bptt)):
        run_windows(model, prepared, cfg, device, stats)
    model.train()
    return stats.report()


# The layers whose input begins with a table's numeric columns (`Net.norm`):
# the numeric columns in layout order, then the id embeddings, the engram
# vector and the layer's own extras.
INPUT_LAYERS = {"tile_embed.weight": "tile", "glob.weight": "global", "entity.weight": "entity", "cand.weight": "candidate", "event.weight": "event"}


def input_column_source(old: Layout, new: Layout, table: str, old_in: int, new_in: int):
    """Per input of the new layer, the input of the old layer it takes (−1: a
    column the old layout lacks, whose weights start at zero): the numeric
    columns by name, the rest of the input in place. None when the two do
    not map (other id columns, another tail)."""
    numeric = lambda lay: [n for n in lay.names[table] if not n.endswith(ID_SUFFIX)]
    ids = lambda lay: [n for n in lay.names[table] if n.endswith(ID_SUFFIX)]
    on, nn_ = numeric(old), numeric(new)
    if ids(old) != ids(new) or old_in - len(on) != new_in - len(nn_):
        return None
    at = {n: i for i, n in enumerate(on)}
    return [at.get(n, -1) for n in nn_] + list(range(len(on), old_in))


def init_from(model: Net, path: str) -> dict:
    """Loads every tensor of the checkpoint whose name and shape match;
    a tensor that only grew along its first dimension (the per-head
    embeddings when a head is added) gets the old rows as its prefix. When
    the feature layout changed (columns added, removed or moved: a new data
    format), the input layers of the tables take the old weights of each
    column by name and zero weights for a new column, so the net computes
    what the old one did on the old columns (`columns` in the report); the
    input scales are the new layout's. The rest stays freshly initialised.
    Returns and prints what was new."""
    ck = torch.load(path, map_location="cpu", weights_only=False)
    old = ck["state"] if "state" in ck else ck
    old_layout = Layout.from_json(ck["layout"]) if isinstance(ck, dict) and "layout" in ck else None
    new_layout = Layout.from_json(model.layout_json)
    state = model.state_dict()
    loaded, extended, new, mismatched, columns = [], [], [], [], []
    for name, t in state.items():
        o = old.get(name)
        table = INPUT_LAYERS.get(name)
        if name in ("in_tile", "in_global", "in_entity", "in_candidate", "in_event"):
            # The input scales are the layout's own.
            loaded.append(name)
        elif o is None:
            new.append(name)
        elif table and old_layout is not None and old_layout.names.get(table) != new_layout.names.get(table):
            src = input_column_source(old_layout, new_layout, table, o.shape[1], t.shape[1]) if o.ndim == 2 and o.shape[0] == t.shape[0] else None
            if src is None:
                mismatched.append(f"{name} {tuple(o.shape)}≠{tuple(t.shape)}")
                continue
            mapped = torch.zeros_like(t)
            keep = [j for j, s in enumerate(src) if s >= 0]
            mapped[:, keep] = o[:, [src[j] for j in keep]].to(t.dtype)
            state[name] = mapped
            added = [n for n in new_layout.names[table] if n not in old_layout.names[table]]
            dropped = [n for n in old_layout.names[table] if n not in new_layout.names[table]]
            columns.append(f"{table}: {len(added)} new (zero), {len(dropped)} dropped")
            loaded.append(name)
        elif o.shape == t.shape:
            state[name] = o.to(t.dtype)
            loaded.append(name)
        elif o.ndim == t.ndim and o.ndim >= 1 and o.shape[1:] == t.shape[1:] and o.shape[0] < t.shape[0]:
            grown = t.clone()
            grown[: o.shape[0]] = o.to(t.dtype)
            state[name] = grown
            extended.append(f"{name} {tuple(o.shape)}→{tuple(t.shape)}")
        else:
            mismatched.append(f"{name} {tuple(o.shape)}≠{tuple(t.shape)}")
    model.load_state_dict(state)
    unused = sorted(set(old) - set(state))
    report = {"from": str(path), "loaded": len(loaded), "extended": extended, "new": new, "shape_changed": mismatched, "columns": columns, "unused": unused}
    print(f"init from {path}: {len(loaded)} tensors loaded; columns {columns}; extended {extended}; new {new}; shape changed (fresh) {mismatched}; unused {unused}", flush=True)
    return report


# Config fields a run never takes from its --init: the layout's (the table
# widths and id columns, `Layout.config`) and the training-only ones its own
# data and flags set.
_NOT_CARRIED = ("n_decision_heads", "engram_entity_small", "id_tile", "id_global", "id_entity", "id_candidate", "id_event",
                "aux", "privileged_value", "n_aux", "n_truth")


def carried_config(path: str, preset: str, flags: dict) -> dict:
    """The config of the checkpoint `path` that a run started from it keeps:
    the fields set neither by the layout, the preset nor a flag of the run —
    the defer choice, the trace's heads and shift, the aims' count. A flag of
    the run that differs from the checkpoint's is printed (the run's wins);
    keys this code does not know (a newer tree's training-only heads) are
    dropped."""
    ck = torch.load(path, map_location="cpu", weights_only=False)
    conf = ck.get("config", {}) if isinstance(ck, dict) else {}
    known = {f.name for f in fields(Config)}
    out = {}
    for k, v in conf.items():
        if k not in known or k in PRESETS[preset] or k in _NOT_CARRIED or k.startswith(("c_", "intent")):
            continue
        v = tuple(v) if isinstance(v, list) else v
        if k in flags:
            if flags[k] != v:
                print(f"--init {path}: its {k} is {v}, the run's {flags[k]}", flush=True)
            continue
        out[k] = v
    return out


def flag_config(a) -> dict:
    return {"engram": int(a.engram), "engram_buckets": a.engram_buckets, "engram_dim": a.engram_dim, "aim": int(a.aim), "aux": int(a.aux),
            "privileged_value": int(a.privileged_value), "coarse": int(a.coarse), "trace": int(a.turn_trace), "events": int(getattr(a, "events", 0))} | (
            {"drop_move_strike": 1} if getattr(a, "drop_move_strike", False) else {})


class Optimizers:
    """Several optimisers stepped as one."""

    def __init__(self, *opts):
        self.opts = [o for o in opts if o is not None]
        self.param_groups = [g for o in self.opts for g in o.param_groups]

    def zero_grad(self, set_to_none=True):
        for o in self.opts:
            o.zero_grad(set_to_none=set_to_none)

    def step(self):
        for o in self.opts:
            o.step()


def make_optimizer(model: Net, kind: str, lr: float, weight_decay: float):
    """AdamW for everything (`adam`), or Muon for the 2-D weight matrices
    of the hidden layers and AdamW for embeddings, biases, the edge rows and
    the output layers of the heads (`muon`; torch.optim.Muon)."""
    if kind == "adam":
        return torch.optim.AdamW(model.parameters(), lr=lr, weight_decay=weight_decay)
    last = {"value_out", "aux_head"} | {f"head_logit.{h}" for h in range(model.cfg.n_decision_heads)}
    muon, adam = [], []
    for name, p in model.named_parameters():
        module = name.rsplit(".", 1)[0]
        is_matrix = p.ndim == 2 and name.endswith("weight") and "emb" not in name and "engram" not in name and module not in last
        (muon if is_matrix else adam).append(p)
    return Optimizers(
        torch.optim.Muon(muon, lr=lr, weight_decay=weight_decay, adjust_lr_fn="match_rms_adamw"),
        torch.optim.AdamW(adam, lr=lr, weight_decay=weight_decay),
    )


def _selection_score(val: dict) -> float:
    """Checkpoint choice: the mean held-out top-1 of the heads, unit decisions
    counted without waits (waits are most of them and easy)."""
    keys = ["unit_no_wait"] + [k for k in val["decisions"] if k not in ("unit", "unit_no_wait", "unit_wait")]
    vals = [val[k] for k in keys if val.get(k) is not None]
    return float(np.mean(vals)) if vals else val["all"]


def _record_score(rec: dict) -> float:
    """An epoch's checkpoint score: `_selection_score` of the held-out games,
    with two sources the mean over both (so neither is forgotten)."""
    by = rec.get("val_by_source")
    if by:
        return float(np.mean([_selection_score(v) for v in by.values()]))
    return _selection_score(rec["val"])


def _game_hash(name: str) -> float:
    return int(hashlib.sha1(name.encode()).hexdigest()[:8], 16) / 0xFFFFFFFF


def _list_logs(spec: str) -> list[str]:
    """A .txt list of paths, a directory of logs (or of directories of
    logs: the books, one directory each), or one log."""
    p = Path(spec)
    if p.is_file() and p.suffix == ".txt":
        return [line.strip() for line in p.read_text().splitlines() if line.strip()]
    if p.is_dir():
        return (sorted(str(x) for x in p.iterdir() if x.is_file() and x.suffix == ".aurlog")
                or sorted(str(x) for x in p.rglob("*.aurlog"))
                or sorted(str(x) for x in p.iterdir() if x.is_file()))
    return [spec]


def _train_subset(logs: list[str], fraction: float) -> list[str]:
    """The training logs kept by `--train-fraction`: by a hash of the path
    (salted apart from the held-out split), so a smaller share is a subset
    of a larger one."""
    if fraction >= 1.0:
        return logs
    return [l for l in logs if _game_hash("subset:" + l) < fraction]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--data", help="a directory of shard directories (or one shard); with --logs: the DAgger shards of the mixture")
    ap.add_argument("--progress-every", type=int, default=25, help="print the running train loss every this many steps (0: only at the epoch's end)")
    ap.add_argument("--logs", help="replay logs: a directory, or a .txt list of paths (the expert's games)")
    ap.add_argument("--mix", type=float, default=0.5, help="with --logs and --data (or --logs2): the share of window batches drawn from the second source")
    ap.add_argument("--features-bin", default=default_binary(), help="the feature binary for --logs (or $AURELIA_FEATURES_BIN)")
    ap.add_argument("--logs2", help="a second set of replay logs (e.g. games against the books), a source of its own in place of --data: "
                                    "mixed in by --mix, held out and reported on its own")
    ap.add_argument("--logs2-name", default="books", help="the name --logs2 is reported under")
    ap.add_argument("--data-mix", type=float, help="with --logs, --logs2 and --data: the share of window batches drawn from --data "
                                                     "(the rest split by --mix); needed for the three sources")
    ap.add_argument("--data-name", help="the name --data is reported under (default dagger with --logs, else data)")
    ap.add_argument("--data-in-memory", action="store_true", help="read --data into memory instead of mapping it (many small shards)")
    ap.add_argument("--features-bin2", help="the feature binary for --logs2 (default --features-bin)")
    ap.add_argument("--train-fraction", type=float, default=1.0,
                    help="keep this share of each log source's training games (a data-scaling curve); the held-out games stay all")
    ap.add_argument("--turn-sample", type=int, default=1)
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--out", required=True)
    ap.add_argument("--preset", default="small", choices=sorted(PRESETS))
    ap.add_argument("--epochs", type=int, default=10)
    ap.add_argument("--max-steps", type=int, default=0, help="stop after this many window batches")
    ap.add_argument("--windows-per-batch", type=int, default=16)
    ap.add_argument("--window", type=int, default=32, help="cut windows into pieces of at most this many turns")
    ap.add_argument("--bptt", type=int, default=8, help="turns per truncated back-propagation")
    ap.add_argument("--lr", type=float, default=2e-3)
    ap.add_argument("--weight-decay", type=float, default=1e-4)
    ap.add_argument("--value-weight", type=float, default=0.1)
    ap.add_argument("--wait-weight", type=float, default=0.25, help="loss weight of a unit decision whose expert choice is wait")
    ap.add_argument("--val-fraction", type=float, default=0.15)
    ap.add_argument("--device", default="auto")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--max-minutes", type=float, default=0,
                    help="stop training after this many minutes of the run (then the held-out check, checkpoints and export): "
                         "a chunk of a run on a shared GPU, continued with --init <out>/last.pt")
    ap.add_argument("--eval-int", type=int, default=200, help="held-out turns checked with the integer reference at the end (0: skip)")
    ap.add_argument("--optim", default="adam", choices=["adam", "muon"], help="muon: Muon for the 2-D weight matrices, AdamW for the rest")
    ap.add_argument("--init", help="a checkpoint (best.pt) to start from: every tensor whose name and shape match is loaded")
    ap.add_argument("--decision-weight", action="append", default=[], metavar="KEY=W",
                    help="loss weight of decisions: KEY a head (religion), a tag name (wait), HEAD:TAG or HEAD_TAGNAME (religion_save); repeatable")
    ap.add_argument("--schedule", default="step", choices=["step", "wsd"],
                    help="step: --lr after a 50-step warm-up; wsd: warm-up, stable, then a linear decay to 5%% over the last --decay-share of "
                         "--max-minutes of training time (evaluations not counted)")
    ap.add_argument("--decay-share", type=float, default=0.2)
    ap.add_argument("--eval-every-minutes", type=float, default=0, help="also evaluate the held-out sources every this many minutes of training time")
    ap.add_argument("--holdout-rungs", default="", metavar="SPEC",
                    help="RUNGS:SHARE, e.g. 27-34:0.25: that share of the --data siege scenes of those rungs (a scene: rung, seed and map; "
                         "every copy) left out of training, listed in OUT/holdout.txt (v2_bench evaluates them)")
    ap.add_argument("--cand-aux-decay", type=float, default=0.0,
                    help="the candidate aux head as hints: its weight falls linearly from --cand-aux-weight to 0 over this share of --max-minutes (0: constant)")
    ap.add_argument("--combat-weight", type=float, default=1.0, help="v2: the combat module's preview loss weight (0: off)")
    ap.add_argument("--combat-init", help="v2: a pretrained combat module (v2_bench pretrain-combat)")
    ap.add_argument("--autocast", choices=["off", "bf16", "fp16"], default="off", help="mixed precision on the device (norms and softmax stay fp32)")
    flags = ap.add_argument_group("flags (each off in the E1 baseline)")
    flags.add_argument("--drop-move-strike", action="store_true", help="zero the 14 format-10 move-strike candidate columns in the input")
    flags.add_argument("--engram", action="store_true", help="hashed lookup embeddings on tiles, entities, candidates")
    flags.add_argument("--engram-buckets", type=int, default=1 << 14)
    flags.add_argument("--engram-dim", type=int, default=16)
    flags.add_argument("--aim", action="store_true", help="a goal token from the aim weights (zeros in imitation)")
    flags.add_argument("--aux", action="store_true", help="auxiliary future heads (aux.npy)")
    flags.add_argument("--aux-weight", type=float, default=0.1)
    flags.add_argument("--cand-aux-weight", type=float, default=0.0,
                       help="the candidate aux head (cand_aux.py): --data of format 10 with its move-strike columns cut from the input and predicted")
    flags.add_argument("--cand-aux-targets", default="", help="target groups (cand_aux.GROUPS), comma-separated; empty: all")
    flags.add_argument("--privileged-value", action="store_true", help="the value head reads the truth row (truth.npy)")
    flags.add_argument("--coarse", action="store_true", help="a second region level of 16×16-tile blocks")
    flags.add_argument("--turn-trace", action=argparse.BooleanOptionalAction, default=True,
                       help="each decision's context adds the turn trace of the choices made before it in the player-turn (on by default; weights version 3)")
    flags.add_argument("--defer", action=argparse.BooleanOptionalAction, default=None,
                       help="the unit head's defer choice (format 9; weights version 5); default: the --init checkpoint's, off without one")
    flags.add_argument("--events", action=argparse.BooleanOptionalAction, default=None,
                       help="every token attends over the turn's events (format 7; on by default when the data has the event table; weights version 4)")
    a = ap.parse_args(argv)

    torch.manual_seed(a.seed)
    rng = np.random.default_rng(a.seed)
    device = pick_device(a.device)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)

    if not (a.data or a.logs):
        raise SystemExit("give --data, --logs, or both (a mixture, --mix)")
    if a.logs2 and (not a.logs or (a.data and a.data_mix is None)):
        raise SystemExit("--logs2 goes with --logs, and with --data only given --data-mix")
    if a.data_mix is not None and not (a.logs and a.logs2 and a.data):
        raise SystemExit("--data-mix is the share of --data beside --logs and --logs2")
    sources = {}  # name → (train batches factory, held-out WindowIndex)
    layout = None

    def log_source(spec: str, features_bin: str, label: str):
        logs = _list_logs(spec)
        val_logs = [l for l in logs if _game_hash(Path(l).name) < a.val_fraction] or logs[:1]
        held = set(val_logs)
        train_logs = _train_subset([l for l in logs if l not in held] or logs, a.train_fraction)
        mat = LogMaterializer(features_bin, a.turn_sample)
        val_shards = list(stream_shards(val_logs, mat, a.workers))

        def log_batches(window_games=8):
            order = list(train_logs)
            rng.shuffle(order)
            buf = []
            for shard in stream_shards(order, mat, a.workers):
                buf.append(shard)
                if len(buf) >= window_games:
                    yield from WindowIndex(buf, a.window).batches(a.windows_per_batch, rng)
                    buf = []
            if buf:
                yield from WindowIndex(buf, a.window).batches(a.windows_per_batch, rng)

        print(f"{label}: {len(train_logs)} train (fraction {a.train_fraction}), {len(val_logs)} held out", flush=True)
        return (log_batches, WindowIndex(val_shards, a.window)), val_shards[0].layout

    if a.logs:
        if not a.features_bin:
            raise SystemExit("--logs needs --features-bin (or AURELIA_FEATURES_BIN)")
        sources["expert"], layout = log_source(a.logs, a.features_bin, "logs")
    if a.logs2:
        sources[a.logs2_name], layout2 = log_source(a.logs2, a.features_bin2 or a.features_bin, a.logs2_name)
        if layout2.hash() != layout.hash():
            raise SystemExit("--logs and --logs2 have different feature layouts")
    if a.data:
        shards = load_shards(a.data, in_memory=a.data_in_memory)
        if a.cand_aux_weight > 0 and shards and a.drop_move_strike:
            # The hints of the v2 comparison: the targets beside the full
            # rows (the net's input zeroes the move-strike columns itself).
            from .cand_aux import select
            from .v2_bench import attach_targets

            select(a.cand_aux_targets)
            attach_targets(shards)
        elif a.cand_aux_weight > 0 and shards:
            from .cand_aux import attach, select

            select(a.cand_aux_targets)
            attach(shards)
        if not shards:
            raise SystemExit(f"no shards under {a.data}")
        if layout is not None and shards[0].layout.hash() != layout.hash():
            raise SystemExit("--data and --logs have different feature layouts")
        layout = layout or shards[0].layout
        if a.holdout_rungs:
            from .v2_bench import holdout_split

            shards, held = holdout_split(shards, a.holdout_rungs)
            (out / "holdout.txt").write_text("".join(f"{s.path}\n" for s in held))
            print(f"holdout {a.holdout_rungs}: {len(held)} shards left out of training (OUT/holdout.txt)", flush=True)
        train_shards, val_shards = split_by_game(shards, a.val_fraction)
        train_index = WindowIndex(train_shards, a.window)
        name = a.data_name or ("dagger" if a.logs else "data")
        sources[name] = (lambda: train_index.batches(a.windows_per_batch, rng), WindowIndex(val_shards, a.window))
        print(f"{name}: games {len(train_shards)} train, {len(val_shards)} held out; windows {len(train_index)} ({train_index.turns()} turns) train", flush=True)
    names = list(sources)
    primary = names[0]  # the expert logs when given: they decide the held-out checkpoint first and the integer check

    def train_iter():
        """One epoch of window batches. With both sources each batch is drawn
        from the second (--data or --logs2) with probability --mix; the epoch
        ends when the expert stream has made one pass (the second stream
        restarts when it runs out; with --mix 1 the epoch is one pass of it)."""
        if len(names) == 1:
            yield from sources[names[0]][0]()
            return
        if len(names) == 3:
            yield from three_iter()
            return
        second = names[1]
        expert, dagger = sources["expert"][0](), sources[second][0]()
        while True:
            if rng.random() < a.mix:
                batch = next(dagger, None)
                if batch is None:
                    if a.mix >= 1:
                        return
                    dagger = sources[second][0]()
                    batch = next(dagger, None)
                if batch is not None:
                    yield batch
                    continue
            batch = next(expert, None)
            if batch is None:
                return
            yield batch

    def three_iter():
        """`train_iter` of three sources (--logs, --logs2, --data): each batch
        from --data with probability --data-mix, else from --logs2 with
        probability --mix, else from the expert; the epoch ends with the
        expert stream's pass, the others restart when they run out."""
        streams = {n: sources[n][0]() for n in names}
        expert, second, third = names
        while True:
            r = rng.random()
            pick = third if r < a.data_mix else second if r < a.data_mix + (1 - a.data_mix) * a.mix else expert
            batch = next(streams[pick], None)
            if batch is None:
                if pick == expert:
                    return
                streams[pick] = sources[pick][0]()
                batch = next(streams[pick], None)
            if batch is not None:
                yield batch

    val_index = sources[primary][1]
    val_iter = lambda: val_index.batches(a.windows_per_batch)

    if a.events is None:
        a.events = layout.count("event") > 0
    if a.events and not layout.count("event"):
        raise SystemExit("--events needs shards with the event table (format 7)")
    carried = carried_config(a.init, a.preset, flag_config(a)) if a.init else {}
    if a.defer is not None:
        carried["defer"] = int(a.defer)
    if carried:
        print(f"config from --init and --defer: {carried}", flush=True)
    cfg = layout.config(a.preset, **{**carried, **flag_config(a)})
    if a.cand_aux_weight > 0:
        from .cand_aux import N as n_cand_aux

        cfg = dataclasses.replace(cfg, cand_aux=n_cand_aux)
    from .model_v2 import build_net

    model = build_net(cfg, layout)
    model.cand_aux_weight = a.cand_aux_weight
    model.combat_weight = a.combat_weight
    if a.combat_init and getattr(model, "combat", None) is not None:
        model.combat.load_state_dict(torch.load(a.combat_init, map_location="cpu", weights_only=False)["state"])
        print(f"combat module from {a.combat_init}", flush=True)
    init_report = init_from(model, a.init) if a.init else None
    if cfg.defer and init_report and "defer_bias" in init_report["new"]:
        zero_defer_tag(model)  # a net that gains the choice: the defer tag row as `aurelia_nn.defer` leaves it
    model = model.to(device)
    if a.autocast != "off":
        autocast_methods(model, device, torch.bfloat16 if a.autocast == "bf16" else torch.float16)
    rules = parse_decision_weights(a.decision_weight, layout)
    if rules:
        print(f"decision weights: {rules}", flush=True)
    print(f"parameters: {parameter_count(model)} ({a.preset}, flags {flag_config(a)}, defer {cfg.defer}); device {device}", flush=True)
    opt = make_optimizer(model, a.optim, a.lr, a.weight_decay)
    step = 0
    best = -1.0
    history = []
    t_start = time.time()
    clock = {"eval": 0.0, "last_eval": 0.0}  # seconds spent evaluating; training time at the last periodic evaluation
    train_seconds = lambda: time.time() - t_start - clock["eval"]
    out_of_time = lambda: a.max_minutes > 0 and train_seconds() > 60 * a.max_minutes
    curve = []  # periodic held-out evaluations (--eval-every-minutes)

    def lr_at(step: int) -> float:
        lr = a.lr * min(1.0, (step + 1) / 50)
        if a.schedule == "wsd" and a.max_minutes > 0:
            left = 1.0 - train_seconds() / (60 * a.max_minutes)
            if left < a.decay_share:
                lr *= max(0.05, left / a.decay_share)
        return lr
    for epoch in range(a.epochs):
        stats = HeadStats(layout.heads)
        turns_seen = 0
        t0 = time.time()
        for prepared in prefetch(train_iter(), lambda b: Prepared(window_turns(b), cfg, a.bptt)):
            for g in opt.param_groups:
                g["lr"] = lr_at(step)
            if a.cand_aux_decay > 0 and a.max_minutes > 0:
                model.cand_aux_weight = a.cand_aux_weight * max(0.0, 1.0 - train_seconds() / (60 * a.max_minutes * a.cand_aux_decay))
            turns_seen += run_windows(model, prepared, cfg, device, stats, a.value_weight, a.wait_weight, opt, a.bptt, a.aux_weight if a.aux else 0.0, rules)
            if cfg.arch == 2 and device.type == "mps":
                torch.mps.empty_cache()  # v2's whole-game pieces: hand the cached blocks back (the machine's memory is shared)
            step += 1
            if a.progress_every and step % a.progress_every == 0:
                tr = stats.report()
                print(f"  progress step {step}: train ce {tr['ce']:.3f} top1 {tr['all']:.3f} | "
                      f"{turns_seen / max(time.time() - t0, 1e-9):.1f} turns/s", flush=True)
            if a.eval_every_minutes and train_seconds() - clock["last_eval"] >= 60 * a.eval_every_minutes:
                te = time.time()
                clock["last_eval"] = train_seconds()
                vs = {n: evaluate(model, sources[n][1].batches(a.windows_per_batch), cfg, device) for n in names}
                curve.append({"step": step, "train_seconds": clock["last_eval"], "val_by_source": vs})
                print(f"  eval at step {step}, {clock['last_eval'] / 60:.1f} min: " + " ".join(f"{n} ce {v['ce']:.3f} top1 {v['all']:.3f}" for n, v in vs.items()), flush=True)
                save_checkpoint(out / "last.pt", model, {"history": history, "curve": curve})
                clock["eval"] += time.time() - te
            if (a.max_steps and step >= a.max_steps) or out_of_time():
                break
        dt = time.time() - t0
        te = time.time()
        vals = {n: evaluate(model, sources[n][1].batches(a.windows_per_batch), cfg, device) for n in names}
        val = vals[primary]
        tr = stats.report()
        rec = {"epoch": epoch, "step": step, "train": tr, "val": val, "turns_per_s": turns_seen / max(dt, 1e-9)}
        if cfg.cand_aux and a.data:
            from .cand_aux import evaluate as cand_aux_evaluate

            rec["cand_aux"] = cand_aux_evaluate(model, sources[a.data_name or ("dagger" if a.logs else "data")][1].batches(a.windows_per_batch), cfg, device, a.bptt)
            print(f"  cand aux (held-out): {json.dumps(rec['cand_aux'])}", flush=True)
        if len(names) > 1:
            rec["val_by_source"] = vals
        history.append(rec)
        print(f"epoch {epoch} step {step}: train ce {tr['ce']:.3f} top1 {tr['all']:.3f} | {rec['turns_per_s']:.1f} turns/s", flush=True)
        for n, v in vals.items():
            heads = " ".join(f"{h}={v[h]:.3f}" if v[h] is not None else f"{h}=-" for h in stats.groups)
            label = f"{n}-val" if len(names) > 1 else "held-out"
            print(f"  {label}: ce {v['ce']:.3f} top1 {v['all']:.3f} [{heads}]", flush=True)
        save_checkpoint(out / "last.pt", model, {"history": history, "curve": curve})
        clock["eval"] += time.time() - te
        score = _record_score(rec)
        if score > best:
            best = score
            save_checkpoint(out / "best.pt", model, {"history": history})
        if (a.max_steps and step >= a.max_steps) or out_of_time():
            break

    from .export import write
    from .model import load_checkpoint
    from .quantize import agreement, quantize

    best_model = load_checkpoint(out / "best.pt")
    v2 = cfg.arch == 2
    qm = None if v2 else quantize(best_model, layout)
    size = 0 if v2 else write(qm, out / "weights.bin")  # v2: float only, no weights file yet
    summary = {"curve": curve, "train_seconds": train_seconds(), "steps": step, "schedule": a.schedule, "autocast": a.autocast, "parameters": parameter_count(best_model), "preset": a.preset, "flags": flag_config(a), "optim": a.optim, "init": init_report,
               "decision_weights": [list(r) for r in rules], "weights_bytes": size, "best_val_score": best,
               "best_val": max(history, key=_record_score)["val"], "best_val_by_source": max(history, key=_record_score).get("val_by_source"),
               "sources": names, "mix": a.mix if len(names) > 1 else None, "data_mix": a.data_mix, "history": history,
               "seconds": time.time() - t_start}
    if a.eval_int and not v2:
        windows, n = [], 0
        for batch in val_iter():
            for w in window_turns(batch):
                windows.append(w[: a.eval_int - n])
                n += len(windows[-1])
                if n >= a.eval_int:
                    break
            if n >= a.eval_int:
                break
        summary["int_check"] = agreement(best_model, qm, windows, cfg)
        print(f"integer reference on {summary['int_check']['decisions']} held-out decisions ({n} turns, memory carried): {summary['int_check']}", flush=True)
    with open(out / "summary.json", "w") as f:
        json.dump(summary, f, indent=1)
    print(f"weights: {out / 'weights.bin'} ({size} bytes)" if not v2 else f"v2: no weights file; best checkpoint {out / 'best.pt'}", flush=True)
    return summary


if __name__ == "__main__":
    main()
