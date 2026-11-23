"""The army net in MLX (DESIGN.md, «Body»); `crates/qa/src/army_net/model.rs` is the same arithmetic in Rust.

Batched with padding: tokens [B, T, F_TOK] with a mask, distances [B, T, T], candidates [B, C, F_ACT] with a mask.
"""
import math

import mlx.core as mx
import mlx.nn as nn

from .data import F_ACT, F_TOK, S_CH, W

D_MAX = 15
NEG = -1e9
# The token numbers scattered onto the map (model.rs LOCAL), the encoder's dilations (model.rs DIL).
LOCAL = [1, 2, 3, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 42, 43, 44, 45]
M_IN = S_CH + len(LOCAL)
DIL = [(1, 2), (2, 4), (2, 1), (1, 1)]
# The hex taps in a 3 x 3 kernel [ky = dr + 1][kx = dq + 1]: all but (dq, dr) = (-1, -1) and (1, 1).
HEX_MASK = [[0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]]


class HexConv(nn.Module):
    def __init__(self, c_in, c_out, d):
        super().__init__()
        self.d = d
        scale = math.sqrt(1.0 / (7 * c_in))
        self.weight = mx.random.uniform(-scale, scale, (c_out, 3, 3, c_in))
        self.bias = mx.zeros((c_out,))

    def __call__(self, x):
        w = self.weight * mx.array(HEX_MASK)[None, :, :, None]
        return mx.conv2d(x, w, stride=1, padding=self.d, dilation=self.d) + self.bias


class Block(nn.Module):
    def __init__(self, c, d1, d2):
        super().__init__()
        self.c1 = HexConv(c, c, d1)
        self.c2 = HexConv(c, c, d2)

    def __call__(self, x):
        return x + self.c2(nn.relu(self.c1(nn.relu(x))))


def map_planes(statics, tok, pos):
    """[B, W*W, M_IN]: the static planes and the tokens' local numbers summed on their tiles."""
    b, t, _ = tok.shape
    onehot = (pos[..., None] == mx.arange(W * W)[None, None, :]).astype(mx.float32)  # [B, T, WW] (pos -1: none)
    local = tok[..., mx.array(LOCAL)]
    dyn = onehot.transpose(0, 2, 1) @ local
    return mx.concatenate([statics, dyn], axis=-1)


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
    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400, map_ch=0):
        super().__init__()
        self.config = {'d': d, 'heads': heads, 'layers': layers, 'ff': ff, 'f_tok': F_TOK, 'f_act': F_ACT, 'promos': promos, 'dmax': D_MAX, 'map_ch': map_ch}
        self.has_map = map_ch > 0
        if self.has_map:
            assert map_ch == d, 'the map features enter through d x d projections'
            self.m_in = nn.Linear(M_IN, map_ch)
            self.blocks = [Block(map_ch, d1, d2) for d1, d2 in DIL]
            self.m_tok = nn.Linear(map_ch, d)
            self.m_cand = nn.Linear(map_ch, d)
            self.m_pool = nn.Linear(map_ch, d)
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

    def encode(self, planes):
        """[B, W*W, M_IN] -> [B, W*W, C]."""
        b = planes.shape[0]
        x = self.m_in(planes).reshape(b, W, W, -1)
        for blk in self.blocks:
            x = blk(x)
        return x.reshape(b, W * W, -1)

    @staticmethod
    def at(m, pos):
        """The map features at window indices pos [B, K] (zero where pos < 0) and the mask."""
        b = m.shape[0]
        idx = mx.maximum(pos, 0)
        g = m[mx.arange(b)[:, None], idx]
        return g, (pos >= 0)

    def body(self, tok, dist, tmask, m=None, pos=None):
        h = self.e2(nn.relu(self.e1(tok)))
        if self.has_map:
            g, ok = self.at(m, pos)
            h = h + mx.where(ok[..., None], self.m_tok(g), 0.0)
            pooled = self.m_pool(mx.mean(m, axis=1))
            h = mx.concatenate([h[:, :1] + pooled[:, None, :], h[:, 1:]], axis=1)
        for layer in self.layers:
            h = layer(h, dist, tmask)
        return self.norm(h)

    def pointer_logits(self, h, sel):
        """[B, T + 1]: the selectable tokens' logits (the others masked), then the end's."""
        p = self.pointer(h)[..., 0]
        p = mx.where(sel, p, NEG)
        e = self.end(h[:, 0])
        return mx.concatenate([p, e], axis=1)

    def action_logits(self, h, unit, cand, promo, cmask, m=None, cpos=None):
        """[B, C] for the unit of token `unit` [B]."""
        b = h.shape[0]
        hu = h[mx.arange(b), unit]
        h0 = h[:, 0]
        pre = self.c1(cand) + self.promo[promo]
        if self.has_map:
            g, ok = self.at(m, cpos)
            pre = pre + mx.where(ok[..., None], self.m_cand(g), 0.0)
        c = self.c2(nn.relu(pre))
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
