"""Mixed precision for the MB / M training (stage 2 of the M5 kit): `--bf16` of train_m.

Matmuls run in bfloat16 and their results are cast back to float32 at once. Everything else stays float32: the
weights, the optimizer moments, the memory state (u_s, the plan tokens and the GRUs that update them: `MemNet.step`
runs under `exact()`), the layer norms, the softmax of the attention, the cross entropies and the losses. The map
encoder's convolutions follow `model.ENCODER` (train_m sets it to bfloat16 with this switch, as train.py does for B).
Export and the Rust net are float32 whatever the training used.

`enable(False)` (the default) leaves every function as it was: `mm(a, b)` is `a @ b` and `nn.Linear` is MLX's.
Call `enable` before the first compiled call: `mx.compile` traces the state of the switch once.
"""
import contextlib

import mlx.core as mx
import mlx.nn as nn

STATE = {'on': False}
_LINEAR = nn.Linear.__call__


def mm(a, b):
    """a @ b, in bfloat16 with a float32 result when mixed precision is on."""
    if not STATE['on']:
        return a @ b
    return (a.astype(mx.bfloat16) @ b.astype(mx.bfloat16)).astype(mx.float32)


def _linear(self, x):
    if not STATE['on']:
        return _LINEAR(self, x)
    y = (x.astype(mx.bfloat16) @ self.weight.T.astype(mx.bfloat16)).astype(mx.float32)
    return y + self.bias if 'bias' in self else y


def enable(on=True):
    STATE['on'] = bool(on)
    nn.Linear.__call__ = _linear if on else _LINEAR


@contextlib.contextmanager
def exact():
    """float32 matmuls inside the block (the memory update)."""
    old = STATE['on']
    STATE['on'] = False
    try:
        yield
    finally:
        STATE['on'] = old
