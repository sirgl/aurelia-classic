"""Quantisation of the float model and the exact integer reference forward
pass in NumPy int64 (classic/docs/ai-net-model.md).

The Rust inference (crates/classic-rules/src/ai/net_e2e/model.rs) computes
bit for bit what `encode_turn` and `score` compute here:

- activations: Q8 (value × 256) in i16, saturated to [−32768, 32767];
- a dense layer: int8 weights with a scale per output, exact sums of
  weight × activation (Rust: i32 over chunks of 512 inputs, then i64; here
  int64), then
  `round_shift(sum × mult[j], shift[j]) + bias[j]`, then ReLU if any,
  then saturation;
- `round_shift(v, r) = (v + 2^(r−1)) >> r` (arithmetic shift: rounds half
  up); `div_round(a, b) = floor((2a + b) / 2b)` for b > 0;
- LayerNorm: integer mean and variance, `isqrt` (floor), no affine part;
- softmax: a lookup table of exp(−t/256) at 2^15, weights summed exactly,
  one rounded division per output.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field

import numpy as np

from .common import (
    ACT_BITS,
    ACT_ONE,
    EVENT_TABLE,
    EXP_ONE,
    HEAD_PRODUCTION,
    HEAD_UNIT,
    I16_MAX,
    I16_MIN,
    GATE_BITS,
    GATE_HALF,
    INPUT_SHIFT,
    LN_EPS,
    NATION_SLOT_KINDS,
    UNIT_SLOT_KINDS,
    WAIT_TAG,
    DEFER_TAG,
    Config,
    candidate_engram,
    entity_engram,
    id_index,
    tile_engram,
    param_index,
    tag_index,
)

MULT_BITS = 20  # mult[j] in [2^20, 2^21]


def exp_lut() -> np.ndarray:
    """EXP_LUT[t] = round(2^15 × exp(−t / 256)) for every t where it is
    non-zero; beyond the table exp is 0."""
    vals = []
    t = 0
    while True:
        v = int(round(EXP_ONE * math.exp(-t / ACT_ONE)))
        if v == 0:
            break
        vals.append(v)
        t += 1
    return np.asarray(vals, dtype=np.int64)


def gate_luts() -> tuple[np.ndarray, np.ndarray]:
    """GRU gate tables over Q8 pre-activations −8 … 8: sigmoid and tanh at
    2^15 (index = clamp(x, −2048, 2048) + 2048)."""
    x = (np.arange(2 * GATE_HALF + 1) - GATE_HALF) / ACT_ONE
    one = 1 << GATE_BITS
    sig = np.asarray([int(round(one / (1.0 + math.exp(-v)))) for v in x], np.int64)
    tanh = np.asarray([int(round(one * math.tanh(v))) for v in x], np.int64)
    return sig, tanh


def ln_eps_q(d: int) -> int:
    return int(round(LN_EPS * d * d * 2.0**32))


@dataclass
class QLayer:
    w: np.ndarray  # int64 holding int8 values [out, in]
    mult: np.ndarray  # int64 [out]
    shift: np.ndarray  # int64 [out]
    bias: np.ndarray  # int64 [out], Q8

    @property
    def out_features(self) -> int:
        return self.w.shape[0]

    @property
    def in_features(self) -> int:
        return self.w.shape[1]


@dataclass
class QuantModel:
    cfg: Config
    layout_hash: int
    inputs: dict[str, np.ndarray]  # per table, the input multipliers
    exp_lut: np.ndarray
    edges: np.ndarray  # [n_conv, d_tile] Q8
    tag_emb: np.ndarray  # [heads × tag_buckets, tag_dim] Q8
    param_emb: np.ndarray  # [heads × param_buckets, param_dim] Q8
    sig_lut: np.ndarray = None
    tanh_lut: np.ndarray = None
    id_emb: dict = field(default_factory=dict)  # table → [n_id × id_buckets, id_dim] Q8
    engram: dict = field(default_factory=dict)  # site → [engram_buckets, engram_dim] Q8
    layers: dict[str, QLayer] = field(default_factory=dict)
    # Version 5 (defer on): added to the defer candidate's logit (Q8).
    defer_bias: int = 0


def layer_names(cfg: Config) -> list[str]:
    """The dense layers in the order of the weights file."""
    names = ["tile_embed"] + [f"conv{i}" for i in range(cfg.n_conv)] + ["region", "entity", "global"]
    names += ["slot.player", "slot.nation", "slot.unit"]
    if cfg.coarse:
        names.append("coarse")
    if cfg.aim:
        names.append("aim")
    for i in range(cfg.n_attn):
        names += [f"attn{i}.qkv", f"attn{i}.out", f"attn{i}.ff1", f"attn{i}.ff2"]
    names.append("candidate")
    for h in range(cfg.n_decision_heads):
        names += [f"head{h}.query", f"head{h}.hidden", f"head{h}.logit"]
    names += ["value.hidden", "value.out"]
    for kind in ("player", "nation", "unit"):
        names += [f"gru.{kind}.ih", f"gru.{kind}.hh"]
    if cfg.trace:
        names.append("trace")  # version 3
    if cfg.events:
        names += ["event", "xattn.q", "xattn.kv", "xattn.out"]  # version 4
    return names


def _torch_weights(model, name: str):
    """(weight, bias) of a dense layer of the float model."""
    if name.startswith("gru."):
        _, kind, part = name.split(".")
        cell = getattr(model, f"gru_{kind}")
        return getattr(cell, f"weight_{part}"), getattr(cell, f"bias_{part}")
    if name == "trace":
        return model.trace.weight, model.trace.weight.new_zeros(model.trace.weight.shape[0])
    if name == "event":
        return model.event.weight, model.event.bias
    if name.startswith("xattn."):
        lin = getattr(model.xattn, name[6:])
        return lin.weight, lin.bias
    if name.startswith("slot."):
        lin = getattr(model, f"slot_{name[5:]}")
    elif name in ("coarse", "aim"):
        lin = getattr(model, name)
    else:
        lin = _torch_layer(model, name)
    return lin.weight, lin.bias


def _torch_layer(model, name: str):
    if name == "tile_embed":
        return model.tile_embed
    if name.startswith("conv"):
        return model.convs[int(name[4:])]
    if name in ("region", "entity"):
        return getattr(model, name)
    if name == "global":
        return model.glob
    if name == "candidate":
        return model.cand
    if name.startswith("attn"):
        i, part = name[4:].split(".")
        return getattr(model.blocks[int(i)], part)
    if name.startswith("head"):
        h, part = name[4:].split(".")
        return {"query": model.head_query, "hidden": model.head_hidden, "logit": model.head_logit}[part][int(h)]
    if name == "value.hidden":
        return model.value_hidden
    if name == "value.out":
        return model.value_out
    raise KeyError(name)


def quantize_linear(weight: np.ndarray, bias: np.ndarray) -> QLayer:
    w = np.asarray(weight, dtype=np.float64)
    b = np.asarray(bias, dtype=np.float64)
    scale = np.abs(w).max(axis=1) / 127.0
    scale = np.where(scale > 0, scale, 1.0 / 127.0)
    wq = np.clip(np.round(w / scale[:, None]), -127, 127).astype(np.int64)
    # y_q = sum × scale (input and output both Q8): scale ≈ mult / 2^shift.
    shift = (MULT_BITS - np.floor(np.log2(scale))).astype(np.int64)
    mult = np.round(scale * np.exp2(shift.astype(np.float64))).astype(np.int64)
    if (shift < 1).any() or (shift > 62).any():
        raise ValueError(f"a weight scale is out of range: {scale}")
    bq = np.clip(np.round(b * ACT_ONE), -(2**31), 2**31 - 1).astype(np.int64)
    return QLayer(wq, mult, shift, bq)


def _q8(a: np.ndarray) -> np.ndarray:
    return np.clip(np.round(np.asarray(a, np.float64) * ACT_ONE), I16_MIN, I16_MAX).astype(np.int64)


def quantize(model, layout) -> QuantModel:
    cfg = model.cfg
    sd = {k: v.detach().cpu().double().numpy() for k, v in model.state_dict().items()}
    qm = QuantModel(
        cfg=cfg,
        layout_hash=layout.hash(),
        inputs={t: layout.input_multipliers(t) for t in layout.tables()},
        exp_lut=exp_lut(),
        edges=_q8(sd["edges"]),
        tag_emb=_q8(sd["tag_emb.weight"]),
        param_emb=_q8(sd["param_emb.weight"]),
        defer_bias=int(np.round(float(sd["defer_bias"]) * ACT_ONE)) if cfg.defer else 0,
    )
    qm.sig_lut, qm.tanh_lut = gate_luts()
    for table in ("tile", "global", "entity", "candidate") + ((EVENT_TABLE,) if cfg.events else ()):
        if cfg.id_columns(table):
            qm.id_emb[table] = _q8(sd[f"id_emb.{table}.weight"])
    if cfg.engram:
        for site in ("tile", "entity", "candidate"):
            qm.engram[site] = _q8(sd[f"engram.{site}.weight"])
    for name in layer_names(cfg):
        w, b = _torch_weights(model, name)
        qm.layers[name] = quantize_linear(w.detach().cpu().double().numpy(), b.detach().cpu().double().numpy())
    return qm


# --- integer primitives --------------------------------------------------


def sat16(x):
    return np.clip(x, I16_MIN, I16_MAX)


def round_shift(v, r):
    r = np.asarray(r, dtype=np.int64)
    return (v + (np.int64(1) << (r - 1))) >> r


def div_round(a, b):
    return (2 * a + b) // (2 * b)


def isqrt(v: np.ndarray) -> np.ndarray:
    v = np.asarray(v, dtype=np.int64)
    s = np.floor(np.sqrt(v.astype(np.float64))).astype(np.int64)
    for _ in range(3):
        s = np.where(s * s > v, s - 1, s)
        s = np.where((s + 1) * (s + 1) <= v, s + 1, s)
    assert ((s * s <= v) & ((s + 1) * (s + 1) > v)).all()
    return s


def dense(x: np.ndarray, layer: QLayer, relu: bool = False) -> np.ndarray:
    acc = np.asarray(x, dtype=np.int64) @ layer.w.T
    y = round_shift(acc * layer.mult, layer.shift) + layer.bias
    if relu:
        y = np.maximum(y, 0)
    return sat16(y)


def input_q8(stored: np.ndarray, mult: np.ndarray) -> np.ndarray:
    return sat16(round_shift(np.asarray(stored, np.int64) * mult, INPUT_SHIFT))


def table_input(qm: "QuantModel", table: str, stored) -> np.ndarray:
    """A table's rows as layer input (Q8): the numeric columns, then each
    id column's embedding row."""
    cfg = qm.cfg
    n = getattr(cfg, f"c_{table}")
    stored = np.asarray(stored, np.int64).reshape(-1, n)
    ids = list(cfg.id_columns(table))
    num = [i for i in range(n) if i not in ids]
    out = [input_q8(stored[:, num], qm.inputs[table][num])]
    for k, c in enumerate(ids):
        out.append(qm.id_emb[table][id_index(k, stored[:, c], cfg)])
    return np.concatenate(out, axis=1)


