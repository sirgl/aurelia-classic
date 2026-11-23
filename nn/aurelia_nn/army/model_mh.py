"""Variant MH of the army net (M-HANDOFF.md, addition 3): M plus the history window of FACTS (our commands and
their outcomes, the enemy's events; `crates/qa/src/army_net/observe.rs`, F_HIST numbers each) linked to the plan.

- The history enters the body as keys / values only: each layer's attention of the tokens and the plan tokens also
  reads the history (`norm1` and the layer's k / v on the history embedding), with a learned bias per layer and head
  and one for the pair (a token, the history of the same unit): the pointer to the unit.
- Each history token's embedding adds `hp(P_0 of the turn it was taken)`: the operation plan in force then.
- The plan update reads `progress` (the objective city's hit-point share now, damage dealt to the city, damage taken,
  units lost over the window) and its hysteresis gate adds `w_p * sigmoid(3 P_ENGAGED) * relu(theta_p - (P_CITY0 -
  city share))`: an assault that has not taken theta_p of the city's hit points since the plan was taken stalls,
  and the plan may change. P_CITY0: the city's share when the plan was taken (hand-set copy, held by the hysteresis).

`crates/qa/src/army_net/model.rs` is the same arithmetic in Rust.
"""
import math

import mlx.core as mx
import mlx.nn as nn

from .data import F_HIST, F_TOK
from .model import NEG
from .model_m import P_ENGAGED, PLANS, SLOTS, MemNet, gru

H_MAX = 32
P_CITY0 = 12
PROGRESS = 4


