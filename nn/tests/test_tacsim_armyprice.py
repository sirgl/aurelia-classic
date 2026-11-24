"""Twin package 1 (`tacsim/armyprice.py`): the price forms against `army_net::price_set`, the verdict of a price form from a row, and
the observation on recorded plays of assault-reinforce whose reinforcements land (the spawn profile of the landed units)."""
from pathlib import Path

import mlx.core as mx

from aurelia_nn.tacsim import armyobs, armyprice

FIXTURE = Path(armyobs.__file__).parent / "fixtures" / "armyprice.jsonl.gz"


def test_price_forms_equal_rust():
    # `price_set` of army_net.rs: careful = the loss cap / 8 (half the army, else one unit), pressure 1 / the round limit.
    assert armyprice.price_set("careful", "assault-heal", 9, 30, None) == [_f32(4 / 8), _f32(1 / 30), 1.0]
    assert armyprice.price_set("careful", "planned-assault", 9, 30, None) == [_f32(1 / 8), _f32(1 / 30), 1.0]
    assert armyprice.price_set("careful", "siege", 5, 20, None) == [_f32(1 / 8), _f32(1 / 20), 1.0]
    # any cost = the whole army, pressure 1 / D (no D: the scene's deadline, else its limit).
    assert armyprice.price_set("anycost:6", "assault-heal", 9, 30, None) == [_f32(9 / 8), _f32(1 / 6), 1.0]
    assert armyprice.price_set("anycost", "assault-peace", 12, 9, 7) == [_f32(12 / 8), _f32(1 / 7), 1.0]
    assert armyprice.price_set("anycost", "siege", 5, 20, None) == [_f32(5 / 8), _f32(1 / 20), 1.0]


def test_price_form_verdict_from_a_row():
    # careful is the family's verdict as it stands; any cost is the capture by D with the losses free.
    assert armyprice.row_verdict(5, 4, True, False, "anycost", 6)
    assert not armyprice.row_verdict(7, 0, False, True, "anycost", 6)
    assert not armyprice.row_verdict(-1, 0, False, False, "anycost", 6)
    assert armyprice.row_verdict(5, 4, True, False, "careful", 6) is False
    assert armyprice.row_verdict(5, 0, False, True, "careful", 6) is True


def test_landed_reinforcements_are_observed_as_rust_does():
    mx.set_default_device(mx.cpu)
    res = armyobs.check(str(FIXTURE), show=3)
    st = res["stats"]
    assert res["bad"] == {}, res
    assert st["plays"] == 2
    assert st["tokens equal"] == st["token rows (window)"] > 0
    assert st["candidate lists equal"] == st["candidate lists"] > 0
    assert not any(k.startswith("moments skipped: a unit the play created") for k in st)


def _f32(x):
    return armyprice._f32(x)
