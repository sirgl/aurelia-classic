"""Per-layer linear probes of an MB / MBA net and the causal check of a concept's subspace (M-HANDOFF.md, backlog 2).

python -m aurelia_nn.army.probe_layers --weights runs/netMBa/weights.safetensors --hold runs/teach4-hold.bin --out OUT.json

On held-out plays (one in --every; the memory unrolled as in training) the residual stream is read after the
embedding (stage 0) and after each transformer layer (1..L):
- danger (the hit points each of our selectable units loses in the enemy's next turn) from the unit's token: ridge
  R^2, and the rank at 90 % (the fewest principal directions of the stream whose ridge keeps 90 % of the R^2);
- the chosen strike's dealt (the rules' preview of the labelled strike) from the acting unit's token: ridge R^2;
- the teacher's stage (staged-assault plays, at the memory states) from the plan token P_0: logistic accuracy.
Fit on the even plays, scored on the odd ones. The causal check: the danger direction (the ridge's, unit norm) is
projected out of every own unit token at a stage, the rest of the net runs on, and the share of decisions whose
choice (unit or action) changes is reported against the same projection of a control direction (the difference of
two units' states, so a direction the stream varies along).
"""
import argparse
import json

import mlx.core as mx
import mlx.nn as nn

from .model import NEG
from .model_m import PLANS
from .model_mb import MBANet, MBNet
from .probe_m import fit
from .train_m import DEC_KEYS, DeviceM, Stream, map_feats, plan_batch, unroll_memory

K_OWN = 1


def layers_of(model, b, um, pm, hook=None):
    """The residual stream after the embedding and after each layer (list of [N, T + PLANS, d]) and the final H;
    `hook(stage, x) -> x` may change the stream after a stage."""
    tok, dist, tmask, slot = b['tok'], b['dist'], b['tmask'], b['slot']
    n, t, _ = tok.shape
    h = model.e2(nn.relu(model.e1(tok)))
    us = um[mx.arange(n)[:, None], mx.maximum(slot, 0)]
    h = h + mx.where((slot >= 0)[..., None], model.um(us), 0.0)
    h = h + mx.where(b['tok_ok'][..., None], model.m_tok(b['gtok']), 0.0)
    top = model.m_pool(b['mmean'])
    if isinstance(model, MBANet):
        top = top + b['price'] @ model.w_price
    h = mx.concatenate([h[:, :1] + top[:, None, :], h[:, 1:]], axis=1)
    x = mx.concatenate([h, pm], axis=1)
    mask = mx.concatenate([tmask, mx.ones((n, PLANS), dtype=mx.bool_)], axis=1)
    is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
    pair_plan = is_plan[:, None] | is_plan[None, :]
    dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
    stages = []
    if hook is not None:
        x = hook(0, x)
    stages.append(x)
    for li, layer in enumerate(model.layers):
        x = model.layer_m(layer, x, dpad, mask, pair_plan, model.plan_bias[li])
        if hook is not None:
            x = hook(li + 1, x)
        stages.append(x)
    return stages, model.norm(x)


def choices(model, b, hfull):
    t = b['tok'].shape[1]
    h = hfull[:, :t]
    pl = model.pointer_logits(h, b['sel'])
    if isinstance(model, MBANet):
        al = model.action_logits_a(h, b['unit'], b['cand'], b['promo'], b['cmask'], b['gcand'], b['cand_ok'])[0]
    else:
        al = model.action_logits_g(h, b['unit'], b['cand'], b['promo'], b['cmask'], b['gcand'], b['cand_ok'])
    return mx.argmax(pl, axis=1), mx.argmax(mx.where(b['cmask'], al, NEG), axis=1)


def ridge(xtr, ytr, xte, yte, lam=1e-2):
    """R^2 on the test rows of a ridge fit (standardised inputs) and the weight in the raw input space."""
    mu, sd = mx.mean(xtr, axis=0), mx.std(xtr, axis=0) + 1e-6
    a, c = (xtr - mu) / sd, (xte - mu) / sd
    ym = mx.mean(ytr)
    d = a.shape[1]
    w = mx.linalg.solve(a.T @ a / a.shape[0] + lam * mx.eye(d), a.T @ (ytr - ym) / a.shape[0], stream=mx.cpu)
    pred = c @ w + ym
    r2 = 1.0 - mx.sum((yte - pred) ** 2) / mx.maximum(mx.sum((yte - mx.mean(yte)) ** 2), 1e-9)
    return float(r2.item()), w / sd


