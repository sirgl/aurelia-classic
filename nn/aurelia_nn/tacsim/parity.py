"""The parity guard: replay the Rust plays of `ai-exam suite-dump` in the MLX simulator, all scenes of a
file in one batch, and compare after every move.

    python -m aurelia_nn.tacsim.parity DUMP.jsonl [--mode free|forced] [--limit N] [--show 5]

Checked at every step: the legal options of the acting unit (fortify, set up, found, every move and
attack tile; every city-strike tile), both sides' visibility of the whole window, and the state after
the step (every unit slot: alive, tile, hit points, moves left, maximum, attacks made, stance,
experience; every city: owner, damage, strike made; the combat RNG state). `free`: the MLX state runs
on its own from the scene's start (a scene matches when every step of its play matches); `forced`:
after each comparison the MLX state is set to Rust's (per-step parity). Border growth and city growth
(the economy, not the tactics) come from Rust's state at each end of turn: an exogenous schedule.
"""

import argparse
import collections
import json
import sys
import time

import mlx.core as mx

from . import exact
from .geometry import Geometry
from .scene import Batch, VERBS, load_lines
from .sim import Sim, A_PAD, A_UNIT, A_CITY, A_END, A_ROUND

UNIT_FIELDS = ["alive", "tile", "hp", "mv", "mmax", "att", "st", "xp", "side", "kind"]


def step_action(st):
    if "unit" in st:
        opts = st["options"]
        pick = st["pick"]
        verb, tgt = (opts[pick][0], opts[pick][1]) if pick < len(opts) else ("N", -1)
        return A_UNIT, st["side"], st["unit"], VERBS[verb], tgt
    if "city" in st:
        opts = st["options"]
        pick = st["pick"]
        verb, tgt = (opts[pick][0], opts[pick][1]) if pick < len(opts) else ("N", -1)
        return A_CITY, st["side"], st["city"], VERBS[verb], tgt
    if "end_turn" in st:
        return A_END, st["side"], 0, 0, -1
    return A_ROUND, 0, 0, 0, -1


