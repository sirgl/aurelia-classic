"""The performance model of the twin's own play (`armyplay.Game`): where a scene-round's time goes, the counters of
each stage, and the machine's constants the model is built from.

    python -m aurelia_nn.tacsim.armyperf constants --out C.json
    python -m aurelia_nn.tacsim.armyperf play DUMP.jsonl --weights W --limit 200 --copies 50 --rounds 3 --mode wall|time|count --out R.json

Modes of `play`:
- wall: the play as it runs (no added evaluation): the throughput;
- time: every stage fenced (its inputs evaluated at its entry, charged to the caller; its result and the game's state at
  its exit): inclusive and exclusive seconds per stage path;
- count: the same fences, plus per stage the host syncs the code itself makes (`.item()`, `.tolist()`, `bool()`,
  `int()`, `float()`, `mx.eval`) and the graph nodes each evaluation runs (`mx.export_to_dot` before it: the
  primitives, the view ones apart), and the rows x tokens the net's parts see (the analytic FLOPs).

The constants (`constants`): the cost of a host sync, of a graph node (the host's build and the GPU's dispatch) by
the size of its output, the memory bandwidth, the peak of a large matmul and of the map encoder's convolution.
"""

import argparse
import collections
import io
import json
import os
import re
import time

import mlx.core as mx
from mlx.utils import tree_flatten

# Primitives that make no kernel on the GPU (a view of their input; Reshape / Slice copy only when not contiguous).
VIEWS = {"Broadcast", "Reshape", "Slice", "ExpandDims", "Squeeze", "Transpose", "AsStrided", "StopGradient", "Depends", "Split",
         "BroadcastAxes", "Unflatten", "Flatten"}
NODE_RE = re.compile(r'\[label ="([A-Za-z0-9_]+)', re.M)


def arrays_of(*objs):
    out = []
    for o in objs:
        if o is None:
            continue
        if isinstance(o, mx.array):
            out.append(o)
        elif isinstance(o, (dict, list, tuple)):
            out.extend(v for _, v in tree_flatten(o) if isinstance(v, mx.array))
    return out


class Probe:
    """The stage stack, its fences and its counters."""

    def __init__(self, game, mode):
        self.game, self.mode = game, mode
        self.stack = []
        self.st = collections.defaultdict(collections.Counter)
        self.inside = False  # our own fence evaluations: not the code's syncs
        self.orig = {}

    # -- the counters of a sync
    def nodes(self, arrs):
        if not arrs:
            return
        buf = io.StringIO()
        mx.export_to_dot(buf, *arrs)
        names = NODE_RE.findall(buf.getvalue())
        c = self.st[tuple(self.stack)]
        c["nodes"] += len(names)
        c["view nodes"] += sum(1 for n in names if n in VIEWS)
        for n in names:
            if n not in VIEWS:
                self.st[("<prims>",)][n] += 1

    def fence(self, *objs):
        arrs = arrays_of(*objs)
        if self.mode == "count":
            self.nodes(arrs)
        self.inside = True
        mx.eval(arrs)
        self.inside = False

    def sync(self, kind, arrs):
        if self.inside:
            return
        c = self.st[tuple(self.stack)]
        c["syncs"] += 1
        c["sync " + kind] += 1
        self.nodes(arrs)

    def install_sync_counters(self):
        probe = self
        A = mx.array
        for name in ("item", "tolist", "__bool__", "__int__", "__float__"):
            f = getattr(A, name)
            self.orig[name] = f

            def g(self_, *a, _f=f, _n=name, **k):
                probe.sync(_n.strip("_"), [self_])
                return _f(self_, *a, **k)
            setattr(A, name, g)
        ev = mx.eval
        self.orig["eval"] = ev

        def eval_(*a, **k):
            probe.sync("eval", arrays_of(*a))
            return ev(*a, **k)
        mx.eval = eval_

    # -- a stage
    def wrap(self, owner, name, label, shape_of=None):
        f = getattr(owner, name)
        probe = self

        def g(*a, **k):
            if probe.mode != "wall":
                probe.fence(a, k, probe.game.s)
            probe.stack.append(label)
            c = probe.st[tuple(probe.stack)]
            c["calls"] += 1
            if shape_of is not None:
                for kk, v in shape_of(a, k).items():
                    c[kk] += v
            t = time.perf_counter()
            r = f(*a, **k)
            if probe.mode != "wall":
                probe.fence(r, probe.game.s)
            c["s"] += time.perf_counter() - t
            probe.stack.pop()
            return r
        setattr(owner, name, g)


