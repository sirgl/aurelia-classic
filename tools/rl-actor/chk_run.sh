#!/bin/sh
cd /Users/roman.ivanov/civ/work/nets/rust-actor
D=data
F=siege,ram,bombard,promotion-choice,planned-assault,raid,prey,assault-heal,hill-archers,city-fire
rm -rf data/chk; mkdir -p data/chk
env ARMY_NET=/Users/roman.ivanov/civ/work/nets/mlx-army/data/g7/N175/army-net.bin ARMY_SAMPLE_PLAN=$D/plan-chk.json ARMY_OBS_DUMP=data/chk/dump.jsonl \
  bin/ai-exam-actor suite --kept $D/kept-chk.jsonl --families $F --instances 70 --players army-net --out data/chk --jobs 1 --record data/chk/rec.bin | tail -1
