"""Multi-family GRPO of the army net inside the exact MLX twin, with an imitation anchor (GATES.md, "multi-family GRPO").

    python -m aurelia_nn.tacsim.armyrl --train data/tr-MBf6c-r.jsonl --held data/ho-MBf6c-r.jsonl \\
        --weights RUN/weights.safetensors --anchor data/teach-r-train.bin --bars data/teach-r-train/rows.jsonl \\
        --out data/rl1 [--K 3] [--anchor-w 1.0]

Per update: P plays drawn from every twin family (at least `--min-per-family` each, the rest weighted to the families
whose running training win rate is below their bar: 0.9 x the best teacher's rate on the training instances), each
play x G copies on one combat stream (`salt` off); per play K turns (distinct rounds of the play's length in its Rust
record) where the copies sample the net's picks and actions (temperature 1), every other turn greedy. A copy's
reward is its family's win. Advantage: (reward - the group's mean) / the group's std (a group with one outcome gives
none). RL loss: minus the advantage times the log-probability of the copy's sampled picks and actions, averaged over
the decisions of the groups that carry a gradient (the memory and the turn's map are inputs: the map encoder gets
no RL gradient). Anchor: `--anchor-w` x the imitation loss of `train_m` (the MBf6c recipe: decision weights won 1 /
lost 0.1 / best teacher x2, siege / ambush-fog / hunt-fog x2, aux 0.3) on `--anchor-plays` recorded teacher plays
of all families. One Adam step on the sum (clip 1.0). Reported per update: the share of groups with a gradient
(all and per family), the KL of the policy to the start's on the sampled decisions (diagnostic, not in the loss),
the two gradient norms. Every `--ckpt-every` updates: the weights and the greedy held-out own play per line.
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
from mlx.utils import tree_flatten, tree_map

from . import armymemgrad
from . import armyobs as A
from . import armyprice as PR
from .armyloop import Brain, load_net
from .armyplay import Game

FAMILIES = ["siege", "ram", "bombard", "promotion-choice", "planned-assault", "raid", "prey", "assault-heal", "hill-archers", "city-fire"]
# Package 1's two families (their stake is their own: the "at any cost" families), played by `--price` (step 1.8).
PACKAGE1 = ["assault-peace", "assault-reinforce"]
PRICE_KEYS = ("w_price", "w_price_all")
SIEGE_UP = None


def setup(g):
    g.scene_rule = True
    g.hold_heals = True
    g.judge_after_script = True
    g.compare = False


def line_of(sc):
    fam = sc["play"].split("/")[0]
    return "prey-land" if fam == "prey" and sc["orders"].get("context") == "land" else fam


def evaluate(plays, brain, price=False):
    """Greedy own play of every held-out play once: {line: [wins, plays]}. With `price` (step 1.8) each play of a price family
    is played three times (its own stake, `careful`, `anycost:D`) and counted on its line `f`, `f@careful`, `f@anycost`."""
    entries = []
    for p in plays:
        fam = p[0]["play"].split("/")[0]
        entries += [PR.Entry(p, f) for f in ((PR.NATIVE, PR.CAREFUL, PR.ANYCOST) if price and fam in PR.PRICE_D else (PR.NATIVE,))]
    g = Game([e.play for e in entries], 1, brain)
    setup(g)
    PR.prepare_game(g, entries)
    g.play()
    rw = PR.rewards(g, entries, 1)
    out = collections.defaultdict(lambda: [0, 0])
    for e, r in zip(entries, rw):
        k = line_of(e.play[0]) if e.form == PR.NATIVE else e.line
        out[k][0] += int(r == 1.0)
        out[k][1] += 1
    return dict(out)


def play_length(p):
    """Rounds the play lasted in its Rust record (its moments' rounds)."""
    sc, ms, _ = p
    s0 = sc["orders"]["start_round"]
    return max([m["round"] - s0 for m in ms] + [0]) + 1


def bars_from(rows_path, families):
    """family -> 0.9 x the best teacher's win rate on the rows (the training instances' teacher plays)."""
    w = collections.defaultdict(lambda: [0, 0])
    for l in open(rows_path):
        r = json.loads(l)
        if r["family"] in families and r["info"].get("stage", "player") == "player":
            w[(r["family"], r["player"])][0] += r["win"]
            w[(r["family"], r["player"])][1] += 1
    best = {}
    for (f, p), (a, n) in w.items():
        if n and (f not in best or a / n > best[f][1]):
            best[f] = (p, a / n)
    return {f: (p, r, 0.9 * r) for f, (p, r) in best.items()}


def allocate(P, families, rate, bars, min_each, boost):
    """Plays per family: min_each each, the rest by 1 + boost x max(0, bar - running rate) (largest remainder)."""
    n = {f: min_each for f in families}
    rest = P - min_each * len(families)
    wts = {f: 1.0 + boost * max(0.0, bars[f][2] - rate[f]) if rate.get(f) is not None else 1.0 for f in families}
    tot = sum(wts.values())
    share = {f: rest * wts[f] / tot for f in families}
    for f in families:
        n[f] += int(share[f])
    left = P - sum(n.values())
    for f in sorted(families, key=lambda f: -(share[f] - int(share[f])))[:left]:
        n[f] += 1
    return n, wts


def advantages(verdict, P, G, rewards=None, gkey=None):
    """Group-normalised advantages; row p + P k is copy k of play p; the plays with the same `gkey` form ONE group (a scene under one
    stake; a group of more than G copies is the same scene played twice). `rewards`: the per-row reward (step 1.8: the price form's
    verdict); None: the verdict == win. Returns (advantages, rewards, live: per play)."""
    R = rewards if rewards is not None else [1.0 if v == 1 else 0.0 for v in verdict]
    gkey = list(range(P)) if gkey is None else gkey
    members = collections.defaultdict(list)
    for p in range(P):
        members[gkey[p]].append(p)
    adv = [0.0] * (P * G)
    live = [False] * P
    for plays in members.values():
        rows = [p + P * k for p in plays for k in range(G)]
        rs = [R[i] for i in rows]
        mean = sum(rs) / len(rs)
        std = (sum((x - mean) ** 2 for x in rs) / len(rs)) ** 0.5
        if std > 1e-6:
            for p in plays:
                live[p] = True
            for i, x in zip(rows, rs):
                adv[i] = (x - mean) / (std + 1e-6)
    return adv, R, live


def rec_inputs(rec, turn_M):
    tok = rec["tok"]
    c = rec.get("cand")
    M = None
    if turn_M:
        M = mx.take(turn_M[rec["rel"]], rec["gid"], axis=0)
    return tok, c, M


def logp_parts(brain, tok, um, pm, price, M, sel, pidx, pick, c, aidx, x=None):
    """Full log-softmax of the pointer and of the actions (None without candidates) at the recorded decisions. `x`: the
    notes / history the decision read (MBN / MBH), inputs: the gradient reaches their read path, not their writing."""
    h = brain.body(tok, um, pm, M, price, x)
    lg = brain.pointer(h, sel)
    lp = lg - mx.logsumexp(lg, axis=-1, keepdims=True)
    la = None
    if c is not None:
        al = brain.actions(h, mx.maximum(pick, 0), c, M)
        la = al - mx.logsumexp(al, axis=-1, keepdims=True)
    return lp, la


def make_rl_grad(model, brain):
    def loss_fn(tok, um, pm, price, M, sel, pidx, pick, w, c, aidx, x):
        lp, la = logp_parts(brain, tok, um, pm, price, M, sel, pidx, pick, c, aidx, x)
        l = mx.take_along_axis(lp, pidx[:, None], axis=1)[:, 0]
        if la is not None:
            l = l + mx.where(pick >= 0, mx.take_along_axis(la, aidx[:, None], axis=1)[:, 0], 0.0)
        return -mx.sum(w * l)
    return nn.value_and_grad(model, loss_fn)


def memory_rl_gradient(g, adv, n_live, rl_grad, turn_grad, window):
    """The RL gradient of a play with the memory unrolled over `window` turns (`armymemgrad`): the records of a turn
    that has earlier turns go through `turn_grad`, those of turn 0 (nothing before them) through `rl_grad` as always.
    Returns (loss, the summed gradient or None, the largest drift of the recomputed memory from the recorded one)."""
    by_turn = collections.defaultdict(list)
    for rec in g.records:
        by_turn[rec["rel"]].append(rec)
    total, loss, drift = None, 0.0, 0.0
    for rel, recs in sorted(by_turn.items()):
        live = [(rec, [adv[i] / max(n_live, 1) for i in rec["gid"].tolist()]) for rec in recs]
        live = [(rec, mx.array(w, dtype=mx.float32)) for rec, w in live if any(w)]
        if not live:
            continue
        if rel == 0:
            parts = []
            for rec, w in live:
                tok, c, M = rec_inputs(rec, g.turn_M)
                l, gr = rl_grad(tok, rec["um"], rec["pm"], rec["price"], M, rec["sel"], rec["pidx"], rec["pick"], w, c, rec.get("aidx"), rec.get("x"))
                mx.eval(gr, l)
                parts.append((l.item(), gr))
        else:
            l, gr, d = turn_grad(g.mem_tape, rel, window, [r for r, _ in live], [w for _, w in live], lambda rec: rec_inputs(rec, g.turn_M))
            parts, drift = [(l, gr)], max(drift, d)
        for l, gr in parts:
            total = gr if total is None else tree_map(lambda x, y: x + y, total, gr)
            loss += l
        mx.eval(total)
    return loss, total, drift


def kl_sum(brain, ref, rec, turn_M):
    """Sum over the record's rows of KL(policy || start) of the pointer, plus of the action where one was taken."""
    tok, c, M = rec_inputs(rec, turn_M)
    args = (tok, rec["um"], rec["pm"], rec["price"], M, rec["sel"], rec["pidx"], rec["pick"], c, rec.get("aidx"), rec.get("x"))
    lp, la = logp_parts(brain, *args)
    lq, lb = logp_parts(ref, *args)
    p = mx.exp(lp)
    k = mx.sum(mx.where(p > 0, p * (lp - lq), 0.0), axis=1)
    if la is not None:
        pa = mx.exp(la)
        ka = mx.sum(mx.where(pa > 0, pa * (la - lb), 0.0), axis=1)
        k = k + mx.where(rec["pick"] >= 0, ka, 0.0)
    return mx.sum(k).item(), int(k.shape[0])


class Anchor:
    """The imitation loss of `train_m` on recorded teacher plays (the MBf6c recipe), `plays` a step."""

    def __init__(self, path, plays, seed, model, exclude=()):
        from aurelia_nn.army import train_m as TM
        from aurelia_nn.army.train import SIEGE, best_teachers, decision_weights
        self.TM = TM
        self.stream = TM.Stream([path], 1, {}, None)
        self.best, _ = best_teachers(self.stream.metas)
        self.decision_weights = decision_weights
        self.up = set(SIEGE) | {"ambush-fog", "hunt-fog"}
        self.tm = self.stream.t_max
        self.plays = plays
        self.rng = random.Random(seed * 7919 + 1)
        self.order = []
        self.model = model
        # The plays the anchor draws from: all, or those of the families not in `exclude` (the ones RL does not train).
        self.pool = [i for i, m in enumerate(self.stream.metas) if m["family"] not in set(exclude)]

        def fn(m, dev, plan):
            loss, main, aux, parts, accs = TM.run_step(m, dev, plan, self.tm, 0.3)
            return loss, main
        self.grad_fn = nn.value_and_grad(model, fn)

    def device(self, ids):
        flat = self.stream.flat(ids)
        w = self.decision_weights(flat, self.best, 0.1, 2.0)
        w = [x * (2.0 if flat.plays[p]["family"] in self.up else 1.0) for x, p in zip(w, flat.play)]
        w = [0.0 if pt < -1 else x for x, pt in zip(w, flat.ptr)]
        return self.TM.DeviceM(flat, w, self.tm, 128)

    def grad(self):
        if len(self.order) < self.plays:
            self.order = list(self.pool)
            self.rng.shuffle(self.order)
        ids, self.order = self.order[:self.plays], self.order[self.plays:]
        dev = self.device(ids)
        plan = self.TM.plan_batch(dev, list(range(len(ids))), 999, 0.1, self.rng, self.tm)
        (loss, main), g = self.grad_fn(self.model, dev, plan)
        mx.eval(loss, main, g)
        return loss.item(), main.item(), g


def norm_of(g):
    return sum(mx.sum(v * v).item() for _, v in tree_flatten(g)) ** 0.5


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--train", required=True)
    ap.add_argument("--held", required=True)
    ap.add_argument("--weights", required=True)
    ap.add_argument("--anchor", default=None, help="teacher recordings (.bin) for the imitation anchor; none: no anchor")
    ap.add_argument("--bars", required=True, help="teacher rows of the training instances (the bars)")
    ap.add_argument("--out", required=True)
    ap.add_argument("--P", type=int, default=64)
    ap.add_argument("--G", type=int, default=8)
    ap.add_argument("--K", type=int, default=3, help="sampled turns per play")
    ap.add_argument("--min-per-family", type=int, default=2)
    ap.add_argument("--boost", type=float, default=4.0)
    ap.add_argument("--lr", type=float, default=3e-5)
    ap.add_argument("--anchor-w", type=float, default=1.0)
    ap.add_argument("--anchor-auto", type=float, default=0.0, help="lambda = this x ||g_RL|| / ||g_anchor|| of the first update (one digit); 0: --anchor-w")
    ap.add_argument("--anchor-plays", type=int, default=32)
    ap.add_argument("--anchor-skip-twin", action="store_true", help="the anchor draws only the plays of the families the twin does not train")
    ap.add_argument("--updates", type=int, default=300)
    ap.add_argument("--minutes", type=float, default=150.0)
    ap.add_argument("--ckpt-every", type=int, default=25)
    ap.add_argument("--clip", type=float, default=1.0)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--kl-every", type=int, default=1)
    ap.add_argument("--probe", action="store_true", help="no update: only the play and the share of groups with a gradient")
    ap.add_argument("--anchor-only", action="store_true", help="the control: no play, no RL; each update is the anchor's step alone")
    ap.add_argument("--no-eval0", action="store_true")
    ap.add_argument("--mem-grad", type=int, default=0, help="the RL gradient through the memory's last W turns' steps (armymemgrad); 0: the memory is an input")
    ap.add_argument("--price", action="store_true", help="step 1.8: the price in the reward: the six price families as `f@careful` / `f@anycost` lines (a group = a scene under one stake), package 1's two families added; two optimisers")
    ap.add_argument("--price-lr-mult", type=float, default=10.0, help="the lr of w_price / w_price_all as a multiple of --lr")
    ap.add_argument("--price-G", type=int, default=0, help="copies of a price-line group (0: --G)")
    ap.add_argument("--D-file", default=os.environ.get("ARMY_D_FILE", ""), help="a JSON file of the per-family any-cost best rates, {family: {\"anycost\": {name: rate}}} (the bars of the any-cost lines; needed with --price; default: $ARMY_D_FILE)")
    ap.add_argument("--new-layers", default="", help="net-deep: comma list of layer indices that are new (their own lr group)")
    ap.add_argument("--new-lr-mult", type=float, default=1.0, help="the new layers learn at this multiple of --lr")
    ap.add_argument("--split", type=int, default=70, help="training scenes: instances below it; the rest of --train is the validation set")
    a = ap.parse_args(argv)
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    random.seed(a.seed)
    mx.random.seed(a.seed)
    os.makedirs(a.out, exist_ok=True)
    t0 = time.time()
    FAMS = FAMILIES + (PACKAGE1 if a.price else [])
    train_all = [p for path in a.train.split(",") for p in A.load(path, families=FAMS)]
    held = [p for path in a.held.split(",") for p in A.load(path, families=FAMS)]
    inst = lambda p: int(p[0]["play"].split("/")[2])
    val = [p for p in train_all if inst(p) >= a.split]
    train_all = [p for p in train_all if inst(p) < a.split]
    by_fam = collections.defaultdict(list)
    for p in train_all:
        by_fam[p[0]["play"].split("/")[0]].append(p)
    fams_f = [f for f in FAMS if by_fam.get(f)]
    bars_f = bars_from(a.bars, set(fams_f))
    # The RL lines: a family, or with --price a price family's two lines (careful / anycost).
    lines = [PR.line_name(f, form) for f, form in PR.lines_of(fams_f, a.price)]
    fam_of_line = {PR.line_name(f, form): (f, form) for f, form in PR.lines_of(fams_f, a.price)}
    bars = {}
    for ln, (f, form) in fam_of_line.items():
        if form == PR.ANYCOST:
            if not a.D_file:
                raise SystemExit("--price needs --D-file (or $ARMY_D_FILE): the any-cost best rates per family")
            best = max(json.load(open(a.D_file))[f]["anycost"].values())
            bars[ln] = ("anycost", best, 0.9 * best)
        else:
            bars[ln] = bars_f[f]
    fams = lines
    print(f"loaded {len(train_all)} training plays ({', '.join(f'{f} {len(by_fam[f])}' for f in fams_f)}), {len(val)} validation plays (instances >= {a.split}), {len(held)} held-out plays in {time.time() - t0:.0f}s", flush=True)
    print("lines: " + " ".join(lines), flush=True)
    print("bars: " + ", ".join(f"{f} {bars[f][0]} {bars[f][1]:.3f} -> {bars[f][2]:.3f}" for f in fams), flush=True)
    model = load_net(a.weights)
    brain = Brain(model)
    ref = Brain(load_net(a.weights))
    anchor = Anchor(a.anchor, a.anchor_plays, a.seed, model, exclude=FAMS if a.anchor_skip_twin else ()) if a.anchor else None
    curve = {"settings": {k: v for k, v in vars(a).items()}, "bars": {f: list(bars[f]) for f in fams}, "train_plays": {f: len(by_fam[f]) for f in fams_f},
             "held_plays": len(held), "val_plays": len(val), "held": [], "val": [], "updates": []}
    if anchor:
        curve["anchor"] = {"plays": len(anchor.stream), "pool": len(anchor.pool), "decisions": anchor.stream.decisions, "best": {f: list(v) for f, v in anchor.best.items()}}

    def save_curve():
        with open(os.path.join(a.out, "curve.json"), "w") as fh:
            json.dump(curve, fh, indent=1)

    def held_eval(u):
        for name, plays in (("val", val), ("held", held)):
            if not plays:
                continue
            t = time.time()
            r = evaluate(plays, brain, a.price)
            w = sum(x[0] for x in r.values()); n = sum(x[1] for x in r.values())
            fam_mean = sum(v[0] / v[1] for k, v in r.items() if k != "prey-land") / max(1, sum(1 for k in r if k != "prey-land"))
            curve[name].append({"update": u, "lines": r, "wins": w, "plays": n, "family_mean": round(fam_mean, 4)})
            print(f"{name.upper()} {u}: {100 * w / n:.1f}% (family mean {100 * fam_mean:.1f}) | " + " ".join(f"{k} {100 * v[0] / v[1]:.1f}" for k, v in sorted(r.items())) + f" ({time.time() - t:.0f}s)", flush=True)
        save_curve()

    if not a.no_eval0 and not a.probe:
        held_eval(0)
    opt = optim.Adam(learning_rate=a.lr)
    if a.new_layers:
        new_l = {f"layers.{i}." for i in a.new_layers.split(",")}
        opt = optim.MultiOptimizer([optim.Adam(learning_rate=a.lr * a.new_lr_mult), opt], [lambda path, _: any(path.startswith(x) for x in new_l)])
        curve["new_layers"] = {"layers": sorted(new_l), "lr": a.lr * a.new_lr_mult}
    opt_price = optim.Adam(learning_rate=a.lr * a.price_lr_mult) if a.price else None
    rl_grad = make_rl_grad(model, brain)
    turn_grad = armymemgrad.make_turn_gradient(model, brain, logp_parts) if a.mem_grad else None
    rate = {}
    lam = a.anchor_w
    t_train = time.time()
    done = 0
    for u in range(1, a.updates + 1):
        if time.time() - t_train > a.minutes * 60:
            break
        tu = time.time()
        if a.anchor_only:
            la, lm, ga = anchor.grad()
            n_an = norm_of(ga)
            ga, nrm = optim.clip_grad_norm(ga, a.clip)
            opt.update(model, ga)
            mx.eval(model.parameters(), opt.state)
            row = {"update": u, "anchor_loss": round(la, 4), "anchor_main": round(lm, 4), "grad_anchor": round(n_an, 4), "seconds": round(time.time() - tu, 1)}
            curve["updates"].append(row)
            print(json.dumps(row), flush=True)
            done = u
            if u % a.ckpt_every == 0:
                model.save_weights(os.path.join(a.out, f"ckpt-{u:04d}.safetensors"))
                held_eval(u)
            continue
        n_of, wts = allocate(a.P, fams, rate, bars, a.min_per_family, a.boost)
        entries, gkey = [], []
        for ln in fams:
            f, form = fam_of_line[ln]
            dup = -(-a.price_G // a.G) if (a.price and form != PR.NATIVE and a.price_G > a.G) else 1
            for p in random.sample(by_fam[f], min(n_of[ln], len(by_fam[f]))):
                for _ in range(dup):
                    gkey.append(len(gkey) if dup == 1 else (ln, id(p)))
                    entries.append(PR.Entry(p, form))
        # A group = a scene under one stake (G copies): the copies of a price line's group may be more (`--price-G`).
        P, G = len(entries), a.G
        sub = [e.play for e in entries]
        plan = collections.defaultdict(lambda: [False] * (P * G))
        for pi, p in enumerate(sub):
            L = play_length(p)
            for r in random.sample(range(L), min(a.K, L)):
                for k in range(G):
                    plan[r][pi + P * k] = True
        g = Game(sub, G, brain, salt=False)
        setup(g)
        PR.prepare_game(g, entries)
        if a.mem_grad:
            g.mem_tape = armymemgrad.Tape(armymemgrad.wanted_turns(plan, a.mem_grad))
        g.sample_plan = {r: mx.array(m) for r, m in plan.items()}
        g.sample_key = mx.random.key(a.seed * 100003 + u)
        g.play()
        verdict = g.verdict.tolist()
        adv, R, live = advantages(verdict, P, G, PR.rewards(g, entries, G), gkey)
        fam_of = [e.line for e in entries]
        per = collections.defaultdict(lambda: [0, 0.0, 0])  # plays, reward sum, live groups
        for pi, f in enumerate(fam_of):
            per[f][0] += 1
            per[f][1] += sum(R[pi + P * k] for k in range(G)) / G
            per[f][2] += live[pi]
        for f, (n, s, _) in per.items():
            rate[f] = s / n if f not in rate else 0.8 * rate[f] + 0.2 * (s / n)
        n_live = sum(sum(1 for i in rec["gid"].tolist() if adv[i]) for rec in g.records)
        n_dec = sum(len(rec["gid"]) for rec in g.records)
        row = {"update": u, "P": P, "decisions": n_dec, "live_decisions": n_live, "mean_reward": round(sum(R) / len(R), 4),
               "groups_with_gradient": round(sum(live) / P, 3),
               "families": {f: {"plays": v[0], "reward": round(v[1] / v[0], 3), "live": v[2]} for f, v in sorted(per.items())},
               "weights": {f: round(w, 3) for f, w in wts.items()}}
        if True:
            total, loss_rl = None, 0.0
            if a.mem_grad:
                loss_rl, total, drift = memory_rl_gradient(g, adv, n_live, rl_grad, turn_grad, a.mem_grad)
                row["memory_drift"] = round(drift, 7)
            else:
                for rec in g.records:
                    gid = rec["gid"].tolist()
                    w = [adv[i] / max(n_live, 1) for i in gid]
                    if not any(w):
                        continue
                    tok, c, M = rec_inputs(rec, g.turn_M)
                    l, gr = rl_grad(tok, rec["um"], rec["pm"], rec["price"], M, rec["sel"], rec["pidx"], rec["pick"], mx.array(w, dtype=mx.float32), c, rec.get("aidx"), rec.get("x"))
                    total = gr if total is None else tree_map(lambda x, y: x + y, total, gr)
                    mx.eval(total, l)
                    loss_rl += l.item()
            n_rl = norm_of(total) if total is not None else 0.0
            if total is not None:
                row["grad_memory_rl"] = armymemgrad.group_norms(total)[0]
            if a.kl_every and u % a.kl_every == 0 and g.records:
                ks = [kl_sum(brain, ref, rec, g.turn_M) for rec in g.records]
                row["kl_to_start"] = round(sum(x for x, _ in ks) / max(sum(n for _, n in ks), 1), 5)
            la = lm = n_an = 0.0
            if anchor:
                la, lm, ga = anchor.grad()
                n_an = norm_of(ga)
                if a.anchor_auto and u == 1 and n_an > 0:
                    lam = float(f"{a.anchor_auto * n_rl / n_an:.1g}")
                    curve["lambda"] = lam
                    print(f"lambda = {a.anchor_auto} x {n_rl:.4f} / {n_an:.4f} -> {lam}", flush=True)
                ga = tree_map(lambda x: lam * x, ga)
                total = ga if total is None else tree_map(lambda x, y: x + y, total, ga)
            norm = 0.0
            if total is not None and not a.probe:
                total, nrm = optim.clip_grad_norm(total, a.clip)
                norm = nrm.item()
                if norm == norm and norm < float("inf"):
                    if opt_price is not None:
                        # The stake's projections have their own Adam (lr x --price-lr-mult); the main Adam sees zero for them.
                        gp = {k: total[k] for k in PRICE_KEYS}
                        total = dict(total)
                        for k in PRICE_KEYS:
                            total[k] = mx.zeros_like(total[k])
                        model.update(opt_price.apply_gradients(gp, {k: getattr(model, k) for k in PRICE_KEYS}))
                        row["w_price_norm"] = [round(mx.sum(getattr(model, k) ** 2).item() ** 0.5, 5) for k in PRICE_KEYS]
                    opt.update(model, total)
                    mx.eval(model.parameters(), opt.state)
                else:
                    row["skipped"] = True
            row.update({"loss_rl": round(loss_rl, 6), "grad_rl": round(n_rl, 4), "anchor_loss": round(la, 4), "anchor_main": round(lm, 4),
                        "grad_anchor": round(n_an, 4), "grad_total": round(norm, 4)})
        row["seconds"] = round(time.time() - tu, 1)
        curve["updates"].append(row)
        print(json.dumps({k: v for k, v in row.items() if k not in ("families", "weights")}) + " | " +
              " ".join(f"{f[:18]} {v['reward']:.2f}/{v['live']}" for f, v in row["families"].items()), flush=True)
        done = u
        del g
        mx.clear_cache()
        if not a.probe and u % a.ckpt_every == 0:
            model.save_weights(os.path.join(a.out, f"ckpt-{u:04d}.safetensors"))
            held_eval(u)
        save_curve()
    if not a.probe and (not curve["held"] or curve["held"][-1]["update"] != done):
        model.save_weights(os.path.join(a.out, f"ckpt-{done:04d}.safetensors"))
        held_eval(done)
    curve["done"] = {"updates": done, "train_seconds": round(time.time() - t_train)}
    save_curve()


if __name__ == "__main__":
    main()
