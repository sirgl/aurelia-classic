"""Selection of the checkpoint of an rl3 run (the rule of GATES.md, G1.10, written before the runs).

    python -m aurelia_nn.rl3.select_ckpt --run RUN --kept kept-all.jsonl --gatestats DIR [--updates 50,100,150,last] [--val-from 60 --val-to 80]

Candidates: the checkpoints of 50, 100, 150 and the last update. A candidate qualifies when no family DROPs against update 0 by gatestats
`harm` on the validation rows (instances 60-79 of every pool family, greedy; tolerance 8 points, Holm over the families, alpha 0.05).
Of those the highest mean of the family win rates wins; none qualifies: the start (update 0) is selected and the report says so.
Writes RUN/selection.json (the decision, the table of the candidates) and RUN/selection/<update>/report.{md,json} (gatestats).
The held-out instances 80-99 are not read here.
"""
import argparse
import collections
import glob
import json
import os
import subprocess
import sys

SPEC = {"name": "rl3 selection: no family DROP against the start", "alpha": 0.05, "min_n": 10, "bootstrap_reps": 1000,
        "harm": {"tolerance_pts": 8, "exclude": []}}


def val_dirs(run):
    out = {}
    for d in glob.glob(os.path.join(run, "val", "u*")):
        if os.path.exists(os.path.join(d, "done")) and os.path.exists(os.path.join(d, "out", "rows.jsonl")):
            out[int(os.path.basename(d)[1:])] = d
    return out


def family_rates(rows_path):
    per = collections.defaultdict(lambda: [0, 0])
    for line in open(rows_path):
        r = json.loads(line)
        per[r["family"]][0] += int(bool(r["win"]))
        per[r["family"]][1] += 1
    return {f: w / n for f, (w, n) in per.items()}


def mean_rate(rows_path):
    r = family_rates(rows_path)
    return sum(r.values()) / max(len(r), 1)


def pick(table):
    """table: [{update, drops, mean}], update 0 is the start. Returns the selected row (the start when none qualifies)."""
    ok = [t for t in table if t["update"] != 0 and not t["drops"]]
    if not ok:
        return next(t for t in table if t["update"] == 0)
    return max(ok, key=lambda t: (t["mean"], t["update"]))


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--kept", required=True)
    ap.add_argument("--gatestats", required=True, help="the directory of gate.py")
    ap.add_argument("--updates", default="50,100,150,last")
    ap.add_argument("--val-from", type=int, default=60)
    ap.add_argument("--val-to", type=int, default=80)
    ap.add_argument("--rule-dependent", default=None, help="rule_dependent.json of the gate binary's source (GATESTATS_RULE_DEPENDENT)")
    a = ap.parse_args(argv)
    vals = val_dirs(a.run)
    if 0 not in vals:
        raise SystemExit("no validation of update 0 in the run")
    last = max(vals)
    want = [last if x == "last" else int(x) for x in a.updates.split(",")]
    want = sorted({u for u in want if u in vals and u != 0})
    missing = sorted({last if x == "last" else int(x) for x in a.updates.split(",")} - set(want) - {0})
    env = dict(os.environ)
    if a.rule_dependent:
        env["GATESTATS_RULE_DEPENDENT"] = a.rule_dependent
    spec_path = os.path.join(a.run, "selection-spec.json")
    json.dump(SPEC, open(spec_path, "w"), indent=1)
    base = os.path.join(vals[0], "out", "rows.jsonl")
    table = [{"update": 0, "drops": [], "mean": mean_rate(base), "weights": None}]
    for u in want:
        rows = os.path.join(vals[u], "out", "rows.jsonl")
        out = os.path.join(a.run, "selection", f"u{u:04d}")
        os.makedirs(out, exist_ok=True)
        inputs = {"A_label": "update 0", "B_label": f"update {u}", "slices": [{"name": "val", "kept": a.kept, "window": [a.val_from, a.val_to], "A": [base], "B": [rows]}]}
        ipath = os.path.join(out, "inputs.json")
        json.dump(inputs, open(ipath, "w"), indent=1)
        p = subprocess.run([sys.executable, os.path.join(a.gatestats, "gate.py"), "report", "--inputs", ipath, "--spec", spec_path, "--out", out], env=env, capture_output=True, text=True)
        if p.returncode != 0:
            raise SystemExit(f"gatestats failed for update {u} (rc {p.returncode}): {p.stderr.strip()}")
        rep = json.load(open(os.path.join(out, "report.json")))
        drops = [n for n in rep["harm_set"] if rep["families"][n]["drop"]]
        table.append({"update": u, "drops": drops, "mean": mean_rate(rows), "weights": os.path.join(a.run, f"ckpt-{u:04d}.safetensors")})
    chosen = pick(table)
    res = {"selected": chosen["update"], "start_selected": chosen["update"] == 0, "weights": chosen["weights"], "table": table, "missing_validations": missing,
           "rule": "no family DROP against update 0 (gatestats harm, tolerance 8, Holm, alpha 0.05) on instances %d-%d; then the highest family-mean win rate; none: the start" % (a.val_from, a.val_to - 1)}
    json.dump(res, open(os.path.join(a.run, "selection.json"), "w"), indent=1)
    for t in table:
        print(f"update {t['update']:>4}: mean {t['mean']:.4f}, DROP {len(t['drops'])} {', '.join(t['drops'])}")
    print("SELECTED", "the start (R1 = R0)" if res["start_selected"] else f"update {chosen['update']}", chosen["weights"] or "")
    if missing:
        print("not validated:", missing)


if __name__ == "__main__":
    main()
