"""The float32 damage (`Sim(fast_damage=True)`) against the exact one: speed and statistical agreement.

    python -m aurelia_nn.tacsim.fastcheck DUMP.jsonl --families duel,siege --copies 2000 --rounds 6

Both modes play the same scenes from the same start with the same random policy keys; play diverges
once a damage differs by a hit point, so the comparison is statistical, per family: the side that
wins (the other has no piece left, or a city changed hands to it), the pieces lost per side and the
hit points left. The difference of means is given with its standard error (z > 3: a real bias).
"""

import argparse
import json
import math
import time

import mlx.core as mx

from . import bench
from .scene import load_lines

I32 = mx.int32


def outcome(s, sim, n_pieces):
    U = s["u_alive"].shape[1]
    piece = mx.arange(U)[None, :] < n_pieces[:, None]
    alive = s["u_alive"] & piece
    out = {}
    for side in (0, 1):
        own = alive & (s["u_side"] == side)
        out[f"alive{side}"] = mx.sum(own.astype(I32), axis=1)
        out[f"hp{side}"] = mx.sum(mx.where(own, s["u_hp"], 0), axis=1)
    took = mx.any(sim.cs["alive"] & (s["c_owner"] != s["c_owner0"]), axis=1) if "c_owner0" in s else mx.zeros_like(out["alive0"]) > 0
    out["win0"] = ((out["alive1"] == 0) | (took & mx.any(s["c_owner"] == 0, axis=1))).astype(mx.float32)
    out["win1"] = ((out["alive0"] == 0) | (took & mx.any(s["c_owner"] == 1, axis=1))).astype(mx.float32)
    return {k: v.astype(mx.float32) for k, v in out.items()}


def stats(x):
    v = x.tolist()
    n = len(v)
    m = sum(v) / n
    var = sum((a - m) ** 2 for a in v) / max(1, n - 1)
    return m, math.sqrt(var / n)


def run(path, families, copies, rounds):
    rows = []
    for fam in families:
        dumps = load_lines(path, families=[fam])
        if not dumps:
            continue
        res = {}
        for fast in (False, True):
            sim, s0, tb, _ = bench.replicate(path, copies, dumps=dumps, fast_damage=fast)
            s0["c_owner0"] = s0["c_owner"]
            runner = bench.Runner(sim, "random", None)
            bench.play(runner, dict(s0), 1, seed=99)
            t = time.perf_counter()
            s, launches, _, secs, _ = bench.play(runner, dict(s0), rounds, seed=7)
            res[fast] = (outcome(s, sim, sim.n_pieces), secs)
        row = {"family": fam, "copies": copies, "rounds": rounds, "exact_s": round(res[False][1], 3), "fast_s": round(res[True][1], 3)}
        for k in res[False][0]:
            me, se = stats(res[False][0][k])
            mf, sf = stats(res[True][0][k])
            row[k] = {"exact": round(me, 4), "fast": round(mf, 4), "z": round((mf - me) / max(1e-9, math.hypot(se, sf)), 2)}
        rows.append(row)
        print(json.dumps(row), flush=True)
    return rows


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--families", default="duel,focus,kite,siege,ram,retreat,convoy")
    ap.add_argument("--copies", type=int, default=2000)
    ap.add_argument("--rounds", type=int, default=6)
    ap.add_argument("--out")
    o = ap.parse_args(argv)
    rows = run(o.dump, o.families.split(","), o.copies, o.rounds)
    if o.out:
        with open(o.out, "w") as f:
            json.dump(rows, f, indent=1)


if __name__ == "__main__":
    main()
