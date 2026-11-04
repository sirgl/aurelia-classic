"""The v2 comparison's measurements (docs/ai-net-model.md, «v2 architecture»).

    python -m aurelia_nn.v2_bench holdout DATA --spec 27-34:0.25
        the siege scenes `train_imitation --holdout-rungs` leaves out
    python -m aurelia_nn.v2_bench pretrain-combat DATA --out combat.pt
        the combat module distilled from the attack candidates' previews
    python -m aurelia_nn.v2_bench materialize-val --logs LIST --features-bin "BIN ARGS" --out DIR
        the held-out whole games of a log list (train_imitation's split), once
    python -m aurelia_nn.v2_bench eval CKPT --data DATA --holdout HOLDOUT.txt --val-expert DIR --val-books DIR --out OUT.json
        held-out CE and top-1 (whole games, books, grounds), top-1 on the
        held-out scenes' unit decisions split into moves and attacks, the
        combat module's accuracy, and detached probes: the candidate state →
        the move-strike quantities, the trunk's tile state at the
        candidate's tile → the danger and focus fields
    python -m aurelia_nn.v2_bench profile --preset v2 --data DATA [--autocast bf16]
        one training step taken apart (data, forward parts, backward, optimizer)
"""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import re
import time
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn

from . import cand_aux
from .common import HEAD_UNIT
from .data import Shard, WindowIndex, load_shards, split_by_game

_SCENE = re.compile(r"^(?:dup\d+-)?(search-random|search-siege|scripted-siege)-r(\d+)-g\d+m\d+-(.+?)(?:\.shard)?$")


def scene_of(shard_path) -> tuple[int, str] | None:
    """(rung, scene key) of a siege ground shard by its name (the copies
    `dupN-` share the key), None for another shard."""
    m = _SCENE.match(Path(shard_path).name)
    if not m:
        return None
    rung = int(m.group(2))
    return rung, f"r{rung:02d}/{m.group(1).replace('search-siege', 'search-random')}/{m.group(3)}"


def _h(key: str) -> float:
    return int(hashlib.sha1(("holdout:" + key).encode()).hexdigest()[:8], 16) / 0xFFFFFFFF


def holdout_split(shards: list, spec: str):
    """(kept, held out): `spec` RUNGS:SHARE (27-34:0.25) — a scene of those
    rungs is held out, every copy of it, when the hash of its key falls
    under SHARE."""
    rungs, share = spec.split(":")
    lo, _, hi = rungs.partition("-")
    lo, hi, share = int(lo), int(hi or lo), float(share)
    kept, held = [], []
    for s in shards:
        sc = scene_of(s.path)
        (held if sc and lo <= sc[0] <= hi and _h(sc[1]) < share else kept).append(s)
    return kept, held


def is_copy(path) -> bool:
    return Path(path).name.startswith("dup")


# ---------------------------------------------------------------- combat


def attack_pairs(shards: list) -> dict:
    """The attack candidates with a preview of the shards' unit decisions:
    attacker and defender entity rows and their tiles' rows, the targets
    (model_v2.combat_targets)."""
    from .model_v2 import combat_targets

    layout = shards[0].layout
    names = list(layout.names["candidate"])
    col = {n: i for i, n in enumerate(names)}
    hp = list(layout.names["entity"]).index("hp")
    out = {k: [] for k in ("ae", "at", "de", "dt", "y")}
    for s in shards:
        for t in range(len(s)):
            x = s.turn(t)
            et = x.entity_meta[:, 1]
            for d in x.decisions:
                if d.head != HEAD_UNIT or d.actor < 0:
                    continue
                r = d.rows
                ent = d.meta[:, 2]
                att = ((r[:, col["act_melee"]] + r[:, col["act_ranged"]]) > 0) & (r[:, col["preview_defender_hp"]] > 0) & (ent >= 0)
                if not att.any() or et[d.actor] < 0:
                    continue
                idx = np.nonzero(att)[0]
                de = ent[idx]
                ok = et[de] >= 0
                idx, de = idx[ok], de[ok]
                if not len(idx):
                    continue
                y = combat_targets(torch.as_tensor(r[idx].astype(np.float32)), names, torch.full((len(idx),), float(x.entities[d.actor, hp]))).numpy()
                out["ae"].append(np.repeat(x.entities[d.actor][None], len(idx), 0))
                out["at"].append(np.repeat(x.tiles[et[d.actor]][None], len(idx), 0))
                out["de"].append(x.entities[de])
                out["dt"].append(x.tiles[et[de]])
                out["y"].append(y)
    return {k: (np.concatenate(v) if v else np.zeros((0, 4), np.float32)) for k, v in out.items()}


