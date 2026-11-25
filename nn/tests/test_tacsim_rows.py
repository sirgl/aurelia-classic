"""Line 8: a unit's pass computes its masks and previews for the rows that need them, not for the whole game.

The class: a family script's call (`sim.masks`, `obs.preview`) ran over all B rows of the game and was masked to the rows of the
unit that acts (package 8 alone: 6.7 of 10 s per round in these two). The fix: `Sim.masks(..., rows=)` computes on the rows and
puts the result back by position; the scripts' previews are packed to their valid pairs (`Obs.preview_packed`)."""
import re
from pathlib import Path

import mlx.core as mx

from aurelia_nn.army.model_mb import MBNet
from aurelia_nn.tacsim import armyobs, armyplay, sim as simmod
from aurelia_nn.tacsim.armyloop import Brain

FIXTURE = Path(armyobs.__file__).parent / "fixtures" / "armymix.jsonl.gz"
TACSIM = Path(armyobs.__file__).parent
# Files whose passes are a family's script or the rule: every mask call there states its rows, every preview is packed.
PASSES = ("armycamp", "armyciv", "armyfog", "armyparty", "armyrule", "armystd")


def _game(copies=4):
    mx.set_default_device(mx.cpu)
    mx.random.seed(0)
    plays = armyobs.load(str(FIXTURE))
    return armyplay.Game(plays, copies, Brain(MBNet()))


def _leaves(mk):
    for k, v in sorted(mk.items()):
        if isinstance(v, tuple):
            for i, x in enumerate(v):
                yield (k, i), x
        else:
            yield k, v


def test_masks_of_some_rows_equal_the_masks_of_all_rows_on_those_rows():
    g = _game()
    B = g.B
    assert B >= 3 * simmod.Sim.ROWS_MIN
    s, sim = g.s, g.sim
    rows = (mx.arange(B) % 5) == 1  # a fifth of the rows: the compacted path
    assert 2 * int(mx.sum(rows).item()) < B
    checked = 0
    for side in (0, 1):
        sd = mx.full((B,), side, dtype=mx.int32)
        for u in range(min(g.U, 4)):
            me = mx.full((B,), u, dtype=mx.int32)
            full = sim.masks(s, sd, me)
            part = sim.masks(s, sd, me, rows=rows)
            for (k, a), (_, b) in zip(_leaves(full), _leaves(part)):
                assert a.shape == b.shape, (k, a.shape, b.shape)
                m = rows.reshape((-1,) + (1,) * (a.ndim - 1))
                assert bool(mx.all(mx.where(m, a == b, True)).item()), (side, u, k)
                checked += 1
    assert checked > 30


def test_masks_of_some_rows_run_on_those_rows_only():
    """The guard that the compaction is real: the wavefront is computed for the rows asked, not for the whole batch."""
    g = _game()
    B = g.B
    seen = []
    orig = simmod.Sim._masks_full

    def spy(self, s, side, slot):
        seen.append(self.B)
        return orig(self, s, side, slot)
    simmod.Sim._masks_full = spy
    try:
        sd, me = mx.zeros((B,), dtype=mx.int32), mx.zeros((B,), dtype=mx.int32)
        g.sim.masks(g.s, sd, me, rows=(mx.arange(B) == 3))
        g.sim.masks(g.s, sd, me, rows=mx.ones((B,), dtype=mx.bool_))
        g.sim.masks(g.s, sd, me)
    finally:
        simmod.Sim._masks_full = orig
    assert seen == [1, B, B], seen


def test_the_scripts_state_the_rows_of_every_mask_and_pack_every_preview():
    """The lint of the class: in the family scripts and the rule no `sim.masks(...)` without `rows=` and no `obs.preview(...)`
    (use `preview_packed`, which computes the valid pairs only)."""
    bad = []
    for name in PASSES:
        for no, line in enumerate((TACSIM / f"{name}.py").read_text().splitlines(), 1):
            code = line.split("#")[0]
            if re.search(r"\.masks\(", code) and "rows=" not in code:
                bad.append((name, no, line.strip()))
            if re.search(r"obs\.preview\(", code):
                bad.append((name, no, line.strip()))
    assert not bad, bad

