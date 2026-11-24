"""The RL gradient through the memory (twin package 3, plan 13.2: "градиент через память в RL"), behind `armyrl --mem-grad W`.

Without it a sampled decision reads the memory (`um`, `pm`) of its turn as a constant: the parameters that WRITE the
memory (the unit and plan GRUs, the plan attention, the hysteresis) get no RL gradient, only the read side does. With
`--mem-grad W` the memory a sampled turn r reads is recomputed from the memory at the start of turn r - W (a constant)
by the W memory steps of the turns between (`Brain.body` then `Brain.step` on each turn's end tokens, as `Game.learner_turn`
ran them), and the gradient of the turn's decisions flows back through those steps into the memory's parameters and the
body's. Truncated back-propagation: the window W is the flag.

The tape: `Game.mem_tape` (None: nothing is kept) keeps, for the turns in `want`, the inputs of the turn's end step
(the end tokens, the streams the body read, the map, the price, the memory and the live rows). The gradient pass of a sampled
turn (`turn_gradient`): the chain once for the turn's rows, each decision record's gradient with respect to the parameters and
to the memory it read (one backward), the cotangents of the memory summed over the records, then one backward through the
chain with those cotangents. The notes / history streams stay inputs (no gradient through their writing), as in `armyrl`.
"""

import mlx.core as mx
import mlx.nn as nn
from mlx.utils import tree_flatten, tree_map

# The parameters that write the memory (`model_m.step`) and the ones that read it (the unit memory's readout).
WRITE = ("gu_wx", "gu_bx", "gu_wh", "gu_bh", "gp_wx", "gp_bx", "gp_wh", "gp_bh", "pq.", "pk.", "hyst")
READ = ("um.",)


class Tape:
    """The inputs of the memory's step at the end of the turns in `want` (`Game.learner_turn` calls `note`)."""

    def __init__(self, want):
        self.want = set(want)
        self.turns = {}

    def note(self, game, rel, tok, x, live):
        if rel not in self.want:
            return
        keys = ("feat", "dist", "mask", "slot") + (("pos",) if game.brain.has_map else ())
        self.turns[rel] = {"tok": {k: tok[k] for k in keys}, "x": x, "um": game.um, "pm": game.pm, "alive": game.alive, "M": game.M,
                           "price": game.price, "live": live}


def wanted_turns(sampled, window):
    """The turns whose end step a sampled turn's memory depends on within the window (`sampled`: the sampled rounds)."""
    return {t for r in sampled for t in range(max(0, r - window), r)}


def _rows(a, rows):
    return mx.take(a, rows, axis=0)


def _rows_tree(tree, rows):
    return tree_map(lambda a: _rows(a, rows), tree)


def unroll(brain, tape, rel, window, rows):
    """The memory (um, pm) the rows read at turn `rel`: from the memory at the start of turn `rel - window` through the
    memory steps of the turns between. `rows` [n]: batch rows."""
    start = max(0, rel - window)
    first = tape.turns[start]
    um, pm = _rows(first["um"], rows), _rows(first["pm"], rows)
    for t in range(start, rel):
        end = tape.turns[t]
        tok = _rows_tree(end["tok"], rows)
        x = None if end["x"] is None else _rows_tree(end["x"], rows)
        h = brain.body(tok, um, pm, _rows(end["M"], rows) if brain.has_map else None, _rows(end["price"], rows), x)
        um2, pm2, _ = brain.step(h, tok, um, pm, _rows(end["alive"], rows))
        live = _rows(end["live"], rows)
        um = mx.where(live[:, None, None], um2, um)
        pm = mx.where(live[:, None, None], pm2, pm)
    return um, pm


