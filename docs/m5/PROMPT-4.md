# Prompt 4 for the Claude Code session on the Apple M5 Max Mac: one night of long RL on the honest net input (start about 18:30, hard stop 08:00)

You are Claude Code on the Apple M5 Max (6 performance + 12 efficiency cores, 128 GB). The owner pasted this and left; the job runs
unattended and ends with ONE pushed results branch. Read this whole file first, then work through the steps. You are the only agent on
this machine for this job.

Layout: `~/aurelia-night4/classic` (this repository, branch `exp/m5-rl-honest`) and `~/aurelia-night4/platform` beside it (the platform
repository; `tools/m5/platform.commit` names the commit the kit was made with, `setup4.sh` checks it out).

## What the night measures

The RL loop of the lane (`nn/aurelia_nn/rl3/loop.py`: Rust actors play sampled rounds, an MLX learner updates, an imitation anchor holds the
teachers' plays) on the honest input (the net reads only what its side knows, observation version 2), from the typed start net N175T,
**600 updates** per run (the lane's own runs have 185), then two runs continued to 1 500. Two arms, seeds 1 to 5:

- `base`: the recipe of gate G1.10h, nothing tuned (lambda by the auto rule of update 1).
- `boost`: the same, plus `--anchor-boost` on the siege group (siege, planned-assault, promotion-choice, assault-heal, city-fire, hill-archers, citadel),
  factor 3, with lambda fixed at the value the base run of the same seed got (the campaign reads it from that run's log).

The queue of `tools/m5/campaign4.toml`, in this order (the most important first; a night cut short still has paired seeds):

| Part | Jobs | What |
|---|---|---|
| P0 | setup, seed, threads, sanity, teach, lengths | the gate binary built from this commit and the pinned MLX 0.32.4 venv; the start nets and the kept sets unpacked from `lab-seed4/`; the **actor thread count measured** (the honest N175 full table at 6 and at 18 threads, the faster is used; ties go to 6); **sanity: the honest N175 (full 6 323 plays, quick 1 641) and N175T (quick) played here must equal the M4's rows play for play** (win, rounds, measures); the anchor's teacher recordings made here (`teach-r-train.bin` about 9.8 GB, `p7-teach-train.bin` about 0.15 GB); the greedy lengths of N175T on the training scenes |
| P1 | rl-base-s1, tables-base-s1, rl-boost-s1, tables-boost-s1, then seeds 2 and 3 | each RL run: 600 updates, validation (instances 60-79, greedy) at update 0 and every 50, a checkpoint every 5 (kept: every 50th, 185, 400, 600); then the **held-out tables** (instances 80-99; full = 47 families, quick = 12 families) of checkpoints 185, 400 and 600 and the gatestats reports against the honest N175 |
| P2 | the same for seeds 4 and 5 | five paired seeds in all |
| P3 | rl-ext-base-s1, tables-ext-base-s1, rl-ext-boost-s1, tables-ext-boost-s1 | the two runs of seed 1 **continued from their update-600 checkpoint to 1 500** (resume, same seed stream; validation every 50), held-out tables of 1 000 and 1 500 |
| P4 | the same continuation for seed 2 | only if time remains |
| end | summary | `lab/runs/campaign4/summary.md`: per run the validation curve (to 1 500 where continued), per-family held-out at 185 / 400 / 600 (/ 1 000 / 1 500), the siege-group mean, the paired base-vs-boost comparison by seed and pooled |

Note for the continuation: a resume restarts the anchor's shuffled draw order (the scene and actor draws are functions of seed and update and stay the same); a watchdog restart does the same. Record it in the report.

The campaign estimates a 600-update run at 85 minutes and a continuation (900 more updates) at 130 minutes (the M4's figures) and rescales after the first run; the whole queue is about 20 hours on the M4's figures, so **the deadline always cuts the tail**: expected, not a failure. It starts no job that would end after the
deadline (13.5 h from the first start, or the time in `M5_DEADLINE=HH:MM`, minus 25 minutes for the summary and the push). **Start with `M5_DEADLINE=08:00 tools/m5/campaign4.sh start`** (and the same variable for `plan`). The order is by importance; the pairs are interleaved, so a cut tail leaves complete pairs.

## Hard rules

1. Every heavy job runs through the vendored broker `tools/res/res run ...`; the campaign already does. Never start a loop, a suite or a training by hand outside it.
2. One GPU job at a time (the broker's GPU lock). Never start a second MLX job beside the campaign.
3. Never open or copy the recordings (`lab/data/anchor/*.bin`) by hand.
4. Keep at least 25 GB of free disk (the broker waits below it). Short of disk: delete `target/debug` and `lab/runs/rl-*/ckpt-*` of runs whose tables are done; never delete `lab/runs/campaign4`, `lab/data/anchor`, `lab/runs/tables`.
5. **The only push is the one of step 5** (the new branch `exp/m5-rl-honest-result`, by `tools/m5/push4.sh`). No other push, no force, no other branch on a remote, no uploads.
6. macOS has no `timeout`; long things run in the background; never a foreground `sleep` (poll the status file). Kill only PIDs you started yourself, never `killall` or `pkill` by a broad name.
7. **Do not change code, recipes, seeds, arms, the order or the thresholds.** You may change only the kit's paths and permissions (`tools/m5/env.sh`, file modes) and, only if a job cannot start at all, its own parameters in `tools/m5/campaign4.toml` (record each change with the reason in the report). If something is broken, record it with the log lines and let the queue go on.
8. Install nothing globally. `tools/m5/setup4.sh` writes only into the repo (`.venv-m5`, `target`, `lab`); rustup and Python 3.12 should be there (else it prints what to install). If something needs a password or a GUI click, write it into `lab/STATUS4.md` and stop.

## Step 0. Check the machine (3 min)

```sh
cd ~/aurelia-night4/classic
date +%H:%M; pmset -g batt | head -2; df -h . | tail -1        # on the charger; >= 45 GB free (recording 10 GB, nets, checkpoints)
git log --oneline -3; git -C ../platform log --oneline -1; cat tools/m5/platform.commit
M5_DEADLINE=08:00 tools/m5/campaign4.sh plan | tee lab-plan.txt | tail -30        # every job with its estimate and its end time
```
Start the status loop and note its PID in `lab/STATUS4.md.pids`:
```sh
mkdir -p lab && nohup sh -c 'while :; do (date; tools/m5/campaign4.sh status; tools/res/res status | head -8; df -h . | tail -1; tail -3 lab/logs/campaign4.log) > lab/STATUS4.md.tmp 2>&1; mv lab/STATUS4.md.tmp lab/STATUS4.md; sleep 1800; done' >/dev/null 2>&1 &
echo "status loop pid $!" >> lab/STATUS4.md.pids
```

## Step 1. Smoke first (about 5 minutes after the build)

```sh
tools/m5/campaign4.sh --smoke 2>&1 | tail -40          # every job kind for 1-2 updates, in lab-smoke/ (its own lab: the real run does not see it)
sed -n 1,60p lab-smoke/runs/campaign4-smoke/summary.md
```
It builds the binary and the venv (the setup job: the build takes 10-15 minutes the first time, the real run reuses it) and must end with every job `ok`
(look at the Jobs table). In the smoke the `sanity` job compares a few families only. If a job fails, read its log in `lab-smoke/logs/`, fix only what
rule 7 allows, run the smoke again (it resumes; `rm -rf lab-smoke` starts it over). Do not spend more than 40 minutes on the smoke; if it still fails,
write the failing job and its log lines into `lab/STATUS4.md` and start the real run anyway only if the failure is in a job kind the real run does not need first.
`rm -rf lab-smoke` when it is done.

## Step 2. Run the night

```sh
M5_DEADLINE=08:00 tools/m5/campaign4.sh start
tools/m5/campaign4.sh status
```
Check every 30-60 minutes (`tools/m5/campaign4.sh status`, `tail lab/logs/campaign4.log`, `tools/res/res status`): is the PID in `lab/logs/campaign4.pid`
alive, does the newest `lab/runs/rl-*/log.jsonl` grow (a run silent for 30 minutes is restarted by its watchdog from the newest checkpoint), is the
disk above 25 GB, any `thermal` line. Put your notes in `lab/STATUS4-notes.md`.

**Stop rules.**
- **P0 sanity differs** (`sanity` ends `failed_final`, "does not play the M4's honest rows play for play"): the queue halts by itself (`skipped_fatal` for the rest). Do NOT start the night in any other way.
  Write what differs (the numbers in `lab/runs/sanity/sanity.json`, the first example plays), go to step 4 and 5 with that report. Do not "fix" the binary.
- **Crash or reboot:** `M5_DEADLINE=08:00 tools/m5/campaign4.sh start` again (finished jobs are skipped, an RL run resumes from its newest checkpoint). If the campaign dies three times in a row for the same reason, stop and report.
- **A run fails twice** (`failed_final`): the queue goes on to the next job (a boost run whose base failed is `skipped_dep`). Record the log lines.
- **The deadline, by hand:** at 07:50 (the hard stop is 08:00, the push included: the summary and the report must be done before), if a job still runs, `tools/m5/campaign4.sh stop` (the queue stops after it); if it still runs 20 minutes later, kill its process (the PID in `tools/res/res status` named `c4-...`, started by this campaign) and record it as cut. Then `tools/m5/campaign4.sh summary`, go to step 4.
- **What to read while it runs** (do not stop the queue for it): after `threads`: which count was chosen and that `rows_equal_across_threads` has 0 different. After `rl-base-s1`: the lambda (the lane's own seed-1 run had 0.2), the median update seconds (the M4: 7.5 s), the validation curve at 50 / 100 / 150 against `docs/m5` expectations: the family mean should rise from about 0.60 at update 0.

## Step 3. (nothing to do: the queue writes `lab/runs/campaign4/summary.md` at its end)

## Step 4. The report `docs/m5/NIGHT-RESULTS-4.md` (English, by the deadline minus 20 minutes)

Fill `docs/m5/NIGHT-RESULTS-4.template.md` (copy it to `docs/m5/NIGHT-RESULTS-4.md`): machine and versions, thread choice, sanity, the jobs and
their durations against the estimates, the validation curves, the held-out lines at 185 / 400 / 600, the paired base-vs-boost table, problems.
Plain statements, numbers from `lab/runs/campaign4/summary.md`, no guesses; mark what you did not verify. Do not interpret beyond the numbers: the
coordinator reads the gate lines. Short of time: the summary alone is enough (`push4.sh` copies it as the report when the file is missing).

## Step 5. Push the one new branch

```sh
cd ~/aurelia-night4/classic
tools/m5/push4.sh --dry-run        # copies the results into docs/m5/night4/, rewrites machine paths, runs the checks; must end "the checks passed"
tools/m5/push4.sh                  # creates exp/m5-rl-honest-result, commits docs/m5/night4 + docs/m5/NIGHT-RESULTS-4.md, pushes it to origin
```
The checks print nothing when they pass: no file over 3 MB, no absolute home path or secret pattern, no private name. If a check fails, fix the copied
files (not the checks) and run it again. If the push is refused, the script packs `lab/m5-night4-results.tgz`: write "need GitHub access" into `lab/STATUS4.md` and print the path.

## Step 6. Finish

Stop what you started (the status loop, `tools/m5/campaign4.sh stop` if needed, `tools/res/res stop`), leave the files in place, and print in the last
message: the branch and commit, the night's duration, the chosen thread count, the sanity verdict (1 line), per seed the siege-group mean base against boost at
185 / 400 / 600 (and 1 000 / 1 500 where continued) (1 line each), and what the deadline cut.
