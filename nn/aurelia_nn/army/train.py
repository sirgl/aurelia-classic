"""Imitation of the teachers by the army net in MLX (DESIGN.md, «Training»).

python -m aurelia_nn.army.train --train runs/teach-train.bin --hold runs/teach-hold.bin --out runs/net1 [--epochs 8]

Writes OUT/curves.jsonl (one line per evaluation: step, epoch, losses, held-out accuracies, decisions/s),
OUT/weights.safetensors (MLX), OUT/army-net.bin (the Rust format, `export.py`), OUT/teachers.json (the best
teacher per family, the weights' rule).
"""
import argparse
import collections
import json
import os
import time

import mlx.core as mx
import mlx.nn as nn
import mlx.optimizers as optim

from .data import F_ACT, F_TOK, S_CH, W, Flat, read_into
from .export import export
from .model import ArmyNet, map_planes

C_CAP = 128


class Device:
    """A Flat on the GPU, and the batches gathered from it."""

    def __init__(self, flat, weights, tm, cm):
        self.n = len(flat)
        self.tm, self.cm = tm, cm
        u16 = lambda b: mx.array(memoryview(b).cast('H'))
        self.tok = u16(flat.tok).view(mx.float16).reshape(-1, F_TOK)
        self.dist = mx.array(memoryview(flat.dist).cast('B'))
        self.sel = mx.array(memoryview(flat.sel).cast('B'))
        self.danger = u16(flat.danger).view(mx.float16)
        self.cand = u16(flat.cand).view(mx.float16).reshape(-1, F_ACT) if len(flat.cand) else mx.zeros((1, F_ACT), mx.float16)
        self.promo = u16(flat.promo) if len(flat.promo) else mx.zeros((1,), mx.uint16)
        self.pos = u16(flat.pos).view(mx.int16)
        self.cpos = u16(flat.cpos).view(mx.int16) if len(flat.cpos) else mx.zeros((1,), mx.int16)
        self.statics = u16(flat.statics).view(mx.float16).reshape(-1, W * W, S_CH)
        self.play = mx.array(list(flat.play), mx.int32)
        i32 = lambda a: mx.array(list(a), mx.int32)
        self.t = i32(flat.t)
        self.c = i32(flat.c)
        self.tok_off = i32(flat.tok_off)
        self.dist_off = i32(flat.dist_off)
        self.cand_off = i32(flat.cand_off)
        self.ptr = i32(flat.ptr)
        self.act = i32(flat.act)
        self.w = mx.array(weights, mx.float32)
        plays = flat.plays
        self.win = mx.array([float(plays[p]['win']) for p in flat.play], mx.float32)
        lost = []
        for p in flat.play:
            m = plays[p].get('measures', {})
            units = m.get('units', 0) or 1
            lost.append(min(1.0, m.get('lost', 0) / units))
        self.lost = mx.array(lost, mx.float32)
        self.families = sorted({m['family'] for m in plays})
        fam_ix = {f: i for i, f in enumerate(self.families)}
        self.fam = [fam_ix[plays[p]['family']] for p in flat.play]
        self.n_cand = self.cand.shape[0]
        self.n_tok = self.tok.shape[0]
        self.n_dist = self.dist.shape[0]

    def batch(self, idx):
        tm, cm = self.tm, self.cm
        t = self.t[idx]
        c = self.c[idx]
        ar = mx.arange(tm)
        tmask = ar[None, :] < t[:, None]
        ti = mx.minimum(self.tok_off[idx][:, None] + mx.minimum(ar[None, :], t[:, None] - 1), self.n_tok - 1)
        tok = mx.where(tmask[..., None], self.tok[ti].astype(mx.float32), 0.0)
        sel = (self.sel[ti] > 0) & tmask
        danger = mx.where(tmask, self.danger[ti].astype(mx.float32), -1.0)
        ii = mx.minimum(ar[None, :, None], t[:, None, None] - 1)
        jj = mx.minimum(ar[None, None, :], t[:, None, None] - 1)
        di = mx.minimum(self.dist_off[idx][:, None, None] + ii * t[:, None, None] + jj, self.n_dist - 1)
        dist = self.dist[di].astype(mx.int32)
        arc = mx.arange(cm)
        cmask = arc[None, :] < c[:, None]
        ci = mx.minimum(self.cand_off[idx][:, None] + mx.minimum(arc[None, :], mx.maximum(c[:, None] - 1, 0)), self.n_cand - 1)
        cand = mx.where(cmask[..., None], self.cand[ci].astype(mx.float32), 0.0)
        promo = mx.where(cmask, self.promo[ci].astype(mx.int32), 0)
        pos = mx.where(tmask, self.pos[ti].astype(mx.int32), -1)
        cpos = mx.where(cmask, self.cpos[ci].astype(mx.int32), -1)
        ptr = self.ptr[idx]
        ptr_label = mx.where(ptr >= 0, ptr, tm)
        unit = mx.maximum(ptr, 0)
        act = self.act[idx]
        return {'pos': pos, 'cpos': cpos, 'statics': self.statics[self.play[idx]].astype(mx.float32), 'tok': tok, 'dist': dist, 'tmask': tmask, 'sel': sel, 'danger': danger, 'cand': cand, 'promo': promo, 'cmask': cmask,
                'ptr': ptr_label, 'unit': unit, 'act': mx.maximum(act, 0), 'has_act': (act >= 0).astype(mx.float32),
                'w': self.w[idx], 'win': self.win[idx], 'lost': self.lost[idx]}


