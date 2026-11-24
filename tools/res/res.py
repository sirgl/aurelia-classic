#!/usr/bin/env python3
"""The resource broker for the army-net experiments: CPU threads, one GPU, disk, RAM, a queue.

    res start | stop | status | info
    res run --cpu N|MIN-MAX [--gpu] [--ram GB] [--disk GB] [--name LABEL] -- COMMAND ARGS...
    res queue --cpu 2-6 [--gpu] [--ram GB] [--priority 50] [--name N] [--cwd DIR] [--log FILE] -- COMMAND...

State lives in $RES_HOME (default <repo>/.res); nothing is tied to a user or a path. The budget is detected on
start: performance cores (`sysctl hw.perflevel0.physicalcpu`), RAM (`hw.memsize`), power source and the
thermal state (`pmset`, ProcessInfo via a small Swift probe when swiftc exists). `res.toml` overrides any of it.

- CPU: a job takes N slots (flock on slot files) and waits until N fit inside the budget. `--cpu MIN-MAX` is
  elastic; the job reads RES_THREADS. The budget is read when the job asks.
- GPU: one job at a time (flock on $RES_HOME/gpu.lock). Locks die with the process.
- Disk: the job waits while free space is below --disk GB (default disk_min_gb, 25).
- RAM: the job waits while free + inactive memory is below --ram GB.
The command gets CARGO_BUILD_JOBS, RAYON_NUM_THREADS, OMP_NUM_THREADS, AURELIA_THREADS and RES_THREADS = N, and
RES_MLX_GB (the MLX memory limit); it runs under nice 10; its exit code is passed through.
"""
from __future__ import annotations

import argparse
import fcntl
import json
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
HOME = Path(os.environ.get("RES_HOME") or REPO / ".res")
SLOTS = HOME / "cpu"
QUEUE = HOME / "queue"
BUDGET_FILE = HOME / "budget.json"
GPU_LOCK = HOME / "gpu.lock"
PID_FILE = HOME / "resd.pid"
LOG = HOME / "resd.log"
MAX_SLOTS = 64
WAIT_SECONDS = 10


def sh(*cmd: str) -> str:
    try:
        return subprocess.run(list(cmd), capture_output=True, text=True, timeout=20).stdout
    except (OSError, subprocess.TimeoutExpired):
        return ""


def sysctl_int(key: str, default: int) -> int:
    out = sh("sysctl", "-n", key).strip()
    return int(out) if out.isdigit() else default


def read_config() -> dict:
    """A flat `key = value` file (ints, floats, strings): no TOML library needed, so any python3 works."""
    for path in (os.environ.get("RES_CONFIG"), HOME / "res.toml", HERE / "res.toml"):
        if path and Path(path).exists():
            cfg = {}
            for line in Path(path).read_text().splitlines():
                line = line.split("#", 1)[0].strip()
                if "=" not in line:
                    continue
                key, value = (x.strip() for x in line.split("=", 1))
                value = value.strip('"\'')
                try:
                    cfg[key] = int(value)
                except ValueError:
                    try:
                        cfg[key] = float(value)
                    except ValueError:
                        cfg[key] = value
            return cfg
    return {}


