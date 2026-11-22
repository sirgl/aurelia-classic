"""Where a step's time goes (GPU): each part of `Sim.step` compiled and timed alone at one batch size.

    python -m aurelia_nn.tacsim.profile DUMP.jsonl --batch 10000
"""
import argparse
import json
import time

import mlx.core as mx

from . import bench
from .sim import A_UNIT, V_NONE, V_MOVE, V_ATTACK, g1

I32 = mx.int32


def timeit(f, *args, n=10):
    out = f(*args)
    mx.eval(out)
    t = time.perf_counter()
    for _ in range(n):
        out = f(*args)
        mx.eval(out)
    return 1000 * (time.perf_counter() - t) / n


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--batch", type=int, default=10000)
    o = ap.parse_args(argv)
    sim, s, tb, _ = bench.replicate(o.dump, o.batch)
    B = o.batch
    side = mx.zeros((B,), dtype=I32)
    slot = bench.kth_unit(s, side, mx.array(0, dtype=I32))
    key = mx.random.key(0)
    rows = {}
    rows["occupancy"] = timeit(mx.compile(lambda s: sim.occupancy(s)), s)
    rows["controls_and_blocks (sight at unit tiles, ZOC)"] = timeit(mx.compile(lambda s: sim.controls_and_blocks(s, side)[:2]), s)
    def reach(s):
        block, ctrl = sim.controls_and_blocks(s, side)[:2]
        out = sim.reach(s, slot, block, ctrl)
        sim._edges = None
        return out
    rows["controls + reach wavefront"] = timeit(reach, s)
    masks = mx.compile(lambda s: {k: v for k, v in sim.masks(s, side, slot).items() if k != "edges"})
    rows["masks (all options)"] = timeit(masks, s)
    def step_with(verb):
        def f(s):
            mk = sim.masks(s, side, slot)
            vb, tg = bench.decide(sim, s, slot, mk, "random", None, key)
            a = {"typ": mx.full((B,), A_UNIT, dtype=I32), "side": side, "slot": slot, "verb": vb if verb is None else mx.full((B,), verb, dtype=I32), "tgt": tg}
            return sim.step(s, a, mk, kinds=(A_UNIT,))
        return mx.compile(f)
    rows["masks + decide + step (random action)"] = timeit(step_with(None), s)
    rows["masks + step (verb none)"] = timeit(step_with(V_NONE), s)
    rows["attack branch alone"] = timeit(mx.compile(lambda s: sim.attack(dict(s), side, slot, g1(s["u_tile"], slot), mx.ones((B,), dtype=mx.bool_))), s)
    rows["end_turn"] = timeit(mx.compile(lambda s: sim.end_turn(dict(s), side, mx.ones((B,), dtype=mx.bool_))), s)
    print(json.dumps({"batch": B, "ms": {k: round(v, 2) for k, v in rows.items()}}, indent=1))
    round_trip(o.dump)


def round_trip(dump, batches=(1000, 10000)):
    """The per-turn GPU <-> CPU round trip: sync, copy the dynamic state to host bytes and back."""
    out = []
    for B in batches:
        sim, s, tb, _ = bench.replicate(dump, B)
        dyn = {k: v for k, v in s.items() if k.startswith(("u_", "c_")) or k in ("owner", "rng")}
        nbytes = sum(v.nbytes for v in dyn.values())
        f = mx.compile(lambda x: {k: v + 0 for k, v in x.items()})
        mx.eval(f(dyn))
        t = time.perf_counter()
        for _ in range(20):
            mx.eval(f(dyn))
        launch = (time.perf_counter() - t) / 20
        t = time.perf_counter()
        for _ in range(20):
            host = {k: bytes(memoryview(v)) for k, v in dyn.items()}
            back = {k: mx.array(memoryview(bytearray(host[k])).cast("B")) for k in host}
            mx.eval(back)
        trip = (time.perf_counter() - t) / 20
        out.append({"batch": B, "state_mb": round(nbytes / 2**20, 1), "launch_ms": round(1000 * launch, 2), "copy_out_and_in_ms": round(1000 * trip, 2)})
    print(json.dumps(out))
    return out


if __name__ == "__main__":
    main()
