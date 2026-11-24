"""The time of a round by parts, with the kernels and the host syncs of each (line 7 of the plan).

    python -m aurelia_nn.tacsim.armyparts DUMP.jsonl --weights W --limit 400 --copies 25 --rounds 3 --mode time|count|wall --out R.json

Modes: `wall` (the play as it runs), `time` (every part fenced by an evaluation at its entry and exit: inclusive and exclusive
seconds), `count` (the same fences plus the host syncs the code itself makes, `.item()`, `.tolist()`, `bool()`, `int()`,
`float()`, `mx.eval`, and the graph's non-view nodes = kernels, per part). The method is the performance model's
(`armyperf` of the perf/model branch): time ~ nodes x 0.42 us + kernels x 2.3 us + host syncs x 108 us + FLOP / speed +
bytes / bandwidth. The parts are the ones of a round: our turn (observation, net, apply), our turn's end (civilians rule,
rule), the enemy's turn (each family's script, the rule), the third seat, the camp / neutral passes, the judges, the economy
schedule. Also `syncs_of(fn)`: the count of host syncs a call makes (the guard tests use it).
"""

import argparse
import collections
import io
import json
import re
import time
import types

import mlx.core as mx
from mlx.utils import tree_flatten

VIEWS = {"Broadcast", "Reshape", "Slice", "ExpandDims", "Squeeze", "Transpose", "AsStrided", "StopGradient", "Depends", "Split",
         "BroadcastAxes", "Unflatten", "Flatten"}
NODE_RE = re.compile(r'\[label ="([A-Za-z0-9_]+)', re.M)
SYNC_NAMES = ("item", "tolist", "__bool__", "__int__", "__float__")


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


class SyncCounter:
    """Counts the host syncs (`.item()`, `.tolist()`, `bool()`, `int()`, `float()` of an array, `mx.eval`) while installed."""

    def __init__(self, on_sync=None):
        self.n = collections.Counter()
        self.on_sync = on_sync
        self.orig = {}
        self.inside = False

    def __enter__(self):
        me = self
        for name in SYNC_NAMES:
            f = getattr(mx.array, name)
            self.orig[name] = f

            def g(self_, *a, _f=f, _n=name, **k):
                me.hit(_n.strip("_"), [self_])
                return _f(self_, *a, **k)
            setattr(mx.array, name, g)
        ev = mx.eval
        self.orig["eval"] = ev

        def eval_(*a, **k):
            me.hit("eval", arrays_of(*a))
            return ev(*a, **k)
        mx.eval = eval_
        return self

    def __exit__(self, *e):
        for name in SYNC_NAMES:
            setattr(mx.array, name, self.orig[name])
        mx.eval = self.orig["eval"]

    def hit(self, kind, arrs):
        if self.inside:
            return
        self.n[kind] += 1
        self.n["all"] += 1
        if self.on_sync:
            self.on_sync(kind, arrs)


def syncs_of(fn, *a, **k):
    """(result, the number of host syncs `fn(*a, **k)` makes)."""
    with SyncCounter() as c:
        r = fn(*a, **k)
    return r, c.n["all"]


class Probe:
    def __init__(self, game, mode):
        self.game, self.mode = game, mode
        self.stack = []
        self.st = collections.defaultdict(collections.Counter)
        self.counter = None
        self.done = set()
        self.keep = []

    def nodes(self, arrs):
        if not arrs:
            return
        buf = io.StringIO()
        mx.export_to_dot(buf, *arrs)
        names = NODE_RE.findall(buf.getvalue())
        c = self.st[tuple(self.stack)]
        c["nodes"] += len(names)
        c["kernels"] += sum(1 for n in names if n not in VIEWS)

    def fence(self, *objs):
        arrs = arrays_of(*objs)
        if self.mode == "count":
            self.nodes(arrs)
        if self.counter:
            self.counter.inside = True
        mx.eval(arrs)
        if self.counter:
            self.counter.inside = False

    def on_sync(self, kind, arrs):
        c = self.st[tuple(self.stack)]
        c["syncs"] += 1
        self.nodes(arrs)

    def wrap(self, owner, name, label):
        if isinstance(owner, types.ModuleType) or owner is getattr(self.game, "brain", None):
            # a module's function or the net, shared by the games of a batch: wrapped once (the owner is kept, so its id stays)
            if (id(owner), name) in self.done:
                return
            self.done.add((id(owner), name))
            self.keep.append(owner)
        f = getattr(owner, name)
        probe = self

        def g(*a, **k):
            if probe.mode != "wall":
                probe.fence(a, k, probe.game.s)
            probe.stack.append(label)
            c = probe.st[tuple(probe.stack)]
            c["calls"] += 1
            t = time.perf_counter()
            r = f(*a, **k)
            if probe.mode != "wall":
                probe.fence(r, probe.game.s)
            c["s"] += time.perf_counter() - t
            probe.stack.pop()
            return r
        setattr(owner, name, g)