def combat_metrics(pred: np.ndarray, y: np.ndarray) -> dict:
    """MAE of the shares (percent points), accuracy, recall and precision of
    kill and capture (base: predicting the majority)."""
    out = {"n": int(len(y))}
    for j, n in enumerate(("dealt", "taken")):
        out[f"{n}_mae_pct"] = float(np.abs(pred[:, j] - y[:, j]).mean() * 100)
        out[f"{n}_mae_pct_mean"] = float(np.abs(y[:, j].mean() - y[:, j]).mean() * 100)
    for j, n in ((2, "kills"), (3, "captures")):
        yb, qb = y[:, j] > 0.5, pred[:, j] > 0
        out[n] = {"positives": int(yb.sum()), "accuracy": float((yb == qb).mean()), "base": float(max(yb.mean(), 1 - yb.mean())),
                  "recall": float((qb & yb).sum() / max(yb.sum(), 1)), "precision": float((qb & yb).sum() / max(qb.sum(), 1))}
    return out


def pretrain_combat(a):
    from .model_v2 import Combat, combat_loss

    shards = load_shards(a.data, in_memory=True)
    kept, held = holdout_split(shards, a.spec)
    kept = [s for s in kept if not is_copy(s.path)]
    held = [s for s in held if not is_copy(s.path)]
    t0 = time.time()
    tr, te = attack_pairs(kept), attack_pairs(held)
    print(f"attack pairs: {len(tr['y'])} train, {len(te['y'])} held-out scenes ({time.time() - t0:.0f} s)", flush=True)
    torch.manual_seed(0)
    m = Combat(shards[0].layout)
    side = lambda d, sl: (m.side(torch.as_tensor(d["ae"][sl].astype(np.float32)), torch.as_tensor(d["at"][sl].astype(np.float32))),
                          m.side(torch.as_tensor(d["de"][sl].astype(np.float32)), torch.as_tensor(d["dt"][sl].astype(np.float32))))
    xa, xd = side(tr, slice(None))
    y = torch.as_tensor(tr["y"])
    ya, yd = side(te, slice(None))
    opt = torch.optim.AdamW(m.parameters(), lr=a.lr, weight_decay=1e-4)
    n = len(y)
    rng = np.random.default_rng(0)
    for step in range(a.steps):
        lr = a.lr * min(1.0, (step + 1) / 100) * (1.0 if step < 0.8 * a.steps else max(0.05, (a.steps - step) / (0.2 * a.steps)))
        for g in opt.param_groups:
            g["lr"] = lr
        idx = torch.as_tensor(rng.integers(0, n, a.batch))
        out, _ = m.run(m.att(xa[idx]) + m.dfd(xd[idx]))
        loss = combat_loss(out, y[idx], torch.ones(len(idx), dtype=torch.bool)) / len(idx)
        opt.zero_grad()
        loss.backward()
        opt.step()
        if step % 1000 == 0 or step == a.steps - 1:
            with torch.no_grad():
                pe, _ = m.run(m.att(ya) + m.dfd(yd))
            rep = combat_metrics(pe.numpy(), te["y"])
            print(f"step {step}: loss {loss.item():.4f} | held-out dealt MAE {rep['dealt_mae_pct']:.1f} pp, kills recall {rep['kills']['recall']:.2f} "
                  f"precision {rep['kills']['precision']:.2f}, captures recall {rep['captures']['recall']:.2f}", flush=True)
    with torch.no_grad():
        pe, _ = m.run(m.att(ya) + m.dfd(yd))
    rep = combat_metrics(pe.numpy(), te["y"])
    torch.save({"state": m.state_dict(), "metrics": rep, "train_pairs": int(n)}, a.out)
    print(json.dumps(rep, indent=1))


# ---------------------------------------------------------------- held-out logs


