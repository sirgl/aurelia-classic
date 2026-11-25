"""The summary of night 4: per run the validation curve, the held-out per-family tables at the checkpoints, the siege-group mean and the paired
base-vs-boost comparison by seed. Used by campaign4.py (job `summary`) and runnable alone:

    python -I tools/m5/summary4.py LAB_DIR [--smoke]      writes LAB_DIR/runs/campaign4[-smoke]/summary.md and summary.json

Held-out = instances 80-99 of the kept scenes, the gate binary of this commit, full table (every pool family) and quick table (12 families,
ARMY_PLAY_ON_CAP=1). The reference is the honest N175 (the M4's rows, lab-seed4/ref). A play is paired by (family, era, instance, attempt).
"""
from __future__ import annotations

import json
import random
import statistics
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]


def read_jsonl(p: Path) -> list[dict]:
    out = []
    if p.exists():
        for line in p.read_text().splitlines():
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return out


def key(r: dict) -> tuple:
    return (r["family"], r.get("era"), r.get("instance"), r.get("attempt"))


def signature(r: dict) -> tuple:
    return (bool(r["win"]), r.get("rounds"), json.dumps(r.get("measures"), sort_keys=True))


def rows_equal(ref: Path, new: Path) -> dict:
    """Play for play: the same keys, and for each the same win, rounds and measures."""
    a = {key(r): signature(r) for r in read_jsonl(ref)}
    b = {key(r): signature(r) for r in read_jsonl(new)}
    diff = [k for k in a if k in b and a[k] != b[k]]
    return {"n_ref": len(a), "n_new": len(b), "equal": sum(1 for k in a if k in b and a[k] == b[k]), "different": len(diff),
            "missing": sum(1 for k in a if k not in b), "extra": sum(1 for k in b if k not in a), "examples": [list(map(str, k)) for k in diff[:5]]}


def fam_of(r: dict) -> str:
    f = r["family"]
    if f == "prey" and (r.get("info") or {}).get("context") == "land":
        return "prey-land"
    return f


def rates(rows: list[dict]) -> dict:
    per: dict = {}
    for r in rows:
        w, n = per.get(fam_of(r), (0, 0))
        per[fam_of(r)] = (w + bool(r["win"]), n + 1)
    return {f: w / n for f, (w, n) in sorted(per.items())}


def mean_of(rt: dict, fams) -> float | None:
    v = [rt[f] for f in fams if f in rt]
    return sum(v) / len(v) if v else None


def pct(x) -> str:
    return "-" if x is None else f"{100 * x:.1f}"


def paired_boot(a_rows: list[dict], b_rows: list[dict], fams, reps: int = 2000, seed: int = 7) -> tuple | None:
    """Mean over `fams` of the family win rates: B minus A, a paired bootstrap over the plays (resampled inside each family), 95 % interval, points."""
    fams = [f for f in fams]
    A: dict = {}
    for r in a_rows:
        A[key(r)] = bool(r["win"])
    per: dict = {f: [] for f in fams}
    for r in b_rows:
        if fam_of(r) in per and key(r) in A:
            per[fam_of(r)].append(int(bool(r["win"])) - int(A[key(r)]))
    per = {f: v for f, v in per.items() if v}
    if not per:
        return None
    rng = random.Random(seed)
    point = sum(sum(v) / len(v) for v in per.values()) / len(per)
    ds = []
    for _ in range(reps):
        ds.append(sum(sum(rng.choice(v) for _ in v) / len(v) for v in per.values()) / len(per))
    ds.sort()
    return 100 * point, 100 * ds[int(0.025 * reps)], 100 * ds[int(0.975 * reps)]


def specs_families(full_fams: list[str]) -> dict:
    """The sets of the gate: the 32 (g110_main's mean), and the families new to RL (g110_new's mean)."""
    out = {}
    for name, label in (("g110_main", "mean32"), ("g110_new", "new")):
        p = REPO / "tools" / "gatestats" / "specs" / f"{name}.json"
        if not p.exists():
            continue
        ex = {e.split("/")[-1] for e in json.loads(p.read_text())["mean"]["exclude"]}
        out[label] = [f for f in full_fams if f not in ex]
    return out


