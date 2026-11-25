#!/bin/sh
# watchdog.sh OUT -- COMMAND...: runs the loop and restarts it with --resume while OUT/done is missing: when it exits without finishing, or
# when OUT/heartbeat has not been touched for STALE minutes (default 30; a hung actor or learner). At most MAXR restarts (default 5).
OUT=$1; shift; shift
STALE=${STALE:-30}; MAXR=${MAXR:-5}; POLL=${POLL:-60}; n=0
mkdir -p "$OUT"
while [ ! -f "$OUT/done" ] && [ $n -le $MAXR ]; do
  pkill -f "$OUT/(work|val)/" 2>/dev/null   # actors left behind by a killed loop (their command lines name this run's folders only)
  touch "$OUT/heartbeat"   # a hang before the first update of an attempt is caught too
  "$@" --resume &
  pid=$!
  while kill -0 $pid 2>/dev/null; do
    sleep $POLL
    if [ -f "$OUT/heartbeat" ] && [ -n "$(find "$OUT/heartbeat" -mmin +$STALE 2>/dev/null)" ]; then
      echo "watchdog: no heartbeat for $STALE minutes, killing $pid"; kill $pid; break
    fi
  done
  wait $pid 2>/dev/null
  n=$((n + 1))
  [ -f "$OUT/done" ] || echo "watchdog: exit of attempt $n, restarting from $(cat "$OUT/latest.json" 2>/dev/null)"
done
