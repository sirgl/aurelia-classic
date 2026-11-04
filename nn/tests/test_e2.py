"""The E2 trainer (aurelia_nn/train_e2.py) and its loop (aurelia_nn/e2_loop.py)."""

import argparse
import copy
import csv
import json
import math
import os
import stat
import sys
from pathlib import Path

import numpy as np
import pytest
import torch

from aurelia_nn import e2_loop, quantize, synthetic, train_e2
from aurelia_nn.data import Shard
from aurelia_nn.model import Net, save_checkpoint

sys.path.insert(0, str(Path(__file__).parent))
import e2_fake  # noqa: E402


def args(*argv):
    ap = argparse.ArgumentParser()
    train_e2.add_arguments(ap)
    return ap.parse_args(list(argv) + ["--device", "cpu"])


# -- advantages and rewards ----------------------------------------------------


def test_grpo_advantages_are_normalised_within_each_group():
    rewards = {"a": 0.0, "b": 1.0, "c": 0.5, "d": 0.5, "e": 0.2, "f": 0.2, "g": 0.2, "s": 0.7}
    groups = {"a": 1, "b": 1, "c": 1, "d": 1, "e": 2, "f": 2, "g": 2, "s": 3}
    adv = train_e2.grpo_advantages(rewards, groups, eps=1e-8)
    g1 = np.array([adv[k] for k in "abcd"])
    assert abs(g1.mean()) < 1e-9 and abs(g1.std() - 1) < 1e-6
    assert adv["b"] > adv["c"] == adv["d"] > adv["a"]
    assert all(adv[k] == 0 for k in "efg"), "a group of equal rewards has no advantage"
    assert adv["s"] == 0, "a group of one without a baseline"
    # Groups of one: a running baseline (0 the first time, then r against it).
    base = train_e2.RunningBaseline(decay=0.5)
    assert train_e2.grpo_advantages({"x": 1.0}, {"x": 9}, 1e-8, base)["x"] == 0
    assert train_e2.grpo_advantages({"x": 3.0}, {"x": 9}, 1e-8, base)["x"] > 0
    assert train_e2.grpo_advantages({"x": 0.0}, {"x": 9}, 1e-8, base)["x"] < 0


def test_dr_grpo_advantages_are_the_difference_from_the_group_mean():
    rewards = {"a": 0.50, "b": 0.52, "c": 0.48, "d": 0.50, "e": 0.0, "f": 1.0}
    groups = {"a": 1, "b": 1, "c": 1, "d": 1, "e": 2, "f": 2}
    adv = train_e2.grpo_advantages(rewards, groups, norm="dr")
    assert adv["a"] == pytest.approx(0.0) and adv["b"] == pytest.approx(0.02) and adv["c"] == pytest.approx(-0.02)
    assert adv["f"] == pytest.approx(0.5) and adv["e"] == pytest.approx(-0.5), "no division by the group's std: a small spread stays small"
    old = train_e2.grpo_advantages(rewards, groups, eps=1e-8, norm="group-std")
    assert old["b"] > old["f"] == pytest.approx(1.0), "the old GRPO makes a 0.02 spread larger than a 0.5 one"
    # A group's divisor (the suite's running scale).
    scaled = train_e2.grpo_advantages(rewards, groups, norm="dr", scale={1: 0.02, 2: 0.5})
    assert scaled["b"] == pytest.approx(1.0) and scaled["f"] == pytest.approx(1.0)
    # A group of one: r minus the running baseline's mean, not divided by its std.
    base = train_e2.RunningBaseline(decay=0.5)
    assert train_e2.grpo_advantages({"x": 1.0}, {"x": 9}, baseline=base, norm="dr")["x"] == 0
    assert train_e2.grpo_advantages({"x": 1.5}, {"x": 9}, baseline=base, norm="dr")["x"] == pytest.approx(0.5)


def test_running_scale_is_an_ema_of_the_mean_square():
    sc = train_e2.RunningScale(decay=0.5)
    assert sc.std() == 1.0, "no data: no scaling"
    sc.update(0.04)
    assert sc.std() == pytest.approx(0.2) and sc.std(0.01) == pytest.approx(0.21)
    sc.update(0.16)
    assert sc.var == pytest.approx(0.10) and sc.n == 2
    sc.update(float("nan"))
    assert sc.n == 2, "an iteration without data leaves it"
    assert train_e2.RunningScale(**sc.state()).var == pytest.approx(0.10)


def test_running_scale_floor_keeps_near_equal_rewards_small():
    sc = train_e2.RunningScale()
    sc.update(0.006 ** 2)
    assert sc.std(0.0, 0.05) == pytest.approx(0.05), "a spread far below the floor is not blown up to unit size"
    sc = train_e2.RunningScale()
    sc.update(0.1 ** 2)
    assert sc.std(0.0, 0.05) == pytest.approx(0.1), "a real spread is its own scale"


def _battle_game(kind, group, member, enemy_lost, learner_lost=None, damage=None, held=False, captured=None, start=10, value=100.0):
    """A battle `.e2.json` with its round-by-round record: cumulative losses per round entry."""
    n = len(enemy_lost)
    learner_lost = learner_lost or [0.0] * n
    damage = damage or [0.0] * n
    army = lambda lost: {"player": 0, "kinds": [1, 1, 1, 1], "value": value, "lost_units": 0, "lost_value": lost[-1]}
    return {"group": group, "member": member, "group_size": 2, "temperature": 1.0, "learners": [0], "stem": f"b{group}{member}",
            "seats": [{"player": 0, "learner": True, "points_share": 0.0}],
            "ground": {"suite": "battle", "kind": kind, "edits": [], "battle": {
                "learner": 0, "enemy": 1, "start_round": start, "length": 30, "end_round": start + n - 1, "captured_round": captured, "held": held,
                "target_damage": max(damage), "learner_army": army(learner_lost), "enemy_army": army(enemy_lost), "difficulty": 1.0, "reward": 0.0,
                "rounds": {"round": list(range(start, start + n)), "learner_lost_units": [0] * n, "learner_lost_value": learner_lost,
                           "enemy_lost_units": [0] * n, "enemy_lost_value": enemy_lost, "target_damage": damage}}}}


def test_dense_battle_rewards_per_turn():
    g = _battle_game("field", 0, 0, enemy_lost=[0, 10, 30, 30], learner_lost=[0, 0, 20, 20])
    rounds, r = train_e2.battle_turn_rewards(g)
    assert list(rounds) == [10, 11, 12], "a turn per round entry but the last"
    assert np.allclose(r, [0.05, 0.0, 0.0]), "0.5 × (dealt − taken) of the round"
    assert r.sum() == pytest.approx(train_e2.battle_reward(g) - 0.5), "the sum is the end reward less its constant"
    d = _battle_game("defence", 0, 0, enemy_lost=[0, 20, 20], damage=[0.0, 0.4, 0.2], held=True)
    assert np.allclose(train_e2.battle_turn_rewards(d)[1], [0.1 - 0.1, 0.05 + 0.5]), "the city's damage taken; holding pays at the end"
    s = _battle_game("siege", 0, 0, enemy_lost=[0, 0, 0], damage=[0.0, 0.5, 0.3], captured=12)
    _, rs = train_e2.battle_turn_rewards(s, siege_shaping=0.4)
    assert rs[0] == pytest.approx(0.4 * 0.5 * 0.5) and rs[1] == pytest.approx(0.5 + 0.5 * (1 - 2 / 30)), "the highest damage, then the capture"
    old = copy.deepcopy(g)
    old["ground"]["battle"].pop("rounds")
    assert train_e2.battle_turn_rewards(old) is None, "no record (an older binary): the end reward"


def test_gae_on_a_toy_trace():
    r, v = np.array([0.0, 0.0, 1.0]), np.array([0.5, 0.5, 0.5])
    a = train_e2.gae_with_baseline(r, v, gamma=1.0, lam=1.0)
    assert np.allclose(a, [0.5, 0.5, 0.5]), "λ = 1, γ = 1: the reward-to-go minus V"
    a = train_e2.gae_with_baseline(r, v, gamma=1.0, lam=0.0)
    assert np.allclose(a, [0.0, 0.0, 0.5]), "λ = 0: one-step TD errors"
    a = train_e2.gae_with_baseline(r, v, gamma=0.9, lam=0.5)
    delta = [0 + 0.9 * 0.5 - 0.5, 0 + 0.9 * 0.5 - 0.5, 1 - 0.5]
    want = [delta[0] + 0.45 * delta[1] + 0.45 ** 2 * delta[2], delta[1] + 0.45 * delta[2], delta[2]]
    assert np.allclose(a, want)
    # A group of two sharing a scene: the baseline is the other member's reward-to-go.
    groups = {"g": {"x": (np.array([1, 2]), np.array([0.1, 0.3])), "y": (np.array([1, 2]), np.array([0.0, 0.1]))}}
    out = train_e2.battle_gae(groups, gamma=1.0, lam=1.0)
    assert np.allclose(out["x"][1], [0.4 - 0.1, 0.3 - 0.1]) and np.allclose(out["y"][1], -out["x"][1])
    assert list(out["x"][0]) == [1, 2]
    # A step only one member reached: the other scenes' mean at that step, else 0.
    groups = {"g": {"x": (np.array([1, 2]), np.array([0.0, 1.0])), "y": (np.array([1]), np.array([0.5]))}}
    out = train_e2.battle_gae(groups, gamma=1.0, lam=1.0)
    assert np.allclose(out["x"][1], [1.0 - 0.5, 1.0 - 0.0])


def test_plan_drops_zero_signal_groups_and_scales_per_suite():
    a = args("--adv-norm", "dr", "--zero-tol", "0.001", "--adv-scale-decay", "0.5", "--reward", "points_share")
    games = {}
    # Field battles: group 0 differs, group 1 two equal battles (zero signal).
    games["d/b00"] = _battle_game("field", 0, 0, enemy_lost=[0, 10, 30])
    games["d/b01"] = _battle_game("field", 0, 1, enemy_lost=[0, 0, 10])
    games["d/b10"] = _battle_game("field", 1, 0, enemy_lost=[0, 10, 10])
    games["d/b11"] = _battle_game("field", 1, 1, enemy_lost=[0, 10, 10])
    # Whole games of a duel: one group with a spread, one of equal rewards.
    duel = lambda g, m, share: {"group": g, "member": m, "group_size": 2, "temperature": 1.0, "learners": [0], "spec": {"suite": "duel"},
                                "seats": [{"player": 0, "learner": True, "points_share": share}]}
    games["e/d00"], games["e/d01"] = duel(0, 0, 0.6), duel(0, 1, 0.4)
    games["e/d10"], games["e/d11"] = duel(1, 0, 0.3), duel(1, 1, 0.3)
    scales = {}
    plan = train_e2.plan_advantages(games, a, scales)
    assert plan["drop"] == {"d/b10", "d/b11", "e/d10", "e/d11"}
    st = plan["stats"]
    assert st["battle/field"]["zero_signal"] == 1 and st["battle/field"]["zero_share"] == 0.5
    assert st["duel/points_share"]["groups"] == 2 and st["duel/points_share"]["zero_signal"] == 1
    assert st["duel/points_share"]["raw_rms"] == pytest.approx(0.1) and scales["duel/points_share"].std() == pytest.approx(0.1)
    # The dense battle's per-round advantages, divided by the kind's scale: unit RMS.
    dense = plan["dense"]
    assert set(dense) == {("d/b00", 0), ("d/b01", 0)} and set(dense[("d/b00", 0)]) == {10, 11}
    x = np.array([v for k in dense for v in dense[k].values()])
    assert np.sqrt((x ** 2).mean()) == pytest.approx(1.0 / (1.0 + a.adv_eps / st["battle/field"]["raw_rms"]), rel=1e-6)
    assert dense[("d/b00", 0)][10] > 0 > dense[("d/b01", 0)][10], "the member that dealt more"
    # The game targets of the kept duel group use the running scale; the next iteration's EMA moves.
    t = train_e2.game_targets([("e/d00", games["e/d00"], None), ("e/d01", games["e/d01"], None)], a, {}, scales)
    assert t["e/d00"][0][1] == pytest.approx(0.1 / (0.1 + a.adv_eps)) and t["e/d01"][0][1] == pytest.approx(-t["e/d00"][0][1])
    games["e/d00"]["seats"][0]["points_share"], games["e/d01"]["seats"][0]["points_share"] = 0.8, 0.2
    train_e2.plan_advantages(games, a, scales)
    assert scales["duel/points_share"].var == pytest.approx(0.5 * 0.01 + 0.5 * 0.09)
    # The old mode: no scale kept.
    old = train_e2.plan_advantages(games, args("--adv-norm", "group-std", "--reward", "points_share"), {})
    assert all(s["scale"] == 1.0 for s in old["stats"].values())


