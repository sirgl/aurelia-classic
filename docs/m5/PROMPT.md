# Prompt for the Claude Code session on the M5 Mac

You are Claude Code on an Apple M5 Mac. Roman (the owner) pasted this prompt and left. Your job: run the army-net
experiment campaign unattended, then report back. Read this whole file first, then work through the numbered steps. You
are the only agent on this machine for this job.

## Goal

Run the army-net training and evaluation campaign of the "m5 kit" (Roman's Civ-like game Aurelia: a learned army controller,
the MB net, trained on recorded teacher plays and tested on held-out "trainer" scenes) to its end, measure how fast this
machine is against Roman's M4 laptop, and write a report. Nothing needs Roman's input once started; if you are
blocked, write the blocker into `lab/STATUS.md` and stop (do not guess around a missing prerequisite).

## Hard rules

1. Every heavy job (cargo, training, evals, benchmarks) runs through the vendored broker `tools/res/res run ...` (the
   scripts in `tools/m5/` already do). Never start a training or an eval by hand outside it.
2. One MLX training at a time and at most one probe; never two GPU jobs. If the GPU lock is busy, wait.
3. Stream recordings (the trainer does); never load the `.bin` files whole, never copy them. Keep the MLX memory and cache
   limits the scripts pass (`--mem-gb`, `RES_MLX_GB`).
4. Keep at least 25 GB of free disk at all times (the broker waits below it). If you are short, delete `target/` first,
   then old `lab/runs/*/ckpt`; never delete `lab/runs/campaign`.
5. Push nothing anywhere: no `git push`, no remote branches, no uploads, no gists. Commit nothing except the
   `campaign.toml` auto-tune result (it may stay uncommitted; do not commit at all).
6. macOS has no `timeout`; run long things as background jobs; never a foreground `sleep` (use a short check loop with
   background jobs or poll the status file). Kill only PIDs you started yourself, never `killall` or `pkill` by a broad name.
7. Do not change the code of the game, the trainer or the net. If something is broken, record it in the report with the log
   lines; fix only paths and permissions of the kit itself.
8. Do not install anything globally except what `setup.sh` tells you (Xcode command line tools, rustup, python 3.12 via
   Homebrew); ask nobody; if one of these needs a password or a GUI click that you cannot give, stop and write it in
   `lab/STATUS.md`.

## Step 1. Get the code

The code is two branches on GitHub, cloned side by side (Cargo needs `../platform`):

```sh
mkdir -p ~/aurelia-lab && cd ~/aurelia-lab
git clone -b exp/m5-kit https://github.com/sirgl/aurelia-classic.git classic
git clone -b exp/m5-kit https://github.com/sirgl/aurelia-platform.git platform
cd classic && git log --oneline -3 && ls docs/m5-RUNBOOK.md tools/m5 tools/res
```
If a clone is refused (private repositories), `gh auth login` is the owner's job: write "need GitHub access" into
`~/aurelia-lab/STATUS.md` and stop. Fallback if Roman copied bundle files to `~/Downloads`
(`aurelia-m5-kit-classic.bundle`, `aurelia-m5-kit-platform.bundle`):
`git clone -b exp/m5-kit ~/Downloads/aurelia-m5-kit-classic.bundle classic` and the same for platform. If neither works,
stop and ask Roman. From here on, `cd ~/aurelia-lab/classic` is the working directory and all commands run from it.

Read `docs/m5-RUNBOOK.md` now (the recipes a-f, the memory rules). `docs/army-net/M-HANDOFF.md` explains the line.

## Step 2. Setup (10-25 min)

```sh
xcode-select -p || xcode-select --install     # if the install dialog appears and you cannot click it, stop (rule 8)
tools/m5/setup.sh 2>&1 | tee lab-setup.log
```
`setup.sh` is idempotent and says exactly what is missing. At its end: Python 3.12 venv `.venv-m5` with `mlx==0.32.4`, the
release binaries in `target/release`, the machine printed. Then:
```sh
tools/res/res start && tools/res/res status && tools/res/res info > lab/machine.json
df -h . ; sysctl -n machdep.cpu.brand_string hw.perflevel0.physicalcpu hw.perflevel1.physicalcpu hw.memsize
```
Put the chip, the core counts, RAM and free disk at the top of `lab/STATUS.md`. If free disk is below 60 GB, stop and say so
(the recordings take about 10 GB, the runs more).

## Step 3. Dry run (10 min)

```sh
M5_SMOKE=1 tools/m5/teach.sh        # a few instances: proves the Rust recorder
tools/m5/campaign.sh smoke 2        # smoke data + 2 minutes of the first training, through the broker
```
Pass = `lab/runs/campaign-smoke/results.jsonl` has an `ok` line for `train:d48-l3` and `lab/runs/d48-l3/curves.jsonl` exists.
Then delete the smoke data so the real recording starts clean: `rm -rf lab/runs/teach4-* lab/runs/campaign-smoke lab/runs/d48-l3 lab/logs/d48-l3.log`.
Start the dashboard (recipe d of the runbook) and note its address in `lab/STATUS.md`.

