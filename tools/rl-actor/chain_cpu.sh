#!/bin/sh
cd /Users/roman.ivanov/civ/work/nets/rust-actor
RES=/Users/roman.ivanov/civ/work/res/res
# elastic first (whatever the broker gives, at most 7), then the fixed sizes
for R in 1 2 3; do
  $RES run --cpu 1-7 --name ra-rollE -- python3 -I py/mlock.py sh py/rust_rollout.sh bin/ai-exam-actor 1 auto data/scaleE-$R > logs/scaleE-$R.log 2>&1
done
for N in 1 2 4; do
  for R in 1 2 3; do
    $RES run --cpu $N --name ra-roll$N -- python3 -I py/mlock.py sh py/rust_rollout.sh bin/ai-exam-actor 1 $N data/scale-$N-$R > logs/scale-$N-$R.log 2>&1
  done
done
for v in standard pre-net2; do
  $RES run --cpu 1-4 --name ra-hash -- python3 -I py/mlock.py sh -c "bin/ai-tournament-actor --solo $v --suite standard --speed quick --jobs \$RES_THREADS" > logs/hash-$v.log 2>&1
done
for U in 1 2 3 4 5 6 7 8; do
  $RES run --cpu 1-4 --name ra-san -- python3 -I py/mlock.py sh py/rust_rollout.sh bin/ai-exam-actor $U auto data/san-$U norec > logs/san-$U.log 2>&1
done
