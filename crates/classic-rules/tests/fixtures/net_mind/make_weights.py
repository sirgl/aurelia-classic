"""The pinned weights of `tests/net_e2e_mind.rs`: the small preset with
random weights (torch seed 1) for the feature layout of this build.

Regenerate only on purpose (a change of the feature layout), in the same
change that notes it in classic/docs/ai-neural-architecture.md (Playing):

    cd classic
    cargo +1.98.1 run --release -p aurelia-qa --bin ai-play -- --suite smoke --games 1 --rounds 6 --out /tmp/net-log
    cargo +1.98.1 run --release -p aurelia-qa --bin ai-features -- /tmp/net-log --out /tmp/net-shard
    PYTHONPATH=nn ../.venv-nn/bin/python crates/classic-rules/tests/fixtures/net_mind/make_weights.py \
        /tmp/net-shard/1-smoke-pangaea-tiny.shard/meta.json crates/classic-rules/tests/fixtures/net_mind/random-small.bin \
        crates/classic-rules/tests/fixtures/net_mind/random-small.base-layout.json

With the third argument (a base layout, `Layout.to_json`) the random net is
made on the base layout, with its heads, and carried into this build's
layout the way `train_imitation --init` carries a checkpoint into a new data
format: every column the base lacks gets zero input weights. The net then
plays exactly as the base net did, and the world hash of
`a_game_of_nets_with_pinned_weights_is_pinned` stays. The base is format 5
with the five heads of the first pin (unit … religion); without it the net
is drawn on this build's layout and the pinned game moves (re-pin it).

Format 7: when the layout has the event table the net has the events on
(weights version 4); carried from the base its event layers are new
(random) and `xattn.out` zero, so it reads its events and still plays the
base net's game.
"""
import json
import sys
import tempfile

import torch

from aurelia_nn.data import Layout
from aurelia_nn.export import write
from aurelia_nn.model import Net, save_checkpoint
from aurelia_nn.quantize import quantize
from aurelia_nn.train_imitation import init_from

meta, out = sys.argv[1], sys.argv[2]
layout = Layout.from_meta(json.load(open(meta)))
events = int(layout.count("event") > 0)
torch.manual_seed(1)
if len(sys.argv) > 3:
    base = Layout.from_json(json.load(open(sys.argv[3])))
    layout.heads = base.heads
    with tempfile.NamedTemporaryFile(suffix=".pt") as ck:
        save_checkpoint(ck.name, Net(base.config("small"), base).eval())
        model = Net(layout.config("small", events=events), layout).eval()
        report = init_from(model, ck.name)
    new = {n.split(".")[0] for n in report["new"]}
    assert new <= {"id_emb", "event", "xattn"} and not report["shape_changed"], report
    assert not events or not (model.xattn.out.weight.any() or model.xattn.out.bias.any())
else:
    model = Net(layout.config("small", events=events), layout).eval()
print("heads", layout.heads, "layout hash", hex(layout.hash()))
print(out, write(quantize(model, layout), out), "bytes")