def layer_norm(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, np.int64)
    d = x.shape[-1]
    lg = d.bit_length() - 1
    assert 1 << lg == d and lg <= 16
    s = x.sum(-1, keepdims=True)
    c = d * x - s
    v = (c * c).sum(-1) << (16 - lg)
    sigma = isqrt(v + ln_eps_q(d))
    return sat16(div_round(c << 16, sigma[..., None]))


def score_shift(head_dim: int) -> int:
    root = math.isqrt(head_dim)
    lg = root.bit_length() - 1
    assert root * root == head_dim and 1 << lg == root, "the head width must be a power of 4"
    return ACT_BITS + lg


def attention_block(x: np.ndarray, qm: QuantModel, i: int) -> np.ndarray:
    cfg = qm.cfg
    d, h = cfg.d_model, cfg.n_attn_heads
    dh = d // h
    sh = score_shift(dh)
    lut = qm.exp_lut
    n_lut = len(lut)
    a = layer_norm(x)
    qkv = dense(a, qm.layers[f"attn{i}.qkv"])
    q, k, v = qkv[:, :d], qkv[:, d : 2 * d], qkv[:, 2 * d :]
    outs = []
    for j in range(h):
        sl = slice(j * dh, (j + 1) * dh)
        s = round_shift(q[:, sl] @ k[:, sl].T, sh)
        t = s.max(axis=1, keepdims=True) - s
        e = np.where(t < n_lut, lut[np.minimum(t, n_lut - 1)], 0)
        num = e @ v[:, sl]
        den = e.sum(axis=1, keepdims=True)
        outs.append(div_round(num, den))
    o = sat16(np.concatenate(outs, axis=1))
    x = sat16(x + dense(o, qm.layers[f"attn{i}.out"]))
    a = layer_norm(x)
    f = dense(dense(a, qm.layers[f"attn{i}.ff1"], relu=True), qm.layers[f"attn{i}.ff2"])
    return sat16(x + f)