def _potential_game(group, member, totals, share=0.5, start=10, suite="duel", player=0, cities=None):
    """A whole game with its round-by-round potential (`.e2.json` format 3)."""
    n = len(totals)
    cities = cities if cities is not None else [1.0 if t > 0 else 0.0 for t in totals]
    rounds = list(range(start, start + n - 1)) + [start + n - 2]  # the game ended within its last round
    return {"format": 3, "group": group, "member": member, "group_size": 2, "temperature": 1.0, "learners": [player], "stem": f"p{group}{member}",
            "spec": {"suite": suite}, "seats": [{"player": player, "learner": True, "points_share": share}],
            "potential": {"round": rounds, "seats": [{"player": player, "prices": {"player": player, "round": start, "variant": "standard",
                                                                                  "rounds_left": 300, "centi": {}},
                                                      "total": totals, "raw": {"cities": cities}}]}}


def test_potential_turn_rewards_telescope():
    # Entry 0: no city yet, the starting technologies' few points.
    g = _potential_game(0, 0, [4.0, 100.0, 150.0, 300.0, 300.0], cities=[0, 1, 1, 2, 2])
    rounds, r = train_e2.potential_turn_rewards(g, 0)
    assert list(rounds) == [10, 11, 12, 13], "a turn per entry but the last; the repeated last round is the end within it"
    phi = np.log(np.array([4.0, 100.0, 150.0, 300.0, 300.0]) + 100.0)
    assert np.allclose(r, np.diff(phi)), "Φ = log(P + P0), P0 the potential at the first entry with a city"
    assert r[0] == pytest.approx(math.log(200.0 / 104.0)), "the capital founded: about log 2"
    assert r.sum() == pytest.approx(math.log(400.0 / 104.0)), "the sum telescopes to Φ(end) − Φ(start)"
    old = copy.deepcopy(g)
    old.pop("potential")
    assert train_e2.potential_turn_rewards(old, 0) is None, "an older file: no shaping"
    nocity = copy.deepcopy(g)
    nocity["potential"]["seats"][0]["prices"] = None
    assert train_e2.potential_turn_rewards(nocity, 0) is None, "never a city: no sheet, no shaping"
    out = _potential_game(0, 0, [100.0, 200.0, 0.0])
    assert train_e2.potential_turn_rewards(out, 0)[1][-1] == pytest.approx(math.log(100.0 / 300.0)), "eliminated: a finite drop"


def test_dense_potential_adds_per_turn_advantages():
    games = {
        "d/p00": _potential_game(0, 0, [100.0, 110.0, 130.0, 160.0], share=0.505),
        "d/p01": _potential_game(0, 1, [100.0, 100.0, 105.0, 110.0], share=0.5),
        # Equal outcomes, unequal potentials: kept with the shaping, dropped without.
        "d/p10": _potential_game(1, 0, [50.0, 60.0, 70.0], share=0.3),
        "d/p11": _potential_game(1, 1, [50.0, 55.0, 56.0], share=0.3),
    }
    off = args("--reward", "points_share")
    assert off.dense_potential == 0.0, "off by default: run4's behaviour"
    plan0 = train_e2.plan_advantages(games, off, {})
    assert plan0["drop"] == {"d/p10", "d/p11"} and plan0["shaping"] == {} and "duel/dense" not in plan0["stats"]
    a = args("--reward", "points_share", "--dense-potential", "0.5", "--dense-gamma", "1", "--dense-lambda", "1")
    scales = {}
    plan = train_e2.plan_advantages(games, a, scales)
    assert plan["drop"] == set(), "the shaping carries signal where the outcome has none"
    st = plan["stats"]
    assert st["duel/points_share"]["zero_signal"] == 1 and st["duel/dense"]["groups"] == 2 and st["duel/dense"]["zero_signal"] == 0
    assert "duel/dense" in scales
    sh = plan["shaping"]
    assert set(sh) == {(k, 0) for k in games} and set(sh[("d/p00", 0)]) == {10, 11, 12}
    # γ = λ = 1, two members: the raw advantage at step k is the member's shaping reward-to-go less the other's.
    to_go = lambda t: np.log(t[-1] + t[0]) - np.log(np.asarray(t[:-1]) + t[0])
    raw = to_go(games["d/p00"]["potential"]["seats"][0]["total"]) - to_go(games["d/p01"]["potential"]["seats"][0]["total"])
    div = st["duel/dense"]["scale"]
    assert [sh[("d/p00", 0)][r] for r in (10, 11, 12)] == pytest.approx(list(0.5 * raw / div))
    assert [sh[("d/p01", 0)][r] for r in (10, 11, 12)] == pytest.approx(list(-0.5 * raw / div))
    # The turn's advantage: the outcome's + the shaping's; the value target the outcome alone.
    t = train_e2.game_targets([(k, games[k], None) for k in ("d/p00", "d/p01")], a, {}, scales, plan["dense"], plan["shaping"])
    reward, a_game, _, value, per_round, default = t["d/p00"][0]
    out_adv = train_e2.game_targets([(k, games[k], None) for k in ("d/p00", "d/p01")], a, {}, scales)["d/p00"][0][1]
    assert default == pytest.approx(out_adv) and out_adv > 0, "the member with the better outcome"
    assert per_round[11] == pytest.approx(out_adv + sh[("d/p00", 0)][11])
    assert value == reward == pytest.approx(0.505), "the value head keeps the whole game's outcome"
    # Battles keep their own dense reward; a file without the record keeps the outcome's advantage alone.
    games["d/b00"] = _battle_game("field", 7, 0, enemy_lost=[0, 10, 30])
    games["d/b01"] = _battle_game("field", 7, 1, enemy_lost=[0, 0, 10])
    old = copy.deepcopy(games["d/p00"])
    old.pop("potential")
    assert not train_e2.dense_potential(old, a) and not train_e2.dense_potential(games["d/b00"], a)
    plan = train_e2.plan_advantages(games, a, {})
    assert ("d/b00", 0) in plan["dense"] and ("d/b00", 0) not in plan["shaping"]


def test_e2_shard_turns_without_a_round_take_the_default(tmp_path):
    d = synthetic.write_shard(tmp_path / "s.shard", 0, 12, 8, rounds=4)
    e2_fake.add_e2(d, {"temperature": 1.0, "group": 0, "member": 0, "group_size": 2, "learners": [0, 1], "seats": []})
    shard = train_e2.E2Shard(d)
    shard.set_targets({0: (0.5, 9.0, None, 0.5, {0: 1.0}, 0.25)})
    w = next(w for w in shard.windows if int(shard.turns[w[0], 1]) == 0)
    assert list(shard.turn_adv[w]) == [1.0 if int(r) == 0 else 0.25 for r in shard.turns[w, 0]]


def test_e2_shard_takes_per_round_advantages(tmp_path):
    d = synthetic.write_shard(tmp_path / "s.shard", 0, 12, 8, rounds=4)
    e2_fake.add_e2(d, {"temperature": 1.0, "group": 0, "member": 0, "group_size": 2, "learners": [0, 1], "seats": []})
    shard = train_e2.E2Shard(d)
    shard.set_targets({0: (0.5, 9.0, None, float("nan"), {0: 1.0, 2: -2.0}), 1: (0.5, 0.25, None, float("nan"))})
    for w in shard.windows:
        p = int(shard.turns[w[0], 1])
        got = shard.turn_adv[w]
        if p == 0:
            assert list(got) == [{0: 1.0, 2: -2.0}.get(int(r), 0.0) for r in shard.turns[w, 0]], "the turn's round; no advantage: 0"
        else:
            assert np.all(got == 0.25), "a game advantage for a seat without per-round ones"


def _game(victory=None, winner=0, share=0.0, final=300, rounds=450, science=None):
    seats = [{"player": p, "learner": p == 0, "points_share": share if p == 0 else 1 - share, "victory": victory if p == winner else None} for p in range(2)]
    if science:
        seats[0]["space"] = science
    return {"spec": {"rounds": rounds}, "final_round": final, "seats": seats, "learners": [0]}


def test_rewards():
    win = 1 + 0.2 * (1 - 300 / 450)
    assert train_e2.reward_of(_game("science"), 0, "victory") == pytest.approx(win)
    assert train_e2.reward_of(_game("domination", winner=1), 0, "victory") == 0
    assert train_e2.reward_of(_game("time", share=1.0), 0, "victory") == 0.5, "a win on time counts as the score fallback"
    assert train_e2.reward_of(_game(None, share=0.5), 0, "victory") == 0.25
    assert train_e2.reward_of(_game("time", share=1.0), 0, "points_share") == 1.0
    assert train_e2.reward_of(_game("science"), 0, "science") == pytest.approx(win)
    assert train_e2.reward_of(_game("domination"), 0, "science") == 0, "no science progress recorded, no stand-in"
    rec = {"space_techs": 30, "space_techs_needed": 75, "projects": 1, "projects_needed": 1, "parts": 1, "parts_needed": 7}
    assert train_e2.reward_of(_game("time", science=rec), 0, "science") == pytest.approx(0.3 * (0.6 * 0.4 + 0.4 * 0.25))
    late = _game("science", final=400)
    late["victory"] = {"winner": 0, "kind": "science", "round": 350}
    assert train_e2.reward_of(late, 0, "science") == pytest.approx(1 + 0.2 * (1 - 350 / 450)), "the round of the victory, from the record"
    assert train_e2.reward_of(_game("time"), 0, "science", tech_share=0.5) == pytest.approx(0.3 * 0.6 * 0.5)
    assert train_e2.reward_of(_game("domination"), 0, "conquest") == pytest.approx(win)
    assert train_e2.gae(np.array([0.0, 0.0]), 1.0, 1.0).tolist() == [1.0, 1.0]


# -- the objective -------------------------------------------------------------


def _terms(logits, acc, ref=None, choice=None, logp_b=None, adv=None, beta=0.02, ent=0.001):
    d = logits.shape[0]
    choice = torch.zeros(d, dtype=torch.long) if choice is None else choice
    logp_b = torch.zeros(d) if logp_b is None else logp_b
    adv = torch.ones(d) if adv is None else adv
    return train_e2.e2_terms(logits, ref, acc, choice, logp_b, adv, torch.ones(d), torch.ones(d, dtype=torch.bool), torch.ones(d), 0.2, beta, ent)


def test_refused_candidates_are_outside_the_softmax():
    logits = torch.tensor([[1.0, 50.0, 0.0, float("-inf")]], requires_grad=True)
    acc = torch.tensor([[True, False, True, False]])
    t = _terms(logits, acc, ref=logits.detach().clone())
    expect = torch.log_softmax(torch.tensor([1.0, 0.0]), 0)[0]
    assert t["new"].item() == pytest.approx(expect.item(), abs=1e-6)
    t["loss"].backward()
    assert torch.isfinite(logits.grad).all()
    assert logits.grad[0, 1] == 0 and logits.grad[0, 3] == 0, "no gradient reaches a refused candidate"
    assert logits.grad[0, 0] != 0


def test_kl_is_zero_against_itself():
    torch.manual_seed(0)
    logits = torch.randn(6, 5)
    acc = torch.rand(6, 5) > 0.3
    acc[:, 0] = True
    t = _terms(logits, acc, ref=logits.clone())
    assert t["kl"].abs().max().item() < 1e-6
    other = _terms(logits, acc, ref=logits + torch.randn(6, 5))
    assert (other["kl"] >= -1e-6).all() and other["kl"].max() > 1e-3


