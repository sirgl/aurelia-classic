"""Variant M (learned memory) of the army net: imitation with the memory unrolled over whole plays
(work/nets/army-net/M-HANDOFF.md, «Training»).

python -m aurelia_nn.army.train_m --train runs/teach-train-m.bin --hold runs/teach-hold-m.bin --out runs/netM0

A step takes B plays. The memory is unrolled over their turns: the memory-state decisions of turn r (one per play
and turn: our turn's end; masked where a play has ended) run the body with M_r and update it to M_{r+1}. Then every
labelled decision of those plays is scored with its turn's M_r (the losses of `train.py`), plus the auxiliary head
on the teacher's own state at the memory states (privileged, training only; weight --aux). Truncated BPTT over whole
plays (<= 31 turns) with a length curriculum (4 -> 8 -> full turns), memory dropout (in --mem-drop of the plays the
memory is reset at a random turn), grad clip 1.0.
"""
import argparse
import collections
import json
import os
import random
import time

import mlx.core as mx
import mlx.nn as nn
import mlx.optimizers as optim

from .data import MEMORY_STATE, TRACK, Flat, map_file, read_play_at, skip_play_at
from .export import export
from .model_m import AIMS, P_ENGAGED, P_ETA, P_STAGE, P_TURNS, P_WAITED, PLANS, SLOTS, STAGES, U_ENGAGED, U_RESTING, U_WAITED, MemNet
from .train import Device, best_teachers, decision_weights
from .data import F_HIST
from .model_mh import H_MAX, MHNet
from .model_mb import HX_PARAMS, MBANet, MBHNet, MBNet
from .potential import potential, price_of

u16 = lambda b: mx.array(memoryview(b).cast('H'))

ASSAULT = {'assault-heal', 'promotion-choice', 'hill-archers', 'city-fire', 'planned-assault'}
C_GROUPS = (16, 48, 128)
T_GROUPS = (12, 24, 40)


class Plays:
    """The per-play structure of a Flat: its decisions, its memory states, each decision's turn index."""

    def __init__(self, flat):
        self.first = [m['first'] for m in flat.plays]
        self.count = [m['count'] for m in flat.plays]
        self.ms = []   # per play: the decision indices of its memory states
        self.fd = []   # per play: the first decision of each turn (MB: the turn's map is encoded from it)
        self.turn = [0] * len(flat)  # per decision: memory states before it in its play
        for p, (f, c) in enumerate(zip(self.first, self.count)):
            seen = []
            fd = []
            for i in range(f, f + c):
                self.turn[i] = len(seen)
                if len(fd) == len(seen):
                    fd.append(i)
                if flat.flags[i] & MEMORY_STATE:
                    seen.append(i)
            self.ms.append(seen)
            self.fd.append(fd)
        # Auxiliary targets of the memory states (host side): turns in the stage, expected turns to the capture.
        self.turns_in_stage = {}
        self.eta = {}
        for p, seen in enumerate(self.ms):
            meta = flat.plays[p]
            prev, run = None, 0
            for i in seen:
                st = flat.track[i * TRACK + 2]
                run = run + 1 if st == prev and st >= 0 else (1 if st >= 0 else 0)
                prev = st
                if st >= 0:
                    self.turns_in_stage[i] = run
                if meta['family'] in ASSAULT and meta.get('win'):
                    self.eta[i] = max(0, meta.get('rounds', 0) - flat.round[i])


class Stream:
    """The recordings indexed, not loaded (RAM: the index only; the files stay mapped): `flat(ids)` reads just
    those plays, so a step or an evaluation chunk holds only its own decisions."""

    def __init__(self, paths, nth=1, skip=None):
        self.bufs = []
        self.where = []   # per play: (file index, offset)
        self.metas = []
        self.t_max = 0
        self.decisions = 0
        self.labelled = 0
        seen = 0
        for k, path in enumerate(paths):
            drop = (skip or {}).get(path, set())
            buf = map_file(path)
            self.bufs.append(buf)
            at = 0
            while at < len(buf):
                meta, after, t_max, labelled, n = skip_play_at(buf, at)
                keep = meta['family'] not in drop
                if keep:
                    seen += 1
                    keep = seen % nth == 0
                if keep:
                    self.where.append((k, at))
                    self.metas.append(meta)
                    self.t_max = max(self.t_max, t_max)
                    self.decisions += n
                    self.labelled += labelled
                at = after

    def __len__(self):
        return len(self.metas)

    def flat(self, ids):
        out = Flat()
        for i in ids:
            k, at = self.where[i]
            read_play_at(out, self.bufs[k], at)
        return out


