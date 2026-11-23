#!/usr/bin/env bash
# Recipe (c): the held-out trainer evals of a net (Rust, instances 80-99, player army-net).
#   tools/m5/eval.sh quick NET OUT [ENV=VAL ...]   12 families, ~3-10 min (the quick eval of M-HANDOFF)
#   tools/m5/eval.sh full  NET OUT [ENV=VAL ...]   all 32 families (~30-60 min)
# NET = a run name under lab/runs (its army-net.bin); OUT = the output run dir name. Extra ENV: ARMY_NET_ZERO_MEMORY=1
# M5_FAMILIES=a,b limits the families (a dry run). ARMY_NET_ZERO_MEMORY=1 is the memory-zeroed ablation. M5_JOBS=N limits the threads (default: all performance cores when no training runs; the campaign never overlaps them;
# eval_watch, which runs beside a training, takes performance cores - 1).
set -eu
. "$(dirname "$0")/env.sh"
KIND=$1; NET=$2; OUT=$3; shift 3
m5_res_up
[ -f "$M5_RUNS/$NET/army-net.bin" ] || { echo "eval: $M5_RUNS/$NET/army-net.bin missing"; exit 1; }
FAMS=""
if [ -n "${M5_FAMILIES:-}" ]; then FAMS="--families $M5_FAMILIES"; elif [ "$KIND" = quick ]; then FAMS="--families siege,ram,assault-heal,city-fire,hill-archers,planned-assault,promotion-choice,rush,raid,prey,focus,snipe"; fi
TO=${M5_HOLD_TO:-100}
cd "$AURELIA_LAB"
rm -rf "runs/$OUT"
"$M5_RES" run --cpu "${M5_CPU:-2-$(sysctl -n hw.perflevel0.physicalcpu)}" --name "eval-$KIND-$OUT" -- sh -c \
  "env ARMY_PLAY_ON_CAP=1 ARMY_NET=\"$M5_RUNS/$NET/army-net.bin\" $* \"$M5_EXAM\" suite --kept data/kept-m.jsonl --from 80 --instances $TO $FAMS --players army-net --out \"runs/$OUT\" --jobs \${M5_JOBS:-\$RES_THREADS}" | tail -1
