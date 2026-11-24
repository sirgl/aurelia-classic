#!/usr/bin/env python3 -I
"""A watch on training runs (Roman, 23 Nov 2026: «нужен watcher на параметры обучения или триггеры, что что-то идёт
не так … или что-то неоптимально»). Every 30 s reads each run dir under RUNS (curves.jsonl, config.json) and the
training processes; prints one line per NEW alert (the coordinator's Monitor stream) and keeps the current alerts in
alerts.json for the dashboard. Read-only on the runs.

Triggers (each with its threshold here, in one place):
- slow: decisions/s after warm-up below the run's target (A 20 000, B 5 000; config `target_dps` wins);
- nan: a loss is NaN or infinite;
- diverge: train loss above 1.5 × its minimum for 3 points in a row;
- overfit: hold loss up 3 evaluations in a row while train loss goes down;
- plateau: hold acc_both not better by 0.5 point over the last 6 evaluations (after the first epoch);
- stall: no new curve point for 10 min while the run's process is alive;
- died: the run's curves stopped, no process, and no final weights;
- grad: gradient norm (if logged as `grad_norm`) above 100 or 0;
- memory: the training process above 24 GB resident;
- gpu-idle: a run's process waits for the broker's GPU lock for more than 20 min (it shows as `res: waiting`).
"""

import json
import math
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
LAB = Path(os.environ.get("AURELIA_LAB") or REPO / "lab")  # runs/, logs/, data/ of the experiments
RUNS_DIRS = [LAB / "runs"]
STATE = LAB / "dash" / "alerts.json"
STATE.parent.mkdir(parents=True, exist_ok=True)
TARGET = {"map": 5000, "nomap": 20000, "memory": 15000}
WARM_STEPS = 200
RSS_GB = 24
STALL_S = 600


def curves(d: Path) -> list[dict]:
    p = d / "curves.jsonl"
    if not p.exists():
        return []
    out = []
    for line in p.read_text().splitlines():
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    return out


def config(d: Path) -> dict:
    p = d / "config.json"
    try:
        return json.loads(p.read_text()) if p.exists() else {}
    except json.JSONDecodeError:
        return {}


def procs() -> list[tuple[int, int, str]]:
    out = subprocess.run(["ps", "-axo", "pid=,rss=,command="], capture_output=True, text=True).stdout
    rows = []
    for line in out.splitlines():
        parts = line.strip().split(None, 2)
        if len(parts) == 3 and ("army.train" in parts[2] or "aurelia_nn" in parts[2]):
            rows.append((int(parts[0]), int(parts[1]), parts[2]))
    return rows