@dataclass
class Memory:
    """A player's integer memory (Q8): the player slot, nation and unit slots
    by persistent id."""

    player: np.ndarray
    nations: dict = field(default_factory=dict)
    units: dict = field(default_factory=dict)

    @staticmethod
    def empty(cfg: Config) -> "Memory":
        return Memory(np.zeros(cfg.mem_player, np.int64))


@dataclass
class TurnState:
    tile_h: np.ndarray  # [T, d_tile]
    tokens: np.ndarray  # [R + E + 2 + S, d_model], after the final LayerNorm
    n_regions: int
    n_entities: int
    value: int  # Q8
    slot_entities: np.ndarray  # [S]: the entity of each slot token
    entity_ids: np.ndarray  # [E]
    entity_kinds: np.ndarray  # [E]
    off: dict = None  # Config.offsets
    # The turn trace (trace on): Σ of the traced choices' candidate
    # encodings so far this turn, exact ([d_model], Q8; note_choice).
    trace: np.ndarray = None

    def slot_token(self, e: int) -> int:
        """The token index of entity `e`'s slot, −1 none."""
        hits = np.nonzero(self.slot_entities == e)[0]
        return self.off["slots"] + int(hits[0]) if len(hits) else -1


def _slot_kind(kind: int) -> str | None:
    if kind in UNIT_SLOT_KINDS:
        return "unit"
    if kind in NATION_SLOT_KINDS:
        return "nation"
    return None