class DeviceM(Device):
    def __init__(self, flat, weights, tm, cm):
        super().__init__(flat, weights, tm, cm)
        self.slot = mx.array(memoryview(flat.slot).cast('h'))
        self.flags = mx.array(list(flat.flags), mx.int32)
        self.track = mx.array(list(flat.track), mx.int32).reshape(-1, TRACK)
        self.plays = Plays(flat)
        n = len(flat)
        tis = [-1.0] * n
        eta = [-1.0] * n
        for i, v in self.plays.turns_in_stage.items():
            tis[i] = float(v)
        for i, v in self.plays.eta.items():
            eta[i] = float(v)
        self.tis = mx.array(tis, mx.float32)
        self.eta = mx.array(eta, mx.float32)
        self.ptr_list = list(flat.ptr)
        # Variant MH: the history tokens (record v4).
        self.has_hist = len(flat.hist_age) > 0
        self.h = mx.array(list(flat.h), mx.int32)
        self.hist_off = mx.array(list(flat.hist_off), mx.int32)
        self.hist = u16(flat.hist).view(mx.float16).reshape(-1, F_HIST) if self.has_hist else mx.zeros((1, F_HIST), mx.float16)
        self.hist_slot = mx.array(memoryview(flat.hist_slot).cast('h')) if self.has_hist else mx.zeros((1,), mx.int16)
        self.hist_age = mx.array(memoryview(flat.hist_age).cast('B')) if self.has_hist else mx.zeros((1,), mx.uint8)
        self.n_hist = self.hist.shape[0]
        self.flags_list = list(flat.flags)
        self.round_list = list(flat.round)
        self.outcome_targets(flat)

    def outcome_targets(self, flat):
        """Per decision: the play's price [3], the capture (1 / 0; -1 where the scene has no city to take) and Phi's
        change from this turn's first moment to the first moment 3 turns on (the play's last turn at most)."""
        P = self.plays
        price, capture, cur, fut, first = [], [], [], [], []
        for i, p in enumerate(flat.play):
            meta = flat.plays[p]
            price.append(price_of(meta))
            taken = meta.get('measures', {}).get('taken')
            capture.append(-1.0 if taken is None else float(taken >= 0))
            fd = P.fd[p]
            r = min(P.turn[i], len(fd) - 1)
            cur.append(fd[r])
            fut.append(fd[min(r + 3, len(fd) - 1)])
            first.append(P.first[p])
        self.price = mx.array(price, mx.float32)
        self.capture = mx.array(capture, mx.float32)
        idx = mx.array(sorted(set(cur) | set(fut) | set(first)), mx.int32)
        b = self.batch(idx, self.tm, 1)
        pos = {int(j): k for k, j in enumerate(idx.tolist())}
        own_first = b['tok'][:, 0, 18] * 8.0
        first_of = mx.array([pos[f] for f in (P.first[flat.play[int(j)]] for j in idx.tolist())], mx.int32)
        worth = self.price[idx][:, 0] * 8.0
        phi = potential(b['tok'], b['tmask'], own_first[first_of], worth)
        self.dphi = phi[mx.array([pos[j] for j in fut], mx.int32)] - phi[mx.array([pos[j] for j in cur], mx.int32)]

    def batch_m(self, idx, tm, cm):
        b = self.batch(idx, tm, cm)
        t = self.t[idx]
        ar = mx.arange(tm)
        tmask = ar[None, :] < t[:, None]
        ti = mx.minimum(self.tok_off[idx][:, None] + mx.minimum(ar[None, :], t[:, None] - 1), self.n_tok - 1)
        b['slot'] = mx.where(tmask, self.slot[ti].astype(mx.int32), -1)
        b['track'] = self.track[idx]
        b['tis'] = self.tis[idx]
        b['eta'] = self.eta[idx]
        h = self.h[idx]
        ah = mx.arange(H_MAX)
        hmask = ah[None, :] < h[:, None]
        hi = mx.minimum(self.hist_off[idx][:, None] + mx.minimum(ah[None, :], mx.maximum(h[:, None] - 1, 0)), self.n_hist - 1)
        b['hist'] = mx.where(hmask[..., None], self.hist[hi].astype(mx.float32), 0.0)
        b['hmask'] = hmask
        b['hslot'] = mx.where(hmask, self.hist_slot[hi].astype(mx.int32), -1)
        b['hage'] = mx.where(hmask, self.hist_age[hi].astype(mx.int32), 0)
        b['price'] = self.price[idx]
        b['capture'] = self.capture[idx]
        b['dphi'] = self.dphi[idx]
        return b


def turn_maps(model, dev, plan, tm):
    """MB: every turn's map of the step's plays, [R + 1, B, W*W, C], from each turn's first moment."""
    fds = plan[4]
    out = []
    enc = compiled_fn(model, 'encode', lambda st, tok, pos: model.encode_turn(st, tok, pos))
    for k in range(len(fds)):
        idx = mx.maximum(mx.array(fds[k], mx.int32), 0)
        full = dev.batch_m(idx, tm, 1)
        out.append(enc(full['statics'], full['tok'], full['pos']))
    return mx.stack(out)


def map_feats(b, Ms, tt, cc):
    """The map at the tokens' and candidates' tiles of decisions (turn tt, play column cc), and its mean."""
    tt2, cc2 = tt[:, None], cc[:, None]
    b['gtok'] = Ms[tt2, cc2, mx.maximum(b['pos'], 0)]
    b['tok_ok'] = b['pos'] >= 0
    b['mmean'] = mx.mean(Ms[tt, cc], axis=1)
    if 'cpos' in b:
        b['gcand'] = Ms[tt2, cc2, mx.maximum(b['cpos'], 0)]
        b['cand_ok'] = b['cpos'] >= 0


def compiled_fn(model, name, fn):
    key = (id(model), name)
    if key not in COMPILED:
        COMPILED[key] = mx.compile(fn, inputs=[model.state])
    return COMPILED[key]


def body_any(model, b, um, pm):
    """M's body, or MH's with the history (b['hplan']: P_0 of each history token's turn), or MB's with the map."""
    if isinstance(model, MBANet):
        return model.body_b(b, um, pm)[0]
    if isinstance(model, MBNet):
        return model.body_mbg(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm, b['gtok'], b['tok_ok'], b['mmean'])
    if isinstance(model, MHNet):
        return model.body_mh(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm, b['hist'], b['hmask'], b['hslot'], b['hplan'])
    return model.body_m(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm)


def head_losses(model, b, hfull, h1, al_hid, w):
    """MBA's heads on decisions: danger after layer 1, the strike preview from the scorer's state, the value
    (win, capture, Phi's 3-turn change); and the advantage weights of the imitation (b['awi'] = beta, 0: off)."""
    t = b['tok'].shape[1]
    dmask = (b['danger'] >= 0) & b['sel']
    de = model.danger_early(h1[:, :t])
    esum = mx.sum(mx.where(dmask, (de - b['danger']) ** 2, 0.0))
    cand = b['cand']
    strike = ((cand[..., 2] + cand[..., 3] + cand[..., 4]) > 0.5) & b['cmask']
    xs = model.xs(al_hid)
    xsum = mx.sum(mx.where(strike[..., None], (xs - cand[..., 30:32]) ** 2, 0.0))
    v = model.value_a(hfull, hfull[:, t])
    live = (w > 0).astype(mx.float32)
    v_win = nn.losses.binary_cross_entropy(v[:, 0], b['win'], with_logits=True, reduction='none')
    cap_m = (b['capture'] >= 0).astype(mx.float32)
    v_cap = nn.losses.binary_cross_entropy(v[:, 1], mx.maximum(b['capture'], 0.0), with_logits=True, reduction='none') * cap_m
    v_phi = (v[:, 2] - b['dphi']) ** 2
    vsum = mx.sum(live * (v_win + v_cap + v_phi))
    adv = (b['win'] + b['dphi']) - mx.stop_gradient(mx.sigmoid(v[:, 0]) + v[:, 2])
    beta = b['awi']
    factor = mx.where(beta > 0, mx.exp(mx.clip(adv / mx.maximum(beta, 1e-3), -3.0, 1.0)), 1.0)
    return {'esum': esum, 'xsum': xsum, 'xn': mx.sum(strike), 'vsum': vsum, 'vn': mx.sum(live), 'factor': factor,
            'v_win_ok': mx.sum(live * ((v[:, 0] > 0) == (b['win'] > 0.5))), 'v_phi_se': mx.sum(live * v_phi)}


