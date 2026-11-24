# Prompt 3 for the Claude Code session on the M5 Mac: one night of army-net research

**Roman: three lines, then leave the Mac on the charger.**
1. `mkdir -p ~/aurelia-night && cd ~/aurelia-night && git clone -b exp/m5-night https://github.com/sirgl/aurelia-classic.git classic && git clone -b exp/m5-kit https://github.com/sirgl/aurelia-platform.git platform`
2. `cd ~/aurelia-night/classic && claude`
3. Paste: `Read docs/m5/PROMPT-3.md and do it. Start now; everything, the push included, ends by 08:00.`

---

You are Claude Code on the Apple M5 Max (6 performance + 12 efficiency cores, 128 GB) that ran stages 1 and 2. Roman pasted this
and left. Stage 3 runs by itself, unattended, and ends with ONE pushed branch. Read this whole file first, then work through
the steps. You are the only agent on this machine for this job.

**The machine is free only overnight: everything, the push included, ends by 08:00 local time.** The campaign enforces it
(`deadline = "08:00"`, `report_min = 45` in `tools/m5/campaign3.toml`, fixed at the first start in
`lab/runs/campaign3/deadline.json`): it starts no job whose estimate would end after 07:15 and records it `skipped_deadline`.
The queue is in value order, so what the deadline cuts is the least important (most likely the second RL seed). That is the
expected result, not a failure.

## What the night measures (the queue of `tools/m5/campaign3.toml`, in this order)

| Part | Jobs | What |
|---|---|---|
| P0 | setup, seed, teach-r, dump-MBf6c, sanity-u175, sanity-MBf6c, twin-MBf6c | the release `ai-exam`, the pinned MLX venv; the base nets from `lab-seed/` (MBf6c, rl2 u175) exported for Rust; the teachers recorded on kept-r under the scene rule (train 0-79, held-out 80-99); MBf6c's Rust dumps of the ten twin families (training and held-out scenes, slim); u175 and MBf6c played in Rust on kept-r held-out and compared play by play with the M4's rows (`lab-seed/ref`); MBf6c's own play in the MLX twin |
| P1 | needle | the needle test: synthetic facts with distractors at context 64 / 128 / 256, widths 64 / 96 / 128, layers 3 / 4 / 6, 1 / 3 / 5 needles; the smallest net with >= 95 % at each length (`tools/m5/needle/needle.py`, MLX only, 90 min cap) |
| P2 | arm-base, arm-L6, arm-loop3x2, arm-swiglu, arm-rms-qk, arm-gate, arm-muon, gate-P2, pick-P2 | architecture screening by imitation, seed 1: MB d 64 from scratch on teach-r, the same recipe, data and steps for every arm (`train_m --map --arch ...`, MBXNet in `nn/aurelia_nn/army/model_x.py`); measured by the held-out imitation accuracy and by the twin's own play of the ten families on MBf6c's held-out scenes; gatestats (paired by scene, Holm) of every arm against arm-base; the best two arms picked |
| P3 | arm-base-s2, best1-s2, best2-s2, gate-P3 | seed 2 of the baseline and of the two picked arms; gatestats with the seeds pooled |
| P4 | rl-s2, rl-s2-rust, rl-s3, rl-s3-rust | RL seed variance: the rl2 GRPO recipe (`armyrl`: K 6, the anchor on the 24 families the twin does not train, lambda 3 x the equal-norm rule, 185 updates) from MBf6c with seeds 2 and 3; the checkpoint selected on validation 70-79 played in Rust (quick + full) and compared with MBf6c and with u175 (seed 1, made on the M4) |
| end | summary | `lab/runs/campaign3/summary.md` |

## Hard rules