def forward(model, b):
    m = model.encode(map_planes(b['statics'], b['tok'], b['pos'])) if model.has_map else None
    h = model.body(b['tok'], b['dist'], b['tmask'], m, b['pos'])
    pl = model.pointer_logits(h, b['sel'])
    al = model.action_logits(h, b['unit'], b['cand'], b['promo'], b['cmask'], m, b['cpos'])
    return h, pl, al


def losses(model, b):
    h, pl, al = forward(model, b)
    ce_ptr = mx.logsumexp(pl, axis=1) - mx.take_along_axis(pl, b['ptr'][:, None], axis=1)[:, 0]
    ce_act = (mx.logsumexp(al, axis=1) - mx.take_along_axis(al, b['act'][:, None], axis=1)[:, 0]) * b['has_act']
    v = model.value(h)
    bce = nn.losses.binary_cross_entropy(v[:, 0], b['win'], with_logits=True, reduction='none')
    lost = (mx.sigmoid(v[:, 1]) - b['lost']) ** 2
    dg = model.danger(h)
    dmask = (b['danger'] >= 0) & b['sel']
    dmse = mx.sum(mx.where(dmask, (dg - b['danger']) ** 2, 0.0)) / mx.maximum(mx.sum(dmask), 1)
    w = b['w']
    sw = mx.maximum(mx.sum(w), 1e-6)
    main = mx.sum(w * (ce_ptr + ce_act)) / sw
    val = mx.sum(w * (bce + lost)) / sw
    acc_ptr = (mx.argmax(pl, axis=1) == b['ptr']).astype(mx.float32)
    acc_act = (mx.argmax(al, axis=1) == b['act']).astype(mx.float32)
    return main + 0.5 * val + 0.5 * dmse, (main, val, dmse, acc_ptr, acc_act)


def best_teachers(plays):
    """family -> the player with the highest win rate on these plays (ties: the reference first)."""
    wins = collections.defaultdict(lambda: [0, 0])
    for m in plays:
        k = (m['family'], m['player'])
        wins[k][0] += m['win']
        wins[k][1] += 1
    best = {}
    for (fam, player), (w, n) in sorted(wins.items(), key=lambda x: (x[0][0], x[0][1] == 'unciv', x[0][1])):
        rate = w / max(n, 1)
        if fam not in best or rate > best[fam][1]:
            best[fam] = (player, rate)
    return best, {f'{f}/{p}': [w, n] for (f, p), (w, n) in wins.items()}


def decision_weights(flat, best, lost_weight, best_factor):
    out = []
    for p in flat.play:
        m = flat.plays[p]
        w = 1.0 if m['win'] else lost_weight
        if best.get(m['family'], (None,))[0] == m['player']:
            w *= best_factor
        out.append(w)
    return out


