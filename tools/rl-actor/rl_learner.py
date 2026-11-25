"""The MLX learner of the actor-learner: reads the army-net plays that the Rust actor sampled (`ai-exam suite --record`
with `ARMY_SAMPLE_PLAN`, crates/qa/src/army_net/sample.rs) and computes the SAME GRPO loss and gradient as `armyrl` (the RL part:
minus the group-normalised advantage x the log-probability of the sampled picks and actions, over the decisions of the groups with a
gradient; the memory, the turn's map and the notes are inputs: no gradient through their writing).

What it reads: per play the turns' first moments (for the map), the turn-end memory states (the memory's step), and in a sampled
turn every decision with its pick and action. The memory, the map and the notes at each decision are recomputed with the net's own
unroll (`train_m.unroll_compact`, no gradient), exactly as the anchor's imitation does for the teachers' plays.

    python rl_learner.py RECORDS.bin --weights W.safetensors [--check] [--seconds]
"""
import argparse
import collections
import json
import sys
import time

import mlx.core as mx
import mlx.nn as nn
from mlx.utils import tree_flatten, tree_map

from aurelia_nn.army import train_m as TM
from aurelia_nn.army.train_m import DEC_KEYS, PAD as _PAD, body_any, hist_rows, map_feats_rows, plan_batch, unroll_compact
from aurelia_nn.tacsim.armyloop import load_net

PAD = 64


class DeviceR(TM.DeviceM):
    """DeviceM without the value targets (capture, Phi): the RL loss does not read them (zeros)."""

    def outcome_targets(self, flat):
        from aurelia_nn.army.potential import price_of
        n = len(flat)
        self.price = mx.array([price_of(flat.plays[p]) for p in flat.play], mx.float32)
        self.capture = mx.zeros((n,), mx.float32)
        self.dphi = mx.zeros((n,), mx.float32)


def advantages(metas):
    """Per play: the group-normalised advantage of its win (a group = a scene's copies), as `armyrl.advantages`. Returns (adv, live)."""
    scenes = collections.defaultdict(list)
    for i, m in enumerate(metas):
        scenes[(m["family"], m["era"], m["instance"], m["attempt"])].append(i)
    adv = [0.0] * len(metas)
    live = [False] * len(metas)
    for rows in scenes.values():
        rs = [1.0 if metas[i]["win"] else 0.0 for i in rows]
        mean = sum(rs) / len(rs)
        std = (sum((x - mean) ** 2 for x in rs) / len(rs)) ** 0.5
        if std > 1e-6:
            for i, x in zip(rows, rs):
                adv[i] = (x - mean) / (std + 1e-6)
                live[i] = True
    return adv, live, len(scenes)


def prepare(stream, ids, tm):
    """The device arrays of the plays `ids`, the step's plan, and the advantages."""
    flat = stream.flat(ids)
    adv, live, n_scenes = advantages(flat.plays)
    labelled = [i for i, p in enumerate(flat.ptr) if p >= -1]
    n_live = sum(1 for i in labelled if live[flat.play[i]])
    w = [0.0] * len(flat)
    for i in labelled:
        w[i] = adv[flat.play[i]] / max(n_live, 1)
    dev = DeviceR(flat, w, tm, 128)
    return flat, dev, adv, live, n_live, n_scenes


def unroll(model, dev, flat, tm):
    """The memory, maps and notes at every decision (inputs), and the plan of the labelled decisions."""
    import random
    plan = plan_batch(dev, list(range(len(flat.plays))), 999, 0.0, random.Random(0), tm, train=False)
    u = unroll_compact(model, dev, plan, tm, with_aux=False, compile_=True)
    arrays = [u.U, u.P] + ([u.maps.m] if u.maps is not None else []) + (list(u.NS.values()) if u.NS is not None else [])
    mx.eval(*arrays)
    return plan, u


