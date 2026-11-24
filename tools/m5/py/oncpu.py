"""oncpu.py (-m MODULE | SCRIPT) ARGS...: run an MLX entry point with MLX's default device set to the CPU.

The smoke mode of campaign3 uses it on a machine whose GPU is busy with other lanes: the same code path, no GPU lock."""
import runpy
import sys

import mlx.core as mx

mx.set_default_device(mx.cpu)
if sys.argv[1] == "-m":
    mod = sys.argv[2]
    sys.argv = [mod] + sys.argv[3:]
    runpy.run_module(mod, run_name="__main__", alter_sys=True)
else:
    path = sys.argv[1]
    sys.argv = sys.argv[1:]
    sys.path.insert(0, str(__import__("pathlib").Path(path).resolve().parent))
    runpy.run_path(path, run_name="__main__")
