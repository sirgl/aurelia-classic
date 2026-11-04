"""The E2 self-play loop (classic/docs/ai-neural-architecture.md, «The E2
recipe»): each iteration plays groups of games with the current int8
weights, trains on them (train_e2), exports the next weights, and every few
iterations measures them against `standard` on the bench.

One iteration i:

1. play: G groups × K games with weights iter_{i−1}.bin at temperature T
   (K = `--group-size`, 4, for whole games; `--ground-group-size`, 8, for
   the training grounds)
   (`ai-selfplay`, at `--speed`: quick once the binaries take it, a game
   stopping at its first victory or its last round), 1 learner seat; the
   other seats by `--opponents` (a mix by share: `standard`, the bench's
   books, `league` = one past checkpoint); suites by `--suites` (standard,
   duel, and the training grounds: `battle`, short battles of the kinds of
   `--battle-kinds` against the scripted tactics, `economy`, windows of W
   rounds from the expert's logs (from round 0 too: opening windows), the
   learner's empire at the end against the expert's at the same round
   (`--economy-reward`), `opening`, the suite's games from round 0 for
   `--opening-window` rounds with the learner commanding its units only and
   `standard` everything else, against `standard`, rewarded per turn by
   the change of the opening's potential (`train_e2.opening_phi`);
   classic/docs/ai-net-data.md, «E2: training grounds»); the standard suite's groups mostly mid-game starts
   (`--from-logs`); with `--opponents-pfsp` the opponents' shares are
   reweighted every iteration by the learner's recent results against each
   (prioritised fictitious self-play, `train_e2.pfsp_weights`);
2. train `--epochs` epochs on those games (shards made on the fly; the
   advantages Dr. GRPO style, each suite and battle kind divided by its
   running scale, kept in state.json `adv_scale`; zero-signal groups
   dropped; battles per turn with GAE: `train_e2.plan_advantages`), holding
   the machine's GPU lock (`--gpu-lock`, the flock of `gpu-run.sh`) for the
   training only: play and evals run on the CPU without it;
3. export iter_i.pt (model and optimiser) and iter_i.bin (int8);
4. commit state.json, delete the games; keep the last `--keep` checkpoints
   (and iter_0000, the imitation start);
5. every `--eval-every` iterations: `ai-tournament` against `standard`
   (standard suite, 80 games) in each of `--eval-modes` (all heads; the
   hybrid unit,research + founder,builder scripted); with `--eval-sprt` a
   sequential eval instead: seed blocks until the SPRT decides (at most
   `--eval-max-games`), with `--eval-reference start` on the share minus
   the start net's on the same games; the best kept per mode
   (best-<mode>.pt/.bin); every `--pace-every` iterations the pace suite
   (500 rounds) too, with its science victories by rounds 400 and 500;
   with battles in the mix, the battle eval: per kind the learner's win share
   over `--battle-eval-games` battles (argmax) against the scripted tactics,
   at the fixed setting (difficulty 1, siege shaping 0.4) so evals compare;
   the battles of play follow the curriculum: per kind a difficulty knob
   moved every iteration toward a 50 % win share (`update_curriculum`),
   and for the kinds of `--stage-kinds` a start stage (the reverse
   curriculum: the attackers start at the walls and are moved back a
   round of march at a time as the learner wins; `staged_curriculum`);
6. a row of progress.csv.

With `--exploiter-groups N` a main exploiter (e2_exploiter.py, AlphaStar's)
plays N more groups an iteration against the current main net only, trains
on them as a second learner, and joins the main learner's opponents
(`exploiter-<g>`) whenever it beats the main net clearly.

Everything is resumable from `--state`: a game whose files exist is not
played again, a committed iteration is not trained again, a pending eval is
run first.

    python -m aurelia_nn.e2_loop --state /path/e2/run1 --init small.pt
"""

from __future__ import annotations

import argparse
import contextlib
import csv
import fcntl
import json
import math
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
import torch

from . import train_e2
from .model import save_checkpoint
from .train_imitation import pick_device

NN_DATA = Path("/Users/roman.ivanov/civ/work/nn-data")
# The machine's single GPU lock (`/Users/roman.ivanov/civ/work/gpu-run.sh`: `/usr/bin/lockf -k`, flock(2)).
GPU_LOCK = Path("/Users/roman.ivanov/civ/work/.gpu.lock")
BIN = NN_DATA / "bin"
MODES = {
    "all": {},
    "hybrid": {"AURELIA_NET_HEADS": "unit,research", "AURELIA_NET_SCRIPTED": "founder,builder"},
}
NET_ENV = ("AURELIA_NET_WEIGHTS", "AURELIA_NET_HEADS", "AURELIA_NET_SCRIPTED")
BATTLE_KINDS = train_e2.BATTLE_KINDS
# The kinds `--battle-kinds` takes: the four and the skirmish (`--skirmish-ladder`).
ALL_BATTLE_KINDS = train_e2.ALL_BATTLE_KINDS
SKIRMISH = train_e2.SKIRMISH
CLASH = train_e2.CLASH
# The battle difficulty knob's range (`grounds::DIFFICULTY_RANGE`).
KNOB_RANGE = (0.125, 8.0)
CSV_FIELDS = ("iteration", "games", "mean_reward", "learner_wins", "science_wins", "kl", "entropy", "zero_signal", "first_ratio", "masked", "eval_all", "eval_hybrid",
              "eval_duel_all", "eval_duel_hybrid",
              "eval_all_standard_speed", "pace_sci400", "pace_sci500", *[f"eval_battle_{k.replace('-', '_')}" for k in ALL_BATTLE_KINDS],
              *[f"knob_{k.replace('-', '_')}" for k in ALL_BATTLE_KINDS], "siege_shaping", "reward_battle", "reward_economy", "play_s", "train_s", "eval_s", "wall_h",
              *[f"stage_{k.replace('-', '_')}" for k in BATTLE_KINDS], "rung_siege", "rung_skirmish", "rung_clash")
# The siege ladder (`--siege-ladder`, `grounds::LADDER`): its rungs below
# today's start stages; rung LADDER_RUNGS + s is today's stage s.
LADDER_RUNGS = 41
# The teacher's capture share per rung: `standard` as the siege's learner on
# the same scenes (`--battle-minimal`, Quick, the standard suite's seeds 0-31
# on the duel map, one game a group, 32 scenes a rung; the ladder of 41 rungs,
# default spawn order). Re-measured on 2 November with the ninth promotion's
# `standard` (`siege-join`'s switches: who besieges, the melee closing in, the
# line of sight; classic/docs/ai-siege-walls.md, «Promoted: `siege-join`»), and
# again with the tenth's (`workers-fit`: the world before the war is played by
# `standard`, `grounds::play_prefix`, so the scenes move; classic/docs/ai-workers.md
# «Promoted»), and again with the eleventh's (`army-camps`: the unit kinds by
# purpose and the camps cleared; classic/docs/ai-army.md «Promoted»).
# Rungs past the table take its last entry. The fast rungs build no non-varied
# scene (their entries are the varied shares); `standard` plays a `Disorder`
# rung as its rung inside (the spawn order does not change its share).
LADDER_TEACHER = (1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.97, 0.91,
                  0.75, 0.97, 0.81, 0.69, 0.59, 1.0, 1.0, 0.34, 0.29, 0.97, 0.97, 0.81, 0.16, 0.59, 0.19, 0.19, 0.19, 0.09, 0.16, 0.25, 0.06, 0.0,
                  0.0, 0.0)
# The same with varied scenes (`--ladder-vary`, `--battle-vary`: the war's
# round, the target, the kinds, the side of approach and the map script drawn
# per group), measured the same way.
LADDER_TEACHER_VARY = (1.0, 1.0, 1.0, 0.97, 0.97, 1.0, 1.0, 1.0, 0.97, 1.0, 0.91, 1.0, 1.0, 1.0, 1.0, 1.0, 0.97, 0.97, 0.97, 0.91, 0.94,
                       0.91, 0.41, 0.88, 0.69, 0.38, 0.41, 1.0, 1.0, 0.19, 0.29, 0.97, 0.91, 0.68, 0.17, 0.53, 0.22, 0.16, 0.09, 0.12, 0.03, 0.09,
                       0.0, 0.0, 0.0, 0.03)
# The same counting only captures in the learner's first turn on the rungs
# whose scene allows one (`--rung-success first-turn`, `same_turn`).
LADDER_TEACHER_VARY_FIRST = (1.0, 0.97, 1.0, 0.94, 0.91, 1.0, 1.0, 1.0, 0.97, 1.0, 0.84, 1.0, 1.0, 1.0, 1.0, 1.0, 0.97, 0.97, 0.97, 0.84, 0.88,
                             0.88, 0.41, 0.88, 0.69, 0.38, 0.41, 1.0, 1.0, 0.19, 0.29, 0.97, 0.91, 0.68, 0.17, 0.53, 0.22, 0.16, 0.09, 0.12, 0.03, 0.09,
                             0.0, 0.0, 0.0, 0.03)
LADDER_TEACHER_FLOOR = 0.05
# The skirmish ladder (`--skirmish-ladder`, `grounds::SKIRMISH`): rungs 0-20,
# a 1-HP enemy unit in reach of the learner's up to an even six a side, every
# scene on the minimal world with varied draws (`--battle-minimal --battle-vary`).
SKIRMISH_RUNGS = 21
# The scripted teacher's win share per rung (`standard` with the scene's
# strikes, `grounds::BattleScene::strike`; every enemy unit dead or routed;
# classic/docs/ai-net-data.md, «The skirmish ladder»; re-measured with the tenth
# and the eleventh promotion's `standard`, classic/docs/ai-workers.md and
# classic/docs/ai-army.md «Promoted»): a reference only. The
# ladder moves by absolute shares by default (`--skirmish-teacher` '' : 1.0 a
# rung, so up at 0.9 and down below 0.4 with the default --rung-advance and
# --rung-retreat); `--skirmish-teacher standard` uses this.
SKIRMISH_STANDARD = (1.0, 1.0, 0.88, 0.97, 0.84, 1.0, 0.97, 1.0, 0.91, 0.78, 0.84, 0.91, 0.84, 0.88, 0.75, 0.75, 0.84, 0.78, 0.78, 0.72, 0.75)


