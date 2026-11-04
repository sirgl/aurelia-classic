"""Policy-gradient training on the net's own games (stage E2 of
classic/docs/ai-neural-architecture.md, «The E2 recipe»; the data contract is
classic/docs/ai-net-data.md, «E2: the net's own games»).

Games come from `ai-selfplay` (a learner seat sampling at temperature T,
groups of K games from one start); their shards are made on the fly by
`ai-features --e2` (E2Materializer) and hold only the learner's turns, each
decision labelled with the net's own choice, its behaviour log-probability
(`e2_logp`) and the candidates the rules accepted (`e2_accepted`).

The objective, per decision of a learner turn:

- advantage (`plan_advantages`, `game_targets`): Dr. GRPO by default, per
  game A = (r − mean_group r) / s with r the learner's reward (`reward_of`)
  and s the suite's (battle kind's) running RMS of those differences
  (`RunningScale`, an EMA across groups and iterations, `--adv-scale`), not
  the group's own std, which made near-equal noisy rewards unit-scale
  (`--adv-norm group-std` keeps it); a group of one falls back to a running
  baseline (`RunningBaseline`). A group with equal rewards (`--zero-tol`)
  is dropped and counted. A battle with its round-by-round record gets
  per-turn rewards (`battle_turn_rewards`) and GAE(γ, λ) advantages against
  the group's leave-one-out reward-to-go (`battle_gae`). With
  `--dense-potential W` every other suite adds, turn by turn, W × the
  GAE advantage of the potential's shaping (r_t = the change of log
  potential over the learner's turn, `potential_turn_rewards`) to its
  outcome's advantage. `--advantage gae`
  uses a per-turn GAE(λ) from the behaviour value head (`e2_value`) for
  whole games instead;
- policy: the new log-probability is log_softmax(logits / T) over the
  accepted candidates; ratio = exp(new − behaviour); PPO clipped surrogate
  (clip ε) weighted by A. In the first epoch a decision whose ratio is out
  of [1/c, c] is masked (off-policy masking, DeepSeek-V3.2) and stays masked
  in the later epochs of the iteration;
- KL(new ‖ reference) over the accepted candidates, exact, coefficient β; the
  reference is the frozen imitation net, its log-probabilities made in a
  batch's first epoch and reused by the later ones (`--no-reuse-ref`: every
  epoch);
- entropy bonus over the accepted candidates;
- value head: (v − r)² per turn, weight `--value-weight`.

Training grounds (`ai-selfplay --battle KIND`, `--window A-B`;
classic/docs/ai-net-data.md, «E2: training grounds») are games whose
`.e2.json` has a `ground` record; each suite has its own reward
(`ground_reward`), and its advantages are divided by its own running scale,
so the scales mix:

- battle: per kind from the raw numbers (`battle_reward`), in [0, 1]; with
  `--battle-reward dense` (the default) per turn (`battle_turn_rewards`);
- economy (`economy_reward`, `--economy-reward`): at the window's end the
  learner against the expert, the same player at the same round of the
  source log; `potential` (the default): log(net potential / expert
  potential) clipped to [−1, 1], a potential being the empire's yields a
  turn and stocks at the scripted AI's shadow prices (`ground.economy.potential`);
  `rate`: the same of the science a turn (the aim is science); `share`: the
  window's science share (learner / (learner + the others' mean), 0.5 at
  par). With `--economy-bootstrap` + γ^W · V(final), V the behaviour value
  head at the learner's last turn of the window (`e2_value`, the truncated
  episode's bootstrap; `share` with it is the reward before the expert's).
- opening (`ai-selfplay --opening W`; `opening_phi`): the game's first W
  rounds, the learner net commanding its units only; per turn the change of
  the opening's potential Φ (`opening_turn_rewards`: the empire's log
  potential plus what the nation found — tiles, rivals, city-states, rival
  cities and capitals, camps cleared — minus the units it lost), GAE
  advantages against the group's leave-one-out reward-to-go as a dense
  battle's; the game's reward Φ(end) − Φ(start).

The value head is trained on whole games only (standard, duel): a battle's
or a window's return is not the whole game's outcome V predicts.

The memory is rebuilt exactly as at play: a game's learner turns are one
window from its first turn, run in pieces of `--bptt` turns (truncated BPTT,
the memory carried on detached), as in train_imitation.run_windows.

    python -m aurelia_nn.train_e2 --games GAMES_DIR --init ref.pt --out run/
"""

from __future__ import annotations

import argparse
import copy
import json
import math
import shutil
import subprocess
import tempfile
import time
from collections import defaultdict
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F

from .data import Deferred, Shard, Uploader, bucket, collate, upload_batch
from .materialize import LogMaterializer, stream_shards
from .model import Net, WindowMemory, load_checkpoint, padded_logits, parameter_count, save_checkpoint
from .train_imitation import host_decision_weights, parse_decision_weights, pick_device, prefetch

MISSING_VALUE = -(1 << 31)
NEG = -1e9  # the logit of a candidate outside the softmax's support (finite: no NaN in the gradients)


# ---------------------------------------------------------------------------
# Rewards and advantages


REWARDS = ("victory", "points_share", "science", "conquest")
# The aims of the aim token (Config.n_aims, docs: «Goals: one net, several
# aims»), in the order of the aim weights, and the reward each aim scores.
AIMS = ("science", "score", "conquest")
AIM_REWARD = {"science": "science", "score": "points_share", "conquest": "conquest"}
# Φ_end of the science reward: the share of the spaceship's technologies
# known and of its projects and parts done.
PHI_TECH_WEIGHT, PHI_PARTS_WEIGHT = 0.6, 0.4


def science_progress(seat: dict, proxy: float | None = None) -> tuple[float, str]:
    """Φ_end in [0, 1] of a seat at the game's end and where it came from.

    From the seat's `space` record (`.e2.json` format 2,
    classic/docs/ai-net-data.md): `space_techs` / `space_techs_needed` (the
    technologies of `grand_strategy::space_closure` known, of all of them)
    and (`projects` + `parts`) / (`projects_needed` + `parts_needed`) (Apollo
    and the spaceship's parts): Φ = 0.6 × techs share + 0.4 × finish share.
    Without it (format 1): `proxy` (the shard's estimate,
    E2Shard.tech_share: the share of all technologies known, finish 0),
    else 0."""
    sp = seat.get("space") or {}
    if sp.get("space_techs_needed"):
        techs = float(sp.get("space_techs", 0)) / float(sp["space_techs_needed"])
        need = float(sp.get("projects_needed", 0) or 0) + float(sp.get("parts_needed", 0) or 0)
        finish = (float(sp.get("projects", 0) or 0) + float(sp.get("parts", 0) or 0)) / need if need else 0.0
        return PHI_TECH_WEIGHT * min(techs, 1.0) + PHI_PARTS_WEIGHT * min(finish, 1.0), "record"
    if proxy is not None:
        return PHI_TECH_WEIGHT * min(max(proxy, 0.0), 1.0), "proxy"
    return 0.0, "none"


def reward_of(game: dict, player: int, mode: str = "victory", time_weight: float = 0.5, early_bonus: float = 0.2, max_rounds: int | None = None,
              phi_weight: float = 0.3, tech_share: float | None = None) -> float:
    """The reward of seat `player` of a game (`<stem>.e2.json`, selfplay::GameMeta).

    - `points_share`: the seat's `points_share` (1 the winner, the best score
      shared without one).
    - `victory`: 1 + early_bonus × (1 − victory round / max rounds) for a win
      of any real kind (science, domination, culture, diplomatic); 0 when
      another seat won one; with no winner, or the time victory at the round
      cap (a win by score), time_weight × points_share: a score result is worth
      less than a real win.
    - `science`: 1 + early_bonus × (1 − round / max rounds) for a science
      victory; else phi_weight × Φ_end (`science_progress`, a state at the
      end, not a per-turn sum).
    - `conquest`: as `victory`, domination only, else 0."""
    seats = game["seats"]
    me = next(s for s in seats if int(s["player"]) == int(player))
    if mode == "points_share":
        return float(me["points_share"])
    rounds = max_rounds or game_max_rounds(game)
    won_at = (game.get("victory") or {}).get("round") or me.get("victory_round") or game["final_round"]
    frac = min(max(float(won_at) / max(rounds, 1), 0.0), 1.0)
    win = 1.0 + early_bonus * (1.0 - frac)
    if mode == "science":
        if me.get("victory") == "science":
            return win
        return phi_weight * science_progress(me, tech_share)[0]
    if mode == "conquest":
        return win if me.get("victory") == "domination" else 0.0
    if mode != "victory":
        raise ValueError(f"reward mode {mode!r}")
    winner = next((s for s in seats if s.get("victory")), None)
    if winner is None or winner["victory"] == "time":
        return time_weight * float(me["points_share"])
    return win if int(winner["player"]) == int(player) else 0.0


def game_max_rounds(game: dict) -> int:
    """The game's last round, from its own record (it depends on the game
    speed): `max_rounds` of the game or its spec when written, else the
    spec's `rounds` (the cap ai-selfplay played to)."""
    spec = game.get("spec") or {}
    return int(game.get("max_rounds") or spec.get("max_rounds") or spec["rounds"])


def aim_weights(game: dict, player: int) -> np.ndarray | None:
    """The seat's aim weights (science, score, conquest) when the game
    recorded them (the seat's `aim`, else the game's), else None."""
    me = next((s for s in game["seats"] if int(s["player"]) == int(player)), {})
    w = me.get("aim", game.get("aim"))
    if w is None:
        return None
    if isinstance(w, dict):
        w = [float(w.get(k, 0.0)) for k in AIMS]
    return np.asarray(w, np.float32)


# -- training grounds ----------------------------------------------------------

GROUND_SUITES = ("battle", "economy", "opening")
BATTLE_KINDS = ("field", "siege", "defence", "rush-defence")
# The skirmish ladder's kind (`grounds::SKIRMISH`): a ground of its own, out of
# the default mix and the battle eval unless asked for (`e2_loop --battle-kinds`).
SKIRMISH = "skirmish"
# The battle window (`grounds::BattleKind::Clash`): large armies for a few rounds, rewarded by `clash_phi`.
CLASH = "clash"
ALL_BATTLE_KINDS = (*BATTLE_KINDS, SKIRMISH, CLASH)


def ground_of(game: dict) -> dict | None:
    """The game's training ground (`.e2.json` `ground`), None for a whole game."""
    g = game.get("ground")
    return g if isinstance(g, dict) and g.get("suite") in GROUND_SUITES else None


def suite_of(game: dict) -> str:
    """`battle`, `economy`, or the bench suite of a whole game (its spec's)."""
    g = ground_of(game)
    if g is not None:
        return g["suite"]
    return str((game.get("spec") or {}).get("suite") or "standard")


# The opening's potential Φ (`grounds::OPENING_WEIGHTS`, the Rust reference;
# classic/docs/ai-net-data.md, «E2: the opening ground»): term → weight.
OPENING_WEIGHTS = {"potential": 1.0, "explored": 0.25, "majors_met": 0.0, "city_states_met": 0.0, "rival_cities_seen": 0.0,
                   "rival_capitals_known": 0.0, "camps_cleared": 0.0, "lost_value": 0.0, "improved_worked": 0.0, "improved": 0.0,
                   "civilians_lost": 0.05}


def parse_opening_weights(spec: str | None) -> dict:
    """`--opening-weights term=w,...` over OPENING_WEIGHTS ('' : the defaults)."""
    w = dict(OPENING_WEIGHTS)
    for part in (spec or "").split(","):
        if part.strip():
            k, _, v = part.partition("=")
            if k.strip() not in w:
                raise SystemExit(f"--opening-weights: no term {k.strip()!r} ({', '.join(w)})")
            w[k.strip()] = float(v)
    return w


def opening_terms(seat: dict) -> dict:
    """The terms of an opening seat's Φ per entry (`ground.opening.seats[i]`,
    `grounds::OpeningSeat`): `potential` log(P + P0), P0 the potential at the
    first entry with a city (1 without one: an empire without a city has a
    finite Φ and its first city is worth about log 2, `potential_phi`); the
    others shares of their normalisers (the map's tiles, the rival nations,
    the city-states, the units' production cost at the start); camps, the
    improved tiles worked and owned, the civilians lost counts (the
    penalties negated here)."""
    arr = lambda k: np.asarray(seat.get(k) or [], np.float64)
    share = lambda k, of: arr(k) / float(of) if of else np.zeros(len(arr(k)))
    pot = np.maximum(arr("potential"), 0.0)
    cities = arr("cities")
    p0 = 1.0
    if len(cities) == len(pot) and (cities > 0).any() and pot[int(np.argmax(cities > 0))] > 0:
        p0 = float(pot[int(np.argmax(cities > 0))])
    rivals = int(seat.get("rivals") or 0)
    return {
        "potential": np.log(pot + p0),
        "explored": share("explored", int(seat.get("tiles") or 0)),
        "majors_met": share("majors_met", rivals),
        "city_states_met": share("city_states_met", int(seat.get("city_states") or 0)),
        "rival_cities_seen": share("rival_cities_seen", rivals),
        "rival_capitals_known": share("rival_capitals_known", rivals),
        "camps_cleared": arr("camps_cleared"),
        "lost_value": -share("lost_value", int(seat.get("start_value") or 0)),
        "improved_worked": arr("improved_worked") if seat.get("improved_worked") else np.zeros(len(pot)),
        "improved": arr("improved") if seat.get("improved") else np.zeros(len(pot)),
        "civilians_lost": -arr("civilians_lost") if seat.get("civilians_lost") else np.zeros(len(pot)),
    }


def opening_phi(seat: dict, weights: dict | None = None) -> np.ndarray:
    """Φ per entry of an opening seat: Σ weight × term (`opening_terms`; the
    units lost enter negated), the Rust reference `grounds::opening_phi`."""
    w = OPENING_WEIGHTS if weights is None else weights
    terms = opening_terms(seat)
    return sum(float(w[k]) * terms[k] for k in OPENING_WEIGHTS)


