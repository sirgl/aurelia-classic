"""Logits parity of variant M (MLX, the memory unrolled over each play as in training) and its Rust forward
(`ai-exam army-parity`, which carries the memory decision by decision in the file's order) on recorded plays.

python -m aurelia_nn.army.parity_m --plays FILE --weights OUT/weights.safetensors --rust rust.jsonl [--plays-limit N]
Every decision of the first N whole plays (memory states included) is compared: the pointer's logits and the
labelled unit's action logits; max |delta| <= 1e-4.
"""
import argparse
import json

import mlx.core as mx

from .data import Flat, read_into
from .model_m import MemNet
from .model_mh import MHNet
from .train_m import DeviceM, C_GROUPS, body_any


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--plays', required=True)
    ap.add_argument('--weights', required=True)
    ap.add_argument('--rust', required=True)
    ap.add_argument('--plays-limit', type=int, default=60)
    a = ap.parse_args()
    rust = [json.loads(l) for l in open(a.rust)]
    flat = read_into(Flat(), a.plays)
    w = mx.load(a.weights)
    model = (MHNet if 'h1.weight' in w else MemNet)(promos=w['promo'].shape[0] - 1)
    model.load_weights(a.weights)
    tm = max(flat.t)
    dev = DeviceM(flat, [1.0] * len(flat), tm, 128)
    P = dev.plays
    plays = [p for p in range(min(a.plays_limit, len(flat.plays))) if P.first[p] + P.count[p] <= len(rust)]
    worst_p = worst_a = 0.0
    checked = 0
    turns = 0
    for p in plays:
        ms = P.ms[p]
        um, pm, alive = model.initial(1)
        Us, Ps = [um], [pm]
        def hplan(b, k):
            return mx.stack([Ps[max(k - a, 0)][0, 0] for a in b['hage'][0].tolist()])[None]

        for k, i in enumerate(ms):
            b = dev.batch_m(mx.array([i], mx.int32), tm, 1)
            b['hplan'] = hplan(b, k)
            hf = body_any(model, b, um, pm)
            if isinstance(model, MHNet):
                um, pm, alive, _ = model.step_mh(hf, b['tok'], b['slot'], b['tmask'], um, pm, alive, b['hist'], b['hmask'])
            else:
                um, pm, alive, _ = model.step(hf, b['tok'], b['slot'], b['tmask'], um, pm, alive)
            Us.append(um)
            Ps.append(pm)
        turns += len(ms)
        for i in range(P.first[p], P.first[p] + P.count[p]):
            r = rust[i]
            c = max(1, flat.c[i])
            cg = next(x for x in C_GROUPS if c <= x)
            b = dev.batch_m(mx.array([i], mx.int32), tm, cg)
            k = P.turn[i]
            b['hplan'] = hplan(b, k)
            hf = body_any(model, b, Us[k], Ps[k])
            h = hf[:, :tm]
            pl = model.pointer_logits(h, b['sel'])[0].tolist()
            t = flat.t[i]
            for j in range(t):
                if r['ptr'][j] is not None:
                    worst_p = max(worst_p, abs(r['ptr'][j] - pl[j]))
            worst_p = max(worst_p, abs(r['ptr'][t] - pl[tm]))
            if flat.ptr[i] >= 0:
                al = model.action_logits(h, b['unit'], b['cand'], b['promo'], b['cmask'])[0].tolist()
                for j, v in enumerate(r['act'][:len(al)]):
                    worst_a = max(worst_a, abs(v - al[j]))
            checked += 1
    print(json.dumps({'plays': len(plays), 'decisions': checked, 'memory_updates': turns, 'max_abs_pointer': worst_p, 'max_abs_action': worst_a, 'ok': worst_p <= 1e-4 and worst_a <= 1e-4}))


if __name__ == '__main__':
    main()