def groups_of(dev, plan, u, keep=None):
    """Batches of the labelled decisions (`run_step`'s groups): yields (decision ids, the batch dict, um, pm)."""
    groups = plan[3]
    col = mx.array(u.col, mx.int32)
    for (tg, cg), (ids, cols, turns) in sorted(groups.items()):
        if keep is not None:
            sel = [k for k, i in enumerate(ids) if keep(i)]
            if not sel:
                continue
            ids, cols, turns = [ids[k] for k in sel], [cols[k] for k in sel], [turns[k] for k in sel]
        for s in range(0, len(ids), 1024):
            ci, cc_, ct = ids[s:s + 1024], cols[s:s + 1024], turns[s:s + 1024]
            n = len(ci)
            pad = (-n) % PAD
            idx = mx.array(ci + [ci[0]] * pad, mx.int32)
            full = dev.batch_m(idx, tg, cg, hist_rows(dev, ci))
            b = {k: full[k] for k in DEC_KEYS}
            b['awi'] = mx.array(0.0, mx.float32)
            TM.hist_mask(b)
            live = mx.arange(n + pad) < n
            b['w'] = mx.where(live, b['w'], 0.0)
            b['danger'] = mx.where(live[:, None], b['danger'], -1.0)
            tt = mx.array(ct + [ct[0]] * pad, mx.int32)
            cc = mx.array(cc_ + [cc_[0]] * pad, mx.int32)
            uc = col[cc]
            P0 = u.P[:, :, 0]
            b['hplan'] = P0[mx.maximum(tt[:, None] - full['hage'], 0), uc[:, None]]
            if u.maps is not None:
                b['pos'] = full['pos']
                b['cpos'] = full['cpos']
                map_feats_rows(b, u.maps, u.maps.rows[tt, cc])
                del b['pos'], b['cpos']
            if u.NS is not None:
                for k, v in u.NS.items():
                    b['ns_' + k] = v[tt, uc]
                b['ns_off'] = u.ndrop[uc]
            yield ci, n, b, u.U[tt, uc], u.P[tt, uc]


def logps(model, b, um, pm):
    """Log-probability of the labelled pick and of the labelled action (0 where none) per row, as `armyrl.logp_parts` takes them."""
    hfull = body_any(model, b, um, pm)
    t = b['tok'].shape[1]
    h = hfull[:, :t]
    pl = model.pointer_logits(h, b['sel'])
    al = model.action_logits_g(h, b['unit'], b['cand'], b['promo'], b['cmask'], b['gcand'], b['cand_ok'])
    lp = mx.take_along_axis(pl, b['ptr'][:, None], axis=1)[:, 0] - mx.logsumexp(pl, axis=1)
    la = (mx.take_along_axis(al, b['act'][:, None], axis=1)[:, 0] - mx.logsumexp(al, axis=1)) * b['has_act']
    return lp, la


def make_grad(model):
    def loss_fn(model, b, um, pm):
        lp, la = logps(model, b, um, pm)
        return -mx.sum(b['w'] * (lp + la))
    return nn.value_and_grad(model, loss_fn)


def rl_gradient(model, grad_fn, dev, plan, u, live_play, flat):
    """The summed RL gradient over the groups of labelled decisions of the live plays (the RL loss of armyrl)."""
    total, loss = None, 0.0
    play_of = list(flat.play)
    for ci, n, b, um, pm in groups_of(dev, plan, u, keep=lambda i: live_play[play_of[i]]):
        l, g = grad_fn(model, b, mx.stop_gradient(um), mx.stop_gradient(pm))
        total = g if total is None else tree_map(lambda x, y: x + y, total, g)
        mx.eval(total, l)
        loss += l.item()
    return loss, total


