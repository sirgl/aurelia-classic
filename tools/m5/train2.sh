#!/usr/bin/env bash
# Stage 2 recipes (the MBf line), resumable. One script for the five nets of the chain:
#   tools/m5/train2.sh NAME KIND [extra args of the trainer...]
#   KIND a   A0: the transformer without map or memory      (aurelia_nn.army.train, one GPU job)
#        KIND b   B / B1: A plus the hex-conv map encoder, 64 channels (train, --map, one GPU job; d must be 64)
#        KIND m   M0: A plus memory, from A0 (--init)         (train_m, resumable chunks)
#        KIND mb  MBf: M plus the map encoder, from init-MB   (train_m --map --map-ch 64, resumable chunks)
# Knobs (environment): M5_D M5_LAYERS M5_SEED M5_EPOCHS M5_LR M5_PLAYS M5_INIT (weights to start from) M5_BF16=1
#   (mixed precision, train_m kinds) M5_MAX_STEPS M5_LOG_EVERY M5_EVAL_EVERY M5_CHUNK_MIN M5_MAX_MINUTES M5_PY.
# Defaults per kind are the M4 lab's: a 10 epochs lr 1e-3 bs 512 | b 6 epochs lr 1e-3 bs 512 | m 6 epochs lr 1e-3
#   plays 128 (curriculum 4 -> 8 -> full turns) | mb 2 epochs lr 1e-3 plays 128, siege + ambush-fog + hunt-fog x2.
# Output $AURELIA_LAB/runs/NAME/{army-net.bin,weights.safetensors,curves.jsonl,...}, log logs/NAME.log.
# A crash or reboot: run the same command. m / mb resume from their checkpoint; a / b restart (they are short).
set -eu
. "$(dirname "$0")/env.sh"
NAME=$1; KIND=$2; shift 2
m5_res_up
OUT=$M5_RUNS/$NAME; LOG=$M5_LOGS/$NAME.log
D=${M5_D:-64}; L=${M5_LAYERS:-3}; SEED=${M5_SEED:-1}
for f in teach4-train.bin teach4-hold.bin; do [ -f "$M5_RUNS/$f" ] || { echo "train2: $f missing (tools/m5/teach.sh)"; exit 1; }; done
GB=$(m5_mlx_gb)
DATA="--train $M5_RUNS/teach4-train.bin --hold $M5_RUNS/teach4-hold.bin"
INIT=""; [ -n "${M5_INIT:-}" ] && INIT="--init $M5_INIT"
case "$KIND" in
  a|b)
    [ -f "$OUT/.done" ] && { echo "train2: $NAME already done"; exit 0; }
    MAP=""; [ "$KIND" = b ] && { MAP=--map; [ "$D" = 64 ] || { echo "train2: kind b needs M5_D=64"; exit 1; }; }
    rm -rf "$OUT"; mkdir -p "$OUT"; echo "== $KIND $(date +%H:%M:%S)" >> "$LOG"
    cd "$M5_NN"
    "$M5_RES" run --gpu --cpu 2 --ram "$GB" --name "train-$NAME" -- /usr/bin/time -l "$M5_PY" -m aurelia_nn.army.train \
      --out "$OUT" $DATA $MAP $INIT --d "$D" --layers "$L" --heads 4 --ff $((2 * D)) --seed "$SEED" \
      --epochs "${M5_EPOCHS:-$([ "$KIND" = a ] && echo 10 || echo 6)}" --bs 512 --lr "${M5_LR:-1e-3}" \
      --eval-every "${M5_EVAL_EVERY:-2000}" --log-every "${M5_LOG_EVERY:-200}" "$@" --mem-gb "$GB" >> "$LOG" 2>&1 || { echo "== failed" >> "$LOG"; exit 1; }
    touch "$OUT/.done"; echo "== done $(date +%H:%M:%S)" >> "$LOG" ;;
  m|mb)
    if [ "$KIND" = mb ]; then
      KARGS="--map --map-ch 64 --up siege,ambush-fog,hunt-fog --up-factor 2 --curriculum 4:0.1,8:0.1,999:0.8"; EP=2; EV=600; LG=25
    else
      KARGS=""; EP=6; EV=700; LG=50
    fi
    [ -n "${M5_BF16:-}" ] && [ "${M5_BF16:-0}" != 0 ] && KARGS="$KARGS --bf16"
    [ -n "${M5_MAX_STEPS:-}" ] && KARGS="$KARGS --max-steps $M5_MAX_STEPS"
    CHUNK=${M5_CHUNK_MIN:-30}; DONE_MIN=0
    cd "$M5_NN"
    for job in $(seq 1 80); do
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
        --out "$OUT" $R $DATA $INIT $KARGS --d "$D" --layers "$L" --heads 4 --seed "$SEED" \
        --epochs "${M5_EPOCHS:-$EP}" --plays "${M5_PLAYS:-128}" --lr "${M5_LR:-1e-3}" --eval-every "${M5_EVAL_EVERY:-$EV}" \
        --log-every "${M5_LOG_EVERY:-$LG}" --eval-plays 4 "$@" --mem-gb "$GB" --max-minutes "$LEFT" >> "$LOG" 2>&1 || { echo "== job $job failed" >> "$LOG"; exit 1; }
      [ -n "${M5_MAX_STEPS:-}" ] && break
      DONE_MIN=$("$M5_PY" -I -c "print($DONE_MIN + ($(date +%s) - $START) / 60)")
    done
    echo "== done $(date +%H:%M:%S)" >> "$LOG" ;;
  *) sed -n 2,12p "$0"; exit 1 ;;
esac
