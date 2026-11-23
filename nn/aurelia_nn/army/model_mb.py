"""Variant MB of the army net (M-HANDOFF.md, Roman's line B + memory): M plus B's hex-conv map encoder, encoded ONCE
per turn (from the turn's first moment; no updates within the turn) at MAP_CH channels over the 24 x 24 window.

The encoder is B's (`model.py`: 1 x 1 projection, 4 residual blocks of two hex convolutions, dilations as B); its
output enters as in B: a token adds m_tok(M[its tile]), a candidate m_cand(M[its tile]), the objective
m_pool(mean M). The crop to the army's area of the MB design is not built here: the Rust kernels belong to the
`map-encoder` lane (vectorised conv, incremental updates); MB changes only the configuration.

`crates/qa/src/army_net/model.rs` is the same arithmetic in Rust.
"""
import mlx.core as mx
import mlx.nn as nn

from .model import DIL, M_IN, Block, map_planes
from .model_m import PLANS, MemNet

MAP_CH = 32


class MBNet(MemNet):
    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400, map_ch=MAP_CH):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos)
        self.config['map_ch'] = map_ch
        self.config['map'] = {'ch': map_ch, 'once_per_turn': True}
        self.has_map = True
        self.m_in = nn.Linear(M_IN, map_ch)
        self.blocks = [Block(map_ch, d1, d2) for d1, d2 in DIL]
        self.m_tok = nn.Linear(map_ch, d)
        self.m_cand = nn.Linear(map_ch, d)
        self.m_pool = nn.Linear(map_ch, d)

    def encode_turn(self, statics, tok, pos):
        """The turn's map [B, W*W, C] from its first moment."""
        return self.encode(map_planes(statics, tok, pos))

    def body_mbg(self, tok, dist, tmask, slot, um, pm, gtok, tok_ok, mmean):
        """M's body with the map read at the tokens' tiles (gtok [B, T, C], tok_ok) and its mean (mmean [B, C])."""
        b, t, _ = tok.shape
        h = self.e2(nn.relu(self.e1(tok)))
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        h = h + mx.where(tok_ok[..., None], self.m_tok(gtok), 0.0)
        pooled = self.m_pool(mmean)
        h = mx.concatenate([h[:, :1] + pooled[:, None, :], h[:, 1:]], axis=1)
        x = mx.concatenate([h, pm], axis=1)
        mask = mx.concatenate([tmask, mx.ones((b, PLANS), dtype=mx.bool_)], axis=1)
        is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
        pair_plan = is_plan[:, None] | is_plan[None, :]
        dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
        for li, layer in enumerate(self.layers):
            x = self.layer_m(layer, x, dpad, mask, pair_plan, self.plan_bias[li])
        return self.norm(x)

    def action_logits_g(self, h, unit, cand, promo, cmask, gcand, cand_ok):
        b = h.shape[0]
        hu = h[mx.arange(b), unit]
        h0 = h[:, 0]
        pre = self.c1(cand) + self.promo[promo] + mx.where(cand_ok[..., None], self.m_cand(gcand), 0.0)
        c = self.c2(nn.relu(pre))
        hu_b = mx.broadcast_to(hu[:, None, :], c.shape)
        h0_b = mx.broadcast_to(h0[:, None, :], c.shape)
        joint = mx.concatenate([hu_b, h0_b, c, hu_b * c], axis=-1)
        s = self.s2(nn.relu(self.s1(joint)))[..., 0]
        from .model import NEG
        return mx.where(cmask, s, NEG)


