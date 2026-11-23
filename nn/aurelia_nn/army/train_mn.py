"""Variant MN's training (M-HANDOFF.md, addition 4): `train_m.py`'s with the note stream carried in the unroll.

python -m aurelia_nn.army.train_mn --history --train ... --hold ... --out runs/netMN0 --init runs/netMH0/weights.safetensors

The note labels are the teacher's own state at each memory state (host side, `DeviceMN`): the stage, the turns
waited, the units that started resting (PULL_BACK_HEAL) and those that stopped (RETURN), the reason when the
stage changed (a unit lost; the stage turned to heal-rotate: the city stronger; the stage advanced; other). In a
teacher's play the stream is written from them (teacher forcing); in the net's own play (DAgger) and where the
teacher has no state, from the net's own heads (no gradient through the choice). In 10 % of the plays the stream
is masked (note dropout). The heads' losses join the auxiliary loss (weight --aux).
"""
import collections
import random

import mlx.core as mx
import mlx.nn as nn

from . import train_m as T
from .data import TRACK
from .model_m import P_WAITED, SLOTS, STAGES
from .model_mn import REASONS, STAGE_HEAL, MNNet

NOTE_DROP = 0.1
NOTE_W = 1.0
RNG = random.Random(7)


class DeviceMN(T.DeviceM):
    def __init__(self, flat, weights, tm, cm):
        super().__init__(flat, weights, tm, cm)
        n = len(flat)
        lab = {'n_known': [0] * n, 'n_stage': [0] * n, 'n_waited': [0.0] * n, 'n_pull': [0] * n, 'n_ret': [0] * n, 'n_reason': [-1] * n, 'n_teacher': [0] * n}
        for p, seen in enumerate(self.plays.ms):
            teacher = flat.plays[p]['player'] != 'army-net'
            prev = None
            for i in seen:
                tr = flat.track[i * TRACK:(i + 1) * TRACK]
                if tr[0] < 0:
                    prev = None
                    continue
                stage, waited, rest = tr[2], tr[1], tr[3] & 0xFFFF
                lab['n_known'][i] = 1
                lab['n_teacher'][i] = int(teacher)
                lab['n_stage'][i] = stage
                lab['n_waited'][i] = float(waited)
                if prev is not None:
                    pst, prest, pcount = prev
                    lab['n_pull'][i] = rest & ~prest
                    lab['n_ret'][i] = prest & ~rest
                    if stage != pst:
                        count = sum(1 for s in self.slot_list(i) if s >= 0)
                        lab['n_reason'][i] = 0 if count < pcount else (1 if stage == STAGE_HEAL else (2 if stage > pst else 3))
                else:
                    lab['n_pull'][i] = rest
                prev = (stage, rest, sum(1 for s in self.slot_list(i) if s >= 0))
        for k, v in lab.items():
            setattr(self, k, mx.array(v, mx.float32 if k == 'n_waited' else mx.int32))

    def slot_list(self, i):
        o, t = self.tok_off_list[i], self.t_list[i]
        return self.slot_host[o:o + t]

    def batch_m(self, idx, tm, cm):
        b = super().batch_m(idx, tm, cm)
        for k in ('n_known', 'n_stage', 'n_waited', 'n_pull', 'n_ret', 'n_reason', 'n_teacher'):
            b[k] = getattr(self, k)[idx]
        return b


_orig_init = T.DeviceM.__init__


def _host_slots(self, flat, weights, tm, cm):
    self.slot_host = memoryview(flat.slot).cast('h')
    self.tok_off_list = list(flat.tok_off)
    _orig_init(self, flat, weights, tm, cm)


def bits(x):
    return ((x[:, None] >> mx.arange(SLOTS)[None, :]) & 1).astype(mx.float32)