HEAD_KEYS = ('esum', 'xsum', 'xn', 'vsum', 'vn', 'v_win_ok', 'v_phi_se')


def decision_losses(model, b, um, pm):
    """The losses of `train.py` on decisions read with their memory (um [N, S, U], pm [N, P, d])."""
    if isinstance(model, MBANet):
        return decision_losses_a(model, b, um, pm)
    hfull = body_any(model, b, um, pm)
    t = b['tok'].shape[1]
    h = hfull[:, :t]
    pl = model.pointer_logits(h, b['sel'])
    if isinstance(model, MBNet):
        al = model.action_logits_g(h, b['unit'], b['cand'], b['promo'], b['cmask'], b['gcand'], b['cand_ok'])
    else:
        al = model.action_logits(h, b['unit'], b['cand'], b['promo'], b['cmask'])
    ce_ptr = mx.logsumexp(pl, axis=1) - mx.take_along_axis(pl, b['ptr'][:, None], axis=1)[:, 0]
    ce_act = (mx.logsumexp(al, axis=1) - mx.take_along_axis(al, b['act'][:, None], axis=1)[:, 0]) * b['has_act']
    v = model.value(h)
    bce = nn.losses.binary_cross_entropy(v[:, 0], b['win'], with_logits=True, reduction='none')
    lost = (mx.sigmoid(v[:, 1]) - b['lost']) ** 2
    dg = model.danger(h)
    dmask = (b['danger'] >= 0) & b['sel']
    dsum = mx.sum(mx.where(dmask, (dg - b['danger']) ** 2, 0.0))
    w = b['w']
    acc_ptr = (mx.argmax(pl, axis=1) == b['ptr']).astype(mx.float32)
    acc_act = (mx.argmax(al, axis=1) == b['act']).astype(mx.float32)
    out = {'main': mx.sum(w * (ce_ptr + ce_act)), 'val': mx.sum(w * (bce + lost)), 'w': mx.sum(w), 'dsum': dsum, 'dn': mx.sum(dmask),
           'acc_ptr': acc_ptr, 'acc_act': acc_act, 'has_act': b['has_act']}
    out.update({k: mx.array(0.0) for k in HEAD_KEYS})
    return out


def decision_losses_a(model, b, um, pm):
    """`decision_losses` for MBA: the same losses, the heads beside them, the imitation weighted by the advantage."""
    hfull, h1 = model.body_b(b, um, pm)
    t = b['tok'].shape[1]
    h = hfull[:, :t]
    pl = model.pointer_logits(h, b['sel'])
    al, hid = model.action_logits_a(h, b['unit'], b['cand'], b['promo'], b['cmask'], b['gcand'], b['cand_ok'])
    ce_ptr = mx.logsumexp(pl, axis=1) - mx.take_along_axis(pl, b['ptr'][:, None], axis=1)[:, 0]
    ce_act = (mx.logsumexp(al, axis=1) - mx.take_along_axis(al, b['act'][:, None], axis=1)[:, 0]) * b['has_act']
    v = model.value(h)
    bce = nn.losses.binary_cross_entropy(v[:, 0], b['win'], with_logits=True, reduction='none')
    lost = (mx.sigmoid(v[:, 1]) - b['lost']) ** 2
    dg = model.danger(h)
    dmask = (b['danger'] >= 0) & b['sel']
    dsum = mx.sum(mx.where(dmask, (dg - b['danger']) ** 2, 0.0))
    heads = head_losses(model, b, hfull, h1, hid, b['w'])
    w = b['w'] * heads.pop('factor')
    acc_ptr = (mx.argmax(pl, axis=1) == b['ptr']).astype(mx.float32)
    acc_act = (mx.argmax(al, axis=1) == b['act']).astype(mx.float32)
    out = {'main': mx.sum(w * (ce_ptr + ce_act)), 'val': mx.sum(w * (bce + lost)), 'w': mx.sum(w), 'dsum': dsum, 'dn': mx.sum(dmask),
           'acc_ptr': acc_ptr, 'acc_act': acc_act, 'has_act': b['has_act']}
    out.update(heads)
    return out