def opening_seat(game: dict, player: int) -> dict | None:
    for s in (ground_of(game) or {}).get("opening", {}).get("seats") or []:
        if int(s["player"]) == int(player):
            return s
    return None


def opening_turn_rewards(game: dict, player: int, weights: dict | None = None) -> tuple[np.ndarray, np.ndarray] | None:
    """(round R of each learner turn, Φ(entry R + 1) − Φ(entry R)) of an
    opening (the turn of round R did what lies between the two entries: its
    moves and the others' answer); None with fewer than two entries."""
    o = ground_of(game)["opening"]
    seat = opening_seat(game, player)
    rounds = np.asarray(o.get("round") or [], np.int64)
    if seat is None or len(rounds) < 2:
        return None
    return rounds[:-1], np.diff(opening_phi(seat, weights))


def opening_reward(game: dict, player: int, weights: dict | None = None) -> float:
    """An opening's reward: Φ(end) − Φ(start) (the turn rewards' sum)."""
    seat = opening_seat(game, player)
    if seat is None:
        return 0.0
    phi = opening_phi(seat, weights)
    return float(phi[-1] - phi[0]) if len(phi) else 0.0


def suite_key(game: dict) -> str:
    """The suite and, for a battle, its kind (`battle/siege`): the key of the per-suite logs."""
    g = ground_of(game)
    return f"battle/{g['kind']}" if g is not None and g["suite"] == "battle" else suite_of(game)


def _lost_share(army: dict) -> float:
    value = float(army.get("value") or 0)
    return min(max(float(army.get("lost_value") or 0.0) / value, 0.0), 1.0) if value > 0 else 0.0


SIEGE_SHAPING = 0.4  # the Rust reference's (`grounds::SIEGE_SHAPING`); the loop moves it by the captures seen


def siege_capture(b: dict, speed_decay: float = 0.0) -> float:
    """A siege's capture reward: 0.5 + 0.5 × (1 − (round taken − start) /
    length) as before; with `speed_decay` γ > 0 (`--siege-speed-decay`, the
    siege ladder) 0.5 + 0.5 × γ^(round taken − start): the full reward for a
    capture in the learner's first turn (`captured_round` = the war's first
    round), 0.75 a turn later at γ = 0.5, 0.625 two turns later, so the same
    turn's capture pays clearly more than a slower one."""
    late = int(b["captured_round"]) - int(b["start_round"])
    if speed_decay > 0.0:
        return 0.5 + 0.5 * min(speed_decay, 1.0) ** max(late, 0)
    return 0.5 + 0.5 * min(max(1.0 - late / max(int(b["length"]), 1), 0.0), 1.0)


def battle_reward(game: dict, siege_shaping: float = SIEGE_SHAPING, speed_decay: float = 0.0) -> float:
    """The reward of a battle ground, in [0, 1], from its raw numbers
    (`ground.battle`; the Rust reference is `grounds::battle_reward`):

    - field: 0.5 + 0.5 × (the enemy's lost share − the learner's), each the
      value-weighted share of the army's cost lost (killed: the whole cost,
      wounded: the share of health lost);
    - siege: taken: 0.5 + 0.5 × (1 − (round taken − start) / length); not
      taken: w × (0.5 × the city's highest damage share + 0.25 × the share of
      the garrison's units killed + 0.25 × its value-weighted lost share),
      w = `siege_shaping` < 0.5, so a capture always pays more (the loop
      keeps w high, 0.45, while the net takes no city, and lowers it to 0.2
      as captures appear);
    - defence, rush-defence: 0.5 × held + 0.5 × the enemy's lost share;
    - skirmish: 0.25 + 0.25 × (the enemy's lost share − the learner's) +
      0.5 × W, W the win's worth (`skirmish_win`)."""
    g = ground_of(game)
    b = g["battle"]
    enemy, own = _lost_share(b["enemy_army"]), _lost_share(b["learner_army"])
    kind = g["kind"]
    if kind == "field":
        r = 0.5 + 0.5 * (enemy - own)
    elif kind == SKIRMISH:
        r = 0.25 + SKIRMISH_DENSE * (enemy - own) + SKIRMISH_WIN * skirmish_win(b)
    elif kind == CLASH:
        phi = clash_phi(b)
        r = 0.5 + 0.5 * (float(phi[-1] - phi[0]) if len(phi) else 0.0)
    elif kind == "siege":
        if b.get("captured_round") is not None:
            r = siege_capture(b, speed_decay)
        else:
            army = b["enemy_army"]
            killed = min(float(army.get("lost_units") or 0) / max(len(army.get("kinds") or []), 1), 1.0)
            damage = min(max(float(b.get("target_damage") or 0.0), 0.0), 1.0)
            r = min(max(siege_shaping, 0.0), 0.49) * (0.5 * damage + 0.25 * killed + 0.25 * enemy)
    elif kind in ("defence", "rush-defence"):
        r = 0.5 * float(bool(b.get("held"))) + 0.5 * enemy
    else:
        raise ValueError(f"battle kind {kind!r}")
    return min(max(r, 0.0), 1.0)


SKIRMISH_DENSE = 0.25  # a skirmish's damage terms: the end reward stays in [0, 1] with the win worth as much as the whole damage swing
SKIRMISH_WIN = 0.5


def skirmish_win(b: dict) -> float:
    """A skirmish win's worth (`grounds::skirmish_win`): 1 − 0.5 × (the
    rounds it took − 1) / the round limit; 0 without a win."""
    if b.get("won_round") is None:
        return 0.0
    took = max(int(b["won_round"]) - int(b["start_round"]) - 1, 0)
    return 1.0 - 0.5 * took / max(int(b["length"]), 1)


def clash_phi(b: dict) -> np.ndarray:
    """A clash's potential per round entry (`grounds::clash_phi`): (the
    enemy's value destroyed so far + the learner's ready force now) / the
    learner's army value."""
    tr = b.get("rounds") or {}
    enemy = np.asarray(tr.get("enemy_lost_value") or [], np.float64)
    ready = np.asarray(tr.get("learner_ready") or [], np.float64)
    n = min(len(enemy), len(ready))
    return (enemy[:n] + ready[:n]) / max(float(b["learner_army"].get("value") or 0), 1.0)


def battle_won(game: dict) -> bool:
    """A battle won by the learner (the eval's win): field, its army lost a
    smaller share than the enemy's; siege, the city taken; defence, held;
    skirmish, every enemy unit dead (`won_round`)."""
    g = ground_of(game)
    b = g["battle"]
    if g["kind"] == SKIRMISH:
        return b.get("won_round") is not None
    if g["kind"] == CLASH:
        phi = clash_phi(b)
        return bool(len(phi) and phi[-1] > phi[0])
    if g["kind"] == "field":
        return _lost_share(b["learner_army"]) < _lost_share(b["enemy_army"])
    if g["kind"] == "siege":
        return b.get("captured_round") is not None
    return bool(b.get("held"))


BATTLE_REWARDS = ("dense", "end")
DENSE_WEIGHT = 0.5  # the dense terms' weight: their sum is on the end reward's scale (`battle_reward`)
CITY_DAMAGE_WEIGHT = 0.25  # defence: the held city's damage (a share of its hit points) taken, against the garrison's losses


def battle_turn_rewards(game: dict, siege_shaping: float = SIEGE_SHAPING, speed_decay: float = 0.0) -> tuple[np.ndarray, np.ndarray] | None:
    """(round R of each learner turn, its reward) of a battle from its
    round-by-round record (`ground.battle.rounds`, `grounds::BattleRounds`):
    the turn of round R gets what changed between the entries of R and R + 1
    (the learner's moves and the enemy's answer), the last turn also the end
    bonus. None without the record (an older ai-selfplay). With d the change
    of the enemy's value-weighted lost share (damage dealt), t the learner's
    (taken), c the change of the target's damage share:

    - field: 0.5 × (d − t); the sum is the end reward − 0.5;
    - defence, rush-defence: 0.5 × (d − t) − 0.25 × c, + 0.5 at the end when
      the city is held;
    - siege: w × (0.5 × c⁺ + 0.25 × the change of the garrison's killed share
      + 0.25 × d), c⁺ of the highest damage so far (the end reward's shaping,
      paid as it happens; w = `siege_shaping`), + 0.5 + 0.5 × (1 − (round
      taken − start) / length) at the capture;
    - clash: 0.5 × the change of Φ (`clash_phi`): the enemy's value destroyed
      in the round plus the change of the learner's ready force;
    - skirmish: 0.25 × (d − t), + 0.5 × W at the end of a win (`skirmish_win`);
      the sum is the end reward − 0.25 (and − 0.25 × the enemy's wounds at the start).

    Constants and the scene's own wounds at the start cancel in the
    advantages (a group shares its scene)."""
    g = ground_of(game)
    b = g["battle"]
    tr = b.get("rounds") or {}
    rounds = np.asarray(tr.get("round") or [], np.int64)
    if len(rounds) < 2:
        return None
    share = lambda key, army: np.asarray(tr[key], np.float64) / max(float(army.get("value") or 0), 1.0)
    dealt = np.diff(share("enemy_lost_value", b["enemy_army"]))
    taken = np.diff(share("learner_lost_value", b["learner_army"]))
    damage = np.asarray(tr["target_damage"], np.float64)
    kind = g["kind"]
    if kind == "field":
        r = DENSE_WEIGHT * (dealt - taken)
    elif kind == SKIRMISH:
        r = SKIRMISH_DENSE * (dealt - taken)
        r[-1] += SKIRMISH_WIN * skirmish_win(b)
    elif kind == CLASH:
        phi = clash_phi(b)
        if len(phi) < 2:
            return None
        r = 0.5 * np.diff(phi)
        rounds = rounds[: len(phi)]
    elif kind in ("defence", "rush-defence"):
        r = DENSE_WEIGHT * (dealt - taken) - CITY_DAMAGE_WEIGHT * np.diff(damage)
        if b.get("held"):
            r[-1] += 0.5
    elif kind == "siege":
        killed = np.diff(np.asarray(tr["enemy_lost_units"], np.float64)) / max(len(b["enemy_army"].get("kinds") or []), 1)
        w = min(max(siege_shaping, 0.0), 0.49)
        r = w * (0.5 * np.diff(np.maximum.accumulate(damage)) + 0.25 * killed + 0.25 * dealt)
        if b.get("captured_round") is not None:
            r[-1] += siege_capture(b, speed_decay)
    else:
        raise ValueError(f"battle kind {kind!r}")
    return rounds[:-1], r


def discounted_to_go(r: np.ndarray, gamma: float) -> np.ndarray:
    out = np.zeros(len(r), np.float64)
    acc = 0.0
    for t in range(len(r) - 1, -1, -1):
        acc = r[t] + gamma * acc
        out[t] = acc
    return out


def gae_with_baseline(r: np.ndarray, v: np.ndarray, gamma: float, lam: float) -> np.ndarray:
    """GAE(γ, λ) of one trajectory: δ_t = r_t + γ V_{t+1} − V_t (V after the
    last step 0: the battle is over), A_t = Σ_l (γλ)^l δ_{t+l}."""
    nxt = np.append(np.asarray(v[1:], np.float64), 0.0)
    delta = np.asarray(r, np.float64) + gamma * nxt - np.asarray(v, np.float64)
    out = np.zeros(len(r), np.float64)
    acc = 0.0
    for t in range(len(r) - 1, -1, -1):
        acc = delta[t] + gamma * lam * acc
        out[t] = acc
    return out


def battle_gae(groups: dict, gamma: float = 0.99, lam: float = 0.95) -> dict:
    """Per-turn advantages of dense battles: `groups` {group: {key: (rounds,
    rewards)}} → {key: (rounds, raw advantages)}, GAE(γ, λ) against a
    baseline V_k (k the turn's step from the war's start): the mean
    discounted reward-to-go at step k of the group's OTHER members (they
    share the scene: leave-one-out, unbiased); a step no other member
    reached: the same over the battles of the call (other scenes), else 0.
    A member's state after its last turn is terminal (V = 0). With λ = 1
    this is the member's reward-to-go minus the others' mean (the per-step
    Dr. GRPO); λ < 1 trades the baseline's bias for less variance."""
    to_go = {k: discounted_to_go(rw, gamma) for members in groups.values() for k, (_, rw) in members.items()}
    # Step k over every battle of the call: sum and count, for the fallback.
    pool_sum, pool_n = defaultdict(float), defaultdict(int)
    for g in to_go.values():
        for k, x in enumerate(g):
            pool_sum[k] += x
            pool_n[k] += 1
    out = {}
    for members in groups.values():
        keys = list(members)
        for key in keys:
            rounds, rw = members[key]
            others = [to_go[o] for o in keys if o != key]
            v = np.zeros(len(rw), np.float64)
            for k in range(len(rw)):
                xs = [o[k] for o in others if k < len(o)]
                if xs:
                    v[k] = float(np.mean(xs))
                else:
                    n = pool_n[k] - 1
                    v[k] = (pool_sum[k] - to_go[key][k]) / n if n > 0 else 0.0
            out[key] = (rounds, gae_with_baseline(rw, v, gamma, lam))
    return out


def potential_track(game: dict, player: int) -> tuple[np.ndarray, np.ndarray, float] | None:
    """(round of each entry, the learner's potential per entry, P0) of
    `.e2.json` `potential` (`selfplay::PotentialRounds`, format 3: entry 0
    the world when the learner starts, entry k the start of round
    `round[k]`, the last the end); P0 is the potential at the price sheet's
    entry, the first with a city (a mid-game start's entry 0, a game from
    round 0 the round its capital stands). None without the record (an
    older ai-selfplay), with fewer than two entries, or when the learner
    never held a city (no price sheet)."""
    rec = game.get("potential") or {}
    rounds = rec.get("round") or []
    seat = next((s for s in rec.get("seats") or [] if int(s["player"]) == int(player)), None)
    if seat is None or len(rounds) < 2 or seat.get("prices") is None or len(seat.get("total") or []) != len(rounds):
        return None
    total = np.maximum(np.asarray(seat["total"], np.float64), 0.0)
    cities = np.asarray((seat.get("raw") or {}).get("cities") or [], np.float64)
    if len(cities) == len(total) and (cities > 0).any():
        at = int(np.argmax(cities > 0))
    else:
        at = next((i for i, r in enumerate(rounds) if int(r) == int(seat["prices"].get("round", -1))), int(np.argmax(total > 0)))
    p0 = float(total[at]) if total[at] > 0 else (float(total[total > 0][0]) if (total > 0).any() else 1.0)
    return np.asarray(rounds, np.int64), total, p0