def _pool(h: np.ndarray, groups, d_tile: int) -> np.ndarray:
    """Mean (rounded) and max of the tile states per group."""
    groups = np.asarray(groups, np.int64)
    n = int(groups.max()) + 1 if len(groups) else 0
    ok = groups >= 0
    sums = np.zeros((n, d_tile), np.int64)
    np.add.at(sums, groups[ok], h[ok])
    cnt = np.bincount(groups[ok], minlength=n).astype(np.int64)
    mean = np.where(cnt[:, None] > 0, div_round(sums, np.maximum(cnt, 1)[:, None]), 0)
    mx = np.zeros((n, d_tile), np.int64)
    np.maximum.at(mx, groups[ok], h[ok])
    return np.concatenate([mean, mx], axis=1)


def event_attention(qm: QuantModel, x: np.ndarray, reg_tok: np.ndarray, ent_tok: np.ndarray, region, events, event_meta, n_tiles: int) -> np.ndarray:
    """The event cross-attention (events on; docs/ai-net-model.md, Events)
    on the tokens `x` before the attention blocks: each event's token is the
    event layer on [its row, the region token of its tile, has a tile, its
    actor's entity token, has an actor]; every token attends over them
    (q = xattn.q(LN(x)), k, v = xattn.kv(LN(event)), the self-attention's
    heads, scores and softmax) and adds xattn.out of the result."""
    cfg = qm.cfg
    d, h = cfg.d_model, cfg.n_attn_heads
    rows = np.asarray(events, np.int64).reshape(-1, cfg.c_event)
    n_ev = len(rows)
    if n_ev == 0:
        return x
    meta = np.asarray(event_meta, np.int64).reshape(n_ev, 4)
    region = np.asarray(region, np.int64)
    tv = (meta[:, 1] >= 0) & (meta[:, 1] < n_tiles)
    rg = np.where(tv, region[np.where(tv, meta[:, 1], 0)] if n_tiles else 0, -1)
    rv = tv & (rg >= 0) & (rg < len(reg_tok))
    reg = np.where(rv[:, None], reg_tok[np.where(rv, rg, 0)], 0)
    av = (meta[:, 2] >= 0) & (meta[:, 2] < len(ent_tok))
    ent = np.where(av[:, None], ent_tok[np.where(av, meta[:, 2], 0)], 0) if len(ent_tok) else np.zeros((n_ev, d), np.int64)
    inp = np.concatenate([table_input(qm, EVENT_TABLE, rows), reg, (rv * ACT_ONE)[:, None], ent, (av * ACT_ONE)[:, None]], axis=1)
    ev = dense(inp, qm.layers["event"])
    kv = dense(layer_norm(ev), qm.layers["xattn.kv"])
    k, v = kv[:, :d], kv[:, d:]
    q = dense(layer_norm(x), qm.layers["xattn.q"])
    dh = d // h
    sh = score_shift(dh)
    lut = qm.exp_lut
    n_lut = len(lut)
    outs = []
    for j in range(h):
        sl = slice(j * dh, (j + 1) * dh)
        s = round_shift(q[:, sl] @ k[:, sl].T, sh)
        t = s.max(axis=1, keepdims=True) - s
        e = np.where(t < n_lut, lut[np.minimum(t, n_lut - 1)], 0)
        outs.append(div_round(e @ v[:, sl], e.sum(axis=1, keepdims=True)))
    o = sat16(np.concatenate(outs, axis=1))
    return sat16(x + dense(o, qm.layers["xattn.out"]))


