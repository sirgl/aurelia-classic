#!/bin/sh
# The GPU chain: twin timing, learner timing (on the Rust records), twin sanity. Each job holds the comparison's own lock, then the broker's slots.
cd /Users/roman.ivanov/civ/work/nets/rust-actor
PY=/Users/roman.ivanov/civ/work/nets/mlx-sim/.venv/bin/python
export PYTHONPATH=classic/nn
RES=/Users/roman.ivanov/civ/work/res/res
W=/Users/roman.ivanov/civ/work/nets/mlx-army/data/g7/N175/weights.safetensors
$RES run --gpu --cpu 1-2 --ram 14 --name ra-twin1 -- python3 -I py/mlock.py $PY py/twin_bench.py --sel data/sel-u1.json --updates 3 --kl --out data/twin-u1.json > logs/twin-u1.log 2>&1
$RES run --gpu --cpu 1-2 --ram 14 --name ra-learn -- python3 -I py/mlock.py $PY py/rl_learner.py data/roll-test/rec.bin --weights $W --repeat 4 --check > logs/learner-u1.log 2>&1
$RES run --gpu --cpu 1-2 --ram 14 --name ra-twin2 -- python3 -I py/mlock.py $PY py/twin_bench.py --sel data/sel-u1.json data/sel-u2.json data/sel-u3.json data/sel-u4.json data/sel-u5.json data/sel-u6.json data/sel-u7.json data/sel-u8.json --updates 8 --rollout-only --out data/twin-san.json > logs/twin-san.log 2>&1
