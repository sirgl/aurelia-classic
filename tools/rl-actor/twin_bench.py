"""The twin's side of the comparison: one `armyrl` update on the fixed scenes of sel-uN.json, with the parts timed.

The update is armyrl's own (nn/aurelia_nn/tacsim/armyrl.py, the code path of `main`): P 64 scenes x G 8 copies in the MLX twin, K = 6
sampled rounds a scene (the rounds of the selection, the same as the Rust actor gets), the copies sampling the picks and actions at
temperature 1; then the RL gradient (memory as an input); the anchor gradient (32 teacher plays) and the Adam step are timed
separately (they are the same work on the actor-learner side). The KL diagnostic of rl2 is timed too and left out of both sides.

    python twin_bench.py --sel data/sel-u1.json --weights W --updates 3 --out data/twin-u1.json
"""
import argparse
import collections
import json
import time

import mlx.core as mx
import mlx.optimizers as optim
from mlx.utils import tree_map

from aurelia_nn.tacsim import armyobs as A
from aurelia_nn.tacsim import armyprice as PR
from aurelia_nn.tacsim import armyrl as R
from aurelia_nn.tacsim.armyloop import Brain, load_net
from aurelia_nn.tacsim.armyplay import Game

DATA = "/Users/roman.ivanov/civ/work/nets/mlx-army/data"


def sync():
    mx.eval(mx.zeros(1))


def run_update(sel, brain, ref, plays_by_name, seed, with_grad=True, with_kl=False, anchor=None, model=None, rl_grad=None):
    t = {}
    entries = [PR.Entry(plays_by_name[s["name"]], PR.NATIVE) for s in sel["scenes"]]
    P, G = len(entries), sel["G"]
    plan = collections.defaultdict(lambda: [False] * (P * G))
    for pi, s in enumerate(sel["scenes"]):
        for r in s["rounds"]:
            for k in range(G):
                plan[r][pi + P * k] = True
    t0 = time.time()
    g = R.Game([e.play for e in entries], G, brain, salt=False)
    R.setup(g)
    PR.prepare_game(g, entries)
    g.sample_plan = {r: mx.array(m) for r, m in plan.items()}
    g.sample_key = mx.random.key(seed)
    t["build"] = time.time() - t0
    t0 = time.time()
    g.play()
    verdict = g.verdict.tolist()
    sync()
    t["rollout"] = time.time() - t0
    rewards = PR.rewards(g, entries, G)
    adv, Rw, live = R.advantages(verdict, P, G, rewards, list(range(P)))
    n_live = sum(sum(1 for i in rec["gid"].tolist() if adv[i]) for rec in g.records)
    n_dec = sum(len(rec["gid"]) for rec in g.records)
    out = {"decisions": n_dec, "live_decisions": n_live, "groups_with_gradient": sum(live) / P, "mean_reward": sum(Rw) / len(Rw), "verdict": verdict,
           "families": [s["family"] for s in sel["scenes"]], "P": P, "G": G}
    total = None
    if with_grad:
        t0 = time.time()
        loss = 0.0
        for rec in g.records:
            w = [adv[i] / max(n_live, 1) for i in rec["gid"].tolist()]
            if not any(w):
                continue
            tok, c, M = R.rec_inputs(rec, g.turn_M)
            l, gr = rl_grad(tok, rec["um"], rec["pm"], rec["price"], M, rec["sel"], rec["pidx"], rec["pick"], mx.array(w, dtype=mx.float32), c, rec.get("aidx"), rec.get("x"))
            total = gr if total is None else tree_map(lambda x, y: x + y, total, gr)
            mx.eval(total, l)
            loss += l.item()
        t["rl_gradient"] = time.time() - t0
        out["loss"] = loss
        out["grad_norm"] = R.norm_of(total) if total is not None else 0.0
    if with_kl and g.records:
        t0 = time.time()
        ks = [R.kl_sum(brain, ref, rec, g.turn_M) for rec in g.records]
        t["kl_diagnostic"] = time.time() - t0
    out["seconds"] = t
    return out, total


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--sel", nargs="+", required=True)
    ap.add_argument("--weights", default=DATA + "/g7/N175/weights.safetensors")
    ap.add_argument("--train", default=DATA + "/tr-MBf6c-r.jsonl")
    ap.add_argument("--anchor", default=DATA + "/teach-r-train.bin")
    ap.add_argument("--updates", type=int, default=1)
    ap.add_argument("--rollout-only", action="store_true")
    ap.add_argument("--no-anchor", action="store_true")
    ap.add_argument("--kl", action="store_true")
    ap.add_argument("--out", required=True)
    a = ap.parse_args(argv)
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    import subprocess
    print('--- res status at start\n' + subprocess.run(['/Users/roman.ivanov/civ/work/res/res', 'status'], capture_output=True, text=True).stdout, flush=True)
    print('TWIN COMMIT', subprocess.run(['git', '-C', '/Users/roman.ivanov/civ/work/nets/rust-actor/classic', 'rev-parse', '--short', 'HEAD~1'], capture_output=True, text=True).stdout.strip(), flush=True)
    sels = [json.load(open(p)) for p in a.sel]
    want = {s["name"] for sel in sels for s in sel["scenes"]}
    t0 = time.time()
    plays = A.load(a.train, families=R.FAMILIES)
    by_name = {p[0]["play"]: p for p in plays if p[0]["play"] in want}
    print(f"loaded {len(by_name)} scenes in {time.time() - t0:.0f}s", flush=True)
    del plays
    model = load_net(a.weights)
    brain = Brain(model)
    ref = Brain(load_net(a.weights))
    rl_grad = R.make_rl_grad(model, brain)
    anchor = R.Anchor(a.anchor, 32, 1, model, exclude=R.FAMILIES) if not a.no_anchor and not a.rollout_only else None
    opt = optim.Adam(learning_rate=0.0)  # lr 0: the step is timed, the weights stay N175 for every update
    results = []
    for k in range(a.updates):
        sel = sels[k % len(sels)]
        out, total = run_update(sel, brain, ref, by_name, 100003 + k, with_grad=not a.rollout_only, with_kl=a.kl, anchor=anchor, model=model, rl_grad=rl_grad)
        if anchor is not None:
            t0 = time.time()
            la, lm, ga = anchor.grad()
            out["seconds"]["anchor_gradient"] = time.time() - t0
            t0 = time.time()
            tot = tree_map(lambda x: 0.1 * x, ga) if total is None else tree_map(lambda x, y: x + 0.1 * y, total, ga)
            tot, nrm = optim.clip_grad_norm(tot, 1.0)
            opt.update(model, tot)
            mx.eval(model.parameters(), opt.state)
            out["seconds"]["clip_adam_step"] = time.time() - t0
        out["update"] = k
        out["peak_gb"] = round(mx.get_peak_memory() / 2**30, 2)
        out["sel"] = sel["u"]
        out["seconds"] = {key: round(v, 3) for key, v in out["seconds"].items()}
        results.append(out)
        print(json.dumps({k2: v for k2, v in out.items() if k2 not in ("verdict", "families")}), flush=True)
        mx.clear_cache()
    json.dump(results, open(a.out, "w"))


if __name__ == "__main__":
    main()
