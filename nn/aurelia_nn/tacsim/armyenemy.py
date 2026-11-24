"""The enemy's turn and the end of ours in MLX against Rust, forced: each turn starts from the Rust state.

    python -m aurelia_nn.tacsim.armyenemy DUMP.jsonl --weights RUN/weights.safetensors [--families F,..] [--rule NAME]

The dump carries the events (`ARMY_OBS_EVENTS`): every command of both seats with what it changed, and each turn's
end. From them the state of every play is rebuilt event by event. Two forced checks, each over all plays' turns in one
batch:

- the enemy's turn: from the Rust state after our turn's end, `Game.enemy_turn` (the enemy's commands and the end of
  its turn) against the Rust state after the enemy's turn's end;
- the end of our turn: from the Rust state after the net's last command, the seat's rule for our army units (the
  fortify that the seat's AI mind gives them after the net) against the Rust state before our turn's end.

Compared: every slot's life, tile and hit points, every window city's damage (and, for our turn's end, the stance).
Reported per family: the turns compared, the turns equal, and the first difference's class.
"""

import argparse
import collections
import json
import time

import mlx.core as mx

from . import armyobs as A
from . import armyplay as P
from .armyloop import Brain, load_net


def rebuild(sc):
    """The play's events with the units and cities after each (lists of records)."""
    units = [list(u) for u in sc["initial"]["units"]]
    cities = [list(c) for c in sc["initial"]["cities"]]
    out = []
    for e in sc.get("events", []):
        for k, u in e["u"]:
            while len(units) <= k:
                units.append([0, -1, 0, 0, 0, 0, 0, 0, -1, -1, 0, "", 0])
            units[k] = list(u)
        for k, c in e["c"]:
            cities[k] = list(c)
        out.append((e, [list(u) for u in units], [list(c) for c in cities]))
    return out


def turn_rows(plays, which):
    """Forced rows: (play index, row before, record after, round). `which`: "enemy" (after our end -> after the
    enemy's end) or "ours" (after the net's last command -> before our end)."""
    rows = []
    for pi, (sc, ms, _) in enumerate(plays):
        by_round = {}
        for m in ms:
            by_round.setdefault(m["round"], m)
        st = rebuild(sc)
        U = None
        for i, (e, units, cities) in enumerate(st):
            if which in ("enemy", "third") and e["side"] == 0 and e["src"] == "end":
                start = (units, cities, e["rng"], e["round"])
                # The enemy's (or, `third`, the third seat's) turn end that follows.
                for e2, u2, c2 in st[i + 1:]:
                    if e2["side"] == (1 if which == "enemy" else -1) and e2["src"] == "end":
                        rows.append((pi, start, (u2, c2), by_round))
                        break
            if which == "ours" and e["side"] == 0 and e["src"] == "end":
                # The last event of the net (src act) before this end, else the turn's start.
                j = i - 1
                while j >= 0 and ((st[j][0]["side"] == 0 and st[j][0]["src"] in ("mind", "rule")) or st[j][0]["src"] == "arena"):
                    j -= 1
                if j >= 0 and st[j][0]["side"] == 0 and st[j][0]["src"] == "act":
                    pe, pu, pc = st[j]
                    # The turn's start: the state before its first command of the net.
                    j0 = j
                    while j0 > 0 and st[j0 - 1][0]["side"] == 0 and st[j0 - 1][0]["src"] == "act":
                        j0 -= 1
                    t0 = [u[1] for u in (st[j0 - 1][1] if j0 > 0 else sc["initial"]["units"])]
                    rows.append((pi, (pu, pc, pe["rng"], pe["round"], t0), (st[i - 1][1], st[i - 1][2]), by_round))
    return rows


