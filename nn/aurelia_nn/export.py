"""The weights file of the end-to-end net: written here, read by
crates/classic-rules/src/ai/net_e2e/model.rs (format in
classic/docs/ai-net-model.md). Little-endian throughout:

    magic "AUNE", u32 version (2: trace off; 3: the turn trace on; 4: the
        events on, the trace either way; 5: the defer choice on, the rest
        either way)
    u32 n, then n × u32: the config (Config.FILE_ORDER: its first 29
        fields in version 2, 32 in version 3, 34 in version 4, all 35 in
        version 5), then
        act_bits, input_shift, exp_lut_len, ln_eps_q, gate_half, gate_bits
    u64 layout hash (FNV-1a 64 of Layout.canonical())
    i32 input multipliers: tile, global, entity, candidate (version 4: then
        event)
    i32 exp_lut[exp_lut_len]
    i32 sigmoid[2 × gate_half + 1], i32 tanh[2 × gate_half + 1]
    i16 edges[n_conv × d_tile]
    i16 tag_emb[n_decision_heads × tag_buckets × tag_dim]
    i16 param_emb[n_decision_heads × param_buckets × param_dim]
    version 5: i32 defer_bias (Q8, added to the defer candidate's logit)
    per table (tile, global, entity, candidate; version 4: then event):
        u32 k, u32 id columns[k], i16 id_emb[k × id_buckets × id_dim]
    engram on: u32 k, u32 small entity columns[k]; then per site (tile,
        entity, candidate) i16 engram[engram_buckets × engram_dim]
    u32 layer count, then per dense layer (quantize.layer_names order):
        u32 out, u32 in, i8 w[out × in] (row per output),
        i32 mult[out], i32 shift[out], i32 bias[out]
        (`trace` after the GRUs with the trace on, d_model × d_model, bias 0;
        version 4: `event`, `xattn.q`, `xattn.kv`, `xattn.out` last)
    "END\\0"

Usage: python -m aurelia_nn.export <checkpoint.pt> <out.bin>
"""

from __future__ import annotations

import argparse
import struct

import numpy as np

from .common import ACT_BITS, EVENT_TABLE, GATE_BITS, GATE_HALF, INPUT_SHIFT, Config
from .quantize import QLayer, QuantModel, layer_names, ln_eps_q

MAGIC = b"AUNE"
VERSION = 5  # the newest written; without defer version 4 (events), 3 (the trace) or 2, byte for byte the files before
VERSIONS = (2, 3, 4, 5)
# The largest |defer bias| (Q8; model.rs MAX_DEFER_BIAS).
MAX_DEFER_BIAS = 1 << 24
END = b"END\0"


def _arr(a, dtype, lo, hi) -> bytes:
    a = np.asarray(a, dtype=np.int64).ravel()
    if a.size and (a.min() < lo or a.max() > hi):
        raise ValueError(f"a value leaves {dtype}: [{a.min()}, {a.max()}]")
    return a.astype(np.dtype(dtype).newbyteorder("<")).tobytes()


def to_bytes(qm: QuantModel) -> bytes:
    cfg = qm.cfg
    out = bytearray(MAGIC)
    version = file_version(cfg)
    out += struct.pack("<I", version)
    fields = Config.FILE_ORDER[: file_fields(version)]
    conf = [getattr(cfg, k) for k in fields] + [ACT_BITS, INPUT_SHIFT, len(qm.exp_lut), ln_eps_q(cfg.d_model), GATE_HALF, GATE_BITS]
    out += struct.pack("<I", len(conf)) + struct.pack(f"<{len(conf)}I", *conf)
    out += struct.pack("<Q", qm.layout_hash)
    tables = file_tables(version)
    for table in tables:
        # A layout without the event table (version 5 of such a net): no multipliers.
        out += _arr(qm.inputs.get(table, np.zeros(0, np.int64)), "i4", -(2**31), 2**31 - 1)
    out += _arr(qm.exp_lut, "i4", 0, 2**31 - 1)
    out += _arr(qm.sig_lut, "i4", 0, 1 << GATE_BITS)
    out += _arr(qm.tanh_lut, "i4", -(1 << GATE_BITS), 1 << GATE_BITS)
    out += _arr(qm.edges, "i2", -32768, 32767)
    out += _arr(qm.tag_emb, "i2", -32768, 32767)
    out += _arr(qm.param_emb, "i2", -32768, 32767)
    if version >= 5:
        out += _arr([qm.defer_bias], "i4", -MAX_DEFER_BIAS, MAX_DEFER_BIAS)
    for table in tables:
        ids = cfg.id_columns(table)
        out += struct.pack("<I", len(ids)) + struct.pack(f"<{len(ids)}I", *ids)
        if ids:
            out += _arr(qm.id_emb[table], "i2", -32768, 32767)
    if cfg.engram:
        small = cfg.engram_entity_small
        out += struct.pack("<I", len(small)) + struct.pack(f"<{len(small)}I", *small)
        for site in ("tile", "entity", "candidate"):
            out += _arr(qm.engram[site], "i2", -32768, 32767)
    names = layer_names(cfg)
    out += struct.pack("<I", len(names))
    for name in names:
        L = qm.layers[name]
        out += struct.pack("<II", L.out_features, L.in_features)
        out += _arr(L.w, "i1", -127, 127)
        out += _arr(L.mult, "i4", 0, 2**31 - 1)
        out += _arr(L.shift, "i4", 1, 62)
        out += _arr(L.bias, "i4", -(2**31), 2**31 - 1)
    out += END
    return bytes(out)


