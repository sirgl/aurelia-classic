"""The learner of rl3: the RL loss of `armyrl` (minus the group-normalised advantage x the log-probability of the sampled pick and
action, normalised by the live decisions) on the plays the Rust actor recorded, with

- the memory and the notes the decisions read taken from the record (Rust writes its state after every turn end, `sample::note_memory`),
  not recomputed: a recomputed hidden state can differ in a discrete element and move a decision by 0.1 in the log-probability (the five
  outliers of the first measurement); the values the learner reads are Rust's;
- the gradient through the memory (truncated back-propagation over `window` turns): the memory a sampled turn reads is recomputed from
  Rust's state `window` turns earlier by the net's memory steps with gradient, and its VALUE is replaced by Rust's (straight-through:
  value = Rust's, gradient = the chain's). `window` 0: the memory is an input (as in rl2).
The turn's map is an input (no RL gradient to the map encoder, as in rl2), and so is the history.
"""
import base64
from array import array
import collections
import random
import time

import mlx.core as mx
import mlx.nn as nn
from mlx.utils import tree_flatten, tree_map

from aurelia_nn.army import train_m as TM
from aurelia_nn.army.model_m import PLANS, SLOTS
from aurelia_nn.army.train_m import DEC_KEYS, NOTE_LABELS, PAD, UNROLL_KEYS, body_any, hist_mask, hist_rows, map_feats_rows, plan_batch, unroll_one_n

U_DIM = 16
# A decision has up to 224 candidates (air-intercept: every tile a fighter can reach); train_m's groups stop at 128 and its DeviceM truncates there, which
# moved the action's log-probability by up to 2 on those decisions. The learner's groups go to 256 (the anchor's own are unchanged).
C_CAP = 256
TM.C_GROUPS = (16, 48, 128, C_CAP)


class DeviceR(TM.DeviceM):
    """DeviceM without the value targets (capture, Phi): the RL loss does not read them (zeros)."""

    def outcome_targets(self, flat):
        from aurelia_nn.army.potential import price_of
        n = len(flat)
        self.price = mx.array([price_of(flat.plays[p]) for p in flat.play], mx.float32)
        self.capture = mx.zeros((n,), mx.float32)
        self.dphi = mx.zeros((n,), mx.float32)


def advantages(metas):
    """Per play: the group-normalised advantage of its win (a group = a scene's copies), as `armyrl.advantages`. Returns (adv, live, scenes)."""
    scenes = collections.defaultdict(list)
    for i, m in enumerate(metas):
        scenes[(m["family"], m["era"], m["instance"], m["attempt"])].append(i)
    adv, live = [0.0] * len(metas), [False] * len(metas)
    for rows in scenes.values():
        rs = [1.0 if metas[i]["win"] else 0.0 for i in rows]
        mean = sum(rs) / len(rs)
        std = (sum((x - mean) ** 2 for x in rs) / len(rs)) ** 0.5
        if std > 1e-6:
            for i, x in zip(rows, rs):
                adv[i] = (x - mean) / (std + 1e-6)
                live[i] = True
    return adv, live, scenes


def lost_slots(flat):
    """The decisions whose picked unit has no army slot in the record (pointer >= 0, slot -1). The net read that unit's memory by its slot, so the
    learner would read another memory than the net did and every log-probability of the decision would be off (the step 1.10 parity outliers:
    the recorder looked the ids up after the command had run, and a unit killed by it lost its slot). Returns the flat indices."""
    slot = memoryview(flat.slot).cast("h")
    return [i for i, p in enumerate(flat.ptr) if p >= 0 and slot[flat.tok_off[i] + p] < 0]


class Batch:
    """The device arrays of the plays of a step: decisions, plan, advantages, the maps."""

    def __init__(self, model, stream, ids=None):
        ids = list(range(len(stream))) if ids is None else ids
        self.tm = max(stream.t_max, 1)
        self.flat = flat = stream.flat(ids)
        lost = lost_slots(flat)
        if lost:
            raise ValueError(f"{len(lost)} decisions of the record pick a unit without an army slot (first: {lost[:5]}): the record's ids were read after the command")
        self.adv, self.live, self.scenes = advantages(flat.plays)
        labelled = [i for i, p in enumerate(flat.ptr) if p >= -1]
        self.n_live = sum(1 for i in labelled if self.live[flat.play[i]])
        w = [0.0] * len(flat)
        for i in labelled:
            w[i] = self.adv[flat.play[i]] / max(self.n_live, 1)
        self.dev = DeviceR(flat, w, self.tm, C_CAP)
        self.plan = plan_batch(self.dev, list(range(len(flat.plays))), 999, 0.0, random.Random(0), self.tm, train=False)
        self.maps = TM.TurnMaps(model, self.dev, self.plan[4], self.tm) if isinstance(model, TM.MBNet) else None
        self.labelled = len(labelled)
        self.states = rust_states(model, flat, self.plan)


