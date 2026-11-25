"""Speed-ups of the twin's own play (`armyplay.Game`) found by the performance model (`armyperf`). Each one is
switched on per game (`install(game, names)`), keeps every result bit-identical, and is checked by
`armyperf play --digest` (the state after each round and every verdict, equal with and without it).

- `bucket`: the compacted decision step's rows and the packed previews' pairs padded to a bucket size (1, 2, 3, 4,
  6, 8, 12, ...: at most 1.5x the rows), with inactive rows / invalid pairs, so the compiled parts see few shapes.
  The rows are independent and the padding's results are dropped: exact.
- `masks`, `preview`: `Sim.masks` and `Obs.preview` run as compiled functions (`mx.compile`). They make no host
  read and their shapes follow their inputs, so a trace is replayed for every call of the same shapes: the host
  does not build their 1 400 / 4 400 graph nodes again, and their element-wise chains fuse. The arrays of the batch's
  rows that compaction swaps (`Sim.cs`, `Sim.tid`, `Sim.n_pieces`, `Obs.t`) are inputs of the compiled function,
  not constants of its trace.
- `branchless`: the unit action's move and strike, and the twin's pillage, run masked in every call instead of being
  skipped after a host read where no row needs them; the tokens' promotion check runs over every slot at once
  (`Sim.promo_mask_all`) instead of over the slots a host read finds ready. No host read is left in `tokens` or
  `Sim.apply_unit`, so they can be compiled too:
- `tokens`, `apply`: `Obs.tokens` and `Sim.apply_unit` compiled as above (they need `branchless`).
- `net`: the net's parts in the loop (the body, the map encoder, the action head) compiled, the model's parameters an
  input (`inputs=[model.state]`: an RL step's new weights are read). Their element-wise chains (the encoder's bias,
  ReLU and residual over [B, 576, 64]; the body's masks and biases) fuse; the matmuls and convolutions stay.
"""

import mlx.core as mx

from . import armyobs as A


def bucket(n, cap):
    """The smallest of 2^k and 3 x 2^(k-1) that is at least n, at most `cap`."""
    b = 1
    while b < n:
        b = b * 3 // 2 if b >= 2 and (b & (b - 1)) == 0 else (b * 4 // 3 if b >= 3 else b + 1)
    return min(b, cap)


class RowCompiled:
    """`fn(*args, **kw)` compiled, with the simulator's and the observer's row arrays as inputs: inside the trace
    the simulator and the observer read the traced copies, then get their own arrays back."""

    def __init__(self, sim, obs, fn):
        self.sim, self.obs, self.fn = sim, obs, fn
        self.compiled = mx.compile(self.inner)

    def inner(self, rows, *args, **kw):
        sim, obs = self.sim, self.obs
        saved = (sim.cs, sim.tid, sim.n_pieces, obs.tab, obs.t)
        tab = A.Tables.__new__(A.Tables)
        tab.k = obs.tab.k
        tab.t = rows["t"]
        sim.cs, sim.tid, sim.n_pieces = rows["cs"], rows["tid"], rows["n_pieces"]
        obs.tab, obs.t = tab, tab.t
        try:
            return self.fn(*args, **kw)
        finally:
            sim.cs, sim.tid, sim.n_pieces, obs.tab, obs.t = saved

    def __call__(self, *args, **kw):
        sim, obs = self.sim, self.obs
        rows = {"cs": sim.cs, "tid": sim.tid, "n_pieces": sim.n_pieces, "t": obs.t}
        return self.compiled(rows, *args, **kw)


def compile_net(game):
    from .armyloop import Brain
    b = game.brain
    m = b.m
    body = mx.compile(lambda tok, um, pm, M, price, x: Brain.body(b, tok, um, pm, M, price, x), inputs=[m.state])
    game.body_fn = lambda tok, um, pm, M, price, x=None: body(tok, um, pm, M, price, x)
    enc = mx.compile(lambda st, feat, pos: m.encode_turn(st, feat, pos), inputs=[m.state])
    b.encode = lambda statics, tok: enc(statics, tok["feat"], tok["pos"])
    keys = ("feat", "promo", "mask", "pos")
    act = mx.compile(lambda h, unit, cand, M: Brain.actions(b, h, unit, cand, M), inputs=[m.state])
    b.actions = lambda h, unit, cand, M: act(h, unit, {k: cand[k] for k in keys if k in cand}, M)


def install(game, names):
    """Switch on the speed-ups `names` (an iterable of the names above) on `game`."""
    sim, obs = game.sim, game.obs
    for name in names:
        if name == "bucket":
            game.row_bucket = bucket
            obs.pack_bucket = bucket
        elif name == "branchless":
            sim.branchless = True
        elif name == "tokens":
            assert sim.branchless, "tokens needs branchless (no host read inside)"
            obs.tokens = RowCompiled(sim, obs, obs.tokens)
        elif name == "apply":
            assert sim.branchless and sim.events is None, "apply needs branchless and no event counters"
            sim.apply_unit = RowCompiled(sim, obs, sim.apply_unit)
        elif name == "net":
            compile_net(game)
        elif name == "masks":
            sim.masks = RowCompiled(sim, obs, sim.masks)
        elif name == "preview":
            obs.preview = RowCompiled(sim, obs, obs.preview)
        else:
            raise ValueError(f"no speed-up {name!r}")