def materialize_val(a):
    """The held-out logs of `--logs` (train_imitation's split: the name's
    hash under --val-fraction) materialized into --out, once."""
    import shlex
    import subprocess
    from concurrent.futures import ThreadPoolExecutor

    from .train_imitation import _game_hash, _list_logs

    logs = _list_logs(a.logs)
    val = [l for l in logs if _game_hash(Path(l).name) < a.val_fraction] or logs[:1]
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    cmd = shlex.split(a.features_bin)

    def one(i_log):
        i, log = i_log
        d = out / f"{i:03d}"
        if (d / "done").exists():
            return
        d.mkdir(exist_ok=True)
        p = subprocess.run(cmd + [log, "--out", str(d)], capture_output=True, text=True)
        if p.returncode:
            print(f"skip {log}: {p.stderr[-500:]}", flush=True)
            return
        (d / "done").write_text(log)

    with ThreadPoolExecutor(a.jobs) as pool:
        list(pool.map(one, enumerate(val)))
    print(f"{len(val)} held-out logs in {out}", flush=True)


# ---------------------------------------------------------------- evaluation


class SplitStats:
    """Top-1 of the unit decisions (not waits) by the chosen candidate's
    kind: a move, an attack, else."""

    def __init__(self, names):
        self.c = {n: i for i, n in enumerate(names)}
        self.right = {"move": 0, "attack": 0, "other": 0}
        self.total = {"move": 0, "attack": 0, "other": 0}

    def add(self, batch, pad):
        host = batch["np"]
        n = host["n_dec"]
        if not n:
            return
        right = (pad[:n].argmax(1) == batch["dec_choice"][:n]).cpu().numpy()
        rows = host["dec_row"]
        heads, wait = host["dec_head"][:n], host["dec_wait"][:n]
        mv = rows[:, self.c["act_move"]] > 0
        at = (rows[:, self.c["act_melee"]] + rows[:, self.c["act_ranged"]]) > 0
        for kind, sel in (("move", mv), ("attack", at), ("other", ~mv & ~at)):
            sel = sel & (heads == HEAD_UNIT) & ~wait
            self.right[kind] += int(right[sel].sum())
            self.total[kind] += int(sel.sum())

    def report(self):
        return {k: {"top1": self.right[k] / max(self.total[k], 1), "n": self.total[k]} for k in self.right}


class Capture:
    """Wraps a net's `score` and `per_head` to collect, per candidate with an
    aux target, the candidate state the logit layer reads and the trunk's
    tile state at the candidate's tile (detached, on the host), the targets,
    and v2's combat module outputs on the attack candidates."""

    def __init__(self, model):
        self.model = model
        self.on = False
        self.hid, self.tile, self.y, self.cpred, self.cy, self.fields = [], [], [], [], [], []
        self.split = None
        score, per_head = model.score, model.per_head
        cap = self

        def per_head_wrapped(x, heads, groups, fn, width):
            if width == 1:
                cap._hid = x
            return per_head(x, heads, groups, fn, width)

        def score_wrapped(batch, tile_h, tokens, off):
            out = score(batch, tile_h, tokens, off)
            if cap.on and "cand_aux" in batch:
                y = batch["cand_aux"]
                keep = torch.isfinite(y).any(-1)
                cd = batch["cand_dec"]
                ct = batch["cand_tile"]
                th = tile_h[batch["dec_b"][cd], ct.clamp(min=0)] * (ct >= 0)[:, None].to(tile_h.dtype)
                n = len(y)
                cap.hid.append(cap._hid[:n][keep[:n]].detach().half().cpu())
                cap.tile.append(th[:n][keep[:n]].detach().half().cpu())
                cap.y.append(y[:n][keep[:n]].cpu())
                fo = getattr(cap.model, "fields_out", None)
                if fo is not None:
                    cap.fields.append(fo[:n][keep[:n]].detach().float().cpu())
                if getattr(cap.model, "combat", None) is not None:
                    from .model_v2 import Combat, combat_targets

                    dec_b, actor, cent = batch["dec_b"], batch["dec_actor"], batch["cand_ent"]
                    pair = cap.model.pair_features(batch, dec_b, actor, batch["ent_tile"])
                    tgt = combat_targets(batch["cand_rows"], cap.model.cand_names, batch["entities"][dec_b[cd], actor[cd].clamp(min=0), cap.model.ent_names.index("hp")])
                    ok = torch.isfinite(tgt[:, 0]) & (cent >= 0) & (actor[cd] >= 0)
                    cap.cpred.append(pair[cd, cent.clamp(min=0), : Combat.OUT][ok].float().cpu())
                    cap.cy.append(tgt[ok].cpu())
            return out

        model.per_head = per_head_wrapped
        model.score = score_wrapped

    def take(self):
        out = (torch.cat(self.hid).float(), torch.cat(self.tile).float(), torch.cat(self.y))
        self.hid, self.tile, self.y = [], [], []
        return out