def test_ppo_clip():
    logits = torch.tensor([[0.0, 0.0]], requires_grad=True)
    acc = torch.ones(1, 2, dtype=torch.bool)
    # Behaviour much less likely than now: ratio 2 > 1 + ε, positive advantage → clipped, no gradient.
    t = _terms(logits, acc, logp_b=torch.tensor([float(np.log(0.25))]), beta=0.0, ent=0.0)
    assert t["ratio"].item() == pytest.approx(2.0)
    t["loss"].backward()
    assert logits.grad.abs().max() == 0


# -- shards: the ratio at the behaviour weights ----------------------------------


def _behaviour_shard(tmp_path, model, temperature=1.0, seed=3):
    """A synthetic shard whose decisions are sampled from the integer
    reference of `model` (the int8 net that plays), some candidates refused;
    the E2 files as ai-features --e2 writes them. Returns the shard's
    directory and the largest |float − integer| logit gap seen."""
    d = synthetic.write_shard(tmp_path / f"b{seed}.shard", seed, 12, 8, rounds=8, players=2)
    s = Shard(d)
    cfg = model.cfg
    qm = quantize.quantize(model, s.layout)
    rng = np.random.default_rng(seed)
    dec = np.load(d / "decisions.npy")
    accepted = np.ones(len(s.candidates), np.uint8)
    logp = np.zeros(len(dec), np.float32)
    gap = 0.0
    from aurelia_nn.data import collate
    from aurelia_nn.model import MemoryBank, memory_batch, padded_logits, store_memory

    order = s._dec_order
    for w in s.windows:
        imem = quantize.Memory.empty(cfg)
        bank = MemoryBank(cfg, "cpu")
        for t in w:
            turn = s.turn(int(t))
            st, il = quantize.turn_logits(qm, turn, imem)
            imem = quantize.end_turn(qm, st, imem)
            with torch.no_grad():
                b = memory_batch([bank], collate([turn], cfg), cfg)
                fl, _v, (p, n, u) = model(b, with_memory=True)[:3]
                store_memory([bank], b, p, n, u)
                fl = padded_logits(b, fl).numpy()
            rows = order[s._dec_start[t] : s._dec_start[t + 1]]
            for j, r in enumerate(rows):
                first, k = int(dec[r, 3]), int(dec[r, 4])
                acc = rng.random(k) > 0.3
                acc[rng.integers(k)] = True
                z = np.where(acc, il[j] / 256.0 / temperature, -np.inf)
                p_ = np.exp(z - z.max())
                p_ /= p_.sum()
                c = int(rng.choice(k, p=p_))
                dec[r, 5] = c
                logp[r] = np.log(p_[c])
                accepted[first : first + k] = acc
                gap = max(gap, float(np.abs(fl[j][:k] - il[j] / 256.0).max()))
    np.save(d / "decisions.npy", dec.astype("<i4"))
    game = {"temperature": temperature, "group": 0, "member": 0, "group_size": 1, "learners": [0, 1],
            "seats": [{"player": p, "learner": True, "points_share": 0.5} for p in range(2)]}
    e2_fake.add_e2(d, game, logp=logp, accepted=accepted)
    return d, gap


@pytest.mark.parametrize("temperature", [1.0, 0.7])
def test_ratio_is_one_at_the_behaviour_weights(tmp_path, temperature):
    layout = Shard(synthetic.write_shard(tmp_path / "l.shard", 0, 12, 8, rounds=2)).layout
    torch.manual_seed(1)
    model = Net(layout.config("small"), layout).eval()
    d, gap = _behaviour_shard(tmp_path, model, temperature)
    shard = train_e2.E2Shard(d)
    shard.set_targets({0: (1.0, 0.5, None), 1: (0.0, -0.5, None)})
    a = args()
    prepared = train_e2.E2Prepared(train_e2.window_turns([shard]), model.cfg, 3)
    stats = train_e2.Stats()
    train_e2.run_e2(model, copy.deepcopy(model), prepared, torch.device("cpu"), a, stats)
    rec = stats.read()
    lr = np.concatenate([x.numpy()[v] for x, v in stats.logr])
    tol = 2 * gap / temperature + 1e-4  # the softmax of logits off by at most `gap` each
    assert rec["decisions"] == len(lr) > 50
    assert np.abs(lr).max() <= tol, (np.abs(lr).max(), tol)
    assert rec["ratio_mean"] == pytest.approx(1.0, abs=tol)
    assert rec["kl"] == pytest.approx(0.0, abs=1e-6), "the reference is the model itself"
    assert rec["masked_frac"] == 0
    # Refusals were applied: with every candidate in the softmax the ratio is off.
    shard.e2_accepted[:] = True
    stats2 = train_e2.Stats()
    train_e2.run_e2(model, None, train_e2.E2Prepared(train_e2.window_turns([shard]), model.cfg, 3), torch.device("cpu"), a, stats2)
    lr2 = np.concatenate([x.numpy()[v] for x, v in stats2.logr])
    assert np.abs(lr2).max() > tol


def test_e2_shard_turn_carries_the_e2_arrays(tmp_path):
    d = synthetic.write_shard(tmp_path / "s.shard", 4, 12, 8, rounds=4, players=2)
    game = {"temperature": 1.0, "group": 0, "member": 0, "group_size": 1, "learners": [0, 1], "seats": []}
    e2_fake.add_e2(d, game)
    s = train_e2.E2Shard(d)
    s.set_targets({0: (0.7, 1.5, None)})
    for t in range(len(s)):
        turn = s.turn(t)
        arr = turn.dec_arrays
        assert len(arr["e2_logp"]) == len(arr["head"]) and len(arr["e2_acc"]) == len(arr["rows"])
        if int(s.turns[t, 1]) == 0:
            assert turn.value_target == 0.7 and (arr["e2_adv"] == 1.5).all()
        else:
            assert np.isnan(turn.value_target) and (arr["e2_adv"] == 0).all()


# -- the loop: resume ----------------------------------------------------------------


def _fakes(tmp_path):
    fake = Path(e2_fake.__file__)
    out = {}
    for name in ("selfplay", "tournament"):
        p = tmp_path / f"fake-{name}"
        p.write_text(f"#!/bin/sh\nexec '{sys.executable}' '{fake}' {name} \"$@\"\n")
        p.chmod(p.stat().st_mode | stat.S_IEXEC)
        out[name] = str(p)
    out["features"] = f"'{sys.executable}' '{fake}' features"
    return out


def _loop_args(state, init, fakes, iterations, extra=()):
    return [
        "--state", str(state), "--init", str(init), "--iterations", str(iterations), "--groups", "2", "--group-size", "2", "--rounds", "6",
        "--jobs", "1", "--workers", "2", "--games-per-batch", "2", "--epochs", "2", "--bptt", "3", "--eval-every", "1", "--eval-standard-speed", "--pace-every", "2",
        "--keep", "1", "--opponents", "standard:0.5,league:0.5", "--suites", "standard:0.5,duel:0.5", "--eval-suites", "standard", "--selfplay-bin", fakes["selfplay"], "--tournament-bin", fakes["tournament"],
        "--features-bin", fakes["features"], "--device", "cpu", "--lr", "1e-3", "--warmup", "1", *extra,
    ]


def test_loop_resumes_to_the_same_weights(tmp_path, monkeypatch):
    monkeypatch.setenv("PYTHONPATH", str(Path(__file__).resolve().parents[1]))
    threads = torch.get_num_threads()
    torch.set_num_threads(1)  # sums in one order: a resumed run must give the same bits
    try:
        _resume_check(tmp_path)
    finally:
        torch.set_num_threads(threads)


def _resume_check(tmp_path):
    layout = Shard(synthetic.write_shard(tmp_path / "l.shard", 0, 12, 8, rounds=2)).layout
    torch.manual_seed(0)
    init = tmp_path / "init.pt"
    save_checkpoint(init, Net(layout.config("small"), layout))
    fakes = _fakes(tmp_path)

    straight = tmp_path / "straight"
    e2_loop.main(_loop_args(straight, init, fakes, 3))
    resumed = tmp_path / "resumed"
    e2_loop.main(_loop_args(resumed, init, fakes, 1))
    e2_loop.main(_loop_args(resumed, init, fakes, 2))
    # A crash after the play of iteration 3: its games are there, training is redone.
    e2_loop.Loop(e2_loop.argparse.Namespace(**{**vars(_parse(_loop_args(resumed, init, fakes, 3))), "iterations": 3})).play(3)
    e2_loop.main(_loop_args(resumed, init, fakes, 3))

    for run in (straight, resumed):
        st = json.loads((run / "state.json").read_text())
        assert st["iteration"] == 3 and not st["eval_pending"]
        assert not any((run / "games").iterdir()), "games are deleted after training"
        rows = list(csv.DictReader(open(run / "progress.csv")))
        assert [r["iteration"] for r in rows] == ["1", "2", "3"]
        assert rows[0]["eval_all"] == "0.5" and rows[0]["eval_all_standard_speed"] == "0.5" and rows[1]["pace_sci500"] != ""
        assert set(st["evals"]["1"]) == {"standard-all-quick", "standard-hybrid-quick", "standard-all"}, "the fakes take --speed: quick, and a standard reference"
        assert (run / "best-all.bin").exists() and (run / "best-hybrid.pt").exists()
        kept = sorted(p.name for p in (run / "ckpt").glob("*.pt"))
        assert kept == ["iter_0000.pt", "iter_0003.pt"]
        assert st["evals"]["2"]["pace-all-quick"]["science_by_400"] == 1 and st["evals"]["2"]["pace-all-quick"]["science_by_500"] == 2
    a = torch.load(straight / "ckpt/iter_0003.pt", weights_only=False)["state"]
    b = torch.load(resumed / "ckpt/iter_0003.pt", weights_only=False)["state"]
    # With threaded sums the order varies a little and Adam turns noise-level
    # gradients into lr-sized steps, so the check runs single-threaded.
    diff = max(float((a[k].float() - b[k].float()).abs().max()) for k in a)
    init_state = torch.load(init, weights_only=False)["state"]
    moved = max(float((a[k].float() - init_state[k].float()).abs().max()) for k in a)
    assert diff < 1e-6 and moved > 1e-3, (diff, moved)


def test_sequential_eval_against_the_start_net(tmp_path):
    """`--eval-sprt` with `--eval-reference start`: the start net's reports
    are played once (offsets until the cap), passed as `--reference`, and the
    test's decision lands in the eval's summary; the pace suite stays plain."""
    fakes = _fakes(tmp_path)
    loop = object.__new__(e2_loop.Loop)
    loop.a = argparse.Namespace(tournament_bin=fakes["tournament"], eval_jobs=1, eval_sprt="-0.05,0.05", eval_sprt_error="0.05,0.05", eval_max_games=8,
                                eval_min_games=0, eval_reference="start", eval_suites="standard", pace_repeat=1)
    loop.eval_dir, loop.ckpt_dir = tmp_path / "evals", tmp_path / "ckpt"
    loop.eval_dir.mkdir()
    loop.speed, loop.speed_flag = "quick", True
    loop.log_file = open(tmp_path / "loop.log", "a")
    loop._check_sequential()
    res = loop.evaluate(3, "all", "standard", "quick")
    assert res["sequential"] == {"decision": "h0", "games": 4, "blocks": 2, "llr": -3.0, "mean": -0.1, "low": -0.2, "high": 0.0, "reference_share": 0.6}
    assert "minus the start net's 0.600: -0.100" in e2_loop.sequential_line(res["sequential"])
    refs = sorted(loop.eval_dir.glob("start-*.json"))
    assert [p.name for p in refs] == ["start-standard-all-quick-offset0.json", "start-standard-all-quick-offset1.json"], "4 games each up to the cap of 8"
    assert all(json.loads(p.read_text())["weights"].endswith("iter_0000.bin") for p in refs)
    argv = json.loads((loop.eval_dir / "iter_0003-standard-all-quick.json").read_text())["argv"]
    assert argv[argv.index("--sprt") + 1] == "-0.05,0.05" and argv.count("--reference") == 2 and argv[argv.index("--max-games") + 1] == "8"
    loop.evaluate(4, "all", "standard", "quick")
    assert len(list(loop.eval_dir.glob("start-*.json"))) == 2, "the reference is played once"
    pace = loop.evaluate(4, "all", "pace", "quick")
    assert "sequential" not in pace
    loop.a.eval_sprt = ""
    assert "sequential" not in loop.evaluate(5, "all", "standard", "quick"), "off by default: the plain 80 games"


