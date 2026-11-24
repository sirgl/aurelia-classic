#!/usr/bin/env bash
# Recipe (e): the speed benchmark, fixed commands, to compare the M5 with the M4 laptop.
#   tools/m5/bench.sh            ~15 min; needs the teacher data (teach.sh) and the release binaries (setup.sh)
# Prints a results block to paste into docs/m5-RUNBOOK.md ("Results template") and writes lab/runs/bench/results.json.
# Quiet machine: close other work first. One job at a time; the training is the only GPU job.
set -eu
. "$(dirname "$0")/env.sh"
m5_res_up
B=$M5_RUNS/bench; mkdir -p "$B"
CPU_LIST=$(sysctl -n hw.perflevel0.physicalcpu)
echo "== 1/4 training decisions/s: MB d64 l3, 5 minutes, seed 1 (the log's dps_mean)"
rm -rf "$M5_RUNS/bench-train" "$M5_LOGS/bench-train.log"
M5_MAX_MINUTES=5 M5_CHUNK_MIN=5 M5_D=64 M5_LAYERS=3 M5_SEED=1 M5_EPOCHS=2 "$M5_REPO/tools/m5/train.sh" bench-train || true
echo "== 2/4 Rust trainer plays/s: the 12 quick families, solvers, held-out 80-89, 1 thread, then $CPU_LIST threads"
for J in 1 "$CPU_LIST"; do
  rm -rf "$M5_RUNS/bench-plays-$J"
  S=$(date +%s)
  ( cd "$AURELIA_LAB" && "$M5_RES" run --cpu "$J" --name "bench-plays-$J" -- sh -c "\"$M5_EXAM\" suite --kept data/kept-m.jsonl --from 80 --instances 90 --families siege,ram,assault-heal,city-fire,hill-archers,planned-assault,promotion-choice,rush,raid,prey,focus,snipe --players solvers --out runs/bench-plays-$J --jobs \$RES_THREADS" >/dev/null )
  E=$(date +%s)
  N=$(wc -l < "$M5_RUNS/bench-plays-$J/rows.jsonl")
  echo "$J $N $((E-S))" >> "$B/plays.txt.new"
done
mv "$B/plays.txt.new" "$B/plays.txt"
echo "== 3/4 ms per AI turn of the net: the quick eval of bench-train on 1 thread (M5_JOBS=1)"
if [ -f "$M5_RUNS/bench-train/army-net.bin" ]; then
  M5_CPU=1 M5_JOBS=1 M5_HOLD_TO=90 "$M5_REPO/tools/m5/eval.sh" quick bench-train bench-evq || true
fi
echo "== 4/4 the classic AI's turn cost: one smoke-suite game on 1 thread"
S=$(date +%s)
( "$M5_RES" run --cpu 1 --name bench-tournament -- "$M5_REPO/target/release/ai-tournament" --suite smoke --jobs 1 --solo standard > "$B/tournament.txt" 2>&1 ) || true
E=$(date +%s); echo "tournament_smoke_s $((E-S))" >> "$B/plays.txt"
"$M5_PY" - <<PY
import json, os, subprocess
lab = os.environ['AURELIA_LAB']; B = lab + '/runs/bench'
out = {'chip': subprocess.run(['sysctl', '-n', 'machdep.cpu.brand_string'], capture_output=True, text=True).stdout.strip(),
       'cores': subprocess.run(['sysctl', '-n', 'hw.physicalcpu'], capture_output=True, text=True).stdout.strip(),
       'ram_gb': int(subprocess.run(['sysctl', '-n', 'hw.memsize'], capture_output=True, text=True).stdout) // 2**30}
cur = lab + '/runs/bench-train/curves.jsonl'
rows = [json.loads(l) for l in open(cur) if l.startswith('{')] if os.path.exists(cur) else []
out['train_decisions_per_s'] = round(rows[-1].get('dps_mean', 0)) if rows else None
for line in open(B + '/plays.txt'):
    p = line.split()
    if p[0] == 'tournament_smoke_s': out['tournament_smoke_s'] = int(p[1])
    else: out[f'trainer_plays_per_s_{p[0]}_threads'] = round(int(p[1]) / max(int(p[2]), 1), 2)
rp = lab + '/runs/bench-evq/rows.jsonl'
if os.path.exists(rp):
    us = turns = 0
    for l in open(rp):
        r = json.loads(l); i = r['info']
        us += sum(float(i.get(k, 0)) for k in ('net_encode_us', 'net_forward_us', 'net_memory_us', 'net_observe_us', 'net_update_us'))
        turns += float(r['measures'].get('net_turns', 0))
    out['net_ms_per_turn_1_thread'] = round(us / 1000 / turns, 2) if turns else None
json.dump(out, open(B + '/results.json', 'w'), indent=1)
print(json.dumps(out, indent=1))
PY
