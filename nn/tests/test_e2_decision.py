"""The per-decision advantage of the E2 trainer (train_e2 `--decision-advantage`):
the search teacher's values of the net's own decisions (`ai-selfplay
--decision-values`, `.e2.json` `decision_values`, the shard's `e2_dvalues.npy`)."""

import argparse
import copy
import sys
from pathlib import Path

import numpy as np
import pytest

from aurelia_nn import synthetic, train_e2

sys.path.insert(0, str(Path(__file__).parent))
import e2_fake  # noqa: E402
from test_e2 import _battle_game  # noqa: E402


def args(*argv):
    ap = argparse.ArgumentParser()
    train_e2.add_arguments(ap)
    return ap.parse_args(list(argv) + ["--device", "cpu"])


def valued(game: dict, *qs) -> dict:
    """`game` with valued decisions: each `qs` item (Q chosen, [Q of every candidate valued])."""
    g = copy.deepcopy(game)
    values = [{"player": 0, "round": 10, "ordinal": i, "chosen": 0, "q_chosen": qc, "q_mean": float(np.mean(q)), "q_best": float(max(q)),
               "candidates": list(range(len(q))), "q": q} for i, (qc, q) in enumerate(qs)]
    g["decision_values"] = {"config": {"k": 6, "depth": 1, "rollouts": 2, "share": 1.0},
                            "stats": {"decisions": len(qs) + 1, "valued": len(qs), "skipped": 1, "seconds": 0.5}, "values": values}
    return g


def test_decision_terms_against_the_mean_or_the_best():
    g = valued(_battle_game("siege", 0, 0, enemy_lost=[0, 0]), (1.0, [1.0, 0.5, 0.0]), (0.25, [0.25, 1.0]))
    assert np.allclose(train_e2.decision_terms(g), [1.0 - 0.5, 0.25 - 0.625])
    assert np.allclose(train_e2.decision_terms(g, "best"), [0.0, 0.25 - 1.0]), "against the best: never positive"
    assert len(train_e2.decision_terms(_battle_game("siege", 0, 0, enemy_lost=[0, 0]))) == 0, "a game without values"
    with pytest.raises(ValueError):
        train_e2.decision_terms(g, "policy")
    rep = train_e2.decision_report([g, _battle_game("siege", 0, 1, enemy_lost=[0, 0])])
    assert rep["games"] == 1 and rep["valued"] == 2 and rep["chosen_best_share"] == 0.5
    assert rep["term_mean"] == pytest.approx((0.5 - 0.375) / 2) and rep["stats"]["skipped"] == 1


def test_a_zero_signal_group_is_kept_for_its_decisions():
    games = {}
    # Two equal sieges (no capture, no damage: the outcome says nothing), but the net chose worse than its alternatives once.
    games["d/b00"] = valued(_battle_game("siege", 0, 0, enemy_lost=[0, 0, 0]), (0.0, [0.0, 1.0]))
    games["d/b01"] = _battle_game("siege", 0, 1, enemy_lost=[0, 0, 0])
    # And a group with a spread (it sets the kind's scale).
    games["d/b10"] = _battle_game("siege", 1, 0, enemy_lost=[0, 0, 0], damage=[0.0, 0.5, 0.5])
    games["d/b11"] = _battle_game("siege", 1, 1, enemy_lost=[0, 0, 0])
    off = train_e2.plan_advantages(games, args("--adv-norm", "dr"), {})
    assert off["drop"] == {"d/b00", "d/b01"}, "without the term the group carries no signal"
    on = args("--adv-norm", "dr", "--decision-advantage", "0.5")
    plan = train_e2.plan_advantages(games, on, {})
    assert plan["drop"] == set()
    st = plan["stats"]["battle/siege"]
    assert st["zero_signal"] == 1 and st["kept_by_decisions"] == 1
    assert all(abs(v) < 1e-9 for v in plan["dense"][("d/b00", 0)].values()), "its turns keep their (zero) advantages"
    # The scale is the turn advantages' (the kept group does not count in it).
    assert plan["scale"]["battle/siege"] == pytest.approx(off["scale"]["battle/siege"])
    # Equal decision values (a decided scene: every candidate ties) are no signal either.
    games["d/b00"] = valued(_battle_game("siege", 0, 0, enemy_lost=[0, 0, 0]), (1.0, [1.0, 1.0]))
    assert train_e2.plan_advantages(games, on, {})["drop"] == {"d/b00", "d/b01"}


def test_game_targets_carry_the_decision_coefficient():
    games = {"d/b00": _battle_game("siege", 0, 0, enemy_lost=[0, 0, 0], damage=[0.0, 0.5, 0.5]),
             "d/b01": _battle_game("siege", 0, 1, enemy_lost=[0, 0, 0])}
    a = args("--adv-norm", "dr", "--decision-advantage", "0.5")
    plan = train_e2.plan_advantages(games, a, {})
    div = plan["scale"]["battle/siege"]
    t = train_e2.game_targets([(k, g, None) for k, g in games.items()], a, {}, {}, plan["dense"], plan["shaping"], plan["scale"])
    assert t["d/b00"][0][6] == pytest.approx(0.5 / div) and t["d/b01"][0][6] == pytest.approx(0.5 / div)
    t0 = train_e2.game_targets([(k, g, None) for k, g in games.items()], args("--adv-norm", "dr"), {}, {}, plan["dense"], plan["shaping"], plan["scale"])
    assert len(t0["d/b00"][0]) == 6, "off: no coefficient"


