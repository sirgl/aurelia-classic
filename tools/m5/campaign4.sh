#!/usr/bin/env bash
# Night 4 of the M5 kit, unattended and resumable (docs/m5/PROMPT-4.md). The queue: tools/m5/campaign4.toml.
#   tools/m5/campaign4.sh plan       the queue with the estimated minutes and the end time of each job (nothing runs)
#   tools/m5/campaign4.sh start      background (caffeinate); log lab/logs/campaign4.log
#   tools/m5/campaign4.sh run        foreground
#   tools/m5/campaign4.sh stop       between jobs (the STOP file; a running job ends first)
#   tools/m5/campaign4.sh status     status.json + the last results       tools/m5/campaign4.sh summary
#   tools/m5/campaign4.sh --smoke    every job kind for 1-2 updates in its own lab (lab-smoke/), foreground, about 5 minutes
# Start again after a crash or a reboot: finished jobs are skipped, an RL run resumes from its newest checkpoint.
set -eu
. "$(dirname "$0")/env.sh"
CFG=${M5_CAMPAIGN4_CONFIG:-$M5_REPO/tools/m5/campaign4.toml}
C4="$M5_PY -I $M5_REPO/tools/m5/campaign4.py"
PIDF=$M5_REPO/lab/logs/campaign4.pid
case "${1:-run}" in
  plan|summary) exec $C4 "$1" --config "$CFG" ;;
  run) m5_res_up; rm -f "$AURELIA_LAB/runs/campaign4/STOP"; exec $C4 run --config "$CFG" ;;
  --smoke|smoke)   # its own lab: the real run never sees the smoke's tiny recordings and nets
    export AURELIA_LAB=${M5_SMOKE_LAB:-$M5_REPO/lab-smoke}; mkdir -p "$AURELIA_LAB/runs" "$AURELIA_LAB/logs" "$AURELIA_LAB/data"
    m5_res_up; rm -f "$AURELIA_LAB/runs/campaign4-smoke/STOP"; exec $C4 run --smoke --config "$CFG" ${M5_ONLY:+--only "$M5_ONLY"} ;;
  start)
    mkdir -p "$(dirname "$PIDF")"
    if [ -f "$PIDF" ] && kill -0 "$(cat "$PIDF")" 2>/dev/null; then echo "night 4 already running (pid $(cat "$PIDF"))"; exit 0; fi
    m5_res_up; rm -f "$AURELIA_LAB/runs/campaign4/STOP"
    nohup caffeinate -i $C4 run --config "$CFG" >> "$AURELIA_LAB/logs/campaign4.log" 2>&1 &
    echo $! > "$PIDF"; echo "night 4 started (pid $!), log $AURELIA_LAB/logs/campaign4.log" ;;
  stop) mkdir -p "$AURELIA_LAB/runs/campaign4"; touch "$AURELIA_LAB/runs/campaign4/STOP"; echo "STOP written: the queue stops after the running job" ;;
  status) $C4 status --config "$CFG"; tail -4 "$AURELIA_LAB/runs/campaign4/results.jsonl" 2>/dev/null | cut -c1-300 || true ;;
  *) sed -n 2,9p "$0"; exit 1 ;;
esac