def rows_tokens(a, k):
    tok = a[0]
    b, t = tok["feat"].shape[:2]
    return {"rows": b, "rows x tokens": b * t, "rows x tokens^2": b * t * t}


def rows_cands(a, k):
    cand = a[2]
    b, c = cand["feat"].shape[:2]
    return {"rows": b, "rows x cands": b * c}


def rows_map(a, k):
    return {"rows": a[1]["feat"].shape[0]}


def instrument(g, probe):
    """The stages of a scene-round (see the module's doc)."""
    from . import armyfog, armyprice, armyrule
    w = probe.wrap
    w(g, "schedule", "schedule (host)")
    w(g, "schedule_step", "schedule_step (host)")
    w(g, "learner_turn", "learner turn")
    w(g, "decide", "decision step")
    w(g.obs, "tokens", "tokens")
    w(g, "body_fn", "body", rows_tokens)
    w(g.brain, "encode", "map encode", rows_map)
    w(g.brain, "pointer", "pointer")
    w(g, "cand_fn", "candidates")
    w(g.brain, "actions", "action head", rows_cands)
    w(g, "apply", "apply")
    w(g, "_compact", "compaction")
    w(g, "_enter_rows", "compaction")
    w(g, "_leave_rows", "compaction")
    w(g, "our_turn_end", "our turn end (stand + rule)")
    w(g.brain, "step", "memory step")
    w(g, "judge", "judge")
    w(g, "enemy_turn", "enemy turn")
    w(armyprice, "land_reinforcements", "reinforcements")
    w(armyrule, "field_turn", "field script")
    w(armyfog, "enemy_turn", "fog script")
    w(armyrule, "promotions", "rule: promotions")
    w(armyrule, "city_strikes", "rule: city strikes")
    w(armyrule, "shots", "rule: shots")
    w(armyrule, "stand_fortify", "rule: stand + fortify")
    w(g.sim, "end_turn", "end turn")
    w(g.sim, "round_end", "round end")
    w(g.sim, "masks", "masks")
    w(g.obs, "preview", "previews")


def summarise(probe, wall):
    """Per stage path: inclusive and exclusive seconds, the counters; and per leaf name the exclusive sums."""
    st = probe.st
    paths = {p: c for p, c in st.items() if p and p[0] != "<prims>"}
    excl = {}
    for p, c in paths.items():
        child = sum(cc["s"] for q, cc in paths.items() if len(q) == len(p) + 1 and q[:len(p)] == p)
        excl[p] = c["s"] - child
    top = sum(c["s"] for p, c in paths.items() if len(p) == 1)
    root = st.get((), collections.Counter())
    by_leaf = collections.defaultdict(collections.Counter)
    for p, c in paths.items():
        b = by_leaf[p[-1]]
        b["exclusive s"] += excl[p]
        for k, v in c.items():
            if k != "s":
                b[k] += v
    rows = [{"path": " / ".join(p), "s": round(c["s"], 4), "exclusive_s": round(excl[p], 4), **{k: v for k, v in c.items() if k != "s"}}
            for p, c in sorted(paths.items())]
    return {"wall_s": round(wall, 3), "staged_s": round(top, 3), "loop_rest_s": round(wall - top, 3),
            "loop_rest_counts": dict(root), "by_stage": {k: {kk: (round(vv, 4) if isinstance(vv, float) else vv) for kk, vv in v.items()} for k, v in sorted(by_leaf.items())},
            "paths": rows, "primitives": dict(st.get(("<prims>",), collections.Counter()).most_common(60))}


