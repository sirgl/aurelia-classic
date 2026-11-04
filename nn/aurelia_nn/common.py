"""Shapes, constants and id hashing shared by the float model, the integer
reference and the Rust inference (classic/docs/ai-net-model.md)."""

from __future__ import annotations

from dataclasses import dataclass, asdict

# Activations between layers are fixed point with this many fraction bits
# (Q8: value × 256), held in i16.
ACT_BITS = 8
ACT_ONE = 1 << ACT_BITS
I16_MIN, I16_MAX = -32768, 32767

# Input normalisation: x_q = round_shift(stored × mult, INPUT_SHIFT), mult =
# round(2^(ACT_BITS + INPUT_SHIFT) / scale).
INPUT_SHIFT = 16

# exp LUT for the softmax: EXP_LUT[t] = round(EXP_ONE × exp(−t / ACT_ONE)).
EXP_BITS = 15
EXP_ONE = 1 << EXP_BITS

LN_EPS = 1e-5

# Decision heads, in the order of decisions.npy's head column.
HEADS = ("unit_command", "research", "production", "policy")
HEAD_UNIT, HEAD_RESEARCH, HEAD_PRODUCTION, HEAD_POLICY = range(4)

# The tables of the feature layout, in the order of the weights file.
TABLES = ("tile", "global", "entity", "candidate")
# Format 7: the event table, after them (in the layout hash when the layout
# has it; in the weights file from version 4).
EVENT_TABLE = "event"


