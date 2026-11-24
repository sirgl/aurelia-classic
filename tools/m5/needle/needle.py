"""The needle test (research plan step 1.1): the smallest pre-LN transformer (width d, layers L) that
retrieves facts from a context of N tokens at >= 95 % accuracy, with hard distractors, for 1 / 3 / 5 needles.

Each token is a fact (entity, attribute, value) embedded as the sum of three embeddings plus a learned position
embedding. The context holds N - k facts, then k query tokens (entity, attribute, QUERY marker). The k query pairs
have distinct entities and distinct attributes; the context holds exactly one fact for each query pair (the needle).
Half of the distractors are hard: they share the entity OR the attribute with a query, never both.

Usage: needle.py run|summary|smoke --out DIR ...  (MLX and the standard library only).
"""
import argparse
import json
import math
import os
import time

import mlx.core as mx
import mlx.nn as nn
import mlx.optimizers as optim
from mlx.utils import tree_flatten

N_ENT, N_ATT, N_VAL = 64, 8, 32
QUERY = N_VAL  # the value id of a query token
HARD_SHARE = 0.5  # share of the distractors that share exactly one field with a query (>= 0.3 required)
WIDTHS, LAYERS, HEADS = (64, 96, 128), (3, 4, 6), 4
PASS = 0.95
EARLY = 0.99


# ---------------------------------------------------------------- data

def _ri(key, shape, hi):
    return mx.random.randint(0, hi, shape, key=key)


def make_batch(key, b, n, k):
    """(ent, att, val, targets, is_hard): int32 [b, n] x3, [b, k], bool [b, n - k] (the context's hard distractors).

    Context positions 0..n-k-1, queries n-k..n-1; the target of query i is the value of its needle."""
    c = n - k
    ks = mx.random.split(key, 13)
    # k distinct entities and k distinct attributes per sequence: the first k of a random permutation.
    qe = mx.argsort(mx.random.uniform(shape=(b, N_ENT), key=ks[0]), axis=1)[:, :k]
    qa = mx.argsort(mx.random.uniform(shape=(b, N_ATT), key=ks[1]), axis=1)[:, :k]
    qe, qa = qe.astype(mx.int32), qa.astype(mx.int32)
    # Every distractor is tied to a random query i; kind 0 = easy, 1 = shares the entity, 2 = shares the attribute.
    tie = _ri(ks[2], (b, c), k)
    hard = mx.random.uniform(shape=(b, c), key=ks[3]) < HARD_SHARE
    which = _ri(ks[4], (b, c), 2)  # 0: share the entity, 1: share the attribute
    te = mx.take_along_axis(qe, tie, axis=1)
    ta = mx.take_along_axis(qa, tie, axis=1)
    # A random fact; the other field of a hard one is shifted off the query's value so it is never equal.
    e = _ri(ks[5], (b, c), N_ENT)
    a = _ri(ks[6], (b, c), N_ATT)
    other_e = (te + 1 + _ri(ks[7], (b, c), N_ENT - 1)) % N_ENT
    other_a = (ta + 1 + _ri(ks[8], (b, c), N_ATT - 1)) % N_ATT
    share_e = hard & (which == 0)
    share_a = hard & (which == 1)
    e = mx.where(share_e, te, mx.where(share_a, other_e, e))
    a = mx.where(share_a, ta, mx.where(share_e, other_a, a))
    # An easy fact may still land on a query pair by chance: move its attribute off that pair's attribute.
    match = mx.zeros((b, c), dtype=mx.bool_)
    for i in range(k):
        match = match | ((e == qe[:, i:i + 1]) & (a == qa[:, i:i + 1]))
    a = mx.where(match, (a + 1 + _ri(ks[9], (b, c), N_ATT - 1)) % N_ATT, a)
    v = _ri(ks[10], (b, c), N_VAL)
    # The needles: k distinct context positions.
    pos = mx.argsort(mx.random.uniform(shape=(b, c), key=ks[11]), axis=1)[:, :k].astype(mx.int32)
    ar = mx.arange(c, dtype=mx.int32)[None, :]
    nv = _ri(ks[12], (b, k), N_VAL)
    targets = nv
    needle = mx.zeros((b, c), dtype=mx.bool_)
    for i in range(k):
        m = ar == pos[:, i:i + 1]
        needle = needle | m
        e = mx.where(m, qe[:, i:i + 1], e)
        a = mx.where(m, qa[:, i:i + 1], a)
        v = mx.where(m, nv[:, i:i + 1], v)
    is_hard = hard & ~needle
    ent = mx.concatenate([e, qe], axis=1).astype(mx.int32)
    att = mx.concatenate([a, qa], axis=1).astype(mx.int32)
    val = mx.concatenate([v, mx.full((b, k), QUERY, dtype=mx.int32)], axis=1).astype(mx.int32)
    return ent, att, val, targets.astype(mx.int32), is_hard


def heldout(seed, n, k, size):
    """The fixed held-out set: (ent, att, val, targets) of `size` sequences, determined by (seed, n, k, size)."""
    key = mx.random.key(1_000_003 * seed + 1000 * n + k)
    return make_batch(key, size, n, k)[:4]