# (module attribute, label): the parts of a round. A part inside another is charged to itself (exclusive) and to the outer
# one (inclusive). The label's first word is the part's group in the table.
def instrument(g, probe):
    from . import armycamp, armyciv, armyfog, armyparty, armyprice, armyrule
    w = probe.wrap
    # the economy schedule (host)
    w(g, "schedule", "economy: schedule")
    w(g, "schedule_step", "economy: schedule_step")
    w(g, "turn_end_economy", "economy: turn_end_economy")
    w(armyparty, "keep_world", "third: keep_world")
    # our turn
    w(g, "learner_turn", "our turn")
    w(g, "decide", "our turn: decision step")
    w(g.obs, "tokens", "our turn: observation (tokens)")
    w(g.brain, "encode", "our turn: net (map encode)")
    w(g, "body_fn", "our turn: net (body)")
    w(g.brain, "pointer", "our turn: net (pointer)")
    w(g, "cand_fn", "our turn: candidates")
    w(g.brain, "actions", "our turn: net (action head)")
    w(g, "apply", "our turn: apply")
    w(g, "_compact", "our turn: row compaction")
    w(g, "_enter_rows", "our turn: row compaction")
    w(g, "_leave_rows", "our turn: row compaction")
    w(g.brain, "step", "our turn: net (memory step)")
    w(g, "our_turn_end", "our turn end")
    w(armyciv, "learner_pass", "our turn end: civilians rule (armyciv)")
    w(armycamp, "mark", "our turn end: marks (camp)")
    w(armyparty, "mark", "our turn end: marks (party)")
    # the enemy's turn
    w(g, "enemy_turn", "enemy turn")
    w(armyprice, "land_reinforcements", "enemy: reinforcements (armyprice)")
    w(armyrule, "enemy_turn", "enemy: rule enemy_turn")
    w(armyrule, "field_turn", "enemy: field script (assault)")
    w(armyfog, "enemy_turn", "enemy: fog script")
    w(armyciv, "enemy_turn", "enemy: civilians script (armyciv)")
    w(armycamp, "enemy_turn", "enemy: camp script (armycamp)")
    w(armyparty, "enemy_turn", "enemy: party script (armyparty)")
    w(armycamp, "barbarian_end", "enemy: barbarian end (armycamp)")
    # the scene rule, every seat
    w(armyrule, "seat_turn", "rule: seat turn")
    w(armyrule, "promotions", "rule: promotions")
    w(armyrule, "city_strikes", "rule: city strikes")
    w(armyrule, "shots", "rule: shots")
    w(armyrule, "stand_fortify", "rule: stand + fortify")
    # the third seat and the camp / neutral passes
    w(g, "third_camp", "third: camp (before the enemy)")
    w(armycamp, "neutral_turn", "third: camp neutral turn")
    w(armycamp, "neutral_rule", "third: camp neutral rule")
    w(g, "third_turn", "third: turn (after the enemy)")
    w(armyparty, "third_turn", "third: party third_turn")
    w(g.sim, "end_turn", "sim: end turn")
    w(g.sim, "round_end", "sim: round end")
    w(g.sim, "masks", "sim: masks")
    w(g.obs, "preview", "sim: previews")
    # the judges
    w(g, "judge", "judges")
    w(armyfog, "judge", "judges: fog")
    w(armycamp, "judge", "judges: camp")
    w(armyparty, "judge", "judges: party")
    w(armyciv, "judge", "judges: civilians")
    w(armyprice, "assault_verdict", "judges: assault")


def summarise(probe, wall):
    st = probe.st
    paths = {p: c for p, c in st.items() if p}
    excl = {}
    for p, c in paths.items():
        child = sum(cc["s"] for q, cc in paths.items() if len(q) == len(p) + 1 and q[:len(p)] == p)
        excl[p] = c["s"] - child
    by = collections.defaultdict(collections.Counter)
    for p, c in paths.items():
        b = by[p[-1]]
        b["exclusive s"] += excl[p]
        b["inclusive s"] += c["s"] if p.count(p[-1]) == 1 else 0
        for k, v in c.items():
            if k != "s":
                b[k] += v
    top = sum(c["s"] for p, c in paths.items() if len(p) == 1)
    return {"wall_s": round(wall, 3), "staged_s": round(top, 3), "loop_rest_s": round(wall - top, 3),
            "loop_rest": dict(st.get((), {})),
            "by_part": {k: {kk: (round(vv, 4) if isinstance(vv, float) else vv) for kk, vv in v.items()} for k, v in sorted(by.items())}}