def ladder_curriculum(knob: float, rung: int, win: float, teacher: float, rate: float, advance: float, retreat: float, last: int) -> tuple[float, int, bool]:
    """One iteration of the siege ladder, (knob, rung, the rung moved),
    relative to the teacher's capture share t on the rung (`--rung-teacher`):
    the learner moves one rung up once its averaged win share reaches
    `advance` × t and one rung down once it falls below `retreat` × t; in
    between nothing moves. t is at least `LADDER_TEACHER_FLOOR`, so a rung
    where the teacher takes nothing is not passed without captures. The
    knob stays where it is below the last rung; at the last rung winning
    raises it ×2^(2 × rate × max(win − advance × t, 0.05)), and losing there
    lowers a knob above 1 toward 1 before the rung moves."""
    teacher = max(teacher, LADDER_TEACHER_FLOOR)
    hi, lo = advance * teacher, retreat * teacher
    if win >= hi:
        if rung < last:
            return knob, rung + 1, True
        return min(knob * 2.0 ** (2 * rate * max(win - hi, 0.05)), KNOB_RANGE[1]), rung, False
    if win < lo:
        if knob > 1.0:
            return max(knob * 2.0 ** (2 * rate * (win - lo)), 1.0), rung, False
        if rung > 0:
            return knob, rung - 1, True
    return knob, rung, False


def staged_curriculum(knob: float, stage: int, win: float, rate: float, advance: float, retreat: float, last: int, band: bool = False) -> tuple[float, int, bool]:
    """One iteration of a kind's two knobs, (knob, stage, the stage moved).
    They are one scale, the stage the coarse part: from (stage 0, knob 0.125),
    the attackers at the walls of a worn city, to (stage `last`, knob 8), a
    march from home on a full garrison. The knob stays at 1 (the documented
    setting) while the stage moves and leaves 1 only at the ends: a learner
    winning more than half raises a knob below 1 toward 1, else moves the
    stage back one round of march once its win share reaches `advance`, and
    at the last stage raises the knob; a learner losing lowers a knob above 1
    toward 1, else moves the stage forward once its win share falls to
    `retreat`, and at stage 0 lowers the knob. The knob moves ×2^(2 × rate ×
    (win − 0.5)) as without stages.

    `band` (`--stage-band`): the same thresholds drive the knob too — the
    win share counts as winning from `advance` up and as losing from
    `retreat` down, and between them nothing moves; the knob moves ×2^(2 ×
    rate × (win − m)), m = (advance + retreat) / 2 the band's middle. So a
    knob below 1 at stage 0 climbs to 1 (and then the stage moves back) at
    the win share that moves a stage, not only above a half: with advance
    0.35 a kind winning 40 % gets harder, where without the band its knob
    falls (run5's siege: 32 iterations at stage 0, knob 0.125, 19 %
    captures)."""
    if band:
        if retreat < win < advance:
            return knob, stage, False
        f = 2.0 ** (2 * rate * (win - 0.5 * (advance + retreat)))
        winning, losing = win >= advance, win <= retreat
    else:
        f = 2.0 ** (2 * rate * (win - 0.5))
        winning, losing = win > 0.5, win < 0.5
    if winning:
        if knob < 1.0:
            return min(knob * f, 1.0), stage, False
        if stage < last:
            return (knob, stage + 1, True) if win >= advance else (knob, stage, False)
        return min(knob * f, KNOB_RANGE[1]), stage, False
    if losing:
        if knob > 1.0:
            return max(knob * f, 1.0), stage, False
        if stage > 0:
            return (knob, stage - 1, True) if win <= retreat else (knob, stage, False)
        return max(knob * f, KNOB_RANGE[0]), stage, False
    return knob, stage, False


def ladder_teacher(text: str, vary: bool = False, first: bool = False) -> tuple[float, ...]:
    """`--rung-teacher`: the teacher's capture share per rung (default
    `LADDER_TEACHER`, `LADDER_TEACHER_VARY` with `--ladder-vary`)."""
    if not text.strip():
        return (LADDER_TEACHER_VARY_FIRST if first else LADDER_TEACHER_VARY) if vary else LADDER_TEACHER
    try:
        shares = tuple(float(x) for x in text.split(",") if x.strip())
    except ValueError as e:
        raise SystemExit(f"--rung-teacher: {e}")
    if not shares or any(not 0.0 < x <= 1.0 for x in shares):
        raise SystemExit("--rung-teacher: shares in (0, 1]")
    return shares


# The battle window's ladder (`--clash-ladder`, `grounds::CLASH`): rungs 0-11,
# the enemy at half the learner's count and 40 % health up to even at two
# rounds of approach; won when the window gains value (Φ up, `train_e2.battle_won`).
CLASH_RUNGS = 12
# The scripted teacher's share of windows that gain value per rung (`standard`
# with the scene's strikes; classic/docs/ai-net-data.md, «The battle window»;
# re-measured with the tenth and the eleventh promotion's `standard`,
# classic/docs/ai-workers.md and classic/docs/ai-army.md «Promoted»): a reference; the ladder moves by absolute shares by default.
CLASH_STANDARD = (0.97, 0.91, 0.91, 0.84, 0.84, 0.78, 0.72, 0.78, 0.62, 0.84, 0.66, 0.62)


def clash_teacher(text: str) -> tuple[float, ...]:
    """`--clash-teacher`: the share per rung the battle window ladder's
    thresholds multiply ('' : 1.0 a rung, absolute; `standard`:
    CLASH_STANDARD; else comma-separated shares)."""
    if not text.strip():
        return (1.0,) * CLASH_RUNGS
    if text.strip() == "standard":
        return CLASH_STANDARD
    return ladder_teacher(text)


def skirmish_teacher(text: str) -> tuple[float, ...]:
    """`--skirmish-teacher`: the share per rung the skirmish ladder's
    thresholds multiply ('' : 1.0 a rung, absolute shares; `standard`:
    SKIRMISH_STANDARD; else comma-separated shares)."""
    if not text.strip():
        return (1.0,) * SKIRMISH_RUNGS
    if text.strip() == "standard":
        return SKIRMISH_STANDARD
    return ladder_teacher(text)


def accepts_option(binary: str, probe: list[str], option: list[str]) -> bool:
    """Whether a binary takes `option` (an older one stops at the unknown
    option before doing anything)."""
    try:
        proc = subprocess.run([binary, *probe, *option], capture_output=True, text=True, timeout=120)
    except (OSError, subprocess.TimeoutExpired):
        return False
    return f"unknown option {option[0]}" not in proc.stdout + proc.stderr and proc.returncode == 0


def accepts_speed(binary: str, probe: list[str]) -> bool:
    return accepts_option(binary, probe, ["--speed", "quick"])


# Opponent seats of a game and starts per seed offset (one size) by suite.
SUITE_OPPONENT_SEATS = {"duel": 1}
SUITE_STARTS = {"standard": 20, "duel": 40}  # standard: 5 maps × 4 seeds a size; duel: 5 maps × 8 seeds
# The training grounds (no bench suite of their own): battles play on the standard suite's seeds.
GROUND_SUITES = train_e2.GROUND_SUITES


def parse_mix(spec: str) -> dict:
    """`a:0.6,b:0.2,c` → {a: 0.6, b: 0.2, c: 1} (a name without a share
    weighs 1), normalised to sum 1."""
    out = {}
    for part in spec.split(","):
        part = part.strip()
        if not part:
            continue
        name, _, w = part.partition(":")
        out[name.strip()] = float(w) if w else 1.0
    total = sum(out.values())
    if not out or total <= 0:
        raise SystemExit(f"empty mix {spec!r}")
    return {k: v / total for k, v in out.items() if v > 0}


def deal(n: int, shares: dict) -> dict:
    """n groups dealt by share, largest remainder (ties in the mix's order);
    names that get none are left out."""
    raw = {k: n * w for k, w in shares.items()}
    out = {k: int(math.floor(v)) for k, v in raw.items()}
    for k in sorted(raw, key=lambda k: -(raw[k] - out[k]))[: n - sum(out.values())]:
        out[k] += 1
    return {k: v for k, v in out.items() if v > 0}


@contextlib.contextmanager
def gpu_lock(path, log=None):
    """Holds the machine's GPU lock while the block runs (MPS jobs at once
    slow each other 4–6×). An exclusive flock(2) on `path`, the lock
    `/usr/bin/lockf` (`gpu-run.sh`) takes, so a loop and a wrapped command
    wait for each other; '' or None: no lock. Yields the seconds waited."""
    if not path:
        yield 0.0
        return
    t0 = time.time()
    with open(path, "a") as f:
        fcntl.flock(f.fileno(), fcntl.LOCK_EX)
        waited = time.time() - t0
        if log is not None and waited > 1.0:
            log(f"  waited {waited:.0f}s for the GPU lock {path}")
        try:
            yield waited
        finally:
            fcntl.flock(f.fileno(), fcntl.LOCK_UN)


def eval_key(suite: str, mode: str, speed: str) -> str:
    return f"{suite}-{mode}" + ("" if speed == "standard" else f"-{speed}")