def machine() -> dict:
    """What the machine is and the budgets derived from it, with res.toml overrides applied."""
    cfg = read_config()
    cores = sysctl_int("hw.physicalcpu", os.cpu_count() or 4)
    perf = sysctl_int("hw.perflevel0.physicalcpu", cores)
    eff = max(0, cores - perf)
    ram = float(cfg.get("ram_total_gb", sysctl_int("hw.memsize", 16 * 2**30) / 2**30))
    charger = int(cfg.get("cpu_charger", max(2, perf)))  # performance cores only: the efficiency cores stay for the system
    battery = int(cfg.get("cpu_battery", max(2, perf // 2)))
    mlx = float(cfg.get("mlx_gb", max(6.0, ram * 0.6)))
    return {"cores": cores, "perf_cores": perf, "eff_cores": eff, "ram_gb": round(ram, 1), "cpu_charger": charger,
            "cpu_battery": battery, "cpu_floor": int(cfg.get("cpu_floor", 2)), "mlx_gb": round(mlx, 1),
            "disk_min_gb": float(cfg.get("disk_min_gb", 25)), "tick": int(cfg.get("tick", 30)),
            "other_load_allowed": float(cfg.get("other_load_allowed", 2)),
            "ram_free_min_percent": int(cfg.get("ram_free_min_percent", 12)),
            "chip": sh("sysctl", "-n", "machdep.cpu.brand_string").strip()}


def on_charger() -> bool:
    out = sh("pmset", "-g", "batt")
    if "Battery Power" in out:
        return False
    return True  # AC Power, or a desktop without a battery


def cpu_budget() -> int:
    """The daemon's budget when it is fresh, else the static one from the detected machine."""
    try:
        state = json.loads(BUDGET_FILE.read_text())
        if time.time() - state["time"] < 120:
            return int(state["budget"])
    except (OSError, ValueError, KeyError):
        pass
    m = machine()
    return m["cpu_charger"] if on_charger() else m["cpu_battery"]


def try_lock(path: Path):
    path.parent.mkdir(parents=True, exist_ok=True)
    handle = open(path, "a+")
    try:
        fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        handle.close()
        return None
    return handle


def held_count() -> int:
    count = 0
    for index in range(MAX_SLOTS):
        path = SLOTS / f"slot-{index:02}"
        if not path.exists():
            continue
        handle = try_lock(path)
        if handle is None:
            count += 1
        else:
            handle.close()
    return count


def free_disk_gb() -> float:
    return shutil.disk_usage(HOME if HOME.exists() else REPO).free / 2**30


def free_ram_gb() -> float:
    out = sh("vm_stat")
    page = 16384
    for word in out.splitlines()[:1]:
        if "page size of" in word:
            page = int(word.split("page size of")[1].split()[0])
    pages = 0
    for line in out.splitlines():
        if line.startswith(("Pages free", "Pages inactive")):
            pages += int(line.split(":")[1].strip().rstrip("."))
    return pages * page / 2**30


def take_slots(low: int, high: int, over: int = 0) -> list:
    room = cpu_budget() + over - held_count()
    want = min(high, room)
    if want < low:
        return []
    taken = []
    for index in range(MAX_SLOTS):
        handle = try_lock(SLOTS / f"slot-{index:02}")
        if handle is None:
            continue
        taken.append(handle)
        if len(taken) == want:
            return taken
    if len(taken) >= low:
        return taken
    for handle in taken:
        handle.close()
    return []


def cpu_range(text: str) -> tuple[int, int]:
    if "-" in text:
        low, high = text.split("-", 1)
        return int(low), int(high)
    return int(text), int(text)


def note(handle, name: str) -> None:
    handle.seek(0)
    handle.truncate()
    handle.write(f"{os.getpid()}\t{time.strftime('%H:%M:%S')}\t{name}\n")
    handle.flush()


def wait_for(what: str, ready) -> None:
    said = False
    while not ready():
        if not said:
            print(f"res: waiting for {what}", file=sys.stderr, flush=True)
            said = True
        time.sleep(WAIT_SECONDS)


def acquire(args) -> tuple[list, object]:
    disk = args.disk if args.disk is not None else machine()["disk_min_gb"]
    wait_for(f"{disk} GB of free disk", lambda: free_disk_gb() >= disk)
    wait_for(f"{args.ram} GB of free RAM", lambda: free_ram_gb() >= args.ram)
    low, high = cpu_range(args.cpu)
    said = False
    while True:
        held = take_slots(min(low, cpu_budget()), high, args.over)
        if held:
            break
        if not said:
            print(f"res: waiting for {low} CPU slots", file=sys.stderr, flush=True)
            said = True
        time.sleep(WAIT_SECONDS)
    gpu = None
    if args.gpu:
        gpu = open(GPU_LOCK, "a+")
        try:
            fcntl.flock(gpu, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print("res: waiting for the GPU", file=sys.stderr, flush=True)
            fcntl.flock(gpu, fcntl.LOCK_EX)
    for handle in held:
        note(handle, args.name)
    return held, gpu


def run(args) -> int:
    SLOTS.mkdir(parents=True, exist_ok=True)
    held, gpu = acquire(args)
    threads = str(len(held))
    env = dict(os.environ)
    for key in ("RES_THREADS", "CARGO_BUILD_JOBS", "RAYON_NUM_THREADS", "OMP_NUM_THREADS", "AURELIA_THREADS"):
        env[key] = threads
    env["RES_MLX_GB"] = str(machine()["mlx_gb"])
    env["RES_HOME"] = str(HOME)
    child = subprocess.Popen(["nice", "-n", "10", *args.command], env=env)
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, lambda s, _f: child.send_signal(s))
    return child.wait()


def info() -> int:
    m = machine()
    m["on_charger"] = on_charger()
    m["budget_now"] = cpu_budget()
    print(json.dumps(m, indent=1))
    return 0


def status() -> int:
    SLOTS.mkdir(parents=True, exist_ok=True)
    m = machine()
    why = ""
    try:
        why = " (" + json.loads(BUDGET_FILE.read_text())["why"] + ")"
    except (OSError, ValueError, KeyError):
        why = " (static: the daemon is down; `res start`)"
    print(f"{m['chip']}: {m['perf_cores']}P+{m['eff_cores']}E cores, {m['ram_gb']} GB RAM; power: {'charger' if on_charger() else 'battery'}; "
          f"CPU budget {cpu_budget()}{why}; held {held_count()}; free disk {free_disk_gb():.0f} GB; free RAM {free_ram_gb():.0f} GB")
    for index in range(MAX_SLOTS):
        path = SLOTS / f"slot-{index:02}"
        if path.exists():
            handle = try_lock(path)
            if handle is None:
                print(f"slot {index:02}: {path.read_text().strip()}")
            else:
                handle.close()
    gpu = try_lock(GPU_LOCK)
    if gpu is None:
        print("gpu: busy")
    else:
        gpu.close()
        print("gpu: free")
    print(f"queued {len(list(QUEUE.glob('*.json')))}, running {len(list((QUEUE / 'running').glob('*.json')))}")
    return 0


def daemon_alive() -> int | None:
    try:
        pid = int(PID_FILE.read_text())
        os.kill(pid, 0)
        return pid
    except (OSError, ValueError):
        return None


def start() -> int:
    HOME.mkdir(parents=True, exist_ok=True)
    pid = daemon_alive()
    if pid:
        print(f"res: daemon already running (pid {pid})")
        return 0
    probe = HOME / "bin" / "thermal"
    swift = HERE / "thermal.swift"
    if not probe.exists() and shutil.which("swiftc") and swift.exists():
        probe.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["swiftc", "-O", str(swift), "-o", str(probe)], capture_output=True)
    out = open(HOME / "resd.out", "a")
    child = subprocess.Popen([sys.executable, "-I", str(HERE / "resd.py")], stdout=out, stderr=out, start_new_session=True,
                             env=dict(os.environ, RES_HOME=str(HOME)))
    PID_FILE.write_text(str(child.pid))
    for _ in range(20):
        if BUDGET_FILE.exists() and time.time() - json.loads(BUDGET_FILE.read_text())["time"] < 60:
            break
        time.sleep(0.5)
    print(f"res: daemon started (pid {child.pid}), state in {HOME}")
    return status()


def stop() -> int:
    pid = daemon_alive()
    if not pid:
        print("res: daemon not running")
        return 0
    os.kill(pid, signal.SIGTERM)  # only our own daemon, by its pid file
    PID_FILE.unlink(missing_ok=True)
    print(f"res: daemon {pid} stopped")
    return 0


def queue(args) -> int:
    QUEUE.mkdir(parents=True, exist_ok=True)
    job = {"cmd": args.command, "cwd": os.path.abspath(args.cwd or os.getcwd()), "cpu": args.cpu, "gpu": args.gpu, "ram": args.ram}
    if args.log:
        job["log"] = os.path.abspath(args.log)
    path = QUEUE / f"{args.priority:03}-{args.name}.json"
    path.write_text(json.dumps(job))
    print(path)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(prog="res")
    sub = parser.add_subparsers(dest="cmd", required=True)
    for name in ("run", "queue"):
        p = sub.add_parser(name)
        p.add_argument("--cpu", default="1", help="N or MIN-MAX (elastic)")
        p.add_argument("--gpu", action="store_true")
        p.add_argument("--ram", type=float, default=0)
        p.add_argument("--name", default="job")
        p.add_argument("command", nargs=argparse.REMAINDER)
        if name == "run":
            p.add_argument("--over", type=int, default=0)
            p.add_argument("--disk", type=float, default=None)
        else:
            p.add_argument("--priority", type=int, default=50)
            p.add_argument("--cwd")
            p.add_argument("--log")
    for name in ("start", "stop", "status", "info"):
        sub.add_parser(name)
    args = parser.parse_args()
    if args.cmd in ("run", "queue"):
        if args.command and args.command[0] == "--":
            args.command = args.command[1:]
        if not args.command:
            parser.error(f"{args.cmd} needs a command after --")
        return run(args) if args.cmd == "run" else queue(args)
    return {"start": start, "stop": stop, "status": status, "info": info}[args.cmd]()


if __name__ == "__main__":
    sys.exit(main())
