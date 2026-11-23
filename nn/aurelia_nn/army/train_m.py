"""Variant M (learned memory) of the army net: imitation with the memory unrolled over whole plays
(work/nets/army-net/M-HANDOFF.md, «Training»).

python -m aurelia_nn.army.train_m --train runs/teach-train-m.bin --hold runs/teach-hold-m.bin --out runs/netM0

A step takes B plays. The memory is unrolled over their turns: the memory-state decisions of turn r (one per play
and turn: our turn's end; masked where a play has ended) run the body with M_r and update it to M_{r+1}. Then every
labelled decision of those plays is scored with its turn's M_r (the losses of `train.py`), plus the auxiliary head
on the teacher's own state at the memory states (privileged, training only; weight --aux). Truncated BPTT over whole
plays (<= 31 turns) with a length curriculum (4 -> 8 -> full turns), memory dropout (in --mem-drop of the plays the
memory is reset at a random turn), grad clip 1.0.
"""
import argparse
import collections
import json
import os
import random
import time

import mlx.core as mx
import mlx.nn as nn
import mlx.optimizers as optim

from .data import MEMORY_STATE, TRACK
from .export import export
from .model_m import AIMS, P_ENGAGED, P_ETA, P_STAGE, P_TURNS, P_WAITED, PLANS, SLOTS, STAGES, U_ENGAGED, U_RESTING, U_WAITED, MemNet
from .train import Device, best_teachers, decision_weights, load

ASSAULT = {'assault-heal', 'promotion-choice', 'hill-archers', 'city-fire', 'planned-assault'}
C_GROUPS = (16, 48, 128)
T_GROUPS = (12, 24, 40)


class Plays:
    """The per-play structure of a Flat: its decisions, its memory states, each decision's turn index."""

    def __init__(self, flat):
        self.first = [m['first'] for m in flat.plays]
        self.count = [m['count'] for m in flat.plays]
        self.ms = []   # per play: the decision indices of its memory states
        self.turn = [0] * len(flat)  # per decision: memory states before it in its play
        for p, (f, c) in enumerate(zip(self.first, self.count)):
            seen = []
            for i in range(f, f + c):
                self.turn[i] = len(seen)
                if flat.flags[i] & MEMORY_STATE:
                    seen.append(i)
            self.ms.append(seen)
        # Auxiliary targets of the memory states (host side): turns in the stage, expected turns to the capture.
        self.turns_in_stage = {}
        self.eta = {}
        for p, seen in enumerate(self.ms):
            meta = flat.plays[p]
            prev, run = None, 0
            for i in seen:
                st = flat.track[i * TRACK + 2]
                run = run + 1 if st == prev and st >= 0 else (1 if st >= 0 else 0)
                prev = st
                if st >= 0:
                    self.turns_in_stage[i] = run
                if meta['family'] in ASSAULT and meta.get('win'):
                    self.eta[i] = max(0, meta.get('rounds', 0) - flat.round[i])


class DeviceM(Device):
    def __init__(self, flat, weights, tm, cm):
        super().__init__(flat, weights, tm, cm)
        self.slot = mx.array(memoryview(flat.slot).cast('h'))
        self.flags = mx.array(list(flat.flags), mx.int32)
        self.track = mx.array(list(flat.track), mx.int32).reshape(-1, TRACK)
        self.plays = Plays(flat)
        n = len(flat)
        tis = [-1.0] * n
        eta = [-1.0] * n
        for i, v in self.plays.turns_in_stage.items():
            tis[i] = float(v)
        for i, v in self.plays.eta.items():
            eta[i] = float(v)
        self.tis = mx.array(tis, mx.float32)
        self.eta = mx.array(eta, mx.float32)
        self.ptr_list = list(flat.ptr)
        self.flags_list = list(flat.flags)
        self.round_list = list(flat.round)

    def batch_m(self, idx, tm, cm):
        b = self.batch(idx, tm, cm)
        t = self.t[idx]
        ar = mx.arange(tm)
        tmask = ar[None, :] < t[:, None]
        ti = mx.minimum(self.tok_off[idx][:, None] + mx.minimum(ar[None, :], t[:, None] - 1), self.n_tok - 1)
        b['slot'] = mx.where(tmask, self.slot[ti].astype(mx.int32), -1)
        b['track'] = self.track[idx]
        b['tis'] = self.tis[idx]
        b['eta'] = self.eta[idx]
        return b