class Loop:
    def __init__(self, a):
        self.a = a
        self.state_dir = Path(a.state)
        self.ckpt_dir = self.state_dir / "ckpt"
        self.games_dir = self.state_dir / "games"
        self.eval_dir = self.state_dir / "evals"
        for d in (self.ckpt_dir, self.games_dir, self.eval_dir):
            d.mkdir(parents=True, exist_ok=True)
        self.log_file = open(self.state_dir / "loop.log", "a")
        self.device = pick_device(a.device)
        self.state = self._load_state()
        self.speed, self.speed_flag = self._speed()
        self.midgame, self.give = self._curriculum()
        self.suites, self.opponents = self._mixes()
        self.battle_kinds = parse_mix(a.battle_kinds)
        self.stage_kinds = {k.strip() for k in getattr(a, "stage_kinds", "").split(",") if k.strip()}
        if self.stage_kinds - set(BATTLE_KINDS):
            raise SystemExit(f"--stage-kinds: unknown {sorted(self.stage_kinds - set(BATTLE_KINDS))} ({', '.join(BATTLE_KINDS)})")
        self.ladder_teacher = ladder_teacher(getattr(a, "rung_teacher", ""), getattr(a, "ladder_vary", False), getattr(a, "rung_success", "any") == "first-turn")
        self.skirmish_teacher = skirmish_teacher(getattr(a, "skirmish_teacher", ""))
        self.clash_teacher = clash_teacher(getattr(a, "clash_teacher", ""))
        self.suites = self._grounds(self.suites)
        self._check_sequential()
        self.run_opponent: dict = {}
        self.last_play: dict = {}
        self.exploiter = self._exploiter()

    # -- bookkeeping ---------------------------------------------------------

    def log(self, msg: str):
        line = f"[{time.strftime('%H:%M:%S')}] {msg}"
        print(line, flush=True)
        self.log_file.write(line + "\n")
        self.log_file.flush()

    def _write_json(self, path: Path, obj):
        tmp = path.with_suffix(path.suffix + ".tmp")
        tmp.write_text(json.dumps(obj, indent=1))
        tmp.replace(path)

    def save_state(self):
        self._write_json(self.state_dir / "state.json", self.state)

    def ckpt(self, i: int, ext: str) -> Path:
        return self.ckpt_dir / f"iter_{i:04d}.{ext}"

    def _load_state(self) -> dict:
        path = self.state_dir / "state.json"
        if path.exists():
            st = json.loads(path.read_text())
            self.log(f"resume at iteration {st['iteration']} ({self.state_dir})")
            return st
        a = self.a
        self.log(f"new run in {self.state_dir} from {a.init}")
        model = train_e2.load_model(a.init, a.aim)
        save_checkpoint(self.state_dir / "ref.pt", train_e2.load_model(a.ref or a.init, a.aim))
        save_checkpoint(self.ckpt(0, "pt"), model, {"step": 0, "iteration": 0})
        train_e2.export_weights(model, self.ckpt(0, "bin"))
        st = {"iteration": 0, "step": 0, "baselines": {}, "adv_scale": {}, "best": {}, "evals": {}, "eval_pending": [], "pending_row": None,
              "wall_seconds": 0.0, "init": str(a.init), "args": vars(a)}
        self.state = st
        self.save_state()
        return st

    def _speed(self) -> tuple[str, bool]:
        """The game speed of play and eval, and whether the binaries take
        `--speed` (with `standard` and an older binary: no flag, its speed).
        `auto`: quick when both binaries take it, else standard."""
        a = self.a
        tmp = self.state_dir / "speed-probe"
        ok = accepts_speed(a.selfplay_bin, ["--net", "dummy", "--games", "0", "--out", str(tmp)]) and accepts_speed(a.tournament_bin, ["--list"])
        shutil.rmtree(tmp, ignore_errors=True)
        speed = a.speed if a.speed != "auto" else ("quick" if ok else "standard")
        if speed != "standard" and not ok:
            raise SystemExit(f"--speed {speed}: {a.selfplay_bin} or {a.tournament_bin} takes no --speed")
        self.log(f"game speed {speed} ({'passed as --speed' if ok else 'the binaries take no --speed: their own, standard'})")
        return speed, ok

    def _mixes(self) -> tuple[dict, dict]:
        """The suite and opponent mixes; an opponent the binary does not know
        (`ai-tournament --list`) is left out, its share to `standard`."""
        a = self.a
        suites = parse_mix(a.suites)
        opponents = parse_mix(a.opponents)
        try:
            listed = subprocess.run([a.tournament_bin, "--list"], capture_output=True, text=True, timeout=120).stdout
            known = {x.strip() for line in listed.splitlines() if ":" in line for x in line.split(":", 1)[1].split(",")}
        except (OSError, subprocess.TimeoutExpired):
            known = set()
        # `book@base` (a book on another base, classic/docs/ai-books.md «Re-based»): both names known.
        missing = [n for n in opponents if n not in ("league", "standard") and not all(x in known for x in n.split("@"))]
        for n in missing:
            opponents["standard"] = opponents.get("standard", 0.0) + opponents.pop(n)
        self.log(f"suites {suites}; opponents {opponents}" + (f" (not in the frozen binaries, their share to standard: {missing})" if missing else ""))
        return suites, opponents

    def _curriculum(self) -> tuple[bool, bool]:
        """Whether the selfplay binary takes the mid-game starts and the
        given spaceship technologies; without them every group starts at
        round 0 (said in the log)."""
        a = self.a
        if not a.from_logs:
            self.log("curriculum: off (--from-logs empty): every group from round 0")
            return False, False
        tmp = self.state_dir / "option-probe"
        probe = ["--net", "dummy", "--games", "0", "--out", str(tmp)]
        mid = Path(a.from_logs).is_dir() and accepts_option(a.selfplay_bin, probe, ["--from-logs", a.from_logs])
        give = mid and a.give_share > 0 and accepts_option(a.selfplay_bin, probe, ["--from-logs", a.from_logs, "--give-space-techs"])
        shutil.rmtree(tmp, ignore_errors=True)
        self.log(f"curriculum: mid-game starts {'on' if mid else 'UNAVAILABLE (the selfplay binary takes no --from-logs, or no logs): every group from round 0'}"
                 + (f" from {a.from_logs} at round {a.from_round} ± {a.from_round_jitter}, learner {a.learner_seat}; "
                    f"--give-space-techs {'on for ' + str(a.give_share) if give else 'unavailable'}; round-0 groups {a.round0_share} every {a.round0_every} iterations" if mid else ""))
        return mid, give

    def economy_logs(self) -> str:
        return self.a.economy_logs or self.a.from_logs

    def _grounds(self, suites: dict) -> dict:
        """The training grounds of the suite mix the selfplay binary can play
        (`--battle`, `--window` with the economy logs); a ground it cannot
        play gives its share to `standard` (said in the log)."""
        a = self.a
        tmp = self.state_dir / "ground-probe"
        probe = ["--net", "dummy", "--games", "0", "--out", str(tmp)]
        ok = {}
        if "battle" in suites:
            unknown = [k for k in self.battle_kinds if k not in ALL_BATTLE_KINDS]
            if unknown:
                raise SystemExit(f"--battle-kinds: unknown {unknown} ({', '.join(ALL_BATTLE_KINDS)})")
            ok["battle"] = accepts_option(a.selfplay_bin, probe, ["--battle", "field"])
            if ok["battle"] and self.stage_kinds and not accepts_option(a.selfplay_bin, probe, ["--battle", "field", "--battle-stage", "0"]):
                raise SystemExit(f"--stage-kinds: {a.selfplay_bin} takes no --battle-stage")
            if ok["battle"] and getattr(a, "siege_ladder", False) and not accepts_option(a.selfplay_bin, probe, ["--battle", "siege", "--battle-rung", "0", "--battle-minimal",
                                                                                                      *(["--battle-vary"] if getattr(a, "ladder_vary", False) else [])]):
                raise SystemExit(f"--siege-ladder: {a.selfplay_bin} takes no --battle-rung --battle-minimal" + (" --battle-vary" if getattr(a, "ladder_vary", False) else ""))
            if ok["battle"] and SKIRMISH in self.battle_kinds and not accepts_option(a.selfplay_bin, probe, ["--battle", SKIRMISH, "--battle-rung", "0", "--battle-minimal",
                                                                                                           "--battle-vary"]):
                raise SystemExit(f"--battle-kinds {SKIRMISH}: {a.selfplay_bin} takes no --battle {SKIRMISH} --battle-rung --battle-minimal --battle-vary")
            if ok["battle"] and CLASH in self.battle_kinds and not accepts_option(a.selfplay_bin, probe, ["--battle", CLASH, "--battle-rung", "0", "--battle-minimal",
                                                                                                        "--battle-vary"]):
                raise SystemExit(f"--battle-kinds {CLASH}: {a.selfplay_bin} takes no --battle {CLASH} --battle-rung --battle-minimal --battle-vary")
            if ok["battle"] and getattr(a, "ladder_pool", "") and not accepts_option(a.selfplay_bin, probe, ["--battle", "siege", "--ladder-pool", a.ladder_pool]):
                raise SystemExit(f"--ladder-pool: {a.selfplay_bin} takes no --ladder-pool {a.ladder_pool}")
            if ok["battle"] and getattr(a, "ladder_solvable", False) and not accepts_option(a.selfplay_bin, probe, ["--battle", "siege", "--ladder-solvable"]):
                raise SystemExit(f"--ladder-solvable: {a.selfplay_bin} takes no --ladder-solvable")
            if ok["battle"] and getattr(a, "decision_values", "") and not accepts_option(a.selfplay_bin, probe, ["--battle", "siege", "--decision-values", a.decision_values]):
                raise SystemExit(f"--decision-values: {a.selfplay_bin} takes no --decision-values {a.decision_values}")
        if "opening" in suites:
            ok["opening"] = accepts_option(a.selfplay_bin, probe, ["--opening", "0"])
        if "economy" in suites:
            logs = self.economy_logs()
            ok["economy"] = bool(logs) and Path(logs).is_dir() and accepts_option(a.selfplay_bin, probe, ["--from-logs", logs, "--window", a.economy_window])
        shutil.rmtree(tmp, ignore_errors=True)
        out = dict(suites)
        for g, fine in ok.items():
            if not fine:
                out["standard"] = out.get("standard", 0.0) + out.pop(g)
        if ok:
            self.log(f"grounds: {', '.join(f'{g} ' + ('on' if v else 'UNAVAILABLE (its share to standard)') for g, v in ok.items())}"
                     + (f"; battle kinds {self.battle_kinds}, {a.battle_rounds} rounds, armies {a.battle_army}" if ok.get("battle") else "")
                     + (f"; start stages for {sorted(self.stage_kinds)} from {a.stage_start} to {a.stage_last}, back at a win share ≥ {a.stage_advance}, "
                        f"forward at ≤ {a.stage_retreat}" if ok.get("battle") and self.stage_kinds else "")
                     + (f"; the siege ladder from rung {a.rung_start} to {self.ladder_last()}, up at {a.rung_advance} × the teacher's capture share, "
                        f"down below {a.rung_retreat} × it" if ok.get("battle") and getattr(a, "siege_ladder", False) else "")
                     + (f"; the skirmish ladder from rung {getattr(a, 'skirmish_rung_start', 0)} to {self.ladder_last(SKIRMISH)}, up at {a.rung_advance} × "
                        f"{self.skirmish_teacher}, down below {a.rung_retreat} × it" if ok.get("battle") and self.rung(SKIRMISH) is not None else "")
                     + (f"; economy from {self.economy_logs()} at rounds {a.economy_rounds}, windows {a.economy_window} (opening "
                        f"{a.economy_opening_window or 'the logs speed of 90 Standard rounds'}), reward {a.economy_reward}"
                        f"{f' + {a.economy_gamma}^W V(final)' if a.economy_bootstrap else ''}" if ok.get("economy") else "")
                     + (f"; openings on the {a.opening_suite} suite's maps, {a.opening_window or 'the speed of 135 Standard rounds'} rounds, the learner's units only, "
                        f"Φ weights {train_e2.opening_weights(a)}" if ok.get("opening") else ""))
        return out

    def opponent_mix(self) -> dict:
        """The opponents' shares of this iteration: `--opponents`, with
        `--opponents-pfsp` reweighted by the learner's recent win share
        against each (`train_e2.pfsp_weights`)."""
        if not getattr(self.a, "opponents_pfsp", False):
            return self.main_opponents()
        return train_e2.pfsp_weights(self.main_opponents(), self.state.get("pfsp", {}), self.a.pfsp_power, self.a.pfsp_floor)

    # -- the main exploiter (e2_exploiter.py; off without --exploiter-groups) --

    def _exploiter(self):
        if getattr(self.a, "exploiter_groups", 0) <= 0:
            return None
        from . import e2_exploiter

        return e2_exploiter.MainExploiter(self)

    def main_opponents(self) -> dict:
        """`--opponents` with the promoted exploiters of the state (`--exploiter-league-share`)."""
        from . import e2_exploiter

        league = e2_exploiter.promoted_league(getattr(self, "state", {}))
        return e2_exploiter.league_mix(self.opponents, league, getattr(self.a, "exploiter_league_share", 0.0)) if league else dict(self.opponents)

    def opponent_seat(self, opp: str, league) -> str:
        """The `--opponents` name of opponent `opp` (a past checkpoint or a promoted exploiter: `net:<file>`)."""
        if opp == "league":
            return f"net:{league}"
        from . import e2_exploiter

        return next((f"net:{p['bin']}" for p in e2_exploiter.promoted_league(getattr(self, "state", {})) if p["name"] == opp), opp)

    def group_size(self, suite: str) -> int:
        """A group's games: `--ground-group-size` for a training ground, else `--group-size`."""
        a = self.a
        return getattr(a, "ground_group_size", 8) if suite in GROUND_SUITES else getattr(a, "group_size", 4)

    def knob(self, kind: str) -> float:
        """The kind's difficulty knob now (1: the eval's setting)."""
        return float(getattr(self, "state", {}).get("curriculum", {}).get("knobs", {}).get(kind, 1.0))

    def stage(self, kind: str) -> int | None:
        """The kind's start stage now (None: not of `--stage-kinds`, the documented placement); the
        battle window's approach stage off its ladder (`--clash-stage-start`)."""
        if kind == CLASH:
            return int(getattr(self.a, "clash_stage_start", 0))
        if kind not in getattr(self, "stage_kinds", ()):
            return None
        return int(getattr(self, "state", {}).get("curriculum", {}).get("stages", {}).get(kind, self.a.stage_start))

    def rung(self, kind: str) -> int | None:
        """The kind's ladder rung now (None: no `--siege-ladder` / `--skirmish-ladder` / `--clash-ladder`, or another kind)."""
        if kind == SKIRMISH and getattr(self.a, "skirmish_ladder", False):
            return int(getattr(self, "state", {}).get("curriculum", {}).get("rungs", {}).get(kind, getattr(self.a, "skirmish_rung_start", 0)))
        if kind == CLASH and getattr(self.a, "clash_ladder", False):
            return int(getattr(self, "state", {}).get("curriculum", {}).get("rungs", {}).get(kind, getattr(self.a, "clash_rung_start", 0)))
        if kind != "siege" or not getattr(self.a, "siege_ladder", False):
            return None
        return int(getattr(self, "state", {}).get("curriculum", {}).get("rungs", {}).get(kind, self.a.rung_start))

    def ladder_last(self, kind: str = "siege") -> int:
        """The ladder's last rung: the siege's, today's last start stage; the skirmish's and the battle window's, their tables' last."""
        if kind == SKIRMISH:
            return SKIRMISH_RUNGS - 1
        if kind == CLASH:
            return CLASH_RUNGS - 1
        return LADDER_RUNGS + self.a.stage_last

    def update_curriculum(self, summary: dict) -> dict:
        """The battle curriculum after an iteration: per kind the learner's
        win share (an exponential average over iterations, `--curriculum-decay`)
        moves the knob ×2^(2 × rate × (win − 0.5)), within KNOB_RANGE: harder
        while the net wins more than half, easier while it loses; a kind of
        `--stage-kinds` moves its start stage first (`staged_curriculum`; its
        win average starts again at a new stage). The siege shaping weight
        goes from 0.45 (no capture seen) down to 0.2 (a capture share of a
        half and more)."""
        a = self.a
        cur = self.state.setdefault("curriculum", {"knobs": {}, "win": {}, "captures": None, "siege_shaping": 0.45})
        stages = cur.setdefault("stages", {})
        self.played_stages = {k: self.stage(k) for k in BATTLE_KINDS if self.stage(k) is not None and self.rung(k) is None}
        self.played_rungs = {k: self.rung(k) for k in ALL_BATTLE_KINDS if self.rung(k) is not None}
        rungs = cur.setdefault("rungs", {}) if self.played_rungs else cur.get("rungs", {})
        for kind, o in (summary.get("battle_outcomes") or {}).items():
            old = cur["win"].get(kind)
            # The siege ladder's same-turn rule: a win is a capture in the learner's first turn where the scene allows one.
            win = o.get("first_turn_share", o["win_share"]) if kind == "siege" and getattr(a, "rung_success", "any") == "first-turn" and self.rung(kind) is not None else o["win_share"]
            cur["win"][kind] = win if old is None else a.curriculum_decay * old + (1 - a.curriculum_decay) * win
            stage, rung = (None if kind == CLASH else self.stage(kind)), self.rung(kind)
            if rung is not None:
                # A ladder: up and down by the teacher's share on the rung (the skirmish's: absolute by default).
                table = {SKIRMISH: getattr(self, "skirmish_teacher", None) or skirmish_teacher(""), CLASH: getattr(self, "clash_teacher", None) or clash_teacher("")}.get(
                    kind, getattr(self, "ladder_teacher", LADDER_TEACHER))
                teacher = table[min(rung, len(table) - 1)]
                knob, rungs[kind], moved = ladder_curriculum(self.knob(kind), rung, cur["win"][kind], teacher, a.curriculum_rate, a.rung_advance, a.rung_retreat,
                                                             self.ladder_last(kind))
                cur["knobs"][kind] = min(max(knob, KNOB_RANGE[0]), KNOB_RANGE[1])
                if moved:
                    cur["win"].pop(kind)
            elif stage is None:
                knob = self.knob(kind) * 2.0 ** (2 * a.curriculum_rate * (cur["win"][kind] - 0.5))
                cur["knobs"][kind] = min(max(knob, KNOB_RANGE[0]), KNOB_RANGE[1])
            else:
                knob, stages[kind], moved = staged_curriculum(self.knob(kind), stage, cur["win"][kind], a.curriculum_rate, a.stage_advance, a.stage_retreat, a.stage_last,
                                                               getattr(a, "stage_band", False))
                cur["knobs"][kind] = min(max(knob, KNOB_RANGE[0]), KNOB_RANGE[1])
                if moved:
                    cur["win"].pop(kind)
            if kind == "siege":
                share = o["captures"] / max(o["games"], 1)
                cur["captures"] = share if cur["captures"] is None else a.curriculum_decay * cur["captures"] + (1 - a.curriculum_decay) * share
                cur["siege_shaping"] = 0.45 - 0.25 * min(cur["captures"] / 0.5, 1.0)
        self.log(f"curriculum: win share {({k: round(v, 3) for k, v in cur['win'].items()})}, next knobs {({k: round(v, 3) for k, v in cur['knobs'].items()})}, "
                 + (f"next stages {dict(stages)}, " if stages else "") + (f"next rungs {dict(rungs)}, " if rungs else "") + f"siege shaping {cur['siege_shaping']:.3f}")
        return {k: (dict(v) if isinstance(v, dict) else v) for k, v in cur.items()}

    def speed_args(self, speed: str | None = None) -> list[str]:
        return ["--speed", speed or self.speed] if self.speed_flag else []

    def rounds(self) -> list[str]:
        """`--rounds`: given, or 450 at standard speed; at another speed the
        binary's own last round for that speed (450 is a standard-speed number)."""
        r = self.a.rounds or (450 if self.speed == "standard" else 0)
        return ["--rounds", str(r)] if r else []

    # -- phases --------------------------------------------------------------

    def past_checkpoints(self, current: int) -> list[Path]:
        return [p for p in sorted(self.ckpt_dir.glob("iter_*.bin")) if int(p.stem[5:]) < current]

    def plan(self, i: int) -> list[dict]:
        """The selfplay runs of iteration i. The groups are dealt to the
        suites of `--suites` by share; the standard suite's groups to
        round-0 starts (`--round0-share` every `--round0-every` iterations,
        all of them without mid-game support), mid-game starts from
        `--from-logs`, and mid-game starts with `--give-space-techs`; each
        pool to the opponents of `--opponents` by share (`league`: one past
        checkpoint; without one yet its share goes to `standard`). A game of
        opponent X has X in one seat and `standard` in the others."""
        a = self.a
        past = self.past_checkpoints(i - 1)
        rng = np.random.default_rng([a.seed, i, 1])
        league = past[int(rng.integers(len(past)))] if past else None
        opps = {n: w for n, w in self.opponent_mix().items() if n != "league" or league is not None}
        if not opps:
            opps = {"standard": 1.0}
        total = sum(opps.values())
        opps = {k: w / total for k, w in opps.items()}  # a league share without a past checkpoint goes to the others
        runs = []
        for suite, g_suite in deal(a.groups, self.suites).items():
            if suite == "battle":
                # Battles: the learner's side against the scripted tactics (`standard`), one run per kind.
                for kind, n in deal(g_suite, self.battle_kinds).items():
                    extra = ["--battle", kind, "--battle-rounds", a.battle_rounds, "--battle-army", a.battle_army,
                             "--battle-difficulty", f"{self.knob(kind):.4f}"]
                    if kind == CLASH:
                        # The battle window: its own armies and window, its approach stage, varied scenes in the minimal world.
                        start = ["--battle-rung", str(self.rung(kind))] if self.rung(kind) is not None else ["--battle-stage", str(self.stage(kind))]
                        extra = ["--battle", kind, "--battle-rounds", getattr(a, "clash_rounds", "4-5"), "--battle-army", getattr(a, "clash_army", "8-14"),
                                 "--battle-difficulty", f"{self.knob(kind):.4f}", *start, "--battle-minimal", "--battle-vary"]
                    elif kind == SKIRMISH:
                        # Skirmishes: varied scenes in the minimal world, at the ladder's rung (without --skirmish-ladder the binary's, six a side).
                        extra += [*(["--battle-rung", str(self.rung(kind))] if self.rung(kind) is not None else []), "--battle-minimal", "--battle-vary"]
                    elif self.rung(kind) is not None:
                        # The ladder's scenes in their minimal world (the teacher's table was measured there).
                        extra += ["--battle-rung", str(self.rung(kind)), "--battle-minimal", *(["--battle-vary"] if getattr(a, "ladder_vary", False) else []),
                                  *(["--ladder-solvable"] if getattr(a, "ladder_solvable", False) else []),
                                  *(["--ladder-pool", a.ladder_pool] if getattr(a, "ladder_pool", "") else [])]
                    elif self.stage(kind) is not None:
                        extra += ["--battle-stage", str(self.stage(kind))]
                    if getattr(a, "decision_values", ""):
                        # The net's unit decisions valued by the search teacher's rollouts (train_e2 --decision-advantage).
                        extra += ["--decision-values", a.decision_values]
                    runs.append({"name": f"battle-{kind}", "suite": "battle", "pool": "battle", "groups": n, "group_size": self.group_size("battle"), "opponents": ["standard"],
                                 "extra": extra, "opponent": None})
                continue
            if suite == "opening":
                # Openings: the learner's units against `standard` on every other seat (its other heads `standard` too).
                runs.append({"name": "opening", "suite": "opening", "pool": "opening", "groups": g_suite, "group_size": self.group_size("opening"),
                             "opponents": ["standard"], "extra": ["--opening", str(getattr(a, "opening_window", 0))], "opponent": None})
                continue
            pools = [("zero", g_suite, [])]
            if suite == "economy":
                opening = getattr(a, "economy_opening_window", 0)
                pools = [("economy", g_suite, ["--from-rounds", a.economy_rounds, "--window", a.economy_window, *(["--opening-window", str(opening)] if opening else [])])]
            if suite == "standard" and self.midgame:
                g0 = int(round(g_suite * a.round0_share)) if a.round0_every and i % a.round0_every == 0 else 0
                g_mid = g_suite - g0
                g_give = int(round(g_mid * a.give_share)) if self.give else 0
                pools = [("zero", g0, []), ("mid", g_mid - g_give, []), ("give", g_give, ["--give-space-techs"])]
            seats = SUITE_OPPONENT_SEATS.get(suite, a.opponent_seats)
            for pool, groups, extra in pools:
                for opp, n in deal(groups, opps).items():
                    name = self.opponent_seat(opp, league)
                    opponents = ["standard"] if opp == "standard" else ["standard"] * max(seats - 1, 0) + [name]
                    runs.append({"name": f"{suite}-{pool}-{opp}", "suite": suite, "pool": pool, "groups": n, "group_size": self.group_size(suite), "opponents": opponents,
                                 "extra": extra, "opponent": opp})
        return runs

    def play(self, i: int) -> list[str]:
        a = self.a
        behaviour = self.ckpt(i - 1, "bin")
        out = self.games_dir / f"iter_{i:04d}"
        offsets = {}  # per suite, the suite seed offset of the next round-0 group
        offset_mid = (i - 1) * a.groups  # mid-game starts: the source log of group g is (offset + g) mod their count
        offset_eco = (i - 1) * a.groups + 7  # economy windows: their own sequence of source logs
        env = {k: v for k, v in os.environ.items() if k not in NET_ENV}
        plan = self.plan(i)
        self.run_opponent = {r["name"]: r["opponent"] for r in plan}
        self.last_play = {}
        for j, run in enumerate(plan):
            d = out / run["name"]
            d.mkdir(parents=True, exist_ok=True)
            n, suite, k = run["groups"], run["suite"], run.get("group_size", a.group_size)
            # The grounds have no bench suite: battles play the standard suite's seeds, economy windows ignore it,
            # openings play --opening-suite's (the duel's by default: the smallest map, two nations).
            bench = getattr(a, "opening_suite", "duel") if suite == "opening" else ("standard" if suite in GROUND_SUITES else suite)
            sizes = a.sizes if suite in ("standard", "battle", "economy") or (suite == "opening" and bench == "standard") else ""
            starts = SUITE_STARTS.get("standard" if suite == "battle" else bench if suite == "opening" else suite, a.starts_per_offset) * (len(sizes.split(",")) if sizes else 1)
            t_run = time.time()
            cmd = [a.selfplay_bin, "--net", str(behaviour), "--suite", bench, *(["--sizes", sizes] if sizes else []),
                   "--games", str(n * k), "--group", str(k), "--temperature", str(a.temperature),
                   "--sample-seed", str(i * 100 + j), "--opponents", ",".join(run["opponents"]), "--jobs", str(a.jobs)]
            if run["pool"] == "battle":
                # Battles play the standard suite's seeds (the map scripts per group, --battle-scripts' default).
                off = offsets.setdefault(suite, a.seed_offset + 5000 + (i - 1) * (math.ceil(a.groups / starts) + 1))
                cmd += [*run["extra"], *self.speed_args(), "--seed-offset", str(off)]
                offsets[suite] = off + math.ceil(n / starts)
            elif run["pool"] == "opening":
                # Openings play --opening-suite's seeds, a sequence of their own.
                off = offsets.setdefault(suite, a.seed_offset + 7000 + (i - 1) * (math.ceil(a.groups / starts) + 1))
                cmd += [*run["extra"], *self.speed_args(), "--seed-offset", str(off)]
                offsets[suite] = off + math.ceil(n / starts)
            elif run["pool"] == "economy":
                # Economy windows: a mid-game start at a round of --economy-rounds, W rounds, then stop.
                cmd += ["--from-logs", self.economy_logs(), "--learner-seat", a.learner_seat, "--seed-offset", str(offset_eco), *run["extra"]]
                offset_eco += n
            elif run["pool"] == "zero":
                off = offsets.setdefault(suite, a.seed_offset + (i - 1) * (math.ceil(a.groups / starts) + len(self.opponents) * 3 + 6))
                cmd += [*self.rounds(), *self.speed_args(), "--seed-offset", str(off)]
                offsets[suite] = off + math.ceil(n / starts)
            else:
                # A mid-game start keeps its source log's speed; the last round is the binary's (the speed's) unless --mid-rounds.
                cmd += ["--from-logs", a.from_logs, "--from-round", str(a.from_round), "--from-round-jitter", str(a.from_round_jitter),
                        "--learner-seat", a.learner_seat, "--seed-offset", str(offset_mid), *run["extra"]]
                cmd += ["--rounds", str(a.mid_rounds)] if a.mid_rounds else []
                offset_mid += n
            self.log(f"play {run['name']}: {n}×{k} games, opponents {','.join(run['opponents'])}")
            with open(d / "selfplay.log", "a") as f:
                proc = subprocess.run(cmd + ["--out", str(d)], stdout=f, stderr=subprocess.STDOUT, env=env)
            if proc.returncode != 0:
                self.log(f"  ai-selfplay exited {proc.returncode} (see {d / 'selfplay.log'}); training on the games it finished")
            done = len(list(d.glob("*.e2.json")))
            rec = self.last_play.setdefault(suite, {"seconds": 0.0, "games": 0})
            rec["seconds"] += time.time() - t_run
            rec["games"] += done
        for suite, rec in self.last_play.items():
            rec["games_per_hour"] = rec["games"] * 3600.0 / max(rec["seconds"], 1e-9)
            self.log(f"  play {suite}: {rec['games']} games in {rec['seconds']:.0f}s ({rec['games_per_hour']:.0f} games/hour)")
        logs = sorted(str(p) for p in out.glob("*/*.aurlog") if p.with_name(p.name[: -len('.aurlog')] + ".e2.json").exists())
        return logs

    def load_training(self):
        """The model, its optimiser and the frozen reference at the
        committed iteration."""
        a = self.a
        path = self.ckpt(self.state["iteration"], "pt")
        model = train_e2.load_model(path, a.aim).to(self.device)
        opt = train_e2.make_optimizer(model, a.lr, a.weight_decay)
        ck = torch.load(path, map_location="cpu", weights_only=False)
        if ck.get("optim"):
            opt.load_state_dict(ck["optim"])
        ref = train_e2.load_model(self.state_dir / "ref.pt", a.aim).to(self.device).eval()
        for p in ref.parameters():
            p.requires_grad_(False)
        return model, opt, ref

    def evaluate(self, i: int, mode: str, suite: str, speed: str) -> dict:
        a = self.a
        out = self.eval_dir / f"iter_{i:04d}-{eval_key(suite, mode, speed)}.json"
        if not out.exists():
            args = ["--suite", suite, *self.speed_args(speed)]
            if suite == "pace" and a.pace_repeat > 1:
                args += ["--repeat", str(a.pace_repeat)]
            if a.eval_sprt and suite != "pace":
                args += self.sequential_args(mode, suite, speed)
            code = self.tournament(self.ckpt(i, "bin"), mode, args, out, f"eval iteration {i} {suite} {mode} {speed}")
            if code is not None:
                return {"error": code}
        return summarize_report(json.loads(out.read_text()))

    def _check_sequential(self):
        """Sequential evals (`--eval-sprt`) need a tournament binary that takes `--sprt`."""
        a = self.a
        if not a.eval_sprt:
            return
        if not accepts_option(a.tournament_bin, ["--list"], ["--sprt", a.eval_sprt, "--sprt-error", a.eval_sprt_error]):
            raise SystemExit(f"--eval-sprt: {a.tournament_bin} takes no --sprt (build ai-tournament from bench/ai-tournament)")
        self.log(f"sequential evals of {a.eval_suites}: SPRT {a.eval_sprt} (α, β {a.eval_sprt_error}), at most {a.eval_max_games} games"
                 + (f", at least {a.eval_min_games}" if a.eval_min_games else "")
                 + ("; the blocks' values are the share minus the start net's on the same games" if a.eval_reference == "start" else ""))

    def tournament(self, weights: Path, mode: str, args: list[str], out: Path, what: str) -> int | None:
        """`ai-tournament` with the net of `weights` in `mode` against
        `standard`, its report saved at `out`; None, or the exit code of a
        failed run."""
        a = self.a
        env = {k: v for k, v in os.environ.items() if k not in NET_ENV}
        env.update({"AURELIA_NET_WEIGHTS": str(weights), **MODES[mode]})
        cmd = [a.tournament_bin, "--candidate", "net", "--baseline", "standard", "--jobs", str(a.eval_jobs), "--save", str(out) + ".part", *args]
        self.log(f"{what}: {' '.join(cmd[1:])}")
        with open(out.with_suffix(".log"), "w") as f:
            proc = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT, env=env)
        if proc.returncode != 0 or not Path(str(out) + ".part").exists():
            self.log(f"  ai-tournament exited {proc.returncode} (see {out.with_suffix('.log')})")
            return proc.returncode or 1
        Path(str(out) + ".part").replace(out)
        return None

    def sequential_args(self, mode: str, suite: str, speed: str) -> list[str]:
        """The sequential eval's options (`--eval-sprt`): seed blocks until
        the SPRT decides, at most `--eval-max-games`; with `--eval-reference
        start` the blocks' values are the share minus the start net's
        (iter_0000) on the same games, its reports played once per suite,
        mode and speed (plain runs at seed offsets 0, 1, … covering the cap)."""
        a = self.a
        args = ["--sprt", a.eval_sprt, "--sprt-error", a.eval_sprt_error, "--max-games", str(a.eval_max_games)]
        if a.eval_min_games:
            args += ["--min-games", str(a.eval_min_games)]
        if a.eval_reference == "start":
            for path in self.start_reference(mode, suite, speed):
                args += ["--reference", str(path)]
        return args

    def start_reference(self, mode: str, suite: str, speed: str) -> list[Path]:
        paths, games, offset = [], 0, 0
        while games < self.a.eval_max_games:
            path = self.eval_dir / f"start-{eval_key(suite, mode, speed)}-offset{offset}.json"
            if not path.exists():
                code = self.tournament(self.ckpt(0, "bin"), mode, ["--suite", suite, "--seed-offset", str(offset), *self.speed_args(speed)], path,
                                       f"start reference {suite} {mode} {speed} offset {offset}")
                if code is not None:
                    raise SystemExit(f"the start net's reference failed: see {path.with_suffix('.log')}")
            paths.append(path)
            rep = json.loads(path.read_text())
            games += int(rep.get("games") or (rep.get("versus") or {}).get("games") or 1)
            offset += 1
        return paths

    def evaluate_battle(self, i: int, speed: str) -> dict:
        """The battle eval: per kind, `--battle-eval-games` battles (one game
        a group, argmax) of the weights of iteration i against the scripted
        tactics, on fixed seeds; the learner's win share (`train_e2.battle_won`)
        and mean reward."""
        a = self.a
        out = self.eval_dir / f"iter_{i:04d}-battle.json"
        if out.exists():
            return json.loads(out.read_text())
        env = {k: v for k, v in os.environ.items() if k not in NET_ENV}
        res = {"kinds": {}}
        # The four kinds always; the skirmish when it is in the mix, at a fixed rung (`--skirmish-eval-rung`).
        kinds = [*BATTLE_KINDS, *([k for k in (SKIRMISH, CLASH) if k in self.battle_kinds])]
        for kind in kinds:
            d = self.eval_dir / f"iter_{i:04d}-battle" / kind
            extra = ["--battle-rung", str(getattr(a, "skirmish_eval_rung", 3)), "--battle-minimal", "--battle-vary"] if kind == SKIRMISH else []
            # The battle window's eval: in contact (stage 0), its armies and window.
            extra += ["--battle-stage", "0", "--battle-minimal", "--battle-vary"] if kind == CLASH else []
            rounds, army = (getattr(a, "clash_rounds", "4-5"), getattr(a, "clash_army", "8-14")) if kind == CLASH else (a.battle_rounds, a.battle_army)
            cmd = [a.selfplay_bin, "--net", str(self.ckpt(i, "bin")), "--suite", "standard", "--sizes", a.sizes, "--battle", kind, "--battle-rounds", rounds,
                   "--battle-army", army, "--battle-difficulty", "1", *extra, "--games", str(a.battle_eval_games), "--group", "1", "--temperature", "0",
                   "--seed-offset", str(a.battle_eval_offset), "--opponents", "standard", "--jobs", str(a.eval_jobs), *self.speed_args(speed), "--out", str(d)]
            d.mkdir(parents=True, exist_ok=True)
            with open(d / "selfplay.log", "a") as f:
                subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT, env=env)
            games = [json.loads(p.read_text()) for p in sorted(d.glob("*.e2.json"))]
            games = [g for g in games if train_e2.ground_of(g)]
            res["kinds"][kind] = {"games": len(games), "win_share": float(np.mean([train_e2.battle_won(g) for g in games])) if games else None,  # clash: Φ gained
                                  "mean_reward": float(np.mean([train_e2.battle_reward(g) for g in games])) if games else None}
        shutil.rmtree(self.eval_dir / f"iter_{i:04d}-battle", ignore_errors=True)
        self._write_json(out, res)
        return res

    def run_evals(self):
        st = self.state
        while st["eval_pending"]:
            i, mode, suite, speed = (st["eval_pending"][0] + ["standard"])[:4]
            key = eval_key(suite, mode, speed)
            t0 = time.time()
            if suite == "battle":
                res = self.evaluate_battle(i, speed)
                res["seconds"] = time.time() - t0
                st["evals"].setdefault(str(i), {})["battle"] = res
                self.log("  battle eval: " + ", ".join(f"{k} {v['win_share'] if v['win_share'] is None else round(v['win_share'], 3)} ({v['games']})" for k, v in res["kinds"].items())
                         + f", {res['seconds']:.0f}s")
                if st["pending_row"] is not None:
                    st["pending_row"]["eval_s"] = st["pending_row"].get("eval_s", 0.0) + res["seconds"]
                st["eval_pending"].pop(0)
                self.save_state()
                continue
            res = self.evaluate(i, mode, suite, speed)
            res["seconds"] = time.time() - t0
            res["speed"] = speed
            st["evals"].setdefault(str(i), {})[key] = res
            if "share" in res:
                self.log(f"  {key}: share {res['share']:.3f} [{res['share_low']:.3f}, {res['share_high']:.3f}], wins {res['wins']}/{res['games']}, "
                         f"science {res['science_wins']} (≤400: {res['science_by_400']}, ≤500: {res['science_by_500']}), mean science round {res['science_round']}, "
                         f"{res['seconds']:.0f}s" + sequential_line(res.get("sequential")))
                best = st["best"].get(key)
                if suite != "pace" and speed == self.speed and (best is None or res["share"] > best["share"]):
                    stem = f"best-{mode}" if suite == "standard" else f"best-{suite}-{mode}"
                    for ext in ("pt", "bin"):
                        shutil.copyfile(self.ckpt(i, ext), self.state_dir / f"{stem}.{ext}")
                    st["best"][key] = {"iteration": i, "share": res["share"], "low": res["share_low"], "high": res["share_high"]}
                    self.log(f"  best {mode}: iteration {i}")
            if st["pending_row"] is not None:
                st["pending_row"]["eval_s"] = st["pending_row"].get("eval_s", 0.0) + res["seconds"]
            st["eval_pending"].pop(0)
            self.save_state()

    def write_row(self):
        st = self.state
        row = st["pending_row"]
        if row is None:
            return
        ev = st["evals"].get(str(row["iteration"]), {})
        get = lambda k, f: ev.get(k, {}).get(f, "")
        pace = eval_key("pace", self.a.pace_modes.split(",")[0], self.speed)
        row.update({"eval_all": get(eval_key("standard", "all", self.speed), "share"), "eval_hybrid": get(eval_key("standard", "hybrid", self.speed), "share"),
                    "eval_duel_all": get(eval_key("duel", "all", self.speed), "share"), "eval_duel_hybrid": get(eval_key("duel", "hybrid", self.speed), "share"),
                    "eval_all_standard_speed": get("standard-all", "share") if self.speed != "standard" else "",
                    "pace_sci400": get(pace, "science_by_400"), "pace_sci500": get(pace, "science_by_500"),
                    **{f"eval_battle_{k.replace('-', '_')}": _blank_none(ev.get("battle", {}).get("kinds", {}).get(k, {}).get("win_share")) for k in ALL_BATTLE_KINDS},
                    "wall_h": round(st["wall_seconds"] / 3600, 3)})
        path = self.state_dir / "progress.csv"
        new = not path.exists()
        # An existing file keeps its header (a run begun before a column was added).
        fields = CSV_FIELDS if new else next(csv.reader(open(path, newline="")), None) or CSV_FIELDS
        with open(path, "a", newline="") as f:
            w = csv.DictWriter(f, fieldnames=fields, extrasaction="ignore")
            if new:
                w.writeheader()
            w.writerow({k: (round(v, 5) if isinstance(v, float) else v) for k, v in row.items()})
        st["pending_row"] = None
        self.save_state()

    def prune(self):
        cur = self.state["iteration"]
        keep = {0, cur} | {i for i in range(cur - self.a.keep + 1, cur + 1)}
        keep |= {int(i) for i, *_ in self.state["eval_pending"]}
        for p in self.ckpt_dir.glob("iter_*.*"):
            if p.suffix in (".pt", ".bin") and int(p.stem[5:]) not in keep:
                p.unlink()

    def update_pfsp(self, summary: dict) -> dict:
        """The learner's win share against each opponent (its mean points
        share in the whole games of the runs against it, standard and duel),
        an exponential average over iterations (`--pfsp-decay`); returns it
        and the next iteration's weights."""
        a = self.a
        st = self.state.setdefault("pfsp", {})
        seen = {}
        for run, v in (summary.get("by_run") or {}).items():
            opp = self.run_opponent.get(run)
            if opp is None or run.split("-", 1)[0] in GROUND_SUITES:
                continue
            g, sh = seen.get(opp, (0, 0.0))
            seen[opp] = (g + v["games"], sh + v["points_share"] * v["games"])
        for opp, (g, total) in seen.items():
            now = total / max(g, 1)
            st[opp] = now if opp not in st else a.pfsp_decay * st[opp] + (1 - a.pfsp_decay) * now
        weights = train_e2.pfsp_weights(self.main_opponents(), st, a.pfsp_power, a.pfsp_floor)
        self.log(f"pfsp: win share {({k: round(v, 3) for k, v in st.items()})}, next weights {({k: round(v, 3) for k, v in weights.items()})}")
        return {"win_share": dict(st), "weights": weights}

    # -- the loop -------------------------------------------------------------

    def run(self):
        a = self.a
        st = self.state
        self.run_evals()
        self.write_row()
        model = opt = ref = None
        t_run = time.time()
        while st["iteration"] < a.iterations:
            if a.hours and time.time() - t_run > a.hours * 3600:
                self.log(f"time limit {a.hours} h reached")
                break
            i = st["iteration"] + 1
            t0 = time.time()
            logs = self.play(i)
            t_play = time.time() - t0
            self.log(f"iteration {i}: {len(logs)} games played in {t_play:.0f}s")
            if not logs:
                raise SystemExit("no games were played")
            ex_logs = self.exploiter.play(i) if self.exploiter else None
            if model is None:
                model, opt, ref = self.load_training()
            t1 = time.time()
            baselines = {k: train_e2.RunningBaseline(**v) for k, v in st["baselines"].items()}
            scales = {k: train_e2.RunningScale(**v) for k, v in st.get("adv_scale", {}).items()}
            a.siege_shaping = st.get("curriculum", {}).get("siege_shaping", 0.45)
            # Only the training holds the GPU: play and evals run on the CPU.
            with gpu_lock(a.gpu_lock if self.device.type != "cpu" else "", self.log) as gpu_wait:
                summary = train_e2.train_games(model, ref, opt, logs, a, self.device, np.random.default_rng([a.seed, i, 2]), baselines, self.log, st["step"], scales)
            t_train = time.time() - t1 - gpu_wait
            t2 = time.time()
            save_checkpoint(self.ckpt(i, "pt"), model, {"optim": opt.state_dict(), "step": st["step"] + summary["steps"], "iteration": i})
            train_e2.export_weights(model, self.ckpt(i, "bin"))
            t_export = time.time() - t2
            compact = {k: v for k, v in summary.items() if k != "updates"}
            compact.update({"iteration": i, "play_s": t_play, "train_s": t_train, "gpu_wait_s": gpu_wait, "export_s": t_export, "play_by_suite": self.last_play})
            if a.opponents_pfsp:
                compact["pfsp"] = self.update_pfsp(summary)
            if self.suites.get("battle"):
                compact["curriculum"] = self.update_curriculum(summary)
            compact["siege_shaping"] = a.siege_shaping
            if self.exploiter:
                # Its training after the main one (one at a time under the GPU lock), committed with the iteration below.
                compact["exploiter"] = self.exploiter.train(i, ex_logs)
            with open(self.state_dir / "iterations.jsonl", "a") as f:
                f.write(json.dumps(compact | {"updates": summary["updates"]}) + "\n")
            st["step"] += summary["steps"]
            st["baselines"] = {k: b.state() for k, b in baselines.items()}
            st["adv_scale"] = {k: sc.state() for k, sc in scales.items()}
            st["iteration"] = i
            st["wall_seconds"] += time.time() - t0
            st["pending_row"] = {"iteration": i, "games": summary["games"], "mean_reward": summary.get("mean_reward"), "learner_wins": summary.get("learner_wins"),
                                 "science_wins": summary.get("learner_science_wins"), "kl": summary["kl"], "entropy": summary["entropy"],
                                 "zero_signal": summary.get("games_dropped", 0) / max(summary["games"], 1),
                                 "first_ratio": summary["first_update"].get("ratio_mean"), "masked": summary["masked_frac"],
                                 **{f"knob_{k.replace('-', '_')}": round(o["difficulty"], 4) for k, o in (summary.get("battle_outcomes") or {}).items()}, "siege_shaping": a.siege_shaping,
                                 **{f"stage_{k.replace('-', '_')}": s for k, s in getattr(self, "played_stages", {}).items()},
                                 **{f"rung_{k.replace('-', '_')}": r for k, r in getattr(self, "played_rungs", {}).items()},
                                 "reward_battle": _suite_mean(summary, "battle/"), "reward_economy": _suite_mean(summary, "economy"),
                                 "play_s": round(t_play), "train_s": round(t_train), "eval_s": 0.0}
            if a.eval_every and i % a.eval_every == 0:
                st["eval_pending"] += [[i, m, su, self.speed] for su in a.eval_suites.split(",") if su for m in a.eval_modes.split(",") if m]
                if self.speed != "standard" and a.eval_standard_speed:
                    st["eval_pending"].append([i, "all", "standard", "standard"])  # the reference at standard speed
                if self.suites.get("battle") and a.battle_eval_games > 0:
                    st["eval_pending"].append([i, "all", "battle", self.speed])
            if a.pace_every and i % a.pace_every == 0:
                st["eval_pending"] += [[i, m, "pace", self.speed] for m in a.pace_modes.split(",") if m]
            self.save_state()
            shutil.rmtree(self.games_dir / f"iter_{i:04d}", ignore_errors=True)
            if self.exploiter:
                self.exploiter.after_commit(i, compact.get("exploiter"))
            self.log(f"iteration {i}: play {t_play:.0f}s, train {t_train:.0f}s ({summary['turns_per_s']:.0f} turns/s), export {t_export:.1f}s; "
                     f"reward {summary.get('mean_reward', float('nan')):.4f}, kl {summary['kl']:.5f}, first ratio {summary['first_update'].get('ratio_mean')}")
            t3 = time.time()
            self.run_evals()
            st["wall_seconds"] += time.time() - t3
            self.write_row()
            self.prune()
        self.log("done")