def aux_losses(model, b, um1, pm1, logits):
    """The auxiliary head on the teacher's own state at the memory states: um1 / pm1 = the memory after the update,
    logits = the plan's attention over the tokens at the update (its target pointer)."""
    tr = b['track']
    known = (tr[:, 0] >= 0).astype(mx.float32)  # [N]
    engaged = (tr[:, 0] > 0).astype(mx.float32)
    waited = mx.minimum(tr[:, 1], 10).astype(mx.float32) * 0.1
    stage = mx.maximum(tr[:, 2], 0)
    mask = tr[:, 3].astype(mx.int32) & 0xFFFF
    slot = b['slot']
    has = (slot >= 0) & b['tmask']
    sl = mx.maximum(slot, 0)
    n = slot.shape[0]
    u = um1[mx.arange(n)[:, None], sl]  # [N, T, U]
    au = model.aux_u
    rest_t = ((mask[:, None] >> sl) & 1).astype(mx.float32)
    um_known = has.astype(mx.float32) * known[:, None]
    rest_l = nn.losses.binary_cross_entropy(au[1, 0] * u[..., U_RESTING] + au[1, 1], rest_t, with_logits=True, reduction='none')
    eng_l = nn.losses.binary_cross_entropy(au[2, 0] * u[..., U_ENGAGED] + au[2, 1], mx.broadcast_to(engaged[:, None], rest_t.shape), with_logits=True, reduction='none')
    wait_l = (au[0, 0] * u[..., U_WAITED] + au[0, 1] - waited[:, None]) ** 2
    nu = mx.maximum(mx.sum(um_known), 1.0)
    unit = mx.sum(um_known * (rest_l + eng_l + wait_l)) / nu
    p0 = pm1[:, 0]
    ap = model.aux_p
    st_logits = ap[0, 0] * p0[:, P_STAGE:P_STAGE + STAGES] + ap[0, 1]
    st_l = mx.logsumexp(st_logits, axis=1) - mx.take_along_axis(st_logits, stage[:, None], axis=1)[:, 0]
    peng_l = nn.losses.binary_cross_entropy(ap[1, 0] * p0[:, P_ENGAGED] + ap[1, 1], engaged, with_logits=True, reduction='none')
    pwait_l = (p0[:, P_WAITED] - waited) ** 2
    tis = b['tis']
    tis_m = (tis >= 0).astype(mx.float32)
    tis_l = (p0[:, P_TURNS] - mx.minimum(tis, 10.0) * 0.1) ** 2
    eta = b['eta']
    eta_m = (eta >= 0).astype(mx.float32)
    eta_l = (p0[:, P_ETA] - eta / 30.0) ** 2
    nk = mx.maximum(mx.sum(known), 1.0)
    plan = mx.sum(known * (st_l + peng_l + pwait_l)) / nk + mx.sum(tis_m * tis_l) / mx.maximum(mx.sum(tis_m), 1.0) + mx.sum(eta_m * eta_l) / mx.maximum(mx.sum(eta_m), 1.0)
    # The target: the objective city's token (a city token marked "is the objective").
    tok = b['tok']
    is_obj = (tok[..., 43] > 0.5) & (tok[..., 3] > 0.5) & b['tmask']
    has_obj = mx.any(is_obj, axis=1).astype(mx.float32)
    tgt = mx.argmax(is_obj.astype(mx.int32), axis=1)
    lg = logits[:, 0]
    tgt_l = mx.logsumexp(lg, axis=1) - mx.take_along_axis(lg, tgt[:, None], axis=1)[:, 0]
    target = mx.sum(has_obj * tgt_l) / mx.maximum(mx.sum(has_obj), 1.0)
    acc_stage = mx.sum(known * (mx.argmax(st_logits, axis=1) == stage)) / nk
    return unit + plan + target, {'aux_unit': unit, 'aux_plan': plan, 'aux_target': target, 'stage_acc': acc_stage, 'known': mx.sum(known)}


def plan_batch(dev, plays, L, drop, rng, tm, train=True):
    """Index arrays of one step: the memory states [R, B] (-1 pad), the resets, and the labelled decisions by
    candidate group: (C, decision indices, their play column, their turn)."""
    P = dev.plays
    R = max(min(len(P.ms[p]), L) for p in plays) if plays else 0
    ms = [[-1] * len(plays) for _ in range(R)]
    for col, p in enumerate(plays):
        for r, i in enumerate(P.ms[p][:R]):
            ms[r][col] = i
    reset = [-1] * len(plays)
    if train and drop > 0:
        for col, p in enumerate(plays):
            n = min(len(P.ms[p]), L)
            if n >= 2 and rng.random() < drop:
                reset[col] = rng.randrange(1, n)
    groups = collections.defaultdict(lambda: ([], [], []))
    for col, p in enumerate(plays):
        f, c = P.first[p], P.count[p]
        for i in range(f, f + c):
            if P.turn[i] >= L:
                break
            if dev.ptr_list[i] < -1:
                continue
            cg = next(x for x in C_GROUPS if dev.c_list[i] <= x)
            tg = next(x for x in T_GROUPS if dev.t_list[i] <= x)
            g = groups[(min(tg, tm), cg)]
            g[0].append(i)
            g[1].append(col)
            g[2].append(P.turn[i])
    fds = [[-1] * len(plays) for _ in range(R + 1)]
    for col, p in enumerate(plays):
        for k, i in enumerate(P.fd[p][:R + 1]):
            fds[k][col] = i
    return R, ms, reset, dict(groups), fds


AUX_KEYS = ('aux_unit', 'aux_plan', 'aux_target', 'stage_acc', 'known')
UNROLL_KEYS = ('tok', 'dist', 'tmask', 'slot', 'track', 'tis', 'eta', 'hist', 'hmask', 'hslot', 'hage', 'price')
DEC_KEYS = ('tok', 'dist', 'tmask', 'slot', 'sel', 'unit', 'cand', 'promo', 'cmask', 'ptr', 'act', 'has_act', 'win', 'lost', 'danger', 'w', 'hist', 'hmask', 'hslot',
            'hage', 'price', 'capture', 'dphi')
DEC_OUT = ('main', 'val', 'w', 'dsum', 'dn') + HEAD_KEYS
# History ablation (proof (a) of MH): the history masked everywhere.
ABLATE = {'history': False}


def hist_mask(b):
    if ABLATE['history']:
        b['hmask'] = mx.zeros(b['hmask'].shape, dtype=mx.bool_)
# Decision groups are padded to a multiple of this (rows of weight 0), so the compiled forward sees few shapes.
PAD = 256


