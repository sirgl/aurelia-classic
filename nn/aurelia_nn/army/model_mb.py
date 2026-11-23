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
