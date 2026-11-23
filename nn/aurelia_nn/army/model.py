"""The army net in MLX (DESIGN.md, «Body»); `crates/qa/src/army_net/model.rs` is the same arithmetic in Rust.

Batched with padding: tokens [B, T, F_TOK] with a mask, distances [B, T, T], candidates [B, C, F_ACT] with a mask.
"""
import math

import mlx.core as mx
import mlx.nn as nn

from .data import F_ACT, F_TOK

D_MAX = 15
NEG = -1e9


class Layer(nn.Module):
    def __init__(self, d, heads, ff):
        super().__init__()
        self.heads = heads
        self.norm1 = nn.LayerNorm(d, eps=1e-5)
        self.q = nn.Linear(d, d)
        self.k = nn.Linear(d, d)
        self.v = nn.Linear(d, d)
        self.o = nn.Linear(d, d)
        self.dist_bias = mx.zeros((heads, D_MAX + 1))
        self.norm2 = nn.LayerNorm(d, eps=1e-5)
        self.f1 = nn.Linear(d, ff)
        self.f2 = nn.Linear(ff, d)

    def __call__(self, h, dist, key_mask):
        b, t, d = h.shape
        dh = d // self.heads
        a = self.norm1(h)
        q = self.q(a).reshape(b, t, self.heads, dh).transpose(0, 2, 1, 3)
        k = self.k(a).reshape(b, t, self.heads, dh).transpose(0, 2, 1, 3)
        v = self.v(a).reshape(b, t, self.heads, dh).transpose(0, 2, 1, 3)
        s = (q @ k.transpose(0, 1, 3, 2)) * (1.0 / math.sqrt(dh))
        # bias[h, dist[b, i, j]] -> [b, h, i, j]
        bias = self.dist_bias[:, dist].transpose(1, 0, 2, 3)
        s = s + bias + mx.where(key_mask[:, None, None, :], 0.0, NEG)
        w = mx.softmax(s, axis=-1)
        ctx = (w @ v).transpose(0, 2, 1, 3).reshape(b, t, d)
        h = h + self.o(ctx)
        m = self.norm2(h)
        return h + self.f2(nn.relu(self.f1(m)))


class ArmyNet(nn.Module):
    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400):
        super().__init__()
        self.config = {'d': d, 'heads': heads, 'layers': layers, 'ff': ff, 'f_tok': F_TOK, 'f_act': F_ACT, 'promos': promos, 'dmax': D_MAX}
        self.e1 = nn.Linear(F_TOK, d)
        self.e2 = nn.Linear(d, d)
        self.layers = [Layer(d, heads, ff) for _ in range(layers)]
        self.norm = nn.LayerNorm(d, eps=1e-5)
        self.pointer = nn.Linear(d, 1)
        self.end = nn.Linear(d, 1)
        self.c1 = nn.Linear(F_ACT, d)
        self.promo = mx.zeros((promos + 1, d))
        self.c2 = nn.Linear(d, d)
        self.s1 = nn.Linear(4 * d, d)
        self.s2 = nn.Linear(d, 1)
        self.v1 = nn.Linear(d, d)
        self.v2 = nn.Linear(d, 2)
        self.g1 = nn.Linear(d, d)
        self.g2 = nn.Linear(d, 1)

    def body(self, tok, dist, tmask):
        h = self.e2(nn.relu(self.e1(tok)))
        for layer in self.layers:
            h = layer(h, dist, tmask)
        return self.norm(h)

    def pointer_logits(self, h, sel):
        """[B, T + 1]: the selectable tokens' logits (the others masked), then the end's."""
        p = self.pointer(h)[..., 0]
        p = mx.where(sel, p, NEG)
        e = self.end(h[:, 0])
        return mx.concatenate([p, e], axis=1)

    def action_logits(self, h, unit, cand, promo, cmask):
        """[B, C] for the unit of token `unit` [B]."""
        b = h.shape[0]
        hu = h[mx.arange(b), unit]
        h0 = h[:, 0]
        c = self.c2(nn.relu(self.c1(cand) + self.promo[promo]))
        cn = c.shape[1]
        hu_b = mx.broadcast_to(hu[:, None, :], c.shape)
        h0_b = mx.broadcast_to(h0[:, None, :], c.shape)
        joint = mx.concatenate([hu_b, h0_b, c, hu_b * c], axis=-1)
        s = self.s2(nn.relu(self.s1(joint)))[..., 0]
        del cn
        return mx.where(cmask, s, NEG)

    def value(self, h):
        return self.v2(nn.relu(self.v1(h[:, 0])))

    def danger(self, h):
        return self.g2(nn.relu(self.g1(h)))[..., 0]