def file_version(cfg: Config) -> int:
    """5 with the defer choice, else 4 with the events, else 3 with the turn
    trace, else 2 (byte for byte the files before them)."""
    return 5 if cfg.defer else 4 if cfg.events else 3 if cfg.trace else 2


def file_fields(version: int) -> int:
    """The config fields of a file of `version`."""
    return {2: Config.FILE_FIELDS_V2, 3: Config.FILE_FIELDS_V3, 4: Config.FILE_FIELDS_V4}.get(version, len(Config.FILE_ORDER))


def file_tables(version: int) -> tuple:
    """The tables whose multipliers and id columns a file of `version` holds."""
    return ("tile", "global", "entity", "candidate") + ((EVENT_TABLE,) if version >= 4 else ())


def write(qm: QuantModel, path) -> int:
    data = to_bytes(qm)
    with open(path, "wb") as f:
        f.write(data)
    return len(data)


def from_bytes(data: bytes) -> QuantModel:
    pos = 0

    def take(fmt):
        nonlocal pos
        vals = struct.unpack_from(fmt, data, pos)
        pos += struct.calcsize(fmt)
        return vals

    def arr(dtype, n):
        nonlocal pos
        dt = np.dtype(dtype).newbyteorder("<")
        a = np.frombuffer(data, dtype=dt, count=n, offset=pos).astype(np.int64)
        pos += n * dt.itemsize
        return a

    if data[:4] != MAGIC:
        raise ValueError("not an AUNE weights file")
    pos = 4
    (version,) = take("<I")
    if version not in VERSIONS:
        raise ValueError(f"weights version {version}")
    (n,) = take("<I")
    conf = take(f"<{n}I")
    k = file_fields(version)
    cfg = Config(**dict(zip(Config.FILE_ORDER, conf[:k])))
    act_bits, input_shift, lut_len, _eps, gate_half, gate_bits = conf[k : k + 6]
    if act_bits != ACT_BITS or input_shift != INPUT_SHIFT or gate_half != GATE_HALF or gate_bits != GATE_BITS:
        raise ValueError("fixed-point constants differ")
    (layout_hash,) = take("<Q")
    inputs = {t: arr("i4", getattr(cfg, f"c_{t}")) for t in file_tables(version)}
    lut = arr("i4", lut_len)
    sig = arr("i4", 2 * gate_half + 1)
    tanh = arr("i4", 2 * gate_half + 1)
    edges = arr("i2", cfg.n_conv * cfg.d_tile).reshape(cfg.n_conv, cfg.d_tile)
    tag = arr("i2", cfg.n_decision_heads * cfg.tag_buckets * cfg.tag_dim).reshape(-1, cfg.tag_dim)
    param = arr("i2", cfg.n_decision_heads * cfg.param_buckets * cfg.param_dim).reshape(-1, cfg.param_dim)
    defer_bias = int(arr("i4", 1)[0]) if version >= 5 else 0
    id_cols, id_emb = {}, {}
    for table in file_tables(version):
        (k_ids,) = take("<I")
        id_cols[table] = tuple(take(f"<{k_ids}I")) if k_ids else ()
        if k_ids:
            id_emb[table] = arr("i2", k_ids * cfg.id_buckets * cfg.id_dim).reshape(-1, cfg.id_dim)
    import dataclasses

    cfg = dataclasses.replace(cfg, **{f"id_{t}": id_cols[t] for t in id_cols})
    engram = {}
    if cfg.engram:
        (k_small,) = take("<I")
        cfg = dataclasses.replace(cfg, engram_entity_small=tuple(take(f"<{k_small}I")) if k_small else ())
        for site in ("tile", "entity", "candidate"):
            engram[site] = arr("i2", cfg.engram_buckets * cfg.engram_dim).reshape(-1, cfg.engram_dim)
    qm = QuantModel(cfg, layout_hash, inputs, lut, edges, tag, param, sig, tanh, id_emb, engram, defer_bias=defer_bias)
    (count,) = take("<I")
    names = layer_names(cfg)
    if count != len(names):
        raise ValueError(f"{count} layers, expected {len(names)}")
    for name in names:
        o, i = take("<II")
        w = arr("i1", o * i).reshape(o, i)
        qm.layers[name] = QLayer(w, arr("i4", o), arr("i4", o), arr("i4", o))
    if data[pos : pos + 4] != END or pos + 4 != len(data):
        raise ValueError("bad end of weights file")
    return qm


def read(path) -> QuantModel:
    with open(path, "rb") as f:
        return from_bytes(f.read())


def main():
    from .model import load_checkpoint
    from .data import Layout
    from .quantize import quantize

    ap = argparse.ArgumentParser(description="Quantise a checkpoint and write the weights file.")
    ap.add_argument("checkpoint")
    ap.add_argument("out")
    a = ap.parse_args()
    model = load_checkpoint(a.checkpoint)
    layout = Layout.from_json(model.layout_json)
    size = write(quantize(model, layout), a.out)
    print(f"{a.out}: {size} bytes")


if __name__ == "__main__":
    main()