1. Every heavy job runs through the vendored broker `tools/res/res run ...`; the campaign already does. Never start a training, an eval or a Rust suite by hand outside it.
2. One GPU job at a time (the broker's GPU lock). Never start a second MLX job beside the campaign.
3. Never load or copy the recordings (`lab/runs/teach-r-*.bin`) or the dumps (`lab/data/*-MBf6c-r.jsonl`) by hand.
4. Keep at least 25 GB free disk (the broker waits below it). Short of disk: delete `target/debug` and `lab/runs/*/ckpt` first; never delete `lab/runs/campaign3`, the recordings, the dumps or `lab/runs/net-*`.
5. **The only push is the one of step 5** (the new branch `exp/m5-night-result`, by `tools/m5/push3.sh`). No other push, no force, no other branch on a remote, no uploads, no gists.
6. macOS has no `timeout`; long things run in the background; never a foreground `sleep` (poll the status file). Kill only PIDs you started yourself, never `killall` or `pkill` by a broad name.
7. **Do not change code.** Not the game, not the trainer, not the net, not the campaign's logic. You may change only the kit's paths and permissions (`tools/m5/env.sh`, file modes), and, only if a job cannot start at all, its own parameters in `tools/m5/campaign3.toml` (record each change with the reason in the report). If something is broken, record it with the log lines and let the queue go on.
8. Install nothing globally. `tools/m5/setup.sh` writes only into the repo (`.venv-m5`, `target`); rustup and Python 3.12 are already on this Mac from stage 1. If something needs a password or a GUI click, write it into `lab/STATUS3.md` and stop.

## Step 0. Check the machine (3 min)

```sh
cd ~/aurelia-night/classic
date +%H:%M; pmset -g batt | head -2; df -h . | tail -1         # on the charger; >= 60 GB free (recordings ~11 GB, dumps ~3 GB, nets)
git log --oneline -3; ls ../platform/crates | head -3
tools/res/res start && tools/res/res status | head -5
tools/m5/campaign3.sh plan | tee lab-plan.txt | tail -30         # every job with its estimate and its end time
```
If it is later than 06:30, do not start: write "started too late" into `lab/STATUS3.md` and stop.
Start the status loop and note its PID in `lab/STATUS3.md`:
```sh
mkdir -p lab && nohup sh -c 'while :; do (date; tools/m5/campaign3.sh status; tools/res/res status | head -8; df -h . | tail -1; tail -3 lab/logs/campaign3.log) > lab/STATUS3.md.tmp 2>&1; mv lab/STATUS3.md.tmp lab/STATUS3.md; sleep 3600; done' >/dev/null 2>&1 &
echo "status loop pid $!" >> lab/STATUS3.md.pids
```

## Step 1. Smoke first (10-20 min)

```sh
tools/m5/campaign3.sh smoke 2>&1 | tail -40          # every job kind once, tiny budgets, into lab-smoke/ (its own lab: the real run does not see it)
sed -n 1,80p lab-smoke/runs/campaign3-smoke/summary.md
```
The smoke builds the binary and the venv (the setup job) and must end with every job `ok` (look at the P0 table and the
`Failed` section). If a job fails in the smoke, read its log in `lab-smoke/logs/`, fix only what rule 7 allows, and run the
smoke again (it resumes; `rm -rf lab-smoke` to start it over). If it still fails, start the real run anyway: the failing kind
will fail there too and be recorded; report it. Do not spend more than 40 minutes on the smoke.

## Step 2. Run stage 3 (hours)

```sh
tools/m5/campaign3.sh start
tools/m5/campaign3.sh status
```
Check every 30-60 minutes (`tools/m5/campaign3.sh status`, `tail lab/logs/campaign3.log`, `tools/res/res status`): is the PID in
`lab/logs/campaign3.pid` alive, is a job stuck (no new line in the newest `lab/logs/*.log` for 30 min while the broker shows it
running), is the disk above 25 GB, any `thermal` line. Update `lab/STATUS3.md` by hand at every finding (the loop rewrites it
hourly; put your notes in `lab/STATUS3-notes.md`).
**Crash or reboot:** `tools/m5/campaign3.sh start` again (finished jobs are skipped, a training resumes from its checkpoint, an
RL run starts that seed again). If the campaign dies three times in a row for the same reason, stop and report.
**What to read while it runs (do not stop the queue for it):** after `sanity-u175`: `matches_m4` should be true (the M5 Rust
plays u175 as the M4 did; a few families beyond noise mean a different binary or kept set: report it). After `twin-MBf6c`:
`twin_same_as_rust` should be >= 0.95. After `arm-base`: hold accuracy and the twin mean are the baseline line of P2.
**The deadline, by hand:** the estimates are +-40 %. At 07:15, if a job still runs: `tools/m5/campaign3.sh stop` (the queue
stops after it); if it still runs at 07:25, kill its process (the PID in `tools/res/res status` named `c3-...`, started by this
campaign) and record it as cut. Then `tools/m5/campaign3.sh summary` and go to step 4.

## Step 3. (nothing to do: the queue writes `lab/runs/campaign3/summary.md` at its end)

## Step 4. The report `docs/m5/NIGHT-RESULTS.md` (English, by 07:40)

Sections: 1. Machine, versions, start and end times (`lab/runs/campaign3/versions.json`). 2. P0: the sanity lines (u175 and
MBf6c against the M4's rows: same within noise or not; the twin's agreement with Rust). 3. P1: the needle table (smallest net
>= 95 % per length and needle count). 4. P2: the arms table (params, hold accuracy, twin mean and per family, decisions/s,
minutes) and the gate lines against the baseline (up / down / DROP, mean delta with its CI); which arms were picked.
5. P3: seed 2 and the pooled gate; do the two seeds agree in sign. 6. P4: per RL seed lambda, the selected update, validation
start -> selected, the Rust full mean, the gate against MBf6c, the comparison with u175 (seed 1): how large is the seed spread.
7. Duration: estimates against actual (the Time table), the jobs cut by the deadline. 8. Problems: every failed job with its
log lines, every deviation from this prompt.
Plain statements, numbers from the files, no guesses; mark what you did not verify. Short of time: the summary alone is
enough (`push3.sh` copies it as the report when `NIGHT-RESULTS.md` is missing).

## Step 5. Push the one new branch (by 07:55)

```sh
cd ~/aurelia-night/classic
tools/m5/push3.sh --dry-run        # copies the results into docs/m5/night/, rewrites machine paths, runs the checks; must end "the checks passed"
tools/m5/push3.sh                  # creates exp/m5-night-result, commits docs/m5/night + docs/m5/NIGHT-RESULTS.md, pushes it
```
The checks (in the script; all must print nothing): no file over 3 MB, no absolute home path or secret pattern, no name of
another game's mod. If a check fails, fix the copied files (not the checks) and run it again. If the push is refused, the
script packs `lab/m5-night-results.tgz`: write "need GitHub access" into `lab/STATUS3.md` and print the path.

## Step 6. Finish

Stop what you started (the status loop, `tools/m5/campaign3.sh stop` if needed, `tools/res/res stop`), leave the files in place,
and print in the last message: the branch and commit, the night's duration, the P0 sanity verdict (1 line), the needle answer
(1 line per length), the best P2 arm against the baseline (1 line), the RL seed spread (1 line), and what the deadline cut.