def encode_turn(qm: QuantModel, tiles, neighbors, region, global_row, entities, entity_meta, entity_ids=None, memory: Memory | None = None,
                coarse=None, aim=None, events=None, event_meta=None) -> TurnState:
    """begin_turn: the tile states and tokens of a turn, given the memory
    (and, with those flags on, the coarse block of each tile, the aim
    weights, Q8, and the turn's events: rows [V, C_event] and meta [V, 4])."""
    cfg = qm.cfg
    memory = memory or Memory.empty(cfg)
    tiles = np.asarray(tiles, np.int64).reshape(-1, cfg.c_tile)
    n_tiles = len(tiles)
    tile_in = table_input(qm, "tile", tiles)
    if cfg.engram:
        tile_in = np.concatenate([tile_in, qm.engram["tile"][tile_engram(cfg, tiles)]], axis=1)
    h = dense(tile_in, qm.layers["tile_embed"], relu=True)
    nb = np.asarray(neighbors, np.int64).reshape(n_tiles, 6)
    nb = np.where((nb < 0) | (nb >= n_tiles), n_tiles, nb)
    idx = np.concatenate([np.arange(n_tiles)[:, None], nb], axis=1)
    for l in range(cfg.n_conv):
        ext = np.concatenate([h, qm.edges[l][None, :]], axis=0)
        g = ext[idx].reshape(n_tiles, 7 * cfg.d_tile)
        h = sat16(h + dense(g, qm.layers[f"conv{l}"], relu=True))
    reg_tok = dense(_pool(h, region, cfg.d_tile), qm.layers["region"])
    r = len(reg_tok)
    rc = 0
    if cfg.coarse:
        coarse_tok = dense(_pool(h, coarse, cfg.d_tile), qm.layers["coarse"])
        rc = len(coarse_tok)
        reg_tok = np.concatenate([reg_tok, coarse_tok], axis=0)

    ents = np.asarray(entities, np.int64).reshape(-1, cfg.c_entity)
    n_ent = len(ents)
    em = np.asarray(entity_meta, np.int64).reshape(n_ent, 3)
    ids = np.full(n_ent, -1, np.int64) if entity_ids is None else np.asarray(entity_ids, np.int64).reshape(n_ent)
    kind = np.zeros((n_ent, cfg.n_kinds), np.int64)
    kv = (em[:, 0] >= 0) & (em[:, 0] < cfg.n_kinds)
    kind[np.nonzero(kv)[0], em[kv, 0]] = ACT_ONE
    rel = np.zeros((n_ent, cfg.n_relations), np.int64)
    rv = (em[:, 2] >= 0) & (em[:, 2] < cfg.n_relations)
    rel[np.nonzero(rv)[0], em[rv, 2]] = ACT_ONE
    tv = (em[:, 1] >= 0) & (em[:, 1] < n_tiles)
    eh = np.where(tv[:, None], h[np.where(tv, em[:, 1], 0)], 0) if n_tiles else np.zeros((n_ent, cfg.d_tile), np.int64)
    ent_rows = table_input(qm, "entity", ents)
    if cfg.engram:
        ent_rows = np.concatenate([ent_rows, qm.engram["entity"][entity_engram(cfg, ents, em[:, 0])]], axis=1)
    ent_in = np.concatenate([ent_rows, kind, rel, eh, (tv * ACT_ONE)[:, None]], axis=1)
    ent_tok = dense(ent_in, qm.layers["entity"])
    glob_tok = dense(table_input(qm, "global", np.asarray(global_row, np.int64)[None, :]), qm.layers["global"])
    player_tok = dense(memory.player[None, :], qm.layers["slot.player"])
    slot_entities, slot_toks = [], []
    for e in range(n_ent):
        sk = _slot_kind(int(em[e, 0]))
        if sk is None:
            continue
        width = cfg.mem_unit if sk == "unit" else cfg.mem_nation
        store = memory.units if sk == "unit" else memory.nations
        m = store.get(int(ids[e]), None) if ids[e] >= 0 else None
        m = np.zeros(width, np.int64) if m is None else m
        slot_toks.append(sat16(dense(m[None, :], qm.layers[f"slot.{sk}"])[0] + ent_tok[e]))
        slot_entities.append(e)
    slots = np.asarray(slot_toks, np.int64).reshape(-1, cfg.d_model)
    parts = [reg_tok, ent_tok, glob_tok, player_tok]
    if cfg.aim:
        a = np.zeros(cfg.n_aims, np.int64) if aim is None else np.asarray(aim, np.int64).reshape(cfg.n_aims)
        parts.append(dense(a[None, :], qm.layers["aim"]))
    x = np.concatenate(parts + [slots], axis=0)
    if cfg.events and events is not None:
        x = event_attention(qm, x, reg_tok[:r], ent_tok, region, events, event_meta, n_tiles)
    for i in range(cfg.n_attn):
        x = attention_block(x, qm, i)
    x = layer_norm(x)
    off = cfg.offsets(r, rc, n_ent)
    g = off["global"]
    value = dense(dense(x[g : g + 1], qm.layers["value.hidden"], relu=True), qm.layers["value.out"])[0, 0]
    return TurnState(h, x, r, n_ent, int(value), np.asarray(slot_entities, np.int64), ids, em[:, 0].copy(), off, np.zeros(cfg.d_model, np.int64))