def check() -> list[tuple[str, str]]:
    alerts: list[tuple[str, str]] = []
    ps = procs()
    for root in RUNS_DIRS:
        if not root.exists():
            continue
        for d in sorted(p for p in root.iterdir() if p.is_dir()):
            c = curves(d)
            if not c or d.name.startswith("smoke"):
                continue
            cfg = config(d)
            name = d.name
            mine = [p for p in ps if f"/{name}" in p[2] or f" {name}" in p[2]]
            alive = bool(mine)
            last = c[-1]
            age = time.time() - (d / "curves.jsonl").stat().st_mtime
            # finished: the final weights are written and the last point reached the planned epochs (a process that
            # merely names the run — an evaluation reading its weights — is not training); a finished run is history,
            # every trigger below is about a run still training
            planned = float(cfg.get("epochs", 0) or 0)
            if (d / "weights.safetensors").exists() and (planned == 0 or last.get("epoch", 0) >= planned - 0.01):
                continue
            # nan
            for r in c[-3:]:
                vals = [r.get("train_loss"), r.get("hold", {}).get("loss")]
                if any(isinstance(v, float) and (math.isnan(v) or math.isinf(v)) for v in vals):
                    alerts.append((f"{name}:nan", f"{name}: потеря NaN/inf на шаге {r.get('step')}"))
                    break
            # slow
            # variant M (memory, unrolled over turns): its gate is 15 000 (army-net/M-HANDOFF.md, gate 3)
            # the B line (MB, MBH, MBN: memory + the map) has B's map target
            target = cfg.get("target_dps") or (TARGET["map"] if cfg.get("map") or name.endswith("B") or name.startswith("netMB") else TARGET["memory"] if name.startswith("netM") else TARGET["nomap"])
            warm = [r for r in c if r.get("step", 0) >= WARM_STEPS and isinstance(r.get("dps"), (int, float))]
            # the mean of the last 5 points, 10 % slack: the machine is shared and a single point flaps around the line
            if warm:
                recent = [r["dps"] for r in warm[-5:]]
                rate = sum(recent) / len(recent)
                if rate < 0.9 * target:
                    alerts.append((f"{name}:slow", f"{name}: в среднем {rate:.0f} решений/с при цели {target}"))
            # diverge
            tl = [r.get("train_loss") for r in c if isinstance(r.get("train_loss"), (int, float))]
            if len(tl) >= 4 and min(tl) > 0 and all(v > 1.5 * min(tl) for v in tl[-3:]):
                alerts.append((f"{name}:diverge", f"{name}: потеря обучения в 1,5 раза выше минимума три точки подряд"))
            # overfit / plateau on hold evaluations
            ev = [r for r in c if isinstance(r.get("hold"), dict) and "loss" in r["hold"]]
            if len(ev) >= 4:
                hl = [r["hold"]["loss"] for r in ev[-4:]]
                tr = [r.get("train_loss", 0) for r in ev[-4:]]
                if hl[0] < hl[1] < hl[2] < hl[3] and tr[3] < tr[0]:
                    alerts.append((f"{name}:overfit", f"{name}: потеря на отложенных растёт 3 проверки подряд, на обучении падает"))
            if len(ev) >= 7 and ev[-1].get("epoch", 0) >= 1:
                acc = [r["hold"].get("acc_both", 0) for r in ev[-7:]]
                if max(acc[1:]) < acc[0] + 0.005:
                    alerts.append((f"{name}:plateau", f"{name}: точность на отложенных не растёт 6 проверок ({acc[-1]*100:.1f} %)"))
            # grad
            g = last.get("grad_norm")
            if isinstance(g, (int, float)) and (g > 100 or g == 0):
                alerts.append((f"{name}:grad", f"{name}: норма градиента {g:.3g}"))
            # stall / died
            if alive and age > STALL_S:
                alerts.append((f"{name}:stall", f"{name}: нет новых точек {age/60:.0f} мин, процесс жив"))
            # a run stopped by the GPU-job time limit is paused, not dead (its log says so; it is resumed later)
            log = d.parent.parent / "logs" / f"{name}.log"
            paused = log.exists() and "stopped at step" in log.read_text(errors="ignore")[-4000:]
            if not alive and not paused and age > STALL_S:
                alerts.append((f"{name}:died", f"{name}: точки остановились {age/60:.0f} мин назад, процесса нет, итоговых весов нет"))
            # memory
            for pid, rss_kb, _ in mine:
                if rss_kb / 1024 / 1024 > RSS_GB:
                    alerts.append((f"{name}:memory", f"{name}: процесс {pid} занимает {rss_kb/1024/1024:.1f} ГБ"))
    # waiting for the GPU lock
    for pid, _, cmd in ps:
        pass
    out = subprocess.run(["ps", "-axo", "pid=,etime=,command="], capture_output=True, text=True).stdout
    for line in out.splitlines():
        if "res.py run" in line and "--gpu" in line:
            parts = line.split(None, 2)
            et = parts[1]
            mins = 0
            bits = et.replace("-", ":").split(":")
            try:
                nums = [int(b) for b in bits]
                mins = nums[-2] + 60 * (nums[-3] if len(nums) >= 3 else 0) + 1440 * (nums[-4] if len(nums) >= 4 else 0)
            except ValueError:
                pass
            # a GPU job's wrapper that has started nothing yet is still waiting for the lock (the holder runs a child)
            children = subprocess.run(["pgrep", "-P", parts[0]], capture_output=True, text=True).stdout.split()
            if mins >= 20 and not children:
                label = parts[2].split("--name", 1)[-1].split()[0] if "--name" in parts[2] else "?"
                alerts.append((f"gpu-wait:{parts[0]}", f"задача «{label}» ждёт видеокарту {mins} мин (pid {parts[0]})"))
    return alerts


def main() -> int:
    end = time.time() + float(sys.argv[1] if len(sys.argv) > 1 else 1790)
    seen: dict[str, str] = {}
    # a run's speed hovers around the line: its slow alert is told once per run, never cleared or re-raised
    # kept across the watch's restarts (it is re-armed every 30 min)
    told_file = STATE.with_name("told_slow.json")
    told_slow: set[str] = set(json.loads(told_file.read_text())) if told_file.exists() else set()
    while time.time() < end:
        try:
            now = dict(check())
        except Exception as e:  # noqa: BLE001 (a watch reports its own failure and goes on)
            now = {"watch:error": f"сторож обучения: ошибка {e!r}"}
        for k, msg in now.items():
            if k.endswith(":slow"):
                if k not in told_slow:
                    told_slow.add(k)
                    told_file.write_text(json.dumps(sorted(told_slow)))
                    print(f"{time.strftime('%H:%M')} TRAIN {msg} (сообщаю один раз за прогон)", flush=True)
            elif k not in seen:
                print(f"{time.strftime('%H:%M')} TRAIN {msg}", flush=True)
        for k in seen:
            if k not in now and not k.endswith(":slow"):
                print(f"{time.strftime('%H:%M')} TRAIN cleared: {seen[k]}", flush=True)
        seen = now
        STATE.write_text(json.dumps({"time": time.time(), "alerts": list(now.values())}, ensure_ascii=False))
        time.sleep(30)
    return 0


if __name__ == "__main__":
    sys.exit(main())