def attach_targets(shards):
    """The aux targets on each turn (`dec_arrays["cand_aux"]`, cand_aux.targets_of)
    from the full format-10 rows and candidate_targets.npy; the rows and the
    layout stay as they are."""
    layout = shards[0].layout
    names = list(layout.names["candidate"])
    ent_names = list(layout.names["entity"])
    e_civ, e_str, e_rng = ent_names.index("civilian"), ent_names.index("strength"), ent_names.index("ranged_strength")
    for shard in shards:
        path = Path(shard.path) / "candidate_targets.npy"
        meta_extra = json.loads((Path(shard.path) / "meta.json").read_text())["layout"].get("candidate_targets", [])
        extra_names = [e["name"] for e in meta_extra]
        extra = np.load(path) if path.exists() else None
        if extra is not None:
            # One array: the rows and their targets side by side (Shard.turn gathers both).
            shard.candidates = np.concatenate([np.asarray(shard.candidates), extra.astype(np.int16)], axis=1)
        original = shard.turn
        n = len(names)

        def turn(t, original=original, has_extra=extra is not None, extra_names=extra_names):
            x = original(t)
            arrays = x.dec_arrays
            full = arrays["rows"]
            rows, ext = np.ascontiguousarray(full[:, :n]), (full[:, n:] if has_extra else None)
            actor = np.repeat(arrays["actor"], arrays["k"])
            fights = np.zeros(len(rows), bool)
            ok = actor >= 0
            fights[ok] = (x.entities[actor[ok], e_civ] == 0) & ((x.entities[actor[ok], e_str] > 0) | (x.entities[actor[ok], e_rng] > 0))
            fights &= np.repeat(arrays["head"] == HEAD_UNIT, arrays["k"])
            x.dec_arrays = dict(arrays, rows=rows, cand_aux=cand_aux.targets_of(rows, ext, names, extra_names, fights))
            starts = np.cumsum(arrays["k"]) - arrays["k"]
            for d, o, k in zip(x.decisions, starts, arrays["k"]):
                d.rows = rows[o : o + k]
            return x

        shard.turn = turn
    return shards


def run_eval(model, shards, cfg, device, split=None, capture=None, wpb=16):
    """train_imitation.evaluate over the shards' windows, with the split
    stats and the capture."""
    from .model import padded_logits
    from .train_imitation import HeadStats, Prepared, prefetch, run_windows, window_turns

    stats = HeadStats(model.heads)
    if split is not None:
        add = stats.add

        def add2(batch, pad, ce):
            split.add(batch, pad)
            add(batch, pad, ce)

        stats.add = add2
    if capture is not None:
        capture.on = True
    model.eval()
    with torch.no_grad():
        for prepared in prefetch(WindowIndex(shards, 32).batches(wpb), lambda b: Prepared(window_turns(b), cfg, 8)):
            run_windows(model, prepared, cfg, device, stats)
    if capture is not None:
        capture.on = False
    return stats.report()


def fit_probe(xtr, ytr, xte, yte, steps=3000, seed=0, hidden=128):
    """A detached probe (the net frozen): Linear → ReLU → Linear to the aux
    targets, masked BCE / squared error, AdamW; cand_aux.Metrics on the
    held-out features."""
    torch.manual_seed(seed)
    mu, sd = xtr.mean(0), xtr.std(0) + 1e-4
    xtr, xte = (xtr - mu) / sd, (xte - mu) / sd
    m = nn.Sequential(nn.Linear(xtr.shape[1], hidden), nn.ReLU(), nn.Linear(hidden, ytr.shape[1]))
    opt = torch.optim.AdamW(m.parameters(), lr=2e-3, weight_decay=1e-4)
    binary = torch.as_tensor(cand_aux.BINARY)
    g = torch.Generator().manual_seed(seed)
    for step in range(steps):
        idx = torch.randint(0, len(xtr), (1024,), generator=g)
        p, t = m(xtr[idx]), ytr[idx]
        ok = torch.isfinite(t)
        tt = t.nan_to_num()
        per = torch.where(binary, nn.functional.binary_cross_entropy_with_logits(p, tt.clamp(0, 1), reduction="none"), (p - tt) ** 2)
        loss = (per * ok).sum() / ok.sum().clamp(min=1)
        opt.zero_grad()
        loss.backward()
        opt.step()
    met = cand_aux.Metrics()
    with torch.no_grad():
        met.add(m(xte), yte)
    return met.report()


