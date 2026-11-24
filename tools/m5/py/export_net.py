"""export_net.py WEIGHTS OUT.bin: an army net's safetensors (any class load_net knows) in the Rust format, on the CPU."""
import sys

import mlx.core as mx

mx.set_default_device(mx.cpu)
from aurelia_nn.army.export import export  # noqa: E402
from aurelia_nn.tacsim.armyloop import load_net  # noqa: E402

export(load_net(sys.argv[1]), sys.argv[2])
print("exported", sys.argv[2])
