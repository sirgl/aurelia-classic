"""Line 7: a batch that mixes families is played as one game per family group (`armyplay.Batch`), and the split changes nothing."""
from pathlib import Path

import mlx.core as mx

from aurelia_nn.army.model_mb import MBNet
from aurelia_nn.tacsim import armycamp, armyciv, armyfog, armyobs, armyparty, armyplay, armyrule
from aurelia_nn.tacsim.armyloop import Brain
from aurelia_nn.tacsim.armyparts import syncs_of

FIXTURE = Path(armyobs.__file__).parent / "fixtures" / "armymix.jsonl.gz"


def test_every_family_has_one_group_and_the_groups_are_the_modules():
    names = {n for n, _ in armyplay.GROUPS}
    assert names == {"party", "camp", "civ", "fog", "assault"}
    for fam in ("siege", "ram", "raid", "prey", "bombard"):
        assert armyplay.family_group(fam) == "plain"
    for fam in armyplay.armythird.THIRD:
        assert armyplay.family_group(fam) == "party"
    assert armyplay.family_group("camp") == armyplay.family_group("camp-race") == "camp"
    assert {armyplay.family_group(f) for f in ("guard", "convoy", "snipe")} == {"civ"}
    assert {armyplay.family_group(f) for f in ("assault-peace", "assault-reinforce", "hill-archers", "city-fire")} == {"assault"}
    seen = [f for _, fams in armyplay.GROUPS for f in fams]
    assert len(seen) == len(set(seen)), "a family in two groups"


def test_split_plays_and_when_to_split():
    plays = armyobs.load(str(FIXTURE))
    groups = dict(armyplay.split_plays(plays))
    assert set(groups) == {"plain", "assault", "camp", "civ", "party"}
    assert sorted(i for ids in groups.values() for i in ids) == list(range(len(plays)))
    assert armyplay.should_split(plays, 1000) and not armyplay.should_split(plays, 1)
    assert armyplay.should_split(plays, 1, "on") and not armyplay.should_split(plays, 1000, "off")
    assert not armyplay.should_split(plays[:1], 10000)  # one group: nothing to split


def _play(split, copies=2, rounds=3):
    mx.set_default_device(mx.cpu)
    mx.random.seed(0)
    brain = Brain(MBNet())
    plays = armyobs.load(str(FIXTURE))
    b = armyplay.Batch(plays, copies, brain, split=split, setup=lambda g: setattr(g, "fort_rule", "old"))
    return b, b.play(rounds).tolist()


def test_a_split_batch_plays_the_same_rows_as_one_mixed_game():
    one, v1 = _play("off")
    many, v2 = _play("on")
    assert len(one.groups) == 1 and len(many.groups) == 5
    assert v1 == v2
    for k in ("scene-rounds", "net decisions", "net turns"):
        assert one.counts[k] == many.counts[k], k
    assert any(x >= 0 for x in v1)


def test_a_game_of_one_group_makes_no_host_sync_in_the_passes_of_the_other_groups():
    """The guard of the class "a family's pass runs on a batch without the family": in a game of one group the other groups'
    passes return before any device read (their state is absent or the family test is a python one)."""
    mx.set_default_device(mx.cpu)
    plays = armyobs.load(str(FIXTURE))
    brain = Brain(MBNet())
    ids = dict(armyplay.split_plays(plays))
    g = armyplay.Game([plays[i] for i in ids["plain"]], 1, brain)
    live = ~g.s["done"] & (g.verdict < 0)
    bad = []
    for fn in (armycamp.enemy_turn, armycamp.neutral_turn, armycamp.neutral_rule, armycamp.barbarian_end, armyparty.enemy_turn,
               armyparty.third_turn, armyciv.enemy_turn, armyciv.learner_pass, armyfog.enemy_turn, armyrule.field_turn):
        _, n = syncs_of(fn, g, live) if fn is not armycamp.barbarian_end else syncs_of(fn, g, live)
        if n:
            bad.append((fn.__module__, fn.__name__, n))
    _, n = syncs_of(armyparty.judge, g, 1, g.verdict)
    if n:
        bad.append(("armyparty.judge", n))
    _, n = syncs_of(armycamp.judge, g, 1, g.verdict)
    if n:
        bad.append(("armycamp.judge", n))
    _, n = syncs_of(armyciv.judge, g, 1, g.verdict)
    if n:
        bad.append(("armyciv.judge", n))
    _, n = syncs_of(armyfog.judge, g, 1, g.verdict)
    if n:
        bad.append(("armyfog.judge", n))
    assert not bad, bad
