# Prompt 2 for the Claude Code session on the M5 Mac: stage 2

You are Claude Code on the Apple M5 Max that ran stage 1 (`docs/m5/PROMPT.md`). Roman pasted this prompt and left. Stage 2
runs by itself, unattended, and ends with a report pushed as one new branch. Read this whole file first, then work through
the numbered steps. You are the only agent on this machine for this job.

## Why stage 2

Stage 1 trained the MB net **from scratch** on the teacher recordings. It reached hold accuracy 0.68-0.69 and 2-5 of 13 quick
families at 90 % of the teacher. The M4 lab's **MBf recipe** reaches hold accuracy 0.839, assault-heal 65 %, raid 98 %,
planned-assault 21 % and 8 of 13 quick families. So stage 1's from-scratch ladder is only a **lower bound**. Stage 2 builds
the recipe that worked and then measures it properly:

| Step | What | Where it is described |
|---|---|---|
| 1 | The MBf recipe once: A0 -> M0 (memory), B -> B1 (map encoder), MBf = M0's trunk + memory + B1's encoder with zero projections, imitation only, no DAgger; quick eval, 32-family tables, memory-zeroed ablation; compare with the M4 | `docs/army-net/M-HANDOFF.md`, section "B-line restart" |
| 2 | MLX speed: pinned vs newest MLX x f32 / TF32 / bf16 mixed precision, 200 steps each | below |
| 3 | 5 seeds at the best stage-1 ladder size, MBf recipe, 4 epochs of MBf; full tables, memory-zeroed ablation, intervals | below |
| 4 | Wider nets d 192 and d 256 (3 layers), MBf recipe, from scratch (the kit has no growth script) | below |
| 5 | Report `docs/m5/STAGE2-RESULTS.md` + results, pushed as the new branch `exp/m5-stage2-result` | below |

The recipe in numbers (all in `tools/m5/campaign2.toml`, run by `tools/m5/train2.sh`): A0 = the transformer without map or
memory, 10 epochs, bs 512, lr 1e-3 (`aurelia_nn.army.train`). M0 = A0 + memory (`train_m`, no map), 6 epochs, 128 plays per step, lr 1e-3, curriculum 4 -> 8 -> full turns,
init A0. B = A + the 64-channel hex-conv map encoder, 6 epochs, lr 1e-3; B1 = B, 2 more epochs at lr 5e-4, siege x2 (the
encoder does not depend on d, so one B1 at d 64 serves every width). init-MB = `tools/m5/py/make_init_mb_from.py`: M0's weights + B1's encoder, `m_tok`, `m_cand`, `m_pool` zero, so it plays exactly as M0 (step 1
checks this with `tools/m5/py/same_plays.py`). MBf = `train_m --map --map-ch 64` from init-MB, lr 3e-4 cosine, curriculum
`999:1`, 128 plays per step, siege + ambush-fog + hunt-fog weighed x2, imitation only (teach4 recordings, no DAgger).
Differences from the M4 lab, stated in the report: the kit's A0 and M0 are trained here on the teach4 recordings (the M4's
M0 used an older recording and an AH0 init), B1 has no DAgger data, the teachers' best rates come from `teach4-hold` only.
Numbers are comparable with the M4 in kind, not digit for digit.

M4 reference for step 1 (MBf, held-out, quick eval): assault-heal 65 % (memory zeroed 39 %), planned-assault 21 %, raid 98 %, 8 of 13
quick families at >= 90 % of the best teacher, 17 of 32 in the full table, hold accuracy 0.839, 3.07 ms per AI turn, 3.45 k decisions/s training.
M4's M0 for orientation: assault-heal 71 %, planned-assault 15 %.

## Hard rules (the same as stage 1, with one change: rule 5)

