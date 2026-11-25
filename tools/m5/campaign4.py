#!/usr/bin/env python3
"""Night 4 of the M5 kit: long RL on the honest net input, a deadline-bounded, value-ordered, resumable queue.

    campaign4.py run     [--config campaign4.toml] [--smoke] [--only job,job]
    campaign4.py plan    [--smoke]      the queue with the estimated minutes and the time each job would end
    campaign4.py status  [--smoke]
    campaign4.py summary [--smoke]

The queue: the P0 jobs of campaign4.toml, then for every seed the arms in order (base-s1, boost-s1, base-s2, ...), each RL run
followed by its tables job, then the summary. Every finished job appends one line to lab/runs/campaign4/results.jsonl; a job
with an "ok" line is skipped after a crash or a reboot (an RL run resumes from its newest checkpoint). The deadline (fixed at
the first start in deadline.json) minus report_min bounds every job's estimated end: a job that does not fit is recorded
"skipped_deadline", the queue goes on. A failed `fatal` job (the P0 sanity) stops the queue. The file runs/campaign4/STOP stops the
queue between jobs. Every heavy command goes through the broker (M5_RES): one GPU job at a time.
"""
from __future__ import annotations

import argparse
import gzip
import json
import os
import shutil
import statistics
import subprocess
import sys
import time
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
import summary4  # noqa: E402

LAB = Path(os.environ.get("AURELIA_LAB") or REPO / "lab")
M5 = REPO / "tools" / "m5"
RES = os.environ.get("M5_RES") or str(REPO / "tools" / "res" / "res")
PY = os.environ.get("M5_PY") or str(REPO / ".venv-m5" / "bin" / "python")
EXAM = os.environ.get("AURELIA_EXAM_BIN") or str(REPO / "target" / "release" / "ai-exam")
NN = REPO / "nn"
GATE = REPO / "tools" / "gatestats" / "gate.py"
SPECS = REPO / "tools" / "gatestats" / "specs"
SEED = REPO / "lab-seed4"
D = LAB / "data"


def now() -> str:
    return time.strftime("%Y-%m-%d %H:%M:%S")


def log(msg: str) -> None:
    print(f"[campaign4] {now()} {msg}", flush=True)


def read_jsonl(p: Path) -> list[dict]:
    out = []
    if p.exists():
        for line in p.read_text().splitlines():
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return out


def gunzip(src: Path, dst: Path) -> None:
    with gzip.open(src, "rb") as fi, open(dst, "wb") as fo:
        shutil.copyfileobj(fi, fo)