def machine():
    """The load of the machine while we measure (other lanes run)."""
    import subprocess
    la = os.getloadavg()
    try:
        # The GPU lock file of the machine's job broker, named by AURELIA_GPU_LOCK; "?" when none is named.
        lock = os.environ.get("AURELIA_GPU_LOCK", "")
        gpu = subprocess.run(["lsof", lock], capture_output=True, text=True).stdout.count("\n") if lock else "?"
    except Exception:
        gpu = "?"
    return {"loadavg": [round(x, 2) for x in la], "gpu_lock_lines": gpu, "time": time.strftime("%Y-%m-%d %H:%M:%S")}


def digest(s):
    """A hash of every array of the state (its key, dtype, shape and bytes)."""
    import hashlib
    h = hashlib.sha256()
    for k in sorted(s):
        v = s[k]
        if isinstance(v, mx.array):
            h.update(k.encode())
            h.update(str((v.dtype, v.shape)).encode())
            w = v.astype(mx.uint8) if v.dtype == mx.bool_ else v
            mx.eval(w)
            h.update(bytes(memoryview(w)))
    return h.hexdigest()[:16]


def install_digest(g, out):
    """After each round's end, the digest of the state (and of the verdicts so far)."""
    f = g.sim.round_end

    def round_end(s, take):
        r = f(s, take)
        out.append(digest({**r, "_verdict": g.verdict}))
        return r
    g.sim.round_end = round_end


def play(a):
    from . import armyobs as A
    from . import armyplay as P
    from .armyloop import Brain, load_net
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    brain = Brain(load_net(a.weights))
    t = time.time()
    plays = A.load(a.dump, limit=a.limit, families=a.families.split(",") if a.families else None)
    t_load = time.time() - t
    res = {"setup": vars(a), "machine_before": machine(), "load_s": round(t_load, 1), "runs": []}
    for rep in range(a.repeat):
        mx.reset_peak_memory()
        g = P.Game(plays, a.copies, brain, facts=None if a.facts == "auto" else a.facts == "on")
        g.fort_rule = "old"
        g.scene_rule = True
        g.compare = False
        if a.compile:
            g.enable_compile()
        from . import armyfast
        armyfast.install(g, [x for x in a.fast.split(",") if x])
        digests = []
        if a.digest:
            install_digest(g, digests)
        probe = Probe(g, a.mode)
        if a.mode != "wall":
            instrument(g, probe)
        if a.mode == "count":
            probe.install_sync_counters()
        mx.eval(g.s)
        t0 = time.time()
        v = g.play(a.rounds)
        mx.eval(v)
        wall = time.time() - t0
        if a.mode == "count":
            mx.eval = probe.orig["eval"]
            for name in ("item", "tolist", "__bool__", "__int__", "__float__"):
                setattr(mx.array, name, probe.orig[name])
        run = {"scenes": g.B, "rounds": a.rounds, "wall_s": round(wall, 3), "scene_rounds": g.counts["scene-rounds"],
               "scene_rounds_per_s": round(g.counts["scene-rounds"] / wall, 1), "net_decisions": g.counts["net decisions"],
               "peak_gb": round(mx.get_peak_memory() / 1e9, 2), "verdict_sum": int(mx.sum(v).item()),
               "verdicts": v.tolist() if a.digest else None, "digests": digests or None, "counts": dict(g.counts)}
        if a.mode != "wall":
            run.update(summarise(probe, wall))
        res["runs"].append(run)
        print(json.dumps({k: run[k] for k in ("scenes", "wall_s", "scene_rounds_per_s", "peak_gb", "verdict_sum")}), flush=True)
        del g
        mx.clear_cache()
    res["machine_after"] = machine()
    if a.out:
        json.dump(res, open(a.out, "w"), indent=1)


# ---------------------------------------------------------------------------------------------------- the net's parts