def evaluate(model, dev, bs):
    tot = collections.defaultdict(float)
    n = 0
    n_act = 0.0
    fam_hit = collections.defaultdict(float)
    fam_n = collections.defaultdict(int)
    for s in range(0, dev.n, bs):
        idx = mx.arange(s, min(s + bs, dev.n))
        b = dev.batch(idx)
        loss, (main, val, dmse, acc_ptr, acc_act) = losses(model, b)
        k = idx.shape[0]
        has = b['has_act']
        both = acc_ptr * mx.where(has > 0, acc_act, 1.0)
        tot['loss'] += loss.item() * k
        tot['main'] += main.item() * k
        tot['acc_ptr'] += mx.sum(acc_ptr).item()
        tot['acc_act'] += mx.sum(acc_act * has).item()
        tot['acc_both'] += mx.sum(both).item()
        n_act += mx.sum(has).item()
        for j, hit in enumerate(both.tolist()):
            f = dev.families[dev.fam[s + j]]
            fam_hit[f] += hit
            fam_n[f] += 1
        n += k
    ev = {'loss': tot['loss'] / n, 'main': tot['main'] / n, 'acc_ptr': tot['acc_ptr'] / n, 'acc_act': tot['acc_act'] / max(n_act, 1), 'acc_both': tot['acc_both'] / n}
    return ev, {f: fam_hit[f] / fam_n[f] for f in sorted(fam_n)}


