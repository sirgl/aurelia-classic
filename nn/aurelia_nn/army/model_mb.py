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