class MBANet(MBNet):
    """MB with the price input and the auxiliary heads (M-HANDOFF.md, backlog 2 and addition 8). Starts equal to MB:
    the price's projection is zero; the heads read the body and do not change the play.
    - price (3: worth in units / 8, time pressure, given) -> d, added to the objective token (`w_price`, zero init);
    - danger after layer 1 (`ge1`, `ge2`: the hit points each of our units loses in the enemy's next turn), beside
      MB's danger head on the last layer;
    - the strike's expected dealt / taken (the rules' preview, candidate columns 30, 31) from the action scorer's
      hidden state of that candidate (`xs`);
    - value on [H_0, P_0] (`va1`, `va2`): the win (logit), the capture (logit), Phi's change over the next 3 turns.
    The war target and stage on the plan tokens are M's auxiliary head already (`aux_target`, the stage)."""

    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400, map_ch=MAP_CH):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos, map_ch=map_ch)
        self.config['aux_heads'] = True
        self.w_price = mx.zeros((3, d))
        self.ge1 = nn.Linear(d, d)
        self.ge2 = nn.Linear(d, 1)
        self.xs = nn.Linear(d, 2)
        self.va1 = nn.Linear(2 * d, d)
        self.va2 = nn.Linear(d, 3)

    def body_b(self, b, um, pm):
        """The body on a batch dict: (H, the stream after layer 1)."""
        return self.body_mba(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm, b['gtok'], b['tok_ok'], b['mmean'], b['price'])

    def body_mba(self, tok, dist, tmask, slot, um, pm, gtok, tok_ok, mmean, price):
        """`body_mbg` with the price on the objective token; also returns the residual stream after layer 1."""
        b, t, _ = tok.shape
        h = self.e2(nn.relu(self.e1(tok)))
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        h = h + mx.where(tok_ok[..., None], self.m_tok(gtok), 0.0)
        top = self.m_pool(mmean) + price @ self.w_price
        h = mx.concatenate([h[:, :1] + top[:, None, :], h[:, 1:]], axis=1)
        x = mx.concatenate([h, pm], axis=1)
        mask = mx.concatenate([tmask, mx.ones((b, PLANS), dtype=mx.bool_)], axis=1)
        is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
        pair_plan = is_plan[:, None] | is_plan[None, :]
        dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
        h1 = None
        for li, layer in enumerate(self.layers):
            x = self.layer_m(layer, x, dpad, mask, pair_plan, self.plan_bias[li])
            if li == 0:
                h1 = x
        return self.norm(x), h1

    def action_logits_a(self, h, unit, cand, promo, cmask, gcand, cand_ok):
        """`action_logits_g` and the scorer's hidden state per candidate [B, C, d]."""
        b = h.shape[0]
        hu = h[mx.arange(b), unit]
        h0 = h[:, 0]
        pre = self.c1(cand) + self.promo[promo] + mx.where(cand_ok[..., None], self.m_cand(gcand), 0.0)
        c = self.c2(nn.relu(pre))
        hu_b = mx.broadcast_to(hu[:, None, :], c.shape)
        h0_b = mx.broadcast_to(h0[:, None, :], c.shape)
        joint = mx.concatenate([hu_b, h0_b, c, hu_b * c], axis=-1)
        hid = nn.relu(self.s1(joint))
        s = self.s2(hid)[..., 0]
        from .model import NEG
        return mx.where(cmask, s, NEG), hid

    def danger_early(self, h1):
        return self.ge2(nn.relu(self.ge1(h1)))[..., 0]

    def value_a(self, h, p0):
        return self.va2(nn.relu(self.va1(mx.concatenate([h[:, 0], p0], axis=-1))))


# MBH: the history's parameters (their own lr group in train_m: `--new-lr-mult`).
HX_PARAMS = ('hx1', 'hx2', 'hxp', 'hx_null', 'hx_same', 'hx_slope', 'ho')
# The fact's own age number (history column 10, age / 3): left out of MBH's embedding (age enters as a bias only).
HX_AGE_COL = 10