def group_norms(grad):
    """The gradient's norms by part: the memory's write side, its read side, everything else (a tree of gradients)."""
    sums = {"write": 0.0, "read": 0.0, "rest": 0.0}
    per = {}
    for name, g in tree_flatten(grad):
        n2 = mx.sum(g * g).item()
        part = "write" if name.startswith(WRITE) else "read" if name.startswith(READ) else "rest"
        sums[part] += n2
        if part != "rest":
            per[name] = n2 ** 0.5
    return {k: round(v ** 0.5, 6) for k, v in sums.items()}, {k: round(v, 6) for k, v in per.items()}


def _value_grad_with_memory(model, fn):
    """`fn(um, pm, *args)` -> loss; returns (loss, the parameters' gradient, d loss / d um, d loss / d pm)."""
    def inner(params, um, pm, *args):
        model.update(params)
        return fn(um, pm, *args)
    vg = mx.value_and_grad(inner, argnums=(0, 1, 2))

    def run(um, pm, *args):
        loss, (gp, gu, gq) = vg(model.trainable_parameters(), um, pm, *args)
        return loss, gp, gu, gq
    return run


def make_turn_gradient(model, brain, logp_parts):
    """The gradient pass of one sampled turn with the memory unrolled (`logp_parts`: `armyrl.logp_parts`)."""

    def record_loss(um, pm, tok, price, M, sel, pidx, pick, w, c, aidx, x):
        lp, la = logp_parts(brain, tok, um, pm, price, M, sel, pidx, pick, c, aidx, x)
        l = mx.take_along_axis(lp, pidx[:, None], axis=1)[:, 0]
        if la is not None:
            l = l + mx.where(pick >= 0, mx.take_along_axis(la, aidx[:, None], axis=1)[:, 0], 0.0)
        return -mx.sum(w * l)

    per_record = _value_grad_with_memory(model, record_loss)

    def through_chain(tape, rel, window, rows, cot_um, cot_pm):
        def surrogate(params):
            model.update(params)
            um, pm = unroll(brain, tape, rel, window, rows)
            return mx.sum(um * cot_um) + mx.sum(pm * cot_pm)
        return mx.grad(surrogate)(model.trainable_parameters())

    def turn_gradient(tape, rel, window, recs, weights, inputs):
        """`recs`: the records of turn `rel`; `weights[i]`: record i's advantage weights [n_i]; `inputs(rec)`: (tok, cand, M).
        Returns (loss, gradient, the largest |memory recomputed - memory recorded|)."""
        rows_list = sorted({i for rec in recs for i in rec["gid"].tolist()})
        rows = mx.array(rows_list, dtype=mx.int32)
        pos = mx.zeros((max(rows_list) + 1,), dtype=mx.int32).at[rows].add(mx.arange(len(rows_list), dtype=mx.int32))
        um_c, pm_c = unroll(brain, tape, rel, window, rows)
        total, loss = None, 0.0
        cot_um, cot_pm = mx.zeros_like(um_c), mx.zeros_like(pm_c)
        drift = 0.0
        for rec, w in zip(recs, weights):
            at = mx.take(pos, rec["gid"])
            um, pm = mx.take(um_c, at, axis=0), mx.take(pm_c, at, axis=0)
            drift = max(drift, mx.max(mx.abs(um - rec["um"])).item(), mx.max(mx.abs(pm - rec["pm"])).item())
            tok, c, M = inputs(rec)
            l, gp, gu, gq = per_record(um, pm, tok, rec["price"], M, rec["sel"], rec["pidx"], rec["pick"], w, c, rec.get("aidx"), rec.get("x"))
            cot_um = cot_um.at[at].add(gu)
            cot_pm = cot_pm.at[at].add(gq)
            total = gp if total is None else tree_map(lambda a, b: a + b, total, gp)
            mx.eval(total, cot_um, cot_pm, l)
            loss += l.item()
        through = through_chain(tape, rel, window, rows, cot_um, cot_pm)
        total = tree_map(lambda a, b: a + b, total, through)
        mx.eval(total)
        return loss, total, drift

    return turn_gradient