def eval_data(data: str, holdout: str | None, val_expert: str | None, val_books: str | None, val_fraction: float = 0.03,
              probe_rungs: str = "27-34") -> dict:
    """The evaluation's shards, loaded once: the held-out whole games and
    books (memory-mapped), the ground's held-out games, the held-out siege
    scenes (originals) and the probe's training scenes (the kept originals
    of `probe_rungs`), the ground ones read into memory and only those (the
    whole ground is 5 GB); the scenes with the aux targets attached."""
    from .data import Shard, find_shards

    out = {}
    for name, d in (("expert", val_expert), ("books", val_books)):
        if d:
            out[name] = load_shards(d)
    paths = find_shards(data)
    held_paths = set(Path(holdout).read_text().split()) if holdout else set()
    kept = [p for p in paths if str(p) not in held_paths]
    held = [p for p in paths if str(p) in held_paths and not is_copy(p)]
    _, gval = split_by_game([_Named(p) for p in kept], val_fraction)
    lo, hi = (int(x) for x in probe_rungs.split("-"))
    ptrain = [p for p in kept if (sc := scene_of(p)) and lo <= sc[0] <= hi and not is_copy(p)]
    out["ground"] = [Shard(s.path, in_memory=True) for s in gval]
    # Separate copies: attach_targets patches its shards' turns.
    out["held"] = attach_targets([Shard(p, in_memory=True) for p in held])
    out["ptrain"] = attach_targets([Shard(p, in_memory=True) for p in ptrain]) if ptrain else []
    return out


class _Named:
    """A shard path as split_by_game reads it (its `game`, the name)."""

    def __init__(self, path):
        self.path = path
        self.game = Path(path).name


def prepare_model(model, device, autocast: str = "off"):
    """The net as the evaluation runs it: the candidate aux head replaced by
    the identity so that `score` keeps the candidate state (the probes')."""
    model = model.to(device)
    if autocast != "off":
        from .train_imitation import autocast_methods

        autocast_methods(model, device, torch.bfloat16)
    cfg = dataclasses.replace(model.cfg, cand_aux=cand_aux.N)
    model.cfg = cfg
    model.cand_aux_head = nn.Identity()
    model.cand_aux_sink = None
    if hasattr(model, "aux_input"):
        model.aux_input = lambda h: h
    return model


def evaluate_checkpoint(a):
    from .model import load_checkpoint
    from .train_imitation import pick_device

    device = pick_device(a.device)
    data = eval_data(a.data, a.holdout, a.val_expert, a.val_books, a.val_fraction, a.probe_rungs)
    res = evaluate_model(load_checkpoint(a.checkpoint), data, device, a.autocast, getattr(a, "windows_per_batch", 16))
    res = {"checkpoint": a.checkpoint} | res
    Path(a.out).write_text(json.dumps(res, indent=1))
    print(f"wrote {a.out} ({res['seconds']:.0f} s)", flush=True)


