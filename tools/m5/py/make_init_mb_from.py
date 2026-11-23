"""MB's function-preserving init from an M net (the B-line restart "MBf"): the M net's trunk and memory plus B1's trained
64-channel map encoder; the map's projections m_tok, m_cand, m_pool are ZERO, so the net plays exactly as the M net
before training (check: tools/m5/py/same_plays.py on the two quick evals).

python tools/m5/py/make_init_mb_from.py M0_WEIGHTS B1_WEIGHTS OUT_DIR [--d 64 --layers 3 --heads 4]
The encoder (m_in, blocks) does not depend on d: one B1 (trained at d 64) serves every width. ff = 2 d as in train_m."""
import argparse
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', 'nn'))
import mlx.core as mx
import mlx.nn as nn

mx.set_default_device(mx.cpu)
mx.set_memory_limit(4 * 2 ** 30)
mx.set_cache_limit(2 ** 30)
from aurelia_nn.army.export import export
from aurelia_nn.army.model_mb import MBNet


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('m_weights')
    ap.add_argument('b1_weights')
    ap.add_argument('out')
    ap.add_argument('--d', type=int, default=64)
    ap.add_argument('--layers', type=int, default=3)
    ap.add_argument('--heads', type=int, default=4)
    ap.add_argument('--map-ch', type=int, default=64)
    a = ap.parse_args()
    m = mx.load(a.m_weights)
    b1 = mx.load(a.b1_weights)
    model = MBNet(d=a.d, heads=a.heads, layers=a.layers, ff=2 * a.d, promos=m['promo'].shape[0] - 1, map_ch=a.map_ch)
    model.hand_init()
    own = dict(nn.utils.tree_flatten(model.parameters()))
    take = {k: v for k, v in m.items() if k in own and own[k].shape == v.shape}
    enc = {k: v for k, v in b1.items() if (k.startswith('m_in.') or k.startswith('blocks.')) and k in own and own[k].shape == v.shape}
    zero = {k: mx.zeros(own[k].shape) for k in own if k.split('.')[0] in ('m_tok', 'm_cand', 'm_pool')}
    left = sorted(k for k in own if k not in take and k not in enc and k not in zero)
    print('from', a.m_weights, len(take), '| B1 encoder', len(enc), '| zero', sorted(zero), '| drawn (must be empty):', left)
    assert not left, left
    model.load_weights(list(take.items()) + list(enc.items()) + list(zero.items()), strict=True)
    mx.eval(model.parameters())
    os.makedirs(a.out, exist_ok=True)
    model.save_weights(a.out + '/weights.safetensors')
    export(model, a.out + '/army-net.bin')


if __name__ == '__main__':
    main()