def unroll_one(model, b, valid, rs, um, pm, alive, with_aux):
    """One turn of the memory unroll on the memory states `b` (valid: the play has this turn; rs: reset after it)."""
    hfull = body_any(model, b, um, pm)
    if isinstance(model, MHNet):
        um2, pm2, al2, lg = model.step_mh(hfull, b['tok'], b['slot'], b['tmask'], um, pm, alive, b['hist'], b['hmask'])
    else:
        um2, pm2, al2, lg = model.step(hfull, b['tok'], b['slot'], b['tmask'], um, pm, alive)
    if with_aux:
        bb = dict(b)
        bb['track'] = mx.where(valid[:, None], b['track'], -1)
        bb['tis'] = mx.where(valid, b['tis'], -1.0)
        bb['eta'] = mx.where(valid, b['eta'], -1.0)
        bb['tmask'] = b['tmask'] & valid[:, None]
        a, parts = aux_losses(model, bb, um2, pm2, lg)
        parts = tuple(parts[k] for k in AUX_KEYS)
    else:
        a, parts = mx.array(0.0), tuple(mx.array(0.0) for _ in AUX_KEYS)
    um = mx.where(valid[:, None, None], um2, um)
    pm = mx.where(valid[:, None, None], pm2, pm)
    alive = mx.where(valid[:, None], al2, alive)
    p0 = mx.broadcast_to(model.plan0[None], pm.shape)
    um = mx.where(rs[:, None, None], 0.0, um)
    pm = mx.where(rs[:, None, None], p0, pm)
    alive = mx.where(rs[:, None], 0.0, alive)
    return um, pm, alive, a, parts


def dec_one(model, b, um, pm):
    out = decision_losses(model, b, um, pm)
    return tuple(out[k] for k in DEC_OUT + ('acc_ptr', 'acc_act', 'has_act'))


COMPILED = {}
# Checkpoints of the play over training: one per half-epoch and one at each job's end, the last few kept.
CKPT_KEEP = 8


def save_checkpoint(model, out, epoch):
    """OUT/ckpt/e<epoch>.safetensors and its Rust export .bin, a line in OUT/ckpt/index.jsonl; only the last
    CKPT_KEEP checkpoints' files stay (the index keeps every line)."""
    d = os.path.join(out, 'ckpt')
    os.makedirs(d, exist_ok=True)
    name = f'e{epoch:.2f}'
    model.save_weights(os.path.join(d, name + '.safetensors'))
    export(model, os.path.join(d, name + '.bin'))
    with open(os.path.join(d, 'index.jsonl'), 'a') as f:
        f.write(json.dumps({'epoch': round(epoch, 2), 'path': os.path.abspath(os.path.join(d, name + '.bin')), 'weights': os.path.abspath(os.path.join(d, name + '.safetensors'))}) + '\n')
    kept = sorted((float(n[1:-len('.safetensors')]) for n in os.listdir(d) if n.endswith('.safetensors')))
    for e in kept[:-CKPT_KEEP]:
        for ext in ('.safetensors', '.bin'):
            path = os.path.join(d, f'e{e:.2f}{ext}')
            if os.path.exists(path):
                os.remove(path)


def compiled(model, name, with_aux=True):
    """The compiled stage `name` (its parameters read from the model's state at each call)."""
    key = (id(model), name, with_aux)
    if key not in COMPILED:
        if name == 'unroll':
            fn = lambda b, valid, rs, um, pm, alive: unroll_one(model, b, valid, rs, um, pm, alive, with_aux)
        else:
            fn = lambda b, um, pm: dec_one(model, b, um, pm)
        COMPILED[key] = mx.compile(fn, inputs=[model.state])
    return COMPILED[key]


def unroll_memory(model, dev, plan, tm, with_aux=True, compile_=True):
    """The memory unrolled over the turns of a step's plays: (turn maps or None, U [R + 1, B, S, U], P [R + 1, B,
    PLANS, d], the auxiliary loss, its parts)."""
    R, ms, reset, groups = plan[:4]
    B = len(reset)
    Ms = turn_maps(model, dev, plan, tm) if isinstance(model, MBNet) else None
    um, pm, alive = model.initial(B)
    Us, Ps = [um], [pm]
    aux_tot = mx.array(0.0)
    aux_parts = collections.defaultdict(float)
    reset_a = mx.array(reset, mx.int32)
    unroll = compiled(model, 'unroll', with_aux) if compile_ else (lambda b, v, rs, um, pm, al: unroll_one(model, b, v, rs, um, pm, al, with_aux))
    for r in range(R):
        idx = mx.array(ms[r], mx.int32)
        valid = idx >= 0
        full = dev.batch_m(mx.maximum(idx, 0), tm, 1)
        b = {k: full[k] for k in UNROLL_KEYS}
        if Ms is not None:
            b['pos'] = full['pos']
            map_feats(b, Ms, mx.full((B,), r, mx.int32), mx.arange(B))
            del b['pos']
        hist_mask(b)
        # P_0 of each history token's turn (age 0: this turn's plan, pm).
        p0s = mx.stack([P[:, 0] for P in Ps[::-1][:4]] + [Ps[0][:, 0]] * max(0, 4 - len(Ps)), axis=1)  # [B, 4, d], age 0..3
        b['hplan'] = p0s[mx.arange(B)[:, None], mx.minimum(full['hage'], 3)]
        um, pm, alive, a, parts = unroll(b, valid, reset_a == r + 1, um, pm, alive)
        aux_tot = aux_tot + a / R
        for k, v in zip(AUX_KEYS, parts):
            aux_parts[k] = aux_parts[k] + v / R
        Us.append(um)
        Ps.append(pm)
    Us = mx.stack(Us)  # [R + 1, B, S, U]
    Ps = mx.stack(Ps)
    return Ms, Us, Ps, aux_tot, dict(aux_parts)


