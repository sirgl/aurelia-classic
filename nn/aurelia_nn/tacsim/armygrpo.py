"""The GRPO smoke of the army net on assault-heal, inside the exact MLX twin of the trainer scenes (`armyplay`).

    python -m aurelia_nn.tacsim.armygrpo --train data/ev-M0-rule-train.jsonl --held data/ev-M0-rule.jsonl \\
        --weights RUN/weights.safetensors --out data/grpo-smoke.json --save data/grpo-smoke-weights.safetensors

Turn-level groups (GATES.md, "the GRPO smoke on assault-heal"): per update P plays x G copies, every copy of a play on
the same combat stream; one turn r (drawn per update) where the copies sample the net's pointer picks and actions
(temperature 1), every other turn greedy; a copy's reward is its win at the play's end; the advantage is the reward
minus its group's mean over the group's standard deviation (a group with one outcome gives no gradient). Loss: minus
the advantage times the log-probability of the copy's sampled picks and actions of turn r (the memory is an input),
mean over the sampled decisions, Adam, no KL. The held-out evaluation is the greedy own play of the net on the held-out
plays (`copies=1`), the same as the G2 gate's.
"""

import argparse
import json
import os
import random
import time

import mlx.core as mx
import mlx.nn as nn
import mlx.optimizers as optim
from mlx.utils import tree_map

from . import armyobs as A
from .armyloop import Brain, load_net
from .armyplay import Game

FAMILY = "assault-heal"


def setup(g):
    g.scene_rule = True
    g.hold_heals = True
    g.judge_after_script = True


def evaluate(plays, brain):
    """Greedy own play of every held-out play once: (wins, plays)."""
    g = Game(plays, 1, brain)
    setup(g)
    g.compare = False
    v = g.play().tolist()
    return sum(1 for x in v if x == 1), len(v)


def sample_games(plays, brain, P, G, r, key):
    """One update's play: P plays x G copies, turn r sampled. Returns the game (records, verdict)."""
    g = Game(plays, G, brain, salt=False)
    setup(g)
    g.compare = False
    g.sample_rel = r
    g.sample_key = key
    g.play()
    return g


def advantages(verdict, P, G):
    """Per row: (reward - the group's mean) / (the group's std + eps); row b plays play b % P. Returns (adv [B] list, rewards [B] list, share of groups with a gradient)."""
    R = [1.0 if v == 1 else 0.0 for v in verdict]
    adv = [0.0] * (P * G)
    live = 0
    for p in range(P):
        rs = [R[p + P * k] for k in range(G)]
        mean = sum(rs) / G
        std = (sum((x - mean) ** 2 for x in rs) / G) ** 0.5
        if std > 1e-6:
            live += 1
            for k in range(G):
                adv[p + P * k] = (rs[k] - mean) / (std + 1e-6)
    return adv, R, live / P


def make_loss(model, brain):
    def loss_fn(feat, dist, mask, slot, um, pm, price, sel, pidx, pk, w, cfeat, cpromo, cmask, aidx, has_act):
        tok = {"feat": feat, "dist": dist, "mask": mask, "slot": slot}
        h = brain.body(tok, um, pm, None, price)
        n = feat.shape[0]
        lg = brain.pointer(h, sel)
        lp = mx.take_along_axis(lg - mx.logsumexp(lg, axis=-1, keepdims=True), pidx[:, None], axis=1)[:, 0]
        if cfeat is not None:
            al = brain.actions(h, pk, {"feat": cfeat, "promo": cpromo, "mask": cmask}, None)
            la = mx.take_along_axis(al - mx.logsumexp(al, axis=-1, keepdims=True), aidx[:, None], axis=1)[:, 0]
            lp = lp + mx.where(has_act, la, 0.0)
        return -mx.sum(w * lp)
    return nn.value_and_grad(model, loss_fn)


