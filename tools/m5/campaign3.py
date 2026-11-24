#!/usr/bin/env python3
"""Stage 3 of the M5 kit: one night of army-net research, a deadline-bounded, value-ordered, resumable queue.

    campaign3.py run     [--config campaign3.toml] [--smoke]
    campaign3.py plan    [--smoke]      the queue with the estimated minutes and the time each job would end
    campaign3.py status  [--smoke]
    campaign3.py summary [--smoke]

The queue is the [[job]] list of campaign3.toml in its order (value order). Every finished job appends one line to
lab/runs/campaign3/results.jsonl; a job with an "ok" line is skipped after a crash or a reboot (a training resumes from
its checkpoint). The deadline (local HH:MM, fixed at the first start in deadline.json) minus report_min bounds every
job's estimated end: a job that does not fit is recorded "skipped_deadline" and the queue goes on. A job whose `after`
did not end ok is "skipped_dep". The file lab/runs/campaign3/STOP stops the queue between jobs. --smoke runs every
job kind once with the [smoke] budgets into lab/runs/campaign3-smoke/.
Every heavy command goes through the broker (M5_RES, default tools/res/res): one GPU job at a time.
"""
from __future__ import annotations

import argparse
import copy
import gzip
import json
import math
import os
import shutil
import statistics
import subprocess
import sys
import threading
import time
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
LAB = Path(os.environ.get("AURELIA_LAB") or REPO / "lab")
M5 = REPO / "tools" / "m5"
RES = os.environ.get("M5_RES") or str(REPO / "tools" / "res" / "res")
PY = os.environ.get("M5_PY") or str(REPO / ".venv-m5" / "bin" / "python")
EXAM = os.environ.get("AURELIA_EXAM_BIN") or str(REPO / "target" / "release" / "ai-exam")
NN = REPO / "nn"
GATE = REPO / "tools" / "gatestats" / "gate.py"
SEED = REPO / "lab-seed"
R = LAB / "runs"
D = LAB / "data"


def now() -> str:
    return time.strftime("%Y-%m-%d %H:%M:%S")


def log(msg: str) -> None:
    print(f"[campaign3] {now()} {msg}", flush=True)


def read_jsonl(p: Path) -> list[dict]:
    out = []
    if p.exists():
        for line in p.read_text().splitlines():
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return out


def perf_cores() -> int:
    try:
        return int(subprocess.run(["sysctl", "-n", "hw.perflevel0.physicalcpu"], capture_output=True, text=True).stdout.strip())
    except ValueError:
        return 4