def check(model, dev, plan, u, flat):
    """The learner's log-probs of the labelled decisions against Rust's own (the play headers' `sample`): the largest differences."""
    want = {}
    for p, meta in enumerate(flat.plays):
        for idx, lp, la in meta.get("sample", []):
            want[meta["first"] + idx] = (lp, la)
    dp, da, n, big = [], [], 0, []
    for ci, k, b, um, pm in groups_of(dev, plan, u):
        lp, la = logps(model, b, um, pm)
        mx.eval(lp, la)
        lp, la = lp[:k].tolist(), la[:k].tolist()
        for i, a, c in zip(ci, lp, la):
            if i not in want:
                continue
            r_lp, r_la = want[i]
            n += 1
            dp.append(abs(a - r_lp))
            if r_la is not None:
                da.append(abs(c - r_la))
            big.append((max(dp[-1], abs(c - r_la) if r_la is not None else 0.0), flat.plays[flat.play[i]]["family"], flat.play[i], i - flat.plays[flat.play[i]]["first"],
                        flat.round[i], int(flat.ptr[i]), int(flat.act[i]), round(a, 4), round(r_lp, 4), None if r_la is None else round(c, 4), r_la if r_la is None else round(r_la, 4)))
    return {"checked": n, "missing": len(want) - n, "max_pick": max(dp, default=0), "max_action": max(da, default=0),
            "mean_pick": sum(dp) / max(len(dp), 1), "mean_action": sum(da) / max(len(da), 1),
            "share_over_1e-4": sum(1 for x in dp + da if x > 1e-4) / max(len(dp + da), 1),
            "worst": sorted(big, reverse=True)[:12]}


def step(model, stream, ids=None, grad_fn=None, timing=None):
    """One learner step: (loss, gradient, info). `timing` collects the seconds of the parts."""
    t = time.time()
    ids = list(range(len(stream))) if ids is None else ids
    tm = max(stream.t_max, 1)
    flat, dev, adv, live, n_live, n_scenes = prepare(stream, ids, tm)
    mx.eval(dev.w)
    t1 = time.time()
    plan, u = unroll(model, dev, flat, tm)
    t2 = time.time()
    grad_fn = grad_fn or make_grad(model)
    loss, g = rl_gradient(model, grad_fn, dev, plan, u, live, flat)
    t3 = time.time()
    if timing is not None:
        timing.update({"read_and_upload": t1 - t, "memory_unroll": t2 - t1, "gradient": t3 - t2, "total": t3 - t})
    info = {"plays": len(flat.plays), "scenes": n_scenes, "decisions": len(flat), "labelled": sum(1 for p in flat.ptr if p >= -1), "live_decisions": n_live,
            "groups_with_gradient": sum(1 for x in live if x) / max(len(live), 1), "loss": loss}
    return loss, g, info, (flat, dev, plan, u)


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("records", nargs="+")
    ap.add_argument("--weights", required=True)
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--repeat", type=int, default=1)
    ap.add_argument("--cpu", action="store_true", help="debug: the CPU device")
    ap.add_argument("--first", type=int, default=0, help="debug: only the first N plays")
    a = ap.parse_args(argv)
    if a.cpu:
        mx.set_default_device(mx.cpu)
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    model = load_net(a.weights)
    t = time.time()
    stream = TM.Stream(a.records, 1, {}, None)
    print(f"indexed {len(stream)} plays, {stream.decisions} decisions, {stream.labelled} labelled, t_max {stream.t_max} in {time.time() - t:.1f}s", flush=True)
    grad_fn = make_grad(model)
    for r in range(a.repeat):
        timing = {}
        loss, g, info, (flat, dev, plan, u) = step(model, stream, ids=list(range(a.first)) if a.first else None, grad_fn=grad_fn, timing=timing)
        gn = sum(mx.sum(v * v).item() for _, v in tree_flatten(g)) ** 0.5 if g is not None else 0.0
        print(json.dumps({"repeat": r, **info, "grad_norm": round(gn, 5), "seconds": {k: round(v, 2) for k, v in timing.items()}, "peak_gb": round(mx.get_peak_memory() / 2**30, 2)}), flush=True)
        if a.check and r == 0:
            print("CHECK", json.dumps(check(model, dev, plan, u, flat)), flush=True)


if __name__ == "__main__":
    main()