@dataclass
class Config:
    c_tile: int
    c_global: int
    c_entity: int
    c_candidate: int
    d_tile: int = 32
    d_model: int = 64
    n_attn_heads: int = 4
    n_conv: int = 3
    n_attn: int = 2
    d_ff: int = 128
    d_head: int = 32
    n_kinds: int = 8
    n_relations: int = 8
    n_decision_heads: int = 4
    tag_buckets: int = 128
    tag_dim: int = 8
    param_buckets: int = 256
    param_dim: int = 16
    mem_player: int = 128
    mem_nation: int = 32
    mem_unit: int = 16
    id_buckets: int = 256
    id_dim: int = 4
    # Flags (each off in the E1 baseline; docs/ai-net-model.md, Flags).
    engram: int = 0  # hashed lookup embeddings on tiles, entities, candidates
    engram_buckets: int = 1 << 14  # a power of two
    engram_dim: int = 16
    aim: int = 0  # a goal token from the aim weights
    n_aims: int = 3  # science, score, conquest
    coarse: int = 0  # a second region level of 16×16-tile blocks
    # The turn trace (docs/ai-net-model.md, The turn trace): every decision's
    # context adds W_tr · clamp(Σ u / 2^trace_shift), the sum over the
    # candidate encodings u of the choices made earlier in the player-turn
    # by the heads of the trace_heads bit mask (a wait excepted). Weights
    # file version 3; off: version 2.
    trace: int = 0
    trace_heads: int = 0b1111  # unit, research, production, policy: the heads the net plays
    trace_shift: int = 3
    # The events (docs/ai-net-model.md, Events; format 7): every token
    # attends over the turn's event tokens and adds the result through
    # xattn.out, which starts at zero. Weights file version 4 (written when
    # on); c_event is the event table's width.
    events: int = 0
    c_event: int = 0
    # The defer choice (docs/ai-net-model.md, Defer; format 9): the unit
    # head also scores the defer candidate (tag DEFER_TAG, a zero row), its
    # logit plus the learned `defer_bias` (Net.defer_bias, Q8 in the file).
    # Weights file version 5 (written when on).
    defer: int = 0
    # Training only (not in the weights file).
    aux: int = 0  # auxiliary future heads on the global token (aux.npy)
    privileged_value: int = 0  # the value head reads the truth row (truth.npy)
    n_aux: int = 0
    n_truth: int = 0
    # The candidate aux head (format 10, `cand_aux.py`): its outputs (0 off).
    cand_aux: int = 0
    # The v2 architecture (docs/ai-net-model.md, «v2 architecture»;
    # `model_v2.py`): 0 the net above, 2 v2 (float only: no weights file
    # yet). v2's map blocks (`n_conv` of them) have these dilations and an
    # MLP of `map_ratio × d_tile`; `combat` the pair features of the combat
    # module in the candidate scorer.
    arch: int = 0
    map_dilations: tuple = ()
    map_ratio: int = 2
    combat: int = 0
    # Measurement only (`model_v2.Measured`): the format-10 move-strike
    # columns zeroed in the candidate input; the candidate aux head as a
    # probe (its input detached), its targets read off the candidate rows.
    drop_move_strike: int = 0
    cand_aux_probe: int = 0
    # Id columns (layout names ending in `_id_plus_1`) per table: embedded,
    # not fed as numbers. Written in their own section of the weights file.
    id_tile: tuple = ()
    id_global: tuple = ()
    id_entity: tuple = ()
    id_candidate: tuple = ()
    id_event: tuple = ()
    # Engram keys beyond the id columns: small numeric columns (clipped to
    # 0 … 15) of the entity table (`level`, `promotions`).
    engram_entity_small: tuple = ()

    # Order of the config block in the weights file (append only).
    FILE_ORDER = (
        "c_tile", "c_global", "c_entity", "c_candidate", "d_tile", "d_model", "n_attn_heads", "n_conv", "n_attn",
        "d_ff", "d_head", "n_kinds", "n_relations", "n_decision_heads", "tag_buckets", "tag_dim", "param_buckets",
        "param_dim", "mem_player", "mem_nation", "mem_unit", "id_buckets", "id_dim",
        "engram", "engram_buckets", "engram_dim", "aim", "n_aims", "coarse",
        # Version 3 only (a version-2 file ends its config at `coarse`).
        "trace", "trace_heads", "trace_shift",
        # Version 4 on.
        "events", "c_event",
        # Version 5 only.
        "defer",
    )
    # The config fields of a version-2, 3 and 4 file.
    FILE_FIELDS_V2 = 29
    FILE_FIELDS_V3 = 32
    FILE_FIELDS_V4 = 34

    def traces(self, head: int) -> bool:
        """Whether a choice of `head` enters the turn trace (trace on)."""
        return bool(self.trace) and 0 <= head < 31 and bool((self.trace_heads >> head) & 1)

    @staticmethod
    def preset(name: str, c_tile: int, c_global: int, c_entity: int, c_candidate: int) -> "Config":
        return Config(c_tile=c_tile, c_global=c_global, c_entity=c_entity, c_candidate=c_candidate, **PRESETS[name])

    def as_dict(self) -> dict:
        return asdict(self)

    def id_columns(self, table: str) -> tuple:
        return tuple(getattr(self, f"id_{table}"))

    def row_in(self, table: str) -> int:
        """Width of a table row as a layer input: the numeric columns, an
        embedding of id_dim per id column, then (engram on, tile, entity and
        candidate tables) the engram vector."""
        n = getattr(self, f"c_{table}")
        k = len(self.id_columns(table))
        return n - k + k * self.id_dim + (self.engram_dim if self.engram and table not in ("global", EVENT_TABLE) else 0)

    @property
    def entity_in(self) -> int:
        return self.row_in("entity") + self.n_kinds + self.n_relations + self.d_tile + 1

    @property
    def candidate_in(self) -> int:
        return self.row_in("candidate") + self.tag_dim + self.param_dim + self.d_tile + 1 + self.d_model + 1

    @property
    def event_in(self) -> int:
        """The event layer's input: the event row, the region token of its
        tile, has a tile, the actor's entity token, has an actor."""
        return self.row_in(EVENT_TABLE) + 2 * (self.d_model + 1)

    def offsets(self, r: int, rc: int, e: int) -> dict:
        """Token indices: regions [0, r), coarse regions [r, r + rc),
        entities, global, player slot, aim (when on), then the entity slots."""
        ent = r + rc
        g = ent + e
        aim = g + 2 if self.aim else -1
        return {"ent": ent, "global": g, "player": g + 1, "aim": aim, "slots": g + 2 + (1 if self.aim else 0)}

    @property
    def head_dim(self) -> int:
        return self.d_model // self.n_attn_heads