def gru(qm: QuantModel, kind: str, x: np.ndarray, hidden: np.ndarray) -> np.ndarray:
    """One integer GRU step (PyTorch's GRUCell: gates r, z, n)."""
    m = len(hidden)
    gi = dense(np.asarray(x, np.int64)[None, :], qm.layers[f"gru.{kind}.ih"])[0]
    gh = dense(np.asarray(hidden, np.int64)[None, :], qm.layers[f"gru.{kind}.hh"])[0]
    idx = lambda v: np.clip(v, -GATE_HALF, GATE_HALF) + GATE_HALF
    r = qm.sig_lut[idx(gi[:m] + gh[:m])]
    z = qm.sig_lut[idx(gi[m : 2 * m] + gh[m : 2 * m])]
    rn = round_shift(r * gh[2 * m :], GATE_BITS)
    n = qm.tanh_lut[idx(gi[2 * m :] + rn)]
    one = 1 << GATE_BITS
    return sat16(round_shift((one - z) * n + z * (np.asarray(hidden, np.int64) << (GATE_BITS - ACT_BITS)), 2 * GATE_BITS - ACT_BITS))


def end_turn(qm: QuantModel, st: TurnState, memory: Memory | None = None) -> Memory:
    """The memory after the turn: the player slot and the slots of the
    turn's nations and units updated from their tokens (other nations kept,
    units gone dropped); in entity order, a repeated id keeps the last."""
    cfg = qm.cfg
    memory = memory or Memory.empty(cfg)
    new = Memory(gru(qm, "player", st.tokens[st.off["player"]], memory.player), dict(memory.nations), {})
    for s, e in enumerate(st.slot_entities):
        ident = int(st.entity_ids[e])
        if ident < 0:
            continue
        sk = _slot_kind(int(st.entity_kinds[e]))
        store_old = memory.units if sk == "unit" else memory.nations
        width = cfg.mem_unit if sk == "unit" else cfg.mem_nation
        old = store_old.get(ident)
        old = np.zeros(width, np.int64) if old is None else old
        out = gru(qm, sk, st.tokens[st.off["slots"] + s], old)
        (new.units if sk == "unit" else new.nations)[ident] = out
    return new


