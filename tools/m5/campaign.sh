#!/usr/bin/env bash
# The overnight campaign, unattended and resumable.
#   tools/m5/campaign.sh start      run in the background (caffeinate keeps the Mac awake; log lab/logs/campaign.log)
#   tools/m5/campaign.sh run        run in the foreground
#   tools/m5/campaign.sh status     status.json + the last results     tools/m5/campaign.sh stop
#   tools/m5/campaign.sh smoke [MIN]  dry run: smoke teacher data + MIN minutes (default 2) of the first training
#   tools/m5/campaign.sh autostart on|off   a LaunchAgent that resumes the campaign after a reboot/login
# Start it again after a crash or a reboot: finished jobs are skipped, a training resumes from its checkpoint.
set -eu
. "$(dirname "$0")/env.sh"
CAMP=$M5_RUNS/campaign
CFG=${M5_CAMPAIGN_CONFIG:-$M5_REPO/tools/m5/campaign.toml}
PIDF=$M5_LOGS/campaign.pid
case "${1:-run}" in
  run)
    m5_res_up
    exec "$M5_PY" "$M5_REPO/tools/m5/campaign.py" run --config "$CFG" ;;
  smoke)
    m5_res_up
    exec "$M5_PY" "$M5_REPO/tools/m5/campaign.py" run --config "$CFG" --smoke "${2:-2}" ;;
  start)
    if [ -f "$PIDF" ] && kill -0 "$(cat "$PIDF")" 2>/dev/null; then echo "campaign already running (pid $(cat "$PIDF"))"; exit 0; fi
    m5_res_up
    nohup caffeinate -i "$M5_PY" "$M5_REPO/tools/m5/campaign.py" run --config "$CFG" >> "$M5_LOGS/campaign.log" 2>&1 &
    echo $! > "$PIDF"; echo "campaign started (pid $!), log $M5_LOGS/campaign.log" ;;
  stop)
    [ -f "$PIDF" ] && kill "$(cat "$PIDF")" 2>/dev/null && echo "campaign stopped (a running job continues until its chunk ends; its checkpoint stays)" || echo "not running" ;;
  status)
    "$M5_PY" "$M5_REPO/tools/m5/campaign.py" status --config "$CFG"; tail -5 "$CAMP/results.jsonl" 2>/dev/null || true ;;
  autostart)
    PLIST=$HOME/Library/LaunchAgents/lab.aurelia.m5-campaign.plist
    if [ "${2:-}" = on ]; then
      mkdir -p "$(dirname "$PLIST")"
      cat > "$PLIST" <<PL
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>lab.aurelia.m5-campaign</string>
<key>ProgramArguments</key><array><string>$M5_REPO/tools/m5/campaign.sh</string><string>start</string></array>
<key>RunAtLoad</key><true/></dict></plist>
PL
      launchctl load "$PLIST" && echo "autostart on ($PLIST)"
    else
      launchctl unload "$PLIST" 2>/dev/null; rm -f "$PLIST"; echo "autostart off"
    fi ;;
  *) sed -n 2,10p "$0"; exit 1 ;;
esac
