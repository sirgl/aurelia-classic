"""Architecture variants of the MB army net, switched by an `arch` dict (`MBXNet`). Every flag is off by default;
all off is `MBNet` exactly (the same parameters, names, initial draws and arithmetic).

Flags (`ARCH_OFF` lists them; `parse_arch` reads the command-line string `swiglu,rmsnorm,loop=2`):
- swiglu:    the feed-forward is f2(silu(f1(m)) * f3(m)); the hidden size is round(2/3 ff), at least 8, so the
             parameter count stays that of the ReLU MLP.
- rmsnorm:   norm1, norm2 and the final norm are nn.RMSNorm(d, eps=1e-5).
- qknorm:    q and k are RMS-normalised per head (over dh, learnable scale: `qn`, `kn`) before the dot product.
- gate_attn: output-gated attention, ctx = ctx * sigmoid(g(a)) per head channel, g = Linear(d, d) on the normed
             input. Init: g's weight is the Linear default x 0.1 (small random) and its bias +3, so the gate starts
             near sigmoid(3) = 0.95 and the net starts near the ungated one.
- loop:      the layer stack is applied `loop` times with shared weights (1: off). A pass p >= 1 first adds the
             learned `pass_emb[p]` to the stream, then each layer's result enters through the scalar
             `pass_gate[p, l]`: x <- x + pass_gate[p, l] * (layer(x) - x). Both start at ZERO, so at init a loop
             net computes what the 1-pass net computes. Row 0 of both tensors is unused (pass 0 is the plain stack).
Depth is `layers=` as for every net; the memory step is MemNet's.
"""
import math

import mlx.core as mx
import mlx.nn as nn

from .model import NEG, Layer
from .model_m import PLANS
from .model_mb import MAP_CH, MBNet

ARCH_OFF = {'swiglu': False, 'rmsnorm': False, 'qknorm': False, 'gate_attn': False, 'loop': 1}
FLAGS = ('swiglu', 'rmsnorm', 'qknorm', 'gate_attn')
GATE_BIAS = 3.0


def parse_arch(text):
    """'swiglu,loop=2' -> the full arch dict; '' and 'base': every flag off."""
    arch = dict(ARCH_OFF)
    for item in filter(None, (t.strip() for t in text.split(','))):
        if item == 'base':
            continue
        name, _, value = item.partition('=')
        assert name in ARCH_OFF, f'unknown arch flag {name!r}; known: {sorted(ARCH_OFF)}'
        arch[name] = int(value) if name == 'loop' else True
    assert arch['loop'] >= 1, 'loop counts passes, at least 1'
    return arch


def arch_changes_net(arch):
    """True when the arch is more than MBNet's arithmetic (the Rust export cannot express it)."""
    return any(arch[f] for f in FLAGS) or arch['loop'] > 1


def swiglu_hidden(ff):
    return max(8, round(2 * ff / 3))


def ff_of_swiglu_hidden(hidden):
    """The nominal ff whose swiglu_hidden is `hidden` (for a loader that reads the shape)."""
    for ff in range(int(1.5 * hidden) - 3, int(1.5 * hidden) + 4):
        if swiglu_hidden(ff) == hidden:
            return ff
    raise ValueError(f'no ff gives a swiglu hidden size {hidden}')


class XLayer(Layer):
    """A `Layer` with the flags' parts; the plain parts keep Layer's names (q, k, v, o, f1, f2, norm1, norm2)."""

    def __init__(self, d, heads, ff, arch):
        super().__init__(d, heads, ff)
        dh = d // heads
        if arch['rmsnorm']:
            self.norm1 = nn.RMSNorm(d, eps=1e-5)
            self.norm2 = nn.RMSNorm(d, eps=1e-5)
        if arch['qknorm']:
            self.qn = nn.RMSNorm(dh, eps=1e-5)
            self.kn = nn.RMSNorm(dh, eps=1e-5)
        if arch['gate_attn']:
            self.g = nn.Linear(d, d)
            self.g.weight = self.g.weight * 0.1
            self.g.bias = mx.full((d,), GATE_BIAS)
        if arch['swiglu']:
            hidden = swiglu_hidden(ff)
            self.f1 = nn.Linear(d, hidden)
            self.f3 = nn.Linear(d, hidden)
            self.f2 = nn.Linear(hidden, d)


