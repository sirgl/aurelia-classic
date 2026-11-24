# gatestats: the shared gate statistics

One tool for every lane that compares a candidate net B with a reference net A on the trainer families.
Python 3, standard library only (no numpy needed). Read-only on the data; it writes only the report directory you give it.

## What it does
- **Paired test per family.** A and B are played on the same scenes. Plays are paired by (family, era, instance) of the kept attempt.
  Exact McNemar on the discordant plays (b = A won / B lost, c = the reverse), one- and two-sided. Fisher's exact test is reported
  next to it for reference and is the right test only for unpaired data (`stats.fisher_exact`). Wilson intervals for the rates,
  Newcombe's paired interval for the delta, effect size in points.
- **Many families.** Holm (family-wise) adjusted p-values across the harm set, Benjamini-Hochberg as a report.
  A family counts as a **DROP** when the delta is below `-tolerance_pts` AND the Holm-adjusted one-sided p (H1: B worse) is below alpha.
  Status per family: up / down (Holm-significant) / DROP (down beyond the tolerance) / same.
- **Seeds.** Give several rows files per arm (seed i of A is paired with seed i of B). The tests pool the plays; the report shows
  the delta per seed and flags families whose seeds disagree (range of the per-seed deltas > `seed_z` standard errors).
  Pooled plays only measure play noise of fixed nets; the between-seed spread is in the per-seed column. A real seed-level
  claim needs more than 2 seeds.
- **Power.** Every family row has an MDE: the smallest drop the paired test finds at 80 % power at level alpha/m, given that
  family's n and its observed discordance. `python3 gate.py power --n 140 --m 32 --q 0.1` prints the table before a gate is written.
- **Gate report** (`report.md` + `report.json`): families up / down / same, the mean over families with a paired bootstrap CI
  (resampling instances inside every family, fixed seed), the count of families at >= 90 % of the best teacher with its CI, and
  pass / fail by the spec.

## Run
```
python3 gate.py report --inputs inputs.json --spec spec.json --out DIR
python3 gate.py power --n 40 140 160 --m 32 --q 0.05 0.10 0.20
python3 -m unittest -v        # the tests
```

### inputs.json
Slices are row sets (for example `full` and `quick`). Family names in the spec are `slice/family`.
```
{"A_label": "MBf6c", "B_label": "u175",
 "slices": [{"name": "full", "kept": ".../kept-r.jsonl", "window": [80, 100],
             "A": ["a-seed0/rows.jsonl", "a-seed1/rows.jsonl"],
             "B": ["b-seed0/rows.jsonl", "b-seed1/rows.jsonl"],
             "teachers": [".../f-teach/rows.jsonl"]}]}
```
`kept` filters the plays to the kept attempt of each scene, `window` is the held-out instance range, `teachers` give the best
teacher per family (players `ref-*` and `unciv`; the bar is 90 % of the best teacher's rate, fixed at its point estimate).
A family that appears in two slices with identical plays counts once in the harm set, mean and count.

### spec.json (every block is optional; a gate runs only when its block is present)
```
{"name": "...", "alpha": 0.05, "min_n": 10, "bootstrap_reps": 4000,
 "harm":         {"tolerance_pts": 5, "exclude": ["*/prey-land"], "slices": null,
                  "mode": "significance",            # or "noninferiority": a family must have its 95 % CI above -tolerance
                  "unstable_counts_as_drop": false},  # seeds disagree and delta < 0
 "mean":         {"slices": ["full"], "exclude": [], "min_gain_pts": 0},     # CI lower bound of the mean delta > min_gain
 "required_up":  {"families": ["quick/planned-assault"], "min_gain_pts": 0},  # Holm over this small set, one-sided
 "count_at_bar": {"slices": ["full"], "exclude": [], "frac": 0.9, "min_gain": 1, "require_ci": false}}
```
`exclude` takes glob patterns. See `specs/rl2_gate.json` and `specs/k18_gate.json`; the filled inputs are in `reread/`.

## Writing a realistic gate
Check `gate.py power` first. With ~140 plays per family and ~32 families the paired test finds only drops of about 9-10 points
(at 10 % discordant plays); a "5-point tolerance" with the `significance` mode therefore means "no family is *provably* worse by
more than 5 points", not "no family is worse". `noninferiority` mode demands the proof and will fail almost any gate at this n;
use it only with more plays. More plays per family, or fewer required families, buys more than a tighter tolerance.

## Files
`stats.py` (tests, intervals, Holm, BH, power), `rows.py` (rows.jsonl reader and pairing), `gate.py` (report, power, CLI),
`test_gatestats.py`, `specs/`, `reread/` (the two past gates re-read: `old_vs_new.md`, the reports).
