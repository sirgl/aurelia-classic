"""The RL loop of step 1.10 (rl3): Rust actors sample, the MLX learner learns.

    python -m aurelia_nn.rl3.loop --out RUN --weights W.safetensors --actor-bin ai-exam --kept kept.jsonl --lengths rows.jsonl \\
        --anchor teach.bin [--anchor-v2 teach-v2.bin] [--resume] [--updates N]

One update: the allocation of the scenes over the families (armyrl.allocate) -> plan and kept files -> `ai-exam suite --record` with
`ARMY_SAMPLE_PLAN` (G copies of every scene, K sampled rounds, the net's own log-probs and memory in the record) -> `learner.step` (RL
gradient, optional BPTT over `--window` turns) -> the anchor's gradient (armyrl.Anchor, lambda by rl2's rule: --anchor-auto x of the
equal norms at update 1) -> clip, Adam -> export -> next update. Every `--ckpt-every` updates the weights, Adam's state and the loop's
state are written (atomically): a restart with --resume continues from the last. Every `--val-every` updates the weights play the
validation instances in Rust (greedy). `heartbeat` is touched every update (the watchdog's input).
"""
import argparse
import collections
import json
import os
import random
import shutil
import subprocess
import time

import mlx.core as mx
import mlx.optimizers as optim
from mlx.utils import tree_flatten, tree_map, tree_unflatten

from aurelia_nn.army import train_m as TM
from aurelia_nn.army.export import export
from aurelia_nn.tacsim import armyrl

from . import learner, scenes
from .net import is_new, load_any


def log(out, row):
    with open(os.path.join(out, "log.jsonl"), "a") as fh:
        fh.write(json.dumps(row) + "\n")
    print(json.dumps({k: v for k, v in row.items() if k not in ("families", "weights")}), flush=True)


def run_actor(a, weights_bin, udir, families, jobs, plan=True):
    """The Rust actor of one update (sampling plan) or a validation pass (greedy). Returns (seconds, returncode)."""
    env = dict(os.environ, ARMY_NET=weights_bin)
    cmd = [a.actor_bin, "suite", "--kept", os.path.join(udir, "kept.jsonl") if plan else a.kept, "--kept-version", "2", "--families", ",".join(families),
           "--instances", "60" if plan else str(a.val_to), "--players", "army-net", "--jobs", str(jobs), "--out", os.path.join(udir, "out")]
    if not plan:
        cmd += ["--from", str(a.val_from)]
    else:
        cmd += ["--record", os.path.join(udir, "rec.bin")]
        env["ARMY_SAMPLE_PLAN"] = os.path.join(udir, "plan.json")
    t = time.time()
    p = subprocess.Popen(cmd, env=env, stdout=open(os.path.join(udir, "actor.log"), "w"), stderr=subprocess.STDOUT)
    return p, t


def wait_actor(p, t, minutes):
    while p.poll() is None:
        if time.time() - t > minutes * 60:
            p.kill()
            p.wait()
            return time.time() - t, -9
        time.sleep(0.05)
    return time.time() - t, p.returncode


def rows_of(udir):
    out = []
    for line in open(os.path.join(udir, "out", "rows.jsonl")):
        out.append(json.loads(line))
    return out


