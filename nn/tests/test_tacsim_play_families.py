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