def _parse(argv):
    ns = {}
    real = e2_loop.Loop

    class Capture:
        def __init__(self, a):
            ns["a"] = a

        def run(self):
            pass

    e2_loop.Loop = Capture
    try:
        e2_loop.main(argv)
    finally:
        e2_loop.Loop = real
    return ns["a"]


def test_curriculum_plan(tmp_path):
    loop = object.__new__(e2_loop.Loop)
    loop.a = argparse.Namespace(groups=8, round0_every=2, round0_share=0.25, give_share=0.25, seed=0, opponent_seats=3)
    loop.ckpt_dir = tmp_path
    loop.midgame, loop.give = True, True
    loop.suites, loop.opponents = {"standard": 1.0}, e2_loop.parse_mix("standard:0.5,league:0.5")
    (tmp_path / "iter_0000.bin").write_bytes(b"")
    runs = {r["name"]: r for r in loop.plan(2)}  # a round-0 iteration, a past checkpoint for the league
    assert {k: r["groups"] for k, r in runs.items()} == {"standard-zero-standard": 1, "standard-zero-league": 1, "standard-mid-standard": 2,
                                                         "standard-mid-league": 2, "standard-give-standard": 1, "standard-give-league": 1}
    assert runs["standard-give-standard"]["extra"] == ["--give-space-techs"]
    assert runs["standard-mid-league"]["opponents"] == ["standard", "standard"] + [runs["standard-mid-league"]["opponents"][-1]]
    assert runs["standard-mid-league"]["opponents"][-1].startswith("net:")
    loop.suites, loop.opponents = e2_loop.parse_mix("duel:0.5,standard:0.5"), e2_loop.parse_mix("standard:0.6,tall-book:0.2,league:0.2")
    runs = loop.plan(3)
    duel = [r for r in runs if r["suite"] == "duel"]
    assert sum(r["groups"] for r in duel) == 4 and all(r["pool"] == "zero" for r in duel)
    assert all(len(r["opponents"]) == 1 for r in duel), "a duel has one opponent seat"
    assert {r["name"]: r["opponents"] for r in duel}["duel-zero-tall-book"] == ["tall-book"]
    assert e2_loop.deal(7, e2_loop.parse_mix("a:0.6,b:0.2,c:0.2")) == {"a": 4, "b": 2, "c": 1}  # largest remainder, ties in the mix's order
    loop.suites, loop.opponents = {"standard": 1.0}, {"standard": 1.0}
    assert sum(r["groups"] for r in loop.plan(3)) == 8 and not any(r["pool"] == "zero" for r in loop.plan(3))
    loop.midgame = loop.give = False  # an older binary: every group from round 0
    assert {r["pool"] for r in loop.plan(3)} == {"zero"}


# -- training grounds: rewards, the value bootstrap, PFSP ------------------------


def _battle(kind, own=0.0, enemy=0.0, captured=None, held=False, damage=0.0, start=100, length=30):
    army = lambda lost: {"player": 0, "kinds": [1], "value": 200, "lost_units": 0, "lost_value": 200 * lost}
    return {"learners": [0], "seats": [{"player": 0, "learner": True, "points_share": 0.0}], "spec": {"rounds": 300}, "final_round": start + length,
            "ground": {"suite": "battle", "kind": kind, "edits": [], "battle": {"start_round": start, "length": length, "end_round": start + length,
                                                                               "captured_round": captured, "held": held, "target_damage": damage,
                                                                               "learner_army": army(own), "enemy_army": army(enemy)}}}


def test_battle_rewards_per_kind():
    assert train_e2.battle_reward(_battle("field", own=0.2, enemy=0.6)) == pytest.approx(0.7)
    assert train_e2.battle_reward(_battle("field", own=1.0, enemy=0.0)) == 0.0
    assert train_e2.battle_reward(_battle("field")) == 0.5, "nothing lost on either side: par"
    assert train_e2.battle_reward(_battle("siege", captured=100)) == 1.0, "taken at once"
    assert train_e2.battle_reward(_battle("siege", captured=115)) == pytest.approx(0.75)
    assert train_e2.battle_reward(_battle("siege", enemy=0.5, damage=0.5)) == pytest.approx(0.4 * (0.5 * 0.5 + 0.25 * 0.5))
    killed = _battle("siege", enemy=1.0, damage=1.0)
    killed["ground"]["battle"]["enemy_army"]["lost_units"] = 1
    assert train_e2.battle_reward(killed, siege_shaping=0.45) == pytest.approx(0.45), "damage, kills and losses all at their most"
    assert train_e2.battle_reward(killed, siege_shaping=0.9) < train_e2.battle_reward(_battle("siege", captured=130)), "a capture beats any shaping"
    assert train_e2.battle_reward(_battle("defence", enemy=0.4, held=True)) == pytest.approx(0.7)
    assert train_e2.battle_reward(_battle("rush-defence", enemy=0.4, held=False)) == pytest.approx(0.2)
    rng = np.random.default_rng(0)
    for _ in range(200):
        kind = str(rng.choice(train_e2.BATTLE_KINDS))
        g = _battle(kind, own=rng.uniform(0, 1.5), enemy=rng.uniform(0, 1.5), captured=int(rng.integers(90, 150)) if rng.random() < 0.3 else None,
                    held=bool(rng.random() < 0.5), damage=float(rng.uniform(0, 2)))
        assert 0.0 <= train_e2.battle_reward(g) <= 1.0
    assert train_e2.battle_won(_battle("field", own=0.1, enemy=0.3)) and not train_e2.battle_won(_battle("field", own=0.3, enemy=0.3))
    assert train_e2.battle_won(_battle("siege", captured=120)) and train_e2.battle_won(_battle("defence", held=True))
    assert train_e2.suite_key(_battle("siege")) == "battle/siege" and train_e2.suite_key({"spec": {"suite": "duel"}}) == "duel"


def _economy(share=0.6, start=100, end=160):
    return {"learners": [0], "group": 0, "group_size": 1, "seats": [{"player": 0, "learner": True, "points_share": 0.0}], "spec": {"rounds": 500},
            "final_round": end, "ground": {"suite": "economy", "kind": "window", "edits": [],
                                           "economy": {"from_round": start, "window": end - start, "end_round": end, "science_share": share}}}


def _expert(game, net, expert, net_rate=10.0, expert_rate=10.0, player=0):
    """`_economy` with the potentials (totals and the science component) of the learner and the expert."""
    comps = lambda total, rate: {"total": total, "components": {"science": {"raw": rate, "price": 1.0, "value": rate}}}
    e = game["ground"]["economy"]
    e["potential"] = [{"player": player, "prices": {}, "net": comps(net, net_rate), "expert": comps(expert, expert_rate)}]
    e.update(learner_potential=net, expert_potential=expert, learner_rate=net_rate, expert_rate=expert_rate)
    return game


def test_economy_reward_bootstraps_with_the_value_head():
    r, sci = train_e2.economy_return(_economy(0.6), 0.5, gamma=0.99)
    assert sci == 0.6 and r == pytest.approx(0.6 + 0.99 ** 60 * 0.5)
    assert train_e2.economy_return(_economy(0.6), None)[0] == 0.6, "no value head: the science term alone"
    assert train_e2.economy_return(_economy(0.6, end=130), 1.0, gamma=0.9)[0] == pytest.approx(0.6 + 0.9 ** 30), "W: the rounds played"
    a = args("--economy-gamma", "0.98", "--economy-reward", "share", "--economy-bootstrap")
    games = [("eco", _economy(0.7), None, lambda p: 0.4), ("bat", _battle("siege", captured=100), None, lambda p: 0.9), ("std", _game("time", share=1.0), None, None)]
    t = train_e2.game_targets(games, a, {})
    assert t["eco"][0][0] == pytest.approx(0.7 + 0.98 ** 60 * 0.4) and np.isnan(t["eco"][0][3]), "economy share + bootstrap: the reward before the expert's"
    assert t["bat"][0][0] == 1.0 and np.isnan(t["bat"][0][3]), "battle: no value target"
    assert t["std"][0][3] == t["std"][0][0], "a whole game: the value head learns its outcome"


def test_economy_reward_against_the_expert():
    g = _expert(_economy(0.7), net=1200.0, expert=1000.0, net_rate=30.0, expert_rate=40.0)
    a = args()
    assert a.economy_reward == "potential" and not a.economy_bootstrap, "the default: the potential, no bootstrap"
    assert train_e2.economy_reward(g, 0) == pytest.approx(np.log(1.2)), "log(net / expert potential)"
    assert train_e2.economy_reward(g, 0, mode="rate") == pytest.approx(np.log(0.75)), "rate: the science a turn"
    assert train_e2.economy_reward(g, 0, mode="share") == 0.7
    t = train_e2.game_targets([("eco", g, None, lambda p: 0.4)], a, {})
    assert t["eco"][0][0] == pytest.approx(np.log(1.2)) and np.isnan(t["eco"][0][3]), "no V(final) without --economy-bootstrap; no value target"
    # The clip: [-1, 1]; a side at zero gives the clip's end.
    assert train_e2.economy_reward(_expert(_economy(), 5000.0, 1000.0), 0) == 1.0
    assert train_e2.economy_reward(_expert(_economy(), 100.0, 1000.0), 0) == -1.0
    assert train_e2.economy_reward(_expert(_economy(), 0.0, 1000.0), 0) == -1.0, "the learner out of the game"
    assert train_e2.economy_reward(_expert(_economy(), 10.0, 0.0), 0) == 1.0
    assert train_e2.economy_reward(_expert(_economy(), 0.0, 0.0, 0.0, 0.0), 0, mode="rate") == 0.0
    for x in np.random.default_rng(0).uniform(0, 10, 50):
        assert -1.0 <= train_e2.economy_reward(_expert(_economy(), float(x), 1.0), 0) <= 1.0
    # The bootstrap switch adds gamma^W V(final) to any mode.
    on = args("--economy-bootstrap", "--economy-gamma", "0.9")
    t = train_e2.game_targets([("eco", g, None, lambda p: 0.5)], on, {})
    assert t["eco"][0][0] == pytest.approx(np.log(1.2) + 0.9 ** 60 * 0.5)
    assert train_e2.economy_reward(g, 0, None, "rate", 0.9, True) == pytest.approx(np.log(0.75)), "no value head: nothing added"
    # The learner's own record; the means without one; an error without the expert.
    other = _expert(_economy(), 900.0, 1000.0, player=3)
    other["ground"]["economy"].update(learner_potential=2000.0, expert_potential=1000.0)
    assert train_e2.economy_reward(other, 3) == pytest.approx(np.log(0.9)) and train_e2.economy_reward(other, 0) == pytest.approx(np.log(2.0))
    with pytest.raises(ValueError, match="economy-reward share"):
        train_e2.economy_reward(_economy(), 0)


def test_economy_report_per_start_round():
    rng = np.random.default_rng(1)
    games = []
    for start in (0, 0, 100):
        g = _economy(0.5, start=start, end=start + 60)
        g["ground"]["economy"]["potential"] = [e2_fake.potential_seat(0, rng)]
        games.append(g)
    rep = train_e2.economy_report(games + [_battle("siege")])
    assert list(rep) == ["0", "100", "all"] and rep["0"]["seats"] == 2 and rep["all"]["seats"] == 3
    s = games[2]["ground"]["economy"]["potential"][0]
    assert rep["100"]["potential"]["p50"] == pytest.approx(s["net"]["total"] / s["expert"]["total"])
    sci = rep["100"]["components"]["science"]
    assert sci["ratio"] == pytest.approx(s["net"]["components"]["science"]["raw"] / 20.0)
    assert sci["gap"] == pytest.approx((s["net"]["components"]["science"]["value"] - s["expert"]["components"]["science"]["value"]) / s["expert"]["total"])
    assert sum(c["gap"] for c in rep["100"]["components"].values()) == pytest.approx(rep["100"]["potential"]["p50"] - 1.0), "the gaps add up to the potential's"
    lines = []
    train_e2.log_economy(rep, lines.append)
    assert len(lines) == 6 and lines[0].startswith("  economy R=0: 2 seats")