def load(paths, nth=1):
    flat = Flat()
    seen = [0]

    def keep(meta):
        seen[0] += 1
        return seen[0] % nth == 0

    for p in paths:
        read_into(flat, p, keep if nth > 1 else None)
    return flat


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--train', nargs='+', required=True)
    ap.add_argument('--hold', nargs='+', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--epochs', type=float, default=8)
    ap.add_argument('--bs', type=int, default=512)
    ap.add_argument('--lr', type=float, default=1e-3)
    ap.add_argument('--d', type=int, default=64)
    ap.add_argument('--layers', type=int, default=3)
    ap.add_argument('--heads', type=int, default=4)
    ap.add_argument('--ff', type=int, default=128)
    ap.add_argument('--map', action='store_true', help='variant B: the hex-conv map encoder')
    ap.add_argument('--lost-weight', type=float, default=0.1)
    ap.add_argument('--best-factor', type=float, default=2.0)
    ap.add_argument('--eval-every', type=int, default=1000)
    ap.add_argument('--log-every', type=int, default=200, help='a train-loss point of the curve every N steps')
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--init', default=None, help='start from these MLX weights')
    ap.add_argument('--cpu', action='store_true', help='a smoke test on the CPU (no GPU lock needed)')
    ap.add_argument('--every-nth-play', type=int, default=1, help='keep one play in N (smoke tests)')
    a = ap.parse_args()
    if a.cpu:
        mx.set_default_device(mx.cpu)
    os.makedirs(a.out, exist_ok=True)
    mx.random.seed(a.seed)
    t0 = time.time()
    train = load(a.train, a.every_nth_play)
    hold = load(a.hold, a.every_nth_play)
    best, table = best_teachers(train.plays)
    json.dump({'best': best, 'wins': table, 'lost_weight': a.lost_weight, 'best_factor': a.best_factor}, open(os.path.join(a.out, 'teachers.json'), 'w'), indent=1)
    tm = max(max(train.t), max(hold.t))
    cm = min(C_CAP, max(max(train.c), max(hold.c)))
    print(f'loaded {len(train)} train / {len(hold)} held-out decisions from {len(train.plays)} / {len(hold.plays)} plays in {time.time() - t0:.0f} s; T {tm} C {cm}', flush=True)
    dev = Device(train, decision_weights(train, best, a.lost_weight, a.best_factor), tm, cm)
    hdev = Device(hold, decision_weights(hold, best, a.lost_weight, a.best_factor), tm, cm)
    promos = max(400, int(mx.max(dev.promo).item()) + 1)
    model = ArmyNet(d=a.d, heads=a.heads, layers=a.layers, ff=a.ff, promos=promos, map_ch=a.d if a.map else 0)
    if a.init:
        model.load_weights(a.init)
    mx.eval(model.parameters())
    n_params = sum(v.size for _, v in nn.utils.tree_flatten(model.parameters()))
    steps = int(a.epochs * dev.n / a.bs)
    sched = optim.cosine_decay(a.lr, steps, a.lr * 0.05)
    opt = optim.AdamW(learning_rate=sched, weight_decay=1e-4)
    state = [model.state, opt.state]

    def step_fn(b):
        (loss, aux), grads = nn.value_and_grad(model, losses)(model, b)
        grads, _ = optim.clip_grad_norm(grads, 1.0)
        opt.update(model, grads)
        return loss, aux[0]

    step = mx.compile(step_fn, inputs=state, outputs=state)
    def data_facts(flat, files):
        fam = collections.Counter()
        scenes = set()
        for p in flat.play:
            fam[flat.plays[p]['family']] += 1
        for m in flat.plays:
            scenes.add((m['family'], m['era'], m['instance']))
        players = collections.Counter()
        for m in flat.plays:
            players[m['player']] += m['count']
        return {'files': files, 'decisions': len(flat), 'plays': len(flat.plays), 'scenes': len(scenes), 'decisions_by_family': dict(sorted(fam.items())), 'decisions_by_teacher': dict(players.most_common())}

    config = {'run': os.path.basename(os.path.normpath(a.out)), 'variant': 'B (hex-conv map encoder)' if a.map else 'A (no map)', 'map': a.map,
              'bs': a.bs, 'lr': a.lr, 'schedule': f'AdamW (weight decay 1e-4), cosine decay from lr to 0.05 lr over {steps} steps, grad clip 1.0',
              'epochs': a.epochs, 'steps': steps, 'd': a.d, 'layers': a.layers, 'heads': a.heads, 'ff': a.ff, 'map_ch': a.d if a.map else 0,
              'promos': promos, 'T': tm, 'C': cm, 'parameters': n_params,
              'losses': {'pointer_ce': 1.0, 'action_ce': 1.0, 'value_bce_win': 0.5, 'value_mse_lost_share': 0.5, 'danger_mse': 0.5},
              'decision_weight': {'won_play': 1.0, 'lost_play': a.lost_weight, 'best_teacher_factor': a.best_factor}, 'best_teachers': best,
              'seed': a.seed, 'init': a.init, 'device': str(mx.default_device()), 'eval_every_steps': a.eval_every, 'log_every_steps': a.log_every,
              'train': data_facts(train, a.train), 'hold': data_facts(hold, a.hold)}
    json.dump(config, open(os.path.join(a.out, 'config.json'), 'w'), indent=1)
    curves = open(os.path.join(a.out, 'curves.jsonl'), 'w')
    print(f'{n_params} parameters, {steps} steps of {a.bs}', flush=True)
    seen = 0
    t_train = 0.0
    run_loss = 0.0
    run_n = 0
    log_loss = 0.0
    log_n = 0
    perm = mx.random.permutation(dev.n)
    pos = 0
    best_both = -1.0
    for s in range(1, steps + 1):
        if pos + a.bs > dev.n:
            perm = mx.random.permutation(dev.n)
            pos = 0
        idx = perm[pos:pos + a.bs]
        pos += a.bs
        ts = time.time()
        b = dev.batch(idx)
        loss, main = step(b)
        mx.eval(loss, main, state)
        t_train += time.time() - ts
        seen += a.bs
        run_loss += loss.item()
        run_n += 1
        log_loss += loss.item()
        log_n += 1
        if s % a.log_every == 0 and s % a.eval_every != 0 and s != steps:
            rec = {'step': s, 'epoch': seen / dev.n, 'train_loss': log_loss / log_n, 'dps': seen / t_train, 'wall_s': time.time() - t0}
            log_loss, log_n = 0.0, 0
            curves.write(json.dumps(rec) + '\n')
            curves.flush()
        if s % a.eval_every == 0 or s == steps:
            ev, by_family = evaluate(model, hdev, 2048)
            rec = {'step': s, 'epoch': seen / dev.n, 'train_loss': log_loss / max(log_n, 1), 'hold': ev, 'hold_by_family': by_family, 'dps': seen / t_train, 'wall_s': time.time() - t0}
            run_loss, run_n = 0.0, 0
            log_loss, log_n = 0.0, 0
            curves.write(json.dumps(rec) + '\n')
            curves.flush()
            print(json.dumps({k: v for k, v in rec.items() if k != 'hold_by_family'}), flush=True)
            model.save_weights(os.path.join(a.out, 'last.safetensors'))
            if ev['acc_both'] > best_both:
                best_both = ev['acc_both']
                model.save_weights(os.path.join(a.out, 'weights.safetensors'))
                export(model, os.path.join(a.out, 'army-net.bin'))
    print(f'done in {time.time() - t0:.0f} s; best held-out joint accuracy {best_both:.4f}', flush=True)


if __name__ == '__main__':
    main()
