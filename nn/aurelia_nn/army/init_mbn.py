"""MBN's function-preserving start (M-HANDOFF.md, «MBN (24 Nov)»): an MB net's weights plus the notes' new parameters
(drawn; the heads hand-set; the output projections `no` zero), so the net plays exactly as the MB net.

python -m aurelia_nn.army.init_mbn --mb runs/netMBf3/weights.safetensors --out runs/init-MBN
Writes OUT/weights.safetensors and OUT/army-net.bin (the Rust export)."""
import argparse
import os

import mlx.core as mx
import mlx.nn as nn

from .export import export
from .model_mb import MBNNet


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--mb', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--seed', type=int, default=1)
    a = ap.parse_args()
    mx.set_default_device(mx.cpu)
    mx.random.seed(a.seed)
    w = mx.load(a.mb)
    model = MBNNet(promos=w['promo'].shape[0] - 1, map_ch=w['m_in.weight'].shape[0])
    model.hand_init()
    own = dict(nn.utils.tree_flatten(model.parameters()))
    bad = [k for k in w if k not in own or own[k].shape != w[k].shape]
    if bad:
        raise SystemExit(f'not an MB net of this shape: {bad[:5]}')
    model.load_weights(list(w.items()), strict=False)
    mx.eval(model.parameters())
    os.makedirs(a.out, exist_ok=True)
    model.save_weights(os.path.join(a.out, 'weights.safetensors'))
    export(model, os.path.join(a.out, 'army-net.bin'))
    new = sorted(k for k in own if k not in w)
    print(f'{len(w)} tensors from {a.mb}, new: {new}')


if __name__ == '__main__':
    main()
