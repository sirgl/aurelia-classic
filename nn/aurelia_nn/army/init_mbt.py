"""MBT from an MB net (step 1.5, gate T1 of the typed-token plan): the typed tokens and the third parties' path
added so that the net plays exactly as before.

python -m aurelia_nn.army.init_mbt --from WEIGHTS.safetensors --out DIR [--random-new SEED]

Start values: the objective's, the city's and the third tokens' encoders are copies of the unit's (e1 / e2); the type
embedding, every extras' projection (ty_x, tp_x, cx), the third path's output tp_o and the new map columns of m_in are
zero; tp_null 0, tp_dist a copy of each layer's dist_bias; c_new -10. `--random-new SEED` (the parity check only, never
played in a table) draws the new parts instead: the new encoders' weights N(0, 1 / fan_in) and biases N(0, 0.1^2); ty_emb,
ty_x, tp_x, cx and the new m_in columns N(0, 0.1^2); tp_o's weight N(0, 0.5^2 / d) (bias 0). Writes DIR/weights.safetensors
and DIR/army-net.bin (the Rust format).
"""
import argparse
import os

import mlx.core as mx
import mlx.nn as nn

from .data import S_REL
from .export import export
from .model import M_IN
from .model_mb import MBNet, MBNNet, MBTNet
from .model_mbtn import MBTNNet


def convert(w, seed=None):
    """An MBTNet from MB weights `w` (a dict of arrays)."""
    promos = w['promo'].shape[0] - 1
    map_ch = w['m_in.weight'].shape[0]
    d = w['e2.weight'].shape[0]
    layers = 1 + max(int(k.split('.')[1]) for k in w if k.startswith('layers.'))
    heads = w['layers.0.dist_bias'].shape[0]
    ff = w['layers.0.f1.weight'].shape[0]
    # A net with the notes (MBN: `nx_slope`) becomes MBTN: the typed tokens and the third parties on top of the notes (step 1.10).
    notes = 'nx_slope' in w
    base = (MBNNet if notes else MBNet)(d=d, heads=heads, layers=layers, ff=ff, promos=promos, map_ch=map_ch)
    base.load_weights(list(w.items()))
    net = (MBTNNet if notes else MBTNet)(d=d, heads=heads, layers=layers, ff=ff, promos=promos, map_ch=map_ch)
    flat = dict(nn.utils.tree_flatten(base.parameters()))
    rnd = seed is not None
    if rnd:
        mx.random.seed(seed)
    normal = lambda shape, s: mx.random.normal(shape) * s
    new = {}
    m_in = flat.pop('m_in.weight')
    extra = normal((map_ch, S_REL), 0.1) if rnd else mx.zeros((map_ch, S_REL))
    new['m_in.weight'] = mx.concatenate([m_in, extra], axis=1)
    for a, b in (('ty_obj1', 'ty_obj2'), ('ty_city1', 'ty_city2'), ('tp_u1', 'tp_u2'), ('tp_c1', 'tp_c2')):
        for name, src in ((a, 'e1'), (b, 'e2')):
            wt, bs = flat[f'{src}.weight'], flat[f'{src}.bias']
            if rnd:
                wt = normal(wt.shape, 1.0 / wt.shape[1] ** 0.5)
                bs = normal(bs.shape, 0.1)
            new[f'{name}.weight'], new[f'{name}.bias'] = wt, bs
    for name in ('ty_emb', 'ty_x', 'tp_x', 'cx'):
        shape = getattr(net, name).shape
        new[name] = normal(shape, 0.1) if rnd else mx.zeros(shape)
    new['tp_null'] = mx.zeros((layers, heads))
    new['tp_dist'] = mx.stack([flat[f'layers.{l}.dist_bias'] for l in range(layers)])
    for l in range(layers):
        new[f'tp_o.{l}.weight'] = normal((d, d), 0.5 / d ** 0.5) if rnd else mx.zeros((d, d))
        new[f'tp_o.{l}.bias'] = mx.zeros((d,))
    new['c_new'] = mx.array([-10.0])
    flat.update(new)
    net.load_weights(list(flat.items()))
    mx.eval(net.parameters())
    assert set(dict(nn.utils.tree_flatten(net.parameters()))) == set(flat), 'every MBT parameter is set'
    return net


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--from', dest='src', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--random-new', type=int, default=None)
    a = ap.parse_args()
    mx.set_default_device(mx.cpu)
    net = convert(mx.load(a.src), a.random_new)
    os.makedirs(a.out, exist_ok=True)
    net.save_weights(os.path.join(a.out, 'weights.safetensors'))
    export(net, os.path.join(a.out, 'army-net.bin'))
    print(f'{a.out}: {type(net).__name__} from {a.src}' + (f' (new parts drawn, seed {a.random_new})' if a.random_new is not None else ' (function-preserving start)'))


if __name__ == '__main__':
    main()
