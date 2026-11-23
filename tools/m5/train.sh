#!/usr/bin/env bash
# Recipe (b): train an MB net (the B line: transformer + map encoder + memory) in resumable GPU chunks.
#   tools/m5/train.sh NAME [train_m args that override the defaults...]
# Output: $AURELIA_LAB/runs/NAME/{army-net.bin,weights.safetensors,curves.jsonl,ckpt/...}, log in logs/NAME.log.
# Defaults (the ladder recipe of the M4 lab, trained from scratch on the teacher data): d 64, 3 layers, 2 epochs,
# 128 plays a step, lr 1e-3. Override with M5_D, M5_LAYERS, M5_EPOCHS, M5_SEED, M5_LR, M5_PLAYS, M5_CHUNK_MIN
# (minutes per GPU job, default 30), M5_MAX_MINUTES (stop the whole run after this training time, for smoke runs).
# A crash or a reboot: run the same command again; it resumes from OUT/state.json and the last checkpoint.
set -eu
. "$(dirname "$0")/env.sh"
NAME=$1; shift
m5_res_up
OUT=$M5_RUNS/$NAME; LOG=$M5_LOGS/$NAME.log
CHUNK=${M5_CHUNK_MIN:-30}
for f in teach4-train.bin teach4-hold.bin; do [ -f "$M5_RUNS/$f" ] || { echo "train: $f missing (tools/m5/teach.sh)"; exit 1; }; done
GB=$(m5_mlx_gb)
cd "$M5_NN"
DONE_MIN=0
for job in $(seq 1 60); do
  R=""
  if [ -f "$OUT/state.json" ]; then
    "$M5_PY" -I -c "import json,sys; s=json.load(open('$OUT/state.json')); sys.exit(0 if s['step'] >= s['steps'] else 1)" && break
    R=--resume
  fi
  LEFT=$CHUNK
  if [ -n "${M5_MAX_MINUTES:-}" ]; then
    LEFT=$("$M5_PY" -I -c "print(min($CHUNK, $M5_MAX_MINUTES - $DONE_MIN))")
    "$M5_PY" -I -c "import sys; sys.exit(0 if $LEFT > 0.05 else 1)" || break
  fi
  echo "== job $job $(date +%H:%M:%S) $R" >> "$LOG"
  START=$(date +%s)
  "$M5_RES" run --gpu --cpu 2 --ram "$GB" --name "train-$NAME" -- /usr/bin/time -l "$M5_PY" -m aurelia_nn.army.train_m \
    --out "$OUT" $R --map --map-ch 64 \
    --d "${M5_D:-64}" --layers "${M5_LAYERS:-3}" --seed "${M5_SEED:-1}" \
    --train "$M5_RUNS/teach4-train.bin" --hold "$M5_RUNS/teach4-hold.bin" \
    --up siege,ambush-fog,hunt-fog --up-factor 2 --epochs "${M5_EPOCHS:-2}" --plays "${M5_PLAYS:-128}" --lr "${M5_LR:-1e-3}" \
    --curriculum 4:0.1,8:0.1,999:0.8 --eval-every "${M5_EVAL_EVERY:-600}" --log-every 25 --eval-plays 4 \
    "$@" --mem-gb "$GB" --max-minutes "$LEFT" >> "$LOG" 2>&1 || { echo "== job $job failed" >> "$LOG"; exit 1; }
  DONE_MIN=$("$M5_PY" -I -c "print($DONE_MIN + ($(date +%s) - $START) / 60)")
done
echo "== done $(date +%H:%M:%S)" >> "$LOG"
