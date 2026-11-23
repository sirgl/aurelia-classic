# M5 experiment kit: runbook

## What the kit is

The army-net experiments of the M4 lab, packed so that they run on a second Apple-silicon Mac without the lab's
paths, files or habits. Everything is in this branch (`exp/m5-kit`, from `nn/army-net`, which holds the fast map
encoder); the recordings and nets are NOT in git and are regenerated here.

| Part | Where |
|---|---|
| Resource broker: CPU slots, one GPU lock, disk and RAM guards, a queue, a daemon | `tools/res/` (`res start`, `res run`, `res status`, `res info`) |
| Setup: Xcode CLT, rustup + toolchain 1.98.1, Python 3.12 venv with `mlx==0.32.4`, release binaries | `tools/m5/setup.sh` |
| Recipes (scripts) | `tools/m5/{teach,train,eval,table,bench,campaign}.sh` |
| Dashboard and watchers | `tools/dash/{serve,train_watch,eval_watch}.py`, `tools/dash/victory-share.py` |
| The overnight campaign | `tools/m5/campaign.sh`, `campaign.py`, `campaign.toml` |
| Design and hand-off of the line | `docs/army-net/M-HANDOFF.md`, `DESIGN.md`, `OBSERVATION-SCHEMA.md` |

Layout: `classic` (this repo) and `platform` side by side (Cargo needs `../platform`). Heavy output goes to `lab/`
(git-ignored): `lab/runs` (recordings, nets, eval rows), `lab/logs`, `lab/data` (the kept-instance list). Every path
is relative to the repo or set by an environment variable: `AURELIA_LAB`, `RES_HOME`, `M5_VENV`, `M5_PY`,
`AURELIA_EXAM_BIN`.

What differs from the M4 recipe: the M4 nets were also trained on DAgger recordings (`dagM1.bin`, `dagMBa1.bin`) and
initialised from the earlier M1 and B1 nets. Those need nets that do not exist here, so the kit trains MB from
scratch on the teacher recordings alone (behaviour cloning). Numbers are comparable between M5 runs and with the M4
speed numbers, not with the M4 gate tables. Read `docs/army-net/M-HANDOFF.md` ("Roman's decision", "MB design",
"quick eval") first; it describes the line and its gates.

## The broker

`tools/res/res start` detects the machine and starts the daemon: performance cores (`hw.perflevel0.physicalcpu`)
become the CPU budget (efficiency cores stay for the system), RAM comes from `hw.memsize`, the MLX memory limit is
60 % of RAM, on battery the budget is half the performance cores, a thermal state above nominal lowers the budget.
`res info` prints what it found. Override any of it in `tools/res/res.toml` (copy to `$RES_HOME/res.toml`).
`res run --cpu 2-8 --gpu --ram 12 --disk 25 --name X -- cmd...` waits for slots, the GPU lock (one MLX job at a time),
free RAM and free disk (default 25 GB), then runs the command with `RES_THREADS` set. `res queue ... -- cmd` puts a job
in the daemon's queue. Locks die with the process, so a killed job never blocks anything.

## One-time setup