1. Every heavy job (cargo, training, evals, benchmarks) runs through the vendored broker `tools/res/res run ...`; the scripts in `tools/m5/` already do. Never start a training or an eval by hand outside it.
2. One MLX training at a time and at most one probe; never two GPU jobs. If the GPU lock is busy, wait.
3. Stream recordings (the trainer does); never load the `.bin` files whole by hand, never copy them. Keep the MLX memory and cache limits the scripts pass (`--mem-gb`, `RES_MLX_GB`). `train.py` (A0, B, B1) holds the decisions in RAM, about 10 GB: expected, and reserved through `--ram`.
4. Keep at least 25 GB of free disk at all times (the broker waits below it). If you are short, delete `target/` first, then old `lab/runs/*/ckpt` and `lab/runs/tune-*`, `lab/runs/speed-*`; never delete `lab/runs/campaign`, `lab/runs/campaign2` or the recordings `teach4-*`.
5. **The only push you do is the one of step 5**: the new branch `exp/m5-stage2-result`, from the command given there. No other push, no force, no other remote branch, no uploads, no gists. Nothing but the files named in step 5 is committed.
6. macOS has no `timeout`; run long things as background jobs; never a foreground `sleep` (poll the status file). Kill only PIDs you started yourself, never `killall` or `pkill` by a broad name.
7. Do not change the code of the game, the trainer or the net. If something is broken, record it in the report with the log lines; fix only paths and permissions of the kit itself.
8. Do not install anything globally except what `setup.sh` / `setup2.sh` do (they only write into the repo: `.venv-m5`, `.venv-m5-new`, `target`). If something needs a password or a GUI click that you cannot give, stop and write it into `lab/STATUS.md`.

## Step 0. Update the kit and check stage 1 (5 min)

```sh
cd ~/aurelia-lab/classic
tools/m5/campaign.sh status                        # stage 1 must say "finished"; if it is still running, wait (poll every 10 min), do not start stage 2 beside it
git diff tools/m5/env.sh > lab/env-local.diff      # your local env.sh change (MLX_ENABLE_TF32=0); the branch now carries it
git checkout -- tools/m5/env.sh                    # (a file of this clone only)
git stash list | head -2; git status --short       # campaign.toml may stay modified (the stage-1 auto-tune); that is fine
git pull --no-rebase origin exp/m5-kit 2>&1 | tail -3   # a merge, never a rebase; platform: cd ../platform && git pull --no-rebase origin exp/m5-kit
cd ~/aurelia-lab/classic && git log --oneline -5 && grep -n TF32 tools/m5/env.sh
tools/m5/setup.sh --no-build 2>&1 | tail -3        # the pinned venv is untouched; rebuild is not needed unless Rust changed
tools/res/res start && tools/res/res status
```
If `git pull` refuses because of local changes, `git stash push -- tools/m5/campaign.toml`, pull, `git stash pop`. If the pull
brings Rust changes (`git diff --stat HEAD@{1} -- crates | tail -1` is not empty), rebuild: `tools/m5/setup.sh`.
Start the status loop (same as stage 1, now for stage 2) and note its PID in `lab/STATUS.md`:
```sh
nohup sh -c 'while :; do (date; tools/m5/campaign2.sh status; tools/res/res status; df -h . | tail -1) > lab/STATUS2.md 2>&1; sleep 3600; done' >/dev/null 2>&1 &
```

## Step 1. Dry plan and the estimate (2 min)

```sh
tools/m5/campaign2.sh plan | tee lab/logs/plan2.txt | tail -4
```
It reads stage 1's `lab/runs/campaign/results.jsonl`: the best ladder size (the `pick` line), the calibration (`sec_per_epoch`, quick-eval time),
and prints every job with its estimated minutes, then the total and its parts (hours). If it says `stage 1 found: False`, stage 1 did not finish: stop and write that into `lab/STATUS2.md`.
Copy the total into `lab/STATUS2.md` as "estimate at the start". The campaign prints it again as its `estimate` job and refines it after the speed step. If the estimate is above 30 h, cut in `tools/m5/campaign2.toml`: step 3 `seeds` to 3 entries, and say so in the report.

## Step 2. Run stage 2 (hours)