def potential_phi(total: np.ndarray, p0: float) -> np.ndarray:
    """Φ = log(P + P0) of a learner's potentials P per entry, P0 its
    potential at its first entry with a city (`potential_track`): an empire
    without a city (or out of the game) has a finite Φ, log P0, so founding
    the capital is worth about log 2 and an elimination log((P + P0) / P0),
    not an unbounded jump; from a few times P0 on it is log P to within a
    few per cent, the relative growth."""
    return np.log(np.maximum(np.asarray(total, np.float64), 0.0) + p0)


def potential_turn_rewards(game: dict, player: int) -> tuple[np.ndarray, np.ndarray] | None:
    """(round R of each learner turn, its shaping reward Φ(next entry) −
    Φ(entry of R)) from the round-by-round potential (`potential_track`,
    Φ = `potential_phi`): the learner's turn of round R did what lies
    between the entry of R and the next one (its moves and the others'
    answer, as a battle's round). The sum over a game telescopes to Φ(end)
    − Φ(start), and Φ(start) is the group's (its members share the start):
    the shaping adds log P(end) to the game's return and pays it as it
    accrues. None without the record."""
    tr = potential_track(game, player)
    if tr is None:
        return None
    rounds, total, p0 = tr
    return rounds[:-1], np.diff(potential_phi(total, p0))


def dense_potential(game: dict, a) -> bool:
    """Whether a game gets the potential's shaping (`--dense-potential W` >
    0): every suite but the dense battles (they have their own per-turn
    reward), when its `.e2.json` records the potential."""
    if float(getattr(a, "dense_potential", 0.0) or 0.0) <= 0 or dense_battle(game, a):
        return False
    return any(potential_track(game, p) is not None for p in game["learners"])


def economy_return(game: dict, v_final: float | None, gamma: float = 0.99) -> tuple[float, float]:
    """(the reward, its science term) of an economy window: the science
    share (`ground.economy.science_share`: learner / (learner + the other
    nations' mean), 0.5 at par) + γ^W · V(final), W the window's rounds
    played and V the value head at the learner's last turn (expected final
    points share, in [0, 1]; 0 when the scorer has none)."""
    e = ground_of(game)["economy"]
    share = float(e["science_share"])
    w = max(int(e["end_round"]) - int(e["from_round"]), 0)
    v = 0.0 if v_final is None or not math.isfinite(v_final) else float(v_final)
    return share + gamma ** w * v, share


ECONOMY_REWARDS = ("potential", "rate", "share")
ECONOMY_CLIP = 1.0
# The components of a potential (`grounds::YIELDS`, `grounds::STOCKS`), in order.
ECONOMY_COMPONENTS = ("science", "production", "food", "gold", "culture", "faith", "cities", "citizens", "land", "wonders", "techs")


def log_ratio(net: float, expert: float, clip: float = ECONOMY_CLIP) -> float:
    """log(net / expert) clipped to [−clip, clip]; a side at 0 or below
    gives the clip's end toward the other (both: 0)."""
    if net > 0 and expert > 0:
        return min(max(math.log(net / expert), -clip), clip)
    if net <= 0 and expert <= 0:
        return 0.0
    return clip if net > 0 else -clip


def economy_seat(game: dict, player: int) -> dict | None:
    """The learner's record of `ground.economy.potential` (its potential and
    the expert's at one price sheet), None without one."""
    for s in ground_of(game)["economy"].get("potential") or []:
        if int(s["player"]) == int(player):
            return s
    return None


def economy_pair(game: dict, player: int, mode: str) -> tuple[float, float]:
    """(net, expert) of an economy window at its end for the reward `mode`:
    `potential` the totals, `rate` the science a turn."""
    e = ground_of(game)["economy"]
    s = economy_seat(game, player)
    if s is not None and s.get("expert") is not None:
        if mode == "potential":
            return float(s["net"]["total"]), float(s["expert"]["total"])
        return float(s["net"]["components"]["science"]["raw"]), float(s["expert"]["components"]["science"]["raw"])
    key = {"potential": ("learner_potential", "expert_potential"), "rate": ("learner_rate", "expert_rate")}[mode]
    if e.get(key[1]) is None:
        raise ValueError(f"economy window {game.get('stem')}: no expert's {mode} (a game of an older ai-selfplay, or no source log): use --economy-reward share")
    return float(e[key[0]]), float(e[key[1]])


def economy_reward(game: dict, player: int, v_final: float | None = None, mode: str = "potential", gamma: float = 0.99, bootstrap: bool = False) -> float:
    """The reward of an economy window (`--economy-reward`): `potential`,
    log(net / expert potential) clipped to [−1, 1]; `rate`, the same of the
    science a turn; `share`, the window's science share. With `bootstrap`
    + γ^W · V(final), W the window's rounds played (`economy_return`)."""
    if mode not in ECONOMY_REWARDS:
        raise ValueError(f"--economy-reward {mode}: one of {ECONOMY_REWARDS}")
    term = economy_return(game, None)[1] if mode == "share" else log_ratio(*economy_pair(game, player, mode))
    if not bootstrap:
        return term
    reward, share = economy_return(game, v_final, gamma)
    return term + (reward - share)


def economy_report(games) -> dict:
    """Per start round R of the economy windows (`all` too): the seats, the
    distribution of net / expert of the potential and of the science rate,
    and per component (`ECONOMY_COMPONENTS`) the net's raw amount over the
    expert's (their means) and the priced gap as a share of the expert's
    potential (the mean of (net − expert value) / expert total): what the
    net gains or loses where. Windows without the expert are left out."""
    rows = defaultdict(list)
    for g in games:
        gr = ground_of(g)
        if gr is None or gr["suite"] != "economy":
            continue
        e = gr["economy"]
        for s in e.get("potential") or []:
            if s.get("expert") is not None and float(s["expert"]["total"]) > 0:
                rows[str(int(e["from_round"]))].append(s)
                rows["all"].append(s)

    def ratios(xs):
        x = np.asarray(xs, np.float64)
        if not len(x):
            return None
        q = np.quantile(x, [0.1, 0.5, 0.9])
        return {"mean": float(x.mean()), "min": float(x.min()), "p10": float(q[0]), "p50": float(q[1]), "p90": float(q[2]), "max": float(x.max()),
                "mean_log": float(np.mean([log_ratio(v, 1.0) for v in x]))}

    out = {}
    for r in sorted(rows, key=lambda k: (k == "all", int(k) if k != "all" else 0)):
        ss = rows[r]
        rate = [s["net"]["components"]["science"]["raw"] / s["expert"]["components"]["science"]["raw"] for s in ss
                if s["expert"]["components"]["science"]["raw"] > 0]
        comps = {}
        for c in ECONOMY_COMPONENTS:
            net = np.mean([s["net"]["components"][c]["raw"] for s in ss])
            exp = np.mean([s["expert"]["components"][c]["raw"] for s in ss])
            gap = np.mean([(s["net"]["components"][c]["value"] - s["expert"]["components"][c]["value"]) / s["expert"]["total"] for s in ss])
            comps[c] = {"net": float(net), "expert": float(exp), "ratio": float(net / exp) if exp > 0 else None, "gap": float(gap)}
        out[r] = {"seats": len(ss), "potential": ratios([s["net"]["total"] / s["expert"]["total"] for s in ss]), "rate": ratios(rate), "components": comps}
    return out


def log_economy(report: dict, log=print):
    """`economy_report` in the log: per R the ratios, then each component's
    ratio and priced gap."""
    for r, v in report.items():
        p, q = v["potential"], v["rate"]
        fmt = lambda x: "-" if x is None else f"x{x['p50']:.3f} [p10 {x['p10']:.3f}, p90 {x['p90']:.3f}, mean log {x['mean_log']:+.3f}]"
        log(f"  economy R={r}: {v['seats']} seats, potential {fmt(p)}, science rate {fmt(q)}")
        log("    " + ", ".join(f"{c} x{'-' if x['ratio'] is None else format(x['ratio'], '.2f')} ({x['gap']:+.3f})" for c, x in v["components"].items()))


def pfsp_weights(shares: dict, win_share: dict, power: float = 2.0, floor: float = 0.05, prior: float = 0.5) -> dict:
    """Prioritised fictitious self-play (AlphaStar): each opponent's share
    of the mix × max((1 − the learner's recent win share against it)^power,
    floor), normalised; an opponent not met yet counts `prior`."""
    raw = {k: w * max((1.0 - min(max(float(win_share.get(k, prior)), 0.0), 1.0)) ** power, floor) for k, w in shares.items()}
    total = sum(raw.values())
    return {k: v / total for k, v in raw.items()} if total > 0 else dict(shares)


class RunningBaseline:
    """Exponential moving mean and variance of the reward (groups of one)."""

    def __init__(self, decay: float = 0.95, mean: float = 0.0, var: float = 1.0, n: int = 0):
        self.decay, self.mean, self.var, self.n = decay, mean, var, n

    def advantage(self, r: float, eps: float) -> float:
        a = (r - self.mean) / (math.sqrt(self.var) + eps) if self.n else 0.0
        self.update(r)
        return a

    def update(self, r: float):
        if self.n == 0:
            self.mean, self.var = r, 1.0
        else:
            d = r - self.mean
            self.mean += (1 - self.decay) * d
            self.var = self.decay * self.var + (1 - self.decay) * d * d
        self.n += 1

    def state(self) -> dict:
        return {"decay": self.decay, "mean": self.mean, "var": self.var, "n": self.n}


ADV_NORMS = ("dr", "group-std")


def grpo_advantages(rewards: dict, groups: dict, eps: float = 1e-4, baseline: RunningBaseline | None = None, norm: str = "group-std",
                    scale: dict | None = None) -> dict:
    """Per key (a game's learner seat) its advantage within its group;
    `groups`: key → group key.

    - `norm="dr"` (Dr. GRPO, Liu et al. 2025): A = r − mean of its group, no
      division by the group's std (which turns near-equal noisy rewards into
      unit-scale advantages and weighs groups by how little they differ);
      then divided by `scale[group key]` when given (the suite's running
      scale, `RunningScale`), so the suites mix at comparable sizes;
    - `norm="group-std"` (the original GRPO): A = (r − mean) / (std of the
      group + eps), the std the population one.

    A group of one uses `baseline` (0 without one): r − its mean, divided by
    its std under `group-std`."""
    if norm not in ADV_NORMS:
        raise ValueError(f"advantage norm {norm!r}: one of {ADV_NORMS}")
    members = defaultdict(list)
    for k in rewards:
        members[groups[k]].append(k)
    out = {}
    for g in sorted(members, key=str):
        ks = members[g]
        r = np.asarray([rewards[k] for k in ks], np.float64)
        div = float(scale.get(g, 1.0)) if scale else 1.0
        if len(ks) == 1:
            if baseline is None:
                out[ks[0]] = 0.0
            elif norm == "dr":
                out[ks[0]] = (float(r[0]) - baseline.mean) / div if baseline.n else 0.0
                baseline.update(float(r[0]))
            else:
                out[ks[0]] = baseline.advantage(float(r[0]), eps)
            continue
        if norm == "dr":
            a = (r - r.mean()) / div
        else:
            std = r.std()
            a = (r - r.mean()) / (std + eps) if std > 1e-12 else np.zeros_like(r)  # equal rewards: no advantage (not rounding noise)
        out.update({k: float(x) for k, x in zip(ks, a)})
    return out


def zero_signal(rewards, tol: float) -> bool:
    """A group whose rewards are all equal within `tol` (max − min ≤ tol):
    it carries no learning signal (every advantage ≈ 0) and is dropped."""
    r = np.asarray(list(rewards), np.float64)
    return len(r) > 1 and float(r.max() - r.min()) <= tol


class RunningScale:
    """The scale of a suite's (or battle kind's) advantages: the square root
    of an exponential moving mean of the raw advantages' square (r − its
    group's mean; per turn for a dense battle), pooled over the iteration's
    groups and averaged across iterations (`decay`: the old value's
    weight). The pooled within-group spread, not the spread of the rewards
    across groups: the scenes' differences are no signal. Kept in the loop's
    state (`state.json` `adv_scale`)."""

    def __init__(self, decay: float = 0.8, var: float = 0.0, n: int = 0):
        self.decay, self.var, self.n = decay, var, n

    def update(self, sq_mean: float):
        """One iteration's mean square of the raw advantages."""
        if not math.isfinite(sq_mean):
            return
        self.var = sq_mean if self.n == 0 else self.decay * self.var + (1 - self.decay) * sq_mean
        self.n += 1

    def std(self, eps: float = 0.0, floor: float = 0.0) -> float:
        """The divisor: max(sqrt(var), floor) + eps (1 before any update).
        The floor keeps a suite whose rewards barely differ within a group
        (a spread far below the reward's own unit) from having that noise
        blown up to unit advantages."""
        return max(math.sqrt(self.var), floor) + eps if self.n else 1.0

    def state(self) -> dict:
        return {"decay": self.decay, "var": self.var, "n": self.n}


def gae(values: np.ndarray, reward: float, lam: float) -> np.ndarray:
    """GAE(λ), γ = 1, of one window of turns ending the game: δ_t = V_{t+1} −
    V_t, the last δ = r − V_last; A_t = Σ_k λ^k δ_{t+k}."""
    v = np.asarray(values, np.float64)
    nxt = np.append(v[1:], reward)
    delta = nxt - v
    out = np.zeros_like(v)
    acc = 0.0
    for t in range(len(v) - 1, -1, -1):
        acc = delta[t] + lam * acc
        out[t] = acc
    return out


