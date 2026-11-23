"""Proof (b) of variants M / MH: linear probes of the teacher's own state from the net's memory on held-out plays.

python -m aurelia_nn.army.probe_m --plays runs/teach4-hold.bin --weights runs/netM1/weights.safetensors --out probe.json

The memory is unrolled over each held-out play of a staged-assault teacher (the track is known at its memory
states), and after each update the probe reads:
- `engaged` (the assault went in) from a unit's slot memory u_s (16), from the plan tokens (4 x 64), and, as the
  baseline of what the present already tells, from the moment's raw numbers (the objective token and the mean of
  the army's tokens);
- the stage (gather, march, stage, assault, heal-rotate) from the plan tokens, and (MH) from the plan tokens with the
  history's numbers summed by kind.
Logistic regression (MLX, L2 1e-3) fit on the even plays, scored on the odd ones: AUC for engaged, accuracy for
the stage (and the majority class beside it).
"""
import argparse
import collections
import json

import mlx.core as mx
import mlx.nn as nn

from .data import Flat, F_TOK, read_into
from .model_m import MemNet
from .model_mh import MHNet
from .train_m import DeviceM, UNROLL_KEYS, body_any, plan_batch


def fit(x, y, classes, steps=600, lr=0.05, l2=1e-3):
    x = mx.array(x, mx.float32)
    mu, sd = mx.mean(x, axis=0), mx.std(x, axis=0) + 1e-6
    x = (x - mu) / sd
    y = mx.array(y, mx.int32)
    w = mx.zeros((x.shape[1], classes))
    b = mx.zeros((classes,))

    def loss(w, b):
        lg = x @ w + b
        return mx.mean(mx.logsumexp(lg, axis=1) - mx.take_along_axis(lg, y[:, None], axis=1)[:, 0]) + l2 * mx.sum(w * w)

    g = mx.grad(loss, argnums=(0, 1))
    for _ in range(steps):
        gw, gb = g(w, b)
        w, b = w - lr * gw, b - lr * gb
        mx.eval(w, b)
    return lambda z: ((mx.array(z, mx.float32) - mu) / sd) @ w + b


def auc(scores, labels):
    pairs = sorted(zip(scores, labels))
    pos = sum(labels)
    neg = len(labels) - pos
    if pos == 0 or neg == 0:
        return None
    rank_sum = 0.0
    i = 0
    while i < len(pairs):
        j = i
        while j < len(pairs) and pairs[j][0] == pairs[i][0]:
            j += 1
        r = (i + j + 1) / 2.0
        rank_sum += r * sum(l for _, l in pairs[i:j])
        i = j
    return (rank_sum - pos * (pos + 1) / 2) / (pos * neg)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--plays', nargs='+', required=True)
    ap.add_argument('--weights', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--cpu', action='store_true')
    a = ap.parse_args()
    if a.cpu:
        mx.set_default_device(mx.cpu)
    flat = Flat()
    for p in a.plays:
        read_into(flat, p, lambda m: m['family'] in {'assault-heal', 'promotion-choice', 'hill-archers', 'city-fire', 'planned-assault'} and m['player'].startswith('ref-'))
    w = mx.load(a.weights)
    model = (MHNet if 'h1.weight' in w else MemNet)(promos=w['promo'].shape[0] - 1)
    model.load_weights(a.weights)
    tm = max(flat.t)
    dev = DeviceM(flat, [1.0] * len(flat), tm, 128)
    rows = collections.defaultdict(list)
    plays = list(range(len(flat.plays)))
    for s in range(0, len(plays), 256):
        chunk = plays[s:s + 256]
        R, ms, reset, _, _ = plan_batch(dev, chunk, 10 ** 6, 0.0, None, tm, train=False)
        B = len(chunk)
        um, pm, alive = model.initial(B)
        Ps = [pm]
        for r in range(R):
            idx = mx.array(ms[r], mx.int32)
            valid = (idx >= 0).tolist()
            full = dev.batch_m(mx.maximum(idx, 0), tm, 1)
            b = {k: full[k] for k in UNROLL_KEYS}
            p0s = mx.stack([P[:, 0] for P in Ps[::-1][:4]] + [Ps[0][:, 0]] * max(0, 4 - len(Ps)), axis=1)
            b['hplan'] = p0s[mx.arange(B)[:, None], mx.minimum(full['hage'], 3)]
            hf = body_any(model, b, um, pm)
            if isinstance(model, MHNet):
                um2, pm2, al2, _ = model.step_mh(hf, b['tok'], b['slot'], b['tmask'], um, pm, alive, b['hist'], b['hmask'])
            else:
                um2, pm2, al2, _ = model.step(hf, b['tok'], b['slot'], b['tmask'], um, pm, alive)
            v = mx.array(valid)
            um = mx.where(v[:, None, None], um2, um)
            pm = mx.where(v[:, None, None], pm2, pm)
            alive = mx.where(v[:, None], al2, alive)
            Ps.append(pm)
            mx.eval(um, pm)
            tr = full['track'].tolist()
            slots = full['slot'].tolist()
            tok = full['tok'].tolist()
            hist = full['hist'].tolist()
            hmask = full['hmask'].tolist()
            U = um.tolist()
            Pl = pm.reshape(B, -1).tolist()
            for c in range(B):
                if not valid[c] or tr[c][0] < 0:
                    continue
                test = chunk[c] % 2
                eng, stage, rest = tr[c][0], tr[c][2], tr[c][3] & 0xFFFF
                army = [i for i, sl in enumerate(slots[c]) if sl >= 0]
                present = tok[c][0] + ([sum(tok[c][i][k] for i in army) / max(len(army), 1) for k in range(F_TOK)])
                hsum = [sum(hist[c][j][k] for j in range(len(hmask[c])) if hmask[c][j]) for k in range(20)]
                rows['plan'].append((test, Pl[c], eng, stage))
                rows['present'].append((test, present, eng, stage))
                rows['plan_hist'].append((test, Pl[c] + hsum, eng, stage))
                for i in army:
                    sl = slots[c][i]
                    rows['unit'].append((test, U[c][sl], eng, (rest >> sl) & 1))
    out = {'weights': a.weights, 'plays': len(plays)}
    for name, data in rows.items():
        tr_ = [r for r in data if r[0] == 0]
        te = [r for r in data if r[0] == 1]
        res = {'train': len(tr_), 'test': len(te)}
        f = fit([r[1] for r in tr_], [r[2] for r in tr_], 2)
        sc = (f([r[1] for r in te])[:, 1] - f([r[1] for r in te])[:, 0]).tolist()
        res['engaged_auc'] = auc(sc, [r[2] for r in te])
        if name == 'unit':
            f = fit([r[1] for r in tr_], [r[3] for r in tr_], 2)
            sc = (f([r[1] for r in te])[:, 1] - f([r[1] for r in te])[:, 0]).tolist()
            res['resting_auc'] = auc(sc, [r[3] for r in te])
        else:
            f = fit([r[1] for r in tr_], [r[3] for r in tr_], 6)
            pred = mx.argmax(f([r[1] for r in te]), axis=1).tolist()
            res['stage_acc'] = sum(int(p == r[3]) for p, r in zip(pred, te)) / max(len(te), 1)
            maj = collections.Counter(r[3] for r in tr_).most_common(1)[0][0]
            res['stage_majority'] = sum(int(r[3] == maj) for r in te) / max(len(te), 1)
        out[name] = res
        print(name, json.dumps(res), flush=True)
    json.dump(out, open(a.out, 'w'), indent=1)


if __name__ == '__main__':
    main()
