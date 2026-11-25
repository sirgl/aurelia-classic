"""Deepen an MB army net by new layers that start silent (net-deep, GATES.md D1).

    python -m aurelia_nn.army.deepen --base u175.safetensors --kind m --after 3 --n 2 --seed 1 --out OUT.safetensors

The base's layers keep their weights; `n` new layers of `kind` (c: the classic `Layer`, m: `ModernLayer`) are inserted
after the base's layer `after` (3: on top of the trunk, before the final norm). A new layer's q, k, v, MLP input are
drawn (MLX's default init, `--seed`), its outputs are zero (classic: `o`, `f2`; modern: `o`, `w2`, and the gate `g`),
its distance bias zero, its plan-pair bias -2 (MemNet's start). So the deep net computes exactly the base's function:
every residual add of a new layer adds 0.0.
"""
import argparse

import mlx.core as mx
from mlx.utils import tree_flatten

from .model import ModernLayer, shape_of, zero_out_classic
from .model_mb import MBNet


def deepen(w, kind, after, n, seed):
    n0, kinds0 = shape_of(w)
    assert kinds0 == 'c' * n0, 'the base is a classic net'
    kinds = kinds0[:after] + kind * n + kinds0[after:]
    mx.random.seed(seed)
    net = MBNet(promos=w['promo'].shape[0] - 1, map_ch=w['m_in.weight'].shape[0], layers=len(kinds), kinds=kinds)
    new = dict(tree_flatten(net.parameters()))
    # old layer i -> its index in the deep net
    where = {i: (i if i < after else i + n) for i in range(n0)}
    out = {}
    for k, v in w.items():
        if k.startswith('layers.'):
            i = int(k.split('.')[1])
            out[k.replace(f'layers.{i}.', f'layers.{where[i]}.', 1)] = v
        elif k == 'plan_bias':
            out[k] = mx.concatenate([v[:after], mx.full((n, v.shape[1]), -2.0), v[after:]], axis=0)
        else:
            out[k] = v
    for li in range(after, after + n):
        layer = net.layers[li]
        if isinstance(layer, ModernLayer):
            layer.zero_out()
        else:
            zero_out_classic(layer)
        for k, v in tree_flatten(layer.parameters()):
            out[f'layers.{li}.{k}'] = v
    missing = set(new) - set(out)
    extra = set(out) - set(new)
    assert not missing and not extra, (missing, extra)
    return out, kinds


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--base', required=True)
    ap.add_argument('--kind', choices=['c', 'm'], required=True)
    ap.add_argument('--after', type=int, default=3)
    ap.add_argument('--n', type=int, default=2)
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--out', required=True)
    a = ap.parse_args()
    mx.set_default_device(mx.cpu)
    w = mx.load(a.base)
    out, kinds = deepen(w, a.kind, a.after, a.n, a.seed)
    mx.save_safetensors(a.out, out)
    print(f'{a.out}: layers {kinds}, {sum(v.size for v in out.values())} weights (base {sum(v.size for v in w.values())})')


if __name__ == '__main__':
    main()