def decision_losses(model, b, um, pm):
    """The losses of `train.py` on decisions read with their memory (um [N, S, U], pm [N, P, d])."""
    hfull = model.body_m(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm)
    t = b['tok'].shape[1]
    h = hfull[:, :t]
    pl = model.pointer_logits(h, b['sel'])
    al = model.action_logits(h, b['unit'], b['cand'], b['promo'], b['cmask'])
    ce_ptr = mx.logsumexp(pl, axis=1) - mx.take_along_axis(pl, b['ptr'][:, None], axis=1)[:, 0]
    ce_act = (mx.logsumexp(al, axis=1) - mx.take_along_axis(al, b['act'][:, None], axis=1)[:, 0]) * b['has_act']
    v = model.value(h)
    bce = nn.losses.binary_cross_entropy(v[:, 0], b['win'], with_logits=True, reduction='none')
    lost = (mx.sigmoid(v[:, 1]) - b['lost']) ** 2
    dg = model.danger(h)
    dmask = (b['danger'] >= 0) & b['sel']
    dsum = mx.sum(mx.where(dmask, (dg - b['danger']) ** 2, 0.0))
    w = b['w']
    acc_ptr = (mx.argmax(pl, axis=1) == b['ptr']).astype(mx.float32)
    acc_act = (mx.argmax(al, axis=1) == b['act']).astype(mx.float32)
    return {'main': mx.sum(w * (ce_ptr + ce_act)), 'val': mx.sum(w * (bce + lost)), 'w': mx.sum(w), 'dsum': dsum, 'dn': mx.sum(dmask),
            'acc_ptr': acc_ptr, 'acc_act': acc_act, 'has_act': b['has_act']}