def run_step(model, dev, plan, tm, aux_w, with_aux=True, compile_=True, head_w=0.0, value_w=0.0, awi=0.0):
    """The loss of one step's plays (and its parts). MBA: head_w weighs the auxiliary heads (danger after layer 1,
    the strike preview), value_w the value head, awi = the advantage weighting's beta (0: plain imitation)."""
    groups = plan[3]
    Ms, Us, Ps, aux_tot, aux_parts = unroll_memory(model, dev, plan, tm, with_aux, compile_)
    dec = compiled(model, 'dec') if compile_ else (lambda b, um, pm: dec_one(model, b, um, pm))
    tot = collections.defaultdict(lambda: mx.array(0.0))
    accs = []
    for (tg, cg), (ids, cols, turns) in sorted(groups.items()):
        n = len(ids)
        pad = (-n) % PAD
        idx = mx.array(ids + [ids[0]] * pad, mx.int32)
        full = dev.batch_m(idx, tg, cg)
        b = {k: full[k] for k in DEC_KEYS}
        b['awi'] = mx.array(awi, mx.float32)
        hist_mask(b)
        live = mx.arange(n + pad) < n
        b['w'] = mx.where(live, b['w'], 0.0)
        b['danger'] = mx.where(live[:, None], b['danger'], -1.0)
        tt = mx.array(turns + [turns[0]] * pad, mx.int32)
        cc = mx.array(cols + [cols[0]] * pad, mx.int32)
        P0 = Ps[:, :, 0]  # [R + 1, B, d]
        b['hplan'] = P0[mx.maximum(tt[:, None] - full['hage'], 0), cc[:, None]]
        if Ms is not None:
            b['pos'] = full['pos']
            b['cpos'] = full['cpos']
            map_feats(b, Ms, tt, cc)
            del b['pos'], b['cpos']
        out = dec(b, Us[tt, cc], Ps[tt, cc])
        k_out = len(DEC_OUT)
        for k, v in zip(DEC_OUT, out[:k_out]):
            tot[k] = tot[k] + v
        accs.append((ids, out[k_out][:n], out[k_out + 1][:n], out[k_out + 2][:n]))
    sw = mx.maximum(tot['w'], 1e-6)
    main = tot['main'] / sw
    val = tot['val'] / sw
    dmse = tot['dsum'] / mx.maximum(tot['dn'], 1)
    heads = {'danger_early': tot['esum'] / mx.maximum(tot['dn'], 1), 'strike': tot['xsum'] / mx.maximum(2 * tot['xn'], 1),
             'value': tot['vsum'] / mx.maximum(tot['vn'], 1), 'value_win_acc': tot['v_win_ok'] / mx.maximum(tot['vn'], 1),
             'value_phi_mse': tot['v_phi_se'] / mx.maximum(tot['vn'], 1)}
    loss = main + 0.5 * val + 0.5 * dmse + aux_w * aux_tot + head_w * (heads['danger_early'] + heads['strike']) + value_w * heads['value']
    parts = aux_parts
    parts.update(heads)
    return loss, main, aux_tot, parts, accs


