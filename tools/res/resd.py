#!/usr/bin/env python3
"""The broker's daemon: a dynamic CPU budget and a queue of background jobs (started by `res start`).

Every `tick` seconds it writes $RES_HOME/budget.json (read by `res run`) and starts queued jobs into free slots.

Budget (threads for our jobs; the ceiling is detected, see res.py `machine()`):
- on battery: cpu_battery;
- on the charger: cpu_charger; if the battery falls by 2 points or more in 10 minutes on the charger (the charger
  cannot keep up), 70 % of it;
- heat: a cap that rises by one a tick while ProcessInfo.thermalState is nominal, holds while fair, falls by two
  while serious (never below half the ceiling), the floor at critical; a pmset CPU speed limit below 100 % scales it;
- other load on the machine (CPU of processes that are not ours) above `other_load_allowed` cores: the budget gives
  way by the excess, never below cpu_floor;
- RAM: below ram_free_min_percent free, no new queued job starts.
Queue: $RES_HOME/queue/<priority>-<name>.json = {"cmd": [...], "cwd": "...", "cpu": "2-6", "gpu": false,
"ram": 0, "log": "..."} (`res queue`); a lower priority number starts first; started jobs move to queue/running/,
finished ones to queue/done/ with their exit code appended to the log.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import res  # noqa: E402 (the broker's own module: paths, machine detection, the slot test)

HOME, QUEUE, BUDGET, LOG = res.HOME, res.QUEUE, res.BUDGET_FILE, res.LOG
OURS = ("cargo", "rustc", "nextest", "/ld", "clang", "ld64", "aurelia", "ai-exam", "ai-tournament", "ai-record",
        "train_m", "pytest", "resd.py", "res.py", "swift", str(res.REPO))


def log(line: str) -> None:
    HOME.mkdir(parents=True, exist_ok=True)
    with open(LOG, "a") as f:
        f.write(f"{time.strftime('%d %H:%M:%S')} {line}\n")


def power() -> tuple[bool, int]:
    out = res.sh("pmset", "-g", "batt")
    pct = 100
    for word in out.split():
        if word.endswith("%;"):
            pct = int(word[:-2])
    return "Battery Power" not in out, pct


def speed_limit() -> int:
    for line in res.sh("pmset", "-g", "therm").splitlines():
        if "CPU_Speed_Limit" in line:
            return int(line.split("=")[-1].strip())
    return 100


def thermal() -> str:
    probe = HOME / "bin" / "thermal"
    if probe.exists():
        return res.sh(str(probe)).strip() or "unknown"
    return "nominal" if speed_limit() >= 100 else "serious"


def other_load() -> float:
    total = 0.0
    for line in res.sh("ps", "-axo", "pcpu=,command=").splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) < 2 or any(mark in parts[1] for mark in OURS):
            continue
        total += float(parts[0])
    return total / 100


def ram_free_percent() -> int:
    for line in res.sh("memory_pressure").splitlines():
        if "free percentage" in line:
            return int(line.split(":")[-1].strip().rstrip("%"))
    return 100


class Budget:
    def __init__(self) -> None:
        self.history: list[tuple[float, int]] = []
        self.last = None
        self.cap = res.machine()["cpu_charger"]
        self.idle_ticks = 0

    def compute(self) -> dict:
        m = res.machine()  # re-read each tick: res.toml edits apply without a restart
        ceiling, battery, floor = m["cpu_charger"], m["cpu_battery"], m["cpu_floor"]
        charger, pct = power()
        now = time.time()
        self.history = [(t, p) for t, p in self.history if now - t <= 600] + [(now, pct)]
        held = res.held_count()
        if not charger:
            base, why = battery, "battery"
        elif self.history[0][1] - pct >= 2:
            base, why = max(floor, ceiling * 7 // 10), f"charger cannot keep up ({self.history[0][1]} -> {pct} %)"
        else:
            base, why = ceiling, "charger"
        heat = thermal()
        if heat == "nominal":
            self.cap = min(ceiling, self.cap + 1)
        elif heat == "serious":
            self.cap = max(ceiling // 2, floor, self.cap - 2)
        elif heat == "critical":
            self.cap = floor
        if self.cap < base:
            base, why = self.cap, f"{why}; thermal {heat}, cap {self.cap}"
        limit = speed_limit()
        if limit < 100:
            base, why = max(floor, base * limit // 100), f"{why}; thermal limit {limit} %"
        other = other_load()
        if other > m["other_load_allowed"]:
            give = int(other - m["other_load_allowed"])
            if give > 0:
                base, why = max(floor, base - give), f"{why}; other load {other:.1f}"
        ram = ram_free_percent()
        starts = ram >= m["ram_free_min_percent"]
        if not starts:
            why = f"{why}; RAM {ram} % free, no new starts"
        queued = len(list(QUEUE.glob("*.json")))
        self.idle_ticks = self.idle_ticks + 1 if held <= base - 3 and queued == 0 else 0
        return {"time": now, "budget": base, "held": held, "starts": starts, "why": why, "battery": pct,
                "charger": charger, "thermal": heat, "queued": queued, "idle_ticks": self.idle_ticks}


def start_queued(state: dict) -> None:
    if not state["starts"]:
        return
    free = state["budget"] - state["held"]
    (QUEUE / "running").mkdir(parents=True, exist_ok=True)
    (QUEUE / "done").mkdir(exist_ok=True)
    for path in sorted(QUEUE.glob("*.json")):
        job = json.loads(path.read_text())
        low = int(str(job.get("cpu", "1")).split("-")[0])
        if low > free:
            continue
        running = QUEUE / "running" / path.name
        path.rename(running)
        args = [str(HERE / "res"), "run", "--cpu", str(job.get("cpu", "1")), "--name", path.stem, "--ram", str(job.get("ram", 0))]
        if job.get("gpu"):
            args.append("--gpu")
        wrapper = ["sh", "-c", 'r=0; "$@" || r=$?; echo "exit $r" >> "$LOG_FILE"; mv "$0" "$(dirname "$0")/../done/" 2>/dev/null',
                   str(running), *args, "--", *job["cmd"]]
        logfile = job.get("log", str(HOME / f"{path.stem}.log"))
        env = dict(os.environ, LOG_FILE=logfile)
        with open(logfile, "a") as out:
            subprocess.Popen(wrapper, cwd=job.get("cwd", str(HOME)), stdout=out, stderr=out, env=env, start_new_session=True)
        log(f"started {path.name} ({job.get('cpu')} CPU)")
        return


def main() -> int:
    QUEUE.mkdir(parents=True, exist_ok=True)
    budget = Budget()
    end = time.time() + float(sys.argv[1] if len(sys.argv) > 1 else 30 * 86400)
    while time.time() < end:
        state = budget.compute()
        BUDGET.write_text(json.dumps(state))
        if budget.last != (state["budget"], state["why"].split(";")[0]):
            log(f"budget {state['budget']} ({state['why']}), held {state['held']}")
            budget.last = (state["budget"], state["why"].split(";")[0])
        start_queued(state)
        time.sleep(res.machine()["tick"])
    log("daemon ended")
    return 0


if __name__ == "__main__":
    sys.exit(main())
