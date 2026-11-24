"""Where the time of the net in the loop goes (`armyplay.Game`): each part of a round timed with an evaluation after it.

    python -m aurelia_nn.tacsim.armyprof DUMP.jsonl --weights RUN/weights.safetensors --limit 200 --copies 50 --rounds 3

The parts: the tokens, the body, the pointer, the candidates, the action head, applying the action, the turn's end
(memory step), the enemy's turn, and the host's own share (the rest of the wall time). Timing with an evaluation after
each part removes the overlap of lazy evaluation, so the parts add up to more than an untimed run: the shares matter.
"""

import argparse
import collections
import json
import time

import mlx.core as mx

from . import armyobs as A
from . import armyplay as P
from .armyloop import Brain, load_net


def wrap(obj, name, acc, key):
    f = getattr(obj, name)

    def g(*a, **k):
        t = time.time()
        r = f(*a, **k)
        mx.eval(r)
        acc[key] += time.time() - t
        acc["n " + key] += 1
        return r
    setattr(obj, name, g)


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--weights", required=True)
    ap.add_argument("--limit", type=int, default=200)
    ap.add_argument("--copies", type=int, default=50)
    ap.add_argument("--rounds", type=int, default=3)
    ap.add_argument("--families")
    ap.add_argument("--untimed", action="store_true", help="only the wall time")
    ap.add_argument("--compile", action="store_true")
    ap.add_argument("--legacy", default="", help="parts of the step before the speed-up: nfo (foes counted per call, a host round-trip), mk (the masks twice); comma list")
    ap.add_argument("--out")
    a = ap.parse_args(argv)
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    brain = Brain(load_net(a.weights))
    plays = A.load(a.dump, limit=a.limit, families=a.families.split(",") if a.families else None)
    g = P.Game(plays, a.copies, brain)
    g.fort_rule = "old"
    if a.compile:
        g.enable_compile()
    if "mk" in a.legacy:
        g.legacy = True
    if "nfo" in a.legacy:
        g.obs.nfo = None
    acc = collections.Counter()
    if not a.untimed:
        wrap(g.obs, "tokens", acc, "tokens")
        wrap(g.sim, "masks", acc, "  masks (all callers)")
        wrap(g.obs, "preview", acc, "  previews (all callers)")
        wrap(g.obs, "spot", acc, "  spots (all callers)")
        wrap(g.obs, "field", acc, "  field (all callers)")
        wrap(g, "cand_fn", acc, "candidates")
        wrap(g, "body_fn", acc, "body")
        wrap(brain, "pointer", acc, "pointer")
        wrap(brain, "actions", acc, "actions")
        wrap(brain, "step", acc, "memory step")
        wrap(g, "apply", acc, "apply")
        wrap(g, "enemy_turn", acc, "enemy turn")
    t0 = time.time()
    g.play(a.rounds)
    wall = time.time() - t0
    parts = {k: round(v, 3) for k, v in acc.items() if not k.startswith("n ")}
    timed = sum(v for k, v in acc.items() if not k.startswith("n ") and k != "enemy turn")
    res = {"scenes": g.B, "rounds": a.rounds, "wall_s": round(wall, 2), "scene_rounds": g.counts["scene-rounds"],
           "scene_rounds_per_s": round(g.counts["scene-rounds"] / wall, 1), "net_decisions": g.counts["net decisions"],
           "parts_s": parts, "calls": {k[2:]: v for k, v in acc.items() if k.startswith("n ")},
           "host_and_rest_s": round(wall - timed - acc["enemy turn"], 3) if acc else None,
           "peak_gb": round(mx.get_peak_memory() / 1e9, 2)}
    print(json.dumps(res, indent=1))
    if a.out:
        json.dump(res, open(a.out, "w"), indent=1)


if __name__ == "__main__":
    main()
