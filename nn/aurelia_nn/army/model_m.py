"""Variant M of the army net (work/nets/army-net/M-HANDOFF.md): A plus a learned memory kept between our turns.

- Per-unit memory u_s in R^16 for army slot s (the unit's index in the army, < SLOTS), zero at the play's start.
  A unit's token adds `um(u_s)`. At our turn's end (a memory-state decision) every army token updates its slot:
  u_s <- GRU16([H_i, f_i], u_s) (H_i: the token's final state, f_i: its raw numbers).
- Plan tokens: PLANS vectors P_k in R^d, learned initial values, appended to the token sequence (attention bias of
  any pair with a plan token: a learned scalar per layer and head). At our turn's end
  x_k = [attention(q = P_k, K = V = H of the tokens), f_objective], P~_k = GRU_d(x_k, P_k) and, with hysteresis,
  P_k <- P_k + c_k (P~_k - P_k), c_k = sigmoid(kappa_k (|P~_k - P_k| / |P_k| - theta_k) + w_k * event)
  (theta 0.15, kappa 20, w 10 at the start: the plan changes when the new one differs by more than 15 % or on an
  event: a unit of the army lost since the last turn end).
- Named, hand-initialised dimensions (`hand_init`, each with its why), then trained; the auxiliary head reads them
  and is supervised by the teacher's own state (the staged assault's track) during training only.

`crates/qa/src/army_net/model.rs` is the same arithmetic in Rust.
"""
import math

import mlx.core as mx
import mlx.nn as nn

from . import mixed
from .data import F_TOK
from .model import D_MAX, NEG, ArmyNet

SLOTS = 16
U = 16
PLANS = 4
AIMS = 8
STAGES = 6
STAGE_NAMES = ['gather', 'march', 'stage', 'assault', 'heal-rotate', 'withdraw']

# Named dimensions of a unit's memory u_s.
U_WAITED = 0   # +0.1 a turn the unit did not strike, back to 0 on a strike (ref-planned goes after waited >= 5)
U_RESTING = 1  # +0.1 a turn the unit stays wounded (< 90 hp) and still, 0 otherwise (ref-rotate rests to 90 hp)
U_ENGAGED = 2  # latches to +1 at the first strike from the city's fire, -1 before (ref-planned's `engaged`)
U_HP = 3       # tanh(hp) at our last turn end (healing against being shot)
# Named dimensions of the plan tokens (the first is the army's operation; its aux targets).
P_STAGE = 0    # 0..5: the stage logits (gather, march, stage, assault, heal-rotate, withdraw)
P_TURNS = 6    # turns in the stage x 0.1
P_BALANCE = 7  # tanh(2 (our hp - their hp) / 800) when the plan was taken (the hysteresis keeps it)
P_ETA = 8      # expected turns to the capture / 30
P_LEFT = 9     # tanh(rounds left / 30)
P_ENGAGED = 10  # the staged assault went in
P_WAITED = 11  # turns waited for the stage x 0.1


def gru(x, h, wx, bx, wh, bh):
    """One GRU step (PyTorch's convention): gates in the order r, z, n; x [..., in], h [..., H]."""
    n_h = h.shape[-1]
    gx = x @ wx + bx
    gh = h @ wh + bh
    r = mx.sigmoid(gx[..., :n_h] + gh[..., :n_h])
    z = mx.sigmoid(gx[..., n_h:2 * n_h] + gh[..., n_h:2 * n_h])
    n = mx.tanh(gx[..., 2 * n_h:] + r * gh[..., 2 * n_h:])
    return (1 - z) * n + z * h


