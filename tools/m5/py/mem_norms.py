"""Task 1 (c), 24 Nov: does the memory reach the army tokens, and is the map drowning it?

python tools/m5/py/mem_norms.py OUT.json NAME=PATH/weights.safetensors ...  (CPU, held-out assault plays; run from anywhere,
the data is $AURELIA_LAB/runs/teach4-hold.bin)

For each net, over the held-out plays of the assault families (teach4-hold.bin, the scripted references), the memory
is unrolled as in training (`train_m.unroll_memory`), and at every memory-state decision the army tokens' input
embedding is split into its parts: the token's own embedding e = e2(relu(e1(tok))), the memory's um(u_s), and (MB)
the map's m_tok(M[tile]). Reported: the mean L2 norm of each part per army token, the ratio memory / map, the mean
|u_s| per named slot dimension, the attention mass the tokens give to the plan tokens per layer, and the weight
norms of um, m_tok, m_pool, m_cand.
"""
import json
import sys

import os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', 'nn'))
import mlx.core as mx
import mlx.nn as nn

mx.set_default_device(mx.cpu)
mx.set_memory_limit(8 * 2 ** 30)
mx.set_cache_limit(1 * 2 ** 30)

from aurelia_nn.army.model_m import MemNet, PLANS
from aurelia_nn.army.model_mb import MBANet, MBHNet, MBNet
from aurelia_nn.army.train_m import DeviceM, Stream, UNROLL_KEYS, map_feats, plan_batch, unroll_memory