# Model sizes (parameters with the shipped layout: 47 tile, 247 global, 86
# entity, 90 candidate features). An attention head is 16 or 64 wide (a power
# of 4); d_model a power of two.
PRESETS = {
    "small": dict(d_tile=32, n_conv=3, d_model=64, n_attn=2, n_attn_heads=4, d_ff=128, d_head=32),
    "medium": dict(d_tile=64, n_conv=4, d_model=128, n_attn=4, n_attn_heads=8, d_ff=384, d_head=64),
    "large": dict(d_tile=96, n_conv=5, d_model=256, n_attn=4, n_attn_heads=4, d_ff=1024, d_head=128),
    # docs/ai-net-model.md, «v2 architecture»: ConvNeXt-style hex blocks with
    # dilations, RMSNorm, QK-norm, ReLU², a hex-distance attention bias, a
    # candidate scorer that attends to the entities from the move's
    # destination, the combat module. Float only (no weights file yet).
    "v2": dict(d_tile=96, n_conv=6, d_model=256, n_attn=5, n_attn_heads=4, d_ff=512, d_head=128, arch=2,
               map_dilations=(1, 1, 2, 2, 4, 4), map_ratio=2, combat=1),
}

# The unit head's "wait" candidate (meta.json `tags.wait`).
WAIT_TAG = 250
# Format 9: the unit head's "defer" candidate (meta.json `tags.defer`): the
# unit decides again after the rest of the turn's queue. Its tag bucket with
# the 128 buckets of a head (1 + 251 mod 127) is no other unit tag's.
DEFER_TAG = 251
# The defer bias a net starts with (logits): a net that gains the choice
# (Net with defer on, `aurelia_nn.defer`) rarely takes it before E2 trains it.
DEFER_BIAS_INIT = -4.0

# Entity kinds (entity_meta column 0) that carry a memory slot: an own unit a
# unit slot, a city-state or another nation a nation slot.
UNIT_SLOT_KINDS = (0,)
NATION_SLOT_KINDS = (4, 5)

# GRU gate tables: index = clamp(pre-activation Q8, −GATE_HALF, GATE_HALF) +
# GATE_HALF; values at 2^GATE_BITS.
GATE_HALF = 8 * 256
GATE_BITS = 15


ID_SUFFIX = "_id_plus_1"
# Entity columns that join the id columns in the entity's engram key.
ENGRAM_SMALL_NAMES = ("level", "promotions")
ENGRAM_SITE_TILE, ENGRAM_SITE_ENTITY, ENGRAM_SITE_CANDIDATE = 1, 2, 3
COARSE_BLOCK = 16
_FNV_OFFSET = 0xCBF29CE484222325
_FNV_PRIME = 0x100000001B3


def engram_hash(keys, buckets: int):
    """Bucket of each row of `keys` (int array [N, k]): FNV-1a over the
    values as u32 words, then `(h ^ (h >> 29)) & (buckets − 1)`. Shared
    with the Rust forward (model.rs `engram_bucket`)."""
    import numpy as np

    keys = np.asarray(keys, dtype=np.int64)
    h = np.full(keys.shape[0], _FNV_OFFSET, dtype=np.uint64)
    prime = np.uint64(_FNV_PRIME)
    for j in range(keys.shape[1]):
        h = (h ^ (keys[:, j] & 0xFFFFFFFF).astype(np.uint64)) * prime
    h = h ^ (h >> np.uint64(29))
    return (h & np.uint64(buckets - 1)).astype(np.int64)