def unroll_one_mn(model, b, valid, rs, um, pm, alive, ns, drop):
    hfull = model.body_mn(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm, b['hist'], b['hmask'], b['hslot'], b['hplan'], ns)
    um2, pm2, al2, lg = model.step_mh(hfull, b['tok'], b['slot'], b['tmask'], um, pm, alive, b['hist'], b['hmask'])
    bb = dict(b)
    bb['track'] = mx.where(valid[:, None], b['track'], -1)
    bb['tis'] = mx.where(valid, b['tis'], -1.0)
    bb['eta'] = mx.where(valid, b['eta'], -1.0)
    bb['tmask'] = b['tmask'] & valid[:, None]
    a, parts = T.aux_losses(model, bb, um2, pm2, lg)
    # The note heads, and their losses on the teacher's notes.
    has = (b['slot'] >= 0) & bb['tmask']
    sl = mx.maximum(b['slot'], 0)
    onehot = ((sl[..., None] == mx.arange(SLOTS)[None, None, :]) & has[..., None]).astype(mx.float32)
    cover = mx.minimum(mx.sum(onehot, axis=1), 1.0)
    event = mx.minimum(mx.sum(alive * (1.0 - cover), axis=-1), 1.0)
    stage_lg, reason_lg, pull_lg, ret_lg = model.note_heads(hfull, b['tok'], b['slot'], bb['tmask'], um2, pm, pm2, event, mx.zeros(event.shape))
    known = (b['n_known'] > 0) & valid
    kf = known.astype(mx.float32)
    tok_pull = mx.take_along_axis(bits(b['n_pull']), sl, axis=1)
    tok_ret = mx.take_along_axis(bits(b['n_ret']), sl, axis=1)
    hm = has.astype(mx.float32) * kf[:, None]
    l_pull = mx.sum(hm * nn.losses.binary_cross_entropy(pull_lg, tok_pull, with_logits=True, reduction='none')) / mx.maximum(mx.sum(hm), 1.0)
    l_ret = mx.sum(hm * nn.losses.binary_cross_entropy(ret_lg, tok_ret, with_logits=True, reduction='none')) / mx.maximum(mx.sum(hm), 1.0)
    rk = (known & (b['n_reason'] >= 0)).astype(mx.float32)
    l_reason = mx.sum(rk * (mx.logsumexp(reason_lg, axis=1) - mx.take_along_axis(reason_lg, mx.maximum(b['n_reason'], 0)[:, None], axis=1)[:, 0])) / mx.maximum(mx.sum(rk), 1.0)
    a = a + NOTE_W * (l_pull + l_ret + l_reason)
    # The notes written: the teacher's in its plays, the net's own elsewhere.
    own_stage = mx.stop_gradient(mx.argmax(stage_lg, axis=1))
    stage_i = mx.where(known & (b['n_teacher'] > 0), b['n_stage'], own_stage)
    stage_oh = (stage_i[:, None] == mx.arange(STAGES)[None]).astype(mx.float32)
    force = (known & (b['n_teacher'] > 0))
    own_pull = mx.stop_gradient(mx.minimum(mx.einsum('bt,bts->bs', (pull_lg > 0).astype(mx.float32), onehot), 1.0))
    own_ret = mx.stop_gradient(mx.minimum(mx.einsum('bt,bts->bs', (ret_lg > 0).astype(mx.float32), onehot), 1.0))
    pull_s = mx.where(force[:, None], bits(b['n_pull']), own_pull)
    ret_s = mx.where(force[:, None], bits(b['n_ret']), own_ret)
    own_reason = mx.stop_gradient(mx.argmax(reason_lg, axis=1))
    reason_i = mx.where(force & (b['n_reason'] >= 0), b['n_reason'], own_reason)
    reason_oh = (reason_i[:, None] == mx.arange(len(REASONS))[None]).astype(mx.float32)
    waited = mx.where(force, b['n_waited'], mx.maximum(mx.stop_gradient(pm2[:, 0, P_WAITED]) * 10.0, 0.0))
    free = mx.tanh(model.free(pm2[:, 0]))
    ns2 = model.write_notes(ns, stage_oh, waited, reason_oh, pull_s, ret_s, free, valid)
    um = mx.where(valid[:, None, None], um2, um)
    pm = mx.where(valid[:, None, None], pm2, pm)
    alive = mx.where(valid[:, None], al2, alive)
    p0 = mx.broadcast_to(model.plan0[None], pm.shape)
    um = mx.where(rs[:, None, None], 0.0, um)
    pm = mx.where(rs[:, None, None], p0, pm)
    alive = mx.where(rs[:, None], 0.0, alive)
    fresh = model.initial_notes(um.shape[0])
    ns2 = {k: mx.where(rs.reshape((-1,) + (1,) * (v.ndim - 1)), fresh[k], v) for k, v in ns2.items()}
    parts = tuple(parts[k] for k in T.AUX_KEYS) + (l_pull, l_ret, l_reason)
    return um, pm, alive, ns2, a, parts


def masked_notes(ns, drop):
    """Note dropout: the stream of the dropped plays reads as empty."""
    return {k: mx.where(drop.reshape((-1,) + (1,) * (v.ndim - 1)), (mx.full(v.shape, 99.0) if k == 'change_age' else mx.zeros(v.shape)), v) for k, v in ns.items()}


def dec_one_mn(model, b, um, pm, ns):
    hfull = model.body_mn(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm, b['hist'], b['hmask'], b['hslot'], b['hplan'], ns)
    t = b['tok'].shape[1]
    h = hfull[:, :t]
    pl = model.pointer_logits(h, b['sel'])
    al = model.action_logits(h, b['unit'], b['cand'], b['promo'], b['cmask'])
    ce_ptr = mx.logsumexp(pl, axis=1) - mx.take_along_axis(pl, b['ptr'][:, None], axis=1)[:, 0]
    ce_act = (mx.logsumexp(al, axis=1) - mx.take_along_axis(al, b['act'][:, None], axis=1)[:, 0]) * b['has_act']
    v = model.value(h)
    bce = nn.losses.binary_cross_entropy(v[:, 0], b['win'], with_logits=True, reduction='none')
    lost = (mx.sigmoid(v[:, 1]) - b['lost']) ** 2
    dg = model.danger(h)
    dmask = (b['danger'] >= 0) & b['sel']
    w = b['w']
    return (mx.sum(w * (ce_ptr + ce_act)), mx.sum(w * (bce + lost)), mx.sum(w), mx.sum(mx.where(dmask, (dg - b['danger']) ** 2, 0.0)), mx.sum(dmask),
            (mx.argmax(pl, axis=1) == b['ptr']).astype(mx.float32), (mx.argmax(al, axis=1) == b['act']).astype(mx.float32), b['has_act'])