def test_e2_shard_last_value_and_targets(tmp_path):
    d = synthetic.write_shard(tmp_path / "v.shard", 5, 12, 8, rounds=4, players=2)
    e2_fake.add_e2(d, {"temperature": 1.0, "group": 0, "member": 0, "group_size": 1, "learners": [0, 1], "seats": []})
    turns = np.load(d / "turns.npy")
    value = (turns[:, 0] * 10 + turns[:, 1]).astype("<i4")
    value[(turns[:, 1] == 1)] = train_e2.MISSING_VALUE
    np.save(d / "e2_value.npy", value)
    s = train_e2.E2Shard(d)
    last = int(turns[turns[:, 1] == 0, 0].max())
    assert s.last_value(0) == pytest.approx((last * 10) / 256.0), "the value at the player's last turn, Q8"
    assert s.last_value(1) is None and s.last_value(7) is None
    s.set_targets({0: (0.3, 1.0, None, float("nan")), 1: (0.6, -1.0, None, 0.25)}, advantage="gae")
    for t in range(len(s)):
        turn = s.turn(t)
        if int(s.turns[t, 1]) == 0:
            assert np.isnan(turn.value_target) and (turn.dec_arrays["e2_adv"] == 1.0).all(), "no value target: the game's advantage, not GAE"
        else:
            assert turn.value_target == 0.25


def test_pfsp_weights():
    shares = {"standard": 0.5, "tall-book": 0.25, "league": 0.25}
    w = train_e2.pfsp_weights(shares, {"standard": 0.9, "tall-book": 0.1, "league": 0.5}, power=2, floor=0.05)
    assert sum(w.values()) == pytest.approx(1.0)
    assert w["tall-book"] > w["league"] > w["standard"], "the opponent the learner loses to most is played most"
    raw = {"standard": 0.5 * max(0.1 ** 2, 0.05), "tall-book": 0.25 * 0.9 ** 2, "league": 0.25 * 0.25}
    assert w["standard"] == pytest.approx(raw["standard"] / sum(raw.values())), "the floor keeps a beaten opponent in the mix"
    assert train_e2.pfsp_weights(shares, {}) == pytest.approx(shares), "no results yet: the configured shares"
    assert train_e2.pfsp_weights({"a": 1.0, "b": 1.0}, {"a": 1.0, "b": 1.0}, floor=0.0) == {"a": 1.0, "b": 1.0}


def test_plan_with_grounds(tmp_path):
    loop = object.__new__(e2_loop.Loop)
    loop.a = argparse.Namespace(groups=10, round0_every=0, round0_share=0.0, give_share=0.0, seed=0, opponent_seats=3, battle_rounds="20-40",
                                battle_army="6-10", economy_rounds="0,0,50,100", economy_window="50-80", economy_opening_window=60, opponents_pfsp=True,
                                pfsp_power=2.0, pfsp_floor=0.05, group_size=4, ground_group_size=8)
    loop.ckpt_dir = tmp_path
    loop.midgame, loop.give = True, False
    loop.state = {"pfsp": {"standard": 0.9, "tall-book": 0.1}}
    loop.suites = e2_loop.parse_mix("standard:0.3,duel:0.3,battle:0.2,economy:0.2")
    loop.opponents = e2_loop.parse_mix("standard:0.5,tall-book:0.5")
    loop.battle_kinds = e2_loop.parse_mix("field:0.25,siege:0.25,defence:0.2,rush-defence:0.3")
    runs = loop.plan(1)
    battle = {r["name"]: r for r in runs if r["suite"] == "battle"}
    assert sum(r["groups"] for r in battle.values()) == 2 and all(r["opponents"] == ["standard"] for r in battle.values())
    assert all(r["group_size"] == (8 if r["suite"] in ("battle", "economy") else 4) for r in runs), "grounds in groups of 8, whole games of 4"
    assert all(r["extra"][:2] == ["--battle", r["name"][len("battle-"):]] for r in battle.values())
    eco = [r for r in runs if r["suite"] == "economy"]
    assert sum(r["groups"] for r in eco) == 2 and all(r["pool"] == "economy" and "--window" in r["extra"] for r in eco)
    assert all(r["extra"][r["extra"].index("--from-rounds") + 1] == "0,0,50,100" and r["extra"][-2:] == ["--opening-window", "60"] for r in eco)
    # PFSP: the learner wins 90 % against standard, 10 % against tall-book: tall-book takes most groups.
    by_opp = {}
    for r in runs:
        if r["opponent"]:
            by_opp[r["opponent"]] = by_opp.get(r["opponent"], 0) + r["groups"]
    assert by_opp["tall-book"] > by_opp.get("standard", 0)


def test_loop_with_grounds_and_pfsp(tmp_path, monkeypatch):
    monkeypatch.setenv("PYTHONPATH", str(Path(__file__).resolve().parents[1]))
    layout = Shard(synthetic.write_shard(tmp_path / "l.shard", 0, 12, 8, rounds=2)).layout
    torch.manual_seed(0)
    init = tmp_path / "init.pt"
    save_checkpoint(init, Net(layout.config("small"), layout))
    fakes = _fakes(tmp_path)
    logs = tmp_path / "expert"
    logs.mkdir()
    state = tmp_path / "run"
    argv = _loop_args(state, init, fakes, 1, ["--groups", "10", "--suites", "standard:0.3,duel:0.3,battle:0.2,economy:0.2", "--opponents-pfsp",
                                              "--economy-logs", str(logs), "--battle-eval-games", "4", "--from-logs", ""])
    e2_loop.main(argv)
    it = [json.loads(line) for line in open(state / "iterations.jsonl")]
    assert len(it) == 1
    rec = it[0]
    assert set(rec["play_by_suite"]) == {"standard", "duel", "battle", "economy"}
    assert all(v["games"] > 0 and v["games_per_hour"] > 0 for v in rec["play_by_suite"].values())
    assert {k for k in rec["by_suite"] if k.startswith("battle/")} and "economy" in rec["by_suite"]
    assert rec["economy"]["all"]["seats"] == rec["by_suite"]["economy"]["n"], "every economy seat against its expert"
    assert -1.0 <= rec["by_suite"]["economy"]["min"] <= rec["by_suite"]["economy"]["max"] <= 1.0, "the potential's clipped log ratio"
    assert all(0.0 <= v["min"] <= v["max"] <= 1.0 for k, v in rec["by_suite"].items() if k.startswith("battle/"))
    assert set(rec["pfsp"]["weights"]) == {"standard", "league"} and sum(rec["pfsp"]["weights"].values()) == pytest.approx(1.0)
    st = json.loads((state / "state.json").read_text())
    kinds = st["evals"]["1"]["battle"]["kinds"]
    assert set(kinds) == set(train_e2.BATTLE_KINDS) and all(v["games"] == 4 and 0 <= v["win_share"] <= 1 for v in kinds.values())
    rows = list(csv.DictReader(open(state / "progress.csv")))
    assert rows[0]["eval_battle_siege"] != "" and rows[0]["reward_battle"] != "" and rows[0]["reward_economy"] != ""
    played = set(rec["battle_outcomes"])
    assert played and set(rec["curriculum"]["knobs"]) == played, "the next knobs of the kinds played"
    assert all(rows[0][f"knob_{k.replace('-', '_')}"] == "1.0" for k in played), "the knobs played: the documented setting at first"
    assert rec["siege_shaping"] == 0.45
    # Dr. GRPO with running scales: per suite and battle kind, kept in the state; battles in groups of 8, per turn.
    adv = rec["advantages"]
    assert {k for k in adv if k.startswith("battle/")} == {f"battle/{k}" for k in played}
    assert all(0.0 <= v["zero_share"] <= 1.0 and v["groups"] > 0 for v in adv.values())
    assert set(st["adv_scale"]) == {k for k, v in adv.items() if v["raw_rms"] == v["raw_rms"]}, "a scale for every key with data"
    assert rows[0]["zero_signal"] != "" and rows[0]["entropy"] != ""


def test_battle_curriculum_moves_the_knobs_toward_even():
    loop = object.__new__(e2_loop.Loop)
    loop.a = argparse.Namespace(curriculum_rate=1.0, curriculum_decay=0.5)
    loop.state = {}
    loop.log = lambda msg: None
    out = lambda win, captures=0: {"battle_outcomes": {"field": {"games": 4, "wins": 4 * win, "win_share": win, "captures": 0, "difficulty": 1.0},
                                                       "siege": {"games": 4, "wins": captures, "win_share": captures / 4, "captures": captures, "difficulty": 1.0}}}
    assert loop.knob("field") == 1.0, "the documented setting before any result"
    loop.update_curriculum(out(1.0))
    assert loop.knob("field") == pytest.approx(2.0) and loop.knob("siege") == pytest.approx(0.5), "the net wins every field battle and takes no city"
    assert loop.state["curriculum"]["siege_shaping"] == pytest.approx(0.45), "no capture: the shaping at its most"
    loop.update_curriculum(out(0.5, captures=4))
    assert loop.state["curriculum"]["win"]["field"] == pytest.approx(0.75), "an exponential average of the win share"
    assert loop.knob("field") > 2.0 and loop.knob("siege") == pytest.approx(0.5), "an even average leaves the knob"
    assert loop.state["curriculum"]["siege_shaping"] == pytest.approx(0.2), "captures half the time: the shaping at its least"
    for _ in range(20):
        loop.update_curriculum(out(1.0, captures=4))
    assert loop.knob("field") == e2_loop.KNOB_RANGE[1] == 8.0, "within the knob's range"


def test_staged_curriculum_moves_the_stage_first():
    """The reverse curriculum: the stage moves back while the learner wins
    and forward while it loses, the knob held at 1; the knob moves only at the
    ends (below 1 at stage 0, above 1 at the last stage) and returns to 1
    before the stage moves again."""
    step = lambda knob, stage, win: e2_loop.staged_curriculum(knob, stage, win, 1.0, 0.6, 0.3, 3)
    assert step(1.0, 0, 0.7) == (1.0, 1, True), "winning: one round of march back"
    assert step(1.0, 1, 0.55) == (1.0, 1, False), "above even but short of the advance: hold"
    assert step(1.0, 2, 0.2) == (1.0, 1, True), "losing: one stage forward"
    assert step(1.0, 2, 0.4) == (1.0, 2, False), "within the band: hold"
    knob, stage, moved = step(1.0, 0, 0.0)
    assert stage == 0 and not moved and knob == pytest.approx(0.5), "losing at the walls: the knob falls below 1"
    assert step(0.5, 0, 1.0) == (1.0, 0, False), "winning again: the knob back to 1 before the stage moves"
    knob, stage, moved = step(1.0, 3, 1.0)
    assert stage == 3 and knob == pytest.approx(2.0), "winning at the last stage: the knob rises"
    assert step(2.0, 3, 0.0) == (1.0, 3, False), "losing there: the knob back to 1 first"
    assert step(0.125, 0, 0.0)[0] == e2_loop.KNOB_RANGE[0], "within the knob's range"
    loop = object.__new__(e2_loop.Loop)
    loop.a = argparse.Namespace(curriculum_rate=1.0, curriculum_decay=0.5, stage_start=0, stage_advance=0.6, stage_retreat=0.3, stage_last=3)
    loop.stage_kinds = {"siege"}
    loop.state = {}
    loop.log = lambda msg: None
    out = lambda win: {"battle_outcomes": {k: {"games": 4, "wins": 4 * win, "win_share": win, "captures": 4 * win if k == "siege" else 0, "difficulty": 1.0}
                                           for k in ("field", "siege")}}
    assert (loop.stage("siege"), loop.stage("field")) == (0, None), "a staged kind starts at --stage-start; the others have none"
    loop.update_curriculum(out(1.0))
    assert loop.stage("siege") == 1 and loop.knob("siege") == 1.0 and loop.knob("field") == pytest.approx(2.0), "the stage moves, not the knob"
    assert loop.played_stages == {"siege": 0} and "siege" not in loop.state["curriculum"]["win"], "a new stage averages its win share anew"
    loop.update_curriculum(out(0.0))
    assert loop.stage("siege") == 0 and loop.knob("siege") == 1.0


