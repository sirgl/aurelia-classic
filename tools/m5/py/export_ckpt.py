"""export_ckpt.py WEIGHTS OUT.bin: the weights of an rl3 net (N175, N175T or a checkpoint of a run) as the Rust net file, on the CPU."""
import sys

import mlx.core as mx

mx.set_default_device(mx.cpu)
from aurelia_nn.army.export import export  # noqa: E402
from aurelia_nn.rl3.net import load_any  # noqa: E402

export(load_any(sys.argv[1]), sys.argv[2])
print("exported", sys.argv[1], "->", sys.argv[2])