class MemNet(ArmyNet):
    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos, map_ch=0)
        self.config['memory'] = {'slots': SLOTS, 'u': U, 'plans': PLANS}
        x_in = d + F_TOK
        s = lambda n_in, shape: mx.random.uniform(-1.0 / math.sqrt(n_in), 1.0 / math.sqrt(n_in), shape)
        self.um = nn.Linear(U, d)
        # Small at the start, so a net without memory that starts the shared parts plays as it did.
        self.um.weight = self.um.weight * 0.1
        self.um.bias = mx.zeros((d,))
        self.plan0 = mx.random.normal((PLANS, d)) * 0.1
        # Tokens look at the plan tokens little at the start (exp(-2) of an even pair).
        self.plan_bias = mx.full((layers, heads), -2.0)
        self.gu_wx = s(x_in, (x_in, 3 * U)) * 0.3
        self.gu_bx = mx.zeros((3 * U,))
        self.gu_wh = s(U, (U, 3 * U)) * 0.3
        self.gu_bh = mx.zeros((3 * U,))
        self.pq = nn.Linear(d, d)
        self.pk = nn.Linear(d, d)
        self.gp_wx = s(x_in, (x_in, 3 * d)) * 0.3
        self.gp_bx = mx.zeros((3 * d,))
        self.gp_wh = s(d, (d, 3 * d)) * 0.3
        self.gp_bh = mx.zeros((3 * d,))
        self.hyst = mx.array([[0.15, 20.0, 10.0]] * PLANS)  # [plan][theta, kappa, w_event]
        # Readouts of the named dimensions for the auxiliary head (training only): scale, shift.
        self.aux_u = mx.array([[1.0, 0.0], [20.0, -1.0], [3.0, 0.0], [1.0, 0.0]])  # waited, resting, engaged, hp
        self.aux_p = mx.array([[5.0, 0.0], [3.0, 0.0]])  # stage logits, engaged

    def hand_init(self):
        """The named dimensions' gates and inputs, set by hand from the token numbers that carry the fact."""
        d = self.config['d']
        fx = lambda i: d + i  # the raw token number i in the GRU input
        wx = mx.array(self.gu_wx)
        bx = mx.zeros((3 * U,))
        wh = mx.array(self.gu_wh)
        bh = mx.zeros((3 * U,))
        r, z, n = 0, U, 2 * U
        named = [U_WAITED, U_RESTING, U_ENGAGED, U_HP]
        for k in named:
            for g in (r, z, n):
                wx[:, g + k] = 0.0
                wh[:, g + k] = 0.0
        # The free dimensions keep (update gate biased to keep: forget-gate bias + 2).
        for k in range(U):
            if k not in named:
                bx[z + k] = 2.0
        # waited: r = 1 while the unit has not struck (f10 = can attack), n = tanh(r (h + 0.1)), z = 0.
        wx[fx(10), r + U_WAITED] = 12.0
        bx[r + U_WAITED] = -6.0
        bx[z + U_WAITED] = -6.0
        wh[U_WAITED, n + U_WAITED] = 1.0
        bh[n + U_WAITED] = 0.1
        # resting since: r = 1 while still (f8 = moves left share) and wounded (f7 = hp < 0.9).
        wx[fx(8), r + U_RESTING] = 20.0
        wx[fx(7), r + U_RESTING] = -20.0
        bx[r + U_RESTING] = -2.0
        bx[z + U_RESTING] = -6.0
        wh[U_RESTING, n + U_RESTING] = 1.0
        bh[n + U_RESTING] = 0.1
        # engaged: keep once +1 (z from h), else n = tanh(10 (in the city's fire f36 - can attack f10) - 5).
        wh[U_ENGAGED, z + U_ENGAGED] = 12.0
        bx[z + U_ENGAGED] = -6.0
        wx[fx(36), n + U_ENGAGED] = 10.0
        wx[fx(10), n + U_ENGAGED] = -10.0
        bx[n + U_ENGAGED] = -5.0
        # hp at last turn: n = tanh(hp), z = 0.
        wx[fx(7), n + U_HP] = 1.0
        bx[z + U_HP] = -6.0
        self.gu_wx, self.gu_bx, self.gu_wh, self.gu_bh = wx, bx, wh, bh
        # The plan's GRU.
        H = d
        wx = mx.array(self.gp_wx)
        bx = mx.zeros((3 * H,))
        wh = mx.array(self.gp_wh)
        bh = mx.zeros((3 * H,))
        r, z, n = 0, H, 2 * H
        copied = [P_BALANCE, P_LEFT]
        for k in copied:
            for g in (r, z, n):
                wx[:, g + k] = 0.0
                wh[:, g + k] = 0.0
        for k in range(H):
            if k not in copied:
                bx[z + k] = 2.0
        # balance: tanh(2 (our army hp f20 - theirs f21)), replaced at every commitment (the hysteresis holds it).
        wx[fx(12 + AIMS), n + P_BALANCE] = 2.0
        wx[fx(13 + AIMS), n + P_BALANCE] = -2.0
        bx[z + P_BALANCE] = -6.0
        # rounds left: tanh(f15).
        wx[fx(7 + AIMS), n + P_LEFT] = 1.0
        bx[z + P_LEFT] = -6.0
        self.gp_wx, self.gp_bx, self.gp_wh, self.gp_bh = wx, bx, wh, bh
        self.hyst = mx.array([[0.15, 20.0, 10.0]] * PLANS)

    def initial(self, b):
        """The memory at a play's start for b plays: U [b, SLOTS, U], P [b, PLANS, d], alive [b, SLOTS]."""
        d = self.config['d']
        return mx.zeros((b, SLOTS, U)), mx.broadcast_to(self.plan0[None], (b, PLANS, d)), mx.zeros((b, SLOTS))

    def body_m(self, tok, dist, tmask, slot, um, pm):
        """tok [B, T, F], dist [B, T, T], tmask [B, T], slot [B, T] (-1 none), um [B, SLOTS, U], pm [B, PLANS, d] ->
        H [B, T + PLANS, d] (the plan tokens last)."""
        b, t, _ = tok.shape
        h = self.e2(nn.relu(self.e1(tok)))
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        x = mx.concatenate([h, pm], axis=1)
        mask = mx.concatenate([tmask, mx.ones((b, PLANS), dtype=mx.bool_)], axis=1)
        is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
        pair_plan = is_plan[:, None] | is_plan[None, :]  # [T+P, T+P]
        dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
        for li, layer in enumerate(self.layers):
            x = self.layer_m(layer, x, dpad, mask, pair_plan, self.plan_bias[li])
        return self.norm(x)

    @staticmethod
    def layer_m(layer, h, dist, key_mask, pair_plan, pbias):
        b, t, d = h.shape
        heads = layer.heads
        dh = d // heads
        a = layer.norm1(h)
        q = layer.q(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        k = layer.k(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        v = layer.v(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        s = mixed.mm(q, k.transpose(0, 1, 3, 2)) * (1.0 / math.sqrt(dh))
        bias = layer.dist_bias[:, dist].transpose(1, 0, 2, 3)  # [b, h, t, t]
        bias = mx.where(pair_plan[None, None], pbias[None, :, None, None], bias)
        s = s + bias + mx.where(key_mask[:, None, None, :], 0.0, NEG)
        w = mx.softmax(s, axis=-1)
        ctx = mixed.mm(w, v).transpose(0, 2, 1, 3).reshape(b, t, d)
        h = h + layer.o(ctx)
        m = layer.norm2(h)
        return h + layer.f2(nn.relu(layer.f1(m)))

    def step(self, *args):
        """The memory update in float32 whatever the training's precision (`mixed.exact`)."""
        with mixed.exact():
            return self._step(*args)

    def _step(self, hfull, tok, slot, tmask, um, pm, alive):
        """The memory update at our turn's end. hfull [B, T + PLANS, d] from `body_m` on this moment with (um, pm).
        Returns (um', pm', alive', plan attention logits [B, PLANS, T])."""
        b, t, _ = tok.shape
        d = self.config['d']
        ht = hfull[:, :t]
        x = mx.concatenate([ht, tok], axis=-1)
        has = (slot >= 0) & tmask
        sl = mx.maximum(slot, 0)
        us = um[mx.arange(b)[:, None], sl]
        un = gru(x, us, self.gu_wx, self.gu_bx, self.gu_wh, self.gu_bh)
        onehot = ((sl[..., None] == mx.arange(SLOTS)[None, None, :]) & has[..., None]).astype(mx.float32)  # [B, T, S]
        cover = mx.minimum(mx.sum(onehot, axis=1), 1.0)  # [B, S]
        um2 = um * (1.0 - cover[..., None]) + mx.einsum('bts,btk->bsk', onehot, un)
        q = self.pq(pm)
        kk = self.pk(ht)
        logits = (q @ kk.transpose(0, 2, 1)) * (1.0 / math.sqrt(d)) + mx.where(tmask[:, None, :], 0.0, NEG)
        att = mx.softmax(logits, axis=-1)
        pooled = att @ ht
        xo = mx.concatenate([pooled, mx.broadcast_to(tok[:, :1], (b, PLANS, tok.shape[-1]))], axis=-1)
        pn = gru(xo, pm, self.gp_wx, self.gp_bx, self.gp_wh, self.gp_bh)
        delta = mx.sqrt(mx.sum((pn - pm) ** 2, axis=-1) + 1e-12) / (mx.sqrt(mx.sum(pm ** 2, axis=-1) + 1e-12) + 1e-3)
        event = mx.minimum(mx.sum(alive * (1.0 - cover), axis=-1), 1.0)  # [B]
        th, ka, we = self.hyst[:, 0], self.hyst[:, 1], self.hyst[:, 2]
        c = mx.sigmoid(ka[None] * (delta - th[None]) + we[None] * event[:, None])
        pm2 = pm + c[..., None] * (pn - pm)
        return um2, pm2, cover, logits