def _shard(tmp_path, dvalues=True):
    d = synthetic.write_shard(tmp_path / "s.shard", 0, 12, 8, rounds=4, players=2)
    e2_fake.add_e2(d, {"temperature": 1.0, "group": 0, "member": 0, "group_size": 2, "learners": [0, 1], "seats": []})
    n = len(np.load(d / "decisions.npy"))
    q = np.full((n, 4), np.nan, np.float32)
    q[::2] = np.stack([np.linspace(0, 1, n)[::2], np.full(len(q[::2]), 0.5), np.ones(len(q[::2])), np.full(len(q[::2]), 3)], 1)
    if dvalues:
        np.save(d / "e2_dvalues.npy", q.astype("<f4"))
    return d, q


def test_the_shard_adds_the_decision_term_to_its_turn_advantage(tmp_path):
    d, q = _shard(tmp_path)
    s = train_e2.E2Shard(d)
    s.set_targets({0: (0.5, 0.0, None, float("nan"), {0: 1.0, 1: -1.0}, 0.25, 2.0), 1: (0.5, 0.3, None, float("nan"))})
    seen = 0
    for t in range(len(s)):
        arr = s.turn(t).dec_arrays
        rows = arr["e2_row"]
        if int(s.turns[t, 1]) == 0:
            term = np.nan_to_num(q[rows, 0] - q[rows, 1], nan=0.0)
            assert np.allclose(arr["e2_adv"], s.turn_adv[t] + 2.0 * term), "turn advantage + coefficient × (Q chosen − Q mean)"
            seen += int((term != 0).sum())
        else:
            assert np.allclose(arr["e2_adv"], 0.3), "a seat without the coefficient: its turn advantage alone"
    assert seen > 0
    s.set_targets({0: (0.5, 0.0, None, float("nan"), {0: 1.0}, 0.0, 2.0)}, dec_baseline="best")
    t = next(t for t in range(len(s)) if int(s.turns[t, 1]) == 0)
    arr = s.turn(t).dec_arrays
    rows = arr["e2_row"]
    assert np.allclose(arr["e2_adv"], s.turn_adv[t] + 2.0 * np.nan_to_num(q[rows, 0] - q[rows, 2], nan=0.0))


def test_a_shard_without_values_trains_as_before(tmp_path):
    d, _ = _shard(tmp_path, dvalues=False)
    s = train_e2.E2Shard(d)
    assert np.isnan(s.e2_dvalues).all()
    s.set_targets({0: (0.5, 1.5, None, float("nan"), None, 0.0, 2.0)})
    for t in range(len(s)):
        if int(s.turns[t, 1]) == 0:
            assert np.allclose(s.turn(t).dec_arrays["e2_adv"], 1.5)
    np.save(d / "e2_dvalues.npy", np.zeros((3, 4), "<f4"))
    with pytest.raises(ValueError):
        train_e2.E2Shard(d)


def test_distillation_raises_the_best_scored_candidate_even_unsampled():
    import torch
    # One decision, four candidates; the net puts all its mass on 0; the search scored 0, 2 and 3, and 3 is the best.
    logits = torch.tensor([[5.0, 0.0, -5.0, -5.0], [0.0, 0.0, 0.0, 0.0]], requires_grad=True)
    acc = torch.ones(2, 4, dtype=torch.bool)
    cq = torch.tensor([[0.1, float("nan"), 0.2, 1.0], [0.5, 0.5, float("nan"), float("nan")]])
    ce, used = train_e2.distill_terms(logits, acc, torch.ones(2), cq, torch.tensor([True, True]), tau=0.1)
    assert used.tolist() == [True, False], "a row whose scored values tie teaches nothing"
    ce.sum().backward()
    g = logits.grad[0]
    assert g[3] < 0 and g[0] > 0, "descent raises the best scored candidate and lowers the sampled worse one"
    assert g[1] > 0, "an unscored candidate gets no target mass"
    assert torch.all(logits.grad[1] == 0)


def test_the_shard_maps_each_scored_candidate_to_its_row(tmp_path):
    d = synthetic.write_shard(tmp_path / "s.shard", 0, 12, 8, rounds=4, players=2)
    dec = np.load(d / "decisions.npy")
    turns = np.load(d / "turns.npy")
    i = next(i for i, r in enumerate(dec) if r[4] >= 3)
    rnd = int(turns[int(dec[i, 0]), 0])
    ordinal = 7
    game = {"temperature": 1.0, "group": 0, "member": 0, "group_size": 2, "learners": [0, 1], "seats": [],
            "decision_values": {"values": [{"player": 0, "round": rnd, "ordinal": ordinal, "chosen": 0, "q_chosen": 0.1, "q_mean": 0.4, "q_best": 0.9,
                                            "candidates": [0, 2], "q": [0.1, 0.9]}]}}
    e2_fake.add_e2(d, game)
    e2d = np.zeros((len(dec), 4), "<i4")
    e2d[:, 0] = -1
    e2d[i, 0] = ordinal
    np.save(d / "e2_decisions.npy", e2d)
    s = train_e2.E2Shard(d)
    first = int(dec[i, 3])
    assert s.e2_cq[first] == pytest.approx(0.1) and s.e2_cq[first + 2] == pytest.approx(0.9)
    assert np.isfinite(s.e2_cq).sum() == 2
