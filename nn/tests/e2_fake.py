"""Stand-ins for the E2 binaries (ai-selfplay, ai-features --e2,
ai-tournament) for tests/test_e2.py: synthetic games and shards in the
contract's layout (classic/docs/ai-net-data.md, «E2: the net's own games»)."""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import numpy as np

NN = Path(__file__).resolve().parents[1]
if str(NN) not in sys.path:
    sys.path.insert(0, str(NN))

from aurelia_nn import synthetic  # noqa: E402


def opts(argv):
    out, rest, i = {}, [], 0
    while i < len(argv):
        if argv[i].startswith("--"):
            if i + 1 < len(argv) and not argv[i + 1].startswith("--"):
                out[argv[i][2:]] = argv[i + 1]
                i += 2
            else:
                out[argv[i][2:]] = True
                i += 1
        else:
            rest.append(argv[i])
            i += 1
    return out, rest


def selfplay(argv):
    o, _ = opts(argv)
    out = Path(o["out"])
    out.mkdir(parents=True, exist_ok=True)
    k, n = int(o.get("group", 1)), int(o["games"])
    rounds = int(o.get("rounds", 6))
    for idx in range(n):
        g, m = idx // k, idx % k
        seed = int(o.get("seed-offset", 0)) * 1000 + g
        size = "duel" if o.get("suite") == "duel" else "tiny"  # another map size: batches must not mix them
        stem = f"g{g:05d}m{m:02d}-{seed}-synthetic-{size}"
        if (out / f"{stem}.e2.json").exists():
            continue
        rng = np.random.default_rng([seed, m, int(o.get("sample-seed", 0))])
        (out / f"{stem}.aurlog").write_text(json.dumps({"seed": seed, "width": 10 if size == "duel" else 12, "height": 8, "rounds": 6, "players": 2}))
        share = float(rng.random())
        science = idx % 3 == 0
        # E2_FAKE_NET_WIN=P: against a net opponent (`net:<file>`) only seat 0 learns and wins with probability P (a duel's points share).
        net_win = os.environ.get("E2_FAKE_NET_WIN") if any(x.startswith("net:") for x in str(o.get("opponents", "")).split(",")) else None
        if net_win is not None:
            share = float(rng.random() < float(net_win))
        seats = [{"player": p, "mind": "learner", "learner": net_win is None or p == 0, "score": 10, "points_share": share if p == 0 else 1 - share, "rank": 1 + p,
                  "alive": True, "eliminated": None, "victory": ("science" if science else "time") if p == 0 else None,
                  "space": {"techs": 50, "space_techs": int(rng.integers(0, 75)), "space_techs_needed": 75, "projects": 0, "projects_needed": 1,
                            "parts_installed": 0, "parts": 0, "parts_needed": 7, "science": 100}}
                 for p in range(2)]
        game = {"format": 1, "stem": stem, "index": idx, "group": g, "member": m, "group_size": k, "spec": {"rounds": rounds, "size": size},
                "final_round": rounds - 1 if science else rounds, "temperature": float(o.get("temperature", 1)), "net": o.get("net"),
                "learners": [0, 1] if net_win is None else [0], "seats": seats}
        game["ground"] = ground(o, rng)
        (out / f"{stem}.e2.json").write_text(json.dumps(game))


