# Night 4 results: long RL on the honest input (YYYY-MM-DD HH:MM to HH:MM)

Numbers are from `docs/m5/night4/summary.md` and `results.jsonl`. Fill the brackets; delete what did not happen.

## 1. Machine
[chip, cores, RAM, macOS, MLX version, commit of the kit, platform commit, start and end, free disk at start and end]

## 2. Threads and sanity (P0)
- Actor threads chosen: [6 / 18]; seconds of the full N175 table at 6 and at 18: [..]; rows equal across the two: [yes / no].
- Sanity against the M4's honest rows (play for play): N175 full [x / 6323 equal], N175 quick [x / 1641], N175T quick [x / 1641]. Verdict: [equal / differs, which families].
- Anchor recording sizes: [teach-r-train.bin, p7-teach-train.bin]; the exported nets equal the shipped Rust files: [yes / no].

## 3. The jobs
[the Jobs table of the summary: status, estimated and actual minutes; which jobs the deadline cut]

## 4. Per run (base / boost, seeds 1 to 5; the continuations of seed 1 and 2 to 1 500)
For each: lambda, median seconds per update, validation curve (mean47 and siege group at 0, 50, ..., 600), parity warnings, plays with a refused sampled action.

## 5. Held-out (instances 80-99) at 185, 400, 600 (1 000, 1 500)
For each run: siege-group mean, mean47, mean32, new-to-RL mean, quick mean; the families that moved most against the honest N175; the gatestats DROP lines.

## 6. Paired comparison, boost against base, by seed
[the table of the summary: siege-group delta with its interval, mean47 delta, mean32 delta, at 185 / 400 / 600; the pooled line if two seeds or more]

## 7. Problems
[every failed or restarted job with its log lines; every deviation from PROMPT-4.md; what was not verified]