# ---------------------------------------------------------------- model

class Layer(nn.Module):
    """Pre-LayerNorm attention + ReLU MLP of 2d (the army net's layer shape)."""

    def __init__(self, d, heads):
        super().__init__()
        self.h = heads
        self.ln1 = nn.LayerNorm(d)
        self.qkv = nn.Linear(d, 3 * d)
        self.o = nn.Linear(d, d)
        self.ln2 = nn.LayerNorm(d)
        self.f1 = nn.Linear(d, 2 * d)
        self.f2 = nn.Linear(2 * d, d)

    def __call__(self, x):
        b, t, d = x.shape
        q, k, v = mx.split(self.qkv(self.ln1(x)), 3, axis=-1)
        sp = lambda z: z.reshape(b, t, self.h, d // self.h).transpose(0, 2, 1, 3)
        att = mx.fast.scaled_dot_product_attention(sp(q), sp(k), sp(v), scale=(d // self.h) ** -0.5)
        x = x + self.o(att.transpose(0, 2, 1, 3).reshape(b, t, d))
        return x + self.f2(nn.relu(self.f1(self.ln2(x))))


class Needle(nn.Module):
    def __init__(self, d, layers, n, k, heads=HEADS):
        super().__init__()
        self.k = k
        self.ee = nn.Embedding(N_ENT, d)
        self.ae = nn.Embedding(N_ATT, d)
        self.ve = nn.Embedding(N_VAL + 1, d)
        self.pe = nn.Embedding(n, d)
        self.layers = [Layer(d, heads) for _ in range(layers)]
        self.ln = nn.LayerNorm(d)
        self.head = nn.Linear(d, N_VAL)

    def __call__(self, ent, att, val):
        x = self.ee(ent) + self.ae(att) + self.ve(val) + self.pe(mx.arange(ent.shape[1]))[None]
        for layer in self.layers:
            x = layer(x)
        return self.head(self.ln(x[:, -self.k:]))  # [B, k, 32]


def n_params(model):
    return sum(v.size for _, v in tree_flatten(model.parameters()))


def loss_fn(model, ent, att, val, tgt):
    return nn.losses.cross_entropy(model(ent, att, val), tgt).mean()


def accuracy(model, data, chunk=256):
    ent, att, val, tgt = data
    ok = 0
    for i in range(0, ent.shape[0], chunk):
        pred = mx.argmax(model(ent[i:i + chunk], att[i:i + chunk], val[i:i + chunk]), axis=-1)
        ok += (pred == tgt[i:i + chunk]).sum().item()
    return ok / tgt.size


# ---------------------------------------------------------------- training

def train_net(n, k, d, layers, steps, eval_every, seed, held, deadline=None, batch=64, log=print):
    """Train one net; returns the result dict, or None when the deadline stopped it inside the net."""
    mx.random.seed(seed)
    model = Needle(d, layers, n, k)
    mx.eval(model.parameters())
    params = n_params(model)
    warm = min(100, max(1, steps // 10))
    sched = optim.join_schedules(
        [optim.linear_schedule(0.0, 1e-3, warm), optim.cosine_decay(1e-3, max(1, steps - warm), 5e-5)], [warm])
    opt = optim.AdamW(learning_rate=sched)
    state = [model.state, opt.state]
    vg = nn.value_and_grad(model, loss_fn)

    def step(ent, att, val, tgt):
        loss, grads = vg(model, ent, att, val, tgt)
        opt.update(model, grads)
        return loss

    try:
        step = mx.compile(step, inputs=state, outputs=state)
    except Exception:  # fall back to the eager step
        pass
    t0 = time.time()
    best = final = 0.0
    done = 0
    for s in range(1, steps + 1):
        ent, att, val, tgt, _ = make_batch(_step_key(seed, s), batch, n, k)
        loss = step(ent, att, val, tgt)
        mx.eval(loss, state)
        done = s
        if s % eval_every == 0 or s == steps:
            final = accuracy(model, held)
            best = max(best, final)
            log(f'    n={n} k={k} d={d} L={layers} step {s} loss {loss.item():.3f} acc {final:.3f}')
            if final >= EARLY:
                break
            if deadline is not None and time.time() > deadline and s < steps:
                return None
    secs = time.time() - t0
    return dict(length=n, needles=k, d=d, layers=layers, params=params, steps_run=done, best_acc=round(best, 4),
                final_acc=round(final, 4), seconds=round(secs, 1), steps_per_s=round(done / max(secs, 1e-9), 2))


def _step_key(seed, s):
    return mx.random.key((1 << 40) + seed * 100_000_007 + s)


def read_results(out):
    path = os.path.join(out, 'results.jsonl')
    if not os.path.exists(path):
        return []
    with open(path) as f:
        return [json.loads(line) for line in f if line.strip()]


def run_grid(out, lengths, widths, layers, needles, steps, eval_every, seed, max_minutes=None, held_size=2048,
             log=print):
    os.makedirs(out, exist_ok=True)
    path = os.path.join(out, 'results.jsonl')
    deadline = time.time() + max_minutes * 60 if max_minutes else None
    rows = read_results(out)
    seen = {(r['length'], r['needles'], r['d'], r['layers']) for r in rows}
    for n in lengths:
        for k in needles:
            nets = sorted(((d, l, n_params(Needle(d, l, n, k))) for d in widths for l in layers), key=lambda t: t[2])
            held = None
            passed = any(r['length'] == n and r['needles'] == k and not r.get('skipped') and r['best_acc'] >= PASS
                         for r in rows)
            for d, l, params in nets:
                if (n, k, d, l) in seen:
                    continue
                if deadline is not None and time.time() > deadline:
                    log('time limit reached: stopping between nets')
                    return
                if passed:
                    r = dict(length=n, needles=k, d=d, layers=l, params=params, skipped='skipped: smaller net passed')
                else:
                    if held is None:
                        held = heldout(seed, n, k, held_size)
                        mx.eval(held)
                    log(f'  net length={n} needles={k} d={d} L={l} params={params}')
                    r = train_net(n, k, d, l, steps, eval_every, seed, held, deadline, log=log)
                    if r is None:
                        log('time limit reached inside a net: not recorded')
                        return
                    log(f'  -> best {r["best_acc"]} final {r["final_acc"]} {r["steps_per_s"]} steps/s')
                    passed = r['best_acc'] >= PASS
                with open(path, 'a') as f:
                    f.write(json.dumps(r) + '\n')
                rows.append(r)
                seen.add((n, k, d, l))


# ---------------------------------------------------------------- summary

def summary(out):
    rows = read_results(out)
    lines = ['# Needle test', '', f'Smallest pre-LN transformer (heads {HEADS}, MLP 2d) with best held-out accuracy >= {PASS:.0%}.', '']
    lengths = sorted({r['length'] for r in rows})
    for n in lengths:
        lines += [f'## Length {n}', '', '| needles | smallest net >= 95 % | params | best acc |', '|---|---|---|---|']
        for k in sorted({r['needles'] for r in rows if r['length'] == n}):
            sel = [r for r in rows if r['length'] == n and r['needles'] == k and not r.get('skipped')]
            ok = sorted((r for r in sel if r['best_acc'] >= PASS), key=lambda r: r['params'])
            if ok:
                r = ok[0]
                lines.append(f'| {k} | d{r["d"]} x L{r["layers"]} | {r["params"]} | {r["best_acc"]:.3f} |')
            else:
                best = max((r['best_acc'] for r in sel), default=0.0)
                dm = max((r['d'] for r in sel), default=0)
                lm = max((r['layers'] for r in sel), default=0)
                lines.append(f'| {k} | none up to d{dm} L{lm} (best acc {best:.3f}) | - | {best:.3f} |')
        lines.append('')
    lines += ['## All runs', '', '| length | needles | d | L | params | steps | best | final | seconds | steps/s |',
              '|---|---|---|---|---|---|---|---|---|---|']
    for r in sorted(rows, key=lambda r: (r['length'], r['needles'], r['params'])):
        if r.get('skipped'):
            lines.append(f'| {r["length"]} | {r["needles"]} | {r["d"]} | {r["layers"]} | {r["params"]} | {r["skipped"]} | | | | |')
        else:
            lines.append(f'| {r["length"]} | {r["needles"]} | {r["d"]} | {r["layers"]} | {r["params"]} | {r["steps_run"]} | '
                         f'{r["best_acc"]:.3f} | {r["final_acc"]:.3f} | {r["seconds"]} | {r["steps_per_s"]} |')
    os.makedirs(out, exist_ok=True)
    with open(os.path.join(out, 'summary.md'), 'w') as f:
        f.write('\n'.join(lines) + '\n')


# ---------------------------------------------------------------- CLI

def ints(s):
    return [int(x) for x in s.split(',')]


def main():
    mx.set_memory_limit(8 * 1024 ** 3)
    mx.set_cache_limit(1024 ** 3)
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest='cmd', required=True)
    r = sub.add_parser('run')
    r.add_argument('--out', required=True)
    r.add_argument('--lengths', default='64,128,256')
    r.add_argument('--widths', default='64,96,128')
    r.add_argument('--layers', default='3,4,6')
    r.add_argument('--needles', default='1,3,5')
    r.add_argument('--steps', type=int, default=3000)
    r.add_argument('--eval-every', type=int, default=250)
    r.add_argument('--seed', type=int, default=1)
    r.add_argument('--max-minutes', type=float, default=None)
    s = sub.add_parser('summary')
    s.add_argument('--out', required=True)
    m = sub.add_parser('smoke')
    m.add_argument('--out', required=True)
    a = p.parse_args()
    if a.cmd == 'run':
        run_grid(a.out, ints(a.lengths), ints(a.widths), ints(a.layers), ints(a.needles), a.steps, a.eval_every,
                 a.seed, a.max_minutes)
        summary(a.out)
    elif a.cmd == 'summary':
        summary(a.out)
    else:
        run_grid(a.out, [64], [64], [3, 4], [1], 30, 10, 1, held_size=256)
        summary(a.out)


if __name__ == '__main__':
    main()