class States:
    """The memory states the decisions read, [R + 1, B, ...] (row r: the state before turn r; B = the plays in order), Rust's."""

    def __init__(self, U, P, A, NS, source):
        self.U, self.P, self.A, self.NS, self.source = U, P, A, NS, source
        self.col = list(range(U.shape[1]))


def rust_states(model, flat, plan):
    """Rust's memory state after each turn end of each play (record header `memory`), as arrays [R + 1, B, ...]; row 0 is the net's
    initial state. A play without it keeps the initial state in every row (counted in `source`)."""
    R = plan[0]
    B = len(flat.plays)
    d = model.config["d"]
    um, pm, al = model.initial(1)
    ns0 = model.initial_notes(1)
    keys = [("u", SLOTS * U_DIM, um), ("p", PLANS * d, pm), ("alive", SLOTS, al)]
    notes = [("stage", 6), ("stage_age", 1), ("engaged", 1), ("waited", 1), ("free", 8), ("change_age", 1), ("reason", 4), ("rest", SLOTS), ("rest_age", SLOTS)]
    keys += [(k, w, ns0[k]) for k, w in notes]
    shapes = {k: (R + 1, B) + tuple(v.shape[1:]) for k, w, v in keys}
    buf = {k: array("f", [float(x) for x in v.reshape(-1).tolist()]) * ((R + 1) * B) for k, w, v in keys}
    width = {k: w for k, w, _ in keys}
    order = [k for k, _, _ in keys]
    ok = 0
    for c, meta in enumerate(flat.plays):
        if "memory" not in meta:
            continue
        L = meta["memory_len"]
        raw = array("f")
        raw.frombytes(base64.b64decode(meta["memory"]))
        n = min(len(raw) // L, R)
        for t in range(n):
            o = t * L
            for k in order:
                w = width[k]
                at = ((t + 1) * B + c) * w
                buf[k][at:at + w] = raw[o:o + w]
                o += w
        ok += 1
    arr = {k: mx.array(buf[k]).reshape(shapes[k]) for k in order}
    return States(arr["u"], arr["p"], arr["alive"], {k: arr[k] for k, _ in notes}, f"rust {ok} of {B} plays")


def decision_batch(model, bt, tg, cg, ids, cols, turns, um_of=None):
    """The batch of labelled decisions (padded to PAD) and its memory inputs. `um_of(cols_padded, turns_padded)` -> (um, pm, ns dict)
    overrides the states (the chain); None: Rust's states."""
    dev, st = bt.dev, bt.states
    n = len(ids)
    pad = (-n) % PAD
    idx = mx.array(ids + [ids[0]] * pad, mx.int32)
    full = dev.batch_m(idx, tg, cg, hist_rows(dev, ids))
    b = {k: full[k] for k in DEC_KEYS if k in full}
    b["awi"] = mx.array(0.0, mx.float32)
    hist_mask(b)
    live = mx.arange(n + pad) < n
    b["w"] = mx.where(live, b["w"], 0.0)
    b["danger"] = mx.where(live[:, None], b["danger"], -1.0)
    tt = mx.array(turns + [turns[0]] * pad, mx.int32)
    cc = mx.array(cols + [cols[0]] * pad, mx.int32)
    b["hplan"] = st.P[:, :, 0][mx.maximum(tt[:, None] - full["hage"], 0), cc[:, None]]
    if bt.maps is not None:
        b["pos"], b["cpos"] = full["pos"], full["cpos"]
        map_feats_rows(b, bt.maps, bt.maps.rows[tt, cc])
        del b["pos"], b["cpos"]
    if um_of is None:
        um, pm, ns = st.U[tt, cc], st.P[tt, cc], {k: v[tt, cc] for k, v in st.NS.items()}
    else:
        um, pm, ns = um_of(cc)
    for k, v in ns.items():
        b["ns_" + k] = v
    b["ns_off"] = mx.zeros((n + pad,), dtype=mx.bool_)
    return b, n, um, pm


def logps(model, b, um, pm):
    """Log-probability of the labelled pick and of the labelled action (0 where none) per row."""
    hfull = body_any(model, b, um, pm)
    t = b["tok"].shape[1]
    h = hfull[:, :t]
    pl = model.pointer_logits(h, b["sel"])
    if isinstance(model, TM.MBTNet):
        al = model.action_logits_t(h, b)
    else:
        al = model.action_logits_g(h, b["unit"], b["cand"], b["promo"], b["cmask"], b["gcand"], b["cand_ok"])
    lp = mx.take_along_axis(pl, b["ptr"][:, None], axis=1)[:, 0] - mx.logsumexp(pl, axis=1)
    la = (mx.take_along_axis(al, b["act"][:, None], axis=1)[:, 0] - mx.logsumexp(al, axis=1)) * b["has_act"]
    return lp, la


def chain(model, bt, cols, r, window):
    """The memory the plays `cols` read at turn r, by the memory steps of turns r - window .. r - 1 with gradient from Rust's state at
    turn r - window. Returns (um, pm, ns dict) in the order of `cols`, valued as Rust's states of turn r, gradient from the chain, and
    the largest drift of the chain's value from Rust's."""
    dev, st, plan, tm = bt.dev, bt.states, bt.plan, bt.tm
    ms, reset = plan[1], plan[2]
    s = max(0, r - window)
    ci = mx.array(cols, mx.int32)
    um, pm, al = mx.stop_gradient(st.U[s, ci]), mx.stop_gradient(st.P[s, ci]), mx.stop_gradient(st.A[s, ci])
    ns = {k: mx.stop_gradient(v[s, ci]) for k, v in st.NS.items()}
    for t in range(s, r):
        ids = [ms[t][c] for c in cols]
        idx = mx.array(ids, mx.int32)
        valid = idx >= 0
        full = dev.batch_m(mx.maximum(idx, 0), tm, 1, hist_rows(dev, [max(i, 0) for i in ids]))
        b = {k: full[k] for k in UNROLL_KEYS + NOTE_LABELS if k in full}
        if bt.maps is not None:
            b["pos"] = full["pos"]
            map_feats_rows(b, bt.maps, bt.maps.rows[t][ci])
            del b["pos"]
        for k in model.NOTE_STATE:
            b["ns_" + k] = ns[k]
        b["ns_off"] = mx.zeros((len(cols),), dtype=mx.bool_)
        rs = mx.array([reset[c] == t + 1 for c in cols])
        um, pm, al, nst, _, _ = unroll_one_n(model, b, valid, rs, um, pm, al, False)
        ns = dict(zip(model.NOTE_STATE, nst))
    drift = mx.max(mx.abs(um - st.U[r, ci]))
    st_ = lambda x, ref: ref + (x - mx.stop_gradient(x))
    return st_(um, st.U[r, ci]), st_(pm, st.P[r, ci]), {k: st_(v, st.NS[k][r, ci]) for k, v in ns.items()}, drift


def rl_gradient(model, bt, window, timing=None):
    """The summed RL gradient over the labelled decisions of the live plays. Returns (loss, gradient or None, info)."""
    flat, plan = bt.flat, bt.plan
    live_play, play_of = bt.live, list(flat.play)
    groups = plan[3]
    keep = lambda i: live_play[play_of[i]]
    total, loss, drift = None, 0.0, 0.0
    if window <= 0:
        def loss_fn(model, b, um, pm):
            lp, la = logps(model, b, um, pm)
            return -mx.sum(b["w"] * (lp + la))
        grad_fn = nn.value_and_grad(model, loss_fn)
        for (tg, cg), (ids, cols, turns) in sorted(groups.items()):
            sel = [k for k, i in enumerate(ids) if keep(i)]
            for s in range(0, len(sel), 1024):
                part = sel[s:s + 1024]
                b, n, um, pm = decision_batch(model, bt, tg, cg, [ids[k] for k in part], [cols[k] for k in part], [turns[k] for k in part])
                l, g = grad_fn(model, b, mx.stop_gradient(um), mx.stop_gradient(pm))
                total = g if total is None else tree_map(lambda x, y: x + y, total, g)
                mx.eval(total, l)
                loss += l.item()
        return loss, total, {"memory_drift": 0.0}
    by_turn = collections.defaultdict(lambda: collections.defaultdict(lambda: ([], [], [])))
    for (tg, cg), (ids, cols, turns) in groups.items():
        for i, c, t in zip(ids, cols, turns):
            if keep(i):
                e = by_turn[t][(tg, cg)]
                e[0].append(i), e[1].append(c), e[2].append(t)
    drifts = []

    def loss_fn(model, r, subs):
        plays = sorted({c for (_, cols, _) in subs.values() for c in cols})
        um_c, pm_c, ns_c, dr = chain(model, bt, plays, r, window)
        drifts.append(dr)
        pos = {c: j for j, c in enumerate(plays)}
        tot = mx.array(0.0)
        for (tg, cg), (ids, cols, turns) in subs.items():
            for s in range(0, len(ids), 1024):
                ci, cc_, ct = ids[s:s + 1024], cols[s:s + 1024], turns[s:s + 1024]

                def um_of(cc, cols_=cc_):
                    rows = mx.array([pos[c] for c in cols_] + [pos[cols_[0]]] * (cc.shape[0] - len(cols_)), mx.int32)
                    return um_c[rows], pm_c[rows], {k: v[rows] for k, v in ns_c.items()}
                b, n, um, pm = decision_batch(model, bt, tg, cg, ci, cc_, ct, um_of)
                lp, la = logps(model, b, um, pm)
                tot = tot - mx.sum(b["w"] * (lp + la))
        return tot
    grad_fn = nn.value_and_grad(model, loss_fn)
    for r in sorted(by_turn):
        l, g = grad_fn(model, r, by_turn[r])
        total = g if total is None else tree_map(lambda x, y: x + y, total, g)
        mx.eval(total, l)
        loss += l.item()
    for dr in drifts:
        drift = max(drift, dr.item())
    return loss, total, {"memory_drift": drift}


def check(model, bt):
    """The learner's log-probs of the labelled sampled decisions against Rust's own (the play headers' `sample`)."""
    flat = bt.flat
    want = {}
    for p, meta in enumerate(flat.plays):
        for idx, lp, la in meta.get("sample", []):
            want[meta["first"] + idx] = (lp, la)
    diffs, worst = [], []
    for (tg, cg), (ids, cols, turns) in sorted(bt.plan[3].items()):
        for s in range(0, len(ids), 1024):
            ci = [i for i in ids[s:s + 1024]]
            b, n, um, pm = decision_batch(model, bt, tg, cg, ci, cols[s:s + 1024], turns[s:s + 1024])
            lp, la = logps(model, b, um, pm)
            mx.eval(lp, la)
            lp, la = lp[:n].tolist(), la[:n].tolist()
            for i, a, c in zip(ci, lp, la):
                if i not in want:
                    continue
                r_lp, r_la = want[i]
                d1 = abs(a - r_lp)
                d2 = abs(c - r_la) if r_la is not None else 0.0
                diffs.append(d1)
                if r_la is not None:
                    diffs.append(d2)
                if max(d1, d2) > 1e-4:
                    worst.append((round(max(d1, d2), 5), flat.plays[flat.play[i]]["family"], int(flat.round[i])))
    n = len(diffs)
    return {"values": n, "max": max(diffs, default=0.0), "share_within_1e-4": sum(1 for x in diffs if x <= 1e-4) / max(n, 1),
            "over_1e-4": sum(1 for x in diffs if x > 1e-4), "over_1e-2": sum(1 for x in diffs if x > 1e-2), "worst": sorted(worst, reverse=True)[:10]}


def step(model, stream, window, ids=None):
    """One learner step: (loss, gradient, info). The seconds of the parts are in info."""
    t = time.time()
    bt = Batch(model, stream, ids)
    mx.eval(bt.dev.w, bt.states.U)
    t1 = time.time()
    loss, g, extra = rl_gradient(model, bt, window)
    t2 = time.time()
    info = {"plays": len(bt.flat.plays), "scenes": len(bt.scenes), "decisions": len(bt.flat), "labelled": bt.labelled, "live_decisions": bt.n_live,
            "groups_with_gradient": sum(1 for x in bt.live if x) / max(len(bt.live), 1), "loss": loss, "read_s": round(t1 - t, 2), "gradient_s": round(t2 - t1, 2), **extra,
            "states": bt.states.source}
    return loss, g, info, bt