R = os.path.join(os.environ.get('AURELIA_LAB') or os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', 'lab'), 'runs')
FAMS = {'assault-heal', 'planned-assault'}


def build(path):
    w = mx.load(path)
    promos = w['promo'].shape[0] - 1
    shape = dict(d=w['e2.weight'].shape[0], heads=4, layers=1 + max(int(k.split('.')[1]) for k in w if k.startswith('layers.')),
                 ff=w['layers.0.f1.weight'].shape[0], promos=promos)
    if 'hx_slope' in w:
        m = MBHNet(map_ch=w['m_tok.weight'].shape[1], **shape)
    elif 'w_price' in w:
        m = MBANet(map_ch=w['m_tok.weight'].shape[1], **shape)
    elif 'm_tok.weight' in w:
        m = MBNet(map_ch=w['m_tok.weight'].shape[1], **shape)
    else:
        m = MemNet(**shape)
    m.load_weights(list(w.items()), strict=False)
    mx.eval(m.parameters())
    return m


def plan_attention(model, b, um, pm):
    """Mean attention mass of the real tokens on the plan tokens, per layer (MemNet.layer_m re-done with weights)."""
    import math
    from aurelia_nn.army.model import NEG
    tok, dist, tmask, slot = b['tok'], b['dist'], b['tmask'], b['slot']
    bsz, t, _ = tok.shape
    h = model.e2(nn.relu(model.e1(tok)))
    has = slot >= 0
    us = um[mx.arange(bsz)[:, None], mx.maximum(slot, 0)]
    h = h + mx.where(has[..., None], model.um(us), 0.0)
    if 'gtok' in b:
        h = h + mx.where(b['tok_ok'][..., None], model.m_tok(b['gtok']), 0.0)
        pooled = model.m_pool(b['mmean'])
        h = mx.concatenate([h[:, :1] + pooled[:, None, :], h[:, 1:]], axis=1)
    x = mx.concatenate([h, pm], axis=1)
    mask = mx.concatenate([tmask, mx.ones((bsz, PLANS), dtype=mx.bool_)], axis=1)
    is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
    pair_plan = is_plan[:, None] | is_plan[None, :]
    dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
    out = []
    for li, layer in enumerate(model.layers):
        heads = layer.heads
        d = x.shape[-1]
        dh = d // heads
        a = layer.norm1(x)
        q = layer.q(a).reshape(bsz, t + PLANS, heads, dh).transpose(0, 2, 1, 3)
        k = layer.k(a).reshape(bsz, t + PLANS, heads, dh).transpose(0, 2, 1, 3)
        s = (q @ k.transpose(0, 1, 3, 2)) * (1.0 / math.sqrt(dh))
        bias = layer.dist_bias[:, dpad].transpose(1, 0, 2, 3)
        bias = mx.where(pair_plan[None, None], model.plan_bias[li][None, :, None, None], bias)
        s = s + bias + mx.where(mask[:, None, None, :], 0.0, NEG)
        wgt = mx.softmax(s, axis=-1)  # [b, h, T+P, T+P]
        mass = mx.sum(wgt[:, :, :t, t:], axis=-1)  # [b, h, T]
        sel = (tmask & has)[:, None, :]
        out.append(float(mx.sum(mx.where(sel, mass, 0.0)) / mx.maximum(mx.sum(sel.astype(mx.float32)) * heads, 1)))
        x = model.layer_m(layer, x, dpad, mask, pair_plan, model.plan_bias[li])
    return out


def main():
    out_path = sys.argv[1]
    nets = [a.split('=', 1) for a in sys.argv[2:]]
    hold = Stream([R + '/teach4-hold.bin'])
    ids = [i for i, m in enumerate(hold.metas) if m['family'] in FAMS and m['player'].startswith('ref-')][:256]
    flat = hold.flat(ids)
    tm = hold.t_max
    dev = DeviceM(flat, [1.0] * len(flat), tm, 128)
    res = {'plays': len(ids), 'families': sorted(FAMS)}
    for name, path in nets:
        model = build(path)
        acc = {'e': 0.0, 'mem': 0.0, 'map': 0.0, 'n': 0.0, 'u_abs': [0.0] * 16, 'plan_att': None, 'n_att': 0}
        for s in range(0, len(ids), 64):
            chunk = list(range(s, min(s + 64, len(ids))))
            plan = plan_batch(dev, chunk, 10 ** 6, 0.0, None, tm, train=False)
            Ms, Us, Ps, _, _ = unroll_memory(model, dev, plan, tm, with_aux=False, compile_=False)
            Rn, ms = plan[0], plan[1]
            B = len(chunk)
            for r in range(Rn):
                idx = mx.array(ms[r], mx.int32)
                valid = idx >= 0
                full = dev.batch_m(mx.maximum(idx, 0), tm, 1)
                b = {k: full[k] for k in UNROLL_KEYS}
                um, pm = Us[r], Ps[r]
                if Ms is not None:
                    b['pos'] = full['pos']
                    map_feats(b, Ms, mx.full((B,), r, mx.int32), mx.arange(B))
                sel = (b['slot'] >= 0) & b['tmask'] & valid[:, None]
                w = sel.astype(mx.float32)
                e = model.e2(nn.relu(model.e1(b['tok'])))
                us = um[mx.arange(B)[:, None], mx.maximum(b['slot'], 0)]
                mem = model.um(us)
                nrm = lambda v: mx.sqrt(mx.sum(v * v, axis=-1))
                acc['e'] += float(mx.sum(nrm(e) * w))
                acc['mem'] += float(mx.sum(nrm(mem) * w))
                if Ms is not None:
                    mp = mx.where(b['tok_ok'][..., None], model.m_tok(b['gtok']), 0.0)
                    acc['map'] += float(mx.sum(nrm(mp) * w))
                ua = mx.sum(mx.abs(us) * w[..., None], axis=(0, 1)).tolist()
                acc['u_abs'] = [x + y for x, y in zip(acc['u_abs'], ua)]
                acc['n'] += float(mx.sum(w))
                if r in (2, 6) and r < Rn:
                    pa = plan_attention(model, b, um, pm)
                    acc['plan_att'] = pa if acc['plan_att'] is None else [x + y for x, y in zip(acc['plan_att'], pa)]
                    acc['n_att'] += 1
        n = max(acc['n'], 1)
        row = {'tokens': int(acc['n']), 'norm_e': acc['e'] / n, 'norm_mem': acc['mem'] / n,
               'norm_map': acc['map'] / n if acc['map'] else None,
               'mem_over_map': (acc['mem'] / acc['map']) if acc['map'] else None,
               'mem_over_e': acc['mem'] / acc['e'],
               'u_abs': [round(x / n, 4) for x in acc['u_abs']],
               'plan_attention_mass': [round(x / max(acc['n_att'], 1), 4) for x in (acc['plan_att'] or [])],
               'plan_bias': model.plan_bias.tolist()}
        wn = lambda k: float(mx.sqrt(mx.sum(dict(nn.utils.tree_flatten(model.parameters()))[k] ** 2)))
        row['weight_norms'] = {k: wn(k) for k in ('um.weight', 'm_tok.weight', 'm_pool.weight', 'm_cand.weight', 'e2.weight')
                               if k in dict(nn.utils.tree_flatten(model.parameters()))}
        res[name] = row
        print(name, json.dumps({k: (round(v, 3) if isinstance(v, float) else v) for k, v in row.items()}), flush=True)
    json.dump(res, open(out_path, 'w'), indent=1)


if __name__ == '__main__':
    main()
