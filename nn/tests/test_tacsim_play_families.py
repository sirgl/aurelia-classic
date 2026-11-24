"""The twin's own play runs for a family without a camp or a third seat (the camp's and the third seat's hooks return
when the batch has none: at 6709fbc `armycamp.neutral_rule` raised KeyError camp_tile on every such play)."""
from pathlib import Path

import mlx.core as mx

from aurelia_nn.army.model_mb import MBNet
from aurelia_nn.tacsim import armyobs, armyplay

FIXTURE = Path(armyobs.__file__).parent / "fixtures" / "armyprice.jsonl.gz"


def test_own_play_of_a_family_without_a_camp_runs(tmp_path):
    mx.set_default_device(mx.cpu)
    weights = tmp_path / "weights.safetensors"
    MBNet().save_weights(str(weights))
    res = armyplay.run(str(FIXTURE), str(weights), rounds=3)
    assert res["families"], res
    assert all(v["plays"] > 0 for v in res["families"].values()), res["families"]


def test_family_tables_of_a_mixed_batch_read_scenes_without_the_family_fields():
    """A mixed batch (`armyprof` on packages 0, 1, 4, 7, 8): neutral-path's waypoints and spare-friends' enemy tiles are
    built over every scene, and the dumps of the older packages have no `centre` or `info` (KeyError 'centre' at f90213f)."""
    import types
    from aurelia_nn.tacsim import armyparty
    mx.set_default_device(mx.cpu)
    win = {"map_w": 40, "map_h": 30, "q0": 0, "r0": 0, "w": 24, "h": 24}
    old = {"scene": {"window": win}, "orders": {}}
    party = {"scene": {"window": win}, "orders": {"centre": 41, "info": {"enemy_tiles": "42"}}}
    game = types.SimpleNamespace(plays=[(old, [], None), (party, [], None)], idx=mx.array([0, 1]), sim=types.SimpleNamespace(T=24 * 24))
    cq, cr, mw, mh = armyparty._waypoints(game)
    assert cq.tolist() == [0, 1] and cr.tolist() == [0, 1]
    tiles = armyparty._enemy_tiles(game).tolist()
    assert not any(tiles[0]) and tiles[1][24 + 2]
