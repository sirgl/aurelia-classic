#!/usr/bin/env python3
"""The overnight campaign: a resumable queue of training and eval jobs through the vendored broker.

    campaign.py run [--config campaign.toml] [--smoke MINUTES] [--no-calibrate]
    campaign.py status | summary

Jobs, in order (a net's evals follow its training, so results appear early):
  teach (record the teacher data if missing) -> calibrate (5 min, prints the estimated duration)
  -> for each ladder size: train, quick, full, full-zero  -> pick (best size)
  -> for each seed at the best size: train, quick, full, full-zero  -> summary.
Each finished job appends one line to lab/runs/campaign/results.jsonl; status.json feeds the dashboard.
A crash or a reboot: start it again. Jobs with an "ok" line are skipped; a training resumes from its checkpoint.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import re
import subprocess
import sys
import threading
import time
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
LAB = Path(os.environ.get("AURELIA_LAB") or REPO / "lab")
M5 = REPO / "tools" / "m5"
SIEGE8 = ["siege", "ram", "assault-heal", "city-fire", "hill-archers", "planned-assault", "promotion-choice", "rush"]
T975 = {1: 12.71, 2: 4.303, 3: 3.182, 4: 2.776, 5: 2.571, 6: 2.447, 7: 2.365, 8: 2.306, 9: 2.262}


def now() -> str:
    return time.strftime("%Y-%m-%d %H:%M:%S")


class Campaign:
    def __init__(self, cfg: dict, smoke: float | None):
        self.cfg, self.smoke = cfg, smoke
        self.dir = LAB / "runs" / ("campaign-smoke" if smoke else "campaign")
        self.dir.mkdir(parents=True, exist_ok=True)
        self.results_path = self.dir / "results.jsonl"
        self.current, self.total, self.eta_h, self.state = "-", 0, None, "starting"
        self.t0 = time.time()
        self.calib: dict = {}

    # ---- results -----------------------------------------------------------------------------
    def results(self) -> list[dict]:
        if not self.results_path.exists():
            return []
        out = []
        for line in self.results_path.read_text().splitlines():
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                pass
        return out

    def ok(self) -> dict[str, dict]:
        return {r["job"]: r for r in self.results() if r.get("status") == "ok"}

    def append(self, row: dict) -> None:
        row["time"] = now()
        with open(self.results_path, "a") as f:
            f.write(json.dumps(row) + "\n")
            f.flush()
            os.fsync(f.fileno())

    def write_status(self) -> None:
        done = len(self.ok())
        (self.dir / "status.json").write_text(json.dumps({
            "state": self.state, "done": done, "total": self.total, "current": self.current, "eta_h": self.eta_h,
            "elapsed_h": round((time.time() - self.t0) / 3600, 2), "updated": now(), "smoke": bool(self.smoke)}))

    def heartbeat(self) -> None:
        while True:
            time.sleep(60)
            try:
                self.write_status()
            except OSError:
                pass

    # ---- running scripts ---------------------------------------------------------------------
    def sh(self, args: list[str], env: dict | None = None) -> int:
        e = dict(os.environ, AURELIA_LAB=str(LAB))
        e.update(env or {})
        return subprocess.run([str(a) for a in args], env=e, cwd=REPO).returncode

    def plan(self) -> list[dict]:
        c, ok = self.cfg["campaign"], self.ok()
        jobs: list[dict] = [{"job": "teach", "kind": "teach"}]
        if not self.smoke and "calibrate" not in ok and not self.cfg.get("_no_calibrate"):
            jobs.append({"job": "calibrate", "kind": "calibrate"})
        for l in self.cfg["ladder"]:
            jobs += self.net_jobs(f"d{l['d']}-l{l['layers']}", l["d"], l["layers"], 1, "ladder")
            if self.smoke:
                return jobs[:2]
        jobs.append({"job": "pick", "kind": "pick"})
        best = ok.get("pick", {}).get("best")
        if best:
            for seed in c["seeds"]:
                jobs += self.net_jobs(f"best-d{best['d']}-l{best['layers']}-s{seed}", best["d"], best["layers"], seed, "seed")
        jobs.append({"job": "summary", "kind": "summary"})
        return jobs

    def net_jobs(self, name: str, d: int, layers: int, seed: int, group: str) -> list[dict]:
        base = {"net": name, "d": d, "layers": layers, "seed": seed, "group": group}
        jobs = [{"job": f"train:{name}", "kind": "train", **base}, {"job": f"quick:{name}", "kind": "quick", **base},
                {"job": f"full:{name}", "kind": "full", **base}]
        if self.cfg["campaign"].get("ablate_memory", True):
            jobs.append({"job": f"fullzero:{name}", "kind": "fullzero", **base})
        return jobs

    # ---- estimates ---------------------------------------------------------------------------
    def estimate_job_s(self, job: dict) -> float:
        cal = self.calib
        if not cal:
            return 0.0
        t = self.cfg["train"]
        k = job["kind"]
        if k == "train":
            size = (job["d"] / 64) ** 1.5 * (job["layers"] / 3) ** 0.9
            return t["epochs"] * cal["sec_per_epoch"] * size
        if k == "quick":
            return cal["quick_s"]
        if k in ("full", "fullzero"):
            return cal["quick_s"] * 3
        return 0.0

    def update_eta(self, jobs: list[dict]) -> None:
        ok = self.ok()
        left = [j for j in jobs if j["job"] not in ok]
        s = sum(self.estimate_job_s(j) for j in left)
        if self.calib and not any(j["kind"] == "train" and j.get("group") == "seed" for j in jobs):
            best_guess = {"kind": "train", "d": 96, "layers": 3}
            n_seed = len(self.cfg["campaign"]["seeds"])
            per_net = self.estimate_job_s(best_guess) + self.calib["quick_s"] * 7
            s += n_seed * per_net
        self.eta_h = round(s / 3600, 1) if self.calib else None
        self.total = len(jobs)

    # ---- metrics -----------------------------------------------------------------------------
    def train_metrics(self, name: str) -> dict:
        run = LAB / "runs" / name
        out: dict = {}
        cur = run / "curves.jsonl"
        if cur.exists():
            rows = [json.loads(l) for l in cur.read_text().splitlines() if l.strip().startswith("{")]
            if rows:
                out["dps"] = round(rows[-1].get("dps_mean", 0))
                out["steps"] = rows[-1].get("step")
                hold = [r["hold"] for r in rows if "hold" in r]
                if hold:
                    out["hold_acc_both"] = round(hold[-1].get("acc_both", 0), 4)
        cfg = run / "config.json"
        if cfg.exists():
            out["params"] = json.loads(cfg.read_text()).get("parameters")
        log = LAB / "logs" / f"{name}.log"
        if log.exists():
            rss = [int(m) for m in re.findall(r"^\s*(\d+)\s+peak memory footprint", log.read_text(errors="replace"), re.M)]
            if rss:
                out["peak_ram_gb"] = round(max(rss) / 2**30, 1)
        return out

    def eval_metrics(self, run: str, net: str) -> dict:
        rows_path = LAB / "runs" / run / "rows.jsonl"
        out: dict = {}
        if not rows_path.exists():
            return out
        rows = [json.loads(l) for l in rows_path.read_text().splitlines() if l.strip()]
        us = turns = 0.0
        for r in rows:
            info, meas = r.get("info", {}), r.get("measures", {})
            us += sum(float(info.get(k, 0)) for k in ("net_encode_us", "net_forward_us", "net_memory_us", "net_observe_us", "net_update_us"))
            turns += float(meas.get("net_turns", 0))
        if turns:
            out["ms_per_turn"] = round(us / 1000 / turns, 2)
        table = LAB / "runs" / "campaign" / f"table-{run}.json"
        table.parent.mkdir(exist_ok=True)
        if self.sh([M5 / "table.sh", table, f"{net}={rows_path}"]) == 0 and table.exists():
            t = json.loads(table.read_text())
            fam = [r for r in t["rows"] if net in r["rates"]]
            out["families"] = len(fam)
            out["families_90"] = sum(1 for r in fam if r["pass"].get(net))
            sg = [r["rates"][net] for r in fam if r["family"] in SIEGE8]
            out["siege8_mean"] = round(sum(sg) / len(sg), 4) if sg else None
            out["win_by_family"] = {r["family"]: round(r["rates"][net], 3) for r in fam}
        return out

    # ---- job execution -----------------------------------------------------------------------
    def execute(self, job: dict) -> dict:
        k, t = job["kind"], self.cfg["train"]
        cfgc = self.cfg["campaign"]
        if k == "teach":
            rc = self.sh([M5 / "teach.sh"], {"M5_SMOKE": "1"} if self.smoke else {})
            return {"status": "ok" if rc == 0 else "failed"}
        if k == "calibrate":
            return self.calibrate()
        if k == "train":
            env = {"M5_D": job["d"], "M5_LAYERS": job["layers"], "M5_SEED": job["seed"], "M5_EPOCHS": t["epochs"],
                   "M5_PLAYS": t["plays"], "M5_LR": t["lr"], "M5_CHUNK_MIN": t["chunk_min"]}
            if self.smoke:
                env["M5_MAX_MINUTES"] = self.smoke
            rc = self.sh([M5 / "train.sh", job["net"]], {k2: str(v) for k2, v in env.items()})
            return {"status": "ok" if rc == 0 else "failed", "net": job["net"], "d": job["d"], "layers": job["layers"], "seed": job["seed"],
                    "group": job["group"], **self.train_metrics(job["net"])}
        if k in ("quick", "full", "fullzero"):
            net = job["net"]
            run = {"quick": f"evq-{net}", "full": f"evf-{net}", "fullzero": f"evfz-{net}"}[k]
            env = {"M5_HOLD_TO": cfgc["quick_hold_to"] if k == "quick" else cfgc["full_hold_to"]}
            extra = ["ARMY_NET_ZERO_MEMORY=1"] if k == "fullzero" else []
            rc = self.sh([M5 / "eval.sh", "quick" if k == "quick" else "full", net, run, *extra], {a: str(b) for a, b in env.items()})
            if rc != 0:
                return {"status": "failed"}
            return {"status": "ok", "net": net, "group": job["group"], "d": job["d"], "layers": job["layers"], "seed": job["seed"],
                    **self.eval_metrics(run, net)}
        if k == "pick":
            return self.pick()
        if k == "summary":
            return {"status": "ok", **self.summary()}
        raise ValueError(k)

    def autotune(self) -> dict:
        """Auto-tune of the training width: the plays per step (a step is `plays` whole plays, ~40 decisions each) in
        128 / 256 / 512, 60 s each. Keep the fastest (decisions/s) that stays under the MLX memory limit with the
        same loss over the first steps as 128 (within the noise of its own steps). Written into campaign.toml."""
        info = json.loads(subprocess.run([str(REPO / "tools/res/res"), "info"], capture_output=True, text=True).stdout)
        limit = info["mlx_gb"]
        out: dict = {}
        for plays in self.cfg["campaign"].get("tune_plays", [128, 256, 512]):
            name = f"tune-p{plays}"
            subprocess.run(["rm", "-rf", str(LAB / "runs" / name), str(LAB / "logs" / f"{name}.log")])
            self.sh([M5 / "train.sh", name], {"M5_PLAYS": str(plays), "M5_MAX_MINUTES": "1", "M5_CHUNK_MIN": "1", "M5_EVAL_EVERY": "100000", "M5_EPOCHS": "2", "M5_D": "64", "M5_LAYERS": "3"})
            cur = LAB / "runs" / name / "curves.jsonl"
            rows = [json.loads(l) for l in cur.read_text().splitlines() if l.strip().startswith("{")] if cur.exists() else []
            m = self.train_metrics(name)
            out[plays] = {"dps": m.get("dps"), "peak_ram_gb": m.get("peak_ram_gb"), "losses": [(r["step"], r["loss"]) for r in rows if "loss" in r]}
        ref = out.get(128) or next(iter(out.values()))
        common = min([max(s for s, _ in v["losses"]) if v["losses"] else 0 for v in out.values()] + [200])
        def mean_loss(v):
            xs = [l for s, l in v["losses"] if s <= max(common, 1)] or [l for _, l in v["losses"]]
            return sum(xs) / len(xs) if xs else float("inf"), xs
        rl, rxs = mean_loss(ref)
        noise = (sum((x - rl) ** 2 for x in rxs) / max(len(rxs), 1)) ** 0.5 if rxs else 0
        best, best_dps = None, -1.0
        for plays, v in out.items():
            ml, _ = mean_loss(v)
            v["mean_loss"] = round(ml, 4)
            fits = (v["peak_ram_gb"] or 0) < limit
            same = ml <= rl + max(noise, 0.02 * rl)
            v["ok"] = bool(fits and same and v["dps"])
            if v["ok"] and v["dps"] > best_dps:
                best, best_dps = plays, v["dps"]
        chosen = best or 128
        for v in out.values():
            v.pop("losses", None)
        cfg_path = Path(self.cfg["_path"])
        text = cfg_path.read_text()
        text = re.sub(r"(?m)^plays = \d+", f"plays = {chosen}", text)
        text = text.split("# --- auto-tune")[0].rstrip() + "\n\n# --- auto-tune (written by campaign.py) ---\n[tune]\n" \
            f"when = \"{now()}\"\nmlx_limit_gb = {limit}\nchosen_plays = {chosen}\n" \
            + "".join(f"# plays {p}: {v}\n" for p, v in out.items())
        cfg_path.write_text(text)
        self.cfg["train"]["plays"] = chosen
        print(f"[campaign] auto-tune: plays per step {chosen} ({out})", flush=True)
        return {"chosen_plays": chosen, "tried": out, "mlx_limit_gb": limit}

    def calibrate(self) -> dict:
        """Auto-tune (~5 min), then ~5 min of calibration: 3 of training d 64 / 3 layers at the chosen width, then
        the quick eval on 4 held-out instances. Prints the estimated duration of the campaign."""
        tuned = self.autotune()
        name = "calib"
        t0 = time.time()
        env = {"M5_MAX_MINUTES": "3", "M5_EPOCHS": "2", "M5_D": "64", "M5_LAYERS": "3", "M5_CHUNK_MIN": "3", "M5_PLAYS": str(tuned["chosen_plays"])}
        subprocess.run(["rm", "-rf", str(LAB / "runs" / name), str(LAB / "logs" / f"{name}.log")])
        self.sh([M5 / "train.sh", name], env)
        cur = LAB / "runs" / name / "curves.jsonl"
        rows = [json.loads(l) for l in cur.read_text().splitlines() if l.strip().startswith("{")] if cur.exists() else []
        if not rows or not rows[-1].get("epoch"):
            return {"status": "failed", "why": "calibration training produced no curve", "tune": tuned}
        spe = rows[-1]["wall_s"] / max(rows[-1]["epoch"], 1e-3)
        t1 = time.time()
        self.sh([M5 / "eval.sh", "quick", name, "evq-calib"], {"M5_HOLD_TO": "84"})
        dt = time.time() - t1
        quick_s = dt * 20 / 4
        rp = LAB / "runs" / "evq-calib" / "rows.jsonl"
        plays = sum(1 for _ in rp.open()) if rp.exists() else 0
        cal = {"sec_per_epoch": round(spe), "quick_s": round(quick_s), "dps": rows[-1].get("dps_mean"),
               "eval_plays_per_s": round(plays / dt, 2) if dt else None, "calibration_s": round(time.time() - t0), "tune": tuned}
        self.calib = cal
        self.update_eta(self.plan())
        print(f"[campaign] calibration: {cal['dps']} decisions/s, {cal['eval_plays_per_s']} eval plays/s; estimated campaign: about {self.eta_h} h", flush=True)
        return {"status": "ok", **cal}

    def pick(self) -> dict:
        ladder = [r for r in self.results() if r.get("status") == "ok" and r["job"].startswith("full:") and r.get("group") == "ladder"]
        if not ladder:
            return {"status": "failed", "why": "no ladder net has a full table"}
        params = {r["net"]: r.get("params") for r in self.results() if r.get("job", "").startswith("train:") and r.get("status") == "ok"}
        best = max(ladder, key=lambda r: (r.get("families_90", 0), r.get("siege8_mean") or 0, -(params.get(r["net"]) or 0)))
        return {"status": "ok", "best": {"d": best["d"], "layers": best["layers"], "net": best["net"]}}

    # ---- summary -----------------------------------------------------------------------------
    @staticmethod
    def ci(values: list[float]) -> tuple[float, float]:
        n = len(values)
        mean = sum(values) / n
        if n < 2:
            return mean, float("nan")
        sd = math.sqrt(sum((v - mean) ** 2 for v in values) / (n - 1))
        t = T975.get(n - 1, 1.96)
        return mean, t * sd / math.sqrt(n)

    def per_net(self) -> dict[str, dict]:
        nets: dict[str, dict] = {}
        for r in self.ok().values():
            kind = r["job"].split(":")[0]
            if ":" in r["job"] and kind in ("train", "quick", "full", "fullzero"):
                nets.setdefault(r["job"].split(":", 1)[1], {})[kind] = r
        return nets

    def summary(self) -> dict:
        nets = self.per_net()
        lines = ["# Campaign summary", f"Generated {now()}.", "", "## Capacity ladder (seed 1)", "",
                 "| net | params | families >= 90 % (full, of 32) | siege 8 mean win | quick >= 90 % (of 12) | memory-zeroed >= 90 % | dec/s | ms/turn | peak RAM GB |",
                 "|---|---:|---:|---:|---:|---:|---:|---:|---:|"]

        def row(name: str, n: dict) -> str:
            tr, q, f, z = (n.get(k, {}) for k in ("train", "quick", "full", "fullzero"))
            s8 = f.get("siege8_mean")
            return (f"| {name} | {tr.get('params', '')} | {f.get('families_90', '')} | {'' if s8 is None else f'{s8:.3f}'} | {q.get('families_90', '')} | "
                    f"{z.get('families_90', '')} | {tr.get('dps', '')} | {q.get('ms_per_turn', '')} | {tr.get('peak_ram_gb', '')} |")

        for name, n in nets.items():
            if n.get("train", {}).get("group") == "ladder":
                lines.append(row(name, n))
        seeds = {k: v for k, v in nets.items() if v.get("train", {}).get("group") == "seed"}
        out: dict = {"nets": len(nets)}
        if seeds:
            lines += ["", f"## Best size, {len(seeds)} independent seeds (mean +- 95 % interval over seeds)", "",
                      "| net | families >= 90 % | siege 8 mean win | quick >= 90 % | memory-zeroed >= 90 % | dec/s | ms/turn | peak RAM GB |", "|---|---:|---:|---:|---:|---:|---:|---:|"]
            for name, n in seeds.items():
                tr, q, f, z = (n.get(k, {}) for k in ("train", "quick", "full", "fullzero"))
                lines.append(f"| {name} | {f.get('families_90', '')} | {f.get('siege8_mean') or ''} | {q.get('families_90', '')} | {z.get('families_90', '')} | {tr.get('dps', '')} | {q.get('ms_per_turn', '')} | {tr.get('peak_ram_gb', '')} |")
            cols = {"families >= 90 % (full)": [n["full"].get("families_90") for n in seeds.values() if "full" in n],
                    "siege 8 mean win": [n["full"].get("siege8_mean") for n in seeds.values() if "full" in n],
                    "quick >= 90 %": [n["quick"].get("families_90") for n in seeds.values() if "quick" in n],
                    "memory-zeroed >= 90 %": [n["fullzero"].get("families_90") for n in seeds.values() if "fullzero" in n],
                    "decisions/s": [n["train"].get("dps") for n in seeds.values() if "train" in n],
                    "ms per turn": [n["quick"].get("ms_per_turn") for n in seeds.values() if "quick" in n],
                    "peak RAM GB": [n["train"].get("peak_ram_gb") for n in seeds.values() if "train" in n]}
            lines += ["", "| metric | mean | +- 95 % | n |", "|---|---:|---:|---:|"]
            for label, vals in cols.items():
                vals = [v for v in vals if v is not None]
                if vals:
                    m, h = self.ci(vals)
                    lines.append(f"| {label} | {m:.3f} | {h:.3f} | {len(vals)} |")
                    out[label] = [round(m, 4), round(h, 4) if h == h else None]
        (self.dir / "summary.md").write_text("\n".join(lines) + "\n")
        print("\n".join(lines))
        return out

    # ---- the loop ----------------------------------------------------------------------------
    def run(self) -> int:
        threading.Thread(target=self.heartbeat, daemon=True).start()
        prior = self.ok().get("calibrate")
        if prior:
            self.calib = {k: prior[k] for k in ("sec_per_epoch", "quick_s", "dps") if k in prior}
        retries = self.cfg["campaign"].get("retries", 1)
        failed_final: set[str] = {r["job"] for r in self.results() if r.get("status") == "failed_final"}
        while True:
            jobs = self.plan()
            self.update_eta(jobs)
            ok = self.ok()
            todo = [j for j in jobs if j["job"] not in ok and j["job"] not in failed_final]
            if not todo:
                break
            job = todo[0]
            self.current, self.state = job["job"], "running"
            self.write_status()
            if self.eta_h is not None and job["kind"] in ("teach", "train") and "calibrate" in ok and not self._printed():
                print(f"[campaign] estimated duration: about {self.eta_h} h (from the calibration; +-30 %)", flush=True)
            print(f"[campaign] {now()} start {job['job']}", flush=True)
            t0 = time.time()
            for attempt in range(retries + 1):
                try:
                    res = self.execute(job)
                except Exception as e:  # noqa: BLE001 (a failing job must not end the campaign)
                    res = {"status": "failed", "why": repr(e)}
                if res.get("status") == "ok":
                    break
                print(f"[campaign] {job['job']} failed (attempt {attempt + 1})", flush=True)
            minutes = round((time.time() - t0) / 60, 1)
            if res.get("status") != "ok":
                res["status"] = "failed_final"
                failed_final.add(job["job"])
            self.append({"job": job["job"], "kind": job["kind"], "minutes": minutes, **res})
            if self.smoke and job["kind"] == "train":
                break
        self.state = "finished"
        self.current = "-"
        self.eta_h = 0
        self.write_status()
        print(f"[campaign] finished: {self.dir}/summary.md and results.jsonl", flush=True)
        return 0

    def _printed(self) -> bool:
        flag = self.dir / ".eta-printed"
        if flag.exists():
            return True
        flag.write_text(now())
        return False


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["run", "status", "summary"])
    ap.add_argument("--config", default=str(M5 / "campaign.toml"))
    ap.add_argument("--smoke", type=float, default=None, help="dry run: smoke teacher data and MINUTES of the first training")
    ap.add_argument("--no-calibrate", action="store_true")
    a = ap.parse_args()
    cfg = tomllib.loads(Path(a.config).read_text())
    cfg["_no_calibrate"] = a.no_calibrate
    cfg["_path"] = a.config
    c = Campaign(cfg, a.smoke)
    if a.cmd == "status":
        print((c.dir / "status.json").read_text() if (c.dir / "status.json").exists() else "no campaign yet")
        return 0
    if a.cmd == "summary":
        c.summary()
        return 0
    return c.run()


if __name__ == "__main__":
    sys.exit(main())