```sh
tools/m5/campaign2.sh start
tools/m5/campaign2.sh status
```
The queue (all through the broker; resumable: finished jobs are skipped, a training resumes from its checkpoint, a failed job is retried once and then recorded as `failed_final`):

1. `teach` (a no-op when the recordings exist) and `estimate`.
2. **Step 1 of the table above, the MBf recipe** (numerics: pinned MLX, f32, `MLX_ENABLE_TF32=0`, as stage 1):
   `b:enc`, `b1:enc` (the shared encoder), `a0:s2-d64-l3`, `m0:s2-d64-l3`, `initmb:`, `mbf:`, quick evals of M0 and of init-MB (`same:` must print `equal N / N`: the function-preserving check),
   quick, quick memory-zeroed, full 32-family, full memory-zeroed of MBf, and `memnorms:` (does the memory reach the army tokens: `lab/runs/campaign2/mem-norms-*.json`).
3. **Step 2 of the table, MLX speed**: `speed-setup` (`tools/m5/setup2.sh`: a second venv `.venv-m5-new` with the newest `mlx`; versions and the macOS version go to `lab/runs/campaign2/versions.json`), then six runs
   `speed:{pinned,new}-{f32,tf32,bf16}`: MB d 64 / 3 layers, seed 1, 200 steps, the same data order, `--log-every 10`. f32 = `MLX_ENABLE_TF32=0`, tf32 = `MLX_ENABLE_TF32=1`,
   bf16 = `train_m --bf16` (`nn/aurelia_nn/army/mixed.py`: matmuls and the map convolutions in bfloat16; weights, optimizer, memory state and its GRUs, softmax, losses in float32).
   `speed-pick` compares each with the reference (pinned, f32): decisions/s over steps 101-200, the late training loss (a combination is correct when it is within 3 % of the reference and its held-out joint accuracy within 0.01), the worst point of the loss curve; keeps the fastest correct one when it is at least 5 % faster, else the reference.
   It writes `lab/runs/campaign2/speed.json` and prints the choice. **Every later training of stage 2 (M0, MBf of steps 3 and 4) uses that choice.** If the newest MLX does not install or the code does not import on it, the `new-*` rows say "skipped" with the reason: report it.