def test_siege_same_turn_capture_pays_and_counts():
    """--siege-speed-decay: a capture in the learner's first turn pays 1, a
    turn later 0.75 at 0.5 (the old reward without it); battle_outcomes counts
    a same-turn scene's win only for a first-turn capture (first_turn_share),
    other scenes by any capture; --rung-success first-turn moves the ladder
    by it."""
    def game(captured, same):
        b = {"learner": 0, "enemy": 1, "start_round": 50, "length": 20, "end_round": 52, "captured_round": captured, "target_damage": 0.5,
             "learner_army": {"kinds": [1], "value": 10, "lost_units": 0, "lost_value": 0.0}, "enemy_army": {"kinds": [2], "value": 10, "lost_units": 0, "lost_value": 0.0},
             "difficulty": 1.0, "reward": 0.0, "same_turn": same}
        return {"ground": {"suite": "battle", "kind": "siege", "battle": b, "edits": []}}
    assert train_e2.battle_reward(game(50, True), speed_decay=0.5) == 1.0
    assert train_e2.battle_reward(game(51, True), speed_decay=0.5) == pytest.approx(0.75)
    assert train_e2.battle_reward(game(51, True)) == pytest.approx(0.5 + 0.5 * (1 - 1 / 20)), "the old reward by default"
    o = train_e2.battle_outcomes([game(50, True), game(51, True), game(52, False), game(None, True)])["siege"]
    assert (o["captures"], o["first_turn_wins"], o["win_share"]) == (3, 2, 0.75), "the 2nd turn's capture is no first-turn win; a scene without the guarantee counts any"
    loop = object.__new__(e2_loop.Loop)
    loop.a = argparse.Namespace(curriculum_rate=1.0, curriculum_decay=0.5, stage_start=0, stage_advance=0.6, stage_retreat=0.3, stage_last=4,
                                siege_ladder=True, rung_start=0, rung_advance=0.9, rung_retreat=0.4, rung_success="first-turn")
    loop.stage_kinds, loop.state, loop.log = set(), {}, (lambda msg: None)
    loop.ladder_teacher = (1.0,)
    loop.update_curriculum({"battle_outcomes": {"siege": dict(o, difficulty=1.0)}})
    assert loop.rung("siege") == 0, "50 % first-turn wins < 0.9: no step up although 75 % captured"
    assert e2_loop.ladder_teacher("", True, True) == e2_loop.LADDER_TEACHER_VARY_FIRST


def test_siege_ladder_moves_by_the_teachers_share():
    """--siege-ladder: the rung moves up at advance × the teacher's capture
    share on it and down below retreat × it, nothing between; the knob stays
    below the last rung and rises there; the plan passes --battle-rung, not
    --battle-stage; a new rung averages its win share anew."""
    step = lambda knob, rung, win, t: e2_loop.ladder_curriculum(knob, rung, win, t, 1.0, 0.9, 0.4, 13)
    assert step(1.0, 0, 0.9, 1.0) == (1.0, 1, True), "rung 0 passed only near the teacher's 100 %"
    assert step(1.0, 0, 0.85, 1.0) == (1.0, 0, False)
    assert step(1.0, 10, 0.2, 0.2) == (1.0, 11, True), "a hard rung where the teacher takes 20 %: 18 % is enough"
    assert step(1.0, 10, 0.07, 0.2) == (1.0, 9, True), "below 0.4 × the teacher's: one rung down"
    assert step(1.0, 10, 0.1, 0.2) == (1.0, 10, False), "between: hold"
    assert step(1.0, 0, 0.0, 1.0) == (1.0, 0, False), "rung 0 is the floor; the knob stays"
    assert step(1.0, 10, 0.0, 0.0) == (1.0, 9, True) and step(1.0, 10, 0.05, 0.0) == (1.0, 11, True), "a teacher at 0: still captures needed (the floor)"
    knob, rung, moved = step(1.0, 13, 0.5, 0.1)
    assert rung == 13 and not moved and knob > 1.0, "winning at the last rung: the knob rises"
    assert step(2.0, 13, 0.0, 0.1)[0:2] == (pytest.approx(2.0 * 2 ** (2 * -0.04)), 13), "losing there: the knob toward 1 first"
    assert e2_loop.ladder_teacher("") == e2_loop.LADDER_TEACHER and e2_loop.ladder_teacher("1,0.5") == (1.0, 0.5)
    loop = object.__new__(e2_loop.Loop)
    loop.a = argparse.Namespace(curriculum_rate=1.0, curriculum_decay=0.5, stage_start=0, stage_advance=0.6, stage_retreat=0.3, stage_last=4,
                                siege_ladder=True, rung_start=0, rung_advance=0.9, rung_retreat=0.4, battle_rounds="20-40", battle_army="6-10", groups=8,
                                seed=1, suites="battle:1", battle_kinds="siege:1")
    loop.stage_kinds = {"siege"}
    loop.ladder_teacher = (1.0, 0.5)
    loop.state = {}
    loop.log = lambda msg: None
    out = lambda win: {"battle_outcomes": {"siege": {"games": 8, "wins": 8 * win, "win_share": win, "captures": 8 * win, "difficulty": 1.0}}}
    assert loop.rung("siege") == 0 and loop.rung("field") is None and loop.ladder_last() == e2_loop.LADDER_RUNGS + 4
    loop.update_curriculum(out(0.95))
    assert loop.rung("siege") == 1 and loop.played_rungs == {"siege": 0} and loop.played_stages == {}, "the ladder in place of the stage"
    assert "siege" not in loop.state["curriculum"]["win"]
    loop.update_curriculum(out(0.3))
    assert loop.rung("siege") == 1, "0.3 ≥ 0.4 × 0.5 (the table's last entry past its end): hold"
    loop.update_curriculum(out(0.0))
    assert loop.rung("siege") == 0
    loop.a.__dict__.update(round0_every=0, round0_share=0.0, give_share=0.0, opponent_seats=3, opponents_pfsp=False, group_size=4, ground_group_size=8)
    loop.ckpt_dir = Path("/nonexistent")
    loop.midgame, loop.give = False, False
    loop.suites, loop.opponents = e2_loop.parse_mix("battle:1"), e2_loop.parse_mix("standard:1")
    loop.battle_kinds = e2_loop.parse_mix("siege:1")
    extra = next(r["extra"] for r in loop.plan(1) if r["name"] == "battle-siege")
    assert extra[extra.index("--battle-rung") + 1] == "0" and "--battle-minimal" in extra and "--battle-stage" not in extra, "the rung in place of the stage"
    assert "--battle-vary" not in extra
    loop.a.ladder_vary = True
    extra = next(r["extra"] for r in loop.plan(1) if r["name"] == "battle-siege")
    assert "--battle-vary" in extra and e2_loop.ladder_teacher("", True) == e2_loop.LADDER_TEACHER_VARY, "--ladder-vary: varied scenes, their teacher"
    loop.a.ladder_vary = False
    loop.a.siege_ladder = False
    assert loop.rung("siege") is None and loop.stage("siege") == 0, "without the flag: the stages as before"
    extra = next(r["extra"] for r in loop.plan(1) if r["name"] == "battle-siege")
    assert "--battle-rung" not in extra and extra[extra.index("--battle-stage") + 1] == "0"


def test_skirmish_rewards_and_ladder():
    """The skirmish (`grounds::SKIRMISH`): won when every enemy unit is dead;
    0.25 + 0.25 × (dealt − taken shares) + 0.5 × the win's worth (1 in the
    first round, toward 0.5 at the limit), per turn 0.25 × (d − t) and the
    win's half at the end; --skirmish-ladder moves its rung by absolute
    shares by default (up at 0.9, down below 0.4), the plan passes the rung
    with the minimal world and varied scenes; the eval and the defaults
    are as before without it."""
    won = _battle("skirmish", own=0.2, enemy=1.0)
    won["ground"]["battle"]["won_round"] = 101
    assert train_e2.battle_won(won) and not train_e2.battle_won(_battle("skirmish", enemy=0.9))
    assert train_e2.battle_reward(won) == pytest.approx(0.25 + 0.25 * 0.8 + 0.5), "a win in the war's first round: its whole worth"
    won["ground"]["battle"]["won_round"] = 131
    assert train_e2.battle_reward(won) == pytest.approx(0.25 + 0.25 * 0.8 + 0.5 * (1 - 0.5 * 30 / 30)), "at the limit: half"
    assert train_e2.battle_reward(_battle("skirmish", own=1.0)) == 0.0 and train_e2.battle_reward(_battle("skirmish")) == 0.25
    assert train_e2.suite_key(won) == "battle/skirmish" and train_e2.SKIRMISH not in train_e2.BATTLE_KINDS
    g = _battle_game("skirmish", 0, 0, enemy_lost=[100, 150, 200], learner_lost=[0, 20, 20], value=200.0)
    g["ground"]["battle"]["won_round"] = 12
    rounds, r = train_e2.battle_turn_rewards(g)
    assert list(rounds) == [10, 11] and np.allclose(r, [0.25 * (50 - 20) / 200, 0.25 * 50 / 200 + 0.5 * (1 - 0.5 * 1 / 30)])
    assert r.sum() == pytest.approx(train_e2.battle_reward(g) - 0.25 - 0.25 * 100 / 200), "the end reward less its constant and the start's wounds"
    assert e2_loop.skirmish_teacher("") == (1.0,) * e2_loop.SKIRMISH_RUNGS and e2_loop.skirmish_teacher("standard") == e2_loop.SKIRMISH_STANDARD
    loop = object.__new__(e2_loop.Loop)
    loop.a = argparse.Namespace(curriculum_rate=1.0, curriculum_decay=0.0, stage_start=0, stage_advance=0.6, stage_retreat=0.3, stage_last=4,
                                siege_ladder=False, skirmish_ladder=True, skirmish_rung_start=0, rung_start=0, rung_advance=0.9, rung_retreat=0.4,
                                battle_rounds="20-40", battle_army="6-10", groups=8, seed=1, round0_every=0, round0_share=0.0, give_share=0.0, opponent_seats=3,
                                opponents_pfsp=False, group_size=4, ground_group_size=8)
    loop.stage_kinds = set()
    loop.skirmish_teacher = e2_loop.skirmish_teacher("")
    loop.state = {}
    loop.log = lambda msg: None
    out = lambda win: {"battle_outcomes": {"skirmish": {"games": 8, "wins": 8 * win, "win_share": win, "captures": 0, "difficulty": 1.0}}}
    assert loop.rung("skirmish") == 0 and loop.rung("siege") is None and loop.ladder_last("skirmish") == e2_loop.SKIRMISH_RUNGS - 1
    loop.update_curriculum(out(0.85))
    assert loop.rung("skirmish") == 0, "0.85 < 0.9: absolute, not standard's share"
    loop.update_curriculum(out(0.9))
    assert loop.rung("skirmish") == 1 and loop.played_rungs == {"skirmish": 0}
    loop.update_curriculum(out(0.45))
    assert loop.rung("skirmish") == 1, "between 0.4 and 0.9: hold"
    loop.update_curriculum(out(0.3))
    assert loop.rung("skirmish") == 0
    loop.ckpt_dir = Path("/nonexistent")
    loop.midgame, loop.give = False, False
    loop.suites, loop.opponents = e2_loop.parse_mix("battle:1"), e2_loop.parse_mix("standard:1")
    loop.battle_kinds = e2_loop.parse_mix("skirmish:1")
    extra = next(r["extra"] for r in loop.plan(1) if r["name"] == "battle-skirmish")
    assert extra[:2] == ["--battle", "skirmish"] and extra[extra.index("--battle-rung") + 1] == "0" and "--battle-minimal" in extra and "--battle-vary" in extra
    loop.a.skirmish_ladder = False
    extra = next(r["extra"] for r in loop.plan(1) if r["name"] == "battle-skirmish")
    assert "--battle-rung" not in extra and "--battle-vary" in extra, "without the ladder: the binary's rung, six a side"
    assert "rung_skirmish" in e2_loop.CSV_FIELDS and "eval_battle_skirmish" in e2_loop.CSV_FIELDS