def evaluate_model(model, data: dict, device, autocast: str = "off", wpb: int = 16) -> dict:
    """v2_bench eval's measurements of a net on `eval_data`'s shards."""
    model = prepare_model(model, device, autocast)
    cfg = model.cfg
    res = {"parameters": sum(p.numel() for p in model.parameters()), "drop_move_strike": int(getattr(model.cfg, "drop_move_strike", 0))}
    t0 = time.time()
    # Whole games held out (train_imitation's split, materialized once).
    for name in ("expert", "books"):
        if name in data:
            res[name] = run_eval(model, data[name], cfg, device, wpb=wpb)
            print(f"{name}: ce {res[name]['ce']:.3f} top1 {res[name]['all']:.3f} ({time.time() - t0:.0f} s)", flush=True)
    res["ground"] = run_eval(model, data["ground"], cfg, device, wpb=wpb)
    print(f"ground: ce {res['ground']['ce']:.3f} top1 {res['ground']['all']:.3f} ({time.time() - t0:.0f} s)", flush=True)
    held = data["held"]
    cap = Capture(model)
    names = list(held[0].layout.names["candidate"])
    out = {}
    for teacher in ("search", "scripted"):
        sel = [s for s in held if ("/search-random/" in scene_of(s.path)[1]) == (teacher == "search")]
        split = SplitStats(names)
        rep = run_eval(model, sel, cfg, device, split=split, capture=cap, wpb=wpb)
        out[teacher] = {"shards": len(sel), "ce": rep["ce"], "unit_no_wait": rep["unit_no_wait"], "unit": rep["unit"], **split.report()}
    res["holdout_scenes"] = out
    hte, tte, yte = cap.take()
    if cap.fields:
        # v2's sum heads read as they are against the rules' team targets.
        f = torch.cat(cap.fields).numpy()
        y = yte.numpy()
        idx = {t[0]: j for j, t in enumerate(cand_aux.TARGETS)}
        res["sum_heads"] = {}
        for name, j, tgt in (("focus", 0, "focus_damage"), ("danger", 1, "danger_damage")):
            ok = np.isfinite(y[:, idx[tgt]])
            if ok.sum() > 2:
                res["sum_heads"][name] = {"n": int(ok.sum()), "pearson": float(np.corrcoef(f[ok, j], y[ok, idx[tgt]])[0, 1]),
                                          "mae_pct": float(np.abs(f[ok, j] - y[ok, idx[tgt]]).mean() * 100)}
        cap.fields = []
    cte = (torch.cat(cap.cpred), torch.cat(cap.cy)) if cap.cpred else None
    cap.cpred, cap.cy = [], []
    print(f"held-out scenes: {json.dumps(out)} ({time.time() - t0:.0f} s)", flush=True)
    # Probe training features: the kept siege scenes of the same rungs (originals).
    run_eval(model, data["ptrain"], cfg, device, capture=cap, wpb=wpb)
    htr, ttr, ytr = cap.take()
    print(f"probe features: {len(htr)} train, {len(hte)} held-out candidates ({time.time() - t0:.0f} s)", flush=True)
    res["probe_candidate"] = fit_probe(htr, ytr, hte, yte)
    res["probe_trunk_tile"] = fit_probe(ttr, ytr, tte, yte)
    if cte is not None and len(cte[0]):
        res["combat_holdout"] = combat_metrics(cte[0].numpy(), cte[1].numpy())
    res["seconds"] = time.time() - t0
    return res


# ---------------------------------------------------------------- profile


