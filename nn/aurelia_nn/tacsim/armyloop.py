"""The army net in the loop on the GPU (MLX): its decisions on the twin's observations (`armyobs.py`), batched over
plays, against the Rust player's decisions on the same moments.

    python -m aurelia_nn.tacsim.armyloop DUMP.jsonl --weights RUN/weights.safetensors [--group 200]

The dump (`ARMY_OBS_DUMP`, the player `army-net` with the same weights) holds every decision moment of the Rust net
with the state it saw and the pick it made (the token, the candidate applied), and the memory moments at our turns'
ends. Here the MLX net plays each play's moments in order, all plays of a group in lockstep: at a turn's first moment
the map (variant MB) is encoded from the twin's tokens and the twin's static planes; at every decision the body (with
the memory of the turn's start), the pointer (the selectable units, a unit stopped after 6 decisions of a turn), the
greedy pick, the unit's candidates from the twin and the greedy action; at a turn's end the memory step on the twin's
tokens. The states are Rust's (forced replay): what is compared is the decision, given the same state.
"""

import argparse
import collections
import json
import time

import mlx.core as mx

from . import armyobs as A

TURN_END = (1 << 32) - 1
PER_UNIT = 6


def load_net(path):
    from aurelia_nn.army.model_m import MemNet
    from aurelia_nn.army.model_mb import MBANet, MBNet
    w = mx.load(path)
    if "m_in.weight" in w:
        cls = MBANet if "w_price" in w else MBNet
        model = cls(promos=w["promo"].shape[0] - 1, map_ch=w["m_in.weight"].shape[0])
    else:
        model = MemNet(promos=w["promo"].shape[0] - 1)
    model.load_weights(path)
    model.eval()
    return model


class Brain:
    """The net's forward on twin observations, batched (memory per row passed in)."""

    def __init__(self, model):
        from aurelia_nn.army.model_mb import MBANet, MBNet
        self.m = model
        self.has_map = isinstance(model, MBNet)
        self.price = isinstance(model, MBANet)

    def body(self, tok, um, pm, M, price):
        m = self.m
        if self.has_map:
            pos = tok["pos"]
            gtok = M[mx.arange(M.shape[0])[:, None], mx.maximum(pos, 0)]
            mmean = mx.mean(M, axis=1)
            if self.price:
                return m.body_mba(tok["feat"], tok["dist"], tok["mask"], tok["slot"], um, pm, gtok, pos >= 0, mmean, price)[0]
            return m.body_mbg(tok["feat"], tok["dist"], tok["mask"], tok["slot"], um, pm, gtok, pos >= 0, mmean)
        return m.body_m(tok["feat"], tok["dist"], tok["mask"], tok["slot"], um, pm)

    def encode(self, statics, tok):
        return self.m.encode_turn(statics, tok["feat"], tok["pos"])

    def pointer(self, h, sel):
        T = sel.shape[1]
        return self.m.pointer_logits(h[:, :T], sel)

    def actions(self, h, unit_tok, cand, M):
        m = self.m
        T = cand["feat"].shape[1]
        if self.has_map:
            cpos = cand["pos"]
            gc = M[mx.arange(M.shape[0])[:, None], mx.maximum(cpos, 0)]
            return m.action_logits_g(h, unit_tok, cand["feat"], cand["promo"], cand["mask"], gc, cpos >= 0)
        return m.action_logits(h, unit_tok, cand["feat"], cand["promo"], cand["mask"])

    def step(self, h, tok, um, pm, alive):
        return self.m.step(h, tok["feat"], tok["slot"], tok["mask"], um, pm, alive)[:3]


def greedy_pointer(logits, T):
    """The first highest selectable logit, the end when its logit is at least that (seat.rs)."""
    p = logits[:, :T]
    best = mx.max(p, axis=1)
    first = A.first_index_eq(p, best)
    end = logits[:, T] >= best
    return mx.where(end | (best <= -1e8), -1, first)


def replay(path, weights, group=200, limit=None, families=None, show=8, out=None):
    t0 = time.time()
    brain = Brain(load_net(weights))
    st = collections.Counter()
    fam = collections.defaultdict(collections.Counter)
    examples = []
    t_net = t_obs = 0.0
    for plays in A.stream(path, limit=limit, families=families, group=group):
        templates = A.build_templates(plays)
        P = len(plays)
        tb = templates[0]
        U = tb.u
        um, pm, alive = brain.m.initial(P)
        M = None
        statics = None
        if brain.has_map:
            _, sim0, s0, tab0 = A.build_rows(plays, [(pi, {"state": sc["initial"], "ids": [], "handles": []}) for pi, (sc, _, _) in enumerate(plays)], templates)
            statics = A.Obs(sim0, tab0).statics(s0)
            M = mx.zeros((P, 576, brain.m.config["map_ch"]))
        price = mx.array([sc["orders"]["price"] for sc, _, _ in plays], dtype=mx.float32)
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
            h = brain.body(tok, um_r, pm_r, Mr, price[pis])
            # Turn ends: the memory step.
            if any(ends):
                um2, pm2, al2 = brain.step(h, tok, um_r, pm_r, al_r)
                e = mx.array(ends)
                um = um.at[pis].add(mx.where(e[:, None, None], um2 - um_r, 0.0))
                pm = pm.at[pis].add(mx.where(e[:, None, None], pm2 - pm_r, 0.0))
                alive = alive.at[pis].add(mx.where(e[:, None], al2 - al_r, 0.0))
            # Decisions: the pointer over the selectable units not stopped by the limit.
            unit = tok["unit"].tolist()
            stopped = [[made[pi][u] >= PER_UNIT if u >= 0 else False for u in unit[r]] for r, (pi, _) in enumerate(rows)]
            sel = tok["selectable"] & ~mx.array(stopped)
            T = sel.shape[1]
            lg = brain.pointer(h, sel)
            pick = greedy_pointer(lg, T)
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
                best = mx.max(al, axis=1)
                ai = A.first_index_eq(al, best)
                ail = ai.tolist()
                for j, (r, _, _) in enumerate(cand_rows):
                    acts[r] = ail[j]
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
                    if len(examples) < show:
                        examples.append(f"{plays[pi][0]['play']} r{m['round']} s{m['step']}: MLX pick {mp} act {acts.get(r)} vs Rust {rp} {ra}")
                if mp >= 0 and unit[r][mp] >= 0:
                    made[pi][unit[r][mp]] += 1
            st["memory steps"] += sum(ends)
        st["plays"] += P
    res = {"stats": dict(st), "families": {f: dict(c) for f, c in sorted(fam.items())}, "examples": examples,
           "seconds": {"observe": round(t_obs, 1), "net": round(t_net, 1), "all": round(time.time() - t0, 1)}}
    print(json.dumps(res, indent=1))
    if out:
        with open(out, "w") as fh:
            json.dump(res, fh, indent=1)
    return res


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--weights", required=True)
    ap.add_argument("--group", type=int, default=200)
    ap.add_argument("--limit", type=int)
    ap.add_argument("--families")
    ap.add_argument("--show", type=int, default=8)
    ap.add_argument("--out")
    ap.add_argument("--cpu", action="store_true")
    a = ap.parse_args(argv)
    if a.cpu:
        mx.set_default_device(mx.cpu)
    mx.set_memory_limit(12 << 30)
    mx.set_cache_limit(2 << 30)
    replay(a.dump, a.weights, group=a.group, limit=a.limit, families=a.families.split(",") if a.families else None, show=a.show, out=a.out)


if __name__ == "__main__":
    main()