def aux_losses(model, b, um1, pm1, logits):
    """The auxiliary head on the teacher's own state at the memory states: um1 / pm1 = the memory after the update,
    logits = the plan's attention over the tokens at the update (its target pointer)."""
    tr = b['track']
    known = (tr[:, 0] >= 0).astype(mx.float32)  # [N]
    engaged = (tr[:, 0] > 0).astype(mx.float32)
    waited = mx.minimum(tr[:, 1], 10).astype(mx.float32) * 0.1
    stage = mx.maximum(tr[:, 2], 0)
    mask = tr[:, 3].astype(mx.int32) & 0xFFFF
    slot = b['slot']
    has = (slot >= 0) & b['tmask']
    sl = mx.maximum(slot, 0)
    n = slot.shape[0]
    u = um1[mx.arange(n)[:, None], sl]  # [N, T, U]
    au = model.aux_u
    rest_t = ((mask[:, None] >> sl) & 1).astype(mx.float32)
    um_known = has.astype(mx.float32) * known[:, None]
    rest_l = nn.losses.binary_cross_entropy(au[1, 0] * u[..., U_RESTING] + au[1, 1], rest_t, with_logits=True, reduction='none')
    eng_l = nn.losses.binary_cross_entropy(au[2, 0] * u[..., U_ENGAGED] + au[2, 1], mx.broadcast_to(engaged[:, None], rest_t.shape), with_logits=True, reduction='none')
    wait_l = (au[0, 0] * u[..., U_WAITED] + au[0, 1] - waited[:, None]) ** 2
    nu = mx.maximum(mx.sum(um_known), 1.0)
    unit = mx.sum(um_known * (rest_l + eng_l + wait_l)) / nu
    p0 = pm1[:, 0]
    ap = model.aux_p
    st_logits = ap[0, 0] * p0[:, P_STAGE:P_STAGE + STAGES] + ap[0, 1]
    st_l = mx.logsumexp(st_logits, axis=1) - mx.take_along_axis(st_logits, stage[:, None], axis=1)[:, 0]
    peng_l = nn.losses.binary_cross_entropy(ap[1, 0] * p0[:, P_ENGAGED] + ap[1, 1], engaged, with_logits=True, reduction='none')
    pwait_l = (p0[:, P_WAITED] - waited) ** 2
    tis = b['tis']
    tis_m = (tis >= 0).astype(mx.float32)
    tis_l = (p0[:, P_TURNS] - mx.minimum(tis, 10.0) * 0.1) ** 2
    eta = b['eta']
    eta_m = (eta >= 0).astype(mx.float32)
    eta_l = (p0[:, P_ETA] - eta / 30.0) ** 2
    nk = mx.maximum(mx.sum(known), 1.0)
    plan = mx.sum(known * (st_l + peng_l + pwait_l)) / nk + mx.sum(tis_m * tis_l) / mx.maximum(mx.sum(tis_m), 1.0) + mx.sum(eta_m * eta_l) / mx.maximum(mx.sum(eta_m), 1.0)
    # The target: the objective city's token (a city token marked "is the objective").
    tok = b['tok']
    is_obj = (tok[..., 43] > 0.5) & (tok[..., 3] > 0.5) & b['tmask']
    has_obj = mx.any(is_obj, axis=1).astype(mx.float32)
    tgt = mx.argmax(is_obj.astype(mx.int32), axis=1)
    lg = logits[:, 0]
    tgt_l = mx.logsumexp(lg, axis=1) - mx.take_along_axis(lg, tgt[:, None], axis=1)[:, 0]
    target = mx.sum(has_obj * tgt_l) / mx.maximum(mx.sum(has_obj), 1.0)
    acc_stage = mx.sum(known * (mx.argmax(st_logits, axis=1) == stage)) / nk
    return unit + plan + target, {'aux_unit': unit, 'aux_plan': plan, 'aux_target': target, 'stage_acc': acc_stage, 'known': mx.sum(known)}


def plan_batch(dev, plays, L, drop, rng, tm, train=True):
    """Index arrays of one step: the memory states [R, B] (-1 pad), the resets, and the labelled decisions by
    candidate group: (C, decision indices, their play column, their turn)."""
    P = dev.plays
    R = max(min(len(P.ms[p]), L) for p in plays) if plays else 0
    ms = [[-1] * len(plays) for _ in range(R)]
    for col, p in enumerate(plays):
        for r, i in enumerate(P.ms[p][:R]):
            ms[r][col] = i
    reset = [-1] * len(plays)
    if train and drop > 0:
        for col, p in enumerate(plays):
            n = min(len(P.ms[p]), L)
            if n >= 2 and rng.random() < drop:
                reset[col] = rng.randrange(1, n)
    groups = collections.defaultdict(lambda: ([], [], []))
    for col, p in enumerate(plays):
        f, c = P.first[p], P.count[p]
        for i in range(f, f + c):
            if P.turn[i] >= L:
                break
            if dev.ptr_list[i] < -1:
                continue
            cg = next(x for x in C_GROUPS if dev.c_list[i] <= x)
            tg = next(x for x in T_GROUPS if dev.t_list[i] <= x)
            g = groups[(min(tg, tm), cg)]
            g[0].append(i)
            g[1].append(col)
            g[2].append(P.turn[i])
    return R, ms, reset, dict(groups)


AUX_KEYS = ('aux_unit', 'aux_plan', 'aux_target', 'stage_acc', 'known')
UNROLL_KEYS = ('tok', 'dist', 'tmask', 'slot', 'track', 'tis', 'eta')
DEC_KEYS = ('tok', 'dist', 'tmask', 'slot', 'sel', 'unit', 'cand', 'promo', 'cmask', 'ptr', 'act', 'has_act', 'win', 'lost', 'danger', 'w')
# Decision groups are padded to a multiple of this (rows of weight 0), so the compiled forward sees few shapes.
PAD = 256


