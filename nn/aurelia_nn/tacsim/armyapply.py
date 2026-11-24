"""The net's actions applied in MLX against Rust, forced: each decision from the Rust state before it.

    python -m aurelia_nn.tacsim.armyapply DUMP.jsonl --weights RUN/weights.safetensors [--families F,..]

For every decision moment of the dump whose next moment is in the same turn: the MLX state from the Rust state of the
moment, the unit's candidates in MLX, the candidate Rust applied (`applied`) applied by `Game.apply`; then every slot's
life, tile, hit points, moves, attacks and stance and every window city's damage against the Rust state of the next
moment. Reported per family and action kind: the decisions compared, equal, and the first difference's class.
"""

import argparse
import collections
import json
import time

import mlx.core as mx

from . import armyobs as A
from . import armyplay as P
from .armyloop import Brain, load_net
from .sim import g1

KINDS = {A.K_MOVE: "move", A.K_UNIT: "attack unit", A.K_CITY: "attack city", A.K_RANGED: "ranged", A.K_FORT: "fortify",
         A.K_SETUP: "set up", A.K_PROMO: "promote", A.K_DONE: "done", A.K_PILLAGE: "pillage", A.K_OTHER: "other"}


def rows_of(plays):
    out = []
    for pi, (sc, ms, _) in enumerate(plays):
        for m, m2 in zip(ms, ms[1:]):
            # The next decision of the same turn (not the turn end's memory moment: the stand and the rule come before it).
            if m.get("pick", -1) < 0 or m.get("applied", -1) < 0 or m2["round"] != m["round"] or m2["step"] >= (1 << 32) - 1:
                continue
            ref = m["tok"]["ref"][m["pick"]]
            if ref[0] != "u" or ref[1] < 0:
                continue
            row = {"state": m["state"], "ids": m.get("ids", []), "handles": m.get("handles", []), "pillageable": m.get("pillageable") or sc["orders"]["pillageable"]}
            for k in ("wm", "cstr"):
                if k in m:
                    row[k] = m[k]
            out.append((pi, row, ref[1], m["applied"], m2["state"], f"{sc['play']} r{m['round']} s{m['step']}"))
    return out


def run(path, weights, families=None, group=200, out=None, show=4):
    brain = Brain(load_net(weights))
    counts = collections.Counter()
    ex = collections.defaultdict(list)
    t0 = time.time()
    for gp in A.stream(path, families=families, group=group):
        rows = rows_of(gp)
        if not rows:
            continue
        g = P.Game(gp, 1, brain, forced=[(pi, row) for pi, row, *_ in rows])
        B = g.B
        me = mx.array([r[2] for r in rows], dtype=mx.int32)
        ai = mx.array([r[3] for r in rows], dtype=mx.int32)
        cand = g.cand_fn(g.s, me)
        n_c = cand["kind"].shape[1]
        ok_i = ai < n_c
        kind = mx.take_along_axis(cand["kind"], mx.minimum(ai, n_c - 1)[:, None], axis=1)[:, 0]
        tgt = mx.take_along_axis(cand["tgt"], mx.minimum(ai, n_c - 1)[:, None], axis=1)[:, 0]
        g.apply(me, kind, tgt, ok_i, side=0, mk=cand["_mk"])
        s = g.s
        mx.eval(s)
        al, tl, hp, mv, st, att, xp, lv = (s[k].tolist() for k in ("u_alive", "u_tile", "u_hp", "u_mv", "u_st", "u_att", "u_xp", "u_level"))
        dmg = s["c_dmg"].tolist()
        kinds = kind.tolist()
        for b, (pi, _, slot, _, after, name) in enumerate(rows):
            fam = gp[pi][0]["play"].split("/")[0]
            kk = KINDS.get(kinds[b], str(kinds[b]))
            key = (fam, kk)
            first = None
            n_pieces = len(gp[pi][0]["orders"]["roles"])
            for k, u in enumerate(after["units"][:min(g.U, n_pieces)]):
                if (u[0] == 1) != bool(al[b][k]):
                    first = "alive/dead"
                elif u[0] and u[1] != tl[b][k]:
                    first = "tile"
                elif u[0] and u[2] != hp[b][k]:
                    first = "hp"
                elif u[0] and u[3] != mv[b][k]:
                    first = "moves"
                elif u[0] and u[5] != att[b][k]:
                    first = "attacks"
                elif u[0] and u[6] != st[b][k]:
                    first = "stance"
                elif u[0] and u[7] != xp[b][k]:
                    first = "xp"
                elif u[0] and u[10] != lv[b][k]:
                    first = "level"
                if first:
                    first = ("acting " if k == slot else "other ") + first
                    detail = f" slot {k} rust {u[:11]} mlx {[al[b][k], tl[b][k], hp[b][k], mv[b][k], 0, att[b][k], st[b][k], xp[b][k], 0, 0, lv[b][k]]}"
                    break
            if first is None:
                for k, c in enumerate(after["cities"]):
                    if c[1] != dmg[b][k]:
                        first, detail = "city damage", f" city {k} rust {c[1]} mlx {dmg[b][k]}"
                        break
            counts[key + ("n",)] += 1
            if first is None:
                counts[key + ("equal",)] += 1
            else:
                counts[key + (first,)] += 1
                if len(ex[key]) < show:
                    ex[key].append(name + ": " + first + detail)
    res = {"seconds": round(time.time() - t0, 1), "lines": {}}
    for key in sorted({k[:2] for k in counts}):
        n = counts[key + ("n",)]
        res["lines"][" / ".join(key)] = {"n": n, "equal": counts[key + ("equal",)], "diffs": {k[2]: v for k, v in counts.items() if k[:2] == key and k[2] not in ("n", "equal")}, "examples": ex[key]}
    n = sum(v["n"] for v in res["lines"].values()); e = sum(v["equal"] for v in res["lines"].values())
    res["total"] = {"n": n, "equal": e}
    for k, v in res["lines"].items():
        if v["equal"] < v["n"]:
            print(k, v["n"], v["equal"], v["diffs"], v["examples"][:2])
    print("total", res["total"])
    if out:
        json.dump(res, open(out, "w"), indent=1)
    return res


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--weights", required=True)
    ap.add_argument("--families")
    ap.add_argument("--group", type=int, default=200)
    ap.add_argument("--out")
    a = ap.parse_args(argv)
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    run(a.dump, a.weights, families=a.families.split(",") if a.families else None, group=a.group, out=a.out)


if __name__ == "__main__":
    main()
