"""The speed-ups of `armyfast` are exact on the current twin (merge guard of perf/model with nn/mlx-army).

The class: a speed-up compiles a function of the simulator (`mx.compile`, no host read inside) or wraps one by its signature,
and a later change of that function adds a host read (a ruins early-out) or a keyword (`Sim.masks(rows=)`) the wrapper cannot
carry: the speed-up then fails on a game that worked. The guard plays the mixed fixture whole, with and without the full set,
and compares the verdicts and the state's digest after every round."""
from pathlib import Path

import mlx.core as mx

from aurelia_nn.army.model_mb import MBNet
from aurelia_nn.tacsim import armyfast, armyobs, armyperf, armyplay
from aurelia_nn.tacsim.armyloop import Brain

FIXTURE = Path(armyobs.__file__).parent / "fixtures" / "armymix.jsonl.gz"
FULL = ["bucket", "branchless", "masks", "preview", "tokens", "apply"]


def _play(fast, rounds=2):
    mx.set_default_device(mx.cpu)
    mx.random.seed(0)
    plays = armyobs.load(str(FIXTURE))
    g = armyplay.Game(plays, 1, Brain(MBNet()))
    g.fort_rule, g.scene_rule, g.compare = "old", True, False
    armyfast.install(g, fast)
    digests = []
    armyperf.install_digest(g, digests)
    v = g.play(rounds)
    mx.eval(v)
    return v.tolist(), digests


def test_the_full_set_of_speedups_plays_the_fixture_as_the_plain_game_does():
    base = _play([])
    assert base[1], "no round ended: the guard compares nothing"
    assert _play(FULL) == base


def test_compiled_masks_and_previews_without_branchless_take_the_rows_keyword():
    """Without `branchless` the plain path calls `Sim.masks(..., rows=)`; the compiled wrapper must carry (and ignore) it."""
    assert _play(["bucket", "masks", "preview"]) == _play([])