class MBXNet(MBNet):
    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400, map_ch=MAP_CH, arch=None):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos, map_ch=map_ch)
        arch = {**ARCH_OFF, **(arch or {})}
        assert set(arch) == set(ARCH_OFF), sorted(arch)
        self.config['arch'] = arch
        if any(arch[f] for f in FLAGS):
            self.layers = [XLayer(d, heads, ff, arch) for _ in range(layers)]
        if arch['rmsnorm']:
            self.norm = nn.RMSNorm(d, eps=1e-5)
        if arch['loop'] > 1:
            self.pass_emb = mx.zeros((arch['loop'], d))
            self.pass_gate = mx.zeros((arch['loop'], layers))

    @property
    def arch(self):
        return self.config['arch']

    def layer_m(self, layer, h, dist, key_mask, pair_plan, pbias):
        """MemNet.layer_m (the plan bias, the distance bias, the key mask: identical) with the flags' parts."""
        arch = self.arch
        b, t, d = h.shape
        heads = layer.heads
        dh = d // heads
        a = layer.norm1(h)
        q = layer.q(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        k = layer.k(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        v = layer.v(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        if arch['qknorm']:
            q, k = layer.qn(q), layer.kn(k)
        s = (q @ k.transpose(0, 1, 3, 2)) * (1.0 / math.sqrt(dh))
        bias = layer.dist_bias[:, dist].transpose(1, 0, 2, 3)  # [b, h, t, t]
        bias = mx.where(pair_plan[None, None], pbias[None, :, None, None], bias)
        s = s + bias + mx.where(key_mask[:, None, None, :], 0.0, NEG)
        w = mx.softmax(s, axis=-1)
        ctx = (w @ v).transpose(0, 2, 1, 3).reshape(b, t, d)
        if arch['gate_attn']:
            ctx = ctx * mx.sigmoid(layer.g(a))
        h = h + layer.o(ctx)
        m = layer.norm2(h)
        if arch['swiglu']:
            return h + layer.f2(nn.silu(layer.f1(m)) * layer.f3(m))
        return h + layer.f2(nn.relu(layer.f1(m)))

    def stack(self, x, dpad, mask, pair_plan):
        """The layers `loop` times with shared weights, then the final norm."""
        for p in range(self.arch['loop']):
            if p > 0:
                x = x + self.pass_emb[p]
            for li, layer in enumerate(self.layers):
                y = self.layer_m(layer, x, dpad, mask, pair_plan, self.plan_bias[li])
                x = y if p == 0 else x + self.pass_gate[p, li] * (y - x)
        return self.norm(x)

    def plan_inputs(self, h, pm, tmask, dist):
        """The stream with the plan tokens appended, the key mask, the plan-pair mask, the padded distances."""
        b, t, _ = h.shape
        x = mx.concatenate([h, pm], axis=1)
        mask = mx.concatenate([tmask, mx.ones((b, PLANS), dtype=mx.bool_)], axis=1)
        is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
        pair_plan = is_plan[:, None] | is_plan[None, :]
        dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
        return x, mask, dpad, pair_plan

    def body_mbg(self, tok, dist, tmask, slot, um, pm, gtok, tok_ok, mmean):
        b, _, _ = tok.shape
        h = self.e2(nn.relu(self.e1(tok)))
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        h = h + mx.where(tok_ok[..., None], self.m_tok(gtok), 0.0)
        pooled = self.m_pool(mmean)
        h = mx.concatenate([h[:, :1] + pooled[:, None, :], h[:, 1:]], axis=1)
        x, mask, dpad, pair_plan = self.plan_inputs(h, pm, tmask, dist)
        return self.stack(x, dpad, mask, pair_plan)

    def body_m(self, tok, dist, tmask, slot, um, pm):
        """MemNet's body (no map) through the same stack."""
        b, _, _ = tok.shape
        h = self.e2(nn.relu(self.e1(tok)))
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        x, mask, dpad, pair_plan = self.plan_inputs(h, pm, tmask, dist)
        return self.stack(x, dpad, mask, pair_plan)
