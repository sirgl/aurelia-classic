"""The candidate auxiliary head (classic/docs/ai-net-data.md, «Format 10»):
training only, the net predicts per unit candidate what the rules' previews
say of it — the strike from a move's destination, the hostile units' nearness
and the next turn, a blow's outcome, the team's focus fire and the danger of a
tile — from the hidden state the unit head's logit reads (the candidate's
encoding plus the decision's query: the encoding alone does not know the
acting unit). With the format-10 shards' move-strike columns cut from the
input (`attach`), the net must compute them itself.

    train_imitation --cand-aux-weight W     (ground shards of format 10)
    python -m aurelia_nn.cand_aux CKPT SHARDS [--val-fraction F]   held-out accuracy
"""

from __future__ import annotations

import argparse
import copy
import json
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F

from .move_strike import FORMAT_10_CANDIDATE, format_9_of

# (name, source column, kind, divisor, applies to): the per-unit targets read
# off a format-10 candidate row, the team targets off candidate_targets.npy.
# `applies`: "move" a move of a unit that fights, "attack" a blow with a
# preview, "strike" a blow or a move that strikes this turn.
TARGETS = (
    ("move_blow_captures", "move_blow_captures", "bce", 1.0, "move"),
    ("move_blow_kills", "move_blow_kills", "bce", 1.0, "move"),
    ("move_blow_damage", "move_blow_damage_percent", "mse", 100.0, "move"),
    ("move_city_damage", "move_city_damage_percent", "mse", 100.0, "move"),
    ("move_city_adjacent", "move_city_adjacent", "bce", 1.0, "move"),
    ("move_city_in_strike", "move_city_in_strike", "bce", 1.0, "move"),
    ("move_strike_targets", "move_strike_targets", "mse", 4.0, "move"),
    ("enemy_distance", "move_enemy_distance", "mse", 10.0, "move"),
    ("wounded_distance", "move_wounded_enemy_distance", "mse", 10.0, "move"),
    ("wounded_hp", "move_wounded_enemy_hp_percent", "mse", 100.0, "move"),
    ("next_turn_targets", "move_next_turn_targets", "mse", 4.0, "move"),
    ("next_turn_kills", "move_next_turn_kills", "bce", 1.0, "move"),
    ("city_distance", "move_city_distance", "mse", 10.0, "move"),
    ("unseen_city_in_reach", "move_unseen_city_in_reach", "bce", 1.0, "move"),
    ("attack_dealt", "preview_damage_to_defender", "mse", 100.0, "attack"),
    ("attack_taken", "preview_damage_to_attacker", "mse", 100.0, "attack"),
    ("attack_kills", "blow_kills", "bce", 1.0, "attack"),
    ("attack_captures", "blow_captures", "bce", 1.0, "attack"),
    ("focus_damage", "focus_damage_percent", "mse", 100.0, "strike"),
    ("focus_takes", "focus_takes", "bce", 1.0, "strike"),
    ("danger_damage", "danger_damage", "mse", 100.0, "move"),
    ("danger_dies", "danger_dies", "bce", 1.0, "move"),
)
TEAM = ("focus_damage", "focus_takes", "danger_damage", "danger_dies")
# Target groups (`train_imitation --cand-aux-targets`): the strike from a
# move's tile, a blow's preview, focus fire, the enemy's nearness and the
# next turn, the danger of a tile.
GROUPS = {
    "strike": ("move_blow_captures", "move_blow_kills", "move_blow_damage", "move_city_damage", "move_city_adjacent", "move_city_in_strike",
               "move_strike_targets", "city_distance", "unseen_city_in_reach"),
    "attack": ("attack_dealt", "attack_taken", "attack_kills", "attack_captures"),
    "focus": ("focus_damage", "focus_takes"),
    "near": ("enemy_distance", "wounded_distance", "wounded_hp", "next_turn_targets", "next_turn_kills"),
    "danger": ("danger_damage", "danger_dies"),
}
ACTIVE = np.ones(len(TARGETS), bool)


