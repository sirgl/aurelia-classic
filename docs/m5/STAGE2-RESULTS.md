# Stage 2 results: Apple M5 Max (2026-11-24)

Stage 2 ran unattended from 01:31 to 07:01 local time (5.5 h; estimate at the start 9.7 h, +-40 %). 67 jobs ok, 0 failed, 7 skipped by the deadline
(all of the d 256 chain). Numerics: pinned MLX 0.32.4, plain fp32 (`MLX_ENABLE_TF32=0`), as in stage 1.

## 1. Machine and versions
Apple M5 Max, 6 performance + 12 efficiency cores, 128 GB RAM, macOS 27.2.1 (26B131), on charger. MLX pinned: mlx 0.32.4 + mlx-metal 0.32.4.
The "newest" MLX installed into `.venv-m5-new` is also 0.32.4 (it is the newest release in the index), so the pinned-vs-new rows below are repeats
of the same code: their differences (up to 3.8 % in the late loss) are the noise of a 200-step run, not a version effect.

## 2. The MBf recipe on the M5 against the M4 (d 64, 3 layers, seed 1, quick eval 80-99 held-out)
| measure | M5 | M4 (MBf) |
|---|---:|---:|
| hold accuracy (unit and action) | 0.8364 | 0.839 |
| quick families >= 90 % of the best teacher | 8 of 13 | 8 of 13 |
| full families >= 90 % | 16 of 33 | 17 of 32 |
| full families >= 90 %, memory zeroed | 15 | n/a |
| assault-heal win (quick) | 0.671 | 0.65 |
| planned-assault win (quick) | 0.228 | 0.21 |
| raid win (quick) | 0.988 | 0.98 |
| ms per AI turn | 3.21 (quick) | 3.07 |
| training decisions/s | 4622 | 3450 |
Stages: B encoder hold acc 0.828 (45 min), A0 0.805 (4.5 min), M0 0.819 (7 min), MBf 0.836 (15.7 min).
Function-preserving check: `same:` printed `equal 1619 / 1619 (only in A 0, only in B 0)`: init-MB plays exactly as M0.
Memory ablation: assault-heal is 0.671 (94 of 140) for M0, init-MB, MBf and MBf with the memory zeroed, identical; the M4 lab saw 0.65 -> 0.39.
So the memory does not move assault-heal here; the full table loses one family (16 -> 15) and siege 8 changes little. I did not find the cause.
Mem-norms (`mem-norms-s2-d64-l3.json`): for M0 the memory token norm is 0.74 against 4.5 for the entity embedding (ratio 0.164); for MBf 0.78 against 4.6 (ratio 0.169), the map token norm is 0.37.
Stage 1's from-scratch nets reached hold accuracy 0.64-0.69 and 2-5 of 13 quick families: that ladder is a lower bound; the recipe is what closes the gap.
Differences from the M4 lab (as in the prompt): A0 and M0 trained on the teach4 recordings, B1 without DAgger data, teacher rates from `teach4-hold` only.
The kit's `summary.md` prints the step-1 table three times; the 2nd and 3rd blocks (with `None` cells, raid 0.5, 5 of 13) are M0 and init-MB under the MBf header
(a bug of the summary builder, not changed). Values above are the first block, checked against `table-evq*-mbf-s2-d64-l3.json`.

## 3. MLX speed (MB d 64 / 3 layers, 200 steps, seed 1, same data order)
| combination | decisions/s (steps 101-200) | x reference | late loss | loss diff | hold acc | correct |
|---|---:|---:|---:|---:|---:|---|
| pinned-f32 | 4564 | 1.000 | 1.347 | 0.0 % | 0.650 | yes |
| pinned-tf32 | 4608 | 1.010 | 1.286 | 4.5 % | 0.654 | no (loss) |
| pinned-bf16 | 4707 | 1.031 | 1.382 | 2.6 % | 0.645 | yes |
| new-f32 | 4576 | 1.003 | 1.296 | 3.8 % | 0.653 | no (loss) |
| new-tf32 | 4630 | 1.014 | 1.382 | 2.6 % | 0.648 | yes |
| new-bf16 | 4757 | 1.042 | 1.379 | 2.4 % | 0.643 | yes |
Choice: **pinned-f32** (the fastest correct variant, new-bf16, is only 4.2 % faster, below the 5 % bar). It saved no stage time. pinned-f32 and new-f32 are the same code and differ by 3.8 % in the
late loss: the 3 % bar is inside the noise of 200 steps. Neural Accelerators: MLX 0.32.4 has NAX kernels and they are active on this chip (gen 17, macOS >= 26.2). A matmul micro-test
gave 46.6 TFLOPS (fp16) and 37.7 (fp32) at 4096x2048x2048 against 13.9 / 13.6 with the architecture forced to M4; at this net's sizes (128-512) the gain is 0-14 %,
so the training speed is bound by overheads, not by matmul. The conv encoder of the B variant computes in bf16 (mixed precision, in `model.py`).