def write_summary(lab: Path, cdir: Path, cfg: dict, smoke: bool) -> None:
    D, R = lab / "data", lab / "runs"
    night = dict(cfg["night"])
    data = dict(cfg["data"])
    if smoke:
        s = dict(cfg["smoke"])
        night.update({k: v for k, v in s.items() if k not in data and k not in ("families", "deadline_hours")})
        data.update({k: v for k, v in s.items() if k in data})
    group = [f for f in data["siege_group"].split(",") if f]
    ref_full = read_jsonl(D / "ref" / "full-N175h.rows.jsonl")
    ref_quick = read_jsonl(D / "ref" / "quick-N175h.rows.jsonl")
    if smoke:
        fam_smoke = set(cfg["smoke"]["families"].split(","))
        ref_full = [r for r in ref_full if r["family"] in fam_smoke]
        ref_quick = [r for r in ref_quick if r["family"] in fam_smoke]
    rf = rates(ref_full)
    fams = sorted(rf)
    sets = specs_families(fams)
    results = read_jsonl(cdir / "results.jsonl")
    last = {}
    for r in results:
        last[r["job"]] = r
    ckpts = [int(u) for u in night["table_ckpts"]]
    out: list[str] = []
    js: dict = {"runs": {}, "pairs": {}}
    P = out.append

    P(f"# Night 4: long RL on the honest input {'(SMOKE)' if smoke else ''}\n")
    ver = cdir / "versions.json"
    if ver.exists():
        v = json.loads(ver.read_text())
        P(f"Machine: {v.get('chip')}, {v.get('cores_perf')} P + {v.get('cores_eff')} E cores, {v.get('ram_gb')} GB, macOS {v.get('macos')}, MLX {v.get('mlx')}; commit {v.get('git_head')}, platform {v.get('platform_head')}.")
    th = cdir / "threads.json"
    if th.exists():
        t = json.loads(th.read_text())
        P(f"Actor threads: **{t['threads']}** (the full N175 table: {t['seconds']} s; rows equal across thread counts: {t.get('rows_equal_across_threads', {}).get('different') == 0}).")
    san = last.get("sanity")
    if san:
        P(f"P0 sanity: **{san.get('status')}**; " + "; ".join(f"{k}: {x['equal']}/{x['n_ref']} equal, {x['different']} different" for k, x in (san.get('sanity') or {}).items()))
    P(f"\nReference = honest N175 (the M4's rows). Siege group = {', '.join(group)}. `mean47` = mean of the family win rates over the pool, `mean32` = the 32 of gate line 3, `new` = the families new to RL.\n")

    # ---- the jobs -------------------------------------------------------------------------------------------------
    P("## Jobs\n\n| job | status | est min | actual min |\n|---|---|---|---|")
    for r in results:
        P(f"| {r['job']} | {r['status']} | {r.get('est_min', '')} | {r.get('minutes', '')} |")

    # ---- the runs -------------------------------------------------------------------------------------------------
    tables: dict = {}
    run_names = [f"{a}-s{s}" for s in night["seeds"] for a in night["arms"]]
    for run in run_names:
        rd = R / f"rl-{run}"
        log = read_jsonl(rd / "log.jsonl")
        vals = [r for r in log if "validation" in r]
        ups = [r for r in log if "update_s" in r]
        ent: dict = {"updates": len(ups), "validation": {}}
        P(f"\n## Run {run}\n")
        if not ups:
            P("not run.")
            js["runs"][run] = ent
            continue
        lam = ups[0].get("lambda")
        P(f"{len(ups)} updates, lambda {lam}, median update {statistics.median(r['update_s'] for r in ups):.1f} s, MLX peak {max(r.get('peak_gb', 0) for r in ups):.1f} GB, "
          f"plays with a refused sampled action per update (median) {statistics.median(r.get('fallback_plays', 0) for r in ups):.0f}; "
          f"parity checks over 1e-2: {sum((r.get('parity') or {}).get('over_1e-2', 0) for r in ups)}.\n")
        P("Validation (instances 60-79, greedy, in the loop):\n\n| update | mean47 | siege group | win rate of the training plays (mean of the last 10 updates) |\n|---|---|---|---|")
        for r in vals:
            u = r["validation"]
            near = [x["win_rate"] for x in ups if u - 9 <= x["update"] <= max(u, 1)] if u else []
            sg = mean_of(r["rates"], group)
            P(f"| {u} | {pct(r['family_mean'])} | {pct(sg)} | {pct(sum(near) / len(near)) if near else '-'} |")
            ent["validation"][u] = {"family_mean": r["family_mean"], "siege_group": sg}
        ent["held_out"] = {}
        held: dict = {}
        for u in ckpts:
            td = R / "tables" / f"{run}-u{u:04d}"
            if (td / "done").exists():
                held[u] = (read_jsonl(td / "full" / "rows.jsonl"), read_jsonl(td / "quick" / "rows.jsonl"))
        tables[run] = held
        if held:
            P("\nHeld-out (80-99), full table:\n")
            hdr = "| family | N175 |" + "".join(f" u{u} |" for u in held)
            P(hdr + "\n|---|---|" + "---|" * len(held))
            hr = {u: rates(h[0]) for u, h in held.items()}
            for name, fs in (("**siege group**", group), ("**mean47**", fams), ("**mean32**", sets.get("mean32", [])), ("**new to RL**", sets.get("new", []))):
                P(f"| {name} | {pct(mean_of(rf, fs))} |" + "".join(f" {pct(mean_of(hr[u], fs))} |" for u in held))
            qf = [f for f in data["quick_families"].split(",") if f]
            qr = {u: rates(h[1]) for u, h in held.items()}
            P(f"| **quick mean ({len(qf)})** | {pct(mean_of(rates(ref_quick), qf))} |" + "".join(f" {pct(mean_of(qr[u], qf))} |" for u in held))
            for f in fams:
                P(f"| {f} | {pct(rf.get(f))} |" + "".join(f" {pct(hr[u].get(f))} |" for u in held))
            for u in held:
                ent["held_out"][u] = {"mean47": mean_of(hr[u], fams), "siege_group": mean_of(hr[u], group), "mean32": mean_of(hr[u], sets.get("mean32", [])), "new": mean_of(hr[u], sets.get("new", [])),
                                      "rates": hr[u]}
            g = last.get(f"tables-{run}", {}).get("tables", {})
            if g:
                P("\nGate G1.10h lines (gatestats against the honest N175, tolerance 8 points): DROP families per checkpoint\n")
                P("| update | main (no harm, mean32) | new (mean above N175) | third |\n|---|---|---|---|")
                for u, e in g.items():
                    gg = e.get("gate", {}) if isinstance(e, dict) else {}
                    cell = lambda s: ("error" if "error" in gg.get(s, {}) else f"pass={gg.get(s, {}).get('pass')}" + (f" DROP {', '.join(gg[s]['drop'])}" if gg.get(s, {}).get("drop") else ""))
                    P(f"| {u} | {cell('g110_main')} | {cell('g110_new')} | {cell('g110_third')} |")
        js["runs"][run] = ent

    # ---- the pairs ----------------------------------------------------------------------------------------------------
    if "boost" in night["arms"] and "base" in night["arms"]:
        P("\n## Paired comparison: boost against base, by seed (held-out full table, the same scenes)\n")
        P("The delta is boost minus base in points; the interval is a paired bootstrap over the plays of the family set (2000 resamples).\n")
        P("| seed | update | siege group base | siege group boost | delta [95 %] | mean47 delta [95 %] | mean32 delta [95 %] |\n|---|---|---|---|---|---|---|")
        pooled: dict = {u: [] for u in ckpts}
        for s in night["seeds"]:
            b, x = tables.get(f"base-s{s}", {}), tables.get(f"boost-s{s}", {})
            for u in ckpts:
                if u not in b or u not in x:
                    P(f"| {s} | {u} | - | - | pair incomplete | | |")
                    continue
                pooled[u].append((b[u][0], x[u][0]))
                rb, rx = rates(b[u][0]), rates(x[u][0])
                d = paired_boot(b[u][0], x[u][0], group)
                d47 = paired_boot(b[u][0], x[u][0], fams)
                d32 = paired_boot(b[u][0], x[u][0], sets.get("mean32", []))
                f = lambda t: "-" if t is None else f"{t[0]:+.1f} [{t[1]:+.1f}, {t[2]:+.1f}]"
                P(f"| {s} | {u} | {pct(mean_of(rb, group))} | {pct(mean_of(rx, group))} | {f(d)} | {f(d47)} | {f(d32)} |")
                js["pairs"][f"s{s}-u{u}"] = {"siege_group_delta": d, "mean47_delta": d47, "mean32_delta": d32}
        if any(len(v) > 1 for v in pooled.values()):
            P("\nSeeds pooled (every seed's plays resampled together):\n\n| update | seeds | siege group delta [95 %] | mean47 delta [95 %] |\n|---|---|---|---|")
            for u, prs in pooled.items():
                if len(prs) < 2:
                    continue
                a_rows = [dict(r, instance=(i, r.get("instance"))) for i, (a, _) in enumerate(prs) for r in a]
                b_rows = [dict(r, instance=(i, r.get("instance"))) for i, (_, b) in enumerate(prs) for r in b]
                for r in a_rows + b_rows:
                    r["instance"] = json.dumps(r["instance"])
                d = paired_boot(a_rows, b_rows, group)
                d47 = paired_boot(a_rows, b_rows, fams)
                f = lambda t: "-" if t is None else f"{t[0]:+.1f} [{t[1]:+.1f}, {t[2]:+.1f}]"
                P(f"| {u} | {len(prs)} | {f(d)} | {f(d47)} |")
        P("\nPer family, held-out at the last checkpoint, boost minus base (points), seeds side by side:\n")
        last_u = ckpts[-1]
        P("| family | " + " | ".join(f"s{s}" for s in night["seeds"]) + " |\n|---|" + "---|" * len(night["seeds"]))
        for f in fams:
            cells = []
            for s in night["seeds"]:
                b, x = tables.get(f"base-s{s}", {}), tables.get(f"boost-s{s}", {})
                cells.append(f"{100 * (rates(x[last_u][0]).get(f, 0) - rates(b[last_u][0]).get(f, 0)):+.1f}" if last_u in b and last_u in x else "-")
            P(f"| {f} | " + " | ".join(cells) + " |")
    P("\nNothing here is a verdict: the gate lines of G1.10h are read from the tables above by the coordinator.")
    (cdir / "summary.md").write_text("\n".join(out) + "\n")
    (cdir / "summary.json").write_text(json.dumps(js, indent=1))


if __name__ == "__main__":
    lab = Path(sys.argv[1])
    smoke = "--smoke" in sys.argv
    cfg = tomllib.loads((REPO / "tools" / "m5" / "campaign4.toml").read_text())
    write_summary(lab, lab / "runs" / ("campaign4-smoke" if smoke else "campaign4"), cfg, smoke)