def unroll_one(model, b, valid, rs, um, pm, alive, with_aux):
    """One turn of the memory unroll on the memory states `b` (valid: the play has this turn; rs: reset after it)."""
    hfull = model.body_m(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm)
    um2, pm2, al2, lg = model.step(hfull, b['tok'], b['slot'], b['tmask'], um, pm, alive)
    if with_aux:
        bb = dict(b)
        bb['track'] = mx.where(valid[:, None], b['track'], -1)
        bb['tis'] = mx.where(valid, b['tis'], -1.0)
        bb['eta'] = mx.where(valid, b['eta'], -1.0)
        bb['tmask'] = b['tmask'] & valid[:, None]
        a, parts = aux_losses(model, bb, um2, pm2, lg)
        parts = tuple(parts[k] for k in AUX_KEYS)
    else:
        a, parts = mx.array(0.0), tuple(mx.array(0.0) for _ in AUX_KEYS)
    um = mx.where(valid[:, None, None], um2, um)
    pm = mx.where(valid[:, None, None], pm2, pm)
    alive = mx.where(valid[:, None], al2, alive)
    p0 = mx.broadcast_to(model.plan0[None], pm.shape)
    um = mx.where(rs[:, None, None], 0.0, um)
    pm = mx.where(rs[:, None, None], p0, pm)
    alive = mx.where(rs[:, None], 0.0, alive)
    return um, pm, alive, a, parts


def dec_one(model, b, um, pm):
    out = decision_losses(model, b, um, pm)
    return tuple(out[k] for k in ('main', 'val', 'w', 'dsum', 'dn', 'acc_ptr', 'acc_act', 'has_act'))


COMPILED = {}


def compiled(model, name, with_aux=True):
    """The compiled stage `name` (its parameters read from the model's state at each call)."""
    key = (id(model), name, with_aux)
    if key not in COMPILED:
        if name == 'unroll':
            fn = lambda b, valid, rs, um, pm, alive: unroll_one(model, b, valid, rs, um, pm, alive, with_aux)
        else:
            fn = lambda b, um, pm: dec_one(model, b, um, pm)
        COMPILED[key] = mx.compile(fn, inputs=[model.state])
    return COMPILED[key]


def run_step(model, dev, plan, tm, aux_w, with_aux=True, compile_=True):
    """The loss of one step's plays (and its parts)."""
    R, ms, reset, groups = plan
    B = len(reset)
    um, pm, alive = model.initial(B)
    Us, Ps = [um], [pm]
    aux_tot = mx.array(0.0)
    aux_parts = collections.defaultdict(float)
    reset_a = mx.array(reset, mx.int32)
    unroll = compiled(model, 'unroll', with_aux) if compile_ else (lambda b, v, rs, um, pm, al: unroll_one(model, b, v, rs, um, pm, al, with_aux))
    dec = compiled(model, 'dec') if compile_ else (lambda b, um, pm: dec_one(model, b, um, pm))
    for r in range(R):
        idx = mx.array(ms[r], mx.int32)
        valid = idx >= 0
        full = dev.batch_m(mx.maximum(idx, 0), tm, 1)
        b = {k: full[k] for k in UNROLL_KEYS}
        um, pm, alive, a, parts = unroll(b, valid, reset_a == r + 1, um, pm, alive)
        aux_tot = aux_tot + a / R
        for k, v in zip(AUX_KEYS, parts):
            aux_parts[k] = aux_parts[k] + v / R
        Us.append(um)
        Ps.append(pm)
    Us = mx.stack(Us)  # [R + 1, B, S, U]
    Ps = mx.stack(Ps)
    tot = collections.defaultdict(lambda: mx.array(0.0))
    accs = []
    for (tg, cg), (ids, cols, turns) in sorted(groups.items()):
        n = len(ids)
        pad = (-n) % PAD
        idx = mx.array(ids + [ids[0]] * pad, mx.int32)
        full = dev.batch_m(idx, tg, cg)
        b = {k: full[k] for k in DEC_KEYS}
        live = mx.arange(n + pad) < n
        b['w'] = mx.where(live, b['w'], 0.0)
        b['danger'] = mx.where(live[:, None], b['danger'], -1.0)
        tt = mx.array(turns + [turns[0]] * pad, mx.int32)
        cc = mx.array(cols + [cols[0]] * pad, mx.int32)
        out = dec(b, Us[tt, cc], Ps[tt, cc])
        for k, v in zip(('main', 'val', 'w', 'dsum', 'dn'), out[:5]):
            tot[k] = tot[k] + v
        accs.append((ids, out[5][:n], out[6][:n], out[7][:n]))
    sw = mx.maximum(tot['w'], 1e-6)
    main = tot['main'] / sw
    val = tot['val'] / sw
    dmse = tot['dsum'] / mx.maximum(tot['dn'], 1)
    loss = main + 0.5 * val + 0.5 * dmse + aux_w * aux_tot
    return loss, main, aux_tot, dict(aux_parts), accs