## Step 4. Teacher data (recipe a, ~3-10 min)

`tools/m5/teach.sh 2>&1 | tail -5`. Expect `teach4-train.bin` about 7.5 GB and `teach4-hold.bin` about 1.9 GB. If it fails,
read the log, retry once, then report.

## Step 5. Speed benchmark (recipe e, ~15 min) -- before the campaign, on a quiet machine

`tools/m5/bench.sh 2>&1 | tee lab/logs/bench.log`. Keep `lab/runs/bench/results.json`.

M4 reference (the owner's M4 Max laptop, 12 performance + 4 efficiency cores, 64 GB):

| Measure | M4 Max |
|---|---:|
| MB training (d 64, 3 layers, plays 128), decisions/s | 2 700-3 000 (the variant with aux heads: 1 900) |
| Rust trainer plays/s, 1 thread (12 quick families, solvers) | about 95 (dry run, 12 quick families, solvers, machine under load 15) |
| Net ms per AI turn, 1 thread (MB, the M-HANDOFF figure) | about 15 (B1 encoder alone: 3.66) |
| Chosen settings | plays 128 per step, MLX limit 12 GB, broker 14 threads |

## Step 6. The campaign (recipe f, hours to a day)

```sh
tools/m5/campaign.sh start
tools/m5/campaign.sh status
```
It first auto-tunes for this machine (about 10 min): broker CPU budget = performance cores, MLX limit = 60 % of RAM, MB
training tried at 128 / 256 / 512 plays per step for 60 s each, the fastest that fits memory with the same early loss kept
and written into `tools/m5/campaign.toml`; then a 3-minute calibration, and it prints the estimated duration (also in
`lab/logs/campaign.log`). Copy that estimate into `lab/STATUS.md`. Then the grid runs by itself: the capacity ladder (d 48/64/96/128
x 3 layers, d 128 with 4 and 6 layers), 3 seeds at the best size, per net the quick eval, the full 32-family table and the
memory-zeroed table, and the summary.

**Status file.** Every hour (and at every job end) rewrite `lab/STATUS.md`: time, elapsed hours, jobs done / total, the current job, the
estimate left, free disk, `tools/res/res status` output, the last 3 lines of `lab/runs/campaign/results.jsonl`, and any
failure. Do it with a background loop that you started yourself, for example
`nohup sh -c 'while :; do (date; tools/m5/campaign.sh status; tools/res/res status; df -h . | tail -1) > lab/STATUS.md 2>&1; sleep 3600; done' >/dev/null 2>&1 &`
(note its PID in `lab/STATUS.md`, kill it at the end). Check the campaign yourself at least every 30-60 minutes: is the PID alive, is a
job stuck (no new line in `lab/logs/*.log` for 20 min), is the disk above 25 GB.

**Crash or reboot.** Run `tools/m5/campaign.sh start` again; it resumes. A job that fails twice is recorded as `failed_final` and the
campaign goes on; do not fix it by editing code. If the whole campaign dies three times in a row for the same reason, stop and
report. Do not change `campaign.toml` except through the auto-tune; if Roman's time is short and the estimate is above 30 h,
you may cut the ladder to d 64/96/128 x 3 layers and 2 seeds (edit `[[ladder]]` and `seeds`) and say so in the report.

## Step 7. Report

When `status.json` says `finished` (or you stopped for a blocker), write `lab/REPORT-m5.md` (English) with:
1. Machine: chip, cores (performance / efficiency), RAM, free disk at start and end.
2. Chosen settings: the auto-tune table (plays per step tried, decisions/s, peak RAM, loss, chosen), broker budget, MLX limit.
3. Speed against the M4 (the table of step 5, filled), and the decisions/s, plays/s measured in the campaign
   (`calibrate` line of `results.jsonl`) next to the M4 reference. One sentence on the ratio.
4. The campaign: duration (estimated, actual), jobs ok / failed, the contents of `lab/runs/campaign/summary.md` (the ladder
   table and the seed table with intervals), the best size and why.
5. Hardware features used, stated plainly: the Rust map encoder calls Apple Accelerate (`cblas_sgemm`, one core per net) and
   a NEON path; training runs on the GPU through MLX. Nothing is hand-tuned for the M5 beyond the auto-tune of step 6.
6. Problems and surprises: anything that failed, any warning about memory or heat (`res status` lines with "thermal"), any
   deviation from this prompt.
Then pack the answer: `tar czf lab/m5-results.tgz lab/REPORT-m5.md lab/STATUS.md lab/machine.json lab/runs/campaign lab/runs/bench lab/runs/table-*.json tools/m5/campaign.toml`
(no recordings, no weights) and print its path. Roman copies `lab/m5-results.tgz` (and `lab/runs/*/army-net.bin` of the best net
if he asks) back to the M4 laptop; do not upload it anywhere.

Finish by stopping what you started (the status loop, the dashboard processes, `tools/res/res stop`) and leave the files in place.
