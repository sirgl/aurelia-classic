"""Gate line 6: the learner's log-probs against Rust's own on a fresh sampled update.

    python -m aurelia_nn.rl3.parity --weights W.safetensors --actor-bin ai-exam --kept kept.jsonl --lengths rows.jsonl --out DIR [--seed N]
"""
import argparse
import json
import os
import shutil
import time

import mlx.core as mx

from aurelia_nn.army import train_m as TM
from aurelia_nn.army.export import export

from . import learner, scenes
from .loop import run_actor, wait_actor
from .net import load_any


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--weights", required=True)
    ap.add_argument("--actor-bin", required=True)
    ap.add_argument("--kept", required=True)
    ap.add_argument("--lengths", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--P", type=int, default=96)
    ap.add_argument("--G", type=int, default=8)
    ap.add_argument("--K", type=int, default=6)
    ap.add_argument("--seed", type=int, default=11)
    ap.add_argument("--jobs", type=int, default=3)
    ap.add_argument("--window", type=int, default=0)
    ap.add_argument("--reuse", action="store_true", help="the record in OUT is kept")
    a = ap.parse_args(argv)
    a.val_to, a.val_from = 60, 0
    pool = scenes.Pool(a.kept, a.lengths)
    model = load_any(a.weights)
    os.makedirs(a.out, exist_ok=True)
    if not a.reuse:
        from aurelia_nn.tacsim import armyrl
        n_of, _ = armyrl.allocate(a.P, pool.families, {f: pool.start_rate(f) for f in pool.families}, {f: ("bar", 0.9, 0.9) for f in pool.families}, 1, 4.0)
        sel = scenes.draw(pool, n_of, a.K, a.G, a.seed, a.out)
        export(model, os.path.join(a.out, "army-net.bin"))
        p, t = run_actor(a, os.path.join(a.out, "army-net.bin"), a.out, sorted({k["family"] for _, k, _ in sel}), a.jobs)
        secs, rc = wait_actor(p, t, 30)
        print(f"actor: {len(sel)} scenes, {secs:.1f}s, rc {rc}", flush=True)
    t = time.time()
    stream = TM.Stream([os.path.join(a.out, "rec.bin")], 1, {}, None)
    print(f"indexed {len(stream)} plays, {stream.decisions} decisions in {time.time() - t:.1f}s", flush=True)
    bt = learner.Batch(model, stream)
    print("states:", bt.states.source, flush=True)
    res = learner.check(model, bt)
    print("PARITY", json.dumps(res), flush=True)
    for w in ([a.window] if a.window else []):
        t = time.time()
        loss, g, extra = learner.rl_gradient(model, bt, w)
        mx.eval(g)
        print(f"BPTT window {w}: loss {loss:.4f} drift {extra['memory_drift']:.2e} {time.time() - t:.2f}s peak {mx.get_peak_memory() / 2**30:.2f} GB", flush=True)


if __name__ == "__main__":
    main()
