"""The army net's weights in the Rust format (`crates/qa/src/army_net/model.rs`):
"ARMW" u32 | json_len u32 | json {config, tensors: [{name, shape, offset}]} | f32 data.
A Linear's weight is written [in, out] (the transpose of MLX's [out, in]); everything else as it is."""
import json
import struct
from array import array

import mlx.core as mx
import mlx.nn as nn


def export(model, path):
    flat = dict(nn.utils.tree_flatten(model.parameters()))
    names = sorted(flat)
    tensors = []
    data = array('f')
    for name in names:
        v = flat[name]
        if name.endswith('.weight') and v.ndim == 2 and not name.split('.')[-2].startswith('norm'):
            v = v.T
        v = mx.array(v, mx.float32)
        tensors.append({'name': name, 'shape': list(v.shape), 'offset': len(data)})
        data.extend(v.reshape(-1).tolist())
    config = {k: v for k, v in model.config.items() if k != 'arch'}  # MBX: the flags are not in the Rust format
    head = json.dumps({'config': config, 'tensors': tensors}).encode()
    with open(path, 'wb') as f:
        f.write(struct.pack('<II', struct.unpack('<I', b'ARMW')[0], len(head)))
        f.write(head)
        f.write(data.tobytes())
