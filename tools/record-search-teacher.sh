#!/bin/bash
# Records the search teacher's ground games for imitation (classic/docs/ai-search-teacher.md).
#
#   tools/record-search-teacher.sh BIN OUT KIND "RUNGS" [N] [JOBS] [SEED_OFFSET] [MIND] [EXTRA...]
#
# BIN          an ai-selfplay binary with `--learner-mind search` (copied under a unique name)
# OUT          the logs' root: one directory a rung, OUT/rNN (as nn-data/logs-ground-*)
# KIND         siege or skirmish
# RUNGS        the ladder's rungs, e.g. "$(seq 0 40)"
# N            scenes a rung (default 96)
# JOBS         games at once (default 4; more at night)
# SEED_OFFSET  the suite's seed offset of the first scene (default 200000: apart from the
#              evals' seeds 0-31 and the scripted teachers' recordings at 100000)
# MIND         the learner mind (default `search`; e.g. `search:base=siege-join`)
# EXTRA        more ai-selfplay flags, e.g. --battle-spawn-order random (siege)
#
# A rung already recorded is skipped game by game (ai-selfplay keeps the games done).
# The shards (labels on the learner's seat, the search's deferrals as the defer candidate's labels):
#   ai-features OUT/rNN --out SHARDS/rNN --minds learner --jobs 4
set -u
BIN=$1; OUT=$2; KIND=$3; RUNGS=$4; N=${5:-96}; JOBS=${6:-4}; SEED=${7:-200000}; MIND=${8:-search}
shift $(( $# < 8 ? $# : 8 ))
for r in $RUNGS; do
  dir=$(printf '%s/r%02d' "$OUT" "$r")
  mkdir -p "$dir"
  "$BIN" --learner-mind "$MIND" --suite standard --speed quick --battle "$KIND" --battle-minimal --battle-vary \
    --battle-rung "$r" --games "$N" --group 1 --seed-offset "$SEED" --jobs "$JOBS" --out "$dir" "$@" > "$dir.log" 2>&1 \
    || echo "rung $r: some games failed (a rung whose scene the draw cannot place, as in the evals): see $dir.log"
  echo "rung $r: $(ls "$dir"/*.e2.json 2>/dev/null | wc -l | tr -d ' ') games"
done