def coarse_of_tiles(width: int, n_tiles: int):
    """The 16×16-tile block of each tile (row-major storage, `q = id % width`)."""
    import numpy as np

    ids = np.arange(n_tiles)
    q, r = ids % width, ids // width
    return ((r // COARSE_BLOCK) * (-(-width // COARSE_BLOCK)) + q // COARSE_BLOCK).astype(np.int32)


def id_index(column: int, stored, cfg: Config):
    """Row of an id column's embedding (column = the id column's rank in its
    table): 0 for none (stored ≤ 0), else 1 + (stored − 1) mod (buckets − 1)."""
    import numpy as np

    v = np.asarray(stored, dtype=np.int64)
    return column * cfg.id_buckets + np.where(v <= 0, 0, 1 + np.mod(v - 1, cfg.id_buckets - 1))


def tag_index(head, tag, cfg: Config):
    """Row of the tag embedding for a candidate's codec tag (works on ints and
    NumPy arrays): bucket 0 is "none", per decision head its own table."""
    import numpy as np

    tag = np.asarray(tag, dtype=np.int64)
    head = np.asarray(head, dtype=np.int64)
    bucket = np.where(tag < 0, 0, 1 + np.mod(tag, cfg.tag_buckets - 1))
    return head * cfg.tag_buckets + bucket


def param_index(head, param, cfg: Config):
    """Row of the parameter embedding (technology, item, policy, improvement,
    promotion id), bucket 0 is "none"."""
    import numpy as np

    param = np.asarray(param, dtype=np.int64)
    head = np.asarray(head, dtype=np.int64)
    bucket = np.where(param < 0, 0, 1 + np.mod(param, cfg.param_buckets - 1))
    return head * cfg.param_buckets + bucket


def fnv1a64(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def tile_engram(cfg: Config, tiles):
    """Engram rows of tile rows ([T, C_tile] stored): the tile's id columns."""
    import numpy as np

    tiles = np.asarray(tiles, np.int64).reshape(-1, cfg.c_tile)
    keys = np.concatenate([np.full((len(tiles), 1), ENGRAM_SITE_TILE), tiles[:, list(cfg.id_tile)]], axis=1)
    return engram_hash(keys, cfg.engram_buckets)


def entity_engram(cfg: Config, entities, kinds):
    """Engram rows of entity rows: kind, the id columns, the small columns
    (`level`, `promotions`) clipped to 0 … 15."""
    import numpy as np

    ents = np.asarray(entities, np.int64).reshape(-1, cfg.c_entity)
    kinds = np.asarray(kinds, np.int64).reshape(-1, 1)
    small = np.clip(ents[:, list(cfg.engram_entity_small)], 0, 15)
    keys = np.concatenate([np.full((len(ents), 1), ENGRAM_SITE_ENTITY), kinds, ents[:, list(cfg.id_entity)], small], axis=1)
    return engram_hash(keys, cfg.engram_buckets)


def candidate_engram(cfg: Config, head, meta, actor_kind, entity_kinds, n_tiles: int):
    """Engram rows of a decision's candidates: head, tag, target kind (the
    target entity's kind + 1, 8 a tile only, 0 none), actor kind (+1, 0
    none), parameter id."""
    import numpy as np

    meta = np.asarray(meta, np.int64).reshape(-1, 4)
    kinds = np.asarray(entity_kinds, np.int64)
    n_ent = len(kinds)
    ent = meta[:, 2]
    ev = (ent >= 0) & (ent < n_ent)
    target = np.where(ev, kinds[np.where(ev, ent, 0)] + 1 if n_ent else 0, np.where((meta[:, 1] >= 0) & (meta[:, 1] < n_tiles), 8, 0))
    n = len(meta)
    keys = np.stack([np.full(n, ENGRAM_SITE_CANDIDATE), np.full(n, head), meta[:, 0], target, np.broadcast_to(np.asarray(actor_kind), (n,)), meta[:, 3]], axis=1)
    return engram_hash(keys, cfg.engram_buckets)