def read_games(logs) -> dict:
    """`<stem>.e2.json` of each log, by the log's path."""
    out = {}
    for log in logs:
        p = Path(log)
        meta = p.with_name(p.name[: -len(".aurlog")] + ".e2.json") if p.name.endswith(".aurlog") else p.with_suffix(".e2.json")
        with open(meta) as f:
            out[str(p)] = json.load(f)
    return out


def group_key(log: str, game: dict):
    """The GRPO group of a game: its directory and group (a group of one: the game)."""
    return (str(Path(log).parent), int(game["group"])) if int(game.get("group_size", 1)) > 1 else (str(log), -1)


def seat_reward(game: dict, player: int, a, tech_share: float | None = None, value: float | None = None) -> tuple[dict, dict]:
    """(reward per component, weight per component) of a learner seat: one
    component, `--reward`, or with `--aim` one per aim of the seat's aim
    weights (each aim its reward, AIM_REWARD); a training ground's suite
    reward (`battle_reward`, `economy_return` with `value`, the value head
    at the learner's last turn) as the component of its suite."""
    kw = dict(time_weight=a.time_weight, early_bonus=a.early_bonus, phi_weight=a.phi_weight, tech_share=tech_share)
    g = ground_of(game)
    if g is not None and g["suite"] == "battle":
        return {"battle": battle_reward(game, getattr(a, "siege_shaping", SIEGE_SHAPING), getattr(a, "siege_speed_decay", 0.0))}, {"battle": 1.0}
    if g is not None and g["suite"] == "economy":
        return {"economy": economy_reward(game, player, value, getattr(a, "economy_reward", "potential"), getattr(a, "economy_gamma", 0.99),
                                          getattr(a, "economy_bootstrap", False))}, {"economy": 1.0}
    if g is not None and g["suite"] == "opening":
        return {"opening": opening_reward(game, player, opening_weights(a))}, {"opening": 1.0}
    if getattr(a, "aim", False):
        w = aim_weights(game, player)
        if w is None:
            raise ValueError(f"--aim: game {game.get('stem')} records no aim for player {player} (ai-selfplay must draw it, feed it to the net and write it)")
        comps = {aim: reward_of(game, player, AIM_REWARD[aim], **kw) for aim, wk in zip(AIMS, w) if wk}
        return comps, {aim: float(wk) for aim, wk in zip(AIMS, w) if wk}
    return {a.reward: reward_of(game, player, a.reward, **kw)}, {a.reward: 1.0}


def scale_key(game: dict, comp: str) -> str:
    """The key of a reward component's running scale and of the per-suite
    advantage logs: the suite and battle kind (`suite_key`), and the
    component when a whole game has several (`--aim`)."""
    k = suite_key(game)
    return k if comp in GROUND_SUITES or ground_of(game) is not None else f"{k}/{comp}"


def needs_shard(game: dict, a) -> bool:
    """Whether a game's reward needs its shard (the science reward's
    stand-in for a seat without the spaceship record; an economy window's
    bootstrap from the value head): its group's advantages are then made
    only when the shards arrive, it is never dropped up front, and its key
    is scaled only once another iteration gave it a running scale."""
    g = ground_of(game)
    if g is not None:
        return g["suite"] == "economy" and getattr(a, "economy_bootstrap", False)
    if a.reward == "science" or getattr(a, "aim", False):
        learners = {int(p) for p in game["learners"]}
        return any(science_progress(s)[1] != "record" for s in game["seats"] if int(s["player"]) in learners)
    return False


def dense_battle(game: dict, a) -> bool:
    """A battle trained per turn (`--battle-reward dense`, a round-by-round record), or an opening (always per turn)."""
    g = ground_of(game)
    if g is not None and g["suite"] == "opening":
        return len((g.get("opening") or {}).get("round") or []) >= 2
    return (g is not None and g["suite"] == "battle" and getattr(a, "battle_reward", "dense") == "dense"
            and len(((g.get("battle") or {}).get("rounds") or {}).get("round") or []) >= 2)


def opening_weights(a) -> dict:
    """The opening's Φ weights of the run (`--opening-weights` over the defaults)."""
    return parse_opening_weights(getattr(a, "opening_weights", ""))


def dense_turn_rewards(game: dict, player: int, a, siege_shaping: float) -> tuple[np.ndarray, np.ndarray] | None:
    """The per-turn rewards of a dense ground: an opening's (`opening_turn_rewards`) or a battle's (`battle_turn_rewards`)."""
    if ground_of(game)["suite"] == "opening":
        return opening_turn_rewards(game, player, opening_weights(a))
    return battle_turn_rewards(game, siege_shaping, getattr(a, "siege_speed_decay", 0.0))


DECISION_BASELINES = ("mean", "best")


def decision_terms(game: dict, baseline: str = "mean") -> np.ndarray:
    """Q(chosen) − baseline of each valued decision of a game
    (`.e2.json` `decision_values.values`, `ai-selfplay --decision-values`:
    the search teacher's rollouts from the decision's world, the ground's
    reward); `baseline` "mean": the mean Q of the candidates valued (the
    net's choice among them), "best": the best's (never positive)."""
    if baseline not in DECISION_BASELINES:
        raise ValueError(f"decision baseline {baseline!r}: one of {DECISION_BASELINES}")
    vals = ((game.get("decision_values") or {}).get("values")) or []
    key = "q_mean" if baseline == "mean" else "q_best"
    return np.asarray([float(v["q_chosen"]) - float(v[key]) for v in vals], np.float64)


def decision_report(games, baseline: str = "mean") -> dict:
    """The valued decisions of an iteration's games: how many, the share
    whose choice was the best valued (to 1e-6), the raw term's mean and
    RMS, the valuing's seconds (one core, summed) and its stats summed."""
    terms, best, stats = [], 0, defaultdict(float)
    with_values = 0
    for game in games:
        dv = game.get("decision_values")
        if not dv:
            continue
        with_values += 1
        for k, v in (dv.get("stats") or {}).items():
            stats[k] += float(v)
        for v in dv.get("values") or []:
            best += int(float(v["q_chosen"]) >= float(v["q_best"]) - 1e-6)
        terms.extend(decision_terms(game, baseline))
    x = np.asarray(terms, np.float64)
    return {"games": with_values, "valued": len(x), "chosen_best_share": best / len(x) if len(x) else float("nan"),
            "term_mean": float(x.mean()) if len(x) else float("nan"), "term_rms": float(np.sqrt((x ** 2).mean())) if len(x) else float("nan"),
            "nonzero_share": float((np.abs(x) > 1e-6).mean()) if len(x) else float("nan"), "stats": dict(stats)}


def plan_advantages(games: dict, a, scales: dict | None = None) -> dict:
    """The iteration's advantages that need no shard, made before any is
    materialised, over its whole groups (`games` {log: .e2.json}):

    - dense battles (`--battle-reward dense`, a round-by-round record):
      per-turn rewards (`battle_turn_rewards`) and GAE advantages against
      the group's leave-one-out reward-to-go (`battle_gae`), per kind;
    - every other group of several (not `needs_shard`): its rewards'
      differences from the group's mean (Dr. GRPO's raw advantage).

    A group whose raw advantages are all within `--zero-tol` of 0 (equal
    rewards) is a zero-signal group: dropped from the iteration's training
    and counted per suite and kind — unless `--decision-advantage` > 0 and
    one of its decisions has a term beyond the tolerance (`decision_terms`):
    its decisions still learn how good each choice was. The mean square of the kept raw
    advantages updates each key's `RunningScale` in `scales` (`--adv-norm
    dr` with `--adv-scale ema`), and the dense advantages are divided by it.
    With `--dense-potential W` > 0 every other game whose `.e2.json`
    records the potential (`dense_potential`) also gets per-turn shaping
    rewards (`potential_turn_rewards`) and their GAE(`--dense-gamma`,
    `--dense-lambda`) advantages against the group's leave-one-out
    reward-to-go (`battle_gae`), per suite (key `<suite>/dense`, its own
    running scale), × W: `game_targets` adds them to the game's outcome
    advantage turn by turn. A group is then dropped only when its outcome
    and its shaping both carry no signal.

    Returns {"drop": logs, "dense": {(log, player): {round: advantage}},
    "shaping": {(log, player): {round: W × scaled shaping advantage}},
    "scale": {key: the divisor of its advantages} (the decision term's too),
    "stats": {key: groups, zero-signal groups and share, seats, raw RMS,
    scale, scaled RMS, scaled |max|}}."""
    tol = getattr(a, "zero_tol", 1e-3)
    dec_on = float(getattr(a, "decision_advantage", 0.0) or 0.0) > 0
    dec_base = getattr(a, "decision_baseline", "mean")
    dec_signal = lambda logs: dec_on and any(len(x) and float(np.abs(x).max()) > tol for x in (decision_terms(games[lg], dec_base) for lg in logs))
    norm = getattr(a, "adv_norm", "dr")
    use_scale = norm == "dr" and getattr(a, "adv_scale", "ema") == "ema" and scales is not None
    by_group = defaultdict(list)
    for log in sorted(games):
        by_group[group_key(log, games[log])].append(log)
    stats = defaultdict(lambda: {"groups": 0, "zero_signal": 0, "kept_by_decisions": 0, "seats": 0, "sq": 0.0, "n": 0, "absmax": 0.0})
    drop, raw_dense = set(), {}
    dense_groups = defaultdict(dict)  # kind → {group: {key: (rounds, rewards)}}
    shaping = getattr(a, "siege_shaping", SIEGE_SHAPING)
    # The potential's shaping: suite → {group: {key: (rounds, rewards)}}, every group (one-game groups and
    # those whose outcome needs a shard too: the shaping needs none).
    pot_groups = defaultdict(dict)
    for gk, logs in sorted(by_group.items(), key=lambda kv: str(kv[0])):
        members = {}
        for lg in logs:
            if dense_potential(games[lg], a):
                for p in games[lg]["learners"]:
                    rr = potential_turn_rewards(games[lg], p)
                    if rr is not None and len(rr[0]):
                        members[(lg, int(p))] = rr
        if members:
            pot_groups[suite_key(games[logs[0]])][gk] = members
    gamma_d, lam_d = getattr(a, "dense_gamma", 0.99), getattr(a, "dense_lambda", 0.95)
    raw_shaping, shaping_signal = {}, set()
    for suite, groups in pot_groups.items():
        adv = battle_gae(groups, gamma_d, lam_d)
        key = f"{suite}/dense"
        s = stats[key]
        for gk, members in groups.items():
            s["groups"] += 1
            s["seats"] += len(members)
            xs = np.concatenate([adv[k][1] for k in members])
            if not len(xs) or float(np.abs(xs).max()) <= tol:
                s["zero_signal"] += 1
                continue
            shaping_signal.add(gk)
            s["sq"] += float((xs ** 2).sum())
            s["n"] += len(xs)
            s["absmax"] = max(s["absmax"], float(np.abs(xs).max()))
            for k in members:
                raw_shaping[k] = (key, adv[k])
    for gk, logs in sorted(by_group.items(), key=lambda kv: str(kv[0])):
        if len(logs) < 2:
            continue  # a group of one: the running baseline's, made with the shards
        if any(needs_shard(games[lg], a) for lg in logs):
            continue
        g0 = games[logs[0]]
        if all(dense_battle(games[lg], a) for lg in logs):
            members = {}
            for lg in logs:
                for p in games[lg]["learners"]:
                    members[(lg, int(p))] = dense_turn_rewards(games[lg], int(p), a, shaping)
            dense_groups[suite_key(g0)][gk] = members
            continue
        comps = {}
        for lg in logs:
            for p in games[lg]["learners"]:
                for n, v in seat_reward(games[lg], p, a)[0].items():
                    comps.setdefault(n, {})[(lg, int(p))] = v
        zero = all(zero_signal(r.values(), tol) for r in comps.values())
        for n, r in comps.items():
            key = scale_key(g0, n)
            s = stats[key]
            s["groups"] += 1
            s["seats"] += len(r)
            if zero:
                s["zero_signal"] += 1
                continue
            x = np.asarray(list(r.values()), np.float64)
            dev = x - x.mean()
            s["sq"] += float((dev ** 2).sum())
            s["n"] += len(dev)
            s["absmax"] = max(s["absmax"], float(np.abs(dev).max()))
        if zero and gk not in shaping_signal:
            if dec_signal(logs):
                stats[scale_key(g0, next(iter(comps)))]["kept_by_decisions"] += 1
            else:
                drop.update(logs)
    gamma, lam = getattr(a, "battle_gamma", 0.99), getattr(a, "gae_lambda", 0.95)
    for key, groups in dense_groups.items():
        adv = battle_gae(groups, gamma, lam)
        s = stats[key]
        for gk, members in groups.items():
            s["groups"] += 1
            s["seats"] += len(members)
            xs = np.concatenate([adv[k][1] for k in members]) if members else np.zeros(0)
            if not len(xs) or float(np.abs(xs).max()) <= tol:
                s["zero_signal"] += 1
                if dec_signal({lg for lg, _ in members}):
                    # No outcome signal, but the decisions' values differ: kept, its turn advantages ≈ 0.
                    s["kept_by_decisions"] += 1
                    for k in members:
                        raw_dense[k] = (key, adv[k])
                else:
                    drop.update(lg for lg, _ in members)
                continue
            s["sq"] += float((xs ** 2).sum())
            s["n"] += len(xs)
            s["absmax"] = max(s["absmax"], float(np.abs(xs).max()))
            for k in members:
                raw_dense[k] = (key, adv[k])
    out_stats = {}
    for key, s in sorted(stats.items()):
        sq_mean = s["sq"] / s["n"] if s["n"] else float("nan")
        div = 1.0
        if use_scale:
            if s["n"]:
                scales.setdefault(key, RunningScale(getattr(a, "adv_scale_decay", 0.8))).update(sq_mean)
            div = scales[key].std(a.adv_eps, getattr(a, "adv_floor", 0.0)) if key in scales else 1.0
        rms = math.sqrt(sq_mean) if s["n"] else float("nan")
        out_stats[key] = {"groups": s["groups"], "zero_signal": s["zero_signal"], "zero_share": s["zero_signal"] / max(s["groups"], 1),
                          "kept_by_decisions": s["kept_by_decisions"], "seats": s["seats"],
                          "raw_rms": rms, "raw_absmax": s["absmax"], "scale": div, "scaled_rms": rms / div if s["n"] else float("nan"),
                          "scaled_absmax": s["absmax"] / div}
    dense = {}
    for k, (key, (rounds, x)) in raw_dense.items():
        div = out_stats[key]["scale"]
        dense[k] = {int(r): float(v) / div for r, v in zip(rounds, x)}
    w = float(getattr(a, "dense_potential", 0.0) or 0.0)
    shaped = {}
    for k, (key, (rounds, x)) in raw_shaping.items():
        div = out_stats[key]["scale"]
        shaped[k] = {int(r): w * float(v) / div for r, v in zip(rounds, x)}
    return {"drop": drop, "dense": dense, "shaping": shaped, "stats": out_stats, "scale": {k: v["scale"] for k, v in out_stats.items()}}