def query(qm: QuantModel, st: TurnState, head: int, actor: int) -> np.ndarray:
    """The head's query of the decision's context (d_model, Q8): the actor's
    token plus its slot's token, or the global token plus the player slot's."""
    if head in (HEAD_UNIT, HEAD_PRODUCTION) and 0 <= actor < st.n_entities:
        ctx = st.tokens[st.off["ent"] + actor]
        s = st.slot_token(actor)
        if s >= 0:
            ctx = sat16(ctx + st.tokens[s])
    else:
        ctx = sat16(st.tokens[st.off["global"]] + st.tokens[st.off["player"]])
    if qm.cfg.trace:
        ctx = sat16(ctx + trace_vector(qm, st))
    return dense(ctx[None, :], qm.layers[f"head{head}.query"])[0]


def trace_vector(qm: QuantModel, st: TurnState) -> np.ndarray:
    """What the turn trace adds to a decision's context: the trace layer on
    sat(round_shift(Σ, trace_shift)) (Q8)."""
    shift = qm.cfg.trace_shift
    s = sat16(round_shift(st.trace, shift) if shift > 0 else st.trace)
    return dense(s[None, :], qm.layers["trace"])[0]


def note_choice(qm: QuantModel, st: TurnState, head: int, actor: int, rows, meta, choice: int):
    """A decision of the turn was made: its chosen candidate's encoding
    joins the turn trace when the head is traced and the choice is no wait
    and no deferral."""
    cfg = qm.cfg
    meta = np.asarray(meta, np.int64).reshape(-1, 4)
    if not cfg.traces(head) or not 0 <= choice < len(meta) or meta[choice, 0] in (WAIT_TAG, DEFER_TAG):
        return
    rows = np.asarray(rows, np.int64).reshape(-1, cfg.c_candidate)
    st.trace = st.trace + candidate_encoding(qm, st, head, actor, rows[choice : choice + 1], meta[choice : choice + 1])[0]


def score(qm: QuantModel, st: TurnState, head: int, actor: int, rows, meta) -> np.ndarray:
    """The logits (Q8, int64) of a decision's candidates."""
    q = query(qm, st, head, actor)
    c = candidate_encoding(qm, st, head, actor, rows, meta)
    hid = np.maximum(sat16(c + q[None, :]), 0)
    z = dense(hid, qm.layers[f"head{head}.hidden"], relu=True)
    return dense(z, qm.layers[f"head{head}.logit"])[:, 0] + defer_offsets(qm, head, meta)


def defer_offsets(qm: QuantModel, head: int, meta) -> np.ndarray:
    """What each candidate's logit adds (Q8, int64): the defer bias on the
    defer candidate of the unit head (defer on), else 0."""
    meta = np.asarray(meta, np.int64).reshape(-1, 4)
    if not qm.cfg.defer or head != HEAD_UNIT:
        return np.zeros(len(meta), np.int64)
    return np.where(meta[:, 0] == DEFER_TAG, np.int64(qm.defer_bias), np.int64(0))


