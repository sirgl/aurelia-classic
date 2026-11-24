#!/usr/bin/env bash
# Stage 3 of the M5 kit, unattended and resumable (docs/m5/PROMPT-3.md). The queue: tools/m5/campaign3.toml.
#   tools/m5/campaign3.sh plan      the queue with the estimated minutes and the end time of each job (nothing runs)
#   tools/m5/campaign3.sh start     background (caffeinate); log lab/logs/campaign3.log
#   tools/m5/campaign3.sh run       foreground
#   tools/m5/campaign3.sh stop      between jobs (the STOP file; a running job ends first)
#   tools/m5/campaign3.sh status    status.json + the last results       tools/m5/campaign3.sh summary
#   tools/m5/campaign3.sh smoke     every job kind once with tiny budgets (lab/runs/campaign3-smoke/), foreground
# Start again after a crash or a reboot: finished jobs are skipped, a training resumes from its checkpoint.
set -eu
. "$(dirname "$0")/env.sh"
CFG=${M5_CAMPAIGN3_CONFIG:-$M5_REPO/tools/m5/campaign3.toml}
C3="$M5_PY -I $M5_REPO/tools/m5/campaign3.py"
PIDF=$M5_LOGS/campaign3.pid
case "${1:-run}" in
  plan|summary) exec $C3 "$1" --config "$CFG" ;;
  run) m5_res_up; rm -f "$M5_RUNS/campaign3/STOP"; exec $C3 run --config "$CFG" ;;
  smoke) m5_res_up; rm -f "$M5_RUNS/campaign3-smoke/STOP"; exec $C3 run --smoke --config "$CFG" ;;
  start)
    if [ -f "$PIDF" ] && kill -0 "$(cat "$PIDF")" 2>/dev/null; then echo "stage 3 already running (pid $(cat "$PIDF"))"; exit 0; fi
    m5_res_up; rm -f "$M5_RUNS/campaign3/STOP"
    nohup caffeinate -i $C3 run --config "$CFG" >> "$M5_LOGS/campaign3.log" 2>&1 &
    echo $! > "$PIDF"; echo "stage 3 started (pid $!), log $M5_LOGS/campaign3.log" ;;
  stop) mkdir -p "$M5_RUNS/campaign3"; touch "$M5_RUNS/campaign3/STOP"; echo "STOP written: the queue stops after the running job" ;;
  status) $C3 status --config "$CFG"; tail -3 "$M5_RUNS/campaign3/results.jsonl" 2>/dev/null | cut -c1-300 || true ;;
  *) sed -n 2,9p "$0"; exit 1 ;;
esac
