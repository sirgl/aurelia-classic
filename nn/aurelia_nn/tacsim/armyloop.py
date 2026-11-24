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
from . import armyhist as H

TURN_END = (1 << 32) - 1
PER_UNIT = 6


def load_net(path):
    from aurelia_nn.army.model_m import MemNet
    from aurelia_nn.army.model_mb import MBANet, MBHNet, MBNet, MBNNet
    w = mx.load(path)
    if "m_in.weight" in w:
        # MBH (the history; an MBA net) and MBN (the notes; an MB net) by their own parameters.
        cls = MBHNet if "hx_slope" in w else (MBNNet if "nx_slope" in w else (MBANet if "w_price" in w else MBNet))
        model = cls(promos=w["promo"].shape[0] - 1, map_ch=w["m_in.weight"].shape[0])
    else:
        model = MemNet(promos=w["promo"].shape[0] - 1)
    model.load_weights(path)
    model.eval()
    return model


class Brain:
    """The net's forward on twin observations, batched (memory per row passed in)."""

    def __init__(self, model):
        from aurelia_nn.army.model_mb import MBANet, MBHNet, MBNet, MBNNet
        self.m = model
        self.has_map = isinstance(model, MBNet)
        self.price = isinstance(model, MBANet)
        # MBN: the notes the net wrote at its last turn's end (`x["ns"]`); MBH: the history's tokens (`x["hist"]`).
        self.notes = isinstance(model, MBNNet)
        self.hist = isinstance(model, MBHNet)
        # The memory's step in Rust's arithmetic order on the CPU (`armymem`; `use_rust_memory`), None: MLX's own.
        self.rust_memory = None

    def use_rust_memory(self):
        from .armymem import RustMemory
        self.rust_memory = RustMemory(self.m)

    def body(self, tok, um, pm, M, price, x=None):
        """H. `x`: the streams of MBN / MBH ({"ns": the note state (MBNNet.NOTE_STATE keys), "hist": `armyhist.tokens`'
        dict with the plan of each fact's turn}); a net without them ignores it."""
        m = self.m
        if self.has_map:
            pos = tok["pos"]
            gtok = M[mx.arange(M.shape[0])[:, None], mx.maximum(pos, 0)]
            mmean = mx.mean(M, axis=1)
            if self.hist:
                hx = x["hist"]
                return m.body_mbh(tok["feat"], tok["dist"], tok["mask"], tok["slot"], um, pm, gtok, pos >= 0, mmean, price,
                                  hx["hist"], hx["hmask"], hx["hslot"], hx["hage"], hx["hplan"])[0]
            if self.notes:
                off = mx.zeros((pos.shape[0],), dtype=mx.bool_)
                return m.body_mbn(tok["feat"], tok["dist"], tok["mask"], tok["slot"], um, pm, gtok, pos >= 0, mmean, x["ns"], off)
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
        if self.rust_memory is not None:
            return self.rust_memory.step(h, tok["feat"], tok["slot"], tok["mask"], um, pm, alive)
        return self.m.step(h, tok["feat"], tok["slot"], tok["mask"], um, pm, alive)[:3]

    def initial_notes(self, B):
        return self.m.initial_notes(B)

    def write_notes(self, h, tok, ns, alive, um2, pm2, al2, valid):
        """MBN at our turn's end, after the memory's step (`train_m.unroll_one_n`'s own path; Rust `write_notes_x`): the
        heads on the turn-end body `h` and the new memory, the notes written into the stream `ns` where `valid` [B]."""
        from aurelia_nn.army.model_m import P_WAITED, SLOTS, STAGES
        from aurelia_nn.army.model_mn import REASONS
        m = self.m
        event = mx.minimum(mx.sum(alive * (1.0 - al2), axis=-1), 1.0)
        stage_lg, eng_lg, rest_lg = m.note_heads(h, tok["feat"], tok["slot"], tok["mask"], um2, pm2, ns)
        stage_i = A.first_index_eq(stage_lg, mx.max(stage_lg, axis=1))
        reason_lg = m.reason_logits(ns, stage_i, event)
        reason_i = A.first_index_eq(reason_lg, mx.max(reason_lg, axis=1))
        slot = tok["slot"]
        has = (slot >= 0) & (slot < SLOTS) & tok["mask"]
        onehot = ((mx.maximum(slot, 0)[..., None] == mx.arange(SLOTS)[None, None, :]) & has[..., None]).astype(mx.float32)
        rest_s = mx.minimum(mx.einsum("bt,bts->bs", (rest_lg > 0).astype(mx.float32), onehot), 1.0)
        stage_oh = (stage_i[:, None] == mx.arange(STAGES)[None]).astype(mx.float32)
        reason_oh = (reason_i[:, None] == mx.arange(len(REASONS))[None]).astype(mx.float32)
        waited = mx.maximum(pm2[:, 0, P_WAITED] * 10.0, 0.0)
        free = mx.tanh(m.free(pm2[:, 0])) if self.rust_memory is None else self.rust_memory.free_note(pm2[:, 0])
        engaged = (eng_lg > 0).astype(mx.float32)
        return m.write_notes(ns, stage_oh, engaged, waited, reason_oh, rest_s, free, valid)