def _blank_none(v):
    """A progress.csv cell: '' for None (a win share of 0 stays 0)."""
    return "" if v is None else v


def _suite_mean(summary: dict, prefix: str):
    """The mean reward of the suites whose key starts with `prefix` (seats weighted), '' without one."""
    rows = [v for k, v in (summary.get("by_suite") or {}).items() if k.startswith(prefix) and v.get("n")]
    n = sum(v["n"] for v in rows)
    return sum(v["mean"] * v["n"] for v in rows) / n if n else ""


def sequential_line(seq: dict | None) -> str:
    """The log's words for a sequential eval's test ('' for a plain one)."""
    if not seq:
        return ""
    line = f"; SPRT {seq['decision']} after {seq['games']} games"
    if seq.get("reference_share") is not None:
        line += f", minus the start net's {seq['reference_share']:.3f}: {seq['mean']:+.3f} [{seq['low']:+.3f}, {seq['high']:+.3f}]"
    return line


def summarize_report(rep: dict) -> dict:
    """The candidate's points share with its CI, wins, science victories (by
    rounds 400 and 500, their mean round) from an ai-tournament JSON report;
    of a sequential run (`--eval-sprt`) also the test's decision, games, and
    the blocks' mean (the share, or with a reference its difference)."""
    v = rep.get("versus") or {}
    share = v.get("share") or {}
    matches = rep.get("matches") or []
    sci = [m for m in matches if m.get("side") == "candidate" and m.get("victory") == "science"]
    rounds = [int(m["rounds"]) for m in sci]
    cand = next((s for s in rep.get("sides", []) if s.get("side") == "candidate"), {})
    science = cand.get("science") or {}
    seq = rep.get("sequential")
    return {
        "share": share.get("mean"), "share_low": share.get("low"), "share_high": share.get("high"), "blocks": share.get("blocks"),
        "games": v.get("games", len(matches)), "wins": v.get("candidate_wins"),
        "wins_by_kind": cand.get("wins"),
        "science_wins": len(sci),
        "science_by_400": sum(r <= 400 for r in rounds), "science_by_500": sum(r <= 500 for r in rounds),
        "science_round": float(np.mean(rounds)) if rounds else None,
        "science_section": {"victories": science.get("victories"), "victory_round": science.get("victory_round"), "earliest": science.get("earliest"),
                            "project": science.get("project"), "parts": science.get("parts"), "complete": science.get("complete")},
        "rounds": rep.get("rounds"), "wall_seconds": rep.get("wall_seconds"),
        **({"sequential": {k: seq.get(k) for k in ("decision", "games", "blocks", "llr", "mean", "low", "high", "reference_share")}} if seq else {}),
    }


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--state", required=True, help="the run's directory (resumable)")
    ap.add_argument("--init", default=str(NN_DATA / "weights/small-prod10.pt"), help="the imitation checkpoint to start from")
    ap.add_argument("--ref", help="the frozen KL reference (default: --init)")
    ap.add_argument("--iterations", type=int, default=1000)
    ap.add_argument("--hours", type=float, default=0.0, help="stop starting iterations after this many hours (0: no limit)")
    ap.add_argument("--seed", type=int, default=0)
    p = ap.add_argument_group("play")
    p.add_argument("--groups", type=int, default=32, help="GRPO groups an iteration")
    p.add_argument("--group-size", type=int, default=4, help="games of a group (K) of the whole-game suites (standard, duel, pace)")
    p.add_argument("--ground-group-size", type=int, default=8,
                   help="games of a group of the training grounds (battle, economy, opening): their rewards are noisier per game and a game is cheap")
    p.add_argument("--temperature", type=float, default=1.0)
    p.add_argument("--speed", default="auto", choices=["auto", "quick", "standard"],
                   help="game speed of play and eval (auto: quick when the binaries take --speed, else standard); with another speed than standard, "
                        "every eval also runs once at standard speed for reference")
    p.add_argument("--rounds", type=int, default=0, help="the last round; a game stops at its first victory (0: 450 at standard speed, the binary's own at another)")
    p.add_argument("--suites", default="standard:0.5,duel:0.5",
                   help="suite mix of the groups by share (duel: 1 learner against 1 opponent; the mid-game curriculum applies to standard; "
                        "grounds: battle, economy, opening)")
    p.add_argument("--sizes", default="tiny", help="the standard suite's map sizes")
    p.add_argument("--starts-per-offset", type=int, default=20, help="the suite's starts per seed offset and size (standard: 5 maps × 4 seeds)")
    p.add_argument("--seed-offset", type=int, default=100, help="the suite seed offset of iteration 1 (the bench plays offset 0)")
    c = ap.add_argument_group("curriculum (mid-game starts toward the science victory)")
    c.add_argument("--from-logs", default=str(NN_DATA / "e1-standard/logs"), help="the expert logs the mid-game starts continue ('' : every group from round 0)")
    c.add_argument("--from-round", type=int, default=300)
    c.add_argument("--from-round-jitter", type=int, default=30)
    c.add_argument("--learner-seat", default="best-science")
    c.add_argument("--mid-rounds", type=int, default=0, help="the last round of a mid-game start (0: the binary's, the speed's own: 500 at standard)")
    c.add_argument("--give-share", type=float, default=0.25, help="share of the mid-game groups with --give-space-techs (the finish: Apollo and the parts)")
    c.add_argument("--round0-every", type=int, default=4, help="every this many iterations, --round0-share of the groups start at round 0")
    c.add_argument("--round0-share", type=float, default=0.25)
    p.add_argument("--opponents", default="standard:0.75,league:0.25",
                   help="opponent mix by share: bench names (standard, tall-book, tall-science, rush-siege, rush-archer, rush-sword, timing-uu, rex-wide …; "
                        "a book on today's standard: rush-siege@standard, tall-book@standard …, classic/docs/ai-books.md) "
                        "and `league` (a past checkpoint); a game of X has X in one seat and standard in the rest; names the frozen binary lacks go to standard")
    p.add_argument("--opponents-pfsp", action="store_true",
                   help="reweight the opponents' shares every iteration by the learner's recent win share against each: share × max((1 − win)^p, floor)")
    p.add_argument("--pfsp-power", type=float, default=2.0)
    p.add_argument("--pfsp-floor", type=float, default=0.05)
    p.add_argument("--pfsp-decay", type=float, default=0.5, help="the win shares' exponential average over iterations (the old value's weight)")
    p.add_argument("--opponent-seats", type=int, default=3, help="opponent seats of a standard-suite game (Tiny: 3; a duel has 1)")
    p.add_argument("--jobs", type=int, default=12)
    p.add_argument("--gpu-lock", default=str(GPU_LOCK), help="the GPU lock file held during training (flock, as gpu-run.sh's lockf); '' : none")
    p.add_argument("--selfplay-bin", default=str(BIN / "ai-selfplay-e2"))
    gr = ap.add_argument_group("training grounds (suites battle, economy and opening)")
    gr.add_argument("--battle-kinds", default="field:0.25,siege:0.25,defence:0.2,rush-defence:0.3",
                    help="the battle suite's groups dealt to the kinds by share (field, siege, defence, rush-defence, skirmish)")
    gr.add_argument("--battle-rounds", default="20-40", help="a battle's length in rounds, drawn per group")
    gr.add_argument("--battle-army", default="6-10", help="a battle's main army, drawn per group")
    gr.add_argument("--ladder-solvable", action="store_true",
                    help="--siege-ladder: every scene that allows a first-turn capture by construction is solvable: ai-selfplay draws it again "
                         "(up to 8 draws, from the fifth another world) until the shallow search teacher takes it in the learner's first turn "
                         "on one of two fixed combat seeds")
    gr.add_argument("--ladder-pool", default="", metavar="FILE",
                    help="--siege-ladder: a pool of solvable scenes (tools/ladder-pool.py): on a rung it has, every group plays one of its "
                         "scenes (read at every iteration: a pool still growing is taken up as it grows)")
    gr.add_argument("--decision-values", default="", metavar="K,DEPTH,ROLLOUTS[,SHARE]",
                    help="battles: ai-selfplay values the net's unit decisions by the search teacher's rollouts (K kept candidates and the net's choice, "
                         "ROLLOUTS seeds of DEPTH round ends, a SHARE of the decisions); with --decision-advantage λ the trainer adds λ × (Q(chosen) − baseline)")
    gr.add_argument("--economy-logs", default="", help="the expert logs of the economy windows (default --from-logs)")
    gr.add_argument("--economy-rounds", default="50,100,150,200,250,300,350",
                    help="an economy window's start round, drawn per group from the list (a value repeated weighs more; 0: an opening window from the game's start)")
    gr.add_argument("--economy-opening-window", type=int, default=0,
                    help="an opening window's rounds (0: the binary's default, 90 Standard rounds at the logs' speed: 60 at Quick)")
    gr.add_argument("--economy-window", default="50-80", help="an economy window's rounds, drawn per group")
    gr.add_argument("--opening-suite", default="duel", choices=["duel", "standard"],
                    help="the bench suite whose maps and seeds the openings play (duel: the duel map, the learner and one rival; standard: --sizes)")
    gr.add_argument("--opening-window", type=int, default=0,
                    help="the opening ground's rounds from the game's start (0: the binary's, 135 Standard rounds at the game speed, 89 at Quick); the suite `opening` in --suites")
    gr.add_argument("--curriculum-rate", type=float, default=1.0, help="battle curriculum: knob × 2^(2 × rate × (win share − 0.5)) an iteration")
    gr.add_argument("--curriculum-decay", type=float, default=0.5, help="battle curriculum: the win share's exponential average (the old value's weight)")
    gr.add_argument("--stage-kinds", default="", help="battle kinds whose start stage the curriculum moves (the reverse curriculum; e.g. field,siege; "
                    "default none: the documented placement, no --battle-stage)")
    gr.add_argument("--stage-start", type=int, default=0, help="the first start stage (0: the attackers at the walls, the armies in contact in a field)")
    gr.add_argument("--stage-last", type=int, default=4, help="the last start stage (rounds of march back from the walls; home at most)")
    gr.add_argument("--stage-advance", type=float, default=0.6, help="a kind's averaged win share at which its start moves back one stage")
    gr.add_argument("--stage-retreat", type=float, default=0.3, help="a kind's averaged win share at which its start moves forward one stage")
    gr.add_argument("--stage-band", action="store_true",
                    help="the stage kinds' difficulty knob moves by the same thresholds: harder from --stage-advance up, easier from --stage-retreat down, "
                         "still between (`staged_curriculum`); without it the knob pivots at a win share of 0.5")
    gr.add_argument("--siege-ladder", action="store_true",
                    help="the siege's start is the siege ladder's rung (`--battle-rung`, grounds::LADDER: the city at 1 hit point and a capturer at its walls, "
                         "the one-turn rungs (move, step aside, shoot, ride in, then strike in the first turn), the ring of ranged units, fewer, "
                         "melee only, farther, then today's start stages to --stage-last) in its minimal world "
                         "(`--battle-minimal`: the duel map, two nations, no city-states or barbarians), moved by the teacher's capture share on the rung "
                         "(`ladder_curriculum`) instead of --stage-kinds")
    gr.add_argument("--rung-start", type=int, default=0, help="--siege-ladder: the first rung")
    gr.add_argument("--rung-advance", type=float, default=0.9, help="--siege-ladder: one rung up at an averaged capture share ≥ this × the teacher's on the rung")
    gr.add_argument("--rung-retreat", type=float, default=0.4, help="--siege-ladder: one rung down below this × the teacher's capture share on the rung")
    gr.add_argument("--ladder-vary", action="store_true",
                    help="--siege-ladder: varied scenes (`--battle-vary`: the war's round, the target among the enemy's cities, the units among the kinds "
                         "of each role, the side of approach, every map script), the teacher's table LADDER_TEACHER_VARY")
    gr.add_argument("--rung-success", default="any", choices=["any", "first-turn"],
                    help="--siege-ladder: a rung's win — any capture (as before), or on the rungs whose scene allows it (`same_turn`) only a capture "
                         "in the learner's first turn (the teacher's table LADDER_TEACHER_VARY_FIRST with --ladder-vary)")
    gr.add_argument("--rung-teacher", default="", help="--siege-ladder: the teacher's capture share per rung, comma-separated (default LADDER_TEACHER, `standard` measured)")
    gr.add_argument("--skirmish-ladder", action="store_true",
                    help="with `skirmish` in --battle-kinds: its start is the skirmish ladder's rung (`--battle-rung`, grounds::SKIRMISH: one blow at a unit at "
                         "1 hit point, one against a wounded one, one against one, two against one or two, three or four a side, six a side, six against eight; "
                         "the minimal world, varied scenes), moved by `ladder_curriculum` with --rung-advance and --rung-retreat")
    gr.add_argument("--skirmish-rung-start", type=int, default=0, help="--skirmish-ladder: the first rung")
    gr.add_argument("--skirmish-teacher", default="",
                    help="--skirmish-ladder: the share per rung the thresholds multiply ('' : 1.0, absolute: up at 0.9, down below 0.4; "
                         "`standard`: SKIRMISH_STANDARD, standard's measured win shares; or comma-separated shares)")
    gr.add_argument("--clash-army", default="8-14", help="the battle window's (`clash` in --battle-kinds) units a side, drawn per group")
    gr.add_argument("--clash-rounds", default="4-5", help="the battle window's rounds after its approach, drawn per group")
    gr.add_argument("--clash-ladder", action="store_true",
                    help="with `clash` in --battle-kinds: its start is the battle window's advantage ladder (`--battle-rung`, grounds::CLASH: the enemy "
                         "at half the count and 40 %% health in contact up to even two rounds apart), moved by `ladder_curriculum` on the share of windows "
                         "that gain value with --rung-advance and --rung-retreat × --clash-teacher")
    gr.add_argument("--clash-rung-start", type=int, default=0, help="--clash-ladder: the first rung")
    gr.add_argument("--clash-stage-start", type=int, default=0, help="without --clash-ladder: the battle window's approach stage (0: in contact), even")
    gr.add_argument("--clash-teacher", default="",
                    help="--clash-ladder: the share per rung the thresholds multiply ('' : 1.0, absolute; `standard`: CLASH_STANDARD; or comma-separated)")
    gr.add_argument("--skirmish-eval-rung", type=int, default=3, help="the battle eval's skirmish rung (with `skirmish` in --battle-kinds)")
    gr.add_argument("--battle-eval-games", type=int, default=40, help="battles per kind of the battle eval (0: none); with battles in the suite mix")
    gr.add_argument("--battle-eval-offset", type=int, default=900, help="the battle eval's seed offset (fixed: every iteration the same battles)")
    e = ap.add_argument_group("eval and checkpoints")
    e.add_argument("--keep", type=int, default=5, help="checkpoints kept (besides iter_0000 and the bests)")
    e.add_argument("--eval-every", type=int, default=5)
    e.add_argument("--eval-standard-speed", action="store_true",
                   help="a training speed other than standard: also evaluate the standard suite at standard speed (slow; off by default)")
    e.add_argument("--eval-modes", default="all,hybrid")
    e.add_argument("--eval-suites", default="standard,duel", help="bench suites of every eval (80 games each)")
    e.add_argument("--eval-jobs", type=int, default=8)
    e.add_argument("--pace-every", type=int, default=20, help="the pace suite (500 rounds) every this many iterations (0: never)")
    e.add_argument("--pace-modes", default="all")
    e.add_argument("--pace-repeat", type=int, default=1, help="pace: seed offsets (1 = its 20 seed blocks, the bench's minimum for a verdict)")
    e.add_argument("--tournament-bin", default=str(BIN / "ai-tournament-e2"))
    e.add_argument("--eval-sprt", default="", metavar="S0,S1",
                   help="sequential evals of the standard and duel suites: seed blocks until an SPRT accepts H0 (the share ≤ S0) or H1 (≥ S1), "
                        "with --eval-reference start the share minus the start net's on the same games (e.g. -0.05,0.05); '' : 80 games as before")
    e.add_argument("--eval-sprt-error", default="0.05,0.05", metavar="ALPHA,BETA", help="the sequential eval's error rates")
    e.add_argument("--eval-max-games", type=int, default=240, help="a sequential eval's cap (undecided after it)")
    e.add_argument("--eval-min-games", type=int, default=0, help="a sequential eval's minimum (0: the binary's, 60 games and 4 seed blocks)")
    e.add_argument("--eval-reference", default="none", choices=["none", "start"],
                   help="start: a sequential eval tests the share minus the start net's (iter_0000) on the same games, played once")
    train_e2.add_arguments(ap)
    from . import e2_exploiter

    e2_exploiter.add_arguments(ap)
    a = ap.parse_args(argv)
    if a.temperature <= 0:
        raise SystemExit("--temperature must be > 0: policy gradient needs sampled games")
    Loop(a).run()


if __name__ == "__main__":
    main()