def evaluate(model, device_of, plays, tm, bs=256):
    """Held-out accuracy with the memory unrolled over whole plays (no dropout), by family; and the aux numbers.
    `device_of(ids)`: the DeviceM of those plays (streamed: one chunk at a time on the device)."""
    fam_hit = collections.defaultdict(float)
    fam_n = collections.defaultdict(int)
    tot = collections.defaultdict(float)
    n = 0
    n_act = 0.0
    aux = collections.defaultdict(float)
    steps = 0
    for s in range(0, len(plays), bs):
        dev = device_of(plays[s:s + bs])
        chunk = list(range(len(dev.plays.first)))
        plan = plan_batch(dev, chunk, 10 ** 6, 0.0, None, tm, train=False)
        loss, main, aux_tot, parts, accs = run_step(model, dev, plan, tm, 0.0)
        mx.eval(loss, main, aux_tot, parts)
        steps += 1
        for k, v in parts.items():
            aux[k] += float(v.item()) if hasattr(v, 'item') else float(v)
        k_dec = 0
        for ids, ap, aa, has in accs:
            ap, aa, has = ap.tolist(), aa.tolist(), has.tolist()
            for j, i in enumerate(ids):
                both = ap[j] * (aa[j] if has[j] > 0 else 1.0)
                f = dev.families[dev.fam[i]]
                fam_hit[f] += both
                fam_n[f] += 1
                tot['acc_ptr'] += ap[j]
                tot['acc_act'] += aa[j] * has[j]
                tot['acc_both'] += both
                n_act += has[j]
                n += 1
                k_dec += 1
        tot['main'] += main.item() * k_dec
    ev = {'main': tot['main'] / max(n, 1), 'acc_ptr': tot['acc_ptr'] / max(n, 1), 'acc_act': tot['acc_act'] / max(n_act, 1), 'acc_both': tot['acc_both'] / max(n, 1), 'decisions': n}
    ev.update({k: v / max(steps, 1) for k, v in aux.items()})
    return ev, {f: fam_hit[f] / fam_n[f] for f in sorted(fam_n)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--train', nargs='+', required=True)
    ap.add_argument('--hold', nargs='+', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--epochs', type=float, default=6)
    ap.add_argument('--plays', type=int, default=96, help='plays per step')
    ap.add_argument('--lr', type=float, default=1e-3)
    ap.add_argument('--aux', type=float, default=0.3, help='weight of the auxiliary head on the teacher state')
    ap.add_argument('--mem-drop', type=float, default=0.1)
    ap.add_argument('--curriculum', default='4:0.15,8:0.15,999:0.7', help='turns:share of the steps, in order')
    ap.add_argument('--lost-weight', type=float, default=0.1)
    ap.add_argument('--best-factor', type=float, default=2.0)
    ap.add_argument('--up', default='', help='families whose decisions weigh --up-factor more ("siege" = the siege set)')
    ap.add_argument('--up-factor', type=float, default=2.0)
    ap.add_argument('--eval-every', type=int, default=1000)
    ap.add_argument('--eval-plays', type=int, default=4, help='evaluate on one held-out play in N during training (all at the end)')
    ap.add_argument('--log-every', type=int, default=100)
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--init', default=None)
    ap.add_argument('--max-minutes', type=float, default=18.0, help='stop (and evaluate, export) after this long: one GPU job <= 20 min')
    ap.add_argument('--cpu', action='store_true')
    ap.add_argument('--d', type=int, default=64)
    ap.add_argument('--layers', type=int, default=3)
    ap.add_argument('--heads', type=int, default=4)
    ap.add_argument('--skip', nargs='*', default=[], help='PATH=fam,fam: leave out those families of that train file ("siege" = the siege set)')
    ap.add_argument('--map', action='store_true', help='variant MB: the map encoder once per turn')
    ap.add_argument('--map-ch', type=int, default=32)
    ap.add_argument('--history', action='store_true', help='variant MH: the history window of facts, linked to the plan')
    ap.add_argument('--every-nth-play', type=int, default=1)
    ap.add_argument('--aux-heads', action='store_true', help='variant MBA: MB with the price input and the auxiliary heads (needs --map)')
    ap.add_argument('--head-w', type=float, default=0.2, help='MBA: weight of the auxiliary heads (decays to 0 over the second half)')
    ap.add_argument('--value-w', type=float, default=0.5, help='MBA: weight of the value head (win, capture, Phi 3 turns on)')
    ap.add_argument('--awi', type=float, default=0.0, help='MBA: advantage-weighted imitation, beta (0: plain)')
    ap.add_argument('--resume', action='store_true', help='continue OUT from its last checkpoint (OUT/state.json: the step; OUT/opt.safetensors: the optimizer)')
    ap.add_argument('--new-lr-mult', type=float, default=1.0, help='MBH: the history parameters (model_mb.HX_PARAMS) learn at this multiple of --lr')
    ap.add_argument('--warmup', type=int, default=20, help='resumed without the optimizer state: lr ramps up over these steps')
    ap.add_argument('--mem-gb', type=float, default=12.0, help='MLX memory limit (match the broker --ram reservation); cache 2 GB')
    a = ap.parse_args()
    mx.set_memory_limit(int(a.mem_gb * 2 ** 30))
    mx.set_cache_limit(2 * 2 ** 30)
    from .train import SIEGE
    if a.cpu:
        mx.set_default_device(mx.cpu)
    os.makedirs(a.out, exist_ok=True)
    mx.random.seed(a.seed)
    rng = random.Random(a.seed)
    t0 = time.time()
    skip = {}
    for spec in a.skip:
        path, fams = spec.split('=', 1)
        out = set()
        for f in fams.split(','):
            out |= SIEGE if f == 'siege' else {f}
        skip[path] = out
    train = Stream(a.train, a.every_nth_play, skip)
    hold = Stream(a.hold, a.every_nth_play)
    best, table = best_teachers(train.metas)
    json.dump({'best': best, 'wins': table, 'lost_weight': a.lost_weight, 'best_factor': a.best_factor}, open(os.path.join(a.out, 'teachers.json'), 'w'), indent=1)
    tm = max(train.t_max, hold.t_max)
    cm = 128
    print(f'indexed {train.decisions} train / {hold.decisions} held-out decisions of {len(train)} / {len(hold)} plays in {time.time() - t0:.0f} s; T {tm}', flush=True)
    up = set()
    for f in filter(None, a.up.split(',')):
        up |= SIEGE if f == 'siege' else {f}

    def device_of(stream, ids, up):
        """The DeviceM of these plays with their decision weights (the family up-weighting on training plays)."""
        flat = stream.flat(ids)
        weights = decision_weights(flat, best, a.lost_weight, a.best_factor)
        if up:
            weights = [w * (a.up_factor if flat.plays[p]['family'] in up else 1.0) for w, p in zip(weights, flat.play)]
        weights = [0.0 if pt < -1 else w for w, pt in zip(weights, flat.ptr)]
        return DeviceM(flat, weights, tm, cm)

    hold_device = lambda ids: device_of(hold, ids, set())
    # The promotion vocabulary: 400 (every recording so far stays below; a larger id fails the gather loudly in MLX).
    promos = 400
    shape = dict(d=a.d, heads=a.heads, layers=a.layers, ff=2 * a.d, promos=promos)
    model = ((MBHNet if a.history else MBANet) if a.aux_heads else MBNet)(map_ch=a.map_ch, **shape) if a.map else (MHNet(**shape) if a.history else MemNet(**shape))
    model.hand_init()
    if a.init:
        # A net without memory (A / AH) starts the shared parts; an M net everything; an M net starts MH's shared
        # parts (a tensor of another shape, the plan's GRU input, stays as drawn).
        own = dict(nn.utils.tree_flatten(model.parameters()))
        init = {k: v for k, v in mx.load(a.init).items() if k in own and own[k].shape == v.shape}
        model.load_weights(list(init.items()), strict=False)
    mx.eval(model.parameters())
    n_params = sum(v.size for _, v in nn.utils.tree_flatten(model.parameters()))
    labelled = train.labelled
    plays_all = list(range(len(train)))
    steps = int(a.epochs * len(plays_all) / a.plays)
    sched = optim.cosine_decay(a.lr, steps, a.lr * 0.05)
    multi = isinstance(model, MBHNet) and a.new_lr_mult != 1.0
    sched_h = optim.cosine_decay(a.lr * a.new_lr_mult, steps, a.lr * a.new_lr_mult * 0.05)
    start = 0
    best_both = -1.0
    state_path = os.path.join(a.out, 'state.json')
    opt_path = os.path.join(a.out, 'opt.safetensors')
    if a.resume:
        state = json.load(open(state_path))
        start, best_both = state['step'], state['best_both']
        model.load_weights(os.path.join(a.out, 'last.safetensors'))
        if not os.path.exists(opt_path):
            # No optimizer state (a run of the code before checkpoints): fresh moments, the lr ramped up again.
            base, base_h, w0 = sched, sched_h, start
            sched = lambda s: base(s) * mx.minimum(1.0, (mx.array(s).astype(mx.float32) - w0) / a.warmup)
            sched_h = lambda s: base_h(s) * mx.minimum(1.0, (mx.array(s).astype(mx.float32) - w0) / a.warmup)
    opt = optim.AdamW(learning_rate=sched, weight_decay=1e-4)
    if multi:
        # MBH: the history's new parameters (zero / freshly drawn) in their own group at --new-lr-mult x the lr.
        opt = optim.MultiOptimizer([optim.AdamW(learning_rate=sched_h, weight_decay=1e-4), opt], [lambda path, _: path.split('.')[0] in HX_PARAMS])
    if a.resume:
        opt.init(model.trainable_parameters())
        if os.path.exists(opt_path):
            opt.state = nn.utils.tree_unflatten(list(mx.load(opt_path).items()))
        else:
            for o in (opt.optimizers if multi else [opt]):
                o.state['step'] = mx.array(start, mx.uint64)
        rng = random.Random(a.seed * 1000003 + start)  # a resumed job draws a new play order
        print(f'resumed at step {start} (best held-out joint accuracy {best_both:.4f}; optimizer state {"loaded" if os.path.exists(opt_path) else "fresh, lr warm-up " + str(a.warmup)})', flush=True)
    cur = []
    acc_share = 0.0
    for spec in a.curriculum.split(','):
        L, share = spec.split(':')
        acc_share += float(share)
        cur.append((int(acc_share * steps), int(L)))
    hold_plays = list(range(len(hold)))
    hold_eval = hold_plays[::a.eval_plays]
    config = {'run': os.path.basename(os.path.normpath(a.out)), 'variant': 'M (learned memory)', 'plays_per_step': a.plays, 'lr': a.lr, 'epochs': a.epochs, 'steps': steps,
              'schedule': f'AdamW (weight decay 1e-4), cosine decay to 0.05 lr over {steps} steps, grad clip 1.0' + (f'; history parameters {sorted(HX_PARAMS)} at {a.new_lr_mult} x lr' if multi else ''), 'aux_weight': a.aux, 'mem_drop': a.mem_drop, 'curriculum': cur,
              'parameters': n_params, 'T': tm, 'C': cm, 'promos': promos, 'init': a.init or 'hand_init', 'best_teachers': best, 'labelled_decisions': labelled,
              'decision_weight': {'won_play': 1.0, 'lost_play': a.lost_weight, 'best_teacher_factor': a.best_factor, 'dagger_label': 2.0, 'up_families': sorted(up), 'up_factor': a.up_factor if up else 1.0},
              'losses': {'pointer_ce': 1.0, 'action_ce': 1.0, 'value': 0.5, 'danger_mse': 0.5, 'aux_teacher_state': a.aux},
              'train': {'files': a.train, 'decisions': train.decisions, 'plays': len(train)}, 'hold': {'files': a.hold, 'decisions': hold.decisions, 'plays': len(hold)}, 'device': str(mx.default_device())}
    if not a.resume:
        json.dump(config, open(os.path.join(a.out, 'config.json'), 'w'), indent=1)
    curves = open(os.path.join(a.out, 'curves.jsonl'), 'a' if a.resume else 'w')
    print(f'{n_params} parameters, {steps} steps of {a.plays} plays; curriculum {cur}', flush=True)

    def loss_fn(model, dev, plan, head_w):
        loss, main, aux, parts, accs = run_step(model, dev, plan, tm, a.aux, head_w=head_w, value_w=a.value_w, awi=a.awi)
        return loss, (main, aux)

    grad_fn = nn.value_and_grad(model, loss_fn)
    order = []
    seen_dec = 0
    log = collections.defaultdict(float)
    log_n = 0
    log_t = 0.0
    log_dec = 0
    t_train = 0.0
    stopped = False
    for s in range(start + 1, steps + 1):
        L = next((l for upto, l in cur if s <= upto), cur[-1][1])
        if not order:
            order = plays_all[:]
            rng.shuffle(order)
        chunk, order = order[:a.plays], order[a.plays:]
        ts = time.time()
        dev = device_of(train, chunk, up)
        plan = plan_batch(dev, list(range(len(chunk))), L, a.mem_drop, rng, tm)
        n_dec = sum(len(g[0]) for g in plan[3].values())
        # The auxiliary heads' weight: full over the first half of the run, then down to 0 at its end.
        head_w = a.head_w * min(1.0, 2.0 * (1.0 - s / steps))
        (loss, (main, aux)), grads = grad_fn(model, dev, plan, head_w)
        grads, norm = optim.clip_grad_norm(grads, 1.0)
        opt.update(model, grads)
        mx.eval(model.parameters(), opt.state, loss, main, aux, norm)
        dt = time.time() - ts
        t_train += dt
        log_t += dt
        seen_dec += n_dec
        log_dec += n_dec
        log['loss'] += loss.item()
        log['main'] += main.item()
        log['aux'] += aux.item()
        log['norm'] += norm.item()
        log_n += 1
        is_eval = s % a.eval_every == 0 or s == steps
        if time.time() - t0 > a.max_minutes * 60:
            is_eval, stopped = True, True
        if s % a.log_every == 0 or is_eval:
            rec = {'step': s, 'L': L, 'epoch': s * a.plays / len(plays_all), **{k: v / log_n for k, v in log.items()}, 'lr': float(sched(s)), 'dps': log_dec / max(log_t, 1e-9), 'dps_mean': seen_dec / t_train, 'wall_s': time.time() - t0}
            if is_eval:
                ev, by_family = evaluate(model, hold_device, hold_plays if (stopped or s == steps) else hold_eval, tm)
                rec['hold'] = ev
                rec['hold_by_family'] = by_family
            log = collections.defaultdict(float)
            log_n, log_t, log_dec = 0, 0.0, 0
            curves.write(json.dumps(rec) + '\n')
            curves.flush()
            print(json.dumps({k: v for k, v in rec.items() if k != 'hold_by_family'}), flush=True)
            if is_eval:
                model.save_weights(os.path.join(a.out, 'last.safetensors'))
                mx.save_safetensors(opt_path, dict(nn.utils.tree_flatten(opt.state)))
                if ev['acc_both'] > best_both:
                    best_both = ev['acc_both']
                    model.save_weights(os.path.join(a.out, 'weights.safetensors'))
                    export(model, os.path.join(a.out, 'army-net.bin'))
                json.dump({'step': s, 'steps': steps, 'best_both': best_both}, open(state_path, 'w'))
        half_epoch = int(2 * s * a.plays / len(plays_all)) > int(2 * (s - 1) * a.plays / len(plays_all))
        if half_epoch or stopped or s == steps:
            save_checkpoint(model, a.out, s * a.plays / len(plays_all))
        if stopped:
            print(f'stopped at step {s} of {steps} (the {a.max_minutes} min limit of one GPU job)', flush=True)
            break
    print(f'done in {time.time() - t0:.0f} s; best held-out joint accuracy {best_both:.4f}', flush=True)


if __name__ == '__main__':
    main()