def select(groups: str):
    """Trains only the targets of `groups` (comma-separated; empty: all);
    the others stay NaN (no loss, no metric)."""
    names = {n for g in groups.split(",") if g for n in GROUPS[g]} if groups else {t[0] for t in TARGETS}
    ACTIVE[:] = [t[0] in names for t in TARGETS]
N = len(TARGETS)
BINARY = np.array([k == "bce" for _, _, k, _, _ in TARGETS])


def targets_of(rows: np.ndarray, extra: np.ndarray | None, names: list[str], extra_names: list[str], fights: np.ndarray) -> np.ndarray:
    """f32 [k, N]: the targets of candidate rows (full format-10 rows), NaN
    where a target does not apply; `fights` per row: its unit fights."""
    col = {n: i for i, n in enumerate(names)}
    xcol = {n: i for i, n in enumerate(extra_names)}
    r = rows.astype(np.float32)
    move = (r[:, col["act_move"]] > 0) & fights
    attack = ((r[:, col["act_melee"]] + r[:, col["act_ranged"]]) > 0) & (r[:, col["preview_defender_hp"]] > 0)
    strike = attack | (move & (r[:, col["move_strike_targets"]] > 0))
    where = {"move": move, "attack": attack, "strike": strike}
    out = np.full((len(rows), N), np.nan, np.float32)
    for j, (_, source, _, div, applies) in enumerate(TARGETS):
        if source in col:
            v = r[:, col[source]]
        elif extra is not None and source in xcol:
            v = extra[:, xcol[source]].astype(np.float32)
        else:
            continue
        if source == "preview_damage_to_defender":
            v = v * 100.0 / np.maximum(r[:, col["preview_defender_hp"]], 1.0)
        out[:, j] = np.where(where[applies] & ACTIVE[j], v / div, np.nan)
    return out


def attach(shards) -> "Layout":
    """Cuts the format-10 shards' candidate rows to format 9's and gives each
    turn its targets (`dec_arrays["cand_aux"]`); returns the format-9 layout
    the net reads. The shards' own layout becomes it, so they mix with
    format-9 sources."""
    if not shards:
        raise ValueError("no shards")
    layout10 = shards[0].layout
    layout9 = format_9_of(layout10)
    layout9.heads, layout9.tags, layout9.extra = layout10.heads, layout10.tags, layout10.extra
    names = list(layout10.names["candidate"])
    n_in = layout9.count("candidate")
    ent_names = list(layout10.names["entity"])
    e_civ, e_str, e_rng = ent_names.index("civilian"), ent_names.index("strength"), ent_names.index("ranged_strength")
    for shard in shards:
        if shard.layout.hash() != layout10.hash():
            raise ValueError(f"{shard.path}: another layout")
        path = Path(shard.path) / "candidate_targets.npy"
        meta_extra = json.loads((Path(shard.path) / "meta.json").read_text())["layout"].get("candidate_targets", [])
        extra_names = [e["name"] for e in meta_extra]
        extra = np.load(path) if path.exists() else None
        if extra is not None:
            # One array: the rows and their targets side by side (Shard.turn gathers both).
            shard.candidates = np.concatenate([np.asarray(shard.candidates), extra.astype(np.int16)], axis=1)
        original = shard.turn
        k_extra = 0 if extra is None else extra.shape[1]

        def turn(t, original=original, k_extra=k_extra, extra_names=extra_names):
            x = original(t)
            arrays = x.dec_arrays
            full = arrays["rows"]
            rows, ext = full[:, : len(names)], (full[:, len(names) :] if k_extra else None)
            actor = np.repeat(arrays["actor"], arrays["k"])
            ents = x.entities
            fights = np.zeros(len(rows), bool)
            ok = actor >= 0
            fights[ok] = (ents[actor[ok], e_civ] == 0) & ((ents[actor[ok], e_str] > 0) | (ents[actor[ok], e_rng] > 0))
            fights &= np.repeat(arrays["head"] == 0, arrays["k"])
            arrays = dict(arrays, rows=np.ascontiguousarray(rows[:, :n_in]), cand_aux=targets_of(rows, ext, names, extra_names, fights))
            x.dec_arrays = arrays
            starts = np.cumsum(arrays["k"]) - arrays["k"]
            for d, o, k in zip(x.decisions, starts, arrays["k"]):
                d.rows = arrays["rows"][o : o + k]
            return x

        shard.turn = turn
        shard.layout = layout9
    return layout9