def as_row(plays, pi, start, by_round, U):
    units, cities, rng, rnd = start[:4]
    m = by_round.get(rnd) or min(by_round.values(), key=lambda x: abs(x["round"] - rnd))
    state = {"units": [u for u in units[:U]], "cities": cities, "rng": rng, "owner": m["state"]["owner"]}
    row = {"state": state, "ids": m.get("ids", []), "handles": m.get("handles", []), "pillageable": m.get("pillageable") or plays[pi][0]["orders"]["pillageable"]}
    for k in ("wm", "cstr"):
        if k in m:
            row[k] = m[k]
    # The ruins and the arrival order at the end of our turn: the round's last moment (the turn end), not its first.
    last = max((x for x in plays[pi][1] if x["round"] == rnd), key=lambda x: x["step"], default=m)
    for k in ("ruins", "upos"):
        if k in last:
            row[k] = last[k]
    row["round"] = rnd
    return row


def compare(g, rows, which, counts, show, ex):
    s = g.s
    al, tl, hp, st = s["u_alive"].tolist(), s["u_tile"].tolist(), s["u_hp"].tolist(), s["u_st"].tolist()
    xp, lv = s["u_xp"].tolist(), s["u_level"].tolist()
    dmg = s["c_dmg"].tolist()
    side = s["u_side"].tolist()
    for b, (pi, start, after, _) in enumerate(rows):
        fam = g.fam[b]
        u2, c2 = after
        first = None
        # The scene's pieces only: a unit a home city trains in the window lives until the round's arena close.
        n_pieces = len(g.plays[pi][0]["orders"]["roles"]) + len(g.plays[pi][0]["scene"].get("reinforce", []))
        if g.plays[pi][0]["scene"].get("n_main") is not None:
            n_pieces = len(g.plays[pi][0]["scene"]["profiles"])  # the third seat's layout: pieces, spare slots, the third's pieces
        for k, u in enumerate(u2[:min(g.U, n_pieces)]):
            who = "enemy" if (u[8] if u[0] else side[b][k]) == 1 else "our"
            what = None
            if (u[0] == 1) != bool(al[b][k]):
                what = f"{who} unit alive/dead"
            elif u[0] and u[1] != tl[b][k]:
                what = f"{who} unit's tile"
            elif u[0] and u[2] != hp[b][k]:
                what = f"{who} unit's hp"
            elif u[0] and which == "ours" and (u[6] in (1, 2)) != (st[b][k] in (1, 2)):
                what = f"{who} unit's fortify"
            elif u[0] and g.scene_rule and u[6] != st[b][k]:
                what = f"{who} unit's stance"
            elif u[0] and g.scene_rule and u[7] != xp[b][k]:
                what = f"{who} unit's xp"
            elif u[0] and g.scene_rule and u[10] != lv[b][k]:
                what = f"{who} unit's level"
            if what and first is None:
                first = what
                detail = f" slot {k} rust {u[:11]} mlx {[int(al[b][k]), tl[b][k], hp[b][k], st[b][k], xp[b][k], lv[b][k]]}"
        if first is None and any(c[1] != dmg[b][k] for k, c in enumerate(c2)):
            first = "city damage"
        counts[(fam, "turns")] += 1
        if first is None:
            counts[(fam, "equal")] += 1
        else:
            counts[(fam, first)] += 1
            if len(ex[fam]) < show:
                ex[fam].append(f"{g.plays[pi][0]['play']} r{start[3]}: {first}" + (detail if first != "city damage" else ""))