def test_battle_window_rewards_and_ladder():
    """The battle window (`clash`): Φ = (the enemy's value destroyed + the
    learner's ready force) / its army value per round; the reward 0.5 + 0.5
    × ΔΦ, per turn 0.5 × the change; a whole unit traded for little damage
    loses, finishing a wounded one pays; --clash-ladder moves its advantage
    rung by the share of windows that gain value, absolute by default; the
    plan passes the window's armies, rounds and rung (off the ladder: the
    stage)."""
    g = _battle_game("clash", 0, 0, enemy_lost=[0, 30, 90], learner_lost=[0, 40, 100], value=200.0)
    g["ground"]["battle"]["rounds"]["learner_ready"] = [200.0, 170.0, 100.0]
    phi = train_e2.clash_phi(g["ground"]["battle"])
    assert np.allclose(phi, [1.0, 1.0, 0.95])
    rounds, r = train_e2.battle_turn_rewards(g)
    assert list(rounds) == [10, 11] and np.allclose(r, [0.0, -0.025])
    assert train_e2.battle_reward(g) == pytest.approx(0.475) and not train_e2.battle_won(g), "a whole unit (100 of ready) for 60 of damage: a loss"
    g["ground"]["battle"]["rounds"]["learner_ready"] = [200.0, 170.0, 170.0]
    assert train_e2.battle_reward(g) == pytest.approx(0.5 + 0.5 * 0.3) and train_e2.battle_won(g), "the second blow finished a wounded one at no cost"
    assert e2_loop.clash_teacher("") == (1.0,) * e2_loop.CLASH_RUNGS and e2_loop.clash_teacher("standard") == e2_loop.CLASH_STANDARD
    loop = object.__new__(e2_loop.Loop)
    loop.a = argparse.Namespace(curriculum_rate=1.0, curriculum_decay=0.0, stage_start=0, stage_advance=0.6, stage_retreat=0.3, stage_last=4, siege_ladder=False,
                                clash_ladder=True, clash_rung_start=0, clash_stage_start=0, rung_advance=0.9, rung_retreat=0.4, clash_army="8-14",
                                clash_rounds="4-5", battle_rounds="20-40", battle_army="6-10", groups=8, seed=1, round0_every=0, round0_share=0.0, give_share=0.0,
                                opponent_seats=3, opponents_pfsp=False, group_size=4, ground_group_size=8)
    loop.stage_kinds = set()
    loop.clash_teacher = e2_loop.clash_teacher("")
    loop.battle_kinds = e2_loop.parse_mix("clash:1")
    loop.state = {}
    loop.log = lambda msg: None
    out = lambda win: {"battle_outcomes": {"clash": {"games": 8, "wins": 8 * win, "win_share": win, "captures": 0, "difficulty": 1.0, "mean_reward": 0.6}}}
    assert loop.rung("clash") == 0 and loop.ladder_last("clash") == e2_loop.CLASH_RUNGS - 1
    loop.update_curriculum(out(0.85))
    assert loop.rung("clash") == 0, "absolute: 0.85 < 0.9"
    loop.update_curriculum(out(0.95))
    assert loop.rung("clash") == 1 and loop.played_rungs == {"clash": 0}
    loop.ckpt_dir = Path("/nonexistent")
    loop.midgame, loop.give = False, False
    loop.suites, loop.opponents = e2_loop.parse_mix("battle:1"), e2_loop.parse_mix("standard:1")
    extra = next(r["extra"] for r in loop.plan(1) if r["name"] == "battle-clash")
    get = lambda k: extra[extra.index(k) + 1]
    assert (get("--battle"), get("--battle-army"), get("--battle-rounds"), get("--battle-rung")) == ("clash", "8-14", "4-5", "1")
    assert "--battle-stage" not in extra and "--battle-minimal" in extra and "--battle-vary" in extra and "rung_clash" in e2_loop.CSV_FIELDS
    loop.a.clash_ladder, loop.a.clash_stage_start = False, 2
    extra = next(r["extra"] for r in loop.plan(1) if r["name"] == "battle-clash")
    assert "--battle-rung" not in extra and extra[extra.index("--battle-stage") + 1] == "2", "off the ladder: even at the stage"


def test_reference_log_probs_are_made_once_per_batch(tmp_path):
    layout = Shard(synthetic.write_shard(tmp_path / "l.shard", 0, 12, 8, rounds=2)).layout
    torch.manual_seed(2)
    model = Net(layout.config("small"), layout)
    ref = Net(layout.config("small"), layout).eval()
    d, _ = _behaviour_shard(tmp_path, model)
    shard = train_e2.E2Shard(d)
    shard.set_targets({0: (1.0, 0.5, None), 1: (0.0, -0.5, None)})
    a = args("--beta", "0.5")
    calls = []
    real = ref.embed
    ref.embed = lambda batch: (calls.append(1), real(batch))[1]

    def two_epochs(reuse):
        m = copy.deepcopy(model)
        opt = torch.optim.SGD(m.parameters(), lr=0.0)  # no step: both epochs see the same weights
        prepared = train_e2.E2Prepared(train_e2.window_turns([shard]), m.cfg, 3)
        out = []
        for epoch in range(2):
            calls.clear()
            stats = train_e2.Stats()
            train_e2.run_e2(m, ref, prepared, torch.device("cpu"), a, stats, opt, {}, epoch == 0, reuse_ref=reuse)
            out.append((stats.read(), len(calls)))
        return out

    cached, fresh = two_epochs(True), two_epochs(False)
    assert cached[0][1] > 0 and cached[1][1] == 0, "the second epoch runs no reference pass"
    assert fresh[1][1] == fresh[0][1], "without reuse every epoch runs it"
    assert cached[1][0]["kl"] > 1e-4, "a reference other than the model"
    for key in ("kl", "entropy", "surrogate", "ratio_mean"):
        assert cached[1][0][key] == pytest.approx(fresh[1][0][key], rel=1e-6, abs=1e-9), key


def test_gpu_lock_is_the_lock_of_gpu_run(tmp_path):
    path = tmp_path / "gpu.lock"
    probe = lambda: __import__("subprocess").run(["/usr/bin/lockf", "-t", "0", str(path), "true"], capture_output=True).returncode
    with e2_loop.gpu_lock(str(path)) as waited:
        assert waited < 1.0
        assert probe() != 0, "lockf (gpu-run.sh) cannot take the lock the loop holds"
    assert probe() == 0, "released after the block"
    with e2_loop.gpu_lock("") as waited:
        assert waited == 0.0


# -- the main exploiter (e2_exploiter.py) -----------------------------------------


def test_exploiter_promotion_rule_and_league_mix():
    from aurelia_nn import e2_exploiter as ex

    a = argparse.Namespace(exploiter_window=3, exploiter_promote=0.7, exploiter_max_iterations=10)
    h = [{"games": 8, "share": 0.9}, {"games": 8, "share": 0.9}]
    assert ex.decide(h, 2, a) is None, "a window not full yet: never promoted"
    assert ex.recent_share(h, 3) == pytest.approx(0.9) and ex.recent_share(h, 3, full=True) is None
    h.append({"games": 16, "share": 0.4})
    assert ex.recent_share(h, 3, full=True) == pytest.approx((8 * 0.9 * 2 + 16 * 0.4) / 32), "games-weighted"
    assert ex.decide(h, 3, a) is None
    h.append({"games": 16, "share": 1.0})
    assert ex.decide(h, 4, a) == "promote", "the last three: (7.2 + 6.4 + 16) / 40 = 0.74"
    assert ex.decide([{"games": 8, "share": 0.1}] * 3, 10, a) == "timeout"
    assert ex.decide([{"games": 8, "share": 0.1}] * 3, 10, argparse.Namespace(**{**vars(a), "exploiter_max_iterations": 0})) is None
    mix = {"standard": 0.75, "league": 0.25}
    assert ex.league_mix(mix, [], 0.2) == mix, "no exploiter promoted: the mix as configured"
    m = ex.league_mix(mix, [{"name": "exploiter-1"}, {"name": "exploiter-2"}], 0.2)
    assert m == pytest.approx({"standard": 0.6, "league": 0.2, "exploiter-1": 0.1, "exploiter-2": 0.1})


def test_exploiter_plan_and_main_mix(tmp_path):
    from aurelia_nn import e2_exploiter as ex

    loop = object.__new__(e2_loop.Loop)
    loop.a = argparse.Namespace(groups=8, round0_every=0, round0_share=0.0, give_share=0.0, seed=0, opponent_seats=3, opponents_pfsp=True,
                                pfsp_power=2.0, pfsp_floor=0.05, group_size=4, exploiter_groups=6, exploiter_suites="duel:0.5,standard:0.5",
                                exploiter_window=3, exploiter_fallback=0.2, exploiter_league_share=0.2)
    loop.ckpt_dir = tmp_path
    loop.midgame = loop.give = False
    loop.suites, loop.opponents = {"duel": 1.0}, e2_loop.parse_mix("standard:1")
    for i in (0, 1, 2):
        (tmp_path / f"iter_{i:04d}.bin").write_bytes(b"")
    league = tmp_path / "exploiter-1.bin"
    league.write_bytes(b"")
    loop.state = {"pfsp": {"standard": 0.2}, "exploiter": {"history": [{"games": 8, "share": 0.6}], "promoted": [{"name": "exploiter-1", "bin": str(league)}]}}
    assert loop.main_opponents() == pytest.approx({"standard": 0.8, "exploiter-1": 0.2})
    runs = loop.plan(3)
    vs = [r for r in runs if r["opponent"] == "exploiter-1"]
    assert vs and all(r["opponents"] == [f"net:{league}"] for r in vs), "the promoted exploiter as a net opponent of the main learner"
    # The exploiter's runs: against the current main net (iter_{i-1}); a weak one plays a past main half the time.
    e = object.__new__(ex.MainExploiter)
    e.loop, e.a, e.suites = loop, loop.a, e2_loop.parse_mix(loop.a.exploiter_suites)
    runs = {r["name"]: r for r in e.plan(3)}
    assert set(runs) == {"exploiter-duel-main", "exploiter-standard-main"} and sum(r["groups"] for r in runs.values()) == 6
    assert runs["exploiter-duel-main"]["opponents"] == [f"net:{tmp_path / 'iter_0002.bin'}"]
    assert runs["exploiter-standard-main"]["opponents"] == ["standard", "standard", f"net:{tmp_path / 'iter_0002.bin'}"]
    loop.state["exploiter"]["history"] = [{"games": 8, "share": 0.1}]
    runs = {r["name"]: r for r in e.plan(3)}
    assert runs["exploiter-duel-past"]["groups"] == 1 and runs["exploiter-duel-main"]["groups"] == 2
    assert runs["exploiter-duel-past"]["opponents"][0] in (f"net:{tmp_path / 'iter_0000.bin'}", f"net:{tmp_path / 'iter_0001.bin'}")
    loop.state = {}
    assert loop.main_opponents() == {"standard": 1.0}, "no exploiter state: the configured mix"


def _exploiter_args(state, init, fakes, iterations):
    return _loop_args(state, init, fakes, iterations, ["--opponents", "standard:0.5,league:0.5", "--suites", "duel:1", "--eval-every", "0", "--pace-every", "0",
                                                       "--opponents-pfsp", "--from-logs", "", "--exploiter-groups", "2", "--exploiter-window", "2",
                                                       "--exploiter-promote", "0.6", "--exploiter-max-iterations", "6", "--exploiter-league-share", "0.6"])


def test_loop_with_exploiter_promotes_and_resumes(tmp_path, monkeypatch):
    monkeypatch.setenv("PYTHONPATH", str(Path(__file__).resolve().parents[1]))
    monkeypatch.setenv("E2_FAKE_NET_WIN", "0.75")
    threads = torch.get_num_threads()
    torch.set_num_threads(1)
    try:
        _exploiter_check(tmp_path)
    finally:
        torch.set_num_threads(threads)