class Campaign:
    def __init__(self, cfg: dict, smoke: bool):
        self.cfg, self.smoke = cfg, smoke
        self.c = dict(cfg["campaign"])
        self.data = dict(cfg["data"])
        self.imit = dict(cfg.get("imit", {}))
        self.rlc = dict(cfg.get("rl", {}))
        if smoke:
            s = cfg.get("smoke", {})
            for k in ("cpu_max", "mlx_gb"):
                if k in s:
                    self.c[k] = s[k]
            for k in ("train_to", "hold_to", "twin_families", "quick_families", "full_families", "teach_families"):
                if k in s:
                    self.data[k] = s[k]
            self.imit.update(s.get("imit", {}))
            self.rlc.update(s.get("rl", {}))
        self.dir = R / ("campaign3-smoke" if smoke else "campaign3")
        self.results_path = self.dir / "results.jsonl"
        self.jobs = self.load_jobs()
        self.current, self.state, self.t0 = "-", "starting", time.time()
        self.cpu_max = int(self.c.get("cpu_max") or 0) or perf_cores()

    # ---- the job list ----------------------------------------------------------------------------------------
    def load_jobs(self) -> list[dict]:
        jobs = [dict(j) for j in self.cfg["job"]]
        names = [j["name"] for j in jobs]
        assert len(names) == len(set(names)), "job names must be unique"
        if self.smoke:
            ov = self.cfg.get("smoke", {})
            for j in jobs:
                if j["kind"] == "needle":
                    j.update(ov.get("needle", {}))
        return jobs

    def job(self, name: str) -> dict | None:
        return next((j for j in self.jobs if j["name"] == name), None)

    def results(self) -> list[dict]:
        return read_jsonl(self.results_path)

    def ok(self) -> dict[str, dict]:
        return {r["job"]: r for r in self.results() if r.get("status") == "ok"}

    def append(self, row: dict) -> None:
        row["time"] = now()
        self.dir.mkdir(parents=True, exist_ok=True)
        with open(self.results_path, "a") as f:
            f.write(json.dumps(row) + "\n")
            f.flush()
            os.fsync(f.fileno())

    def pick(self) -> dict:
        p = self.dir / "pick.json"
        return json.loads(p.read_text()) if p.exists() else {}

    def resolve(self, name: str) -> str | None:
        """'@best1' -> the arm pick-P2 chose; a plain name -> itself."""
        if name and name.startswith("@"):
            return self.pick().get(name[1:])
        return name

    def arm_settings(self, j: dict) -> dict:
        """An imitation job's settings, with `like` (another arm, or @bestN) copied under its own."""
        if "like" not in j:
            return j
        src = self.resolve(j["like"])
        base = self.job(src) if src else None
        if base is None:
            raise RuntimeError(f"{j['name']}: like = {j['like']} is not resolved yet")
        merged = {k: v for k, v in base.items() if k not in ("name", "after", "est_min", "group", "baseline")}
        merged.update({k: v for k, v in j.items() if k != "like"})
        merged["like_resolved"] = src
        return merged

    # ---- estimates and the deadline -----------------------------------------------------------------------------
    def est_min(self, j: dict) -> float:
        if self.smoke:
            return 1.0
        e = float(j.get("est_min", 5))
        done = [r for r in self.ok().values() if r.get("kind") == j["kind"] and r.get("est_min") and r.get("minutes") is not None]
        if done:
            ratio = statistics.median(max(r["minutes"], 0.1) / max(r["est_min"], 0.1) for r in done)
            e *= min(3.0, max(0.5, ratio))
        return e

    def deadline(self, write: bool = True) -> float:
        p = self.dir / "deadline.json"
        if p.exists():
            return float(json.loads(p.read_text())["epoch"])
        if self.smoke:
            d = time.time() + 60 * float(self.cfg.get("smoke", {}).get("deadline_min", 120))
        else:
            hh, mm = (int(x) for x in (os.environ.get("M5_DEADLINE") or self.c.get("deadline", "08:00")).split(":"))
            t = time.localtime()
            d = time.mktime((t.tm_year, t.tm_mon, t.tm_mday, hh, mm, 0, 0, 0, -1))
            if d <= time.time():
                d += 86400
        if write:
            self.dir.mkdir(parents=True, exist_ok=True)
            p.write_text(json.dumps({"epoch": d, "local": time.strftime("%Y-%m-%d %H:%M", time.localtime(d))}))
        return d

    def margin_s(self) -> float:
        return 60 * float(self.c.get("report_min", 45)) * (0.05 if self.smoke else 1.0)

    MODULES = {"train": "aurelia_nn.army.train_m", "play": "aurelia_nn.tacsim.armyplay", "rl": "aurelia_nn.tacsim.armyrl", "slim": "aurelia_nn.tacsim.armyslim"}

    def mod(self, key: str) -> str:
        """The Python module of a step ([modules] in the toml overrides the default)."""
        return self.cfg.get("modules", {}).get(key) or self.MODULES[key]

    # ---- running commands ---------------------------------------------------------------------------------------
    def env(self, extra: dict | None = None) -> dict:
        e = dict(os.environ, AURELIA_LAB=str(LAB), AURELIA_EXAM_BIN=EXAM, ARMY_SCENE_RULE_VERSION=str(self.c.get("scene_rule_version", "1")),
                 PYTHONPATH=str(NN))
        e.update({k: str(v) for k, v in (extra or {}).items()})
        return e

    def mlx_gb(self) -> float:
        if self.c.get("mlx_gb"):
            return float(self.c["mlx_gb"])
        try:
            out = subprocess.run([RES, "info"], capture_output=True, text=True).stdout
            return float(json.loads(out)["mlx_gb"])
        except Exception:  # noqa: BLE001 (the machine broker has no `info`)
            return 12.0

    def res(self, name: str, cmd: list, cpu: str = "1", gpu: bool = False, ram: float = 0, env: dict | None = None,
            cwd: Path | None = None, logname: str | None = None) -> int:
        """One command through the broker; stdout and stderr to lab/logs/<logname>.log (appended)."""
        args = [RES, "run", "--cpu", cpu, "--disk", "25", "--name", f"c3-{name}"]
        if gpu:
            args.append("--gpu")
        if ram:
            args += ["--ram", str(ram)]
        args += ["--", *[str(a) for a in cmd]]
        lp = LAB / "logs" / f"{logname or name}.log"
        lp.parent.mkdir(parents=True, exist_ok=True)
        with open(lp, "a") as fh:
            fh.write(f"== {now()} {' '.join(str(a) for a in cmd)}\n")
            fh.flush()
            rc = subprocess.run(args, env=self.env(env), cwd=str(cwd or LAB), stdout=fh, stderr=subprocess.STDOUT).returncode
            fh.write(f"== {now()} rc {rc}\n")
        return rc

    def cpu_spec(self, lo: int = 2) -> str:
        return f"{min(lo, self.cpu_max)}-{self.cpu_max}"

    def suite(self, name: str, out: str, frm: int, to: int, players: str, families: str | None = None, env: dict | None = None,
              record: str | None = None) -> int:
        """ai-exam suite on the kept set, threads = the broker's grant."""
        fam = f" --families {families}" if families else ""
        rec = f" --record {record}" if record else ""
        cmd = ["sh", "-c", f"\"{EXAM}\" suite --kept {self.data['kept']} --from {frm} --instances {to}{fam} --players {players} --out {out}{rec} --jobs $RES_THREADS"]
        return self.res(name, cmd, cpu=self.cpu_spec(), env=env)

    def py(self, name: str, args: list, gpu: bool = False, cpu: str = "1", ram: float = 0, env: dict | None = None) -> int:
        if gpu and self.smoke and (self.cfg.get("smoke", {}).get("mlx_cpu") or os.environ.get("M5_SMOKE_MLX_CPU")):
            # the smoke on a machine whose GPU other lanes hold: MLX on the CPU, no GPU lock (oncpu.py)
            args = [a for a in args if a != "-I"]
            return self.res(name, [PY, str(M5 / "py" / "oncpu.py"), *args], cpu=cpu, gpu=False, ram=ram, env=env, cwd=NN)
        return self.res(name, [PY, *args], cpu=cpu, gpu=gpu, ram=ram, env=env, cwd=NN)

    # ---- metrics helpers ---------------------------------------------------------------------------------------
    @staticmethod
    def family_wins(rows_path: Path, player: str | None = None) -> dict:
        per: dict = {}
        for r in read_jsonl(rows_path):
            if player and r.get("player") != player:
                continue
            f = r["family"]
            if f == "prey" and (r.get("info") or {}).get("context") == "land":
                f = "prey-land"
            w, n = per.get(f, (0, 0))
            per[f] = (w + bool(r["win"]), n + 1)
        return {f: [w, n] for f, (w, n) in sorted(per.items())}

    @staticmethod
    def mean_rate(fw: dict) -> float | None:
        rates = [w / n for w, n in fw.values() if n]
        return round(sum(rates) / len(rates), 4) if rates else None

    def gatestats(self, out: Path, slices: list[dict], spec: dict, a_label: str, b_label: str) -> dict:
        out.mkdir(parents=True, exist_ok=True)
        inp = {"A_label": a_label, "B_label": b_label, "slices": slices}
        (out / "inputs.json").write_text(json.dumps(inp, indent=1))
        (out / "spec.json").write_text(json.dumps(spec, indent=1))
        rc = subprocess.run([sys.executable, "-I", str(GATE), "report", "--inputs", str(out / "inputs.json"), "--spec", str(out / "spec.json"), "--out", str(out)],
                            capture_output=True, text=True)
        (out / "gate.log").write_text(rc.stdout + rc.stderr)
        rep = out / "report.json"
        if rc.returncode not in (0, 1) or not rep.exists():
            return {"gate_error": (rc.stderr or rc.stdout)[-800:]}
        j = json.loads(rep.read_text())
        fams = j.get("families", {})
        st: dict = {}
        for f, v in fams.items():
            st.setdefault(v.get("status", "?"), []).append(f)
        return {"pass": j.get("pass"), "status": {k: sorted(v) for k, v in st.items()}, "summary": j.get("summary"),
                "report": str(out.relative_to(LAB))}

    # ---- job kinds ---------------------------------------------------------------------------------------------
    def execute(self, j: dict) -> dict:
        k = j["kind"]
        fn = getattr(self, f"job_{k}", None)
        if fn is None:
            raise ValueError(f"unknown job kind {k}")
        return fn(j)

    def job_setup(self, j: dict) -> dict:
        info = {"git_head": subprocess.run(["git", "-C", str(REPO), "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()}
        if os.environ.get("M5_SKIP_SETUP"):   # a machine with its own venv and broker (M5_PY, M5_RES): build only if missing
            if not Path(EXAM).exists():
                rc = self.res("setup-build", ["cargo", "build", "--release", "-p", "aurelia-qa", "--bin", "ai-exam"], cpu=self.cpu_spec(), cwd=REPO)
                if rc:
                    return {"status": "failed", "why": "cargo build failed (lab/logs/setup-build.log)"}
        else:
            rc = subprocess.run([str(M5 / "setup.sh")], cwd=str(REPO), env=self.env()).returncode
            if rc:
                return {"status": "failed", "why": "tools/m5/setup.sh failed"}
        if not Path(EXAM).exists():
            return {"status": "failed", "why": f"no ai-exam binary"}
        v = subprocess.run([PY, "-I", "-c", "import mlx.core as mx, importlib.metadata as m; print(m.version('mlx'), mx.metal.is_available())"],
                           capture_output=True, text=True)
        if v.returncode:
            return {"status": "failed", "why": "the MLX venv does not import: " + v.stderr[-300:]}
        sh = lambda *c: subprocess.run(c, capture_output=True, text=True).stdout.strip()
        info.update({"mlx": v.stdout.strip(), "chip": sh("sysctl", "-n", "machdep.cpu.brand_string"), "macos": sh("sw_vers", "-productVersion"),
                     "cores_perf": perf_cores(), "ram_gb": round(int(sh("sysctl", "-n", "hw.memsize") or 0) / 2 ** 30), "scene_rule_version": self.c.get("scene_rule_version")})
        self.dir.mkdir(parents=True, exist_ok=True)
        (self.dir / "versions.json").write_text(json.dumps(info, indent=1))
        return {"status": "ok", **info}

    def job_seed(self, j: dict) -> dict:
        D.mkdir(parents=True, exist_ok=True)
        (D / "ref").mkdir(exist_ok=True)
        kept = LAB / self.data["kept"]
        with gzip.open(SEED / "kept-r.jsonl.gz", "rb") as fi, open(kept, "wb") as fo:
            shutil.copyfileobj(fi, fo)
        for p in (SEED / "ref").glob("*.rows.jsonl.gz"):
            with gzip.open(p, "rb") as fi, open(D / "ref" / p.name[:-3], "wb") as fo:
                shutil.copyfileobj(fi, fo)
        for n in j.get("nets", []):
            d = R / f"net-{n}"
            d.mkdir(parents=True, exist_ok=True)
            shutil.copy(SEED / "nets" / f"{n}.safetensors", d / "weights.safetensors")
            if not (d / "army-net.bin").exists():
                if self.py(f"export-{n}", [str(M5 / "py" / "export_net.py"), d / "weights.safetensors", d / "army-net.bin"]):
                    return {"status": "failed", "why": f"export of {n} failed"}
        return {"status": "ok", "nets": j.get("nets", []), "kept_lines": sum(1 for _ in open(kept))}

    def job_teach(self, j: dict) -> dict:
        dd = self.data
        out = {}
        for split, frm, to in (("train", dd["train_from"], dd["train_to"]), ("hold", dd["hold_from"], dd["hold_to"])):
            name = f"teach-r-{split}"
            if (R / f"{name}.done").exists():
                continue
            shutil.rmtree(R / name, ignore_errors=True)
            (R / f"{name}.bin").unlink(missing_ok=True)
            if self.suite(name, f"runs/{name}", frm, to, dd["teach_players"], dd.get("teach_families"), record=f"runs/{name}.bin"):
                return {"status": "failed", "why": f"{name} failed (lab/logs/{name}.log)"}
            (R / f"{name}.done").touch()
        for split in ("train", "hold"):
            out[f"{split}_gb"] = round((R / f"teach-r-{split}.bin").stat().st_size / 1e9, 2)
            out[f"{split}_plays"] = len(read_jsonl(R / f"teach-r-{split}" / "rows.jsonl"))
        return {"status": "ok", **out}

    def job_dump(self, j: dict) -> dict:
        n, dd = j["net"], self.data
        net = R / f"net-{n}" / "army-net.bin"
        res = {}
        for split, frm, to, ev in (("tr", dd["train_from"], dd["train_to"], False), ("ho", dd["hold_from"], dd["hold_to"], True)):
            slim = D / f"{split}-{n}-r.jsonl"
            if slim.exists() and slim.stat().st_size > 0:
                continue
            full = D / f"{split}-{n}-r-full.jsonl"
            full.unlink(missing_ok=True)
            env = {"ARMY_NET": net, "ARMY_OBS_DUMP": full}
            if ev:
                env["ARMY_OBS_EVENTS"] = "1"
            if self.suite(f"dump-{split}-{n}", f"runs/{split}-{n}-rows", frm, to, "army-net", dd["twin_families"], env=env):
                return {"status": "failed", "why": f"the Rust dump {split} failed"}
            if self.py(f"slim-{split}-{n}", ["-m", self.mod("slim"), full, slim]):
                return {"status": "failed", "why": f"armyslim {split} failed"}
            full.unlink(missing_ok=True)
        for split in ("tr", "ho"):
            res[f"{split}_mb"] = round((D / f"{split}-{n}-r.jsonl").stat().st_size / 1e6, 1)
            res[f"{split}_rust_win"] = self.family_wins(R / f"{split}-{n}-rows" / "rows.jsonl")
        return {"status": "ok", **res}

    def rust_rows(self, tag: str, netbin: Path) -> tuple[Path, Path] | str:
        """Quick (play-on, the quick families) and full held-out runs of one net -> (quick rows, full rows)."""
        dd = self.data
        q, f = R / f"evq-{tag}", R / f"evf-{tag}"
        for d, env, fams in ((q, {"ARMY_PLAY_ON_CAP": "1"}, dd["quick_families"]), (f, {}, dd.get("full_families"))):
            if (d / "rows.jsonl").exists() and (d / ".done").exists():
                continue
            shutil.rmtree(d, ignore_errors=True)
            if self.suite(d.name, f"runs/{d.name}", dd["hold_from"], dd["hold_to"], "army-net", fams, env={**env, "ARMY_NET": netbin}):
                return f"Rust run {d.name} failed"
            (d / ".done").touch()
        return q / "rows.jsonl", f / "rows.jsonl"

    def rust_gate(self, out: Path, a: tuple, b: tuple, a_label: str, b_label: str, spec_extra: dict | None = None, sanity: bool = False) -> dict:
        teachers = [str(R / "teach-r-hold" / "rows.jsonl")] if (R / "teach-r-hold" / "rows.jsonl").exists() else []
        w = [self.data["hold_from"], self.data["hold_to"]]
        slices = [{"name": "quick", "window": w, "A": [str(x) for x in a[0]], "B": [str(x) for x in b[0]]},
                  {"name": "full", "window": w, "A": [str(x) for x in a[1]], "B": [str(x) for x in b[1]], "teachers": teachers}]
        spec = {"name": f"{b_label} vs {a_label}", "alpha": 0.05, "min_n": 5, "bootstrap_reps": 2000, "harm": {"tolerance_pts": 5},
                "mean": {"slices": ["full"], "min_gain_pts": 0}}
        if sanity:   # the same net on two machines: only the per-family paired tests, no gate blocks
            spec.pop("mean")
        elif teachers:
            spec["count_at_bar"] = {"slices": ["full"], "frac": 0.9, "min_gain": 0}
        spec.update(spec_extra or {})
        return self.gatestats(out, slices, spec, a_label, b_label)

    def job_rust_eval(self, j: dict) -> dict:
        n = j["net"]
        rows = self.rust_rows(f"net-{n}", R / f"net-{n}" / "army-net.bin")
        if isinstance(rows, str):
            return {"status": "failed", "why": rows}
        out = {"quick": self.family_wins(rows[0]), "full": self.family_wins(rows[1])}
        out["quick_mean"], out["full_mean"] = self.mean_rate(out["quick"]), self.mean_rate(out["full"])
        ref = j.get("ref")
        if ref:
            ra, rb = D / "ref" / f"q-{ref}.rows.jsonl", D / "ref" / f"f-{ref}.rows.jsonl"
            g = self.rust_gate(self.dir / f"gate-{j['name']}", ((ra,), (rb,)), ((rows[0],), (rows[1],)), f"{ref} (M4)", f"{n} (this machine)", sanity=True)
            changed = sum(len(v) for s, v in (g.get("status") or {}).items() if s in ("up", "down", "DROP"))
            out.update({"vs_m4": g, "families_changed_beyond_noise": changed, "matches_m4": changed == 0 and "gate_error" not in g})
        return {"status": "ok", **out}

    def twin_play(self, tag: str, weights: Path) -> dict:
        """The net's own play in the MLX twin on MBf6c's held-out dump; rows for gatestats at runs/twin-<tag>/rows.jsonl."""
        dump = D / "ho-MBf6c-r.jsonl"
        if not dump.exists():
            return {"twin_error": "no held-out dump"}
        outj = R / f"twin-{tag}" / "play.json"
        outj.parent.mkdir(parents=True, exist_ok=True)
        args = ["-m", self.mod("play"), dump, "--weights", weights, "--out", outj]
        if self.smoke:
            args += ["--limit", "12"]
        if self.py(f"twin-{tag}", args, gpu=True, ram=8):
            return {"twin_error": f"armyplay failed (lab/logs/twin-{tag}.log)"}
        d = json.loads(outj.read_text())
        with open(outj.parent / "rows.jsonl", "w") as fh:
            for play, (mw, _rw) in sorted(d.get("plays", {}).items()):
                fam, era, inst, att = play.split("/")
                fh.write(json.dumps({"family": fam, "era": era, "instance": int(inst), "attempt": int(att), "player": "army-net", "win": bool(mw)}) + "\n")
        fams = {f: v["mlx_win"] for f, v in d.get("families", {}).items()}
        rates = list(fams.values())
        return {"twin_win": fams, "twin_mean": round(sum(rates) / len(rates), 2) if rates else None, "twin_plays": len(d.get("plays", {})),
                "twin_same_as_rust": round(sum(v["same_verdict"] for v in d["families"].values()) / max(1, sum(v["plays"] for v in d["families"].values())), 4),
                "twin_seconds": d.get("seconds")}

    def job_twin(self, j: dict) -> dict:
        t = self.twin_play(j["name"], R / f"net-{j['net']}" / "weights.safetensors")
        return {"status": "failed" if "twin_error" in t else "ok", **t}

    def job_needle(self, j: dict) -> dict:
        out = self.dir / "needle"
        args = ["-I", str(M5 / "needle" / "needle.py"), "run", "--out", out, "--lengths", j["lengths"], "--widths", j["widths"], "--layers", j["layers"],
                "--needles", j["needles"], "--steps", j["steps"], "--eval-every", j["eval_every"], "--seed", j.get("seed", 1), "--max-minutes", j["max_minutes"]]
        if self.py("needle", args, gpu=True, ram=8):
            return {"status": "failed", "why": "needle.py run failed (lab/logs/needle.log)"}
        subprocess.run([PY, "-I", str(M5 / "needle" / "needle.py"), "summary", "--out", str(out)], capture_output=True)
        rows = read_jsonl(out / "results.jsonl")
        return {"status": "ok", "nets_run": len(rows), "summary_md": str((out / "summary.md").relative_to(LAB))}

    # imitation arms
    def train_imit(self, name: str, s: dict) -> int:
        im = self.imit
        out = R / name
        gb = self.mlx_gb()
        base = ["-m", self.mod("train"), "--train", R / "teach-r-train.bin", "--hold", R / "teach-r-hold.bin", "--out", out,
                "--map", "--map-ch", s.get("map_ch", im["map_ch"]), "--d", s.get("d", im["d"]), "--layers", s.get("layers", 3), "--heads", s.get("heads", im["heads"]),
                "--seed", s.get("seed", 1), "--epochs", s.get("epochs", im["epochs"]), "--plays", s.get("plays", im["plays"]), "--lr", s.get("lr", im["lr"]),
                "--curriculum", s.get("curriculum", im["curriculum"]), "--up", s.get("up", im["up"]), "--up-factor", s.get("up_factor", im["up_factor"]),
                "--eval-every", im["eval_every"], "--log-every", im["log_every"], "--eval-plays", im["eval_plays"], "--arch", s.get("arch", "base"),
                "--mem-gb", gb, *s.get("extra", [])]
        if im.get("max_steps"):
            base += ["--max-steps", im["max_steps"]]
        chunk = float(self.c.get("chunk_min", 30))
        for _ in range(60):
            st = out / "state.json"
            resume = []
            if st.exists():
                s_ = json.loads(st.read_text())
                if s_["step"] >= s_["steps"]:
                    return 0
                resume = ["--resume"]
            if self.py(f"train-{name}", [*base, *resume, "--max-minutes", chunk], gpu=True, cpu="2", ram=gb):
                return 1
            if im.get("max_steps"):
                return 0
        return 1

    def train_metrics(self, name: str) -> dict:
        run = R / name
        out: dict = {}
        rows = [r for r in read_jsonl(run / "curves.jsonl")]
        if rows:
            out["dps"] = round(rows[-1].get("dps_mean", 0))
            out["steps"] = rows[-1].get("step")
            hold = [r["hold"] for r in rows if "hold" in r]
            if hold:
                out["hold_acc_both"] = round(hold[-1].get("acc_both", 0), 4)
                out["hold_acc_best"] = round(max(h.get("acc_both", 0) for h in hold), 4)
        cfgp = run / "config.json"
        if cfgp.exists():
            c = json.loads(cfgp.read_text())
            out["params"] = c.get("parameters")
            out["steps_planned"] = c.get("steps")
        ev = read_jsonl(run / "evals.jsonl")
        if ev:
            last = ev[-1]
            for k in ("acc_both", "acc_unit", "acc_action", "loss"):
                if k in last:
                    out[f"final_{k}"] = last[k]
        return out

    def job_imit(self, j: dict) -> dict:
        s = self.arm_settings(j)
        name = j["name"]
        if self.train_imit(name, s):
            return {"status": "failed", "why": f"training failed (lab/logs/train-{name}.log)", **self.train_metrics(name)}
        m = self.train_metrics(name)
        w = R / name / "weights.safetensors"
        if not w.exists():
            return {"status": "failed", "why": "no weights.safetensors", **m}
        t = self.twin_play(name, w)
        res = {"status": "ok" if "twin_error" not in t else "failed", "arch": s.get("arch"), "layers": s.get("layers"), "seed": s.get("seed"),
               "extra": s.get("extra", []), "like": s.get("like_resolved"), **m, **t}
        if "twin_error" in t:
            res["why"] = t["twin_error"]
        return res

    def job_gate(self, j: dict) -> dict:
        ok = self.ok()
        base = j["base"]
        seeds = int(j.get("seeds", 1))
        w = [self.data["hold_from"], self.data["hold_to"]]

        def runs_of(arm: str) -> list[str] | None:
            names = [arm]
            if seeds > 1:
                for jj in self.jobs:
                    if jj["kind"] == "imit" and int(jj.get("seed", 1)) > 1 and (self.resolve(jj.get("like", "")) == arm):
                        names.append(jj["name"])
            names = names[:seeds]
            if any(n not in ok for n in names):
                return None
            return [str(R / f"twin-{n}" / "rows.jsonl") for n in names]

        a = runs_of(base)
        if not a:
            return {"status": "failed", "why": f"the baseline {base} has no result"}
        out = {}
        for arm in j["arms"]:
            arm_r = self.resolve(arm)
            b = runs_of(arm_r) if arm_r else None
            if not b:
                out[arm_r or arm] = {"missing": True}
                continue
            spec = {"name": f"{arm_r} vs {base}", "alpha": 0.05, "min_n": 5, "bootstrap_reps": 2000, "harm": {"tolerance_pts": 5}, "mean": {"slices": ["twin"], "min_gain_pts": 0}}
            g = self.gatestats(self.dir / f"gate-{j['name']}-{arm_r}", [{"name": "twin", "window": w, "A": a, "B": b}], spec, base, arm_r)
            g["hold_acc_both"] = ok.get(arm_r, {}).get("hold_acc_both")
            g["hold_acc_base"] = ok.get(base, {}).get("hold_acc_both")
            out[arm_r] = g
        return {"status": "ok", "base": base, "seeds": seeds, "arms": out}

    def job_pick(self, j: dict) -> dict:
        ok = self.ok()
        cand = [(ok[a].get("twin_mean") or 0, ok[a].get("hold_acc_both") or 0, a) for a in j["arms"] if a in ok]
        if not cand:
            return {"status": "failed", "why": "no P2 arm finished"}
        cand.sort(reverse=True)
        top = [c[2] for c in cand[: int(j.get("top", 2))]]
        pick = {f"best{i + 1}": a for i, a in enumerate(top)}
        self.dir.mkdir(parents=True, exist_ok=True)
        (self.dir / "pick.json").write_text(json.dumps(pick, indent=1))
        return {"status": "ok", "pick": pick, "ranking": [[c[2], c[0], c[1]] for c in cand]}

    # RL
    @staticmethod
    def two_prop_noise(p1: float, n1: int, p2: float, n2: int) -> float:
        p = (p1 * n1 + p2 * n2) / max(n1 + n2, 1)
        return 1.96 * math.sqrt(max(p * (1 - p), 0.0) * (1 / max(n1, 1) + 1 / max(n2, 1)))

    def rl_select(self, curve: dict) -> dict:
        """rl2's rule: the checkpoint with the highest validation family mean among those whose every validation family is
        not below the start (update 0) beyond the two-proportion 95 % noise; none qualifies: the start."""
        val = curve.get("val") or []
        if not val:
            return {"update": None, "why": "no validation curve"}
        start = next((v for v in val if v["update"] == 0), val[0])
        best = None
        for v in val:
            if v["update"] == 0:
                continue
            below = []
            for f, (w, n) in v["lines"].items():
                w0, n0 = start["lines"].get(f, (0, 0))
                if n and n0 and (w0 / n0 - w / n) > self.two_prop_noise(w0 / n0, n0, w / n, n):
                    below.append(f)
            if not below and (best is None or v["family_mean"] > best["family_mean"]):
                best = v
        if best is None:
            return {"update": 0, "why": "every checkpoint has a validation family below the start: the start is kept", "start_mean": start["family_mean"]}
        return {"update": best["update"], "val_mean": best["family_mean"], "start_mean": start["family_mean"]}

    def job_rl(self, j: dict) -> dict:
        rc_ = self.rlc
        name = j["name"]
        out = R / name
        curve = out / "curve.json"
        if not (out / ".done").exists():
            shutil.rmtree(out, ignore_errors=True)   # armyrl does not resume: a cut run starts again
            gb = self.mlx_gb()
            args = ["-m", self.mod("rl"), "--train", D / "tr-MBf6c-r.jsonl", "--held", D / "ho-MBf6c-r.jsonl",
                    "--weights", R / "net-MBf6c" / "weights.safetensors", "--anchor", R / "teach-r-train.bin", "--bars", R / "teach-r-train" / "rows.jsonl",
                    "--out", out, "--K", rc_["K"], "--anchor-w", rc_["anchor_w"], "--anchor-auto", rc_["anchor_auto"], "--updates", rc_["updates"],
                    "--minutes", rc_["minutes"], "--ckpt-every", rc_["ckpt_every"], "--seed", j["seed"], *rc_.get("extra", [])]
            for k in ("P", "G", "split", "min_per_family"):
                if k in rc_:
                    args += [f"--{k.replace('_', '-')}", rc_[k]]
            if self.py(f"rl-{name}", args, gpu=True, cpu="2", ram=max(gb, 16) if not self.smoke else gb):
                return {"status": "failed", "why": f"armyrl failed (lab/logs/rl-{name}.log)"}
            (out / ".done").touch()
        c = json.loads(curve.read_text())
        sel = self.rl_select(c)
        (out / "selected.json").write_text(json.dumps(sel, indent=1))
        held = {v["update"]: v["family_mean"] for v in c.get("held", [])}
        vals = {v["update"]: v["family_mean"] for v in c.get("val", [])}
        return {"status": "ok", "seed": j["seed"], "selected": sel, "lambda": c.get("lambda"), "done": c.get("done"), "val_curve": vals, "held_curve": held}

    def job_rl_rust(self, j: dict) -> dict:
        rl = j["rl"]
        sel = json.loads((R / rl / "selected.json").read_text())
        u = sel.get("update")
        if u is None:
            return {"status": "failed", "why": "no selected checkpoint"}
        w = R / rl / f"ckpt-{u:04d}.safetensors" if u else R / "net-MBf6c" / "weights.safetensors"
        bin_ = R / rl / f"sel-u{u:04d}.bin"
        if not bin_.exists() and self.py(f"export-{rl}", [str(M5 / "py" / "export_net.py"), w, bin_]):
            return {"status": "failed", "why": "export failed"}
        rows = self.rust_rows(f"{rl}-u{u:04d}", bin_)
        if isinstance(rows, str):
            return {"status": "failed", "why": rows}
        base = (R / "evq-net-MBf6c" / "rows.jsonl", R / "evf-net-MBf6c" / "rows.jsonl")
        g = self.rust_gate(self.dir / f"gate-{rl}-vs-MBf6c", ((base[0],), (base[1],)), ((rows[0],), (rows[1],)), "MBf6c", f"{rl} u{u}",
                           {"required_up": {"families": ["quick/planned-assault", "quick/assault-heal"], "min_gain_pts": 0}})
        u175 = (R / "evq-net-rl2-u175" / "rows.jsonl", R / "evf-net-rl2-u175" / "rows.jsonl")
        g2 = self.rust_gate(self.dir / f"gate-{rl}-vs-u175", ((u175[0],), (u175[1],)), ((rows[0],), (rows[1],)), "rl2 u175 (seed 1)", f"{rl} u{u}") \
            if u175[0].exists() else {"missing": True}
        return {"status": "ok", "update": u, "quick": self.family_wins(rows[0]), "full": self.family_wins(rows[1]),
                "full_mean": self.mean_rate(self.family_wins(rows[1])), "vs_MBf6c": g, "vs_u175": g2}

    # ---- summary -----------------------------------------------------------------------------------------------
    def job_summary(self, j: dict | None = None) -> dict:
        res = self.results()
        ok = {r["job"]: r for r in res if r.get("status") == "ok"}
        last = {}
        for r in res:
            last[r["job"]] = r
        L = [f"# Stage 3 summary{' (SMOKE)' if self.smoke else ''} (generated by tools/m5/campaign3.py)", f"Generated {now()}.", ""]
        v = ok.get("setup", {})
        if v:
            L += [f"Machine: {v.get('chip')}, {v.get('cores_perf')} performance cores, {v.get('ram_gb')} GB, macOS {v.get('macos')}; MLX {v.get('mlx')}; "
                  f"kit commit {v.get('git_head')}; scene rule version {v.get('scene_rule_version')}.", ""]
        # P0
        L += ["## P0: setup and sanity", "", "| job | status | minutes | note |", "|---|---|---:|---|"]
        for jj in self.jobs:
            if jj.get("group") != "P0":
                continue
            r = last.get(jj["name"], {})
            note = ""
            if jj["kind"] == "rust_eval" and r.get("status") == "ok":
                note = (f"quick mean {r.get('quick_mean')}, full mean {r.get('full_mean')}; vs the M4's rows: "
                        f"{'MATCH within noise' if r.get('matches_m4') else str(r.get('families_changed_beyond_noise')) + ' families differ beyond noise'}"
                        f" {json.dumps((r.get('vs_m4') or {}).get('status'))}")
            elif jj["kind"] == "teach" and r.get("status") == "ok":
                note = f"train {r.get('train_plays')} plays ({r.get('train_gb')} GB), hold {r.get('hold_plays')} plays"
            elif jj["kind"] == "twin" and r.get("status") == "ok":
                note = f"twin mean win {r.get('twin_mean')} % over {r.get('twin_plays')} plays, same verdict as Rust {r.get('twin_same_as_rust')}"
            elif r.get("why"):
                note = r["why"]
            L.append(f"| {jj['name']} | {r.get('status', 'not run')} | {r.get('minutes', '')} | {note} |")
        # P1
        nd = self.dir / "needle" / "summary.md"
        L += ["", "## P1: the needle test", ""]
        L += [nd.read_text()] if nd.exists() else [f"(no needle summary; status {last.get('needle', {}).get('status', 'not run')})"]
        # P2 / P3
        for grp, title in (("P2", "P2: architecture screening by imitation (seed 1)"), ("P3", "P3: seed 2")):
            arms = [jj for jj in self.jobs if jj.get("group") == grp and jj["kind"] == "imit"]
            if not arms:
                continue
            fams = sorted({f for jj in arms for f in (last.get(jj["name"], {}).get("twin_win") or {})})
            L += ["", f"## {title}", "", "| arm | arch | layers | seed | params | steps | hold acc (last / best) | twin mean win % | " + " | ".join(fams) + " | dec/s | min | status |",
                  "|---|---|---:|---:|---:|---:|---:|---:|" + "---:|" * len(fams) + "---:|---:|---|"]
            for jj in arms:
                r = last.get(jj["name"], {})
                tw = r.get("twin_win") or {}
                L.append(f"| {jj['name']}{' (' + r['like'] + ')' if r.get('like') else ''} | {r.get('arch', jj.get('arch', ''))} {' '.join(r.get('extra', []) or [])} | {r.get('layers', '')} | "
                         f"{r.get('seed', jj.get('seed', ''))} | {r.get('params', '')} | {r.get('steps', '')} | {r.get('hold_acc_both', '')} / {r.get('hold_acc_best', '')} | "
                         f"{r.get('twin_mean', '')} | " + " | ".join(str(tw.get(f, '')) for f in fams) + f" | {r.get('dps', '')} | {r.get('minutes', '')} | {r.get('status', 'not run')} |")
            for g in [jj for jj in self.jobs if jj.get("group") == grp and jj["kind"] in ("gate", "pick")]:
                r = last.get(g["name"], {})
                if g["kind"] == "pick" and r.get("pick"):
                    L += ["", f"Picked for seed 2: {r['pick']} (ranking: twin mean, hold acc: {r.get('ranking')})."]
                if g["kind"] == "gate" and r.get("arms"):
                    L += ["", f"Gate {g['name']} (gatestats, paired by scene, Holm over families; tolerance 5 points; base {r.get('base')}, seeds {r.get('seeds')}):", "",
                          "| arm | families up | down | DROP | mean delta (CI) | pass | hold acc arm / base |", "|---|---|---|---|---|---|---|"]
                    for arm, gg in r["arms"].items():
                        if gg.get("missing"):
                            L.append(f"| {arm} | (no result) | | | | | |")
                            continue
                        st = gg.get("status") or {}
                        mean = (gg.get("summary") or {}).get("mean") if isinstance(gg.get("summary"), dict) else None
                        L.append(f"| {arm} | {', '.join(st.get('up', []))} | {', '.join(st.get('down', []))} | {', '.join(st.get('DROP', []))} | "
                                 f"{json.dumps(mean) if mean is not None else gg.get('gate_error', '')} | {gg.get('pass')} | {gg.get('hold_acc_both')} / {gg.get('hold_acc_base')} |")
        # P4
        rls = [jj for jj in self.jobs if jj.get("group") == "P4"]
        if rls:
            L += ["", "## P4: RL seed variance (rl2 recipe from MBf6c)", "", "| run | status | minutes | lambda | updates | selected | val mean start -> sel | Rust full mean | vs MBf6c (Rust, paired) | vs u175 seed 1 |",
                  "|---|---|---:|---:|---:|---:|---|---:|---|---|"]
            for jj in rls:
                if jj["kind"] != "rl":
                    continue
                r, rr = last.get(jj["name"], {}), last.get(f"{jj['name']}-rust", {})
                sel = r.get("selected") or {}
                g1, g2 = rr.get("vs_MBf6c") or {}, rr.get("vs_u175") or {}
                L.append(f"| {jj['name']} | {r.get('status', 'not run')} / {rr.get('status', 'not run')} | {r.get('minutes', '')} + {rr.get('minutes', '')} | {r.get('lambda', '')} | "
                         f"{(r.get('done') or {}).get('updates', '')} | {sel.get('update', '')} | {sel.get('start_mean', '')} -> {sel.get('val_mean', '')} | {rr.get('full_mean', '')} | "
                         f"pass {g1.get('pass')} {json.dumps(g1.get('status'))} | {json.dumps(g2.get('status'))} |")
        # time, cuts, failures
        cut = [r for r in res if r.get("status") == "skipped_deadline"]
        dep = [r for r in res if r.get("status") == "skipped_dep"]
        bad = [r for r in res if r.get("status") == "failed_final"]
        L += ["", "## Time", "", "| job | estimated min | actual min |", "|---|---:|---:|"]
        for r in res:
            if r.get("status") == "ok" and r["job"] != "summary":
                L.append(f"| {r['job']} | {r.get('est_min', '')} | {r.get('minutes', '')} |")
        if cut:
            L += ["", f"## Not run: the deadline ({len(cut)} jobs)", ""] + [f"- {r['job']} (estimate {r.get('est_min')} min, {r.get('left_min')} min left)" for r in cut]
        if dep:
            L += ["", "## Not run: a job it needs did not finish", ""] + [f"- {r['job']}: {r.get('why')}" for r in dep]
        if bad:
            L += ["", "## Failed", ""] + [f"- {r['job']}: {r.get('why', '')}" for r in bad]
        self.dir.mkdir(parents=True, exist_ok=True)
        (self.dir / "summary.md").write_text("\n".join(L) + "\n")
        return {"status": "ok", "lines": len(L)}

    # ---- status and the loop -------------------------------------------------------------------------------------
    def write_status(self) -> None:
        res = self.results()
        st = {"state": self.state, "current": self.current, "done": sum(1 for r in res if r.get("status") == "ok"), "total": len(self.jobs),
              "failed": [r["job"] for r in res if r.get("status") == "failed_final"], "skipped": [r["job"] for r in res if str(r.get("status", "")).startswith("skipped")],
              "elapsed_h": round((time.time() - self.t0) / 3600, 2), "updated": now(), "smoke": self.smoke,
              "deadline": time.strftime("%Y-%m-%d %H:%M", time.localtime(self.deadline(write=False)))}
        self.dir.mkdir(parents=True, exist_ok=True)
        (self.dir / "status.json").write_text(json.dumps(st, indent=1))

    def heartbeat(self) -> None:
        while True:
            time.sleep(60)
            try:
                self.write_status()
            except OSError:
                pass

    def run(self) -> int:
        self.dir.mkdir(parents=True, exist_ok=True)
        threading.Thread(target=self.heartbeat, daemon=True).start()
        deadline = self.deadline()
        margin = self.margin_s()
        retries = int(self.c.get("retries", 1))
        log(f"{'SMOKE ' if self.smoke else ''}deadline {time.strftime('%Y-%m-%d %H:%M', time.localtime(deadline))}; no job starts that would end after "
            f"{time.strftime('%H:%M', time.localtime(deadline - margin))}; {len(self.jobs)} jobs; results {self.results_path}")
        final = {r["job"] for r in self.results() if r.get("status") in ("failed_final", "skipped_deadline", "skipped_dep")}
        for j in self.jobs:
            if (self.dir / "STOP").exists():
                log("STOP file found: the queue stops here (the summary is written)")
                break
            ok = self.ok()
            if j["name"] in ok or j["name"] in final:
                continue
            if j["kind"] != "summary":
                miss = [a for a in j.get("after", []) if a not in ok]
                if miss:
                    log(f"skip {j['name']}: needs {miss}")
                    self.append({"job": j["name"], "kind": j["kind"], "group": j.get("group"), "status": "skipped_dep", "why": f"needs {miss}"})
                    continue
                est = self.est_min(j)
                left = (deadline - margin - time.time()) / 60
                if est > left:
                    log(f"skip {j['name']}: about {est:.0f} min, {left:.0f} min left before the deadline")
                    self.append({"job": j["name"], "kind": j["kind"], "group": j.get("group"), "status": "skipped_deadline", "est_min": round(est), "left_min": round(left)})
                    continue
            else:
                est = 0
            self.current, self.state = j["name"], "running"
            self.write_status()
            log(f"start {j['name']} ({j['kind']}, about {est:.0f} min)")
            t0 = time.time()
            res: dict = {}
            for attempt in range(retries + 1):
                try:
                    res = self.execute(j)
                except Exception as e:  # noqa: BLE001 (a failing job must not end the campaign)
                    res = {"status": "failed", "why": repr(e)}
                if res.get("status") == "ok":
                    break
                log(f"{j['name']} failed (attempt {attempt + 1}): {res.get('why', '')}")
            if res.get("status") != "ok":
                res["status"] = "failed_final"
            self.append({"job": j["name"], "kind": j["kind"], "group": j.get("group"), "minutes": round((time.time() - t0) / 60, 1),
                         "est_min": j.get("est_min"), **res})
            log(f"end {j['name']}: {res['status']} in {(time.time() - t0) / 60:.1f} min")
        self.state, self.current = "finished", "-"
        self.job_summary()
        self.write_status()
        log(f"finished: {self.dir / 'summary.md'}")
        return 0

    def plan(self) -> None:
        dl = self.deadline(write=False) - self.margin_s()
        t, ok = time.time(), self.ok()
        log(f"{len(self.jobs)} jobs; deadline minus margin {time.strftime('%Y-%m-%d %H:%M', time.localtime(dl))}")
        for j in self.jobs:
            e = 0 if j["name"] in ok else self.est_min(j)
            mark = "done" if j["name"] in ok else ("" if t + 60 * e <= dl or j["kind"] == "summary" else "past the deadline: would be skipped")
            if mark != "past the deadline: would be skipped":
                t += 60 * e
            print(f"  {j['name']:16} {j['kind']:10} {e:6.0f} min  ends ~{time.strftime('%H:%M', time.localtime(t))}  {mark}")
        print(f"total of the jobs that fit: {(t - time.time()) / 3600:.1f} h")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["run", "plan", "status", "summary"])
    ap.add_argument("--config", default=str(M5 / "campaign3.toml"))
    ap.add_argument("--smoke", action="store_true")
    a = ap.parse_args()
    cfg = tomllib.loads(Path(a.config).read_text())
    if a.smoke and "smoke" not in LAB.name and not os.environ.get("M5_SMOKE_ANY_LAB"):
        print(f"campaign3: --smoke needs its own lab (AURELIA_LAB=.../lab-smoke), not {LAB}: use tools/m5/campaign3.sh smoke", file=sys.stderr)
        return 2
    c = Campaign(cfg, a.smoke)
    if a.cmd == "plan":
        c.plan()
        return 0
    if a.cmd == "status":
        p = c.dir / "status.json"
        print(p.read_text() if p.exists() else "no stage 3 yet")
        return 0
    if a.cmd == "summary":
        c.job_summary()
        print((c.dir / "summary.md").read_text())
        return 0
    return c.run()


if __name__ == "__main__":
    sys.exit(main())