def profile(a):
    """Times training steps on ground + whole-game windows: data (collate),
    the forward parts, backward, optimizer; synchronized per part."""
    from .model_v2 import build_net
    from .train_imitation import Prepared, autocast_methods, make_optimizer, pick_device, run_windows, window_turns, HeadStats

    device = pick_device(a.device)
    shards = load_shards(a.data, in_memory=True)
    if a.whole:
        shards = load_shards(a.whole) + shards[: a.ground]
    else:
        shards = shards[: a.ground]
    layout = shards[0].layout
    cfg = layout.config(a.preset, trace=1, events=1, defer=1, **({"drop_move_strike": 1} if a.drop else {}))
    torch.manual_seed(0)
    model = build_net(cfg, layout).to(device)
    if hasattr(model, "checkpoint_map"):
        model.checkpoint_map = a.checkpoint_map
    model.combat_weight = 1.0
    if a.autocast != "off":
        autocast_methods(model, device, torch.bfloat16 if a.autocast == "bf16" else torch.float16)
    opt = make_optimizer(model, a.optim, a.lr, 1e-4)
    rng = np.random.default_rng(0)
    idx = WindowIndex(shards, 32)
    sync = (lambda: torch.mps.synchronize()) if device.type == "mps" else (lambda: None)
    times = {k: [] for k in ("data", "step")}
    parts = {}
    mem = {}  # MB allocated on the device after each part (max over steps)
    # Per-part timing hooks (synchronizing: slows the step a little).
    if a.parts:
        def timed(name, fn):
            def w(*x, **k):
                sync()
                t = time.time()
                r = fn(*x, **k)
                sync()
                parts.setdefault(name, []).append(time.time() - t)
                if device.type == "mps":
                    mem[name] = max(mem.get(name, 0), torch.mps.current_allocated_memory() / 2**20)
                return r
            return w
        model.embed = timed("embed (map, tokens)", model.embed)
        model.attend = timed("attend (transformer)", model.attend)
        model.score = timed("score (candidates)", model.score)
        opt_step = opt.step
        opt.step = timed("optimizer", opt_step)
    if device.type == "mps":
        torch.mps.reset_peak_memory_stats() if hasattr(torch.mps, "reset_peak_memory_stats") else None
    batches = idx.batches(a.windows_per_batch, rng)
    first, last = HeadStats(model.heads), HeadStats(model.heads)  # train CE of the first and the last quarter of the steps
    for i in range(a.steps):
        t = time.time()
        b = next(batches)
        prep = Prepared(window_turns(b), cfg, 8)
        times["data"].append(time.time() - t)
        sync()
        t = time.time()
        st = first if i < a.steps // 4 else last if i >= a.steps - a.steps // 4 else HeadStats(model.heads)
        run_windows(model, prep, cfg, device, st, 0.1, 0.25, opt, 8)
        sync()
        times["step"].append(time.time() - t)
    skip = min(3, a.steps - 1)
    rep = {k: float(np.mean(v[skip:]) * 1000) for k, v in times.items()}
    rep |= {f"part {k}": float(np.sum(v) / max(a.steps, 1) * 1000) for k, v in parts.items()}
    if device.type == "mps":
        rep["peak_mb"] = torch.mps.driver_allocated_memory() / 2**20
    rep["parameters"] = sum(p.numel() for p in model.parameters())
    rep |= {f"mem after {k}": v for k, v in mem.items()}
    rep["train_ce_first_quarter"] = first.report()["ce"]
    rep["train_ce_last_quarter"] = last.report()["ce"]
    print(json.dumps(rep, indent=1))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("holdout")
    p.add_argument("data")
    p.add_argument("--spec", default="27-34:0.25")
    p = sub.add_parser("pretrain-combat")
    p.add_argument("data")
    p.add_argument("--spec", default="27-34:0.25")
    p.add_argument("--out", required=True)
    p.add_argument("--steps", type=int, default=6000)
    p.add_argument("--batch", type=int, default=512)
    p.add_argument("--lr", type=float, default=2e-3)
    p = sub.add_parser("materialize-val")
    p.add_argument("--logs", required=True)
    p.add_argument("--features-bin", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--val-fraction", type=float, default=0.03)
    p.add_argument("--jobs", type=int, default=4)
    p = sub.add_parser("eval")
    p.add_argument("checkpoint")
    p.add_argument("--data", required=True)
    p.add_argument("--holdout", help="train_imitation's OUT/holdout.txt")
    p.add_argument("--val-expert")
    p.add_argument("--val-books")
    p.add_argument("--val-fraction", type=float, default=0.03)
    p.add_argument("--probe-rungs", default="27-34")
    p.add_argument("--device", default="auto")
    p.add_argument("--autocast", default="off", choices=["off", "bf16"])
    p.add_argument("--windows-per-batch", type=int, default=16)
    p.add_argument("--out", required=True)
    p = sub.add_parser("profile")
    p.add_argument("--preset", default="v2")
    p.add_argument("--data", required=True)
    p.add_argument("--whole", help="a directory of whole-game shards mixed in")
    p.add_argument("--ground", type=int, default=400)
    p.add_argument("--steps", type=int, default=12)
    p.add_argument("--windows-per-batch", type=int, default=16)
    p.add_argument("--device", default="auto")
    p.add_argument("--autocast", default="off", choices=["off", "bf16", "fp16"])
    p.add_argument("--optim", default="muon")
    p.add_argument("--lr", type=float, default=1e-3)
    p.add_argument("--parts", action="store_true")
    p.add_argument("--drop", action="store_true")
    p.add_argument("--checkpoint-map", action=argparse.BooleanOptionalAction, default=True)
    a = ap.parse_args(argv)
    if a.cmd == "holdout":
        shards = load_shards(a.data)
        kept, held = holdout_split(shards, a.spec)
        keys = {scene_of(s.path)[1] for s in held}
        print(f"{len(held)} shards held out ({len([s for s in held if not is_copy(s.path)])} originals, {len(keys)} scenes); {len(kept)} kept")
    elif a.cmd == "pretrain-combat":
        pretrain_combat(a)
    elif a.cmd == "materialize-val":
        materialize_val(a)
    elif a.cmd == "eval":
        evaluate_checkpoint(a)
    elif a.cmd == "profile":
        profile(a)


if __name__ == "__main__":
    main()
