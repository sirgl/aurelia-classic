"""What the layers of a (deepened) MB army net carry (net-deep, GATES.md D1, "What the new layers do").

python -m aurelia_nn.army.deep_probe --weights W.safetensors --hold teach-r-train.bin --every 30 --out OUT.json [--cpu]

On recorded teacher plays (the memory unrolled as in training; even instances fit, odd scored) the residual stream is read
after the embedding (stage 0) and after each layer (1..L). Concepts, each with an exact label from the record:
- stage: the teacher's plan stage (track), from the plan token P_0 at memory states (6 classes, accuracy);
- n_fire: the number of our units standing in a foe city's fire (sum of their token number 36), from the objective token 0
  (ridge R^2);
- falls: the objective city falls in this turn (the play was won and this is its last round; only while the objective is
  a foe city), from token 0 (logistic accuracy and AUC);
- near_foe: hex distance from our selectable unit to the nearest foe unit token (clipped 15), from the unit's token (R^2);
- danger: the hit points the unit loses in the enemy's next turn (recorded), from the unit's token (R^2);
- target: the labelled command's class (strike the objective city / strike a unit / move / other), from the acting unit's
  token (4 classes, accuracy).
Beside: per layer the attention atlas (queries = our selectable units: mass by hex distance, on the plan tokens, on token 0,
entropy; per head), the output norms |attention out| / |h| and |MLP out| / |h| per family, and for modern layers the mean
gate opening sigmoid(g).
"""
import argparse
import collections
import json
import math

import mlx.core as mx
import mlx.nn as nn

from .model import NEG, ModernLayer
from .model_m import PLANS
from .probe_layers import ridge
from .probe_m import auc, fit
from .train_m import DEC_KEYS, DeviceM, Stream, map_feats, plan_batch, unroll_memory

CONCEPTS = ('stage', 'n_fire', 'falls', 'near_foe', 'danger', 'target')
CLASSES = {'stage': 6, 'falls': 2, 'target': 4}