def rank90(xtr, ytr, xte, yte, full):
    """The fewest principal directions of the training stream whose ridge keeps 90 % of the full R^2."""
    mu = mx.mean(xtr, axis=0)
    cov = (xtr - mu).T @ (xtr - mu) / xtr.shape[0]
    _, vecs = mx.linalg.eigh(cov, stream=mx.cpu)
    vecs = vecs[:, ::-1]
    for k in (1, 2, 4, 8, 16, 32, 64):
        if k > vecs.shape[1]:
            break
        p = vecs[:, :k]
        r2, _ = ridge((xtr - mu) @ p, ytr, (xte - mu) @ p, yte)
        if full > 0 and r2 >= 0.9 * full:
            return k
    return None


def projector(project, own):
    """The hook that removes direction u from our unit tokens' stream after stage s (project = (s, u)); None: none."""
    if project is None:
        return None
    s, u = project
    mask = mx.concatenate([own, mx.zeros((own.shape[0], PLANS), dtype=mx.bool_)], axis=1)[..., None]

    def hook(stage, x):
        return x - mx.where(mask, (x @ u)[..., None] * u, 0.0) if stage == s else x
    return hook


def collect(model, stream, ids, tm, project=None):
    """Per stage the probe rows of these plays, and per decision the net's choices and the labels; `project` = (stage,
    direction) projects that direction out of our unit tokens (the causal check)."""
    rows = {'danger': [], 'dealt': [], 'stage': []}
    picks = []
    flat = stream.flat(ids)
    dev = DeviceM(flat, [1.0] * len(flat), tm, 128)
    plan = plan_batch(dev, list(range(len(ids))), 10 ** 6, 0.0, None, tm, train=False)
    Ms, Us, Ps, _, _ = unroll_memory(model, dev, plan, tm, with_aux=False)
    for (tg, cg), (dec, cols, turns) in sorted(plan[3].items()):
        idx = mx.array(dec, mx.int32)
        full = dev.batch_m(idx, tg, cg)
        b = {k: full[k] for k in DEC_KEYS}
        b['pos'], b['cpos'] = full['pos'], full['cpos']
        b['track'] = full['track']
        tt, cc = mx.array(turns, mx.int32), mx.array(cols, mx.int32)
        map_feats(b, Ms, tt, cc)
        hook = projector(project, (b['tok'][..., K_OWN] > 0.5) & b['tmask'])
        stages, hfull = layers_of(model, b, Us[tt, cc], Ps[tt, cc], hook)
        ptr, act = choices(model, b, hfull)
        mx.eval(ptr, act, *stages)
        odd = [flat.plays[flat.play[i]]['instance'] % 2 for i in dec]
        mx.eval(b['ptr'], b['act'], b['has_act'])
        picks.append((ptr, act, b['ptr'], b['act'], b['has_act'], odd))
        t = b['tok'].shape[1]
        own = (b['tok'][..., K_OWN] > 0.5) & b['sel'] & (b['danger'] >= 0)
        strike = mx.take_along_axis((b['cand'][..., 2] + b['cand'][..., 3] + b['cand'][..., 4]), b['act'][:, None], axis=1)[:, 0] > 0.5
        strike = strike & (b['has_act'] > 0)
        dealt = mx.take_along_axis(b['cand'][..., 30], b['act'][:, None], axis=1)[:, 0]
        stage = b['track'][:, 2]
        n = len(dec)
        flat_own = [j * t + k for j, row in enumerate(own.tolist()) for k, v in enumerate(row) if v]
        sel = {'danger': mx.array(flat_own, mx.int32),
               'dealt': mx.array([j for j, v in enumerate(strike.tolist()) if v], mx.int32),
               'stage': mx.array([j for j, v in enumerate(stage.tolist()) if v >= 0], mx.int32)}
        dec_of = {'danger': sel['danger'] // t, 'dealt': sel['dealt'], 'stage': sel['stage']}
        target = {'danger': b['danger'].reshape(-1)[sel['danger']], 'dealt': dealt[sel['dealt']], 'stage': stage[sel['stage']]}
        odd_a = mx.array(odd, mx.int32)
        for s, x in enumerate(stages):
            feats = {'danger': x[:, :t].reshape(n * t, -1)[sel['danger']], 'dealt': x[mx.arange(n), b['unit']][sel['dealt']], 'stage': x[:, t][sel['stage']]}
            for k in rows:
                if sel[k].size:
                    row = (s, odd_a[dec_of[k]], feats[k], target[k])
                    # Evaluated now: a lazy row would keep the whole chunk (its turn maps, the batch) alive.
                    mx.eval(*row[1:])
                    rows[k].append(row)
    return rows, picks


def split(rows, s):
    """(train x, train y, test x, test y) of a stage: even instances train, odd ones test."""
    part = [r for r in rows if r[0] == s]
    if not part:
        return None
    odd = mx.concatenate([r[1] for r in part])
    x = mx.concatenate([r[2] for r in part]).astype(mx.float32)
    y = mx.concatenate([r[3] for r in part]).astype(mx.float32)
    tr = mx.array([i for i, v in enumerate(odd.tolist()) if v == 0], mx.int32)
    te = mx.array([i for i, v in enumerate(odd.tolist()) if v == 1], mx.int32)
    if tr.size == 0 or te.size == 0:
        return None
    return x[tr], y[tr], x[te], y[te]


def agreement(base, other):
    changed = same_n = 0
    for (p0, a0, *_), (p1, a1, *_) in zip(base, other):
        ch = mx.logical_or(p0 != p1, a0 != a1)
        changed += int(mx.sum(ch).item())
        same_n += p0.shape[0]
    return changed / max(same_n, 1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--weights', required=True)
    ap.add_argument('--hold', nargs='+', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--every', type=int, default=16)
    ap.add_argument('--cpu', action='store_true')
    ap.add_argument('--mem-gb', type=float, default=6.0, help='MLX memory limit (match the broker --ram reservation)')
    a = ap.parse_args()
    if a.cpu:
        mx.set_default_device(mx.cpu)
    mx.set_memory_limit(int(a.mem_gb * 2 ** 30))
    mx.set_cache_limit(2 ** 30)
    w = mx.load(a.weights)
    model = (MBANet if 'w_price' in w else MBNet)(promos=w['promo'].shape[0] - 1, map_ch=w['m_in.weight'].shape[0])
    model.load_weights(a.weights)
    stream = Stream(a.hold, a.every)
    tm = stream.t_max
    ids = list(range(len(stream)))
    rows, picks = {'danger': [], 'dealt': [], 'stage': []}, []
    for s in range(0, len(ids), 64):
        r, p = collect(model, stream, ids[s:s + 64], tm)
        mx.clear_cache()
        for k in rows:
            rows[k] += r[k]
        picks += p
    stages = len(model.layers) + 1
    out = {'weights': a.weights, 'plays': len(ids), 'danger': [], 'dealt': [], 'stage': []}
    dirs = {}
    for s in range(stages):
        sp = split(rows['danger'], s)
        if sp:
            r2, wd = ridge(*sp)
            dirs[s] = wd / mx.linalg.norm(wd)
            out['danger'].append({'stage': s, 'r2': round(r2, 3), 'rank90': rank90(*sp, r2), 'n': sp[0].shape[0] + sp[2].shape[0]})
        sp = split(rows['dealt'], s)
        if sp:
            r2, _ = ridge(*sp)
            out['dealt'].append({'stage': s, 'r2': round(r2, 3), 'n': sp[0].shape[0] + sp[2].shape[0]})
        sp = split(rows['stage'], s)
        if sp:
            xtr, ytr, xte, yte = sp
            f = fit(xtr.tolist(), ytr.astype(mx.int32).tolist(), 6)
            acc = float(mx.mean(mx.argmax(f(xte.tolist()), axis=1) == yte.astype(mx.int32)).item())
            major = max(set(yte.tolist()), key=yte.tolist().count)
            out['stage'].append({'stage': s, 'acc': round(acc, 3), 'majority': round(float(mx.mean(yte == major).item()), 3), 'n': xtr.shape[0] + xte.shape[0]})
    # The causal check at stage 1 and the last stage: the danger direction projected out of our unit tokens.
    out['causal'] = []
    mx.random.seed(7)
    for s in (1, stages - 1):
        if s not in dirs:
            continue
        for name in ('danger', 'random'):
            u = dirs[s]
            if name == 'random':
                # A direction the stream itself varies along (the difference of two units' states): a matched control.
                xs = split(rows['danger'], s)[0]
                i, j = mx.random.randint(0, xs.shape[0], (2,)).tolist()
                u = xs[i] - xs[j]
                u = u / mx.linalg.norm(u)
            changed = []
            for st in range(0, len(ids), 64):
                changed += collect(model, stream, ids[st:st + 64], tm, (s, u))[1]
            out['causal'].append({'stage': s, 'direction': name, 'choices_changed': round(agreement(picks, changed), 4)})
    json.dump(out, open(a.out, 'w'), indent=1)
    print(json.dumps(out, indent=1))


if __name__ == '__main__':
    main()
