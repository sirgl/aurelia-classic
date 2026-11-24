#!/usr/bin/env bash
# Recipe (a): record the teacher data (the trainers' plays of the references and Unciv, record v4) into
# $AURELIA_LAB/runs/teach4-{hold,train}.bin (+ rows.jsonl beside them). Idempotent: a finished split is skipped.
#   tools/m5/teach.sh            (full: hold = instances 80-99, train = 0-79; ~3 min on 14 threads, ~25 GB of .bin)
#   M5_SMOKE=1 tools/m5/teach.sh (a few instances, for a dry run)
set -eu
. "$(dirname "$0")/env.sh"
m5_res_up
[ -x "$M5_EXAM" ] || { echo "teach: build first (tools/m5/setup.sh)"; exit 1; }
if [ -n "${M5_SMOKE:-}" ]; then HOLD_TO=${M5_HOLD_TO:-82}; TRAIN_TO=${M5_TRAIN_TO:-3}; else HOLD_TO=100; TRAIN_TO=80; fi
cd "$AURELIA_LAB"
record() {  # NAME FROM TO
  if [ -f "runs/$1.done" ]; then echo "teach: $1 already recorded"; return; fi
  rm -rf "runs/$1" "runs/$1.bin"
  "$M5_RES" run --cpu 4-"${M5_MAX_CPU:-$(sysctl -n hw.perflevel0.physicalcpu)}" --disk 40 --name "teach-$1" -- sh -c \
    "\"$M5_EXAM\" suite --kept data/kept-m.jsonl --from $2 --instances $3 --players solvers,unciv --out runs/$1 --record runs/$1.bin --jobs \$RES_THREADS" | tail -1
  touch "runs/$1.done"
}
record teach4-hold 80 "$HOLD_TO"
record teach4-train 0 "$TRAIN_TO"
ls -la runs/teach4-*.bin