def run(path, weights, families=None, which="enemy", show=3, out=None, group=400, rule="std", charge=False, debug=None, scene_rule=True):
    brain = Brain(load_net(weights))
    counts = collections.Counter()
    ex = collections.defaultdict(list)
    t0 = time.time()
    for gp in A.stream(path, families=families, group=group):
        rows = turn_rows(gp, which)
        if not rows:
            continue
        tb = A.build_templates(gp)[0]
        U = tb.u
        forced = [(pi, as_row(gp, pi, start, br, U)) for pi, start, _, br in rows]
        g = P.Game(gp, 1, brain, forced=forced)
        g.enemy_rule = rule
        g.charge = charge
        g.scene_rule = scene_rule
        g.rel_now = mx.array([st[3] - gp[pi][0]["orders"]["start_round"] for pi, st, _, _ in rows], dtype=mx.int32)
        if which == "third":
            from . import armycamp
            live = ~g.s["done"] & (g.verdict < 0)
            armycamp.neutral_turn(g, live)
            armycamp.neutral_rule(g, live)
        elif which == "enemy":
            if debug:
                from . import armystd
                sh, tg, C = armystd.plan_shots(g)
                sh, tg = sh.tolist(), tg.tolist()
                for b, (pi, st, _, _) in enumerate(rows):
                    name = gp[pi][0]["play"]
                    if f"{name}:{st[3]}" in debug:
                        evs = [e for e in gp[pi][0]["events"] if e["round"] in (st[3], st[3] + 1) and e["side"] == 1 and e["src"] == "mind" and e["cmd"] in ("CityStrike", "RangedAttack", "AttackUnit", "MoveUnit", "Fortify")]
                        print("DEBUG", name, st[3], "MLX plan", [(x - C if x >= C else f"c{x}", t) for x, t in zip(sh[b], tg[b]) if x >= 0])
                        print("DEBUG   Rust", [(e["cmd"], e["who"], e["tgt"], [(k, u[2]) for k, u in e["u"] if u[8] == 0]) for e in evs])
                        print("DEBUG   units", [(k, u[1], u[2], u[8]) for k, u in enumerate(st[0]) if u[0]])
            g.enemy_turn()
        else:
            U = g.U
            g.turn_tile = mx.array([(st[4] + [-1] * U)[:U] for _, st, _, _ in rows], dtype=mx.int32)
            g.our_turn_end(mx.ones((g.B,), dtype=mx.bool_))
        mx.eval(g.s)
        compare(g, rows, which, counts, show, ex)
    res = {"check": which, "rule": rule, "charge": charge, "scene_rule": scene_rule, "seconds": round(time.time() - t0, 1), "families": {}}
    fams = sorted({f for f, _ in counts})
    for f in fams:
        n = counts[(f, "turns")]
        res["families"][f] = {"turns": n, "equal": counts[(f, "equal")], "share": round(counts[(f, "equal")] / max(n, 1), 3),
                              "first differences": {k: v for (ff, k), v in sorted(counts.items(), key=lambda x: -x[1]) if ff == f and k not in ("turns", "equal")},
                              "examples": ex[f]}
    n = sum(counts[(f, "turns")] for f in fams)
    e = sum(counts[(f, "equal")] for f in fams)
    res["total"] = {"turns": n, "equal": e, "share": round(e / max(n, 1), 3)}
    print(json.dumps(res, indent=1))
    if out:
        json.dump(res, open(out, "w"), indent=1)
    return res


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--weights", required=True)
    ap.add_argument("--families")
    ap.add_argument("--which", default="enemy", choices=["enemy", "ours", "third"])
    ap.add_argument("--group", type=int, default=400)
    ap.add_argument("--out")
    ap.add_argument("--rule", default="std", choices=["std", "approx"])
    ap.add_argument("--charge", action="store_true")
    ap.add_argument("--scene-rule", dest="scene_rule", action="store_true", default=True)
    ap.add_argument("--standard", dest="scene_rule", action="store_false", help="Rust ARMY_SCENE_RULE=standard")
    ap.add_argument("--debug", help="PLAY:ROUND,... print the MLX shot plan and the Rust commands of those turns")
    a = ap.parse_args(argv)
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    run(a.dump, a.weights, families=a.families.split(",") if a.families else None, which=a.which, out=a.out, group=a.group, rule=a.rule, charge=a.charge, debug=a.debug.split(',') if a.debug else None, scene_rule=a.scene_rule)


if __name__ == "__main__":
    main()