4. **Step 3 of the table, seeds**: at the best stage-1 size (d, layers from stage 1's `pick`), seeds 21-25: A0, M0, init-MB, MBf (4 epochs), quick, quick memory-zeroed, full, full memory-zeroed. The encoder B1 is shared by the seeds (`seed_encoders = false`); the intervals therefore do not include the encoder's variance: write that in the report. (`seed_encoders = true` retrains B and B1 per seed, about +1 h each.)
5. **Step 4 of the table, wider nets**: d 192 and d 256, 3 layers, seeds 31 / 32, the full chain at that width, from scratch (A0 at that width, M0 from it, init-MB with the shared B1 encoder), MBf 4 epochs, the same evals. The kit has no function-preserving growth script: say "from scratch" in the report.
6. `summary` -> `lab/runs/campaign2/summary.md` (speed table, step-1 table against the M4, seed table with mean and 95 % interval, wide table).

**Status file.** Every hour (the loop of step 0) and at every job end: elapsed hours, jobs done / total, the current job, the estimate left, free disk, `tools/res/res status`, the last 3 lines of `lab/runs/campaign2/results.jsonl`, any failure. Check the campaign at least every 30-60 minutes: is the PID alive (`lab/logs/campaign2.pid`), is a job stuck (no new line in `lab/logs/*.log` for 20 min), is the disk above 25 GB, any `thermal` line in `res status`.
**Crash or reboot.** Run `tools/m5/campaign2.sh start` again. If the whole campaign dies three times in a row for the same reason, stop and report. Do not edit the code to make a job pass. If the bf16 job fails with an error in `mixed.py` or the model, that is a finding: the combination is `failed_final` and the speed step keeps the best of the others.

## Step 3. Check the results while they come (do not skip)

After step 1 of the queue (the MBf recipe) finishes, read `lab/runs/campaign2/summary.md` and `results.jsonl` yourself:
`same:` identical? MBf hold accuracy near 0.84 and assault-heal near 65 % (the M4)? If MBf is far below (hold accuracy < 0.78 or assault-heal < 45 %), do not stop the queue; write the numbers into `lab/STATUS2.md` first line "MBf BELOW THE M4", and in the report say which stage is the likely cause (A0 and M0 accuracy in `lab/runs/a0-*/curves.jsonl`, `m0-*/curves.jsonl`, the `memnorms` json).

## Step 4. The report `docs/m5/STAGE2-RESULTS.md` (English)

Sections: 1. Machine and versions (chip, cores, RAM, macOS, MLX pinned / newest, from `versions.json`). 2. The MBf recipe on the M5 against the M4 (the step-1 table; M0, init-MB, MBf; memory ablation; the function-preserving check line; mem-norms). 3. MLX speed: the six-row table with decisions/s, loss difference to f32, the choice and why (and the time of the whole stage that it saved or not). 4. Seeds: the table, mean +- 95 % interval, the full per-family table of one seed, the memory-zeroed gap, the caveat about the shared encoder. 5. Wider nets: the table against the best size. 6. Stage 1's from-scratch ladder as a **lower bound** (one paragraph with its numbers from `lab/runs/campaign/summary.md`). 7. Duration: estimated at the start, after the speed step, actual. 8. Problems, deviations from this prompt, every failed job with its log lines.
Plain statements, numbers from the files, no guesses; mark what you did not verify.

## Step 5. Push the one new branch

```sh
cd ~/aurelia-lab/classic
git switch -c exp/m5-stage2-result            # from the commit you run on; this is the only branch you create
mkdir -p docs/m5/stage2 && cp lab/runs/campaign2/{results.jsonl,summary.md,speed.json,versions.json,status.json} docs/m5/stage2/ 2>/dev/null
cp lab/runs/campaign2/table-*.json lab/runs/campaign2/mem-norms-*.json docs/m5/stage2/ 2>/dev/null
cp lab/runs/campaign/results.jsonl docs/m5/stage2/stage1-results.jsonl
git add -f docs/m5/STAGE2-RESULTS.md docs/m5/stage2 tools/m5/env.sh tools/m5/campaign.toml tools/m5/campaign2.toml
git status --short | head -30
```
Checks before the commit (all must pass; fix the files, not the checks):
```sh
git diff --cached --stat | tail -3                                      # only small text files; no .bin, no .safetensors
git diff --cached | grep -n -E '/Use''rs/|/ho''me/|BEGIN [A-Z ]*PRIVATE'' KEY|gh''p_|github''_pat_|sk-[A-Za-z0-9]{20}|AK''IA[0-9A-Z]{12}' | head   # secrets and absolute home paths: must print nothing
git grep -i -E 'vo''x populi|community'' patch|\bV''P\b' | head      # another game's mod: must print nothing
```
If a copied json holds an absolute path of this Mac (for example `py` in `speed.json`), replace it by a relative one with `sed` in the copy. Then:
```sh
git commit -m "m5: stage 2 results (MBf recipe, MLX speed, seeds, wider nets)

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
git push https://github.com/sirgl/aurelia-classic.git exp/m5-stage2-result
```
If the push is refused (no access), write "need GitHub access" into `lab/STATUS2.md`, pack `tar czf lab/m5-stage2-results.tgz docs/m5/STAGE2-RESULTS.md docs/m5/stage2` and print its path. Do not push anywhere else.

## Step 6. Finish

Stop what you started (the status loop, dashboard processes, `tools/m5/campaign2.sh stop` if needed, `tools/res/res stop`), leave the files in place, and print in the last message: the branch name and commit, the stage-2 duration, the table of step 1 against the M4 (4 lines), the kept MLX / precision, and the best seed-table row.
