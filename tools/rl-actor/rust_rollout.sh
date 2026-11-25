#!/bin/sh
# rust_rollout.sh BIN UPDATE THREADS OUTDIR [greedy]: the Rust actor's rollout of one rl2-sized update (the 64 scenes of
# data/sel-uN.json x 8 copies, sampling in the plan's rounds), recording the plays for the learner. Run through the broker.
B=$1; U=$2; J=$3; OUT=$4; REC=${5:-rec}
[ "$J" = auto ] && J=${RES_THREADS:-1}
D=/Users/roman.ivanov/civ/work/nets/rust-actor/data
F=siege,ram,bombard,promotion-choice,planned-assault,raid,prey,assault-heal,hill-archers,city-fire
echo '--- res status at start'; /Users/roman.ivanov/civ/work/res/res status | head -14; echo COMMIT $(git -C /Users/roman.ivanov/civ/work/nets/rust-actor/classic rev-parse --short HEAD); rm -rf $OUT; mkdir -p $OUT
S=$(python3 -c "import time;print(time.time())")
env ARMY_NET=/Users/roman.ivanov/civ/work/nets/mlx-army/data/g7/N175/army-net.bin ARMY_SAMPLE_PLAN=$D/plan-u$U.json \
  /usr/bin/time -l -o $OUT/time.txt $B suite --kept $D/kept-u$U.jsonl --families $F --instances 70 --players army-net --out $OUT --jobs $J $([ "$REC" = rec ] && echo --record $OUT/rec.bin) | tail -1
E=$(python3 -c "import time;print(time.time())")
grep 'maximum resident' $OUT/time.txt | awk '{print "MAXRSS_MB", $1/1048576}'
python3 -c "print('WALL', round($E-$S,2), 'THREADS', $J)"
[ "$REC" = rec ] && ls -l $OUT/rec.bin | awk '{print "RECORD_BYTES", $5}'; true