def layer_parts(layer, h, dist, key_mask, pair_plan, pbias):
    """One layer as `MemNet.layer_m`, returning (h', attention weights [b, heads, n, n], attention out, MLP out, gate or None)."""
    b, t, d = h.shape
    heads = layer.heads
    dh = d // heads
    bias = layer.dist_bias[:, dist].transpose(1, 0, 2, 3)
    bias = mx.where(pair_plan[None, None], pbias[None, :, None, None], bias)
    modern = isinstance(layer, ModernLayer)
    a = layer.rms1(h) if modern else layer.norm1(h)
    q = layer.q(a).reshape(b, t, heads, dh)
    k = layer.k(a).reshape(b, t, heads, dh)
    if modern:
        q, k = layer.qn(q), layer.kn(k)
    q, k = q.transpose(0, 2, 1, 3), k.transpose(0, 2, 1, 3)
    v = layer.v(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
    s = (q @ k.transpose(0, 1, 3, 2)) * (1.0 / math.sqrt(dh)) + bias + mx.where(key_mask[:, None, None, :], 0.0, NEG)
    w = mx.softmax(s, axis=-1)
    ctx = (w @ v).transpose(0, 2, 1, 3).reshape(b, t, d)
    gate = None
    if modern:
        gate = mx.sigmoid(layer.g(a))
        ctx = ctx * gate
    att = layer.o(ctx)
    h = h + att
    if modern:
        m = layer.rms2(h)
        mlp = layer.w2(nn.silu(layer.w1(m)) * layer.w3(m))
    else:
        mlp = layer.f2(nn.relu(layer.f1(layer.norm2(h))))
    return h + mlp, w, att, mlp, gate


def stream_of(model, b, um, pm):
    """The embedding as `body_mbg` and the parts of every layer."""
    tok, dist, tmask, slot = b['tok'], b['dist'], b['tmask'], b['slot']
    n, t, _ = tok.shape
    h = model.e2(nn.relu(model.e1(tok)))
    us = um[mx.arange(n)[:, None], mx.maximum(slot, 0)]
    h = h + mx.where((slot >= 0)[..., None], model.um(us), 0.0)
    h = h + mx.where(b['tok_ok'][..., None], model.m_tok(b['gtok']), 0.0)
    top = model.m_pool(b['mmean'])
    h = mx.concatenate([h[:, :1] + top[:, None, :], h[:, 1:]], axis=1)
    x = mx.concatenate([h, pm], axis=1)
    mask = mx.concatenate([tmask, mx.ones((n, PLANS), dtype=mx.bool_)], axis=1)
    is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
    pair_plan = is_plan[:, None] | is_plan[None, :]
    dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
    stages, parts = [x], []
    for li, layer in enumerate(model.layers):
        x, w, att, mlp, gate = layer_parts(layer, x, dpad, mask, pair_plan, model.plan_bias[li])
        stages.append(x)
        parts.append((w, att, mlp, gate))
    return stages, parts


class Acc:
    """Running sums of the atlas and the norms."""

    def __init__(self, layers, heads):
        self.L, self.H = layers, heads
        z = lambda *s: [[0.0] * s[-1] for _ in range(s[0])] if len(s) == 2 else [0.0] * s[0]
        self.dist = [[[0.0] * 16 for _ in range(heads)] for _ in range(layers)]
        self.plan = [z(heads) for _ in range(layers)]
        self.obj = [z(heads) for _ in range(layers)]
        self.ent = [z(heads) for _ in range(layers)]
        self.q = 0
        self.norm = collections.defaultdict(lambda: [0.0, 0.0, 0.0, 0.0])  # (layer, family) -> att, mlp, gate, count

    def add(self, b, stages, parts, fam):
        t = b['tok'].shape[1]
        own = ((b['tok'][..., 1] > 0.5) & b['sel']).astype(mx.float32)  # [n, t] queries
        nq = mx.sum(own).item()
        if nq == 0:
            return
        self.q += nq
        oh = (b['dist'][..., None] == mx.arange(16)[None, None, None, :]).astype(mx.float32)  # [n, t, t, 16]
        famw = {f: mx.array([1.0 if x == f else 0.0 for x in fam]) for f in set(fam)}
        real = b['tmask'].astype(mx.float32)
        for li, (w, att, mlp, gate) in enumerate(parts):
            wt = w[:, :, :t, :t]  # token queries to token keys
            m = own[:, None, :, None]
            dmass = mx.einsum('bhij,bijk->bhk', wt * m, oh)  # [n, heads, 16]
            pmass = mx.sum(w[:, :, :t, t:] * m, axis=(0, 2, 3))
            omass = mx.sum(w[:, :, :t, 0] * own[:, None, :], axis=(0, 2))
            ent = -mx.sum(mx.where(w[:, :, :t] > 0, w[:, :, :t] * mx.log(mx.maximum(w[:, :, :t], 1e-30)), 0.0), axis=-1)
            ent = mx.sum(ent * own[:, None, :], axis=(0, 2))
            ds = mx.sum(dmass, axis=0).tolist()
            for hd in range(self.H):
                for k in range(16):
                    self.dist[li][hd][k] += ds[hd][k]
            for arr, val in ((self.plan[li], pmass.tolist()), (self.obj[li], omass.tolist()), (self.ent[li], ent.tolist())):
                for hd in range(self.H):
                    arr[hd] += val[hd]
            hin = stages[li][:, :t]
            nin = mx.sqrt(mx.sum(hin * hin, axis=-1)) + 1e-6
            ra = mx.sqrt(mx.sum(att[:, :t] ** 2, axis=-1)) / nin
            rm = mx.sqrt(mx.sum(mlp[:, :t] ** 2, axis=-1)) / nin
            rg = mx.mean(gate[:, :t], axis=-1) if gate is not None else mx.zeros(ra.shape)
            for f, fw in famw.items():
                ww = real * fw[:, None]
                c = mx.sum(ww).item()
                acc = self.norm[(li, f)]
                acc[0] += mx.sum(ra * ww).item()
                acc[1] += mx.sum(rm * ww).item()
                acc[2] += mx.sum(rg * ww).item()
                acc[3] += c

    def result(self):
        q = max(self.q, 1)
        out = {'queries': self.q, 'layers': []}
        for li in range(self.L):
            out['layers'].append({'dist_mass': [[round(x / q, 4) for x in self.dist[li][hd]] for hd in range(self.H)],
                                  'plan_mass': [round(x / q, 4) for x in self.plan[li]], 'objective_mass': [round(x / q, 4) for x in self.obj[li]],
                                  'entropy': [round(x / q, 3) for x in self.ent[li]]})
        norms = collections.defaultdict(dict)
        for (li, f), (a, m, g, c) in sorted(self.norm.items()):
            norms[li][f] = {'att': round(a / c, 4), 'mlp': round(m / c, 4), 'gate': round(g / c, 4), 'tokens': int(c)}
        out['norms'] = norms
        return out


def collect(model, stream, ids, tm, acc, rows):
    flat = stream.flat(ids)
    dev = DeviceM(flat, [1.0] * len(flat), tm, 128)
    plan = plan_batch(dev, list(range(len(ids))), 10 ** 6, 0.0, None, tm, train=False)
    Ms, Us, Ps, _, _ = unroll_memory(model, dev, plan, tm, with_aux=False)
    last = {}
    for i in range(len(flat.play)):
        p = flat.play[i]
        last[p] = max(last.get(p, 0), flat.round[i])
    for (tg, cg), (dec, cols, turns) in sorted(plan[3].items()):
        idx = mx.array(dec, mx.int32)
        full = dev.batch_m(idx, tg, cg)
        b = {k: full[k] for k in DEC_KEYS}
        b['pos'], b['cpos'], b['track'] = full['pos'], full['cpos'], full['track']
        tt, cc = mx.array(turns, mx.int32), mx.array(cols, mx.int32)
        map_feats(b, Ms, tt, cc)
        stages, parts = stream_of(model, b, Us[tt, cc], Ps[tt, cc])
        mx.eval(*stages)
        meta = [flat.plays[flat.play[i]] for i in dec]
        fam = [m['family'] for m in meta]
        acc.add(b, stages, parts, fam)
        n, t = b['tok'].shape[0], b['tok'].shape[1]
        tok, tmask = b['tok'], b['tmask']
        odd = mx.array([m['instance'] % 2 for m in meta], mx.int32)
        own = (tok[..., 1] > 0.5) & tmask
        foe = (tok[..., 2] > 0.5) & tmask
        objcity = (tok[..., 3] > 0.5) & (tok[..., 43] > 0.5) & tmask
        foe_obj = mx.any(objcity & (tok[..., 45] < 0.5), axis=1)
        won = mx.array([1.0 if m.get('win', 0) else 0.0 for m in meta])
        is_last = mx.array([1.0 if flat.round[i] == last[flat.play[i]] else 0.0 for i in dec])
        falls = (won * is_last).astype(mx.int32)
        n_fire = mx.sum(mx.where(own, tok[..., 36], 0.0), axis=1)
        dist = b['dist'].astype(mx.float32)
        near = mx.min(mx.where(foe[:, None, :], dist, 15.0), axis=2)  # [n, t]
        units = own & b['sel']
        ar = mx.arange(n)
        act = mx.maximum(b['act'], 0)
        ck = b['cand'][ar, act]  # [n, F_ACT]
        strike = (ck[:, 2] + ck[:, 3] + ck[:, 4]) > 0.5
        cpos = b['cpos'][ar, act]
        first_city = mx.argmax(objcity.astype(mx.int32), axis=1)
        city_pos = b['pos'][ar, first_city]
        on_city = mx.any(objcity, axis=1) & (cpos == city_pos)
        target = mx.where(strike & on_city, 0, mx.where(strike, 1, mx.where(ck[:, 1] > 0.5, 2, 3)))
        has_act = b['has_act'] > 0
        stage = b['track'][:, 2]
        sel = {
            'stage': ([j for j, v in enumerate(stage.tolist()) if v >= 0], 'plan'),
            'n_fire': (list(range(n)), 'obj'),
            'falls': ([j for j, v in enumerate(foe_obj.tolist()) if v], 'obj'),
            'near_foe': ([j * t + k for j, row in enumerate((units & mx.any(foe, axis=1)[:, None]).tolist()) for k, v in enumerate(row) if v], 'tok'),
            'danger': ([j * t + k for j, row in enumerate((units & (b['danger'] >= 0)).tolist()) for k, v in enumerate(row) if v], 'tok'),
            'target': ([j for j, v in enumerate(has_act.tolist()) if v], 'unit'),
        }
        ys = {'stage': stage, 'n_fire': n_fire, 'falls': falls, 'near_foe': near.reshape(-1), 'danger': b['danger'].reshape(-1), 'target': target}
        for c, (ix, where) in sel.items():
            if not ix:
                continue
            ixa = mx.array(ix, mx.int32)
            dec_of = ixa // t if where == 'tok' else ixa
            y = ys[c][ixa]
            fam_of = [fam[j] for j in dec_of.tolist()]
            for s, x in enumerate(stages):
                if where == 'plan':
                    f = x[:, t][ixa]
                elif where == 'obj':
                    f = x[:, 0][ixa]
                elif where == 'tok':
                    f = x[:, :t].reshape(n * t, -1)[ixa]
                else:
                    f = x[ar, b['unit']][ixa]
                row = (odd[dec_of], f.astype(mx.float32), y)
                mx.eval(*row)
                rows[c][s].append(row)
            rows[c]['fam'] += fam_of


def score(rows, c, s, cap):
    part = rows[c][s]
    odd = mx.concatenate([r[0] for r in part])
    x = mx.concatenate([r[1] for r in part])
    y = mx.concatenate([r[2] for r in part])
    o = odd.tolist()
    tr = [i for i, v in enumerate(o) if v == 0][:cap]
    te = [i for i, v in enumerate(o) if v == 1][:cap]
    if len(tr) < 50 or len(te) < 50:
        return None
    tr, te = mx.array(tr, mx.int32), mx.array(te, mx.int32)
    if c in CLASSES:
        yi = y.astype(mx.int32)
        f = fit(x[tr], yi[tr], CLASSES[c])
        lg = f(x[te])
        pred = mx.argmax(lg, axis=1)
        accu = mx.mean((pred == yi[te]).astype(mx.float32)).item()
        major = max(collections.Counter(yi[te].tolist()).values()) / te.size
        out = {'acc': round(accu, 4), 'majority': round(major, 4), 'n_test': int(te.size)}
        if CLASSES[c] == 2:
            sc = (lg[:, 1] - lg[:, 0]).tolist()
            a = auc(sc, yi[te].tolist())
            out['auc'] = round(a, 4) if a is not None else None
        return out
    r2, _ = ridge(x[tr], y[tr].astype(mx.float32), x[te], y[te].astype(mx.float32))
    return {'r2': round(r2, 4), 'n_test': int(te.size)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--weights', required=True)
    ap.add_argument('--hold', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--every', type=int, default=30)
    ap.add_argument('--chunk', type=int, default=48)
    ap.add_argument('--cap', type=int, default=60000)
    ap.add_argument('--cpu', action='store_true')
    ap.add_argument('--mem-gb', type=float, default=12.0)
    a = ap.parse_args()
    if a.cpu:
        mx.set_default_device(mx.cpu)
    mx.set_memory_limit(int(a.mem_gb * 2 ** 30))
    mx.set_cache_limit(2 ** 30)
    from aurelia_nn.tacsim.armyloop import load_net
    model = load_net(a.weights)
    L = len(model.layers)
    stream = Stream([a.hold])
    ids = list(range(0, len(stream), a.every))
    rows = {c: collections.defaultdict(list) for c in CONCEPTS}
    for c in CONCEPTS:
        rows[c]['fam'] = []
    acc = Acc(L, model.config['heads'])
    for i in range(0, len(ids), a.chunk):
        collect(model, stream, ids[i:i + a.chunk], stream.t_max, acc, rows)
        print(f'plays {min(i + a.chunk, len(ids))} / {len(ids)}', flush=True)
    probes = {c: {s: score(rows, c, s, a.cap) for s in range(L + 1) if rows[c][s]} for c in CONCEPTS}
    readable = {}
    for c, per in probes.items():
        key = 'auc' if CLASSES.get(c) == 2 else ('acc' if c in CLASSES else 'r2')
        vals = {s: v[key] for s, v in per.items() if v}
        if vals:
            best = max(vals.values())
            readable[c] = min(s for s, v in vals.items() if v >= 0.95 * best) if best > 0 else None
    out = {'weights': a.weights, 'layers': L, 'kinds': model.config.get('kinds', 'c' * L), 'plays': len(ids), 'probes': probes, 'readable_at': readable,
           'concept_rows': {c: len(rows[c]['fam']) for c in CONCEPTS}}
    out.update(acc.result())
    json.dump(out, open(a.out, 'w'), indent=1)
    print(json.dumps({'probes': probes, 'readable_at': readable}))


if __name__ == '__main__':
    main()
