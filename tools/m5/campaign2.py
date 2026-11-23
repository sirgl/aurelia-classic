#!/usr/bin/env python3
"""Stage 2 of the M5 kit: the MBf recipe, the MLX speed step, paper-grade seeds, wider nets. Resumable like stage 1.

    campaign2.py run [--config campaign2.toml]
    campaign2.py plan | status | summary

Order: (1) the MBf recipe once at d 64 / 3 layers (A0 -> M0, B -> B1, init, MBf, quick / full / memory-zeroed evals),
(2) MLX speed: pinned vs newest MLX x f32 / TF32 / bf16 mixed, 200 steps each, (3) 5 seeds at the best stage-1 ladder
size with the MBf recipe, (4) d 192 and d 256, (5) summary. Every finished job appends a line to
lab/runs/campaign2/results.jsonl; a job with an "ok" line is skipped after a crash or a reboot.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import subprocess
import sys
import threading
import time
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from campaign import LAB, M5, REPO, SIEGE8, Campaign, now  # noqa: E402

M4_SECONDS_PER_DPS = 2850.0   # the M4's MB d 64 decisions/s (2 700-3 000)
PRECISIONS = {"f32": {"tf32": "0", "bf16": False}, "tf32": {"tf32": "1", "bf16": False}, "bf16": {"tf32": "0", "bf16": True}}
M4_REF = {"assault-heal": 0.65, "planned-assault": 0.21, "raid": 0.98, "quick_families_90_of_13": 8, "hold_acc": 0.839,
          "assault-heal_zeroed": 0.39, "full_families_90_of_32": 17}


def size_factor(d: int, layers: int) -> float:
    return (d / 64) ** 1.5 * (layers / 3) ** 0.9


class Stage2(Campaign):
    def __init__(self, cfg: dict):
        self.cfg = cfg
        self.smoke = None
        self.dir = LAB / "runs" / "campaign2"
        self.results_path = self.dir / "results.jsonl"
        self.current, self.total, self.eta_h, self.state = "-", 0, None, "starting"
        self.t0 = time.time()
        base = tomllib.loads((M5 / "campaign.toml").read_text())["train"]
        s = cfg["stage2"]
        self.plays = s["plays"] or base["plays"]
        self.chunk = s["chunk_min"] or base["chunk_min"]
        self.s1 = self._stage1()

    # ---- stage 1 ----------------------------------------------------------------------------
    def _stage1(self) -> dict:
        path = LAB / self.cfg["stage2"]["stage1_results"]
        rows = []
        if path.exists():
            for line in path.read_text().splitlines():
                try:
                    rows.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
        ok = [r for r in rows if r.get("status") == "ok"]
        info: dict = {"found": bool(ok)}
        cal = next((r for r in ok if r["job"] == "calibrate"), None)
        info["calibrate"] = cal
        pick = next((r for r in ok if r["job"] == "pick"), None)
        if pick:
            info["best"] = pick["best"]
        else:
            ladder = [r for r in ok if r["job"].startswith("full:") and r.get("group") == "ladder"]
            params = {r["net"]: r.get("params") for r in ok if r["job"].startswith("train:")}
            if ladder:
                b = max(ladder, key=lambda r: (r.get("families_90", 0), r.get("siege8_mean") or 0, -(params.get(r["net"]) or 0)))
                info["best"] = {"d": b["d"], "layers": b["layers"], "net": b["net"]}
        info["minutes"] = {r["job"]: r.get("minutes") for r in ok}
        info["dps"] = (cal or {}).get("dps")
        return info

    def best(self) -> dict:
        return self.s1.get("best") or {"d": 64, "layers": 3, "net": "(stage 1 missing: placeholder)"}

    # ---- the job list ------------------------------------------------------------------------
    def chain(self, tag: str, d: int, layers: int, seed: int, epochs: float, group: str, enc: str, evals: tuple = ("quick", "full", "fullzero")) -> list[dict]:
        base = {"tag": tag, "d": d, "layers": layers, "seed": seed, "epochs": epochs, "group": group, "enc": enc}
        jobs = [{"job": f"a0:{tag}", "kind": "a0", **base}, {"job": f"m0:{tag}", "kind": "m0", **base},
                {"job": f"initmb:{tag}", "kind": "initmb", **base}, {"job": f"mbf:{tag}", "kind": "mbf", **base}]
        for e in evals:
            jobs.append({"job": f"{e}:mbf-{tag}", "kind": e, "net": f"mbf-{tag}", **base})
        return jobs

    def encoder(self, enc: str, seed: int) -> list[dict]:
        base = {"enc": enc, "seed": seed, "d": 64, "layers": 3, "group": "encoder"}
        return [{"job": f"b:{enc}", "kind": "b", **base}, {"job": f"b1:{enc}", "kind": "b1", **base}]

    def plan(self) -> list[dict]:
        c = self.cfg
        s1, sp, s3, s4 = c["step1"], c["speed"], c["step3"], c["step4"]
        jobs: list[dict] = [{"job": "teach", "kind": "teach"}, {"job": "estimate", "kind": "estimate"}]
        # (1) the MBf recipe once, on the baseline numerics (pinned MLX, f32, TF32 off)
        tag1 = f"s2-d{s1['d']}-l{s1['layers']}"
        jobs += self.encoder("enc", 1)
        jobs += self.chain(tag1, s1["d"], s1["layers"], s1["seed"], s1["mbf_epochs"], "step1", "enc", evals=())
        jobs += [{"job": f"quick:m0-{tag1}", "kind": "quick", "net": f"m0-{tag1}", "tag": tag1, "group": "step1", "d": s1["d"], "layers": s1["layers"], "seed": s1["seed"]},
                 {"job": f"quick:init-{tag1}", "kind": "quick", "net": f"init-{tag1}", "tag": tag1, "group": "step1", "d": s1["d"], "layers": s1["layers"], "seed": s1["seed"]},
                 {"job": f"same:{tag1}", "kind": "same", "tag": tag1}]
        for e in ("quick", "quickzero", "full", "fullzero"):
            jobs.append({"job": f"{e}:mbf-{tag1}", "kind": e, "net": f"mbf-{tag1}", "tag": tag1, "group": "step1", "d": s1["d"], "layers": s1["layers"], "seed": s1["seed"]})
        jobs.append({"job": f"memnorms:{tag1}", "kind": "memnorms", "tag": tag1})
        # (2) MLX speed
        jobs.append({"job": "speed-setup", "kind": "speed-setup"})
        for venv in ("pinned", "new"):
            for prec in PRECISIONS:
                jobs.append({"job": f"speed:{venv}-{prec}", "kind": "speed", "venv": venv, "prec": prec})
        jobs.append({"job": "speed-pick", "kind": "speed-pick"})
        # (3) seeds at the best stage-1 size
        b = self.best()
        for seed in s3["seeds"]:
            enc = f"enc-s{seed}" if s3["seed_encoders"] else "enc"
            if s3["seed_encoders"]:
                jobs += self.encoder(enc, seed)
            jobs += self.chain(f"s3-d{b['d']}-l{b['layers']}-s{seed}", b["d"], b["layers"], seed, s3["mbf_epochs"], "seeds", enc)
        # (4) wider nets
        for w in s4["wide"]:
            jobs += self.chain(f"w-d{w['d']}-l{w['layers']}", w["d"], w["layers"], w["seed"], s4["mbf_epochs"], "wide", "enc")
        jobs.append({"job": "summary", "kind": "summary"})
        return jobs

    # ---- numerics chosen by the speed step -----------------------------------------------------
    def prec(self) -> dict:
        p = self.dir / "speed.json"
        if p.exists():
            j = json.loads(p.read_text())
            return {"py": j["py"], "tf32": j["tf32"], "bf16": j["bf16"], "label": j["label"]}
        return {"py": None, "tf32": "0", "bf16": False, "label": "pinned-f32 (baseline)"}

    def tenv(self, d: int, layers: int, seed: int, epochs: float, lr: float, use_prec: bool = True) -> dict:
        p = self.prec() if use_prec else {"py": None, "tf32": "0", "bf16": False}
        e = {"M5_D": d, "M5_LAYERS": layers, "M5_SEED": seed, "M5_EPOCHS": epochs, "M5_LR": lr, "M5_PLAYS": self.plays,
             "M5_CHUNK_MIN": self.chunk, "MLX_ENABLE_TF32": p["tf32"]}
        if p["py"]:
            e["M5_PY"] = p["py"]
        if p["bf16"]:
            e["M5_BF16"] = "1"
        return {k: str(v) for k, v in e.items()}

    # ---- estimates -----------------------------------------------------------------------------
    def unit_s(self) -> float:
        """Seconds of one MB epoch at d 64 / 3 layers on this machine (stage 1's calibration)."""
        cal = self.s1.get("calibrate") or {}
        u = cal.get("sec_per_epoch") or (2.13e6 / (cal.get("dps") or 5000))
        return float(u) / (self.gain if hasattr(self, "gain") else 1.0)

    def quick_s(self) -> float:
        m = self.s1["minutes"].get("quick:d64-l3")
        cal = self.s1.get("calibrate") or {}
        return float(m) * 60 if m else float(cal.get("quick_s") or 400)

    def est_job_s(self, j: dict) -> float:
        k, e = j["kind"], self.cfg["estimate"]
        U = self.unit_s()
        f = size_factor(j.get("d", 64), j.get("layers", 3))
        if k == "a0":
            return e["a0"] * U * f
        if k == "m0":
            return e["m0"] * U * f
        if k == "b":
            return e["b"] * U
        if k == "b1":
            return e["b1"] * U
        if k == "mbf":
            return j["epochs"] * U * f
        if k in ("quick", "quickzero"):
            return self.quick_s() * f ** 0.5
        if k in ("full", "fullzero"):
            return 3 * self.quick_s() * f ** 0.5
        if k == "speed":
            return 6 * 60
        if k == "speed-setup":
            return 5 * 60
        if k in ("initmb", "memnorms"):
            return 3 * 60
        return 0.0

    def estimate(self, jobs: list[dict]) -> float:
        ok = self.ok()
        s = sum(self.est_job_s(j) for j in jobs if j["job"] not in ok)
        self.eta_h = round(s / 3600, 1)
        self.total = len(jobs)
        return s

    def breakdown(self, jobs: list[dict]) -> dict:
        ok = self.ok()
        out: dict = {}
        for j in jobs:
            if j["job"] not in ok:
                g = j.get("group") or ("speed" if j["kind"].startswith("speed") else j["kind"])
                out[g] = out.get(g, 0.0) + self.est_job_s(j) / 3600
        return {k: round(v, 1) for k, v in out.items()}

    # ---- metrics ---------------------------------------------------------------------------------
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
        self.dir.mkdir(parents=True, exist_ok=True)
        table = self.dir / f"table-{run}.json"
        if self.sh([M5 / "table.sh", table, f"{net}={rows_path}"]) == 0 and table.exists():
            t = json.loads(table.read_text())
            fam = [r for r in t["rows"] if net in r["rates"]]
            out["families"] = len(fam)
            out["families_90"] = sum(1 for r in fam if r["pass"].get(net))
            sg = [r["rates"][net] for r in fam if r["family"] in SIEGE8]
            out["siege8_mean"] = round(sum(sg) / len(sg), 4) if sg else None
            out["win_by_family"] = {r["family"]: round(r["rates"][net], 3) for r in fam}
        return out

    def speed_metrics(self, name: str) -> dict:
        cur = LAB / "runs" / name / "curves.jsonl"
        rows = [json.loads(l) for l in cur.read_text().splitlines() if l.strip().startswith("{")] if cur.exists() else []
        if not rows:
            return {}
        late = [r for r in rows if r["step"] > 100]
        loss = [r["loss"] for r in rows]
        hold = [r["hold"]["acc_both"] for r in rows if "hold" in r]
        m = {"steps": rows[-1]["step"], "dps_mean": round(rows[-1].get("dps_mean", 0)),
             "dps_steady": round(sum(r["dps"] for r in late) / len(late)) if late else None,
             "loss_late": round(sum(r["loss"] for r in late) / len(late), 5) if late else None,
             "loss_curve": [round(x, 4) for x in loss], "nan": any(not math.isfinite(x) for x in loss),
             "hold_acc_both": round(hold[-1], 4) if hold else None}
        m.update({k: v for k, v in self.train_metrics(name).items() if k == "peak_ram_gb"})
        return m

    # ---- job execution -----------------------------------------------------------------------------
    def need(self, *paths: str) -> str | None:
        miss = [p for p in paths if not (LAB / "runs" / p).exists()]
        return f"missing {miss}" if miss else None

    def train(self, job: dict, name: str, kind: str, env: dict, extra: list[str] | None = None) -> dict:
        rc = self.sh([M5 / "train2.sh", name, kind, *(extra or [])], env)
        return {"status": "ok" if rc == 0 else "failed", "net": name, "d": job.get("d"), "layers": job.get("layers"), "seed": job.get("seed"),
                "group": job.get("group"), **self.train_metrics(name)}

    def execute(self, job: dict) -> dict:
        k, rc_, cfg = job["kind"], self.cfg["recipe"], self.cfg["stage2"]
        R = LAB / "runs"
        if k == "teach":
            return {"status": "ok" if self.sh([M5 / "teach.sh"]) == 0 else "failed"}
        if k == "estimate":
            jobs = self.plan()
            s = self.estimate(jobs)
            msg = (f"estimated duration of stage 2: about {self.eta_h} h (+-40 %), by part (h): {self.breakdown(jobs)}"
                   f"; stage-1 unit {self.unit_s():.0f} s per MB epoch, best ladder size d{self.best()['d']}/l{self.best()['layers']}"
                   + ("" if self.s1["found"] else "; STAGE 1 RESULTS NOT FOUND, placeholder size"))
            print(f"[campaign2] {msg}", flush=True)
            return {"status": "ok", "eta_h": self.eta_h, "breakdown": self.breakdown(jobs), "msg": msg}
        if k in ("a0", "m0", "mbf", "initmb", "b", "b1"):
            d, layers, seed = job["d"], job["layers"], job["seed"]
            tag = job.get("tag")
            enc = job.get("enc")
            use_prec = job["group"] != "step1" and job["group"] != "encoder"
            if k == "a0":
                return self.train(job, f"a0-{tag}", "a", self.tenv(d, layers, seed, rc_["a_epochs"], rc_["a_lr"], False))
            if k == "b":
                return self.train(job, f"b-{enc}", "b", self.tenv(64, 3, seed, rc_["b_epochs"], rc_["b_lr"], False))
            if k == "b1":
                if (why := self.need(f"b-{enc}/weights.safetensors")):
                    return {"status": "failed", "why": why}
                e = self.tenv(64, 3, seed, rc_["b1_epochs"], rc_["b1_lr"], False)
                e["M5_INIT"] = str(R / f"b-{enc}" / "weights.safetensors")
                return self.train(job, f"b1-{enc}", "b", e, ["--up", "siege", "--up-factor", "2"])
            if k == "m0":
                if (why := self.need(f"a0-{tag}/weights.safetensors")):
                    return {"status": "failed", "why": why}
                e = self.tenv(d, layers, seed, rc_["m_epochs"], rc_["m_lr"], use_prec)
                e["M5_INIT"] = str(R / f"a0-{tag}" / "weights.safetensors")
                return self.train(job, f"m0-{tag}", "m", e)
            if k == "initmb":
                if (why := self.need(f"m0-{tag}/weights.safetensors", f"b1-{enc}/weights.safetensors")):
                    return {"status": "failed", "why": why}
                out = R / f"init-{tag}"
                rc = subprocess.run([str(REPO / "tools/res/res"), "run", "--cpu", "1", "--name", f"initmb-{tag}", "--", os.environ.get("M5_PY") or str(REPO / ".venv-m5/bin/python"), "-I",
                                     str(M5 / "py/make_init_mb_from.py"), str(R / f"m0-{tag}/weights.safetensors"), str(R / f"b1-{enc}/weights.safetensors"), str(out),
                                     "--d", str(d), "--layers", str(layers)], env=dict(os.environ, AURELIA_LAB=str(LAB)), cwd=REPO).returncode
                return {"status": "ok" if rc == 0 and (out / "army-net.bin").exists() else "failed", "net": f"init-{tag}"}
            if k == "mbf":
                if (why := self.need(f"init-{tag}/weights.safetensors")):
                    return {"status": "failed", "why": why}
                e = self.tenv(d, layers, seed, job["epochs"], rc_["mbf_lr"], use_prec)
                e["M5_INIT"] = str(R / f"init-{tag}" / "weights.safetensors")
                return self.train(job, f"mbf-{tag}", "mb", e, ["--curriculum", "999:1"])
        if k in ("quick", "quickzero", "full", "fullzero"):
            net = job["net"]
            if (why := self.need(f"{net}/army-net.bin")):
                return {"status": "failed", "why": why}
            run = {"quick": f"evq-{net}", "quickzero": f"evqz-{net}", "full": f"evf-{net}", "fullzero": f"evfz-{net}"}[k]
            hold_to = cfg["quick_hold_to"] if k.startswith("quick") else cfg["full_hold_to"]
            extra = ["ARMY_NET_ZERO_MEMORY=1"] if k.endswith("zero") else []
            rc = self.sh([M5 / "eval.sh", "quick" if k.startswith("quick") else "full", net, run, *extra], {"M5_HOLD_TO": str(hold_to)})
            if rc != 0:
                return {"status": "failed"}
            return {"status": "ok", "net": net, "group": job.get("group"), "d": job.get("d"), "layers": job.get("layers"), "seed": job.get("seed"), **self.eval_metrics(run, net)}
        if k == "same":
            a, b = R / f"evq-m0-{job['tag']}/rows.jsonl", R / f"evq-init-{job['tag']}/rows.jsonl"
            if not a.exists() or not b.exists():
                return {"status": "failed", "why": "quick evals of M0 / init missing"}
            r = subprocess.run([sys.executable, str(M5 / "py/same_plays.py"), str(a), str(b)], capture_output=True, text=True)
            print(r.stdout, flush=True)
            return {"status": "ok", "identical": r.returncode == 0, "line": r.stdout.splitlines()[0] if r.stdout else ""}
        if k == "memnorms":
            tag = job["tag"]
            if (why := self.need(f"m0-{tag}/weights.safetensors", f"mbf-{tag}/weights.safetensors")):
                return {"status": "failed", "why": why}
            out = self.dir / f"mem-norms-{tag}.json"
            rc = subprocess.run([str(REPO / "tools/res/res"), "run", "--cpu", "2", "--name", "memnorms", "--", str(REPO / ".venv-m5/bin/python"), "-I", str(M5 / "py/mem_norms.py"), str(out),
                                 f"M0={R / f'm0-{tag}/weights.safetensors'}", f"MBf={R / f'mbf-{tag}/weights.safetensors'}"], env=dict(os.environ, AURELIA_LAB=str(LAB)), cwd=REPO).returncode
            return {"status": "ok" if rc == 0 else "failed"}
        if k == "speed-setup":
            rc = self.sh([M5 / "setup2.sh"])
            v = json.loads((R / "campaign2/versions.json").read_text()) if (R / "campaign2/versions.json").exists() else {}
            return {"status": "ok", **v}
        if k == "speed":
            return self.speed(job)
        if k == "speed-pick":
            return self.speed_pick()
        if k == "summary":
            return {"status": "ok", **self.summary()}
        raise ValueError(k)

    # ---- step 2: speed ----------------------------------------------------------------------------
    def versions(self) -> dict:
        p = LAB / "runs/campaign2/versions.json"
        return json.loads(p.read_text()) if p.exists() else {}

    def speed(self, job: dict) -> dict:
        v = self.versions()
        if job["venv"] == "new" and not v.get("ok"):
            return {"status": "ok", "skipped": True, "why": v.get("note") or "newest MLX not installed"}
        py = v["new_py"] if job["venv"] == "new" else str(REPO / ".venv-m5/bin/python")
        name = f"speed-{job['venv']}-{job['prec']}"
        subprocess.run(["rm", "-rf", str(LAB / "runs" / name), str(LAB / "logs" / f"{name}.log")])
        p = PRECISIONS[job["prec"]]
        sp, s1 = self.cfg["speed"], self.cfg["step1"]
        env = {"M5_PY": py, "MLX_ENABLE_TF32": p["tf32"], "M5_D": "64", "M5_LAYERS": "3", "M5_SEED": "1", "M5_EPOCHS": "2", "M5_PLAYS": str(self.plays),
               "M5_MAX_STEPS": str(sp["steps"]), "M5_LOG_EVERY": "10", "M5_EVAL_EVERY": "100000", "M5_CHUNK_MIN": "60"}
        if p["bf16"]:
            env["M5_BF16"] = "1"
        rc = self.sh([M5 / "train2.sh", name, "mb"], env)
        m = self.speed_metrics(name)
        return {"status": "ok" if rc == 0 and m else "failed", "venv": job["venv"], "prec": job["prec"], "py": py, "tf32": p["tf32"], "bf16": p["bf16"],
                "mlx": v.get("new_mlx" if job["venv"] == "new" else "pinned_mlx"), **m}

    def speed_pick(self) -> dict:
        sp = self.cfg["speed"]
        rows = {r["job"].split(":", 1)[1]: r for r in self.ok().values() if r["job"].startswith("speed:") and not r.get("skipped")}
        ref = rows.get("pinned-f32")
        if not ref or not ref.get("dps_steady"):
            return {"status": "failed", "why": "the reference combination (pinned MLX, f32) has no result"}
        table = {}
        best, best_dps = "pinned-f32", ref["dps_steady"]
        for name, r in rows.items():
            if not r.get("dps_steady") or r.get("loss_late") is None:
                continue
            ld = abs(r["loss_late"] - ref["loss_late"]) / ref["loss_late"]
            ad = abs((r.get("hold_acc_both") or 0) - (ref.get("hold_acc_both") or 0))
            curve = [abs(a - b) / b for a, b in zip(r["loss_curve"], ref["loss_curve"]) if b]
            correct = (not r["nan"]) and ld <= sp["loss_tol"] and ad <= sp["acc_tol"]
            table[name] = {"dps_steady": r["dps_steady"], "dps_mean": r["dps_mean"], "speedup": round(r["dps_steady"] / ref["dps_steady"], 3),
                           "loss_late": r["loss_late"], "loss_rel_diff": round(ld, 4), "loss_curve_max_rel_diff": round(max(curve), 4) if curve else None,
                           "loss_curve_mean_rel_diff": round(sum(curve) / len(curve), 4) if curve else None, "hold_acc_both": r.get("hold_acc_both"),
                           "acc_diff": round(ad, 4), "correct": correct, "peak_ram_gb": r.get("peak_ram_gb")}
            if correct and r["dps_steady"] > best_dps * (1 + sp["min_gain"]) and r["dps_steady"] > best_dps:
                best, best_dps = name, r["dps_steady"]
        chosen = rows[best]
        out = {"label": best, "py": chosen["py"], "tf32": chosen["tf32"], "bf16": chosen["bf16"], "dps_steady": best_dps, "table": table, "versions": self.versions()}
        self.dir.mkdir(parents=True, exist_ok=True)
        (self.dir / "speed.json").write_text(json.dumps(out, indent=1))
        self.gain = best_dps / ref["dps_steady"]
        jobs = self.plan()
        self.estimate(jobs)
        print(f"[campaign2] speed: keeping {best} ({self.gain:.2f}x the reference); estimate of the rest of stage 2: about {self.eta_h} h", flush=True)
        return {"status": "ok", "chosen": best, "gain": round(self.gain, 3), "eta_h": self.eta_h, "table": table}

    # ---- summary -------------------------------------------------------------------------------------
    def nets(self, group: str) -> dict[str, dict]:
        out: dict[str, dict] = {}
        for r in self.ok().values():
            if r.get("group") == group and r.get("net") and r["kind"] in ("mbf", "quick", "quickzero", "full", "fullzero"):
                out.setdefault(r["net"], {})[r["kind"]] = r
        return out

    @staticmethod
    def win(r: dict | None, fam: str):
        return ((r or {}).get("win_by_family") or {}).get(fam)

    def summary(self) -> dict:
        L = ["# Stage 2 summary (generated by campaign2.py)", f"Generated {now()}.", ""]
        v = self.versions()
        if v:
            L += [f"MLX pinned {v.get('pinned_mlx')}; newest {v.get('new_mlx')} (installed: {v.get('ok')}); macOS {v.get('macos')} ({v.get('macos_build')}); {v.get('chip')}.", ""]
        sp = LAB / "runs/campaign2/speed.json"
        if sp.exists():
            j = json.loads(sp.read_text())
            L += ["## Step 2: MLX speed (MB d 64 / 3 layers, 200 steps, same seed)", "",
                  "| combination | decisions/s (steps 101-200) | x reference | late loss | loss diff | worst point diff | hold acc | correct |", "|---|---:|---:|---:|---:|---:|---:|---|"]
            for n, t in j["table"].items():
                L.append(f"| {n} | {t['dps_steady']} | {t['speedup']} | {t['loss_late']} | {t['loss_rel_diff']:.1%} | {t['loss_curve_max_rel_diff']:.1%} | {t['hold_acc_both']} | {t['correct']} |")
            L += ["", f"Kept for the rest of stage 2: **{j['label']}**.", ""]
        out: dict = {}
        s1 = self.nets("step1")
        if s1:
            L += ["## Step 1: the MBf recipe at d 64 / 3 layers against the M4 lab", "", "| measure | M5 | M4 (MBf) |", "|---|---:|---:|"]
            for n, d in s1.items():
                q, qz, f, fz = (d.get(k) for k in ("quick", "quickzero", "full", "fullzero"))
                L += [f"| assault-heal win (quick) | {self.win(q, 'assault-heal')} | 0.65 |", f"| planned-assault win (quick) | {self.win(q, 'planned-assault')} | 0.21 |",
                      f"| raid win (quick) | {self.win(q, 'raid')} | 0.98 |", f"| quick families >= 90 % of the best teacher | {(q or {}).get('families_90')} of {(q or {}).get('families')} | 8 of 13 |",
                      f"| assault-heal, memory zeroed | {self.win(qz, 'assault-heal')} | 0.39 |", f"| full families >= 90 % | {(f or {}).get('families_90')} of {(f or {}).get('families')} | 17 of 32 |",
                      f"| full families >= 90 %, memory zeroed | {(fz or {}).get('families_90')} | n/a |", f"| hold accuracy (train log) | {(d.get('mbf') or {}).get('hold_acc_both')} | 0.839 |"]
        seeds = self.nets("seeds")
        if seeds:
            L += ["", f"## Step 3: {len(seeds)} seeds at d{self.best()['d']}/l{self.best()['layers']}, MBf recipe, {self.cfg['step3']['mbf_epochs']} epochs (mean +- 95 % interval over seeds)", "",
                  "| net | hold acc | quick >= 90 % | full >= 90 % | siege 8 mean | assault-heal | planned-assault | raid | zeroed full >= 90 % | zeroed assault-heal (quick) | ms/turn |", "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
            cols: dict[str, list] = {k: [] for k in ("hold acc", "quick >= 90 %", "full >= 90 %", "siege 8 mean win", "assault-heal", "planned-assault", "raid", "zeroed full >= 90 %", "assault-heal memory-zeroed", "ms per turn")}
            for n, d in seeds.items():
                q, qz, f, fz, t = (d.get(k) for k in ("quick", "quickzero", "full", "fullzero", "mbf"))
                row = [(t or {}).get("hold_acc_both"), (q or {}).get("families_90"), (f or {}).get("families_90"), (f or {}).get("siege8_mean"), self.win(q, "assault-heal"),
                       self.win(q, "planned-assault"), self.win(q, "raid"), (fz or {}).get("families_90"), self.win(qz, "assault-heal"), (q or {}).get("ms_per_turn")]
                L.append(f"| {n} | " + " | ".join("" if x is None else str(x) for x in row) + " |")
                for key, x in zip(cols, row):
                    if x is not None:
                        cols[key].append(x)
            L += ["", "| metric | mean | +- 95 % | n |", "|---|---:|---:|---:|"]
            for key, vals in cols.items():
                if vals:
                    m, h = self.ci(vals)
                    L.append(f"| {key} | {m:.3f} | {h:.3f} | {len(vals)} |")
                    out[key] = [round(m, 4), round(h, 4) if h == h else None]
        wide = self.nets("wide")
        if wide:
            L += ["", "## Step 4: wider nets (MBf recipe from scratch at each width, 1 seed each)", "", "| net | params | hold acc | quick >= 90 % | full >= 90 % | siege 8 mean | assault-heal | zeroed full >= 90 % | ms/turn | dec/s |", "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
            for n, d in wide.items():
                q, f, fz, t = (d.get(k) for k in ("quick", "full", "fullzero", "mbf"))
                row = [(t or {}).get("params"), (t or {}).get("hold_acc_both"), (q or {}).get("families_90"), (f or {}).get("families_90"), (f or {}).get("siege8_mean"),
                       self.win(q, "assault-heal"), (fz or {}).get("families_90"), (q or {}).get("ms_per_turn"), (t or {}).get("dps")]
                L.append(f"| {n} | " + " | ".join("" if x is None else str(x) for x in row) + " |")
        self.dir.mkdir(parents=True, exist_ok=True)
        (self.dir / "summary.md").write_text("\n".join(L) + "\n")
        print("\n".join(L))
        return out

    # ---- the loop ------------------------------------------------------------------------------------
    def run(self) -> int:
        self.dir.mkdir(parents=True, exist_ok=True)
        threading.Thread(target=self.heartbeat, daemon=True).start()
        sp = self.dir / "speed.json"
        if sp.exists():
            ref = {r["job"]: r for r in self.ok().values()}.get("speed-pick") or {}
            self.gain = ref.get("gain", 1.0)
        retries = self.cfg["stage2"]["retries"]
        failed_final = {r["job"] for r in self.results() if r.get("status") == "failed_final"}
        while True:
            jobs = self.plan()
            self.estimate(jobs)
            ok = self.ok()
            todo = [j for j in jobs if j["job"] not in ok and j["job"] not in failed_final]
            if not todo:
                break
            job = todo[0]
            self.current, self.state = job["job"], "running"
            self.write_status()
            print(f"[campaign2] {now()} start {job['job']}  (about {self.eta_h} h left)", flush=True)
            t0 = time.time()
            for attempt in range(retries + 1):
                try:
                    res = self.execute(job)
                except Exception as e:  # noqa: BLE001 (a failing job must not end the campaign)
                    res = {"status": "failed", "why": repr(e)}
                if res.get("status") == "ok":
                    break
                print(f"[campaign2] {job['job']} failed (attempt {attempt + 1}): {res.get('why', '')}", flush=True)
            if res.get("status") != "ok":
                res["status"] = "failed_final"
                failed_final.add(job["job"])
            self.append({"job": job["job"], "kind": job["kind"], "minutes": round((time.time() - t0) / 60, 1), **res})
        self.state, self.current, self.eta_h = "finished", "-", 0
        self.write_status()
        print(f"[campaign2] finished: {self.dir}/summary.md and results.jsonl", flush=True)
        return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["run", "plan", "status", "summary"])
    ap.add_argument("--config", default=str(M5 / "campaign2.toml"))
    a = ap.parse_args()
    cfg = tomllib.loads(Path(a.config).read_text())
    c = Stage2(cfg)
    if a.cmd == "plan":
        jobs = c.plan()
        s = c.estimate(jobs)
        print(f"{len(jobs)} jobs; stage 1 found: {c.s1['found']}; best size {c.best()}; plays {c.plays}; chunk {c.chunk} min")
        for j in jobs:
            print(f"  {j['job']:34} {c.est_job_s(j) / 60:7.1f} min")
        print(f"estimated total: {s / 3600:.1f} h; by part (h): {c.breakdown(jobs)}")
        return 0
    if a.cmd == "status":
        p = c.dir / "status.json"
        print(p.read_text() if p.exists() else "no stage 2 yet")
        return 0
    if a.cmd == "summary":
        c.summary()
        return 0
    return c.run()


if __name__ == "__main__":
    sys.exit(main())
