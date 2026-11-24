"""The MLX twin of the army net's observation (`tacsim/armyobs.py`) against the Rust builder on recorded moments.

The fixture is three plays of `ARMY_OBS_DUMP` (`ai-exam suite --players army-net`, net M0), their first decisions:
a siege of the industrial era (the player-level wounded modifier), a raid (pillage), a promotion choice.
"""
from pathlib import Path

import mlx.core as mx

from aurelia_nn.tacsim import armyobs

FIXTURE = Path(armyobs.__file__).parent / "fixtures" / "armyobs.jsonl.gz"


def test_tokens_and_candidates_equal_rust():
    mx.set_default_device(mx.cpu)
    res = armyobs.check(str(FIXTURE), show=3)
    st = res["stats"]
    assert res["bad"] == {}, res
    assert st["plays"] == 3 and st["static planes equal"] == 3
    assert st["tokens equal"] == st["token rows (window)"] > 0
    assert st["candidate lists equal"] == st["candidate lists"] - st.get("candidate lists reaching outside the window", 0) > 0