def evaluate(model, dev, plays, tm, bs=256):
    """Held-out accuracy with the memory unrolled over whole plays (no dropout), by family; and the aux numbers."""
    fam_hit = collections.defaultdict(float)
    fam_n = collections.defaultdict(int)
    tot = collections.defaultdict(float)
    n = 0
    n_act = 0.0
    aux = collections.defaultdict(float)
    steps = 0
    for s in range(0, len(plays), bs):
        chunk = plays[s:s + bs]
        plan = plan_batch(dev, chunk, 10 ** 6, 0.0, None, tm, train=False)
        loss, main, aux_tot, parts, accs = run_step(model, dev, plan, tm, 0.0)
        mx.eval(loss, main, aux_tot, parts)
        steps += 1
        for k, v in parts.items():
            aux[k] += float(v.item()) if hasattr(v, 'item') else float(v)
        k_dec = 0
        for ids, ap, aa, has in accs:
            ap, aa, has = ap.tolist(), aa.tolist(), has.tolist()
            for j, i in enumerate(ids):
                both = ap[j] * (aa[j] if has[j] > 0 else 1.0)
                f = dev.families[dev.fam[i]]
                fam_hit[f] += both
                fam_n[f] += 1
                tot['acc_ptr'] += ap[j]
                tot['acc_act'] += aa[j] * has[j]
                tot['acc_both'] += both
                n_act += has[j]
                n += 1
                k_dec += 1
        tot['main'] += main.item() * k_dec
    ev = {'main': tot['main'] / max(n, 1), 'acc_ptr': tot['acc_ptr'] / max(n, 1), 'acc_act': tot['acc_act'] / max(n_act, 1), 'acc_both': tot['acc_both'] / max(n, 1), 'decisions': n}
    ev.update({k: v / max(steps, 1) for k, v in aux.items()})
    return ev, {f: fam_hit[f] / fam_n[f] for f in sorted(fam_n)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--train', nargs='+', required=True)
    ap.add_argument('--hold', nargs='+', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--epochs', type=float, default=6)
    ap.add_argument('--plays', type=int, default=96, help='plays per step')
    ap.add_argument('--lr', type=float, default=1e-3)
    ap.add_argument('--aux', type=float, default=0.3, help='weight of the auxiliary head on the teacher state')
    ap.add_argument('--mem-drop', type=float, default=0.1)
    ap.add_argument('--curriculum', default='4:0.15,8:0.15,999:0.7', help='turns:share of the steps, in order')
    ap.add_argument('--lost-weight', type=float, default=0.1)
    ap.add_argument('--best-factor', type=float, default=2.0)
    ap.add_argument('--up', default='', help='families whose decisions weigh --up-factor more ("siege" = the siege set)')
    ap.add_argument('--up-factor', type=float, default=2.0)
    ap.add_argument('--eval-every', type=int, default=1000)
    ap.add_argument('--eval-plays', type=int, default=4, help='evaluate on one held-out play in N during training (all at the end)')
    ap.add_argument('--log-every', type=int, default=100)
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--init', default=None)
    ap.add_argument('--max-minutes', type=float, default=18.0, help='stop (and evaluate, export) after this long: one GPU job <= 20 min')
    ap.add_argument('--cpu', action='store_true')
    ap.add_argument('--every-nth-play', type=int, default=1)
    a = ap.parse_args()
    from .train import SIEGE
    if a.cpu:
        mx.set_default_device(mx.cpu)
    os.makedirs(a.out, exist_ok=True)
    mx.random.seed(a.seed)
    rng = random.Random(a.seed)
    t0 = time.time()
    train = load(a.train, a.every_nth_play)
    hold = load(a.hold, a.every_nth_play)
    best, table = best_teachers(train.plays)
    json.dump({'best': best, 'wins': table, 'lost_weight': a.lost_weight, 'best_factor': a.best_factor}, open(os.path.join(a.out, 'teachers.json'), 'w'), indent=1)
    tm = max(max(train.t), max(hold.t))
    cm = 128
    print(f'loaded {len(train)} train / {len(hold)} held-out decisions from {len(train.plays)} / {len(hold.plays)} plays in {time.time() - t0:.0f} s; T {tm}', flush=True)
    up = set()
    for f in filter(None, a.up.split(',')):
        up |= SIEGE if f == 'siege' else {f}
    weights = decision_weights(train, best, a.lost_weight, a.best_factor)
    if up:
        weights = [w * (a.up_factor if train.plays[p]['family'] in up else 1.0) for w, p in zip(weights, train.play)]
    weights = [0.0 if pt < -1 else w for w, pt in zip(weights, train.ptr)]
    dev = DeviceM(train, weights, tm, cm)
    hw = [0.0 if pt < -1 else w for w, pt in zip(decision_weights(hold, best, a.lost_weight, a.best_factor), hold.ptr)]
    hdev = DeviceM(hold, hw, tm, cm)
    promos = max(400, int(mx.max(dev.promo).item()) + 1)
    model = MemNet(promos=promos)
    model.hand_init()
    if a.init:
        # A net without memory (A / AH) starts the shared parts; an M net starts everything.
        model.load_weights(a.init, strict=False)
    mx.eval(model.parameters())
    n_params = sum(v.size for _, v in nn.utils.tree_flatten(model.parameters()))
    labelled = sum(1 for x in train.ptr if x >= -1)
    plays_all = list(range(len(train.plays)))
    steps = int(a.epochs * len(plays_all) / a.plays)
    sched = optim.cosine_decay(a.lr, steps, a.lr * 0.05)
    opt = optim.AdamW(learning_rate=sched, weight_decay=1e-4)
    cur = []
    acc_share = 0.0
    for spec in a.curriculum.split(','):
        L, share = spec.split(':')
        acc_share += float(share)
        cur.append((int(acc_share * steps), int(L)))
    hold_plays = list(range(len(hold.plays)))
    hold_eval = hold_plays[::a.eval_plays]
    config = {'run': os.path.basename(os.path.normpath(a.out)), 'variant': 'M (learned memory)', 'plays_per_step': a.plays, 'lr': a.lr, 'epochs': a.epochs, 'steps': steps,
              'schedule': f'AdamW (weight decay 1e-4), cosine decay to 0.05 lr over {steps} steps, grad clip 1.0', 'aux_weight': a.aux, 'mem_drop': a.mem_drop, 'curriculum': cur,
              'parameters': n_params, 'T': tm, 'C': cm, 'promos': promos, 'init': a.init or 'hand_init', 'best_teachers': best, 'labelled_decisions': labelled,
              'decision_weight': {'won_play': 1.0, 'lost_play': a.lost_weight, 'best_teacher_factor': a.best_factor, 'dagger_label': 2.0, 'up_families': sorted(up), 'up_factor': a.up_factor if up else 1.0},
              'losses': {'pointer_ce': 1.0, 'action_ce': 1.0, 'value': 0.5, 'danger_mse': 0.5, 'aux_teacher_state': a.aux},
              'train': {'files': a.train, 'decisions': len(train), 'plays': len(train.plays)}, 'hold': {'files': a.hold, 'decisions': len(hold), 'plays': len(hold.plays)}, 'device': str(mx.default_device())}
    json.dump(config, open(os.path.join(a.out, 'config.json'), 'w'), indent=1)
    curves = open(os.path.join(a.out, 'curves.jsonl'), 'w')
    print(f'{n_params} parameters, {steps} steps of {a.plays} plays; curriculum {cur}', flush=True)

    def loss_fn(model, plan):
        loss, main, aux, parts, accs = run_step(model, dev, plan, tm, a.aux)
        return loss, (main, aux)

    grad_fn = nn.value_and_grad(model, loss_fn)
    best_both = -1.0
    order = []
    seen_dec = 0
    log = collections.defaultdict(float)
    log_n = 0
    log_t = 0.0
    log_dec = 0
    t_train = 0.0
    stopped = False
    for s in range(1, steps + 1):
        L = next((l for upto, l in cur if s <= upto), cur[-1][1])
        if not order:
            order = plays_all[:]
            rng.shuffle(order)
        chunk, order = order[:a.plays], order[a.plays:]
        ts = time.time()
        plan = plan_batch(dev, chunk, L, a.mem_drop, rng, tm)
        n_dec = sum(len(g[0]) for g in plan[3].values())
        (loss, (main, aux)), grads = grad_fn(model, plan)
        grads, norm = optim.clip_grad_norm(grads, 1.0)
        opt.update(model, grads)
        mx.eval(model.parameters(), opt.state, loss, main, aux, norm)
        dt = time.time() - ts
        t_train += dt
        log_t += dt
        seen_dec += n_dec
        log_dec += n_dec
        log['loss'] += loss.item()
        log['main'] += main.item()
        log['aux'] += aux.item()
        log['norm'] += norm.item()
        log_n += 1
        is_eval = s % a.eval_every == 0 or s == steps
        if time.time() - t0 > a.max_minutes * 60:
            is_eval, stopped = True, True
        if s % a.log_every == 0 or is_eval:
            rec = {'step': s, 'L': L, 'epoch': s * a.plays / len(plays_all), **{k: v / log_n for k, v in log.items()}, 'lr': float(sched(s)), 'dps': log_dec / max(log_t, 1e-9), 'dps_mean': seen_dec / t_train, 'wall_s': time.time() - t0}
            if is_eval:
                ev, by_family = evaluate(model, hdev, hold_plays if (stopped or s == steps) else hold_eval, tm)
                rec['hold'] = ev
                rec['hold_by_family'] = by_family
            log = collections.defaultdict(float)
            log_n, log_t, log_dec = 0, 0.0, 0
            curves.write(json.dumps(rec) + '\n')
            curves.flush()
            print(json.dumps({k: v for k, v in rec.items() if k != 'hold_by_family'}), flush=True)
            if is_eval:
                model.save_weights(os.path.join(a.out, 'last.safetensors'))
                if ev['acc_both'] > best_both:
                    best_both = ev['acc_both']
                    model.save_weights(os.path.join(a.out, 'weights.safetensors'))
                    export(model, os.path.join(a.out, 'army-net.bin'))
        if stopped:
            print(f'stopped at step {s} of {steps} (the {a.max_minutes} min limit of one GPU job)', flush=True)
            break
    print(f'done in {time.time() - t0:.0f} s; best held-out joint accuracy {best_both:.4f}', flush=True)


if __name__ == '__main__':
    main()