def game_targets(entries, a, baselines: dict | None = None, scales: dict | None = None, dense: dict | None = None,
                 shaping: dict | None = None, adv_scale: dict | None = None) -> dict:
    """Per log {player: (reward, advantage, aim weights or None, value
    target, per-round advantages or None, the advantage of a turn without
    a per-round one[, the decision term's coefficient, with `--decision-advantage`])} for the learner seats of
    `entries` [(log, game meta, tech share of a player or None[, value head
    at the player's last turn or None])], whole groups. Each reward
    component gets its advantage within its group (`grpo_advantages`,
    `--adv-norm`; under `dr` divided by the component's running scale in
    `scales`, `scale_key`) before the components are summed by the aim
    weights (GDPO); a group of one uses the component's running baseline. A
    dense battle's advantages are per round (`dense`, `plan_advantages`);
    its game advantage is their mean (logs only). With the potential's
    shaping (`shaping`, `plan_advantages`, `--dense-potential`) a turn's
    advantage is the game's outcome advantage + its round's shaping
    advantage (a turn without one: the outcome's alone). The value target
    is the reward of a whole game (the outcome, never the shaping), NaN for
    a training ground (the value head learns whole-game outcomes only).
    The decision term's coefficient is `--decision-advantage` λ divided by
    the divisor of the game's turn advantages (`adv_scale`, `plan_advantages`'
    "scale" by `suite_key`; 1 without one): a decision's advantage is its
    turn's + λ × (Q(chosen) − baseline) / the same scale (`E2Shard.turn`)."""
    comps, weights, groups = {}, {}, {}
    games = {}
    for entry in entries:
        log, game, share = entry[:3]
        value = entry[3] if len(entry) > 3 else None
        games[log] = game
        for p in game["learners"]:
            key = (log, int(p))
            comps[key], weights[key] = seat_reward(game, p, a, share(p) if share else None, value(p) if value else None)
            groups[key] = group_key(log, game)
    names = sorted({n for c in comps.values() for n in c})
    norm = getattr(a, "adv_norm", "group-std")
    use_scale = norm == "dr" and getattr(a, "adv_scale", "ema") == "ema" and scales is not None
    adv = defaultdict(float)
    for n in names:
        base = None
        if baselines is not None:
            base = baselines.setdefault(n, RunningBaseline(a.baseline_decay))
        r = {k: c[n] for k, c in comps.items() if n in c}
        div = None
        if use_scale:
            div = {}
            for k in r:
                sc = scales.get(scale_key(games[k[0]], n))
                div[groups[k]] = sc.std(a.adv_eps, getattr(a, "adv_floor", 0.0)) if sc is not None else 1.0
        for k, x in grpo_advantages(r, {k: groups[k] for k in r}, a.adv_eps, base, norm, div).items():
            adv[k] += weights[k][n] * x
    out = defaultdict(dict)
    for (log, p), c in comps.items():
        reward = sum(weights[(log, p)][n] * v for n, v in c.items())
        aim = None
        if getattr(a, "aim", False) and ground_of(games[log]) is None:
            aim = aim_weights(games[log], p)
        value_target = float("nan") if ground_of(games[log]) is not None else reward
        per_round = (dense or {}).get((log, int(p)))
        default = 0.0
        shaped = (shaping or {}).get((log, int(p)))
        if per_round is None and shaped:
            default = adv[(log, p)]
            per_round = {r: default + x for r, x in shaped.items()}
        a_game = float(np.mean(list(per_round.values()))) if per_round else adv[(log, p)]
        lam_dec = float(getattr(a, "decision_advantage", 0.0) or 0.0)
        coef = lam_dec / float((adv_scale or {}).get(suite_key(games[log]), 1.0)) if lam_dec else 0.0
        # The coefficient only when the term is on: the six-element targets otherwise, as before.
        out[log][p] = (reward, a_game, aim, value_target, per_round, default) + ((coef,) if coef else ())
    return dict(out)


# ---------------------------------------------------------------------------
# Shards


class E2Shard(Shard):
    """A shard of `ai-features --e2`: the format-5 arrays plus the E2 files;
    `turn` adds per decision the behaviour log-probability, the accepted mask
    of its candidates, its row and advantage, and sets the turn's value
    target to the learner's reward (`set_targets`)."""

    def __init__(self, path, in_memory: bool = False):
        super().__init__(path, in_memory)
        p = self.path
        self.e2_logp = np.asarray(np.load(p / "e2_logp.npy"), np.float32)
        self.e2_accepted = np.asarray(np.load(p / "e2_accepted.npy")).astype(bool)
        self.e2_value = np.asarray(np.load(p / "e2_value.npy"), np.int64)
        self.e2_info = json.loads((p / "e2.json").read_text()) if (p / "e2.json").exists() else {}
        self.e2_meta = self.meta["e2"]
        self.temperature = float(self.e2_meta["temperature"])
        # The search's values of the decisions (`ai-selfplay --decision-values`): [Q chosen, Q mean, Q best, count], NaN where not valued.
        dv = p / "e2_dvalues.npy"
        self.e2_dvalues = np.asarray(np.load(dv), np.float32).reshape(-1, 4) if dv.exists() else np.full((len(self.e2_logp), 4), np.nan, np.float32)
        if len(self.e2_logp) != len(self.decisions) or len(self.e2_accepted) != len(self.candidates) or len(self.e2_value) != len(self.turns) \
                or len(self.e2_dvalues) != len(self.decisions):
            raise ValueError(f"{p}: E2 arrays not aligned with the shard")
        self.dec_coef: dict[int, float] = {}
        # Per candidate row: the search's value Q of a scored candidate of a valued decision (the
        # sidecar's `decision_values.values[].candidates/q`, found by the decision's round and
        # ordinal), NaN elsewhere: the target of `--decision-distill`.
        self.e2_cq = np.full(len(self.e2_accepted), np.nan, np.float32)
        vals = {(int(v["round"]), int(v["ordinal"])): v for v in ((self.e2_info.get("game") or {}).get("decision_values") or {}).get("values") or []}
        if vals and (p / "e2_decisions.npy").exists():
            ordinal = np.load(p / "e2_decisions.npy")[:, 0]
            for i, row in enumerate(self.decisions):
                v = vals.get((int(self.turns[int(row[0]), 0]), int(ordinal[i])))
                if v is None:
                    continue
                first, k = int(row[3]), int(row[4])
                for c, q in zip(v["candidates"], v["q"]):
                    if 0 <= int(c) < k:
                        self.e2_cq[first + int(c)] = q
        self.dec_baseline = "mean"
        self.reward: dict[int, float] = {}
        self.aim: dict[int, np.ndarray] = {}
        self.turn_adv = np.zeros(len(self.turns), np.float32)

    def tech_share(self, player: int) -> float | None:
        """The share of all technologies the player knew at the start of its
        last turn in the shard (the global row's `tech_*` multi-hot): the
        stand-in for Φ_end's technology term while `.e2.json` records no
        spaceship progress."""
        names = self.layout.names["global"]
        cols = [i for i, n in enumerate(names) if n.startswith("tech_")]
        rows = np.nonzero(self.turns[:, 1] == player)[0]
        if not cols or not len(rows):
            return None
        last = rows[np.argmax(self.turns[rows, 0])]
        return float((np.asarray(self.globals[last])[cols] > 0).sum()) / len(cols)

    def last_value(self, player: int) -> float | None:
        """The behaviour value head at the player's last turn in the shard
        (expected final points share), None without one: the bootstrap of a
        truncated episode (an economy window)."""
        rows = np.nonzero(self.turns[:, 1] == player)[0]
        if not len(rows):
            return None
        v = int(self.e2_value[rows[np.argmax(self.turns[rows, 0])]])
        return None if v == MISSING_VALUE else v / 256.0

    def decision_term(self, rows) -> np.ndarray:
        """Q(chosen) − baseline of the decision rows (`decision_terms`), 0 where not valued."""
        q = self.e2_dvalues[rows]
        base = q[:, 1] if self.dec_baseline == "mean" else q[:, 2]
        return np.nan_to_num(q[:, 0] - base, nan=0.0).astype(np.float32)

    def set_targets(self, targets: dict, advantage: str = "grpo", lam: float = 0.95, dec_baseline: str = "mean"):
        """`targets` {player: (reward, game advantage, aim weights or
        None[, value target[, per-round advantages[, default]]])}: per turn
        row the value target (the reward unless given; NaN: none) and the
        advantage: the per-round one of the turn's round (a dense battle's,
        or the outcome's + the potential's shaping, `plan_advantages`; a
        round without one: `default`, 0 unless given), else the game's, or GAE from e2_value (a ground
        keeps the game's: its reward is not on the value head's scale). The
        targets' seventh element, the decision term's coefficient, adds per
        decision coefficient × `decision_term` (`turn`)."""
        self.reward = {int(p): float(t[3] if len(t) > 3 else t[0]) for p, t in targets.items()}
        self.dec_coef = {int(p): float(t[6]) for p, t in targets.items() if len(t) > 6 and t[6]}
        self.dec_baseline = dec_baseline
        self.aim = {int(p): t[2] for p, t in targets.items() if len(t) > 2 and t[2] is not None}
        for w in self.windows:
            player = int(self.turns[w[0], 1])
            t = targets.get(player, (float("nan"), 0.0))
            r, adv = t[:2]
            per_round = t[4] if len(t) > 4 else None
            if per_round is not None:
                default = float(t[5]) if len(t) > 5 else 0.0
                self.turn_adv[w] = [per_round.get(int(x), default) for x in self.turns[w, 0]]
            elif advantage == "gae" and math.isfinite(self.reward.get(player, float("nan"))):
                v = np.where(self.e2_value[w] == MISSING_VALUE, 0, self.e2_value[w]) / 256.0
                self.turn_adv[w] = gae(v, r, lam)
            else:
                self.turn_adv[w] = adv

    def turn(self, t: int):
        turn = super().turn(t)
        player = int(self.turns[t, 1])
        turn.value_target = self.reward.get(player, float("nan"))
        turn.aim_weights = self.aim.get(player)
        # The decision rows Shard.turn kept, in its order (choice in range).
        rows = self._dec_order[self._dec_start[t] : self._dec_start[t + 1]]
        dec = self.decisions[rows]
        if len(dec):
            keep = (dec[:, 4] > 0) & (dec[:, 5] >= 0) & (dec[:, 5] < dec[:, 4])
            rows, dec = rows[keep], dec[keep]
        k = dec[:, 4] if len(dec) else np.zeros(0, np.int64)
        cand = np.repeat(dec[:, 3] - (np.cumsum(k) - k), k) + np.arange(int(k.sum())) if len(dec) else np.zeros(0, np.int64)
        arr = turn.dec_arrays
        if len(rows) != len(arr["head"]) or len(cand) != len(arr["rows"]):
            raise ValueError(f"{self.path}: turn {t}: E2 rows do not match Shard.turn's decisions")
        arr["e2_row"] = rows
        arr["e2_logp"] = self.e2_logp[rows]
        arr["e2_acc"] = self.e2_accepted[cand]
        arr["e2_cq"] = self.e2_cq[cand]
        arr["e2_adv"] = np.full(len(rows), self.turn_adv[t], np.float32)
        coef = self.dec_coef.get(player, 0.0)
        if coef and len(rows):
            # The decision's own credit: how much better its choice was than the search's alternatives.
            arr["e2_adv"] = arr["e2_adv"] + np.float32(coef) * self.decision_term(rows)
        arr["e2_temp"] = np.full(len(rows), self.temperature, np.float32)
        arr["e2_game"] = self.game
        return turn


class E2Materializer(LogMaterializer):
    """LogMaterializer for `ai-features --e2`: the shard of a self-play log
    (its sidecar beside it) as an E2Shard in memory."""

    def materialize(self, log) -> E2Shard:
        work = tempfile.mkdtemp(prefix="aurelia-e2-", dir=self.tmp_root)
        try:
            stem = Path(log).name[: -len(".aurlog")] if str(log).endswith(".aurlog") else Path(log).stem
            cmd = self.command + [str(log), "--e2", "--out", str(work)]
            proc = subprocess.run(cmd, capture_output=True, text=True, timeout=self.timeout)
            if proc.returncode != 0:
                raise RuntimeError(f"{' '.join(cmd)} failed ({proc.returncode}): {proc.stderr.strip()[-2000:]}")
            out = Path(work) / (stem + ".shard")
            if not (out / "meta.json").exists():
                found = sorted(Path(work).glob("*/meta.json"))
                out = found[0].parent if found else Path(work)
            shard = E2Shard(out, in_memory=True)
            shard.game = str(log)
            return shard
        finally:
            shutil.rmtree(work, ignore_errors=True)


# ---------------------------------------------------------------------------
# Batches