def _exploiter_check(tmp_path):
    layout = Shard(synthetic.write_shard(tmp_path / "l.shard", 0, 12, 8, rounds=2)).layout
    torch.manual_seed(0)
    init = tmp_path / "init.pt"
    save_checkpoint(init, Net(layout.config("small"), layout))
    fakes = _fakes(tmp_path)
    # The defaults: no exploiter, nothing of it in the state.
    off = tmp_path / "off"
    e2_loop.main(_loop_args(off, init, fakes, 1, ["--eval-every", "0", "--pace-every", "0", "--from-logs", ""]))
    assert "exploiter" not in json.loads((off / "state.json").read_text()) and not (off / "exploiter").exists() and not (off / "exploiter.csv").exists()

    straight = tmp_path / "straight"
    e2_loop.main(_exploiter_args(straight, init, fakes, 4))
    resumed = tmp_path / "resumed"
    e2_loop.main(_exploiter_args(resumed, init, fakes, 2))
    # A crash after both plays of iteration 3: their games are kept, the training redone.
    loop = e2_loop.Loop(_parse(_exploiter_args(resumed, init, fakes, 3)))
    loop.play(3)
    loop.exploiter.play(3)
    e2_loop.main(_exploiter_args(resumed, init, fakes, 4))

    states = []
    for run in (straight, resumed):
        st = json.loads((run / "state.json").read_text())
        ex = st["exploiter"]
        states.append(ex)
        assert st["iteration"] == ex["iteration"] == 4
        rows = list(csv.DictReader(open(run / "exploiter.csv")))
        assert [r["iteration"] for r in rows] == ["1", "2", "3", "4"]
        assert all(int(r["games_main"]) == 4 for r in rows), "2 groups of 2 duels against the main net"
        promoted = [r for r in rows if r["event"] == "promote"]
        assert promoted and promoted[0]["iteration"] == "2", "the first full window (0.75 of the duels won) promotes"
        assert ex["promoted"][0]["name"] == "exploiter-1" and (run / "league/exploiter-1.bin").exists() and (run / "league/exploiter-1.pt").exists()
        assert ex["generation"] == 1 + len(promoted) and ex["born"] == int(promoted[-1]["iteration"]), "reset after each promotion"
        assert sorted(p.name for p in (run / "exploiter").glob("iter_*")) == ["iter_0004.bin", "iter_0004.pt"]
        assert not any((run / "games").iterdir()), "both learners' games deleted after training"
        it = [json.loads(line) for line in open(run / "iterations.jsonl")]
        assert all("exploiter" in r for r in it) and len(it) == 4
        # From iteration 3 on, the main learner plays the promoted exploiter (PFSP over its mix).
        assert "exploiter-1" in it[2]["pfsp"]["weights"] and "exploiter-1" not in it[0]["pfsp"]["weights"]
        assert any(k.endswith("-exploiter-1") for k in it[2]["by_run"]), "games of the main learner against exploiter-1"
        assert "exploiter-1" in st["pfsp"]
    # The reset weights: the start's, the trained ones frozen in the league.
    init_state = torch.load(init, weights_only=False)["state"]
    frozen = torch.load(straight / "league/exploiter-1.pt", weights_only=False)["state"]
    assert max(float((frozen[k].float() - init_state[k].float()).abs().max()) for k in frozen) > 1e-3, "the promoted copy had trained"
    assert {k: v for k, v in states[0].items() if k != "promoted"} == {k: v for k, v in states[1].items() if k != "promoted"}
    for name in ("exploiter/iter_0004.pt", "league/exploiter-1.pt"):
        a = torch.load(straight / name, weights_only=False)["state"]
        b = torch.load(resumed / name, weights_only=False)["state"]
        assert max(float((a[k].float() - b[k].float()).abs().max()) for k in a) < 1e-6, name


# -- the opening ground ----------------------------------------------------------


def _opening_game(group, member, explored, potential, lost=None, player=0, cities=None):
    n = len(explored)
    seat = {"player": player, "learner": True, "tiles": 100, "rivals": 4, "city_states": 2, "start_value": 50, "prices": None,
            "explored": explored, "majors_met": [0] * n, "city_states_met": [0] * n, "rival_cities_seen": [0] * n, "rival_capitals_known": [0] * n,
            "camps_cleared": [0] * n, "units_lost": [0] * n, "lost_value": lost or [0] * n, "cities": cities or [0] + [1] * (n - 1), "potential": potential}
    return {"group": group, "member": member, "group_size": 2, "temperature": 1.0, "learners": [player], "spec": {"suite": "standard", "size": "small"},
            "seats": [{"player": player, "learner": True, "points_share": 0.0}],
            "ground": {"suite": "opening", "kind": "window", "edits": [], "opening": {"window": n - 1, "end_round": n - 1, "round": list(range(n)),
                                                                                      "seats": [seat], "reward": 0.0}}}


def test_opening_phi_terms_and_turn_rewards():
    """Φ: log(P + P0) with P0 the potential at the first city, the other terms
    shares of their normalisers, the units lost negated; the turn rewards
    telescope to the game's reward."""
    g = _opening_game(0, 0, [10, 20, 40, 50], [0.0, 100.0, 150.0, 300.0], lost=[0, 0, 25, 25])
    seat = train_e2.opening_seat(g, 0)
    terms = train_e2.opening_terms(seat)
    assert terms["potential"] == pytest.approx(np.log(np.array([0, 100, 150, 300]) + 100.0)), "P0: the potential at the capital's entry"
    assert terms["explored"] == pytest.approx([0.1, 0.2, 0.4, 0.5]) and terms["lost_value"] == pytest.approx([0, 0, -0.5, -0.5])
    w = train_e2.OPENING_WEIGHTS
    phi = train_e2.opening_phi(seat)
    assert phi[3] == pytest.approx(w["potential"] * np.log(400.0) + w["explored"] * 0.5 - w["lost_value"] * 0.5)
    rounds, r = train_e2.opening_turn_rewards(g, 0)
    assert list(rounds) == [0, 1, 2] and r.sum() == pytest.approx(train_e2.opening_reward(g, 0)) == pytest.approx(phi[3] - phi[0])
    assert r[1] < train_e2.OPENING_WEIGHTS["potential"] * np.log(250 / 200) + 0.2, "losing half the start's units costs the round"
    only = train_e2.parse_opening_weights("potential=0,explored=2")
    assert train_e2.opening_reward(g, 0, only) == pytest.approx(2 * 0.4 - w["lost_value"] * 0.5)
    with pytest.raises(SystemExit):
        train_e2.parse_opening_weights("tiles=1")
    # Without a city the potential term is log 1 throughout.
    assert train_e2.opening_terms(train_e2.opening_seat(_opening_game(0, 0, [1, 2], [0.0, 0.0], cities=[0, 0]), 0))["potential"] == pytest.approx([0, 0])


def test_plan_trains_openings_per_turn_without_the_potential_shaping():
    """An opening group's advantages are per turn (GAE against the
    leave-one-out reward-to-go, key `opening`); a group of identical
    members (a scripted learner) is zero-signal and dropped; the potential's
    shaping does not add to an opening (its Φ holds the potential)."""
    a = args("--adv-norm", "dr", "--zero-tol", "0.001", "--reward", "points_share", "--dense-potential", "0.5")
    games = {
        "o/a0": _opening_game(0, 0, [10, 30, 60], [0.0, 100.0, 120.0]),
        "o/a1": _opening_game(0, 1, [10, 15, 20], [0.0, 100.0, 110.0]),
        "o/b0": _opening_game(1, 0, [10, 20, 30], [0.0, 100.0, 120.0]),
        "o/b1": _opening_game(1, 1, [10, 20, 30], [0.0, 100.0, 120.0]),
    }
    plan = train_e2.plan_advantages(games, a, {})
    st = plan["stats"]["opening"]
    assert st["groups"] == 2 and st["zero_signal"] == 1 and plan["drop"] == {"o/b0", "o/b1"}
    assert set(plan["dense"]) == {("o/a0", 0), ("o/a1", 0)} and set(plan["dense"][("o/a0", 0)]) == {0, 1}
    assert plan["dense"][("o/a0", 0)][0] > 0 > plan["dense"][("o/a1", 0)][0], "the member that explored more"
    assert not plan["shaping"] and "opening/dense" not in plan["stats"]
    t = train_e2.game_targets([(k, games[k], None) for k in ("o/a0", "o/a1")], a, {}, {}, plan["dense"])
    assert math.isnan(t["o/a0"][0][3]), "no value target: an opening is not the whole game"
    assert t["o/a0"][0][0] == pytest.approx(train_e2.opening_reward(games["o/a0"], 0))


def test_plan_with_the_opening_ground(tmp_path):
    loop = object.__new__(e2_loop.Loop)
    loop.a = argparse.Namespace(groups=8, round0_every=0, round0_share=0.0, give_share=0.0, seed=0, opponent_seats=3, group_size=4, ground_group_size=8,
                                opening_window=0)
    loop.ckpt_dir = tmp_path
    loop.midgame, loop.give = True, False
    loop.suites, loop.opponents = e2_loop.parse_mix("standard:0.75,opening:0.25"), {"standard": 1.0}
    runs = {r["name"]: r for r in loop.plan(1)}
    o = runs["opening"]
    assert (o["groups"], o["group_size"], o["opponents"], o["extra"], o["opponent"]) == (2, 8, ["standard"], ["--opening", "0"], None)
    assert sum(r["groups"] for r in runs.values()) == 8


def test_loop_with_the_opening_ground(tmp_path, monkeypatch):
    monkeypatch.setenv("PYTHONPATH", str(Path(__file__).resolve().parents[1]))
    layout = Shard(synthetic.write_shard(tmp_path / "l.shard", 0, 12, 8, rounds=2)).layout
    torch.manual_seed(0)
    init = tmp_path / "init.pt"
    save_checkpoint(init, Net(layout.config("small"), layout))
    fakes = _fakes(tmp_path)
    state = tmp_path / "run"
    e2_loop.main(_loop_args(state, init, fakes, 1, ["--groups", "4", "--suites", "standard:0.5,opening:0.5", "--from-logs", "", "--eval-every", "0",
                                                     "--pace-every", "0", "--opening-window", "6"]))
    rec = json.loads(open(state / "iterations.jsonl").readline())
    assert set(rec["play_by_suite"]) == {"standard", "opening"} and rec["play_by_suite"]["opening"]["games"] == 2 * 8, "two groups of the ground size"
    assert "opening" in rec["by_suite"] and rec["advantages"]["opening"]["groups"] == 2
    assert "openings on the duel suite's maps, 6 rounds" in (state / "loop.log").read_text()


def test_staged_curriculum_band_uses_the_stage_thresholds():
    """--stage-band: the knob below 1 at stage 0 climbs at the advance
    threshold (not only above a half), holds between the thresholds, falls
    at the retreat threshold; the stage then moves at the same thresholds."""
    step = lambda knob, stage, win: e2_loop.staged_curriculum(knob, stage, win, 1.0, 0.35, 0.15, 3, band=True)
    knob, stage, moved = step(0.125, 0, 0.4)
    assert stage == 0 and not moved and knob == pytest.approx(0.125 * 2 ** (2 * (0.4 - 0.25))), "winning 40 %: harder"
    assert step(0.125, 0, 0.25) == (0.125, 0, False), "between the thresholds: hold"
    assert step(0.5, 0, 0.1)[0] == pytest.approx(0.5 * 2 ** (2 * (0.1 - 0.25))), "at or below the retreat: easier"
    assert step(0.9, 0, 0.6) == (1.0, 0, False), "the knob reaches 1 first"
    assert step(1.0, 0, 0.35) == (1.0, 1, True) and step(1.0, 2, 0.15) == (1.0, 1, True), "then the stage moves at the same thresholds"
    assert step(1.0, 3, 0.4)[0] > 1.0 and step(2.0, 3, 0.1)[0] == pytest.approx(2.0 * 2 ** (2 * (0.1 - 0.25)))
    # Without the band: the documented pivot at a half (run5's behaviour).
    assert e2_loop.staged_curriculum(0.125, 0, 0.4, 1.0, 0.35, 0.15, 3)[0] == e2_loop.KNOB_RANGE[0]


def test_a_book_on_another_base_is_a_known_opponent(monkeypatch):
    """`--opponents rush-siege@standard` (a book on today's standard, classic/docs/ai-books.md
    «Re-based»): known when both names are in `ai-tournament --list`; otherwise its share
    goes to standard."""
    listed = "classic variants: standard, pre-puppet, rush-siege, tall-book\n"
    monkeypatch.setattr(e2_loop.subprocess, "run", lambda *a, **k: argparse.Namespace(stdout=listed))
    loop = object.__new__(e2_loop.Loop)
    loop.a = argparse.Namespace(suites="duel", opponents="standard:0.5,rush-siege@standard:0.25,tall-book@nope:0.25", tournament_bin="ai-tournament")
    loop.log = lambda msg: None
    _, opponents = loop._mixes()
    assert opponents == {"standard": 0.75, "rush-siege@standard": 0.25}