def netbench(a):
    """The net's parts alone on the GPU at several batch sizes (the real tokens of round 0, trimmed): the body, the map
    encoder, the action head (20 candidates); median of 5, f32; plain and compiled (`armyfast.compile_net`)."""
    from . import armyobs as A
    from . import armyplay as P
    from . import armyfast
    from .armyloop import Brain, load_net
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    brain = Brain(load_net(a.weights))
    plays = A.load(a.dump, limit=a.limit)
    g = P.Game(plays, a.copies, brain)
    tok = P.trim_tokens(g.tokens(0))
    mx.eval(tok, g.statics)
    out = {"machine_before": machine(), "T": int(tok["feat"].shape[1]), "rows": []}
    for comp in (False, True):
        if comp:
            armyfast.compile_net(g)
        for B in (1000, 3000, 10000):
            if B > g.B:
                continue
            sub = {k: v[:B] for k, v in tok.items()}
            um, pm, price, M = g.um[:B], g.pm[:B], g.price[:B], g.M[:B]
            st = g.statics[:B]
            C = 20
            cand = {"feat": mx.random.normal((B, C, 40)).astype(mx.float16), "promo": mx.zeros((B, C), dtype=mx.int32),
                    "mask": mx.ones((B, C), dtype=mx.bool_), "pos": mx.random.randint(0, 576, (B, C)).astype(mx.int32)}
            h = g.body_fn(sub, um, pm, M, price, None)
            mx.eval(cand, h)
            unit = mx.zeros((B,), dtype=mx.int32)
            row = {"compiled": comp, "B": B,
                   "body_ms": 1000 * timeit(lambda: mx.eval(g.body_fn(sub, um, pm, M, price, None))),
                   "encode_ms": 1000 * timeit(lambda: mx.eval(brain.encode(st, sub))),
                   "action_ms": 1000 * timeit(lambda: mx.eval(brain.actions(h, unit, cand, M)))}
            out["rows"].append({k: (round(v, 3) if isinstance(v, float) else v) for k, v in row.items()})
            print(json.dumps(out["rows"][-1]), flush=True)
    if a.out:
        json.dump(out, open(a.out, "w"), indent=1)


# ---------------------------------------------------------------------------------------------------- one RL update

def rl(a):
    """One update of `armyrl` (P plays x G copies, K sampled turns each), its phases timed: the game's build, the
    play (with the twin's stages when --mode time), the RL gradient over the records, the KL, the anchor, the step."""
    import random
    import mlx.optimizers as optim
    from mlx.utils import tree_map
    from . import armyobs as A
    from . import armyrl as R
    from .armyloop import Brain, load_net
    from .armyplay import Game
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    random.seed(a.seed)
    mx.random.seed(a.seed)
    plays = A.load(a.dump, limit=a.limit, families=R.FAMILIES)
    model = load_net(a.weights)
    brain, ref = Brain(model), Brain(load_net(a.weights))
    anchor = R.Anchor(a.anchor, 32, a.seed, model) if a.anchor else None
    rl_grad = R.make_rl_grad(model, brain)
    opt = optim.Adam(learning_rate=3e-5)
    out = {"setup": vars(a), "machine_before": machine(), "updates": []}
    for u in range(a.updates):
        ph = collections.Counter()
        t = time.perf_counter()
        sub = random.sample(plays, min(a.P, len(plays)))
        P, G = len(sub), a.G
        plan = collections.defaultdict(lambda: [False] * (P * G))
        for pi, p in enumerate(sub):
            L = R.play_length(p)
            for r in random.sample(range(L), min(a.K, L)):
                for k in range(G):
                    plan[r][pi + P * k] = True
        g = Game(sub, G, brain, salt=False)
        R.setup(g)
        from . import armyfast
        armyfast.install(g, [x for x in a.fast.split(",") if x])
        g.sample_plan = {r: mx.array(m) for r, m in plan.items()}
        g.sample_key = mx.random.key(a.seed * 100003 + u)
        mx.eval(g.s)
        ph["game build"] = time.perf_counter() - t
        probe = None
        if a.mode == "time":
            probe = Probe(g, "time")
            instrument(g, probe)
        t = time.perf_counter()
        g.play()
        mx.eval(g.verdict)
        ph["play"] = time.perf_counter() - t
        t = time.perf_counter()
        verdict = g.verdict.tolist()
        adv, Rw, live = R.advantages(verdict, P, G)
        n_live = sum(sum(1 for i in rec["gid"].tolist() if adv[i]) for rec in g.records)
        total = None
        n_rec = 0
        for rec in g.records:
            gid = rec["gid"].tolist()
            w = [adv[i] / max(n_live, 1) for i in gid]
            if not any(w):
                continue
            n_rec += 1
            tok, c, M = R.rec_inputs(rec, g.turn_M)
            l, gr = rl_grad(tok, rec["um"], rec["pm"], rec["price"], M, rec["sel"], rec["pidx"], rec["pick"], mx.array(w, dtype=mx.float32), c, rec.get("aidx"), rec.get("x"))
            total = gr if total is None else tree_map(lambda x, y: x + y, total, gr)
            mx.eval(total, l)
        ph["rl gradient"] = time.perf_counter() - t
        t = time.perf_counter()
        ks = [R.kl_sum(brain, ref, rec, g.turn_M) for rec in g.records]
        ph["kl"] = time.perf_counter() - t
        if anchor:
            t = time.perf_counter()
            la, lm, ga = anchor.grad()
            ph["anchor (train_m step, 32 plays)"] = time.perf_counter() - t
            total = ga if total is None else tree_map(lambda x, y: x + y, total, ga)
        t = time.perf_counter()
        if total is not None:
            total, nrm = optim.clip_grad_norm(total, 1.0)
            opt.update(model, total)
            mx.eval(model.parameters(), opt.state)
        ph["optimizer"] = time.perf_counter() - t
        row = {"update": u, "phases_s": {k: round(v, 3) for k, v in ph.items()}, "total_s": round(sum(ph.values()), 2),
               "records": len(g.records), "records_with_gradient": n_rec, "record_rows": sum(int(r["gid"].shape[0]) for r in g.records),
               "scene_rounds": g.counts["scene-rounds"], "rounds": g.rounds_played, "peak_gb": round(mx.get_peak_memory() / 1e9, 2),
               "verdict_digest": digest({"v": g.verdict}),
               "records_digest": {k: digest({f"{i:05d}": rec[k] for i, rec in enumerate(g.records) if k in rec}) for k in ("gid", "pidx", "pick", "aidx", "sel", "um", "pm")}}
        if probe is not None:
            row.update(summarise(probe, ph["play"]))
        out["updates"].append(row)
        print(json.dumps({k: row[k] for k in ("update", "phases_s", "total_s", "records", "scene_rounds", "rounds")}), flush=True)
        del g
        mx.clear_cache()
    out["machine_after"] = machine()
    if a.out:
        json.dump(out, open(a.out, "w"), indent=1)


