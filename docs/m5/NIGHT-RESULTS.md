# Stage 3 results: one night of army-net research on the Apple M5 Max (2026-11-24 23:53 to 2026-11-25 06:37)

All 26 jobs of `tools/m5/campaign3.toml` finished `ok` in 6.7 h (no failures, nothing cut by the deadline). Pinned MLX 0.32.4, kit commit 970d839,
scene rule version 1. Numbers are from `docs/m5/night/summary.md` and `results.jsonl`.

## 1. Machine
Apple M5 Max, 6 performance + 12 efficiency cores, 128 GB RAM, macOS 27.2.1, MLX 0.32.4, on charger. Start 23:53, end 06:37.
The estimates were high for the trainings (actual 15-20 min per imitation arm against 30-52 estimated; needle 15.8 min against 120), so the whole queue fit.

## 2. P0: the M5 plays the M4's nets as the M4 did
| job | result |
|---|---|
| sanity-u175 (Rust, kept-r held-out) | quick mean 0.9427, full mean 0.8347; no family moved beyond noise against the M4's rows (`matches_m4` true) |
| sanity-MBf6c | quick mean 0.8173, full mean 0.7888; no family moved beyond noise (`matches_m4` true) |
| twin-MBf6c (MLX twin against Rust) | twin mean win 79.25 % over 1243 plays; same verdict as Rust on 0.9976 of the plays (bar 0.95) |
So the binary, the kept set and the scene rule on the M5 give the M4's results, and the MLX twin agrees with Rust. The later numbers are comparable.

## 3. P1: the needle test (smallest pre-LN transformer, heads 4, MLP 2d, held-out accuracy >= 95 %)
| context | 1 needle | 3 needles | 5 needles |
|---|---|---|---|
| 64 | d64 x L3 | d64 x L3 | d64 x L3 |
| 128 | d96 x L3 | d64 x L3 | d64 x L3 |
| 256 | none up to d128 x L6 (best 0.040) | d96 x L3 | d64 x L3 |
Every run either reaches 0.99-1.00 or stays at 0.02-0.06 (chance level) for all 3000 steps; nothing in between. The failures (128 / 1 needle at d64, 256 / 1 needle at every size) look like a net that never
finds the signal, not like a net that is too small. I did not test this (no longer training, other seeds, a curriculum); it is a reading of the table, not a result.

## 4. P2: architecture screening by imitation (MB d 64, seed 1, same recipe, data and steps; 10 families on MBf6c's held-out scenes)
| arm | params | hold acc | twin mean win % | delta to base (95 % CI) | gate |
|---|---:|---:|---:|---|---|
| arm-base | 526858 | 0.7855 | 82.02 | | |
| arm-rms-qk | 526506 | 0.7967 | 88.34 | +0.062 (+0.040, +0.085) | pass, 2 families up |
| arm-swiglu | 526792 | 0.8054 | 86.85 | +0.046 (+0.024, +0.068) | pass |
| arm-gate | 539338 | 0.7923 | 85.82 | +0.035 (+0.013, +0.057) | pass, 2 families up |
| arm-muon (rms-qk + Muon, lr 0.02) | 526506 | 0.7955 | 84.99 | +0.019 (-0.004, +0.042) | no |
| arm-loop3x2 | 526992 | 0.7949 | 84.87 | +0.018 (-0.005, +0.039) | no |
| arm-L6 (6 layers) | 627478 | 0.7898 | 84.66 | +0.022 (-0.001, +0.046) | no |
No arm dropped a family (DROP 0). Speed: 8245-10218 decisions/s; 19-24 min per arm. Picked for seed 2: arm-rms-qk and arm-swiglu (ranking: twin mean, then hold accuracy).

## 5. P3: seed 2 and the pooled gate (two seeds)
| arm | seed 2 twin mean % | pooled twin mean (base) | pooled delta (95 % CI) | gate |
|---|---:|---|---|---|
| arm-base-s2 | 85.95 | 0.8344 | | |
| rms-qk | 87.77 | 0.8754 | +0.041 (+0.026, +0.057) | pass, city-fire and planned-assault up |
| swiglu | 87.09 | 0.8634 | +0.029 (+0.013, +0.045) | pass, bombard up |
The two seeds agree in sign for both arms on the mean. The pooled gate lists one family where the seeds disagree: assault-heal (rms-qk: 88.6 then 82.9, base 78.6 then 90.0).
Hold accuracy of seed 2: base 0.793, rms-qk 0.7996, swiglu 0.809. rms-qk stays ahead of swiglu on the twin mean. Two seeds is a small sample.

## 6. P4: RL seed variance (rl2 recipe from MBf6c, 185 updates, the checkpoint chosen on validation 70-79, played in Rust)
| run | lambda | selected update | validation start -> selected | Rust full mean | vs MBf6c (paired) | vs u175 seed 1 (M4) |
|---|---:|---:|---|---:|---|---|
| rl-s2 | 0.09 | 150 | 0.7903 -> 0.9423 | 0.8439 | pass; citadel, assault-heal, city-fire, hill-archers, planned-assault, siege up | none moved beyond noise |
| rl-s3 | 0.3 | 175 | 0.7903 -> 0.9199 | 0.8312 | not passed (DROP: full/bait); citadel, assault-heal, city-fire, planned-assault, siege up | DROP: quick/rush, quick/siege |
References in Rust: MBf6c 0.7888, u175 (seed 1, made on the M4, replayed here) 0.8347. The three RL seeds end at 0.835 (u175), 0.844 and 0.831 on the full mean: a spread of 0.013, all above MBf6c by 0.04-0.055.
Note: lambda differs between the two runs (0.09 and 0.3, set by the equal-norm rule from the run's own gradient norms), so the seed spread includes that difference; I did not separate the two.
rl-s3 has a family drop against both references: one seed is not a clean win everywhere.

## 7. Duration
Estimated total 7.3 h for the jobs that fitted, actual 6.7 h. P0 + needle ended at 00:17, P2 at 02:44, P3 at 03:43, rl-s2 at 05:09, rl-s3 at 06:37. Cut by the deadline: nothing.

## 8. Problems and deviations
- No failed job, no deviation from the prompt; no code or parameter was changed. The smoke (26 jobs, 5 min) ran first on the clean clone; `setup.sh` ran by hand before it because the plan needs the venv.
- `tools/dash/eval_watch.py` (stage 2 note) was not used this night.
- Not verified: the cause of the needle failures (section 3); whether the RL spread is the seed or the lambda; the P2 ranking with more than two seeds; imitation arms are d 64 from scratch, not the MBf recipe of stage 2.