def candidate_encoding(qm: QuantModel, st: TurnState, head: int, actor: int, rows, meta) -> np.ndarray:
    """The candidate layer on a decision's candidates ([k, d_model], Q8):
    the encoding the head's query is added to, and the turn trace sums."""
    cfg = qm.cfg
    rows = np.asarray(rows, np.int64).reshape(-1, cfg.c_candidate)
    meta = np.asarray(meta, np.int64).reshape(len(rows), 4)
    n_tiles = len(st.tile_h)
    tv = (meta[:, 1] >= 0) & (meta[:, 1] < n_tiles)
    th = np.where(tv[:, None], st.tile_h[np.where(tv, meta[:, 1], 0)], 0)
    ev = (meta[:, 2] >= 0) & (meta[:, 2] < st.n_entities)
    eh = np.where(ev[:, None], st.tokens[st.off["ent"] + np.where(ev, meta[:, 2], 0)], 0)
    cand_rows = table_input(qm, "candidate", rows)
    if cfg.engram:
        ak = int(st.entity_kinds[actor]) + 1 if 0 <= actor < st.n_entities else 0
        cand_rows = np.concatenate([cand_rows, qm.engram["candidate"][candidate_engram(cfg, head, meta, ak, st.entity_kinds, n_tiles)]], axis=1)
    inp = np.concatenate(
        [
            cand_rows,
            qm.tag_emb[tag_index(head, meta[:, 0], cfg)],
            qm.param_emb[param_index(head, meta[:, 3], cfg)],
            th,
            (tv * ACT_ONE)[:, None],
            eh,
            (ev * ACT_ONE)[:, None],
        ],
        axis=1,
    )
    return dense(inp, qm.layers["candidate"])


def turn_logits(qm: QuantModel, turn, memory: Memory | None = None) -> tuple[TurnState, list[np.ndarray]]:
    """Integer logits of every decision of a `data.Turn`, in order, each
    choice (the label) joining the turn trace after its decision."""
    st = encode_turn(qm, turn.tiles, turn.neighbors, turn.region, turn.global_row, turn.entities, turn.entity_meta, turn.entity_ids, memory,
                     coarse=turn.coarse, events=turn.events, event_meta=turn.event_meta)
    out = []
    for d in turn.decisions:
        out.append(score(qm, st, d.head, d.actor, d.rows, d.meta))
        note_choice(qm, st, d.head, d.actor, d.rows, d.meta, d.choice)
    return st, out


def agreement(model, qm: QuantModel, windows, cfg: Config) -> dict:
    """Top-1 agreement of the float model and the integer reference, and of
    each with the expert, over the decisions of `windows` (lists of
    consecutive `data.Turn`s of one player), each side carrying its own
    memory along the window."""
    import torch

    from .data import collate
    from .model import MemoryBank, memory_batch, padded_logits, store_memory

    same = total = int_expert = float_expert = 0
    model.eval()
    with torch.no_grad():
        for window in windows:
            bank = MemoryBank(cfg, "cpu")
            imem = Memory.empty(cfg)
            for turn in window:
                batch = memory_batch([bank], collate([turn], cfg), cfg)
                (logits, _value, (new_p, new_n, new_u)) = model(batch, with_memory=True)[:3]
                store_memory([bank], batch, new_p, new_n, new_u)
                st, il = turn_logits(qm, turn, imem)
                imem = end_turn(qm, st, imem)
                if not turn.decisions:
                    continue
                fl = padded_logits(batch, logits).numpy()
                for d, f_row, i_row in zip(turn.decisions, fl, il):
                    fa = int(np.argmax(f_row[: len(d.rows)]))
                    ia = int(np.argmax(i_row))
                    same += fa == ia
                    int_expert += ia == d.choice
                    float_expert += fa == d.choice
                    total += 1
    return {"decisions": total, "float_int": same / max(total, 1), "int_expert": int_expert / max(total, 1), "float_expert": float_expert / max(total, 1)}