```sh
git clone -b exp/m5-kit https://github.com/sirgl/aurelia-classic.git classic
git clone -b exp/m5-kit https://github.com/sirgl/aurelia-platform.git platform
cd classic
xcode-select --install          # once, if `xcode-select -p` fails; wait for it to finish
tools/m5/setup.sh               # 10-25 min the first time (the cargo build is most of it); safe to run again
```
`setup.sh` stops with an exact instruction when rustup or python3.12 is missing (`brew install python@3.12`; rustup from
https://rustup.rs). It writes `lab/venv-freeze.txt`. Then `tools/res/res start && tools/res/res status`.

## Recipes

Run from the `classic` directory. "M4 duration" is what the recipe took on the M4 Max laptop (14 threads, 64 GB).

### (a) Record the teacher data

```sh
tools/m5/teach.sh               # M4: ~3 min on 14 threads; output ~9.5 GB (train 7.5 GB, hold 1.9 GB)
```
Records the trainers' plays of the references and Unciv (record v4: history, track, memory states) for the 32 families:
hold = instances 80-99, train = instances 0-79. Output `lab/runs/teach4-{hold,train}.bin` and the `rows.jsonl` beside
them (the teachers' win rates; the gate tables compare nets with them). The `.done` markers make a second run a no-op;
delete `lab/runs/teach4-*` to re-record. Needs 40 GB free disk.

### (b) Train MB with checkpoints

```sh
tools/m5/train.sh mb-d64-l3                                  # M4: ~30 min (2 epochs, d 64, 3 layers)
M5_D=96 M5_LAYERS=3 M5_SEED=2 tools/m5/train.sh mb-d96-s2    # other sizes and seeds
```
MB = the transformer with memory plus the map encoder (`--map --map-ch 64`), from scratch, 2 epochs, `--plays 128`
(plays per step), lr 1e-3, `--up siege,ambush-fog,hunt-fog`. It runs in resumable GPU chunks (`M5_CHUNK_MIN`, default
30); every chunk ends with a checkpoint, `runs/NAME/ckpt/e<epoch>.safetensors` + its Rust `.bin`, and
`runs/NAME/state.json`. After a crash or a reboot run the same command: it resumes. Output `lab/runs/NAME/army-net.bin`
(the Rust net), `weights.safetensors`, `curves.jsonl`; log `lab/logs/NAME.log`. Knobs: `M5_EPOCHS`, `M5_LR`, `M5_PLAYS`,
`M5_MAX_MINUTES` (stop after N minutes, for tests), `M5_MLX_GB` (MLX limit; default 60 % of RAM).

### (c) Quick eval, full table

```sh
tools/m5/eval.sh quick mb-d64-l3 evq-mb-d64-l3                       # 12 families, M4: ~3 min
tools/m5/eval.sh full  mb-d64-l3 evf-mb-d64-l3                       # all 32 families, M4: ~20-30 min
tools/m5/eval.sh full  mb-d64-l3 evfz-mb-d64-l3 ARMY_NET_ZERO_MEMORY=1   # the memory-zeroed ablation
tools/m5/table.sh lab/runs/table-mb.json mb=lab/runs/evf-mb-d64-l3/rows.jsonl mbz=lab/runs/evfz-mb-d64-l3/rows.jsonl
```
Held-out instances 80-99, player `army-net`, `ARMY_PLAY_ON_CAP=1`. The table prints per family the best teacher and each
net; `*` = at least 90 % of the best teacher. The M-HANDOFF gates: MB at least 18 of 32 families, raid and prey-land at
least 85 %, the line's goal 22 of 32. The quick set is siege, ram, assault-heal, city-fire, hill-archers,
planned-assault, promotion-choice, rush (the 8 siege families) plus raid, prey, focus, snipe.

### (d) Dashboard and watchers

```sh
tools/res/res start
cd tools/dash
nohup python3 serve.py 8765 > ../../lab/logs/serve.log 2>&1 &          # http://127.0.0.1:8765/ (reloads every 15 s)
nohup python3 -I train_watch.py > ../../lab/logs/train_watch.log 2>&1 &  # alerts: slow, nan, diverge, plateau, stall, died, RAM
nohup python3 -I eval_watch.py watch 12 > ../../lab/logs/eval_watch.log 2>&1 &   # quick-evals each checkpoint (12 h)
```
The page shows curves, runs, the gate tables and, during a campaign, a "Campaign" panel. `victory-share.py FILE...`
turns `ai-tournament` outputs into the real-victory share. Stop a watcher with `kill <its pid>` (`pgrep -f serve.py`).

### (e) Speed benchmark: M5 against M4

```sh
tools/m5/bench.sh               # ~15 min, quiet machine, after (a)
```
Fixed commands: (1) MB d64 l3 training for 5 min: decisions/s (`dps_mean` of `curves.jsonl`); (2) the Rust trainer plays of
the 12 quick families (solvers, held-out 80-89) on 1 thread and on all performance cores: plays/s; (3) the net's
ms per AI turn (encode + forward + memory + observe + update over the turns of the quick-eval plays, 1 thread);
(4) one `ai-tournament` smoke suite on 1 thread: wall seconds (a CPU speed proxy for the classic AI turn).
Result file `lab/runs/bench/results.json`. Fill the template:

| Measure | M4 Max laptop | This machine |
|---|---:|---:|
| Chip, performance cores, RAM | M4 Max, 12 P + 4 E, 64 GB | |
| MB training, decisions/s (d64 l3) | 2 700-3 000 (MBa with heads: 1 900) | |
| Rust trainer plays/s, 1 thread | about 95 (dry run under load 15) | |
| Rust trainer plays/s, all performance cores | not measured (run bench.sh on the M4) | |
| Net ms per AI turn, 1 thread (MB) | 15 (M-HANDOFF; the B1 encoder alone 3.66) | |
| ai-tournament smoke suite, seconds, 1 thread | not measured | |
| Settings the auto-tune chose (plays per step, MLX limit) | 128, 12 GB | |

### (f) Overnight campaign

The long, unattended job: the paper-grade grid the M4 cannot afford.
```sh
tools/m5/campaign.sh start        # background, caffeinate keeps the Mac awake; log lab/logs/campaign.log
tools/m5/campaign.sh status       # state, done / total jobs, the last results, the estimate
tools/m5/campaign.sh stop         # between jobs; start again to resume
tools/m5/campaign.sh autostart on # optional: a LaunchAgent restarts it after a reboot (off: `autostart off`)
```
What it runs, through the broker, in this order (a net's evals follow its training, so results arrive early):
0. teacher data if missing (a);
1. **auto-tune + calibration (about 10 min)**: the broker budget is the performance cores, the MLX limit 60 % of RAM;
   MB training is tried at 128 / 256 / 512 plays per step for 60 s each, and the fastest that stays under the memory
   limit with a loss over the first steps within the noise of the 128 run is written into `campaign.toml`
   (`[train] plays`, a `[tune]` record). Then 3 min of training d64 and a quick eval on 4 instances give decisions/s,
   plays/s, seconds per epoch. It prints the estimated duration of the whole campaign (rough, about +-30 %);
2. the capacity ladder on the B line: MB at d 48 / 64 / 96 / 128 with 3 layers, d 128 with 4 and with 6 layers;
3. 3 independent seeds (11, 12, 13) of MB at the best ladder size (most families at 90 % of the teacher, then siege
   mean win, then fewer parameters);
4. for each net: the quick eval, the full 32-family held-out table, the memory-zeroed full table;
5. `lab/runs/campaign/summary.md`: families at >= 90 % of the teacher (of 32), the 8 siege families' mean win, decisions/s,
   ms per turn, peak RAM, with mean and 95 % interval over the seeds.

Every finished job appends a line to `lab/runs/campaign/results.jsonl`; the dashboard shows them. A crash or a reboot:
run `tools/m5/campaign.sh start` again. Jobs with an `ok` line are skipped, a training resumes from its last checkpoint,
a failed job is retried once and then recorded as `failed_final` (the rest goes on). Edit `campaign.toml` to change the
grid (epochs, seeds, the ladder). Dry run: `tools/m5/campaign.sh smoke 2`.
Rust evals use all performance cores when no training runs (the campaign never overlaps them); the dashboard's
`eval_watch` beside a training uses performance cores - 1.
Estimated duration: from the M4 numbers about 9 nets x (30-60 min training + 25-40 min evals), roughly 12-18 h; the
calibration prints the real estimate for this machine.

### (g) Stage 2: the MBf recipe, MLX speed, seeds, wider nets

`docs/m5/PROMPT-2.md` (the prompt), `tools/m5/campaign2.sh plan|start|status|summary`, config `tools/m5/campaign2.toml`,
recipes `tools/m5/train2.sh` (kinds a / b / m / mb), `tools/m5/setup2.sh` (newest-MLX venv), `tools/m5/py/{make_init_mb_from,same_plays,mem_norms}.py`.
Results in `lab/runs/campaign2/`.

## Memory and disk rules (learned the hard way)

- One MLX training and at most one probe (`probe_m`, `probe_layers`) at a time; the GPU lock enforces one per broker. No
  second training "because there is room".
- Stream the recordings: `train_m` reads the `.bin` files indexed and memory-mapped (RAM about the index). Never load
  them whole; do not copy them; past about 32 GB of data split it.
- Memory limits: `train_m --mem-gb N` sets the MLX memory limit and a 2 GB cache (`mx.set_memory_limit`,
  `set_cache_limit`); the scripts pass the broker's value (`RES_MLX_GB`, 60 % of RAM) and reserve the same through
  `res run --ram`. Probes use the same caps.
- Keep at least 25 GB of free disk (the broker's default); the recordings are about 10 GB and each run's checkpoints a
  few hundred MB. Delete `target/` and old `lab/runs/*/ckpt` when short.
- macOS has no `timeout`; run long things as background jobs; never a foreground `sleep`. Kill only your own PIDs.

## Links

[M-HANDOFF.md](army-net/M-HANDOFF.md) (the line, the gates, the commands of the M4 lab),
[DESIGN.md](army-net/DESIGN.md), [OBSERVATION-SCHEMA.md](army-net/OBSERVATION-SCHEMA.md). The M-HANDOFF names the M4 lab's
script paths (`runs/m-quick.sh` and so on); in this kit the same steps are `tools/m5/*.sh`.