class MBHNet(MBANet):
    """MBA plus the history of facts (M-HANDOFF.md, «MBH0 spec», 19:45). Starts equal to MBA: the history's output
    projections `ho` are zero.
    - A fact's embedding: hx2(relu(hx1(fact with its age number zeroed))) + hxp(P_0 of the fact's turn).
    - Per layer a SEPARATE cross-attention of the tokens and plan tokens (queries: the layer's q on norm1 of the
      stream) to the facts (keys / values: the layer's k / v on norm1 of the embedding), its own softmax with one
      always-present null key (logit hx_null[l, h], value 0), plus per head: -hx_slope[l, h] x age (ALiBi-style; age
      is not in the keys, so a fact's keys / values stay fixed while it lives) and hx_same[l, h] for a fact of the
      query's own unit. Its context enters the residual through ho[l] (d x d, zero at the start), beside the
      self-attention's output, before the feed-forward.
    - The memory update is MB's (`MemNet.step`)."""

    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400, map_ch=MAP_CH):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos, map_ch=map_ch)
        from .data import F_HIST
        from .model_mh import H_MAX
        self.config['xhist'] = {'f_hist': F_HIST, 'h_max': H_MAX, 'null_key': True, 'age_col_zeroed': HX_AGE_COL}
        self.hx1 = nn.Linear(F_HIST, d)
        self.hx2 = nn.Linear(d, d)
        self.hxp = nn.Linear(d, d)
        self.hxp.weight = self.hxp.weight * 0.1
        self.hx_null = mx.zeros((layers, heads))
        self.hx_same = mx.full((layers, heads), 1.0)
        self.hx_slope = mx.full((layers, heads), 0.5)
        self.ho = [nn.Linear(d, d) for _ in range(layers)]
        for o in self.ho:
            o.weight = mx.zeros((d, d))
            o.bias = mx.zeros((d,))

    def hx_embed(self, hist, hplan):
        keep = (mx.arange(hist.shape[-1]) != HX_AGE_COL).astype(hist.dtype)
        return self.hx2(nn.relu(self.hx1(hist * keep))) + self.hxp(hplan)

    def body_b(self, b, um, pm):
        return self.body_mbh(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm, b['gtok'], b['tok_ok'], b['mmean'], b['price'],
                             b['hist'], b['hmask'], b['hslot'], b['hage'], b['hplan'])

    def body_mbh(self, tok, dist, tmask, slot, um, pm, gtok, tok_ok, mmean, price, hist, hmask, hslot, hage, hplan):
        """`body_mba` with the history's cross-attention in every layer; returns (H, the stream after layer 1)."""
        b, t, _ = tok.shape
        h = self.e2(nn.relu(self.e1(tok)))
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        h = h + mx.where(tok_ok[..., None], self.m_tok(gtok), 0.0)
        top = self.m_pool(mmean) + price @ self.w_price
        h = mx.concatenate([h[:, :1] + top[:, None, :], h[:, 1:]], axis=1)
        x = mx.concatenate([h, pm], axis=1)
        mask = mx.concatenate([tmask, mx.ones((b, PLANS), dtype=mx.bool_)], axis=1)
        is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
        pair_plan = is_plan[:, None] | is_plan[None, :]
        dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
        qslot = mx.concatenate([slot, mx.full((b, PLANS), -1, dtype=slot.dtype)], axis=1)
        he = self.hx_embed(hist, hplan)
        same = ((qslot[:, :, None] == hslot[:, None, :]) & (hslot[:, None, :] >= 0)).astype(mx.float32)  # [B, T+P, H]
        age = hage.astype(mx.float32)
        h1 = None
        for li, layer in enumerate(self.layers):
            x = self.layer_mbh(layer, self.ho[li], x, dpad, mask, pair_plan, self.plan_bias[li], he, hmask, same, age,
                               self.hx_null[li], self.hx_same[li], self.hx_slope[li])
            if li == 0:
                h1 = x
        return self.norm(x), h1

    @staticmethod
    def layer_mbh(layer, ho, h, dist, key_mask, pair_plan, pbias, he, hmask, same, age, nbias, sbias, slope):
        import math
        from .model import NEG
        b, t, d = h.shape
        heads = layer.heads
        dh = d // heads
        nh = he.shape[1]
        a = layer.norm1(h)
        q = layer.q(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        k = layer.k(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        v = layer.v(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        sc = 1.0 / math.sqrt(dh)
        s = (q @ k.transpose(0, 1, 3, 2)) * sc
        bias = layer.dist_bias[:, dist].transpose(1, 0, 2, 3)
        bias = mx.where(pair_plan[None, None], pbias[None, :, None, None], bias)
        s = s + bias + mx.where(key_mask[:, None, None, :], 0.0, NEG)
        w = mx.softmax(s, axis=-1)
        ctx = (w @ v).transpose(0, 2, 1, 3).reshape(b, t, d)
        # The history: its own softmax over [null, facts].
        ah = layer.norm1(he)
        kh = layer.k(ah).reshape(b, nh, heads, dh).transpose(0, 2, 1, 3)
        vh = layer.v(ah).reshape(b, nh, heads, dh).transpose(0, 2, 1, 3)
        sh = (q @ kh.transpose(0, 1, 3, 2)) * sc - slope[None, :, None, None] * age[:, None, None, :] + same[:, None] * sbias[None, :, None, None]
        sh = sh + mx.where(hmask[:, None, None, :], 0.0, NEG)
        s0 = mx.broadcast_to(nbias[None, :, None, None], (b, heads, t, 1))
        wh = mx.softmax(mx.concatenate([s0, sh], axis=-1), axis=-1)
        ctxh = (wh[..., 1:] @ vh).transpose(0, 2, 1, 3).reshape(b, t, d)
        h = h + layer.o(ctx) + ho(ctxh)
        m = layer.norm2(h)
        return h + layer.f2(nn.relu(layer.f1(m)))