def save_state(a, model, opt, state, name):
    d = a.out
    model.save_weights(os.path.join(d, f"{name}.tmp.safetensors"))
    os.replace(os.path.join(d, f"{name}.tmp.safetensors"), os.path.join(d, f"{name}.safetensors"))
    mx.save_safetensors(os.path.join(d, f"{name}.opt.tmp.safetensors"), dict(tree_flatten(opt.state)))
    os.replace(os.path.join(d, f"{name}.opt.tmp.safetensors"), os.path.join(d, f"{name}.opt.safetensors"))
    tmp = os.path.join(d, f"{name}.state.json.tmp")
    json.dump(state, open(tmp, "w"))
    os.replace(tmp, os.path.join(d, f"{name}.state.json"))
    json.dump({"name": name}, open(os.path.join(d, "latest.json.tmp"), "w"))
    os.replace(os.path.join(d, "latest.json.tmp"), os.path.join(d, "latest.json"))


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--weights", required=True, help="the start weights (safetensors; N175 or its typed conversion)")
    ap.add_argument("--actor-bin", required=True)
    ap.add_argument("--kept", required=True, help="kept scenes of every family (jsonl)")
    ap.add_argument("--lengths", required=True, help="rows of a greedy pass of the start net over the training scenes (lengths and start rates)")
    ap.add_argument("--anchor", required=True)
    ap.add_argument("--anchor-v2", default=None)
    ap.add_argument("--anchor-plays", type=int, default=32)
    ap.add_argument("--anchor-auto", type=float, default=3.0, help="lambda = this x ||g_RL|| / ||g_anchor|| of update 1 (one digit); 0: --anchor-w")
    ap.add_argument("--anchor-w", type=float, default=0.1)
    ap.add_argument("--anchor-exclude", default="", help="comma-separated families the anchor never draws")
    ap.add_argument("--P", type=int, default=96)
    ap.add_argument("--G", type=int, default=8)
    ap.add_argument("--K", type=int, default=6)
    ap.add_argument("--min-per-family", type=int, default=1)
    ap.add_argument("--boost", type=float, default=4.0)
    ap.add_argument("--bar", type=float, default=0.9, help="the win rate each family is pushed to (allocation weights)")
    ap.add_argument("--lr", type=float, default=3e-5)
    ap.add_argument("--new-lr-mult", type=float, default=10.0, help="the typed part's lr as a multiple of --lr")
    ap.add_argument("--clip", type=float, default=1.0)
    ap.add_argument("--window", type=int, default=0, help="BPTT: the memory is recomputed over this many turns with gradient (0: an input)")
    ap.add_argument("--jobs", type=int, default=7, help="the actor's threads")
    ap.add_argument("--updates", type=int, default=185)
    ap.add_argument("--minutes", type=float, default=1e9)
    ap.add_argument("--ckpt-every", type=int, default=25)
    ap.add_argument("--val-every", type=int, default=50)
    ap.add_argument("--val-from", type=int, default=60)
    ap.add_argument("--val-to", type=int, default=80)
    ap.add_argument("--actor-minutes", type=float, default=20.0)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--overlap", action="store_true", help="the actor of update u+1 plays while the learner learns update u (weights one update behind)")
    ap.add_argument("--keep-records", action="store_true")
    ap.add_argument("--resume", action="store_true")
    a = ap.parse_args(argv)
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    os.makedirs(a.out, exist_ok=True)
    pool = scenes.Pool(a.kept, a.lengths)
    fams = pool.families
    bars = {f: ("bar", a.bar, a.bar) for f in fams}
    model = load_any(a.weights)
    model.train()
    state = {"update": 0, "rate": {f: pool.start_rate(f) for f in fams}, "lam": a.anchor_w, "seconds": 0.0}
    opt = optim.Adam(learning_rate=a.lr)
    if any(is_new(k) for k, _ in tree_flatten(model.parameters())):
        opt = optim.MultiOptimizer([optim.Adam(learning_rate=a.lr * a.new_lr_mult), opt], [lambda path, _: is_new(path)])
    latest = os.path.join(a.out, "latest.json")
    if a.resume and os.path.exists(latest):
        name = json.load(open(latest))["name"]
        model.load_weights(os.path.join(a.out, f"{name}.safetensors"))
        state = json.load(open(os.path.join(a.out, f"{name}.state.json")))
        # Adam's state needs its shapes first: one zero step (lr applied to zeros changes nothing), then the saved state replaces it.
        opt.init(model.trainable_parameters())
        opt.state = tree_unflatten(list(mx.load(os.path.join(a.out, f"{name}.opt.safetensors")).items()))
        print(f"resumed from {name}: update {state['update']}", flush=True)
    anchor = armyrl.Anchor(a.anchor, a.anchor_plays, a.seed, model, exclude=tuple(x for x in a.anchor_exclude.split(",") if x), v2_path=a.anchor_v2, v2_unmarked_version=2 if a.anchor_v2 else None)
    t_run = time.time()
    pending = None  # the overlapped actor: (process, start time, udir)

    def start_actor(u):
        udir = os.path.join(a.out, "work", f"u{u}")
        shutil.rmtree(udir, ignore_errors=True)
        os.makedirs(udir)
        n_of, wts = armyrl.allocate(a.P, fams, state["rate"], bars, a.min_per_family, a.boost)
        sel = scenes.draw(pool, n_of, a.K, a.G, a.seed * 100003 + u, udir)
        wbin = os.path.join(udir, "army-net.bin")
        export(model, wbin)
        p, t = run_actor(a, wbin, udir, sorted({k["family"] for _, k, _ in sel}), a.jobs)
        return p, t, udir, n_of, len(sel)

    def val_due(u):
        return bool(a.val_every) and (u % a.val_every == 0 or u == a.updates)

    # The validation of the state we start from (update 0, or the update a restart resumes at, when a crash came between its checkpoint and its validation).
    if val_due(state["update"]) and not os.path.exists(os.path.join(a.out, "val", f"u{state['update']:04d}", "done")):
        validate(a, model, state["update"], fams)
    for u in range(state["update"] + 1, a.updates + 1):
        if time.time() - t_run > a.minutes * 60:
            break
        tu = time.time()
        row = {"update": u}
        cur = pending if pending is not None else start_actor(u)
        pending = None
        p, t0, udir, n_of, n_scenes = cur
        if a.overlap and u < a.updates:
            pending = start_actor(u + 1)  # plays with the weights now: one update behind
        row["actor_s"], rc = wait_actor(p, t0, a.actor_minutes)
        if rc != 0 or not os.path.exists(os.path.join(udir, "rec.bin")):
            row["actor_failed"] = rc
            log(a.out, row)
            raise SystemExit(f"actor failed at update {u} (see {udir}/actor.log)")
        t1 = time.time()
        stream = TM.Stream([os.path.join(udir, "rec.bin")], 1, {}, None)
        loss, g, info, bt = learner.step(model, stream, a.window)
        row.update({k: info[k] for k in ("plays", "scenes", "decisions", "live_decisions", "groups_with_gradient", "memory_drift", "read_s", "gradient_s", "states")})
        row["learner_s"] = round(time.time() - t1, 2)
        n_rl = armyrl.norm_of(g) if g is not None else 0.0
        rows = rows_of(udir)
        per = collections.defaultdict(lambda: [0, 0])
        for r in rows:
            per[r["family"]][0] += int(bool(r["win"]))
            per[r["family"]][1] += 1
        for f, (w, n) in per.items():
            state["rate"][f] = w / n if f not in state["rate"] else 0.8 * state["rate"][f] + 0.2 * (w / n)
        row["win_rate"] = round(sum(w for w, _ in per.values()) / max(sum(n for _, n in per.values()), 1), 4)
        row["families"] = {f: {"win": round(w / n, 3), "scenes": n_of.get(f, 0)} for f, (w, n) in sorted(per.items())}
        t2 = time.time()
        la, lm, ga = anchor.grad()
        n_an = armyrl.norm_of(ga)
        if a.anchor_auto and u == 1 and state["update"] == 0 and n_an > 0:
            state["lam"] = float(f"{a.anchor_auto * n_rl / n_an:.1g}")
            print(f"lambda = {a.anchor_auto} x {n_rl:.4f} / {n_an:.4f} -> {state['lam']}", flush=True)
        ga = tree_map(lambda x: state["lam"] * x, ga)
        total = ga if g is None else tree_map(lambda x, y: x + y, g, ga)
        total, nrm = optim.clip_grad_norm(total, a.clip)
        norm = nrm.item()
        if norm == norm and norm < float("inf"):
            opt.update(model, total)
            mx.eval(model.parameters(), opt.state)
        else:
            row["skipped"] = True
        row["anchor_s"] = round(time.time() - t2, 2)
        row.update({"loss_rl": round(loss, 5), "grad_rl": round(n_rl, 4), "anchor_loss": round(la, 4), "grad_anchor": round(n_an, 4), "grad_total": round(norm, 4), "lambda": state["lam"],
                    "peak_gb": round(mx.get_peak_memory() / 2**30, 2)})
        t3 = time.time()
        if pending is None and not a.overlap:
            pass
        row["update_s"] = round(time.time() - tu, 2)
        state["update"], state["seconds"] = u, state["seconds"] + row["update_s"]
        log(a.out, row)
        open(os.path.join(a.out, "heartbeat"), "w").write(f"{u} {time.time()}\n")
        if not a.keep_records:
            shutil.rmtree(udir, ignore_errors=True)
        del bt, stream
        mx.clear_cache()
        if u % a.ckpt_every == 0 or u == a.updates:
            save_state(a, model, opt, state, f"ckpt-{u:04d}")
        if val_due(u):
            validate(a, model, u, fams)
    if pending is not None:
        pending[0].kill()
    open(os.path.join(a.out, "done"), "w").write(str(state["update"]))


def validate(a, model, u, fams):
    """The weights play the validation instances in Rust, greedy (rows in val/uNNNN/out/rows.jsonl, rates in the log)."""
    d = os.path.join(a.out, "val", f"u{u:04d}")
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    wbin = os.path.join(d, "army-net.bin")
    export(model, wbin)
    t = time.time()
    p, t0 = run_actor(a, wbin, d, fams, a.jobs, plan=False)
    secs, rc = wait_actor(p, t0, 60.0)
    per = collections.defaultdict(lambda: [0, 0])
    if rc == 0:
        for r in rows_of(d):
            per[r["family"]][0] += int(bool(r["win"]))
            per[r["family"]][1] += 1
    mean = sum(w / n for w, n in per.values()) / max(len(per), 1)
    log(a.out, {"validation": u, "seconds": round(secs, 1), "rc": rc, "family_mean": round(mean, 4), "plays": sum(n for _, n in per.values()),
                "rates": {f: round(w / n, 3) for f, (w, n) in sorted(per.items())}})
    os.remove(wbin)
    if rc == 0:
        open(os.path.join(d, "done"), "w").write(str(u))


if __name__ == "__main__":
    main()