class Campaign:
    def __init__(self, cfg: dict, smoke: bool, only: list[str] | None = None):
        self.cfg, self.smoke, self.only = cfg, smoke, only
        self.c = dict(cfg["campaign"])
        self.data = dict(cfg["data"])
        self.night = dict(cfg["night"])
        if smoke:
            s = dict(cfg["smoke"])
            if "deadline_hours" in s:
                self.c["deadline_hours"] = s.pop("deadline_hours")
            for k in list(s):
                if k in self.data:
                    self.data[k] = s.pop(k)
            self.smoke_fams = s.pop("families", "")
            self.night.update(s)
        else:
            self.smoke_fams = ""
        self.R = LAB / "runs"
        self.dir = self.R / ("campaign4-smoke" if smoke else "campaign4")
        self.results_path = self.dir / "results.jsonl"
        self.jobs = self.load_jobs()
        self.current, self.state, self.t0 = "-", "starting", time.time()

    # ---- the job list ----------------------------------------------------------------------------------------
    def load_jobs(self) -> list[dict]:
        jobs = [dict(j) for j in self.cfg["job"]]
        n = self.night
        for s in n["seeds"]:
            for arm in n["arms"]:
                run = f"{arm}-s{s}"
                prev = [j["name"] for j in jobs if j["kind"] == "tables"][-1:]   # the runs are in one line (one GPU): no `after` between them but the order
                jobs.append({"name": f"rl-{run}", "kind": "rl", "run": run, "arm": arm, "seed": s, "after": ["teach", "lengths"] + ([f"rl-base-s{s}"] if arm != "base" else []),
                             "est_min": n["est_rl_min"] if not self.smoke else 1})
                jobs.append({"name": f"tables-{run}", "kind": "tables", "run": run, "after": [f"rl-{run}"], "est_min": n["est_tables_min"] if not self.smoke else 1})
        jobs.append({"name": "summary", "kind": "summary", "est_min": 1})
        names = [j["name"] for j in jobs]
        assert len(names) == len(set(names)), "job names must be unique"
        if self.only:
            unknown = [x for x in self.only if x not in names]
            assert not unknown, f"--only names unknown jobs {unknown}"
            jobs = [j for j in jobs if j["name"] in self.only or j["kind"] == "summary"]
        return jobs

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

    # ---- estimates and the deadline -----------------------------------------------------------------------------
    def est_min(self, j: dict) -> float:
        e = float(j.get("est_min", 5))
        if self.smoke:
            return 1.0
        done = [r for r in self.ok().values() if r.get("kind") == j["kind"] and r.get("est_min") and r.get("minutes") is not None]
        if done:
            ratio = statistics.median(max(r["minutes"], 0.1) / max(r["est_min"], 0.1) for r in done)
            e *= min(3.0, max(0.3, ratio))
        return e

    def deadline(self, write: bool = True) -> float:
        p = self.dir / "deadline.json"
        if p.exists():
            return float(json.loads(p.read_text())["epoch"])
        env = os.environ.get("M5_DEADLINE")
        if env and not self.smoke:
            hh, mm = (int(x) for x in env.split(":"))
            t = time.localtime()
            d = time.mktime((t.tm_year, t.tm_mon, t.tm_mday, hh, mm, 0, 0, 0, -1))
            if d <= time.time():
                d += 86400
        else:
            d = time.time() + 3600 * float(self.c.get("deadline_hours", 6.5))
        if write:
            self.dir.mkdir(parents=True, exist_ok=True)
            p.write_text(json.dumps({"epoch": d, "local": time.strftime("%Y-%m-%d %H:%M", time.localtime(d))}))
        return d

    def margin_s(self) -> float:
        return 60 * float(self.c.get("report_min", 25)) * (0.05 if self.smoke else 1.0)

    # ---- threads ------------------------------------------------------------------------------------------------
    def threads(self) -> int:
        p = self.dir / "threads.json"
        if p.exists():
            return int(json.loads(p.read_text())["threads"])
        return int(self.c.get("threads_default", 6))

    # ---- running commands ---------------------------------------------------------------------------------------
    def env(self, extra: dict | None = None) -> dict:
        e = dict(os.environ, AURELIA_LAB=str(LAB), AURELIA_EXAM_BIN=EXAM, PYTHONPATH=str(NN), MLX_ENABLE_TF32=os.environ.get("MLX_ENABLE_TF32", "0"))
        e.pop("ARMY_SCENE_RULE_VERSION", None)   # the commands name the version themselves (--kept-version, or the teacher recipe's variable)
        e.pop("ARMY_NET", None)
        e.pop("ARMY_PLAY_ON_CAP", None)
        e.update({k: str(v) for k, v in (extra or {}).items()})
        return e

    def res(self, name: str, cmd: list, cpu: int | str = 1, gpu: bool = False, env: dict | None = None, cwd: Path | None = None,
            logname: str | None = None) -> int:
        """One command through the broker; stdout and stderr to lab/logs/<logname>.log (appended)."""
        args = [RES, "run", "--cpu", str(cpu), "--disk", "25", "--name", f"c4-{name}"]
        if gpu:
            args.append("--gpu")
        args += ["--", *[str(a) for a in cmd]]
        lp = LAB / "logs" / f"{logname or name}.log"
        lp.parent.mkdir(parents=True, exist_ok=True)
        with open(lp, "a") as fh:
            fh.write(f"== {now()} {' '.join(str(a) for a in cmd)}\n")
            fh.flush()
            rc = subprocess.run(args, env=self.env(env), cwd=str(cwd or LAB), stdout=fh, stderr=subprocess.STDOUT).returncode
            fh.write(f"== {now()} rc {rc}\n")
        return rc

    def py(self, name: str, args: list, gpu: bool = False, cpu: int | str = 1, env: dict | None = None) -> int:
        return self.res(name, [PY, *args], cpu=cpu, gpu=gpu, env=env, cwd=NN)

    def families(self) -> str:
        if self.smoke:
            return self.smoke_fams
        return (D / "families.txt").read_text().strip()

    def suite(self, name: str, net_bin: Path, out: Path, frm: int, to: int, families: str, threads: int, cap: bool = False) -> int:
        """ai-exam suite of a net on the kept scenes under the honest input (kept set of rule version 2): rows in out/rows.jsonl."""
        cmd = [EXAM, "suite", "--kept", D / "kept-all.jsonl", "--kept-version", "2", "--from", frm, "--instances", to, "--families", families,
               "--players", "army-net", "--jobs", threads, "--out", out]
        return self.res(name, cmd, cpu=threads, env={"ARMY_NET": net_bin, **({"ARMY_PLAY_ON_CAP": "1"} if cap else {})})

    def table_pair(self, name: str, net_bin: Path, out: Path, threads: int) -> int:
        """The two tables of the gate: full = every pool family, quick = ARMY_PLAY_ON_CAP=1 on the quick families; held-out 80-99."""
        a, b = int(self.data["hold_from"]), int(self.data["hold_to"])
        rc = self.suite(f"{name}-full", net_bin, out / "full", a, b, self.families(), threads)
        if rc:
            return rc
        return self.suite(f"{name}-quick", net_bin, out / "quick", a, b, self.data["quick_families"], threads, cap=True)

    # ---- the queue -----------------------------------------------------------------------------------------------
    def plan(self) -> None:
        d = self.deadline(write=False)
        t = time.time()
        ok = self.ok()
        print(f"deadline {time.strftime('%H:%M', time.localtime(d))}, report margin {self.margin_s() / 60:.0f} min, threads {self.threads()}")
        for j in self.jobs:
            if j["name"] in ok:
                print(f"  {j['name']:<16} done")
                continue
            e = self.est_min(j)
            t += 60 * e
            late = "  SKIPPED (after deadline)" if t > d - self.margin_s() and j["kind"] != "summary" else ""
            print(f"  {j['name']:<16} {e:5.0f} min  ends {time.strftime('%H:%M', time.localtime(t))}{late}")

    def write_status(self) -> None:
        self.dir.mkdir(parents=True, exist_ok=True)
        done = self.ok()
        st = {"time": now(), "state": self.state, "current": self.current, "done": [j["name"] for j in self.jobs if j["name"] in done],
              "todo": [j["name"] for j in self.jobs if j["name"] not in done], "threads": self.threads(),
              "deadline": time.strftime("%Y-%m-%d %H:%M", time.localtime(self.deadline(write=False)))}
        (self.dir / "status.json").write_text(json.dumps(st, indent=1))

    def run(self) -> int:
        self.dir.mkdir(parents=True, exist_ok=True)
        stop_file = self.dir / "STOP"
        deadline = self.deadline()
        log(f"start: {len(self.jobs)} jobs, deadline {time.strftime('%Y-%m-%d %H:%M', time.localtime(deadline))}, smoke={self.smoke}")
        halted = False
        for j in self.jobs:
            name = j["name"]
            ok = self.ok()
            if name in ok:
                continue
            if stop_file.exists() and j["kind"] != "summary":
                log("STOP file: the queue stops")
                self.state = "stopped"
                break
            if halted and j["kind"] != "summary":
                self.append({"job": name, "kind": j["kind"], "status": "skipped_fatal"})
                continue
            bad = [a for a in j.get("after", []) if a not in ok and self.job_known(a)]
            if bad:
                self.append({"job": name, "kind": j["kind"], "status": "skipped_dep", "missing": bad})
                log(f"{name}: skipped, {bad} did not end ok")
                continue
            est = self.est_min(j)
            if j["kind"] != "summary" and time.time() + 60 * est > deadline - self.margin_s():
                self.append({"job": name, "kind": j["kind"], "status": "skipped_deadline", "est_min": est})
                log(f"{name}: skipped, {est:.0f} min do not fit before the deadline")
                continue
            self.current, self.state = name, "running"
            self.write_status()
            res: dict = {}
            for attempt in range(int(self.c.get("retries", 1)) + 1):
                t0 = time.time()
                log(f"{name}: start (est {est:.0f} min, attempt {attempt + 1})")
                try:
                    res = getattr(self, f"job_{j['kind']}")(j)
                except Exception as e:  # noqa: BLE001
                    res = {"status": "failed", "why": f"{type(e).__name__}: {e}"}
                res["minutes"] = round((time.time() - t0) / 60, 2)
                log(f"{name}: {res.get('status')} in {res['minutes']} min {res.get('why', '')}")
                if res.get("status") == "ok" or res.get("final"):
                    break
            if res.get("status") != "ok":
                res["status"] = "failed_final"
                if j.get("fatal"):
                    halted = True
                    res["fatal"] = True
            self.append({"job": name, "kind": j["kind"], "est_min": float(j.get("est_min", 5)), **res})
            self.write_status()
        self.state, self.current = ("stopped" if stop_file.exists() else "finished"), "-"
        self.write_status()
        log(f"end: {self.state}")
        return 0

    def job_known(self, name: str) -> bool:
        return any(j["name"] == name for j in self.jobs)

    # ---- job kinds ---------------------------------------------------------------------------------------------
    def job_setup(self, j: dict) -> dict:
        sh = lambda *c: subprocess.run(c, capture_output=True, text=True).stdout.strip()
        info = {"git_head": sh("git", "-C", str(REPO), "rev-parse", "--short", "HEAD"), "platform_head": sh("git", "-C", str(REPO / ".." / "platform"), "rev-parse", "--short", "HEAD")}
        if os.environ.get("M5_SKIP_SETUP"):   # a machine with its own venv, binary and broker: nothing is built here
            if not Path(EXAM).exists():
                return {"status": "failed", "why": f"no ai-exam binary at {EXAM}"}
        else:
            rc = subprocess.run([str(M5 / "setup4.sh")], cwd=str(REPO), env=self.env()).returncode
            if rc:
                return {"status": "failed", "why": "tools/m5/setup4.sh failed"}
        if not Path(EXAM).exists():
            return {"status": "failed", "why": "no ai-exam binary"}
        v = subprocess.run([PY, "-I", "-c", "import mlx.core as mx, importlib.metadata as m; print(m.version('mlx'), mx.metal.is_available())"], capture_output=True, text=True)
        if v.returncode:
            return {"status": "failed", "why": "the MLX venv does not import: " + v.stderr[-300:]}
        info.update({"mlx": v.stdout.strip(), "chip": sh("sysctl", "-n", "machdep.cpu.brand_string"), "macos": sh("sw_vers", "-productVersion"),
                     "cores_perf": sh("sysctl", "-n", "hw.perflevel0.physicalcpu"), "cores_eff": sh("sysctl", "-n", "hw.perflevel1.physicalcpu"),
                     "ram_gb": round(int(sh("sysctl", "-n", "hw.memsize") or 0) / 2 ** 30), "exam_bin": Path(EXAM).name,
                     "free_disk_gb": round(shutil.disk_usage(LAB).free / 2 ** 30)})
        self.dir.mkdir(parents=True, exist_ok=True)
        (self.dir / "versions.json").write_text(json.dumps(info, indent=1))
        return {"status": "ok", **info}

    def job_seed(self, j: dict) -> dict:
        (D / "ref").mkdir(parents=True, exist_ok=True)
        fams = set(self.smoke_fams.split(",")) if self.smoke else None
        for n in ("kept-all", "kept-r", "kept-r2"):
            gunzip(SEED / f"{n}.jsonl.gz", D / f"{n}.jsonl")
        shutil.copy(SEED / "families.txt", D / "families.txt")
        if fams:   # the smoke lab: the kept set and the reference rows of the smoke families only
            for n in ("kept-all", "kept-r", "kept-r2"):
                lines = [ln for ln in (D / f"{n}.jsonl").read_text().splitlines() if json.loads(ln)["family"] in fams]
                (D / f"{n}.jsonl").write_text("\n".join(lines) + "\n")
        for p in (SEED / "ref").glob("*.rows.jsonl.gz"):
            dst = D / "ref" / p.name[:-3]
            gunzip(p, dst)
            if fams:
                dst.write_text("".join(ln + "\n" for ln in dst.read_text().splitlines() if json.loads(ln)["family"] in fams))
        same = {}
        for n in ("N175", "N175T"):
            d = self.R / f"net-{n}"
            d.mkdir(parents=True, exist_ok=True)
            shutil.copy(SEED / "nets" / f"{n}.safetensors", d / "weights.safetensors")
            shutil.copy(SEED / "nets" / f"{n}.army-net.bin", d / "army-net.bin")
            tmp = d / "export-check.bin"   # the export on this machine must give the shipped Rust file
            if self.py(f"export-{n}", [str(M5 / "py" / "export_ckpt.py"), d / "weights.safetensors", tmp]):
                return {"status": "failed", "why": f"export of {n} failed (lab/logs/export-{n}.log)"}
            same[n] = tmp.read_bytes() == (d / "army-net.bin").read_bytes()
        return {"status": "ok", "export_same_as_shipped": same, "kept_lines": sum(1 for _ in open(D / "kept-all.jsonl"))}

    def job_threads(self, j: dict) -> dict:
        """The full honest table of N175 at each candidate thread count; the faster one is used for the night (ties go to the smaller)."""
        net = self.R / "net-N175" / "army-net.bin"
        out: dict = {}
        for t in self.data["threads_probe"]:
            t = int(t)
            tdir = self.R / "threads" / f"t{t}"
            shutil.rmtree(tdir, ignore_errors=True)
            t0 = time.time()
            if self.suite(f"probe-t{t}", net, tdir / "full", int(self.data["hold_from"]), int(self.data["hold_to"]), self.families(), t):
                return {"status": "failed", "why": f"the probe at {t} threads failed (lab/logs/probe-t{t}.log)"}
            out[t] = round(time.time() - t0, 1)
        ranked = sorted(out, key=lambda t: (out[t], t))
        best = ranked[0]
        smaller = min(out)
        if out[smaller] <= out[best] * 1.04:   # within 4 %: fewer threads (less heat, more room for the learner)
            best = smaller
        eq = summary4.rows_equal(self.R / "threads" / f"t{min(out)}" / "full" / "rows.jsonl", self.R / "threads" / f"t{max(out)}" / "full" / "rows.jsonl")
        (self.dir / "threads.json").write_text(json.dumps({"threads": best, "seconds": out, "rows_equal_across_threads": eq}))
        return {"status": "ok", "threads": best, "seconds": out, "rows_equal_across_threads": eq}

    def job_sanity(self, j: dict) -> dict:
        """P0: the M5 plays the M4's honest references play for play (win, rounds, measures). Any difference stops the night."""
        t = self.threads()
        a, b = int(self.data["hold_from"]), int(self.data["hold_to"])
        sd = self.R / "sanity"
        res: dict = {}
        for tag, net, which in (("N175-full", "N175", "full"), ("N175-quick", "N175", "quick"), ("N175T-quick", "N175T", "quick")):
            out = sd / tag
            shutil.rmtree(out, ignore_errors=True)
            fams = self.families() if which == "full" else self.data["quick_families"]
            if self.suite(f"sanity-{tag}", self.R / f"net-{net}" / "army-net.bin", out, a, b, fams, t, cap=(which == "quick")):
                return {"status": "failed", "why": f"{tag} did not run (lab/logs/sanity-{tag}.log)", "final": True}
            res[tag] = summary4.rows_equal(D / "ref" / f"{which}-N175h.rows.jsonl", out / "rows.jsonl")
        (sd / "sanity.json").write_text(json.dumps(res, indent=1))
        bad = {k: v for k, v in res.items() if v["different"] or v["missing"] or v["extra"]}
        if bad:
            return {"status": "failed", "why": "the M5 does not play the M4's honest rows play for play: " + json.dumps(bad), "sanity": res, "final": True}
        return {"status": "ok", "sanity": res}

    def job_teach(self, j: dict) -> dict:
        """The teachers' recordings of the anchor (recipe: G1.10h's anchor, scenes 0-79, the teachers solvers and unciv)."""
        t = self.threads()
        ad = D / "anchor"
        ad.mkdir(parents=True, exist_ok=True)
        to = int(self.data["teach_to"])
        fr = self.data["teach_r_families"]
        r = [EXAM, "suite", "--kept", D / "kept-r.jsonl", "--kept-version", "1", "--from", "0", "--instances", to, *(["--families", fr] if fr else []),
             "--players", "solvers,unciv", "--out", ad / "teach-r-train", "--record", ad / "teach-r-train.bin", "--jobs", t]
        r2 = [EXAM, "suite", "--kept", D / "kept-r2.jsonl", "--kept-version", "2", "--from", "0", "--instances", to, "--families", self.data["teach_r2_families"],
              "--players", "solvers", "--out", ad / "p7-teach-train", "--record", ad / "p7-teach-train.bin", "--jobs", t]
        for p in (ad / "teach-r-train.bin", ad / "p7-teach-train.bin"):
            p.unlink(missing_ok=True)
        if self.res("teach-r", r, cpu=t, env={"ARMY_SCENE_RULE_VERSION": "1"}) or self.res("teach-r2", r2, cpu=t, env={"ARMY_SCENE_RULE_VERSION": "2"}):
            return {"status": "failed", "why": "a teacher recording failed (lab/logs/teach-r.log, teach-r2.log)"}
        sizes = {p.name: p.stat().st_size for p in ad.glob("*.bin")}
        if not all(sizes.get(n) for n in ("teach-r-train.bin", "p7-teach-train.bin")):
            return {"status": "failed", "why": f"empty recording {sizes}"}
        return {"status": "ok", "bytes": sizes}

    def job_lengths(self, j: dict) -> dict:
        """The greedy pass of N175T over the training scenes: the lengths the loop draws its sampled rounds within."""
        out = D / "lengths-h"
        shutil.rmtree(out, ignore_errors=True)
        if self.suite("lengths", self.R / "net-N175T" / "army-net.bin", out, 0, int(self.data["lengths_to"]), self.families(), self.threads()):
            return {"status": "failed", "why": "the lengths pass failed (lab/logs/lengths.log)"}
        return {"status": "ok", "rows": sum(1 for _ in open(out / "rows.jsonl"))}

    # ---- the RL runs ---------------------------------------------------------------------------------------------
    def run_dir(self, run: str) -> Path:
        return self.R / f"rl-{run}"

    def base_lambda(self, seed: int) -> float | None:
        for r in read_jsonl(self.run_dir(f"base-s{seed}") / "log.jsonl"):
            if r.get("update") == 1 and "lambda" in r:
                return float(r["lambda"])
        return None

    def job_rl(self, j: dict) -> dict:
        n = self.night
        run, arm, seed = j["run"], j["arm"], int(j["seed"])
        out = self.run_dir(run)
        t = self.threads()
        args = ["-m", "aurelia_nn.rl3.loop", "--out", out, "--weights", self.R / "net-N175T" / "weights.safetensors", "--actor-bin", EXAM,
                "--kept", D / "kept-all.jsonl", "--lengths", D / "lengths-h" / "rows.jsonl",
                "--anchor", D / "anchor" / "teach-r-train.bin", "--anchor-v2", D / "anchor" / "p7-teach-train.bin",
                "--jobs", t, "--window", n["window"], "--updates", n["updates"], "--ckpt-every", n["ckpt_every"], "--val-every", n["val_every"], "--seed", seed]
        for k, flag in (("P", "--P"), ("G", "--G"), ("K", "--K"), ("val_to", "--val-to")):
            if k in n:
                args += [flag, n[k]]
        if self.smoke:
            args += ["--parity-every", 1]
        extra = {}
        if arm == "boost":
            lam = self.base_lambda(seed)
            if lam is None:
                return {"status": "failed", "why": f"no lambda in the log of base-s{seed}", "final": True}
            args += ["--anchor-auto", 0, "--anchor-w", lam, "--anchor-boost", f"{self.data['siege_group']}:{self.data['anchor_boost_factor']}"]
        cmd = ["sh", M5 / "watchdog.sh", out, "--", PY, *args]
        extra.update({"POLL": 5 if self.smoke else 30, "STALE": 30})
        rc = self.res(f"rl-{run}", cmd, cpu=t, gpu=True, env=extra, cwd=NN, logname=f"rl-{run}")
        if rc or not (out / "done").exists():
            return {"status": "failed", "why": f"the loop did not finish (rc {rc}; lab/logs/rl-{run}.log, {out.name}/log.jsonl)"}
        keep = {int(x) for x in n["keep_ckpts"]} | {int(n["updates"])}
        removed = 0
        for p in out.glob("ckpt-*.safetensors"):
            u = int(p.name.split("-")[1].split(".")[0])
            if u % 50 and u not in keep and not self.smoke:
                for q in out.glob(f"ckpt-{u:04d}.*"):
                    q.unlink()
                removed += 1
        rows = read_jsonl(out / "log.jsonl")
        vals = [r for r in rows if "validation" in r]
        ups = [r for r in rows if "update_s" in r]
        return {"status": "ok", "updates": len(ups), "lambda": self.base_lambda(seed) if arm == "base" else None,
                "val_start": vals[0]["family_mean"] if vals else None, "val_last": vals[-1]["family_mean"] if vals else None,
                "update_s_median": round(statistics.median(r["update_s"] for r in ups), 2) if ups else None, "ckpts_removed": removed}

    # ---- tables of the checkpoints ---------------------------------------------------------------------------------
    def job_tables(self, j: dict) -> dict:
        run = j["run"]
        rd = self.run_dir(run)
        t = self.threads()
        out: dict = {}
        for u in self.night["table_ckpts"]:
            u = int(u)
            ck = rd / f"ckpt-{u:04d}.safetensors"
            td = self.R / "tables" / f"{run}-u{u:04d}"
            if not ck.exists():
                out[u] = "no checkpoint"
                continue
            if not (td / "done").exists():
                shutil.rmtree(td, ignore_errors=True)
                td.mkdir(parents=True)
                nb = td / "army-net.bin"
                if self.py(f"export-{run}-{u}", [str(M5 / "py" / "export_ckpt.py"), ck, nb]):
                    return {"status": "failed", "why": f"export of {ck.name} failed"}
                if self.table_pair(f"tables-{run}-u{u}", nb, td, t):
                    return {"status": "failed", "why": f"the tables of {run} update {u} failed (lab/logs/tables-{run}-u{u}-*.log)"}
                (td / "done").write_text(str(u))
            gate = self.gates(td, f"{run} update {u}")
            out[u] = {"gate": gate}
            (td / "gate.json").write_text(json.dumps(gate, indent=1))
        return {"status": "ok", "tables": out}

    def gates(self, td: Path, label: str) -> dict:
        """gatestats of one checkpoint's tables against the honest N175 (the three specs of G1.10h); a failure here is recorded, not fatal."""
        res = {}
        for spec in ("g110_main", "g110_new", "g110_third"):
            gd = td / f"gate-{spec}"
            inp = {"A_label": "N175 honest", "B_label": label, "slices": [
                {"name": s, "kept": str(D / "kept-all.jsonl"), "window": [int(self.data["hold_from"]), int(self.data["hold_to"])],
                 "A": [str(D / "ref" / f"{s}-N175h.rows.jsonl")], "B": [str(td / s / "rows.jsonl")]} for s in ("full", "quick")]}
            gd.mkdir(exist_ok=True)
            (gd / "inputs.json").write_text(json.dumps(inp, indent=1))
            p = subprocess.run([sys.executable, "-I", str(GATE), "report", "--inputs", str(gd / "inputs.json"), "--spec", str(SPECS / f"{spec}.json"), "--out", str(gd)],
                               capture_output=True, text=True)
            (gd / "gate.log").write_text(p.stdout + p.stderr)
            rep = gd / "report.json"
            if p.returncode not in (0, 1) or not rep.exists():
                res[spec] = {"error": (p.stderr or p.stdout)[-300:]}
                continue
            rj = json.loads(rep.read_text())
            st: dict = {}
            for f, v in rj.get("families", {}).items():
                st.setdefault(v.get("status", "?"), []).append(f)
            res[spec] = {"pass": rj.get("pass"), "drop": sorted(st.get("DROP", [])), "summary": rj.get("summary")}
        return res

    def job_summary(self, j: dict) -> dict:
        summary4.write_summary(LAB, self.dir, self.cfg, self.smoke)
        return {"status": "ok"}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["run", "plan", "status", "summary"])
    ap.add_argument("--config", default=str(M5 / "campaign4.toml"))
    ap.add_argument("--smoke", action="store_true")
    ap.add_argument("--only", default="")
    a = ap.parse_args()
    cfg = tomllib.loads(Path(a.config).read_text())
    c = Campaign(cfg, a.smoke, [x for x in a.only.split(",") if x] or None)
    if a.cmd == "plan":
        c.plan()
    elif a.cmd == "status":
        p = c.dir / "status.json"
        print(p.read_text() if p.exists() else "no status yet")
    elif a.cmd == "summary":
        c.job_summary({})
        print(c.dir / "summary.md")
    else:
        return c.run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
