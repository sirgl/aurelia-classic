"""The candidate aux head's targets and loss (format 10, `aurelia_nn.cand_aux`):
each target applies where the rules give it (a move of a unit that fights, a
blow with a preview, a candidate that strikes) and is NaN elsewhere; the loss
counts only the targets that apply, one decision as much as one label."""

import numpy as np
import torch

from aurelia_nn import cand_aux
from aurelia_nn.cand_aux import GROUPS, TARGETS, loss, select, targets_of
from aurelia_nn.move_strike import FORMAT_10_CANDIDATE

NAMES = ["act_move", "act_melee", "act_ranged", "preview_damage_to_defender", "preview_damage_to_attacker", "preview_defender_hp", "blow_kills", "blow_captures"] + [n for n, _ in FORMAT_10_CANDIDATE]
EXTRA = ["focus_damage_percent", "focus_takes", "danger_damage", "danger_dies"]
J = {t[0]: j for j, t in enumerate(TARGETS)}


def row(**v):
    r = np.zeros(len(NAMES), np.int16)
    for k, x in v.items():
        r[NAMES.index(k)] = x
    return r


def test_every_group_names_targets_and_the_targets_apply_where_the_rules_give_them():
    select("")
    assert {n for g in GROUPS.values() for n in g} == {t[0] for t in TARGETS}
    rows = np.stack([
        row(act_move=1, move_blow_captures=1, move_blow_kills=1, move_blow_damage_percent=100, move_strike_targets=1, move_city_distance=1),
        row(act_melee=1, preview_damage_to_defender=30, preview_damage_to_attacker=12, preview_defender_hp=60, blow_kills=0),
        row(act_move=1, move_enemy_distance=3),
        row(),  # a wait
    ])
    extra = np.array([[250, 1, 40, 0], [120, 1, 0, 0], [0, 0, 70, 1], [0, 0, 0, 0]], np.int16)
    fights = np.array([True, True, False, True])
    t = targets_of(rows, extra, NAMES, EXTRA, fights)
    assert t[0, J["move_blow_captures"]] == 1 and t[0, J["move_blow_damage"]] == 1.0 and t[0, J["city_distance"]] == 0.1
    assert t[0, J["focus_damage"]] == 2.5 and t[0, J["danger_damage"]] == 0.4, "a move that strikes: focus and danger"
    assert np.isnan(t[0, J["attack_dealt"]]), "a move has no blow preview"
    assert t[1, J["attack_dealt"]] == 0.5 and t[1, J["attack_taken"]] == 0.12 and t[1, J["focus_takes"]] == 1
    assert np.isnan(t[1, J["move_blow_kills"]]) and np.isnan(t[1, J["danger_dies"]]), "a blow is no move"
    assert np.isnan(t[2]).all(), "a civilian's move: nothing applies"
    assert np.isnan(t[3]).all(), "a wait: nothing applies"
    select("focus")
    try:
        t = targets_of(rows, extra, NAMES, EXTRA, fights)
        assert np.isfinite(t[:, J["focus_takes"]]).sum() == 2 and np.isnan(t[:, J["move_blow_kills"]]).all()
    finally:
        select("")


def test_the_loss_counts_the_targets_that_apply_one_decision_as_one_label():
    n = cand_aux.N
    target = torch.full((3, n), float("nan"))
    target[0, J["move_blow_kills"]] = 1.0
    target[1, J["move_blow_damage"]] = 0.5
    pred = torch.zeros(3, n, requires_grad=True)
    cand_dec, dec_k = torch.tensor([0, 0, 1]), torch.tensor([2, 1])
    got = loss(pred, target, cand_dec, dec_k)
    want = (np.log(2.0) / 2) + (0.25 / 2)  # BCE of logit 0 on 1, squared error 0.5², each over the decision's 2 candidates
    assert abs(float(got.detach()) - want) < 1e-5
    got.backward()
    assert pred.grad[2].abs().sum() == 0, "no target: no gradient"