def build(dumps, move_radius=None):
    batch = Batch(dumps)
    mr = move_radius or max(1, max(p["moves_land"] for d in dumps for p in d["scene"]["profiles"]) // 60)
    geo = Geometry(batch.w, radius=4, move_radius=mr)
    has_near = any(t["cond"][0] == "other" for d in dumps for p in d["scene"]["profiles"] for t in p["terms"])
    sim = Sim(geo, batch.m, batch.cost_pool, batch.cstatic, batch.n_pieces, move_radius=mr, has_near_imp=has_near)
    return batch, geo, sim


def rust_masks(st, T):
    fort = setup = found = False
    move, att = set(), set()
    for k, t in st["options"]:
        if k == "F":
            fort = True
        elif k == "U":
            setup = True
        elif k == "C":
            found = True
        elif k in ("M",):
            move.add(t)
        elif k in ("A", "S"):
            att.add(t)
    return fort, setup, found, move, att


def run(path, mode="free", limit=None, show=5, families=None, out=None):
    t0 = time.time()
    dumps = load_lines(path, limit=limit, families=families)
    batch, geo, sim = build(dumps)
    B, T, U = batch.b, batch.t, batch.u
    s = batch.initial_state()
    mx.eval(s)
    t_load = time.time() - t0
    n_steps = max(len(d["steps"]) for d in dumps)
    diverged = [None] * B
    stats = collections.defaultdict(lambda: collections.Counter())
    examples = []

    def note(b, k, what):
        key = (dumps[b]["scene"]["family"], dumps[b]["scene"]["era"])
        stats[key]["bad_" + what.split(":")[0]] += 1
        if diverged[b] is None:
            diverged[b] = (k, what)
            if len(examples) < show:
                examples.append((b, k, what))

    t1 = time.time()
    for k in range(n_steps):
        rows = [d["steps"][k] if k < len(d["steps"]) else None for d in dumps]
        acts = [step_action(r) if r is not None else (A_PAD, 0, 0, 0, -1) for r in rows]
        a = {name: mx.array([x[i] for x in acts], dtype=mx.int32) for i, name in enumerate(["typ", "side", "slot", "verb", "tgt"])}
        unit_steps = [b for b in range(B) if acts[b][0] == A_UNIT]
        city_steps = [b for b in range(B) if acts[b][0] == A_CITY]
        mk = sim.masks(s, a["side"], a["slot"])
        # Visibility and options before the step.
        check = [b for b in range(B) if rows[b] is not None and "vis_before" in rows[b] and (mode == "forced" or diverged[b] is None)]
        if check:
            allt = mx.broadcast_to(mx.arange(T)[None, :], (B, T)).astype(mx.int32)
            vis = [sim.seen_by(s, mx.full((B,), side, dtype=mx.int32), allt).tolist() for side in (0, 1)]
            for b in check:
                for side in (0, 1):
                    want = rows[b]["vis_before"][side]
                    got = "".join("1" if v else "0" for v in vis[side][b])
                    stats[(dumps[b]["scene"]["family"], dumps[b]["scene"]["era"])]["vis_checked"] += 1
                    if got != want:
                        bad = [i for i in range(T) if got[i] != want[i]][:6]
                        note(b, k, f"vis: side {side} tiles {bad} mlx {[got[i] for i in bad]}")
        if unit_steps:
            fort, setup, found, mv, att = (mk[x].tolist() for x in ("fort", "setup", "found", "move", "attack"))
            for b in unit_steps:
                if mode == "free" and diverged[b] is not None:
                    continue
                rf, ru, rc, rm, ra = rust_masks(rows[b], T)
                gm = {i for i in range(T) if mv[b][i]}
                ga = {i for i in range(T) if att[b][i]}
                key = (dumps[b]["scene"]["family"], dumps[b]["scene"]["era"])
                stats[key]["masks"] += 1
                if (rf, ru, rc) != (fort[b], setup[b], found[b]):
                    note(b, k, f"mask: F/U/C rust {(rf, ru, rc)} mlx {(fort[b], setup[b], found[b])}")
                elif gm != rm:
                    note(b, k, f"mask: moves rust-only {sorted(rm - gm)[:8]} mlx-only {sorted(gm - rm)[:8]} (unit {acts[b][2]})")
                elif ga != ra:
                    note(b, k, f"mask: attacks rust-only {sorted(ra - ga)} mlx-only {sorted(ga - ra)} (unit {acts[b][2]})")
                else:
                    stats[key]["masks_ok"] += 1
        if city_steps:
            for cs_b in sorted(set(acts[b][2] for b in city_steps)):
                pass
            cmask = sim.city_mask(s, a["side"], a["slot"]).tolist()
            for b in city_steps:
                if mode == "free" and diverged[b] is not None:
                    continue
                _, _, _, _, ra = rust_masks(rows[b], T)
                ga = {i for i in range(T) if cmask[b][i]}
                key = (dumps[b]["scene"]["family"], dumps[b]["scene"]["era"])
                stats[key]["masks"] += 1
                if ga != ra:
                    note(b, k, f"mask: city strike rust-only {sorted(ra - ga)} mlx-only {sorted(ga - ra)}")
                else:
                    stats[key]["masks_ok"] += 1
        s = sim.step(s, a, mk)
        # Exogenous economy at the end of a turn: borders and population.
        ends = [b for b in range(B) if acts[b][0] == A_END]
        if ends:
            rs = batch.state_from([rows[b]["after"] if acts[b][0] == A_END else dumps[b]["initial"] for b in range(B)])
            sel = mx.array([acts[b][0] == A_END for b in range(B)])
            s["owner"] = mx.where(sel[:, None], rs["owner"], s["owner"])
            s["c_pop"] = mx.where(sel[:, None], rs["c_pop"], s["c_pop"])
        mx.eval(s)
        # Compare the state after.
        live = [b for b in range(B) if rows[b] is not None]
        if not live:
            continue
        rs = batch.state_from([rows[b]["after"] if rows[b] is not None else dumps[b]["initial"] for b in range(B)])
        got = {f: s["u_" + f].tolist() for f in UNIT_FIELDS}
        want = {f: rs["u_" + f].tolist() for f in UNIT_FIELDS}
        gc = {f: s[f].tolist() for f in ("c_owner", "c_dmg", "c_strike")}
        wc = {f: rs[f].tolist() for f in ("c_owner", "c_dmg", "c_strike")}
        grng, wrng = exact.u64_to_ints(s["rng"]), exact.u64_to_ints(rs["rng"])
        for b in live:
            key = (dumps[b]["scene"]["family"], dumps[b]["scene"]["era"])
            if mode == "free" and diverged[b] is not None:
                continue
            stats[key]["steps"] += 1
            n_alive_slots = U
            bad = None
            for f in UNIT_FIELDS:
                gw = [got[f][b][i] if got["alive"][b][i] or f == "alive" else 0 for i in range(n_alive_slots)]
                ww = [want[f][b][i] if want["alive"][b][i] or f == "alive" else 0 for i in range(n_alive_slots)]
                if f == "tile":
                    gw = [v if got["alive"][b][i] else -1 for i, v in enumerate(gw)]
                    ww = [v if want["alive"][b][i] else -1 for i, v in enumerate(ww)]
                if gw != ww:
                    i = next(i for i in range(n_alive_slots) if gw[i] != ww[i])
                    bad = f"state: unit {i} {f} rust {ww[i]} mlx {gw[i]} after {describe(rows[b])}"
                    break
            if bad is None:
                for f in ("c_owner", "c_dmg", "c_strike"):
                    if [bool(x) if f == "c_strike" else x for x in gc[f][b][: len(dumps[b]["scene"]["cities"])]] != [bool(x) if f == "c_strike" else x for x in wc[f][b][: len(dumps[b]["scene"]["cities"])]]:
                        bad = f"state: city {f} rust {wc[f][b]} mlx {gc[f][b]} after {describe(rows[b])}"
                        break
            if bad is None and grng[b] != wrng[b]:
                bad = f"state: rng after {describe(rows[b])}"
            if bad:
                note(b, k, bad)
            else:
                stats[key]["steps_ok"] += 1
        if mode == "forced":
            keep = {k2: v for k2, v in s.items()}
            for k2 in ("u_alive", "u_tile", "u_hp", "u_mv", "u_mmax", "u_att", "u_st", "u_xp", "u_side", "u_kind", "c_owner", "c_dmg", "c_strike", "c_pop", "owner", "rng"):
                sel = mx.array([rows[b] is not None for b in range(B)])
                shape = (B,) + (1,) * (keep[k2].ndim - 1)
                keep[k2] = mx.where(sel.reshape(shape), rs[k2], keep[k2])
            s = keep
    t_run = time.time() - t1
    report = summarize(dumps, stats, diverged, examples, batch, t_load, t_run, mode)
    if out:
        with open(out, "w") as f:
            json.dump(report, f, indent=1)
    return report


def describe(st):
    if st is None:
        return "-"
    if "unit" in st:
        o = st["options"][st["pick"]] if st["pick"] < len(st["options"]) else ["N", -1]
        return f"r{st['round']} side {st['side']} unit {st['unit']} {o[0]} {o[1]}"
    if "city" in st:
        o = st["options"][st["pick"]] if st["pick"] < len(st["options"]) else ["N", -1]
        return f"r{st['round']} side {st['side']} city {st['city']} {o[0]} {o[1]}"
    if "end_turn" in st:
        return f"r{st['round']} end turn side {st['side']}"
    return f"r{st['round']} round end"


def summarize(dumps, stats, diverged, examples, batch, t_load, t_run, mode):
    fam_rows = collections.defaultdict(lambda: collections.Counter())
    for b, d in enumerate(dumps):
        key = (d["scene"]["family"], d["scene"]["era"])
        fam_rows[key]["scenes"] += 1
        if diverged[b] is None:
            fam_rows[key]["scenes_ok"] += 1
        if b in batch.unsupported:
            fam_rows[key]["unsupported"] += 1
    rows = []
    for key in sorted(set(stats) | set(fam_rows)):
        st, fr = stats[key], fam_rows[key]
        rows.append({"family": key[0], "era": key[1], "scenes": fr["scenes"], "scenes_ok": fr["scenes_ok"], "unsupported": fr["unsupported"],
                     "steps": st["steps"], "steps_ok": st["steps_ok"], "masks": st["masks"], "masks_ok": st["masks_ok"],
                     "bad": {k: v for k, v in st.items() if k.startswith("bad_")}})
    ex = [{"family": dumps[b]["scene"]["family"], "era": dumps[b]["scene"]["era"], "instance": dumps[b]["scene"]["instance"], "step": k, "what": w} for b, k, w in examples]
    causes = collections.Counter(w.split(":")[0] + ":" + w.split(":")[1].split(" rust")[0][:40] for _, w in [x for x in diverged if x])
    return {"mode": mode, "scenes": len(dumps), "scenes_ok": sum(1 for x in diverged if x is None), "rows": rows, "examples": ex,
            "causes": causes.most_common(20), "load_s": round(t_load, 2), "run_s": round(t_run, 2), "unsupported": {str(k): v for k, v in batch.unsupported.items()}}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--mode", default="free")
    ap.add_argument("--limit", type=int)
    ap.add_argument("--show", type=int, default=8)
    ap.add_argument("--families")
    ap.add_argument("--out")
    o = ap.parse_args(argv)
    rep = run(o.dump, o.mode, o.limit, o.show, o.families.split(",") if o.families else None, o.out)
    print(f"mode {rep['mode']}: {rep['scenes_ok']}/{rep['scenes']} scenes match every step; load {rep['load_s']} s, replay {rep['run_s']} s")
    for r in rep["rows"]:
        print(f"  {r['family']:8} {r['era']:12} scenes {r['scenes_ok']}/{r['scenes']} steps {r['steps_ok']}/{r['steps']} masks {r['masks_ok']}/{r['masks']} {r['bad'] or ''}")
    print("causes:", rep["causes"])
    for e in rep["examples"]:
        print("  ex:", e)
    if rep["unsupported"]:
        print("unsupported profiles in scenes:", list(rep["unsupported"].items())[:5])


if __name__ == "__main__":
    main()