# ---------------------------------------------------------------------------------------------------- the constants

def timeit(fn, n=5):
    best = []
    for _ in range(n):
        t = time.perf_counter()
        fn()
        best.append(time.perf_counter() - t)
    best.sort()
    return best[len(best) // 2]


def constants(a):
    mx.set_memory_limit(16 << 30)
    out = {"machine_before": machine(), "device": mx.device_info()}
    # A host sync: a trivial graph evaluated and read back, per round trip.
    x = mx.array([1], dtype=mx.int32)
    mx.eval(x)

    def syncs(n=2000):
        y = x
        for _ in range(n):
            y = y + 1
            y.item()
    out["t_sync_us"] = round(1e6 * timeit(syncs) / 2000, 2)
    # Building a node on the host alone (no evaluation), and a chain of N dependent nodes evaluated once.
    shapes = [(1000, 32), (10000, 32), (1000, 576), (10000, 576), (100000, 32), (10000, 4096)]
    out["node"] = []
    for sh in shapes:
        z = mx.zeros(sh, dtype=mx.int32)
        mx.eval(z)
        N = 2000

        def build():
            y = z
            for _ in range(N):
                y = y + 1
            return y

        def build_eval():
            mx.eval(build())
        tb = timeit(build) / N
        te = timeit(build_eval) / N
        nbytes = z.nbytes
        out["node"].append({"shape": sh, "bytes_out": nbytes, "t_build_us": round(1e6 * tb, 3), "t_node_us": round(1e6 * te, 3),
                            "eff_gbps": round(2 * nbytes / te / 1e9, 1)})
    # A gather along the row (`take_along_axis`, the simulator's g1 / gn): [B, 576] by [B, 32] indices.
    for B in (1000, 10000):
        src = mx.zeros((B, 576), dtype=mx.int32)
        ix = mx.zeros((B, 32), dtype=mx.int32)
        mx.eval(src, ix)

        def gat(N=1000):
            y = ix
            for _ in range(N):
                y = mx.take_along_axis(src, y, axis=1)
            mx.eval(y)
        out[f"t_gather_us_B{B}"] = round(1e6 * timeit(gat) / 1000, 3)
    # The bandwidth: a large copy.
    big = mx.zeros((1 << 26,), dtype=mx.float32)
    mx.eval(big)
    t = timeit(lambda: mx.eval(big + 1.0))
    out["bandwidth_gbps"] = round(2 * big.nbytes / t / 1e9, 1)
    # The compute peak: a large matmul, f32.
    m = mx.random.normal((4096, 4096))
    mx.eval(m)
    t = timeit(lambda: mx.eval(m @ m))
    out["matmul_tflops_f32"] = round(2 * 4096 ** 3 / t / 1e12, 2)
    # Small matmuls as the body runs them: [B*T, 64] x [64, 64].
    for rows in (40_000, 400_000):
        xx = mx.random.normal((rows, 64))
        ww = mx.random.normal((64, 64))
        mx.eval(xx, ww)
        t = timeit(lambda: mx.eval(xx @ ww))
        out[f"matmul_64_rows{rows}_tflops"] = round(2 * rows * 64 * 64 / t / 1e12, 3)
    # The map encoder's convolution: [B, 24, 24, 64] 3x3 64 -> 64.
    for B in (1000, 4000):
        im = mx.random.normal((B, 24, 24, 64))
        wc = mx.random.normal((64, 3, 3, 64))
        mx.eval(im, wc)
        t = timeit(lambda: mx.eval(mx.conv2d(im, wc, padding=1)))
        out[f"conv3x3_B{B}_tflops"] = round(2 * B * 576 * 64 * 9 * 64 / t / 1e12, 2)
        out[f"conv3x3_B{B}_ms"] = round(1000 * t, 2)
    out["machine_after"] = machine()
    print(json.dumps(out, indent=1))
    if a.out:
        json.dump(out, open(a.out, "w"), indent=1)


def main(argv=None):
    ap = argparse.ArgumentParser()
    sp = ap.add_subparsers(dest="cmd", required=True)
    c = sp.add_parser("constants")
    c.add_argument("--out")
    p = sp.add_parser("play")
    p.add_argument("dump")
    p.add_argument("--weights", required=True)
    p.add_argument("--limit", type=int, default=200)
    p.add_argument("--families")
    p.add_argument("--copies", type=int, default=50)
    p.add_argument("--rounds", type=int, default=3)
    p.add_argument("--mode", choices=["wall", "time", "count"], default="wall")
    p.add_argument("--repeat", type=int, default=1)
    p.add_argument("--facts", choices=["auto", "on", "off"], default="off")
    p.add_argument("--compile", action="store_true")
    p.add_argument("--fast", default="", help="comma list of the speed-ups under test (`armyfast.install`)")
    p.add_argument("--digest", action="store_true", help="the parity of a speed-up: every scene's verdict and a hash of the state after each round")
    p.add_argument("--out")
    r = sp.add_parser("rl")
    r.add_argument("dump")
    r.add_argument("--weights", required=True)
    r.add_argument("--anchor")
    r.add_argument("--limit", type=int, default=600)
    r.add_argument("--P", type=int, default=64)
    r.add_argument("--G", type=int, default=8)
    r.add_argument("--K", type=int, default=3)
    r.add_argument("--seed", type=int, default=1)
    r.add_argument("--updates", type=int, default=3)
    r.add_argument("--mode", choices=["wall", "time"], default="wall")
    r.add_argument("--fast", default="")
    r.add_argument("--out")
    nb = sp.add_parser("netbench")
    nb.add_argument("dump")
    nb.add_argument("--weights", required=True)
    nb.add_argument("--limit", type=int, default=200)
    nb.add_argument("--copies", type=int, default=50)
    nb.add_argument("--out")
    for q in (c, p, r, nb):
        q.add_argument("--cpu", action="store_true", help="the CPU device (a smoke test without the GPU)")
    a = ap.parse_args(argv)
    if a.cpu:
        mx.set_default_device(mx.cpu)
    {"constants": constants, "play": play, "rl": rl, "netbench": netbench}[a.cmd](a)


if __name__ == "__main__":
    main()