def loss(pred: torch.Tensor, target: torch.Tensor, cand_dec: torch.Tensor, dec_k: torch.Tensor) -> torch.Tensor:
    """Σ over decisions of the mean over its candidates of the mean over the
    targets that apply (BCE on logits, squared error): one decision weighs
    as much as one label of the policy loss."""
    ok = torch.isfinite(target)
    t = target.nan_to_num()
    binary = torch.as_tensor(BINARY, device=pred.device)
    per = torch.where(binary, F.binary_cross_entropy_with_logits(pred, t.clamp(0, 1), reduction="none"), (pred - t) ** 2)
    per = (per * ok).sum(-1) / ok.sum(-1).clamp(min=1)
    has = ok.any(-1).to(per.dtype)
    return (per * has / dec_k[cand_dec].clamp(min=1).to(per.dtype)).sum()


class Metrics:
    """Held-out accuracy per target: binary — accuracy, the base rate, recall
    and precision of the positives; regression — mean absolute error in the
    target's units and that of predicting the held-out mean."""

    def __init__(self):
        self.pred, self.target = [], []

    def add(self, pred: torch.Tensor, target: torch.Tensor):
        self.pred.append(pred.detach().float().cpu().numpy())
        self.target.append(target.detach().float().cpu().numpy())

    def report(self) -> dict:
        p, t = np.concatenate(self.pred), np.concatenate(self.target)
        out = {}
        for j, (name, _, kind, div, _) in enumerate(TARGETS):
            ok = np.isfinite(t[:, j])
            if not ok.any():
                continue
            y, q = t[ok, j], p[ok, j]
            if kind == "bce":
                yb, qb = y > 0.5, q > 0
                pos = yb.sum()
                out[name] = {"n": int(ok.sum()), "positives": int(pos), "accuracy": float((yb == qb).mean()), "base": float(max(yb.mean(), 1 - yb.mean())),
                             "recall": float((qb & yb).sum() / max(pos, 1)), "precision": float((qb & yb).sum() / max(qb.sum(), 1))}
            else:
                out[name] = {"n": int(ok.sum()), "mae": float(np.abs(q - y).mean() * div), "mae_mean": float(np.abs(y.mean() - y).mean() * div)}
        return out


def evaluate(model, windows_batches, cfg, device, bptt: int = 8) -> dict:
    """The aux head's held-out metrics over batches of windows (as
    train_imitation.evaluate runs them)."""
    from .train_imitation import Prepared, prefetch, run_windows, window_turns
    from .train_imitation import HeadStats

    metrics = Metrics()
    model.eval()
    model.cand_aux_sink = metrics
    with torch.no_grad():
        for prepared in prefetch(windows_batches, lambda b: Prepared(window_turns(b), cfg, bptt)):
            run_windows(model, prepared, cfg, device, HeadStats(model.heads))
    model.cand_aux_sink = None
    return metrics.report()


def main(argv=None):
    from .data import WindowIndex, load_shards, split_by_game
    from .model import load_checkpoint

    ap = argparse.ArgumentParser(description="The candidate aux head's held-out accuracy.")
    ap.add_argument("checkpoint")
    ap.add_argument("shards", help="format-10 ground shards (a directory)")
    ap.add_argument("--val-fraction", type=float, default=0.03)
    ap.add_argument("--window", type=int, default=32)
    ap.add_argument("--device", default="cpu")
    ap.add_argument("--targets", default="", help="target groups (GROUPS); empty: all")
    a = ap.parse_args(argv)
    shards = load_shards(a.shards, in_memory=True)
    attach(shards)
    select(a.targets)
    _, val = split_by_game(shards, a.val_fraction)
    model = load_checkpoint(a.checkpoint).to(a.device)
    if not model.cfg.cand_aux:
        raise SystemExit("the net has no candidate aux head")
    rep = evaluate(model, WindowIndex(val, a.window).batches(8), model.cfg, a.device)
    print(json.dumps(rep, indent=1))


if __name__ == "__main__":
    main()