class E2Prepared:
    """train_imitation.Prepared plus, per piece, the E2 arrays of its
    decisions laid out as `padded_logits` rows (padding decisions last):
    the accepted mask [D, K], behaviour log-probability, advantage,
    temperature, a real-decision flag; and the (game, row) of each real
    decision for the off-policy mask."""

    def __init__(self, windows, cfg, bptt: int):
        self.bptt = bptt
        self.memory = WindowMemory(windows, cfg, pad=True)
        # The frozen reference's log-probabilities per piece, made in the first
        # epoch and reused by the later ones (`run_e2`, `reuse_ref`).
        self.ref_logp: dict = {}
        self.pieces = []
        self.turns = sum(len(w) for w in windows)
        steps = max(len(w) for w in windows)
        host = Deferred()
        for k0 in range(0, steps, bptt):
            plan, turns = [], []
            for k in range(k0, min(k0 + bptt, steps)):
                active = [i for i, w in enumerate(windows) if k < len(w)]
                plan.append((k, active))
                turns.extend(windows[i][k] for i in active)
            batch = collate(turns, cfg, upload=host, pad=True)
            if cfg.aim:
                z = np.zeros(cfg.n_aims, np.float32)
                batch["aim"] = host(np.stack([z if getattr(t, "aim_weights", None) is None else t.aim_weights for t in turns]).astype(np.float32))
            self.pieces.append((batch, self.memory.segment(plan, batch["ent_id"].shape[1], host), len(turns), e2_piece(turns, batch, host)))


def e2_piece(turns, batch, upload) -> dict:
    n_dec = batch["np"]["n_dec"]
    if not n_dec:
        return {"n_dec": 0}
    arrs = [t.dec_arrays for t in turns]
    k = np.concatenate([a["k"] for a in arrs]).astype(np.int64)
    d_pad, k_max = bucket(n_dec), int(batch["k_max"])
    acc = np.zeros((d_pad, k_max), bool)
    acc[n_dec:, 0] = True  # a padding decision's only position
    dec = np.repeat(np.arange(n_dec), k)
    pos = np.arange(len(dec)) - np.repeat(np.cumsum(k) - k, k)
    acc[dec, pos] = np.concatenate([a["e2_acc"] for a in arrs])
    cq = np.full((d_pad, k_max), np.nan, np.float32)  # the scored candidates' values (`--decision-distill`)
    cq[dec, pos] = np.concatenate([a["e2_cq"] if "e2_cq" in a else np.full(len(a["e2_acc"]), np.nan, np.float32) for a in arrs])
    pad = lambda key, v: np.concatenate([np.concatenate([a[key] for a in arrs]).astype(np.float32), np.full(d_pad - n_dec, v, np.float32)])
    choice = np.concatenate([a["choice"] for a in arrs]).astype(np.int64)
    chosen_ok = acc[np.arange(n_dec), choice]
    valid = np.zeros(d_pad, bool)
    valid[:n_dec] = chosen_ok
    games = [a["e2_game"] for a in arrs for _ in range(len(a["head"]))]
    return {
        "n_dec": n_dec,
        "acc": upload(acc),
        "logp_b": upload(pad("e2_logp", 0.0)),
        "adv": upload(pad("e2_adv", 0.0)),
        "temp": upload(pad("e2_temp", 1.0)),
        "valid": upload(valid),
        "host_valid": valid,
        "host_adv": pad("e2_adv", 0.0)[:n_dec],
        "rows": np.concatenate([a["e2_row"] for a in arrs]),
        "games": games,
        "refused_choice": int((~chosen_ok).sum()),
        "cq": upload(cq),
    }


# ---------------------------------------------------------------------------
# The objective


def ref_log_probs(ref_logits, acc, temp):
    """The reference's log-probabilities over the accepted candidates at
    the decision's temperature, 0 outside them ([D, K])."""
    return F.log_softmax((ref_logits / temp[:, None]).masked_fill(~acc, NEG), dim=1).masked_fill(~acc, 0.0)


def distill_terms(logits, acc, temp, cq, rows, tau: float = 0.1, tol: float = 1e-3):
    """`--decision-distill`: per decision row the cross-entropy of the policy
    (over the accepted candidates at the row's temperature) toward
    softmax(Q / τ) over the candidates the search scored (`cq`, NaN for the
    others), on the rows of `rows` with at least two scored candidates whose
    values differ by more than `tol`; a candidate never sampled is raised
    directly. Returns (cross-entropy per row, the rows used)."""
    scored = torch.isfinite(cq) & acc
    q = torch.where(scored, cq, torch.zeros_like(cq))
    hi = torch.where(scored, q, torch.full_like(q, -1e9)).max(1).values
    lo = torch.where(scored, q, torch.full_like(q, 1e9)).min(1).values
    use = rows & (scored.sum(1) >= 2) & (hi - lo > tol)
    target = torch.softmax(torch.where(scored, q / tau, torch.full_like(q, NEG)), 1).masked_fill(~scored, 0.0)
    lp = F.log_softmax((logits / temp[:, None]).masked_fill(~acc, NEG), dim=1).masked_fill(~acc, 0.0)
    ce = -(target * lp).sum(1)
    return torch.where(use, ce, torch.zeros_like(ce)), use


def e2_terms(logits, ref_logits, acc, choice, logp_b, adv, temp, keep, weight, clip: float = 0.2, beta: float = 0.02, ent_coef: float = 0.001,
             ref_logp=None):
    """Per decision row ([D, K] padded logits in natural units, −inf or any
    value outside `acc`): the PPO surrogate, KL to the reference, entropy,
    the new log-probability and ratio; and the policy loss Σ keep × weight ×
    (surrogate + β KL − ent_coef H) / Σ keep × weight. The reference as its
    logits, or as its log-probabilities already made (`ref_logp`,
    `ref_log_probs`); both None: no KL term."""
    lp = F.log_softmax((logits / temp[:, None]).masked_fill(~acc, NEG), dim=1)
    new = lp.gather(1, choice[:, None]).squeeze(1)
    ratio = torch.exp(new - logp_b)
    surr = -torch.minimum(ratio * adv, ratio.clamp(1 - clip, 1 + clip) * adv)
    p = lp.exp()
    lp0 = lp.masked_fill(~acc, 0.0)
    ent = -(p * lp0).sum(1)
    if ref_logp is None and ref_logits is not None:
        ref_logp = ref_log_probs(ref_logits, acc, temp)
    if ref_logp is not None:
        kl = (p * (lp0 - ref_logp)).sum(1)
    else:
        kl = torch.zeros_like(ent)
    w = keep.to(lp.dtype) * weight
    loss = (w * (surr + beta * kl - ent_coef * ent)).sum() / w.sum().clamp(min=1.0)
    return {"loss": loss, "ratio": ratio, "new": new, "kl": kl, "ent": ent, "surr": surr}


class Stats:
    """Per update (a batch of windows) sums kept on the device, read once."""

    KEYS = ("valid", "kept", "ratio", "ratio_sq", "clip", "kl", "ent", "surr", "v_err", "turns")

    def __init__(self):
        self.sums = None
        self.rmin, self.rmax = [], []
        self.logr = []
        self.host = defaultdict(float)

    def add(self, **vals):
        dev = next((v.device for v in vals.values() if torch.is_tensor(v)), torch.device("cpu"))
        t = torch.stack([vals[k].float() if torch.is_tensor(vals[k]) else torch.tensor(float(vals[k]), device=dev) for k in self.KEYS])
        self.sums = t if self.sums is None else self.sums + t

    def read(self) -> dict:
        s = dict(zip(self.KEYS, self.sums.cpu().tolist())) if self.sums is not None else {k: 0.0 for k in self.KEYS}
        v, kept = max(s["valid"], 1.0), max(s["kept"], 1.0)
        out = {
            "decisions": int(s["valid"]),
            "masked_frac": 1.0 - s["kept"] / v,
            "ratio_mean": s["ratio"] / kept,
            "ratio_std": math.sqrt(max(s["ratio_sq"] / kept - (s["ratio"] / kept) ** 2, 0.0)),
            "clip_frac": s["clip"] / kept,
            "kl": s["kl"] / kept,
            "entropy": s["ent"] / kept,
            "surrogate": s["surr"] / kept,
            "value_mse": s["v_err"] / max(s["turns"], 1.0),
            "turns": int(s["turns"]),
        }
        if self.rmin:
            out["ratio_min"] = float(torch.stack(self.rmin).min().cpu())
            out["ratio_max"] = float(torch.stack(self.rmax).max().cpu())
        if self.logr:
            lr = np.concatenate([x.cpu().numpy()[v] for x, v in self.logr])
            if len(lr):
                out["abs_log_ratio_mean"] = float(np.abs(lr).mean())
                out["log_ratio_p01_p99"] = [float(np.quantile(lr, 0.01)), float(np.quantile(lr, 0.99))]
        return out | dict(self.host)


def forward_tokens(model: Net, batch, seg_views, memory: WindowMemory):
    """The memory-carrying part of train_imitation.run_windows for one
    piece: (logits, tokens, embed offsets)."""
    emb = model.embed(batch)
    off = emb["off"]
    tokens = []
    for turn in seg_views:
        mem = memory.read(turn)
        tok = model.attend(Net.rows(emb, turn["start"], turn["stop"]), *mem)
        memory.write(model, turn, tok, off, mem)
        tokens.append(tok)
    tokens = torch.cat(tokens)
    return model.score(batch, emb["tile_h"], tokens, off), tokens, off


def run_e2(model: Net, ref: Net | None, prepared: E2Prepared, device, a, stats: Stats, opt=None, mask_store: dict | None = None, first_epoch: bool = True,
           rules=None, on_first=None, reuse_ref: bool = True):
    """Runs a batch of game windows piece by piece (bptt turns), the model
    and the frozen reference each with its memory; with `opt`, one step per
    piece. `mask_store` {game: bool per decision row} records the off-policy
    mask in the first epoch and applies it in the later ones. The reference
    is frozen, so with `reuse_ref` its log-probabilities are made once (the
    first run over `prepared`) and kept on the device for the later epochs,
    which then run no reference pass."""
    memory = prepared.memory
    memory.start(device)
    rmem = None
    with_dec = sum(1 for piece in prepared.pieces if piece[3]["n_dec"])
    cached = reuse_ref and len(prepared.ref_logp) == with_dec and with_dec > 0
    if ref is not None and not cached:
        rmem = copy.copy(memory)
        rmem.start(device)
    upload = Uploader(device, blocking=False)
    c = a.mask_ratio
    pending_masks = []
    for i_piece, (host_batch, host_seg, n_turns, e2) in enumerate(prepared.pieces):
        batch = upload_batch(host_batch, upload)
        seg = WindowMemory.views(upload_batch(host_seg, upload))
        logits, tokens, off = forward_tokens(model, batch, seg, memory)
        loss = None
        if e2["n_dec"]:
            ex = upload_batch(e2, upload)
            pad = padded_logits(batch, logits)
            ref_lp = None
            if ref is not None:
                if cached:
                    ref_lp = prepared.ref_logp[i_piece]
                else:
                    with torch.no_grad():
                        rlogits, _, _ = forward_tokens(ref, batch, seg, rmem)
                        ref_lp = ref_log_probs(padded_logits(batch, rlogits), ex["acc"], ex["temp"])
                    if reuse_ref:
                        prepared.ref_logp[i_piece] = ref_lp
            weight = torch.ones(len(pad), device=pad.device)
            if rules or a.wait_weight != 1.0:
                w = host_decision_weights(batch, a.wait_weight, rules)
                weight = upload(np.pad(w, (0, len(pad) - len(w))), pad.dtype)
            valid = ex["valid"]
            if first_epoch or mask_store is None:
                with torch.no_grad():
                    lp = F.log_softmax((pad / ex["temp"][:, None]).masked_fill(~ex["acc"], NEG), dim=1)
                    ratio0 = torch.exp(lp.gather(1, batch["dec_choice"][:, None]).squeeze(1) - ex["logp_b"])
                    keep = valid & (ratio0 >= 1.0 / c) & (ratio0 <= c)
                if mask_store is not None:
                    pending_masks.append((e2["games"], e2["rows"], keep[: e2["n_dec"]]))
            else:
                km = np.zeros(len(pad), bool)
                km[: e2["n_dec"]] = [mask_store.get(g, {}).get(int(r), True) for g, r in zip(e2["games"], e2["rows"])]
                keep = valid & upload(km)
            terms = e2_terms(pad, None, ex["acc"], batch["dec_choice"], ex["logp_b"], ex["adv"], ex["temp"], keep, weight, a.clip, a.beta if ref is not None else 0.0, a.entropy,
                             ref_logp=ref_lp)
            loss = terms["loss"]
            lam_d = float(getattr(a, "decision_distill", 0.0) or 0.0)
            if lam_d > 0:
                # Toward the search's softmax(Q/τ) over the scored candidates: an unsampled better one is raised too.
                ce, used = distill_terms(pad, ex["acc"], ex["temp"], ex["cq"], valid, getattr(a, "distill_tau", 0.1))
                n_used = used.sum()
                loss = loss + lam_d * ce.sum() / n_used.clamp(min=1)
                stats.host["distill_rows"] += float(n_used.detach())
                stats.host["distill_ce"] += float(ce.detach().sum())
            r = terms["ratio"].detach()
            kf = keep.to(r.dtype)
            if on_first is not None:
                on_first(r[valid], (terms["new"].detach() - ex["logp_b"])[valid])
                on_first = None
            stats.add(valid=valid.sum(), kept=kf.sum(), ratio=(r * kf).sum(), ratio_sq=(r * r * kf).sum(),
                      clip=(((r - 1).abs() > a.clip).to(r.dtype) * kf).sum(), kl=(terms["kl"].detach() * kf).sum(),
                      ent=(terms["ent"].detach() * kf).sum(), surr=(terms["surr"].detach() * kf).sum(), v_err=0.0, turns=0)
            stats.rmin.append(torch.where(valid, r, torch.full_like(r, float("inf"))).min())
            stats.rmax.append(torch.where(valid, r, torch.full_like(r, float("-inf"))).max())
            stats.logr.append(((terms["new"].detach() - ex["logp_b"]), e2["host_valid"]))
            stats.host["refused_choice"] += e2["refused_choice"]
        target = host_batch["np"]["value_target"]
        ok = ~np.isnan(target)
        if ok.any():
            okt = upload(ok.astype(np.float32))
            err = model.value(batch, tokens, off) - upload(np.nan_to_num(target).astype(np.float32))
            sq = (err * err * okt).sum()
            vloss = a.value_weight * sq / float(ok.sum())
            loss = vloss if loss is None else loss + vloss
            stats.add(valid=0, kept=0, ratio=0, ratio_sq=0, clip=0, kl=0, ent=0, surr=0, v_err=sq.detach(), turns=float(ok.sum()))
        if opt is not None and loss is not None:
            opt.zero_grad(set_to_none=True)
            loss.backward()
            torch.nn.utils.clip_grad_norm_(model.parameters(), a.grad_clip)
            opt.step()
        memory.detach()
    if mask_store is not None:
        for games, rows, keep in pending_masks:
            keep = keep.cpu().numpy()
            for g, r, k in zip(games, rows, keep):
                mask_store.setdefault(g, {})[int(r)] = bool(k)
    upload.sync()
    return prepared.turns