def ground(o, rng):
    """A training ground's record (`grounds::GroundRecord`) for `--battle KIND`
    or `--window A-B`, raw numbers drawn; None for a whole game."""
    if o.get("battle"):
        army = lambda: {"player": 0, "kinds": [1, 2], "value": 100, "lost_units": 1, "lost_value": float(rng.uniform(0, 100))}
        captured = 20 + int(rng.integers(0, 10)) if rng.random() < 0.5 else None
        learner, enemy = army(), army()
        return {"suite": "battle", "kind": o["battle"], "edits": [], "economy": None,
                "battle": {"learner": 0, "enemy": 1, "start_round": 0, "length": 30, "end_round": 6, "target": 7, "target_owner": 0,
                           "captured_round": captured, "held": bool(rng.random() < 0.5), "target_damage": float(rng.random()),
                           "learner_army": learner, "enemy_army": enemy, "rounds": battle_rounds(rng, learner, enemy), "peace_rounds": 0, "reward": 0.0,
                           "difficulty": float(o.get("battle-difficulty", 1.0))}}
    if o.get("window"):
        rounds = [int(x) for x in str(o.get("from-rounds", "100")).split(",")]
        start = rounds[int(rng.integers(len(rounds)))]
        seats = [potential_seat(p, rng) for p in range(2)]
        return {"suite": "economy", "kind": "window", "edits": [], "battle": None,
                "economy": {"from_round": start, "window": 60, "end_round": start + 60, "seats": [], "learner_science": 1.0, "opponents_mean": 1.0,
                            "science_share": float(rng.random()), "learner_rate": float(np.mean([s["net"]["components"]["science"]["raw"] for s in seats])),
                            "expert_rate": 20.0, "expert_round": start + 60, "learner_potential": float(np.mean([s["net"]["total"] for s in seats])),
                            "expert_potential": float(np.mean([s["expert"]["total"] for s in seats])), "potential": seats}}
    if o.get("opening"):
        return {"suite": "opening", "kind": "window", "edits": [], "battle": None, "economy": None,
                "opening": {"window": 6, "end_round": 6, "round": list(range(7)), "reward": 0.0, "seats": [opening_seat(p, rng) for p in range(2)]}}
    return None


def opening_seat(player: int, rng, rounds: int = 6) -> dict:
    """`grounds::OpeningSeat` over the shard's rounds 0..rounds: growing counts, the capital at round 1."""
    grow = lambda top: [int(x) for x in np.sort(rng.integers(0, top + 1, rounds + 1))]
    return {"player": player, "learner": True, "tiles": 400, "rivals": 3, "city_states": 4, "start_value": 100, "prices": None,
            "explored": grow(120), "majors_met": grow(3), "city_states_met": grow(4), "rival_cities_seen": grow(5), "rival_capitals_known": grow(3),
            "camps_cleared": grow(2), "units_lost": grow(2), "lost_value": grow(80), "cities": [0] + [1] * rounds,
            "potential": [0.0] + [round(float(x), 2) for x in np.sort(rng.uniform(100, 400, rounds))]}


def battle_rounds(rng, learner: dict, enemy: dict, rounds: int = 6) -> dict:
    """`grounds::BattleRounds` of a battle over the shard's rounds 0..rounds:
    cumulative losses growing to the armies' end records."""
    def cum(total):
        steps = np.sort(rng.uniform(0, total, rounds))
        return [0.0] + [round(float(x), 1) for x in steps[:-1]] + [round(float(total), 1)]
    return {"round": list(range(rounds + 1)), "learner_lost_units": [0] * rounds + [learner["lost_units"]], "learner_lost_value": cum(learner["lost_value"]),
            "enemy_lost_units": [0] * rounds + [enemy["lost_units"]], "enemy_lost_value": cum(enemy["lost_value"]),
            "target_damage": [round(float(x), 3) for x in rng.uniform(0, 1, rounds + 1)]}


def potential(raw: dict, prices: dict) -> dict:
    """`grounds::Potential` of raw amounts at prices (centi-points)."""
    comps = {k: {"raw": float(v), "price": prices[k] / 100.0, "value": float(v) * prices[k] / 100.0} for k, v in raw.items()}
    yields = sum(c["value"] for k, c in comps.items() if k in train_yields())
    total = sum(c["value"] for c in comps.values())
    return {"total": total, "yields": yields, "stocks": total - yields, "buildings": 0, "components": comps}


def train_yields():
    return ("science", "production", "food", "gold", "culture", "faith")