def table(res, rounds=3):
    """Markdown rows: per part, per round: exclusive seconds, calls, kernels, syncs."""
    rows = ["| part | s / round | % | calls | kernels / round | syncs / round |", "|---|---|---|---|---|---|"]
    tot = sum(v["exclusive s"] for v in res["by_part"].values()) + res["loop_rest_s"]
    for k, v in sorted(res["by_part"].items(), key=lambda kv: -kv[1]["exclusive s"]):
        rows.append(f"| {k} | {v['exclusive s'] / rounds:.3f} | {100 * v['exclusive s'] / tot:.1f} | {v.get('calls', 0)} | "
                    f"{v.get('kernels', 0) // rounds} | {v.get('syncs', 0) // rounds} |")
    rows.append(f"| (loop rest) | {res['loop_rest_s'] / rounds:.3f} | {100 * res['loop_rest_s'] / tot:.1f} | | {res['loop_rest'].get('kernels', 0) // rounds} | {res['loop_rest'].get('syncs', 0) // rounds} |")
    return "\n".join(rows)


def main(argv=None):
    from . import armyobs as A
    from . import armyplay as P
    from .armyloop import Brain, load_net
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--weights", required=True)
    ap.add_argument("--limit", type=int, default=400)
    ap.add_argument("--copies", type=int, default=25)
    ap.add_argument("--rounds", type=int, default=3)
    ap.add_argument("--families")
    ap.add_argument("--mode", choices=["wall", "time", "count"], default="time")
    ap.add_argument("--facts", choices=["auto", "on", "off"], default="auto")
    ap.add_argument("--cpu", action="store_true")
    ap.add_argument("--split", choices=["auto", "on", "off"], default="off", help="one game per family group (`armyplay.Batch`)")
    ap.add_argument("--out")
    a = ap.parse_args(argv)
    if a.cpu:
        mx.set_default_device(mx.cpu)
    mx.set_memory_limit(16 << 30)
    mx.set_cache_limit(2 << 30)
    brain = Brain(load_net(a.weights))
    plays = A.load(a.dump, limit=a.limit, families=a.families.split(",") if a.families else None)
    mx.reset_peak_memory()
    shared = collections.defaultdict(collections.Counter)
    ctr = SyncCounter()
    probe = Probe(None, a.mode)
    probe.st = shared
    probe.counter = ctr
    ctr.on_sync = probe.on_sync if a.mode == "count" else None

    def setup(g):
        g.fort_rule = "old"
        g.scene_rule = True
        probe.game = g
        if a.mode != "wall":
            instrument(g, probe)
        mx.eval(g.s)
    g = P.Batch(plays, a.copies, brain, split=a.split, setup=setup, facts=None if a.facts == "auto" else a.facts == "on")
    t0 = time.time()
    with ctr:
        v = g.play(a.rounds)
    mx.eval(v)
    wall = g.seconds
    run = {"scenes": g.B, "rounds": a.rounds, "mode": a.mode, "wall_s": round(wall, 3), "scene_rounds": g.counts["scene-rounds"],
           "scene_rounds_per_s": round(g.counts["scene-rounds"] / wall, 1), "peak_gb": round(mx.get_peak_memory() / 1e9, 2),
           "groups": {n: len(ids) for n, ids in g.groups}, "split": a.split, "wall_with_builds_s": round(time.time() - t0, 2),
           "syncs_total": dict(ctr.n), "verdict_sum": int(mx.sum(v).item()), "families": a.families}
    if a.mode != "wall":
        run.update(summarise(probe, wall))
    run["verdict_wins"] = int(mx.sum(g.verdict == 1).item())
    print(json.dumps({k: run[k] for k in ("scenes", "mode", "wall_s", "scene_rounds_per_s", "peak_gb", "syncs_total")}), flush=True)
    if a.mode != "wall":
        print(table(run, a.rounds))
    if a.out:
        json.dump(run, open(a.out, "w"), indent=1)


if __name__ == "__main__":
    main()