NOTE_TOL = (("stage", 0), ("stage_age", 0), ("engaged", 0), ("change_age", 0), ("reason", 0), ("rest", 0), ("rest_age", 0), ("waited", 1e-4), ("free", 1e-4))


def compare_notes(st, examples, plays, rows, ns_r, keys):
    """Gate G7 line 2a's measure on Rust's states: the note stream the twin's memory wrote (along Rust's states) against
    the dump's stream at each moment that has one; the largest float difference per moment counted by size."""
    got = {k: ns_r[k].tolist() for k in keys}
    for r, (pi, m) in enumerate(rows):
        rn = m.get("notes")
        if rn is None:
            continue
        st["notes compared"] += 1
        ok = True
        worst = 0.0
        for k, tol in NOTE_TOL:
            a, b = got[k][r], rn[k]
            a = a if isinstance(a, list) else [a]
            b = b if isinstance(b, list) else [b]
            if len(a) != len(b):
                ok = False
                st[f"notes differ in {k} (length)"] += 1
                continue
            dif = max((abs(x - y) for x, y in zip(a, b)), default=0.0)
            if tol > 0:
                worst = max(worst, dif)
                st["notes: largest difference x 1e6 in " + k] = max(st["notes: largest difference x 1e6 in " + k], int(1e6 * dif))
            if dif > tol + 1e-6:
                ok = False
                st[f"notes differ in {k}"] += 1
                if len(examples) < 40:
                    examples.append(f"{plays[pi][0]['play']} r{m['round']} s{m['step']}: {k} mlx {[round(v, 5) for v in a]} rust {[round(v, 5) for v in b]}")
        st["notes equal" if ok else "notes differ"] += 1
        for lim in (1e-6, 1e-5, 1e-4, 1e-3):
            if worst > lim:
                st[f"notes: float difference > {lim:g}"] += 1


def greedy_pointer(logits, T):
    """The first highest selectable logit, the end when its logit is at least that (seat.rs)."""
    p = logits[:, :T]
    best = mx.max(p, axis=1)
    first = A.first_index_eq(p, best)
    end = logits[:, T] >= best
    return mx.where(end | (best <= -1e8), -1, first)


def replay(path, weights, group=200, limit=None, families=None, show=8, out=None, rust_memory=False, price_form=None, record=None):
    t0 = time.time()
    brain = Brain(load_net(weights))
    if rust_memory:
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
        if price_form:
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
                if record is not None:
                    record[(plays[pi][0]["play"], m["round"], m["step"])] = (mp, acts.get(r))
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
    res = {"stats": dict(st), "dump fields missing": A.missing_report(), "families": {f: dict(c) for f, c in sorted(fam.items())}, "examples": examples, "examples_notes": examples_notes,
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
    ap.add_argument("--rust-memory", action="store_true", help="the memory's step in Rust's arithmetic order (armymem)")
    a = ap.parse_args(argv)
    if a.cpu:
        mx.set_default_device(mx.cpu)
    mx.set_memory_limit(12 << 30)
    mx.set_cache_limit(2 << 30)
    replay(a.dump, a.weights, group=a.group, limit=a.limit, families=a.families.split(",") if a.families else None, show=a.show, out=a.out,
           rust_memory=a.rust_memory)


if __name__ == "__main__":
    main()