# ---------------------------------------------------------------------------
# One iteration's training


def window_turns(shards):
    """Each shard's windows (its learners' runs of turns) as lists of turns."""
    return [[s.turn(int(t)) for t in w] for s in shards for w in s.windows]


def train_games(model: Net, ref: Net | None, opt, logs: list[str], a, device, rng: np.random.Generator, baselines: dict | None = None,
                log=print, step0: int = 0, scales: dict | None = None) -> dict:
    """Trains `a.epochs` epochs over the games `logs` (their shards made on
    the fly, whole GRPO groups to a batch). First the advantages that need
    no shard (`plan_advantages`: dense battles, the running scales, the
    zero-signal groups, which are dropped); the targets of a group are made
    when its shards arrive (the science reward's stand-in reads the shard)
    and kept for the later epochs. `scales` {key: RunningScale} is updated
    (the loop keeps it). Returns the iteration's summary: rewards,
    advantages, per-update records, the first update's ratio, timings."""
    games = read_games(logs)
    games = {k: g for k, g in games.items() if g.get("temperature", 1.0) > 0}
    if scales is None:
        scales = {}
    adv_plan = plan_advantages(games, a, scales)
    played = games
    games = {k: g for k, g in games.items() if k not in adv_plan["drop"]}
    draws = [g["ladder_draw"] for g in played.values() if g.get("ladder_draw")]
    if draws:
        # `--ladder-solvable`: per rung, the scenes drawn again and those none of whose draws was solvable.
        by_rung = defaultdict(list)
        for g in played.values():
            if g.get("ladder_draw"):
                by_rung[(ground_of(g) or {}).get("battle", {}).get("rung")].append(g["ladder_draw"])
        for rung, ds in sorted(by_rung.items(), key=lambda kv: (kv[0] is None, kv[0])):
            log(f"  ladder filter rung {rung}: {len(ds)} games, redrawn {sum(d['attempt'] > 0 for d in ds) / len(ds):.3f}, "
                f"unsolvable {sum(not d['solved'] for d in ds) / len(ds):.3f}, draws {sum(d['draws'] for d in ds) / len(ds):.2f} a scene")
    dec = decision_report(played.values(), getattr(a, "decision_baseline", "mean"))
    if dec["games"]:
        st = dec["stats"]
        log(f"  decision values: {dec['valued']} decisions in {dec['games']} games (of {int(st.get('decisions', 0))}; sampled out {int(st.get('sampled_out', 0))}, "
            f"skipped {int(st.get('skipped', 0))}, mismatched {int(st.get('mismatched', 0))}), the choice the best {dec['chosen_best_share']:.3f}, "
            f"Q(chosen) − {getattr(a, 'decision_baseline', 'mean')} mean {dec['term_mean']:+.4f} rms {dec['term_rms']:.4f} (non-zero {dec['nonzero_share']:.2f}), "
            f"λ {float(getattr(a, 'decision_advantage', 0.0) or 0.0):g}, {st.get('seconds', 0.0):.0f} core-s")
    for key, s in adv_plan["stats"].items():
        log(f"  advantages {key}: {s['groups']} groups, zero-signal {s['zero_signal']} ({s['zero_share']:.0%}), raw rms {s['raw_rms']:.4f} "
            f"|max| {s['raw_absmax']:.4f}, scale {s['scale']:.4f}, scaled rms {s['scaled_rms']:.3f} |max| {s['scaled_absmax']:.3f}"
            + (f", kept by their decisions {s['kept_by_decisions']}" if s.get("kept_by_decisions") else ""))
    if float(getattr(a, "dense_potential", 0.0) or 0.0) > 0:
        # Games of an older ai-selfplay (format < 3) have no potential: their outcome's advantage alone.
        missing = sum(1 for g in played.values() if not dense_battle(g, a) and not dense_potential(g, a))
        if missing:
            log(f"  dense potential: {missing} of {len(played)} games without the potential record (ai-selfplay before format 3): outcome only")
    by_group = defaultdict(list)
    for log_path in sorted(games):
        by_group[group_key(log_path, games[log_path])].append(log_path)
    group_list = sorted(by_group, key=str)
    mat = E2Materializer(a.features_bin, tmp_root=a.tmp_root)
    rules = parse_decision_weights(a.decision_weight, model_layout(model)) if a.decision_weight else None
    mask_store: dict = {}
    targets: dict = {}
    phi_sources = defaultdict(int)
    step = step0
    first = {}
    summary = {"games": len(played), "games_dropped": len(played) - len(games), "advantages": adv_plan["stats"], "updates": [], "decision_values": dec}

    def on_first(ratio, logr):
        lr = logr.cpu().numpy()
        first.update({"ratio_mean": float(ratio.mean().cpu()), "abs_log_ratio_mean": float(np.abs(lr).mean()) if len(lr) else 0.0,
                      "log_ratio_p01_p99": [float(np.quantile(lr, 0.01)), float(np.quantile(lr, 0.99))] if len(lr) else None,
                      "within_1pct": float((np.abs(lr) < 0.01).mean()) if len(lr) else None, "decisions": int(len(lr))})

    def finish(chunk_shards):
        """Targets of a chunk's games (made once), set on its shards."""
        todo = [s for s in chunk_shards if s.game not in targets]
        if todo:
            entries = [(s.game, games[s.game], s.tech_share, s.last_value) for s in todo]
            if a.reward == "science" or getattr(a, "aim", False):
                for s in todo:
                    for p in games[s.game]["learners"]:
                        seat = next(x for x in games[s.game]["seats"] if int(x["player"]) == int(p))
                        phi_sources[science_progress(seat, s.tech_share(int(p)))[1]] += 1
            targets.update(game_targets(entries, a, baselines, scales, adv_plan["dense"], adv_plan.get("shaping"), adv_plan.get("scale")))
        for s in chunk_shards:
            s.set_targets(targets.get(s.game, {}), a.advantage, a.gae_lambda, getattr(a, "decision_baseline", "mean"))
        return chunk_shards

    t0 = time.time()
    # chunk: each batch of games gets its `epochs` passes while in memory
    # (one replay per game: the feature binary is the costly part); full:
    # whole passes over the iteration's games, each replaying them again.
    passes, inner = (1, a.epochs) if a.epoch_mode == "chunk" else (a.epochs, 1)
    for outer in range(passes):
        rng.shuffle(group_list)
        # A batch is one map size (one neighbour table): groups of a size together, sizes in random order.
        size_of = lambda gk: str((games[by_group[gk][0]].get("spec") or {}).get("size", ""))
        sizes = list(dict.fromkeys(size_of(gk) for gk in group_list))
        chunks = []
        for size in sizes:
            cur = []
            for gk in (g for g in group_list if size_of(g) == size):
                cur += by_group[gk]
                if len(cur) >= a.games_per_batch:
                    chunks.append(cur)
                    cur = []
            if cur:
                chunks.append(cur)
        chunk_of = {lg: i for i, c in enumerate(chunks) for lg in c}
        order = [lg for c in chunks for lg in c]

        def by_map(shards):
            """A chunk's shards split by map (its targets made first, on whole groups)."""
            maps = defaultdict(list)
            for sh in finish(shards):
                maps[sh.map_key].append(sh)
            yield from maps.values()

        def batches():
            buf, at = [], 0
            for shard in stream_shards(order, mat, a.workers, a.prefetch or None):
                if chunk_of[shard.game] != at and buf:
                    yield from by_map(buf)
                    buf = []
                at = chunk_of[shard.game]
                buf.append(shard)
            if buf:
                yield from by_map(buf)

        t_wait = time.time()
        for prepared in prefetch(batches(), lambda shards: E2Prepared(window_turns(shards), model.cfg, a.bptt)):
            wait = time.time() - t_wait
            for e_in in range(inner):
                epoch = outer + e_in
                stats = Stats()
                tb = time.time()
                for g in opt.param_groups:
                    g["lr"] = a.lr * min(1.0, (step + 1) / max(a.warmup, 1))
                run_e2(model, ref, prepared, device, a, stats, opt, mask_store, epoch == 0, rules, on_first if not first else None, reuse_ref=a.reuse_ref)
                rec = {"epoch": epoch, "step": step, **stats.read(), "seconds": time.time() - tb, "data_wait": wait if e_in == 0 else 0.0}
                summary["updates"].append(rec)
                log(f"  e{epoch} u{step}: dec {rec['decisions']} ratio {rec['ratio_mean']:.4f}±{rec['ratio_std']:.4f} [{rec.get('ratio_min', 0):.3f}, {rec.get('ratio_max', 0):.3f}] "
                    f"clip {rec['clip_frac']:.3f} masked {rec['masked_frac']:.4f} kl {rec['kl']:.5f} ent {rec['entropy']:.3f} v_mse {rec['value_mse']:.4f} "
                    f"{rec['turns']} turns {rec['seconds']:.1f}s (waited {rec['data_wait']:.1f}s for data)"
                    + (f" distill {int(rec['distill_rows'])} rows ce {rec['distill_ce'] / max(rec['distill_rows'], 1):.3f}" if rec.get("distill_rows") else ""))
                step += 1
                if device.type == "mps":
                    # Batches of very different sizes leave the MPS allocator's cache
                    # fragmented and growing past the machine's memory (77 GB, swapping);
                    # give it back after every update.
                    torch.mps.empty_cache()
            t_wait = time.time()
        if outer == 0:
            rewards = [t[0] for tt in targets.values() for t in tt.values()]
            advs = [t[1] for tt in targets.values() for t in tt.values()]
            learner = [s for lg in targets for s in games[lg]["seats"] if s["learner"]]
            summary.update({
                "games_trained": len(targets),
                "mean_reward": float(np.mean(rewards)) if rewards else float("nan"),
                "adv_std": float(np.std(advs)) if advs else float("nan"),
                "learner_wins": sum(1 for s in learner if s.get("victory") not in (None, "time")),
                "learner_science_wins": sum(1 for s in learner if s.get("victory") == "science"),
                "learner_points_share": float(np.mean([s["points_share"] for s in learner])) if learner else float("nan"),
                "victory_kinds": dict(_count(s.get("victory") for s in learner if s.get("victory"))),
                "phi_source": dict(phi_sources),
            })
            log(f"games {len(targets)}: mean reward {summary['mean_reward']:.4f}, advantage std {summary['adv_std']:.3f}, "
                f"learner wins {summary['learner_wins']} {summary['victory_kinds']} phi {dict(phi_sources)}")
            # Per run of the iteration (the games' directory: suite, start, opponent), whom the learner beats:
            # every game played, also those dropped as zero-signal (a group all won or all lost), or PFSP's
            # win share against an opponent would be pulled toward ½. The reward is NaN where none was made.
            by_run = defaultdict(list)
            for lg, game in played.items():
                for p_ in game["learners"]:
                    seat = next(x for x in game["seats"] if int(x["player"]) == int(p_))
                    t = targets.get(lg, {}).get(p_, targets.get(lg, {}).get(int(p_)))
                    by_run[Path(lg).parent.name].append((t[0] if t is not None else float("nan"), seat))
            summary["by_run"] = {k: {"games": len(v), "mean_reward": float(np.nanmean([r for r, _ in v])) if any(r == r for r, _ in v) else float("nan"),
                                     "wins": sum(1 for _, x in v if x.get("victory") not in (None, "time")),
                                     "science_wins": sum(1 for _, x in v if x.get("victory") == "science"),
                                     "points_share": float(np.mean([x["points_share"] for _, x in v]))} for k, v in sorted(by_run.items())}
            for k, v in summary["by_run"].items():
                log(f"  {k}: {v['games']} seats, reward {v['mean_reward']:.4f}, wins {v['wins']} (science {v['science_wins']}), points share {v['points_share']:.3f}")
            # Per suite and battle kind: the reward's distribution (each suite has its own reward).
            by_suite = defaultdict(list)
            for lg, tt in targets.items():
                by_suite[suite_key(games[lg])].extend(t[0] for t in tt.values())
            for lg in sorted(adv_plan["drop"]):  # the zero-signal groups' rewards count too
                for p_ in played[lg]["learners"]:
                    c, w = seat_reward(played[lg], p_, a)
                    by_suite[suite_key(played[lg])].append(sum(w[n] * v for n, v in c.items()))
            summary["by_suite"] = {k: reward_stats(v) for k, v in sorted(by_suite.items())}
            summary["battle_outcomes"] = battle_outcomes(list(played.values()))  # every battle played: the curriculum's
            for k, v in summary["battle_outcomes"].items():
                log(f"  battle {k}: {v['games']} games, win share {v['win_share']:.3f}, captures {v['captures']}, difficulty {v['difficulty']:.3f}"
                    + (f", first-turn wins {v['first_turn_wins']}" if k == "siege" else ""))
            for k, v in summary["by_suite"].items():
                log(f"  suite {k}: {v['n']} seats, reward mean {v['mean']:.4f} std {v['std']:.4f} [min {v['min']:.3f}, p10 {v['p10']:.3f}, p50 {v['p50']:.3f}, p90 {v['p90']:.3f}, max {v['max']:.3f}]")
            # Economy windows: the net against the expert per start round, each component's ratio and priced gap.
            summary["economy"] = economy_report([games[lg] for lg in targets])
            log_economy(summary["economy"], log)
    summary["first_update"] = first
    summary["steps"] = step - step0
    summary["train_seconds"] = time.time() - t0
    ups = summary["updates"]
    tot = sum(u["decisions"] for u in ups) or 1
    summary["kl"] = sum(u["kl"] * u["decisions"] for u in ups) / tot
    summary["entropy"] = sum(u["entropy"] * u["decisions"] for u in ups) / tot
    summary["clip_frac"] = sum(u["clip_frac"] * u["decisions"] for u in ups) / tot
    summary["masked_frac"] = sum(u["masked_frac"] * u["decisions"] for u in ups if u["epoch"] == 0) / max(sum(u["decisions"] for u in ups if u["epoch"] == 0), 1)
    summary["data_wait_s"] = sum(u["data_wait"] for u in ups)
    summary["update_s"] = sum(u["seconds"] for u in ups)
    summary["turns_per_s"] = sum(u["turns"] for u in ups) / max(summary["train_seconds"], 1e-9)
    return summary