NOTE_KEYS = ('n_known', 'n_stage', 'n_waited', 'n_pull', 'n_ret', 'n_reason', 'n_teacher')
_C = {}


def run_step_mn(model, dev, plan, tm, aux_w, with_aux=True, compile_=True):
    R, ms, reset, groups = plan[:4]
    B = len(reset)
    train = aux_w > 0
    um, pm, alive = model.initial(B)
    ns = model.initial_notes(B)
    Us, Ps, NS = [um], [pm], [ns]
    aux_tot = mx.array(0.0)
    aux_parts = collections.defaultdict(float)
    reset_a = mx.array(reset, mx.int32)
    drop = mx.array([train and RNG.random() < NOTE_DROP for _ in range(B)])
    if 'unroll' not in _C:
        _C['unroll'] = mx.compile(lambda b, v, rs, um, pm, al, ns, dr: unroll_one_mn(model, b, v, rs, um, pm, al, ns, dr), inputs=[model.state])
        _C['dec'] = mx.compile(lambda b, um, pm, ns: dec_one_mn(model, b, um, pm, ns), inputs=[model.state])
    for r in range(R):
        idx = mx.array(ms[r], mx.int32)
        valid = idx >= 0
        full = dev.batch_m(mx.maximum(idx, 0), tm, 1)
        b = {k: full[k] for k in T.UNROLL_KEYS + NOTE_KEYS}
        T.hist_mask(b)
        p0s = mx.stack([P[:, 0] for P in Ps[::-1][:4]] + [Ps[0][:, 0]] * max(0, 4 - len(Ps)), axis=1)
        b['hplan'] = p0s[mx.arange(B)[:, None], mx.minimum(full['hage'], 3)]
        um, pm, alive, ns, a, parts = _C['unroll'](b, valid, reset_a == r + 1, um, pm, alive, masked_notes(ns, drop) if False else ns, drop)
        aux_tot = aux_tot + a / R
        for k, v in zip(T.AUX_KEYS + ('note_pull', 'note_ret', 'note_reason'), parts):
            aux_parts[k] = aux_parts[k] + v / R
        Us.append(um)
        Ps.append(pm)
        NS.append(ns)
    Us = mx.stack(Us)
    Ps = mx.stack(Ps)
    NSs = {k: mx.stack([n[k] for n in NS]) for k in NS[0]}
    tot = collections.defaultdict(lambda: mx.array(0.0))
    accs = []
    for (tg, cg), (ids, cols, turns) in sorted(groups.items()):
        n = len(ids)
        pad = (-n) % T.PAD
        idx = mx.array(ids + [ids[0]] * pad, mx.int32)
        full = dev.batch_m(idx, tg, cg)
        b = {k: full[k] for k in T.DEC_KEYS}
        T.hist_mask(b)
        live = mx.arange(n + pad) < n
        b['w'] = mx.where(live, b['w'], 0.0)
        b['danger'] = mx.where(live[:, None], b['danger'], -1.0)
        tt = mx.array(turns + [turns[0]] * pad, mx.int32)
        cc = mx.array(cols + [cols[0]] * pad, mx.int32)
        P0 = Ps[:, :, 0]
        b['hplan'] = P0[mx.maximum(tt[:, None] - full['hage'], 0), cc[:, None]]
        nsd = {k: v[tt, cc] for k, v in NSs.items()}
        nsd = masked_notes(nsd, drop[cc]) if train else nsd
        if T.ABLATE.get('notes'):
            nsd = masked_notes(nsd, mx.ones((n + pad,), dtype=mx.bool_))
        out = _C['dec'](b, Us[tt, cc], Ps[tt, cc], nsd)
        for k, v in zip(('main', 'val', 'w', 'dsum', 'dn'), out[:5]):
            tot[k] = tot[k] + v
        accs.append((ids, out[5][:n], out[6][:n], out[7][:n]))
    sw = mx.maximum(tot['w'], 1e-6)
    main = tot['main'] / sw
    val = tot['val'] / sw
    dmse = tot['dsum'] / mx.maximum(tot['dn'], 1)
    loss = main + 0.5 * val + 0.5 * dmse + aux_w * aux_tot
    return loss, main, aux_tot, dict(aux_parts), accs


def main():
    T.DeviceM.__init__ = _host_slots
    T.DeviceM = DeviceMN
    T.MHNet = MNNet
    T.run_step = run_step_mn
    T.main()


if __name__ == '__main__':
    main()
