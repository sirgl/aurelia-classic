"""Logits parity of the MLX army net and its Rust forward (`ai-exam army-parity`) on recorded decisions.

python -m aurelia_nn.army.parity --plays FILE --weights OUT/weights.safetensors --rust rust.jsonl [--limit N]
The MLX side runs padded batches (the training path); the Rust side one decision at a time, unpadded.
"""
import argparse
import json

import mlx.core as mx

from .data import read_into, Flat
from .model import ArmyNet
from .train import Device, forward


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--plays', required=True)
    ap.add_argument('--weights', required=True)
    ap.add_argument('--rust', required=True)
    ap.add_argument('--limit', type=int, default=2000)
    a = ap.parse_args()
    rust = [json.loads(l) for l in open(a.rust)][:a.limit]
    flat = read_into(Flat(), a.plays)
    n = min(len(rust), len(flat))
    w = mx.load(a.weights)
    promos = w['promo'].shape[0] - 1
    d = w['e1.weight'].shape[0]
    layers = len({k.split('.')[1] for k in w if k.startswith('layers.')})
    heads = w['layers.0.dist_bias'].shape[0]
    ff = w['layers.0.f1.weight'].shape[0]
    model = ArmyNet(d=d, heads=heads, layers=layers, ff=ff, promos=promos, map_ch=d if 'm_in.weight' in w else 0)
    model.load_weights(a.weights)
    tm = max(flat.t[:n])
    cm = max(1, max(flat.c[:n]))
    dev = Device(flat, [1.0] * len(flat), tm, cm)
    worst_p = worst_a = 0.0
    checked = 0
    for s in range(0, n, 256):
        idx = mx.arange(s, min(s + 256, n))
        b = dev.batch(idx)
        h, pl, al = forward(model, b)
        mx.eval(pl, al)
        pl = pl.tolist()
        al = al.tolist()
        for k in range(idx.shape[0]):
            i = s + k
            r = rust[i]
            t = flat.t[i]
            for j in range(t):
                if r['ptr'][j] is not None:
                    worst_p = max(worst_p, abs(r['ptr'][j] - pl[k][j]))
            worst_p = max(worst_p, abs(r['ptr'][t] - pl[k][tm]))
            for j, v in enumerate(r['act']):
                worst_a = max(worst_a, abs(v - al[k][j]))
            checked += 1
    print(json.dumps({'decisions': checked, 'max_abs_pointer': worst_p, 'max_abs_action': worst_a, 'ok': worst_p <= 1e-4 and worst_a <= 1e-4}))


if __name__ == '__main__':
    main()