def battle_outcomes(games) -> dict:
    """Per battle kind: games, the learner's wins (`battle_won`), win share,
    captures (siege), the mean difficulty knob played, the mean reward
    (`battle_reward` at the default shaping: the clash's curriculum)."""
    out = {}
    for g in games:
        gr = ground_of(g)
        if gr is None or gr["suite"] != "battle":
            continue
        o = out.setdefault(gr["kind"], {"games": 0, "wins": 0, "captures": 0, "first_turn_wins": 0, "difficulty": 0.0, "reward": 0.0})
        o["games"] += 1
        o["reward"] += battle_reward(g)
        o["wins"] += int(battle_won(g))
        o["captures"] += int(gr["battle"].get("captured_round") is not None)
        # The siege ladder's same-turn rule: on a scene that allows it (`same_turn`), a win is a capture in the
        # learner's first turn; elsewhere the win as above.
        b = gr["battle"]
        o["first_turn_wins"] += int(b.get("captured_round") == b.get("start_round")) if b.get("same_turn") else int(battle_won(g))
        o["difficulty"] += float(gr["battle"].get("difficulty", 1.0))
    for o in out.values():
        o["win_share"] = o["wins"] / o["games"]
        o["first_turn_share"] = o["first_turn_wins"] / o["games"]
        o["mean_reward"] = o.pop("reward") / o["games"]
        o["difficulty"] /= o["games"]
    return dict(sorted(out.items()))


def reward_stats(xs) -> dict:
    x = np.asarray(xs, np.float64)
    if not len(x):
        return {"n": 0}
    q = np.quantile(x, [0.1, 0.5, 0.9])
    return {"n": int(len(x)), "mean": float(x.mean()), "std": float(x.std()), "min": float(x.min()), "p10": float(q[0]), "p50": float(q[1]),
            "p90": float(q[2]), "max": float(x.max())}


def _count(xs):
    out = defaultdict(int)
    for x in xs:
        out[x] += 1
    return out


def load_model(path, aim: bool = False) -> Net:
    """A checkpoint as a Net; `aim` on a checkpoint without the aim token:
    the same net with the aim token added (its layer zero, every other
    tensor loaded), so an aim of zeros reads as in imitation."""
    model = load_checkpoint(path)
    if not aim or model.cfg.aim:
        return model
    from .train_imitation import init_from

    cfg = copy.deepcopy(model.cfg)
    cfg.aim = 1
    grown = Net(cfg, model_layout(model))
    init_from(grown, path)
    with torch.no_grad():
        grown.aim.weight.zero_()
        grown.aim.bias.zero_()
    return grown


def model_layout(model: Net):
    from .data import Layout

    return Layout.from_json(model.layout_json)


def make_optimizer(model: Net, lr: float, weight_decay: float):
    return torch.optim.AdamW(model.parameters(), lr=lr, weight_decay=weight_decay)


def export_weights(model: Net, path) -> int:
    """The int8 weights file of `model` (export.py), written through a
    temporary file."""
    from .export import write
    from .quantize import quantize

    model_cpu = copy.deepcopy(model).to("cpu").eval()
    tmp = Path(str(path) + ".tmp")
    size = write(quantize(model_cpu, model_layout(model_cpu)), tmp)
    tmp.replace(path)
    return size


def add_arguments(ap: argparse.ArgumentParser):
    g = ap.add_argument_group("E2 objective")
    g.add_argument("--reward", default="science", choices=["science", "victory", "points_share"],
                   help="science: 1 (+ early bonus) a science victory, else phi_weight × Φ_end (spaceship progress at the end); "
                        "victory: 1 (+ early bonus) a real win of any kind, 0 a loss, time_weight × points share on time or without a winner; "
                        "points_share: the bench's points share")
    g.add_argument("--phi-weight", type=float, default=0.3, help="science reward: the weight of Φ_end without a science victory")
    g.add_argument("--aim", action="store_true", help="aim-conditioned: the games' aim weights (science, score, conquest) feed the aim token and "
                                                      "weigh the aims' rewards, each normalised in its group (GDPO); needs the model's aim flag and aims in .e2.json")
    g.add_argument("--economy-reward", default="potential", choices=list(ECONOMY_REWARDS),
                   help="economy windows: potential, log(net / expert potential at the window's end) in [-1, 1] (the yields a turn and the stocks at the "
                        "scripted AI's shadow prices); rate, the same of the science a turn; share, the window's science share")
    g.add_argument("--economy-bootstrap", action="store_true",
                   help="economy windows: add gamma^W × V(final), W the window's rounds (the truncated episode's bootstrap; off: the end's potential "
                        "already values the investment, and V's scale differs)")
    g.add_argument("--economy-gamma", type=float, default=0.99, help="economy windows: the bootstrap's discount (--economy-bootstrap)")
    g.add_argument("--siege-speed-decay", type=float, default=0.0,
                   help="a siege's capture reward 0.5 + 0.5 × this^(rounds after the first) (`siege_capture`: 1 for a capture in the learner's "
                        "first turn); 0: 0.5 + 0.5 × (1 − rounds / length), as before")
    g.add_argument("--siege-shaping", type=float, default=SIEGE_SHAPING,
                   help="a siege without a capture: this weight (< 0.5) × (damage, kills, losses); the loop sets it from the captures it sees")
    g.add_argument("--time-weight", type=float, default=0.5, help="victory reward: the factor of the points share when the game ends on time or without a winner")
    g.add_argument("--early-bonus", type=float, default=0.2, help="victory reward: + early_bonus × (1 − victory round / max rounds)")
    g.add_argument("--advantage", default="grpo", choices=["grpo", "gae"],
                   help="whole games: grpo, per game within its group (--adv-norm); gae, per turn from the behaviour value head")
    g.add_argument("--adv-norm", default="dr", choices=list(ADV_NORMS),
                   help="dr (Dr. GRPO): A = r - the group's mean, no division by the group's std (then --adv-scale); "
                        "group-std: the original GRPO, A = (r - mean) / (the group's std + adv-eps)")
    g.add_argument("--adv-scale", default="ema", choices=["ema", "none"],
                   help="with --adv-norm dr: divide each suite's (battle kind's) advantages by a running RMS of its raw advantages "
                        "(an EMA across groups and iterations, kept in the loop's state), so the suites mix at comparable scales")
    g.add_argument("--adv-scale-decay", type=float, default=0.8, help="the running scale's EMA: the old value's weight per iteration")
    g.add_argument("--zero-tol", type=float, default=1e-3,
                   help="a group whose rewards are equal within this (a dense battle: every per-turn advantage within it of 0) is "
                        "dropped from the iteration (zero signal) and counted per suite and kind")
    g.add_argument("--battle-reward", default="dense", choices=list(BATTLE_REWARDS),
                   help="dense: per-turn battle rewards (damage dealt − taken each round + the end bonus, `battle_turn_rewards`) and "
                        "GAE against the group's leave-one-out reward-to-go; end: one reward at the end, a game advantage")
    g.add_argument("--dense-potential", type=float, default=0.0, metavar="W",
                   help="the potential's per-turn shaping on every suite but the dense battles (standard, duel, pace, economy): "
                        "r_t = log potential's change over the learner's turn (`.e2.json` potential, format 3), GAE against the "
                        "group's leave-one-out reward-to-go, on its own running scale (<suite>/dense), × W added to the outcome's "
                        "advantage per turn; 0 (default): off, the outcome's advantage alone")
    g.add_argument("--dense-gamma", type=float, default=0.99, help="the potential's shaping: the discount of the reward-to-go and GAE")
    g.add_argument("--dense-lambda", type=float, default=0.95, help="the potential's shaping: GAE's lambda")
    g.add_argument("--battle-gamma", type=float, default=0.99, help="dense battles and openings: the discount of the reward-to-go and GAE")
    g.add_argument("--opening-weights", default="", metavar="TERM=W,...",
                   help=f"openings: the weights of Φ's terms over the defaults {OPENING_WEIGHTS} (`opening_phi`)")
    g.add_argument("--gae-lambda", type=float, default=0.95, help="GAE's lambda (dense battles; --advantage gae)")
    g.add_argument("--adv-eps", type=float, default=0.01,
                   help="added to the divisor (the running scale under dr, the group std under group-std): differences far below it give small advantages")
    g.add_argument("--adv-floor", type=float, default=0.05,
                   help="dr with --adv-scale ema: the running scale's floor, in reward units; a suite whose rewards differ within a group by far "
                        "less (near-equal outcomes) gets proportionally small advantages instead of its noise scaled up to unit size")
    g.add_argument("--baseline-decay", type=float, default=0.95, help="the running baseline of groups of one")
    g.add_argument("--clip", type=float, default=0.2, help="PPO ratio clip ε")
    g.add_argument("--mask-ratio", type=float, default=2.0, help="first epoch: mask decisions whose ratio is outside [1/c, c]")
    g.add_argument("--beta", type=float, default=0.02, help="KL to the frozen reference net")
    g.add_argument("--entropy", type=float, default=0.001)
    g.add_argument("--value-weight", type=float, default=0.5)
    g.add_argument("--wait-weight", type=float, default=1.0, help="weight of a unit decision whose choice is wait (1: as any)")
    g.add_argument("--decision-advantage", type=float, default=0.0, metavar="LAMBDA",
                   help="per-decision credit from the search's values (games of ai-selfplay --decision-values): a valued decision's advantage is its "
                        "turn's + λ × (Q(chosen) − baseline) / the turn advantages' scale; a zero-signal group with such a term is kept (0: off)")
    g.add_argument("--decision-distill", type=float, default=0.0, metavar="LAMBDA_D",
                   help="valued decisions: + λ_d × the cross-entropy of the policy toward softmax(Q/τ) over the candidates the search scored "
                        "(rows whose values differ), so a better candidate the net never samples is raised (0: off)")
    g.add_argument("--distill-tau", type=float, default=0.1, help="--decision-distill: the target's temperature τ on the values")
    g.add_argument("--decision-baseline", default="mean", choices=list(DECISION_BASELINES),
                   help="the decision term's baseline: the mean Q of the candidates valued (default) or the best's")
    g.add_argument("--decision-weight", action="append", default=[], metavar="KEY=W", help="as in train_imitation (default none)")
    t = ap.add_argument_group("E2 training")
    t.add_argument("--epochs", type=int, default=2)
    t.add_argument("--epoch-mode", default="chunk", choices=["chunk", "full"],
                   help="chunk: a batch's epochs run back to back while its shards are in memory (each game replayed once); full: whole passes")
    t.add_argument("--lr", type=float, default=1e-4)
    t.add_argument("--weight-decay", type=float, default=0.0)
    t.add_argument("--warmup", type=int, default=20, help="updates of linear learning-rate warm-up (counted over the run)")
    t.add_argument("--grad-clip", type=float, default=1.0)
    t.add_argument("--bptt", type=int, default=8)
    t.add_argument("--no-reuse-ref", dest="reuse_ref", action="store_false",
                   help="recompute the frozen reference every epoch (default: its log-probabilities are made in a batch's first epoch and reused)")
    t.add_argument("--games-per-batch", type=int, default=16, help="games (whole learner windows) per update batch")
    t.add_argument("--features-bin", default="/Users/roman.ivanov/civ/work/nn-data/bin/ai-features-e2")
    t.add_argument("--workers", type=int, default=12, help="feature binaries at once")
    t.add_argument("--prefetch", type=int, default=0, help="shards waiting in memory (default 2 × workers)")
    t.add_argument("--tmp-root", default=None)
    t.add_argument("--device", default="auto")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--games", required=True, nargs="+", help="directories of self-play games (or .aurlog files)")
    ap.add_argument("--init", required=True, help="the checkpoint to train (best.pt)")
    ap.add_argument("--ref", help="the frozen reference (default: --init)")
    ap.add_argument("--out", required=True)
    ap.add_argument("--seed", type=int, default=0)
    add_arguments(ap)
    a = ap.parse_args(argv)
    device = pick_device(a.device)
    model = load_model(a.init, a.aim).to(device)
    ref = load_model(a.ref or a.init, a.aim).to(device).eval()
    for p in ref.parameters():
        p.requires_grad_(False)
    logs = []
    for g in a.games:
        p = Path(g)
        logs += sorted(str(x) for x in p.glob("*.aurlog")) if p.is_dir() else [str(p)]
    print(f"parameters {parameter_count(model)}; device {device}; {len(logs)} games", flush=True)
    opt = make_optimizer(model, a.lr, a.weight_decay)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    summary = train_games(model, ref, opt, logs, a, device, np.random.default_rng(a.seed), {})
    save_checkpoint(out / "last.pt", model, {"e2": {k: v for k, v in summary.items() if k != "updates"}})
    export_weights(model, out / "weights.bin")
    (out / "summary.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps({k: v for k, v in summary.items() if k != "updates"}), flush=True)


if __name__ == "__main__":
    main()
