"""The twin's log-probabilities of the Rust actor's decisions: a forced replay of an ARMY_OBS_DUMP of sampled plays (armyloop.replay with the
pick and the applied action of the dump scored instead of the greedy ones). Output: {play: [(round, step, pick, lp_pick, lp_action)]}."""
import collections, json, sys, time
import mlx.core as mx
from aurelia_nn.tacsim import armyobs as A
from aurelia_nn.tacsim import armyhist as H
from aurelia_nn.tacsim.armyloop import Brain, load_net, greedy_pointer, TURN_END, PER_UNIT, compare_notes

def replay_logp(path, weights, group=200, limit=None, families=None, record=None):
    t0 = time.time()
    brain = Brain(load_net(weights))
    if False:
        brain.use_rust_memory()
    st = collections.Counter()
    fam = collections.defaultdict(collections.Counter)
    examples = []
    examples_notes = []
    t_net = t_obs = 0.0
    for plays in A.stream(path, limit=limit, families=families, group=group):
        templates = A.build_templates(plays)
        P = len(plays)
        tb = templates[0]
        U = tb.u
        um, pm, alive = brain.m.initial(P)
        ns = brain.initial_notes(P) if brain.notes else None
        ring = mx.broadcast_to(pm[:, 0][:, None, :], (P, H.H_TURNS, pm.shape[-1])) if brain.hist else None
        M = None
        statics = None
        if brain.has_map:
            _, sim0, s0, tab0 = A.build_rows(plays, [(pi, {"state": sc["initial"], "ids": [], "handles": []}) for pi, (sc, _, _) in enumerate(plays)], templates)
            statics = A.Obs(sim0, tab0).statics(s0)
            M = mx.zeros((P, 576, brain.m.config["map_ch"]))
        price = mx.array([sc["orders"]["price"] for sc, _, _ in plays], dtype=mx.float32)
        if False:
            # The probe of package 1: the same states under another price form (`armyprice.price_for`).
            from . import armyprice
            price = armyprice.price_for(price_form, plays)
        made = [collections.Counter() for _ in range(P)]
        diverged = [False] * P
        k = 0
        longest = max(len(ms) for _, ms, _ in plays)
        while k < longest:
            rows = [(pi, ms[k]) for pi, (_, ms, _) in enumerate(plays) if k < len(ms)]
            k += 1
            ta = time.time()
            _, sim, s, tab = A.build_rows(plays, rows, templates)
            inp, skip = A.moment_inputs(plays, rows, s, tab)
            obs = A.Obs(sim, tab)
            tok = obs.tokens(s, **inp)
            mx.eval(tok)
            t_obs += time.time() - ta
            pis = mx.array([pi for pi, _ in rows], dtype=mx.int32)
            tn = time.time()
            ends = [m["step"] == TURN_END for _, m in rows]
            firsts = [m["step"] == 0 for _, m in rows]
            for r, (pi, m) in enumerate(rows):
                if firsts[r]:
                    made[pi] = collections.Counter()
            um_r, pm_r, al_r = um[pis], pm[pis], alive[pis]
            if brain.has_map and any(firsts):
                Mr_new = brain.encode(statics[pis], tok)
                fr = mx.array(firsts)
                M = M.at[pis].add(mx.where(fr[:, None, None], Mr_new - M[pis], 0.0))
            Mr = M[pis] if brain.has_map else None
            x = None
            if brain.notes:
                ns_r = {kk: v[pis] for kk, v in ns.items()}
                x = {"ns": ns_r}
            if brain.hist:
                # MBH reads Rust's history tokens of the moment (the dump's), with the twin's plan of each fact's turn.
                hx = H.dump_tokens([m for _, m in rows])
                hx["hplan"] = H.plan_of(hx, pm_r, ring[pis])
                x = {"hist": hx}
            h = brain.body(tok, um_r, pm_r, Mr, price[pis], x)
            if brain.notes:
                compare_notes(st, examples_notes, plays, rows, ns_r, brain.m.NOTE_STATE)
            # Turn ends: the memory step.
            if any(ends):
                um2, pm2, al2 = brain.step(h, tok, um_r, pm_r, al_r)
                e = mx.array(ends)
                if brain.notes:
                    ns2 = brain.write_notes(h, tok, ns_r, al_r, um2, pm2, al2, e)
                    ns = {kk: v.at[pis].add(mx.where(e.reshape((-1,) + (1,) * (v.ndim - 1)), ns2[kk] - ns_r[kk], 0.0)) for kk, v in ns.items()}
                if brain.hist:
                    new = mx.concatenate([pm_r[:, 0][:, None, :], ring[pis][:, :H.H_TURNS - 1]], axis=1)
                    ring = ring.at[pis].add(mx.where(e[:, None, None], new - ring[pis], 0.0))
                um = um.at[pis].add(mx.where(e[:, None, None], um2 - um_r, 0.0))
                pm = pm.at[pis].add(mx.where(e[:, None, None], pm2 - pm_r, 0.0))
                alive = alive.at[pis].add(mx.where(e[:, None], al2 - al_r, 0.0))
            # Decisions: the pointer over the selectable units not stopped by the limit.
            unit = tok["unit"].tolist()
            stopped = [[made[pi][u] >= PER_UNIT if u >= 0 else False for u in unit[r]] for r, (pi, _) in enumerate(rows)]
            sel = tok["selectable"] & ~mx.array(stopped)
            T = sel.shape[1]
            lg = brain.pointer(h, sel)
            pick = mx.array([m.get("pick", -2) if not e else -2 for e, (_, m) in zip(ends, rows)], dtype=mx.int32)
            lse = mx.logsumexp(lg, axis=1)
            lp_all = (mx.take_along_axis(lg, mx.where(pick >= 0, pick, T)[:, None], axis=1)[:, 0] - lse).tolist()
            mx.eval(pick, um, pm, alive)
            picks = pick.tolist()
            want = [(r, pi, m) for r, (pi, m) in enumerate(rows) if not ends[r]]
            cand_rows = [(r, pi, m) for r, pi, m in want if picks[r] >= 0]
            acts = {}
            if cand_rows:
                bi = mx.array([r for r, _, _ in cand_rows], dtype=mx.int32)
                sub = {kk: mx.take(v, bi, axis=0) for kk, v in s.items()}
                cobs = A.Obs(A._rows_sim(sim, bi), A._rows_tab(tab, bi))
                me = mx.array([unit[r][picks[r]] for r, _, _ in cand_rows], dtype=mx.int32)
                cand = cobs.candidates(sub, me)
                hl = h[bi]
                al = brain.actions(hl, mx.array([picks[r] for r, _, _ in cand_rows], dtype=mx.int32), cand, M[pis][bi] if brain.has_map else None)
                lse_a = mx.logsumexp(al, axis=1)
                app = mx.array([max(rows[r][1].get("applied", 0), 0) for r, _, _ in cand_rows], dtype=mx.int32)
                lpa = (mx.take_along_axis(al, app[:, None], axis=1)[:, 0] - lse_a).tolist()
                for j, (r, _, _) in enumerate(cand_rows):
                    acts[r] = lpa[j]
            t_net += time.time() - tn
            for r, pi, m in want:
                f = plays[pi][0]["play"].split("/")[0]
                rc = m["cands"].get(str(m.get("pick", -1)))
                if not skip[r] and rc and any(a[1] < 0 for a in rc["act"]):
                    skip[r] = "the unit's moves reach off the window"
                if skip[r]:
                    st["decisions outside the twin: " + skip[r]] += 1
                    if picks[r] >= 0 and unit[r][picks[r]] >= 0:
                        made[pi][unit[r][picks[r]]] += 1
                    continue
                st["decisions"] += 1
                fam[f]["decisions"] += 1
                rp, ra = m.get("pick", -2), m.get("applied", -2)
                mp = picks[r]
                if record is not None:
                    record.setdefault(plays[pi][0]["play"], []).append((m["round"] - plays[pi][0]["orders"]["start_round"], m["step"], mp, lp_all[r], acts.get(r)))
                ok_p = mp == rp
                ok = ok_p and (mp < 0 or acts.get(r) == ra)
                if ok:
                    st["equal"] += 1
                    fam[f]["equal"] += 1
                else:
                    st["pointer differs" if not ok_p else "action differs"] += 1
                    fam[f]["differ"] += 1
                    if not diverged[pi]:
                        diverged[pi] = True
                        st["plays with a differing decision"] += 1
                    if len(examples) < 8:
                        examples.append(f"{plays[pi][0]['play']} r{m['round']} s{m['step']}: MLX pick {mp} act {acts.get(r)} vs Rust {rp} {ra}")
                if mp >= 0 and unit[r][mp] >= 0:
                    made[pi][unit[r][mp]] += 1
            st["memory steps"] += sum(ends)
        st["plays"] += P
    res = {"stats": dict(st), "dump fields missing": A.missing_report(), "families": {f: dict(c) for f, c in sorted(fam.items())}, "examples": examples, "examples_notes": examples_notes,
           "seconds": {"observe": round(t_obs, 1), "net": round(t_net, 1), "all": round(time.time() - t0, 1)}}
    print(json.dumps(res, indent=1))
    if False:
        with open(out, "w") as fh:
            json.dump(res, fh, indent=1)
    return res




if __name__ == "__main__":
    rec = {}
    replay_logp(sys.argv[1], sys.argv[2], record=rec)
    json.dump(rec, open(sys.argv[3], "w"))
    print("plays", len(rec), "decisions", sum(len(v) for v in rec.values()))
