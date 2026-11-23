#!/usr/bin/env bash
# Stage 2 of the M5 kit, unattended and resumable (docs/m5/PROMPT-2.md).
#   tools/m5/campaign2.sh plan      the job list with the estimated minutes of each (no job runs; stage 1's results are read)
#   tools/m5/campaign2.sh start     background (caffeinate); log lab/logs/campaign2.log
#   tools/m5/campaign2.sh run       foreground          tools/m5/campaign2.sh stop     between jobs
#   tools/m5/campaign2.sh status    status.json + the last results     tools/m5/campaign2.sh summary
# Start again after a crash or a reboot: finished jobs are skipped, a training resumes from its checkpoint.
set -eu
. "$(dirname "$0")/env.sh"
CAMP=$M5_RUNS/campaign2
CFG=${M5_CAMPAIGN2_CONFIG:-$M5_REPO/tools/m5/campaign2.toml}
PIDF=$M5_LOGS/campaign2.pid
case "${1:-run}" in
  plan|summary) exec "$M5_PY" "$M5_REPO/tools/m5/campaign2.py" "$1" --config "$CFG" ;;
  run) m5_res_up; exec "$M5_PY" "$M5_REPO/tools/m5/campaign2.py" run --config "$CFG" ;;
  start)
    if [ -f "$PIDF" ] && kill -0 "$(cat "$PIDF")" 2>/dev/null; then echo "stage 2 already running (pid $(cat "$PIDF"))"; exit 0; fi
    m5_res_up
    nohup caffeinate -i "$M5_PY" "$M5_REPO/tools/m5/campaign2.py" run --config "$CFG" >> "$M5_LOGS/campaign2.log" 2>&1 &
    echo $! > "$PIDF"; echo "stage 2 started (pid $!), log $M5_LOGS/campaign2.log" ;;
  stop) [ -f "$PIDF" ] && kill "$(cat "$PIDF")" 2>/dev/null && echo "stopped (a running job continues until its chunk ends)" || echo "not running" ;;
  status) "$M5_PY" "$M5_REPO/tools/m5/campaign2.py" status --config "$CFG"; tail -5 "$CAMP/results.jsonl" 2>/dev/null || true ;;
  *) sed -n 2,9p "$0"; exit 1 ;;
esac
