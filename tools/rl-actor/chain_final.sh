#!/bin/sh
cd /Users/roman.ivanov/civ/work/nets/rust-actor
RES=/Users/roman.ivanov/civ/work/res/res
PY=/Users/roman.ivanov/civ/work/nets/mlx-sim/.venv/bin/python; export PYTHONPATH=classic/nn
W=/Users/roman.ivanov/civ/work/nets/mlx-army/data/g7/N175/weights.safetensors
for R in 1 2 3; do $RES run --cpu 7 --name ra-roll7c -- python3 -I py/mlock.py sh py/rust_rollout.sh bin/ai-exam-actor 1 7 data/clean7-$R > logs/clean7-$R.log 2>&1; done
$RES run --gpu --cpu 1 --ram 14 --name ra-twinF -- python3 -I py/mlock.py $PY py/twin_bench.py --sel data/sel-u1.json --updates 3 --out data/twin-u1-clean.json > logs/twin-u1-clean.log 2>&1
$RES run --gpu --cpu 1 --ram 14 --name ra-learnF -- python3 -I py/mlock.py $PY py/rl_learner.py data/clean7-1/rec.bin --weights $W --repeat 4 --check > logs/learner-clean.log 2>&1