def gradient(grad_fn, records, adv, n_total):
    """The summed gradient over the recorded steps (chunk by chunk) of the loss normalised by `n_total` decisions."""
    total, loss = None, 0.0
    for rec in records:
        gid = rec["gid"].tolist()
        w = [adv[i] / n_total for i in gid]
        if not any(w):
            continue
        tok = rec["tok"]
        c = rec.get("cand")
        pick = rec["pick"]
        l, gr = grad_fn(tok["feat"], tok["dist"], tok["mask"], tok["slot"], rec["um"], rec["pm"], rec["price"], rec["sel"], rec["pidx"],
                        mx.maximum(pick, 0), mx.array(w, dtype=mx.float32),
                        None if c is None else c["feat"], None if c is None else c["promo"], None if c is None else c["mask"],
                        None if c is None else rec["aidx"], pick >= 0)
        total = gr if total is None else tree_map(lambda a, b: a + b, total, gr)
        mx.eval(total, l)
        loss += l.item()
    return loss, total


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--train", required=True)
    ap.add_argument("--held", required=True)
    ap.add_argument("--weights", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--save", required=True)
    ap.add_argument("--P", type=int, default=64)
    ap.add_argument("--G", type=int, default=8)
    ap.add_argument("--lr", type=float, default=3e-5)
    ap.add_argument("--updates", type=int, default=300)
    ap.add_argument("--minutes", type=float, default=60.0, help="stop starting updates after this many minutes of training")
    ap.add_argument("--rmax", type=int, default=14)
    ap.add_argument("--eval-every", type=int, default=50)
    ap.add_argument("--clip", type=float, default=1.0)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--eval-only", action="store_true")
    a = ap.parse_args(argv)
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    random.seed(a.seed)
    mx.random.seed(a.seed)
    t0 = time.time()
    train = A.load(a.train, families=[FAMILY])
    held = A.load(a.held, families=[FAMILY])
    print(f"loaded {len(train)} training plays, {len(held)} held-out plays in {time.time() - t0:.0f}s", flush=True)
    model = load_net(a.weights)  # a copy: the weights file is only read
    brain = Brain(model)
    curve = {"settings": {"P": a.P, "G": a.G, "lr": a.lr, "updates": a.updates, "minutes": a.minutes, "r": [0, a.rmax], "clip": a.clip, "seed": a.seed,
                          "optimizer": "Adam", "temperature": 1.0, "train_plays": len(train), "held_plays": len(held)},
             "held": [], "updates": []}

    def save_curve():
        with open(a.out, "w") as fh:
            json.dump(curve, fh, indent=1)

    def held_eval(u):
        t = time.time()
        w, n = evaluate(held, brain)
        curve["held"].append({"update": u, "wins": w, "plays": n, "win_pct": round(100 * w / n, 1)})
        print(f"HELD update {u}: {w}/{n} = {100 * w / n:.1f}%  ({time.time() - t:.0f}s)", flush=True)
        save_curve()
        return w, n

    w0, n0 = held_eval(0)
    if a.eval_only:
        return
    opt = optim.Adam(learning_rate=a.lr)
    grad_fn = make_loss(model, brain)
    t_train = time.time()
    done = 0
    for u in range(1, a.updates + 1):
        if time.time() - t_train > a.minutes * 60:
            break
        tu = time.time()
        r = random.randint(0, a.rmax)
        sub = random.sample(train, a.P)
        key = mx.random.key(a.seed * 100003 + u)
        g = sample_games(sub, brain, a.P, a.G, r, key)
        verdict = g.verdict.tolist()
        adv, R, share = advantages(verdict, a.P, a.G)
        n_dec = sum(len(rec["gid"]) for rec in g.records)
        n_act = sum(int(mx.sum(rec["pick"] >= 0).item()) for rec in g.records)
        loss, gr = (0.0, None)
        norm = 0.0
        if n_dec:
            loss, gr = gradient(grad_fn, g.records, adv, n_dec)
        if gr is not None:
            gr, nrm = optim.clip_grad_norm(gr, a.clip)
            norm = nrm.item()
            if not (norm == norm and norm < float("inf")) or loss != loss:
                print(f"update {u}: non-finite loss {loss} / grad norm {norm}: skipped", flush=True)
            else:
                opt.update(model, gr)
                mx.eval(model.parameters(), opt.state)
        done = u
        row = {"update": u, "r": r, "decisions": n_dec, "picks": n_act, "mean_reward": round(sum(R) / len(R), 4), "groups_with_gradient": round(share, 3),
               "loss": round(loss, 6), "grad_norm": round(norm, 4), "seconds": round(time.time() - tu, 1)}
        curve["updates"].append(row)
        print(json.dumps(row), flush=True)
        del g
        mx.clear_cache()
        if u % a.eval_every == 0:
            held_eval(u)
            model.save_weights(a.save)
    if not curve["held"] or curve["held"][-1]["update"] != done:
        held_eval(done)
    model.save_weights(a.save)
    wf, nf = curve["held"][-1]["wins"], curve["held"][-1]["plays"]
    p1, p2 = w0 / n0, wf / nf
    pp = (w0 + wf) / (n0 + nf)
    se = (pp * (1 - pp) * (1 / n0 + 1 / nf)) ** 0.5
    z = (p2 - p1) / se if se else 0.0
    curve["result"] = {"start_pct": round(100 * p1, 1), "final_pct": round(100 * p2, 1), "diff_pts": round(100 * (p2 - p1), 1), "z": round(z, 2),
                       "outside_95pct_noise": abs(z) > 1.96, "updates_done": done, "train_seconds": round(time.time() - t_train)}
    save_curve()
    print(json.dumps(curve["result"]), flush=True)


if __name__ == "__main__":
    main()