def potential_seat(player: int, rng) -> dict:
    """A learner's `grounds::SeatPotential`: the expert's holdings fixed, the net's drawn around them."""
    from aurelia_nn.train_e2 import ECONOMY_COMPONENTS

    prices = {k: 100 + 10 * i for i, k in enumerate(ECONOMY_COMPONENTS)}
    expert = {k: 20.0 for k in ECONOMY_COMPONENTS}
    net = {k: float(v * rng.uniform(0.5, 1.5)) for k, v in expert.items()}
    sheet = {"player": player, "round": 160, "variant": "pre-war-fix", "rounds_left": 100, "centi": prices}
    return {"player": player, "prices": sheet, "net": potential(net, prices), "expert": potential(expert, prices)}


def features(argv):
    o, logs = opts(argv)
    assert o.get("e2"), "the fake features binary only makes E2 shards"
    for log in logs:
        log = Path(log)
        stem = log.name[: -len(".aurlog")]
        spec = json.loads(log.read_text())
        game = json.loads(log.with_name(stem + ".e2.json").read_text())
        d = synthetic.write_shard(Path(o["out"]) / f"{stem}.shard", spec["seed"], spec["width"], spec["height"], spec["rounds"], spec["players"])
        add_e2(d, game)


def add_e2(d: Path, game: dict, logp=None, accepted=None):
    """The E2 files of a synthetic shard: every candidate accepted and a
    uniform behaviour log-probability unless given."""
    dec = np.load(d / "decisions.npy")
    n_cand = len(np.load(d / "candidates.npy"))
    turns = np.load(d / "turns.npy")
    if accepted is None:
        accepted = np.ones(n_cand, np.uint8)
    if logp is None:
        logp = -np.log(np.maximum(dec[:, 4], 1)).astype(np.float32)
    np.save(d / "e2_accepted.npy", np.asarray(accepted, np.uint8))
    np.save(d / "e2_logp.npy", np.asarray(logp, "<f4"))
    np.save(d / "e2_value.npy", np.zeros(len(turns), "<i4"))
    np.save(d / "e2_decisions.npy", np.zeros((len(dec), 4), "<i4"))
    meta = json.loads((d / "meta.json").read_text())
    meta["e2"] = {"format": 1, "temperature": game["temperature"], "group": game["group"], "member": game["member"], "group_size": game["group_size"],
                  "learners": game["learners"], "seats": game["seats"]}
    (d / "meta.json").write_text(json.dumps(meta))
    (d / "e2.json").write_text(json.dumps({"game": game, "counts": {}}))


def tournament(argv):
    o, _ = opts(argv)
    if o.get("list"):
        print("standard")
        return
    rounds = 500 if o.get("suite") == "pace" else 300
    matches = [{"label": f"m{i}", "rounds": 350 + 40 * i, "winner": 0, "side": "candidate" if i % 2 == 0 else "baseline",
                "victory": "science" if i < 3 else "time", "hash": ""} for i in range(4)]
    rep = {"suite": o.get("suite"), "rounds": rounds, "versus": {"games": 4, "candidate_wins": 2, "share": {"mean": 0.5, "low": 0.3, "high": 0.7, "blocks": 4}},
           "matches": matches, "sides": [{"side": "candidate", "wins": {"science": 2}, "science": {"victories": [[400, {"mean": 0.25}]], "earliest": 350}}],
           "wall_seconds": 0.1, "argv": argv, "weights": os.environ.get("AURELIA_NET_WEIGHTS")}
    if o.get("sprt"):
        reference = "--reference" in argv
        rep["sequential"] = {"decision": "h0", "games": 4, "blocks": 2, "llr": -3.0, "mean": -0.1 if reference else 0.5, "low": -0.2 if reference else 0.4,
                             "high": 0.0 if reference else 0.6, **({"reference_share": 0.6} if reference else {})}
    Path(o["save"]).write_text(json.dumps(rep))


if __name__ == "__main__":
    {"selfplay": selfplay, "features": features, "tournament": tournament}[sys.argv[1]](sys.argv[2:])