class MHNet(MemNet):
    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos)
        self.config['history'] = {'f_hist': F_HIST, 'h_max': H_MAX, 'progress': PROGRESS}
        self.h1 = nn.Linear(F_HIST, d)
        self.h2 = nn.Linear(d, d)
        self.hp = nn.Linear(d, d)
        self.hp.weight = self.hp.weight * 0.1
        self.hist_bias = mx.full((layers, heads), -1.0)
        self.same_bias = mx.zeros((layers, heads))
        x_in = d + F_TOK + PROGRESS
        s = 1.0 / math.sqrt(x_in)
        self.gp_wx = mx.random.uniform(-s, s, (x_in, 3 * d)) * 0.3
        self.stall = mx.array([[0.3, 5.0]] * PLANS)  # [plan][theta_p, w_p]

    def hand_init(self):
        super().hand_init()
        d = self.config['d']
        H = d
        r, z, n = 0, H, 2 * H
        wx = mx.array(self.gp_wx)
        bx = mx.array(self.gp_bx)
        wh = mx.array(self.gp_wh)
        for g in (r, z, n):
            wx[:, g + P_CITY0] = 0.0
            wh[:, g + P_CITY0] = 0.0
        # The city's share when the plan was taken: n = tanh(progress[0]) replaced at each commitment (the gate holds it).
        wx[d + F_TOK + 0, n + P_CITY0] = 1.0
        bx[z + P_CITY0] = -6.0
        self.gp_wx, self.gp_bx, self.gp_wh = wx, bx, wh
        self.stall = mx.array([[0.3, 5.0]] * PLANS)

    def hist_embed(self, hist, hplan):
        return self.h2(nn.relu(self.h1(hist))) + self.hp(hplan)

    def body_mh(self, tok, dist, tmask, slot, um, pm, hist, hmask, hslot, hplan, extra=None):
        b, t, _ = tok.shape
        h = self.e2(nn.relu(self.e1(tok)))
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        x = mx.concatenate([h, pm], axis=1)
        mask = mx.concatenate([tmask, mx.ones((b, PLANS), dtype=mx.bool_)], axis=1)
        is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
        pair_plan = is_plan[:, None] | is_plan[None, :]
        dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
        qslot = mx.concatenate([slot, mx.full((b, PLANS), -1, dtype=slot.dtype)], axis=1)
        he = self.hist_embed(hist, hplan)
        if extra is not None:
            # MN: the notes join the history's keys and values.
            ne, nmask, nslot = extra
            he = mx.concatenate([he, ne], axis=1)
            hmask = mx.concatenate([hmask, nmask], axis=1)
            hslot = mx.concatenate([hslot, nslot.astype(hslot.dtype)], axis=1)
        same = (qslot[:, :, None] == hslot[:, None, :]) & (hslot[:, None, :] >= 0)  # [B, T+P, H]
        for li, layer in enumerate(self.layers):
            x = self.layer_mh(layer, x, dpad, mask, pair_plan, self.plan_bias[li], he, hmask, same, self.hist_bias[li], self.same_bias[li])
        return self.norm(x)

    @staticmethod
    def layer_mh(layer, h, dist, key_mask, pair_plan, pbias, he, hmask, same, hbias, sbias):
        b, t, d = h.shape
        heads = layer.heads
        dh = d // heads
        nh = he.shape[1]
        a = layer.norm1(h)
        ah = layer.norm1(he)
        q = layer.q(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        k = layer.k(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        v = layer.v(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        kh = layer.k(ah).reshape(b, nh, heads, dh).transpose(0, 2, 1, 3)
        vh = layer.v(ah).reshape(b, nh, heads, dh).transpose(0, 2, 1, 3)
        sc = 1.0 / math.sqrt(dh)
        s = (q @ k.transpose(0, 1, 3, 2)) * sc
        bias = layer.dist_bias[:, dist].transpose(1, 0, 2, 3)
        bias = mx.where(pair_plan[None, None], pbias[None, :, None, None], bias)
        s = s + bias + mx.where(key_mask[:, None, None, :], 0.0, NEG)
        sh = (q @ kh.transpose(0, 1, 3, 2)) * sc + hbias[None, :, None, None] + mx.where(same[:, None], sbias[None, :, None, None], 0.0)
        sh = sh + mx.where(hmask[:, None, None, :], 0.0, NEG)
        w = mx.softmax(mx.concatenate([s, sh], axis=-1), axis=-1)
        ctx = (w[..., :t] @ v + w[..., t:] @ vh).transpose(0, 2, 1, 3).reshape(b, t, d)
        h = h + layer.o(ctx)
        m = layer.norm2(h)
        return h + layer.f2(nn.relu(layer.f1(m)))

    @staticmethod
    def progress(tok, tmask, hist, hmask):
        """[B, 4]: the objective city's hit-point share now (1 without one), damage dealt to the city, damage taken,
        units lost over the window (history numbers 14, 19; 15; 17)."""
        is_obj = (tok[..., 43] > 0.5) & (tok[..., 3] > 0.5) & tmask
        city = mx.where(mx.any(is_obj, axis=1), mx.sum(mx.where(is_obj, tok[..., 7], 0.0), axis=1), 1.0)
        hm = hmask.astype(mx.float32)
        dealt = mx.sum(hm * hist[..., 14] * hist[..., 19], axis=1)
        taken = mx.sum(hm * hist[..., 15], axis=1)
        lost = mx.sum(hm * hist[..., 17], axis=1)
        return mx.stack([city, dealt, taken, lost], axis=1)

    def step_mh(self, hfull, tok, slot, tmask, um, pm, alive, hist, hmask):
        b, t, _ = tok.shape
        d = self.config['d']
        ht = hfull[:, :t]
        x = mx.concatenate([ht, tok], axis=-1)
        has = (slot >= 0) & tmask
        sl = mx.maximum(slot, 0)
        us = um[mx.arange(b)[:, None], sl]
        un = gru(x, us, self.gu_wx, self.gu_bx, self.gu_wh, self.gu_bh)
        onehot = ((sl[..., None] == mx.arange(SLOTS)[None, None, :]) & has[..., None]).astype(mx.float32)
        cover = mx.minimum(mx.sum(onehot, axis=1), 1.0)
        um2 = um * (1.0 - cover[..., None]) + mx.einsum('bts,btk->bsk', onehot, un)
        q = self.pq(pm)
        kk = self.pk(ht)
        logits = (q @ kk.transpose(0, 2, 1)) * (1.0 / math.sqrt(d)) + mx.where(tmask[:, None, :], 0.0, NEG)
        att = mx.softmax(logits, axis=-1)
        pooled = att @ ht
        prog = self.progress(tok, tmask, hist, hmask)
        xo = mx.concatenate([pooled, mx.broadcast_to(tok[:, :1], (b, PLANS, tok.shape[-1])), mx.broadcast_to(prog[:, None, :], (b, PLANS, PROGRESS))], axis=-1)
        pn = gru(xo, pm, self.gp_wx, self.gp_bx, self.gp_wh, self.gp_bh)
        delta = mx.sqrt(mx.sum((pn - pm) ** 2, axis=-1) + 1e-12) / (mx.sqrt(mx.sum(pm ** 2, axis=-1) + 1e-12) + 1e-3)
        event = mx.minimum(mx.sum(alive * (1.0 - cover), axis=-1), 1.0)
        th, ka, we = self.hyst[:, 0], self.hyst[:, 1], self.hyst[:, 2]
        tp, wp = self.stall[:, 0], self.stall[:, 1]
        stall = mx.sigmoid(3.0 * pm[..., P_ENGAGED]) * nn.relu(tp[None] - (pm[..., P_CITY0] - mx.tanh(prog[:, :1])))
        c = mx.sigmoid(ka[None] * (delta - th[None]) + we[None] * event[:, None] + wp[None] * stall)
        pm2 = pm + c[..., None] * (pn - pm)
        return um2, pm2, cover, logits