## 4. Seeds 21-25 (d 48, 3 layers, MBf recipe, 2 epochs; the B1 encoder is shared by the seeds, so the intervals do not include the encoder's variance)
| metric | mean | +- 95 % | n |
|---|---:|---:|---:|
| hold accuracy | 0.818 | 0.006 | 5 |
| quick families >= 90 % | 7.2 | 1.0 | 5 |
| full families >= 90 % (of 33) | 15.2 | 1.0 | 5 |
| siege 8 mean win | 0.756 | 0.019 | 5 |
| assault-heal | 0.666 | 0.062 | 5 |
| planned-assault | 0.257 | 0.068 | 5 |
| raid | 0.959 | 0.031 | 5 |
| assault-heal, memory zeroed | 0.624 | 0.087 | 5 |
| ms per AI turn | 2.58 | 0.22 | 5 |
Per seed (hold acc / quick / full): s21 0.816 / 8 / 15; s22 0.825 / 8 / 16; s23 0.822 / 7 / 15; s24 0.818 / 6 / 16; s25 0.813 / 7 / 14. d 48 matches d 64 (0.836, 8, 16) within about 0.02.
Per-family tables: `docs/m5/stage2/table-evf-mbf-s3-d48-l3-s2*.json`.

## 5. Wider nets
| net | params | hold acc | quick >= 90 % | full >= 90 % | zeroed full >= 90 % | siege 8 | ms/turn | decisions/s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| mbf-w-d192-l3 (4 epochs, from scratch, 1 seed) | 1 959 562 | 0.8241 | 6 | 13 | 14 | 0.742 | 18.5 (quick) / 13.0 (full) | 3909 |
d 192 is not better than d 48 / d 64 (hold acc 0.824 against 0.82-0.84, 13 against 15-16 families) and its turn is 5-7x slower. d 256 did not run (see 7). No growth script exists: from scratch.

## 6. Stage 1 as a lower bound
Stage 1 trained MB from scratch (2 epochs): hold accuracy 0.644-0.686, d 48 best (11 of 32 full families, 5 of 12 quick); 3 seeds at d 48: 10.0 +- 4.3 full families at >= 90 %.
The MBf recipe reaches 15.2 +- 1.0 at the same size and 0.818 hold accuracy: stage 1's ladder understates what the net can do.

## 7. Duration
Estimated at the start 9.7 h, actual 5.5 h (01:31-07:01). The estimates were too high for A0, M0 and the wide nets (a0 d192: 9 min against 22.5; m0 d192: 14 against 33; mbf d192: 37 against 95).
Cut by the deadline (7 jobs, all `skipped_deadline` at 07:01 with 14 min left): a0, m0, initmb, mbf, quick, full, fullzero of d 256. Seeds 24-25 and d 192 ran in full.

## 8. Problems and deviations
- `tools/dash/eval_watch.py:155` raises `NameError: name 'DAEMON' is not defined` every 2 min (dashboard watcher only; the campaign's own evals are separate). Code not changed (rule 7).
- The summary builder's step-1 table repeats M0 / init-MB blocks under the MBf header (section 2).
- Spotlight (`mds`) loaded the CPU after the reboot (250-500 %); `.metadata_never_index` markers were placed in the lab folders. Speeds may be a little low.
- Speed step: "new" MLX equals the pinned one (section 1). The loss bar of 3 % is inside the noise.
- No failed jobs; no thermal lines in `res status`. A quick stage-1 restart (plain fp32 instead of TF32) happened before stage 2 and is described in `lab/REPORT-m5.md`.
- Not verified: why memory zeroing leaves assault-heal unchanged; whether the wide nets would gain with more epochs.
