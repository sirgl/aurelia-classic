"""The v2 architecture of the end-to-end net (docs/ai-net-model.md, «v2
architecture»). Float only for now: no integer twin, no weights file.

What the small net could not learn (ai-net-data.md, «Format 10: the strike
from a move»): what a unit can strike from a move's destination. Two limits
were named: a tile saw 3 hexes, and a move candidate saw only its
destination tile's state, not the targets near it or their geometry. v2
answers each:

- **Map**: ConvNeXt-style hex blocks (`HexBlock`): a depthwise hex
  convolution over the tile and its 6 neighbours at a dilation (the
  neighbour `k` steps along each of the 6 directions), RMSNorm, an MLP with
  ReLU², residual. Dilations 1, 1, 2, 2, 4, 4: a tile sees 14 hexes.
- **Tokens** as in the first net (regions, entities, global, the memory
  slots; the events attended before the blocks).
- **Transformer** (`Block`): RMSNorm before attention and FFN, QK-norm (an
  RMSNorm of each head's queries and keys), ReLU² FFN, and a learned
  per-head bias by the hex distance between the two tokens' positions
  (region centroids, the entities' tiles, a slot its entity's; none for the
  global, player and aim tokens: a bucket of its own).
- **Candidate scorer** (`CandidateAttention`): each candidate (the
  candidate's encoding plus its head's query, as before: the acting unit's
  token and slot, the turn trace) attends to the entity tokens after the
  blocks, with keys and values that add an embedding of the hex distance
  from the candidate's tile (a move's destination; else the actor's tile)
  to the entity's, a per-head bias by that distance, and the combat
  module's pair features; a learned null key lets it attend to nothing.
  No FFN per candidate: the heads' MLPs read the residual stream after an
  RMSNorm.
- **Sum heads** (`NetV2.sum_fields`): two aggregations by sum, not by
  attention's average, over the combat module's blows: focus fire (the
  shares of the target's hit points the own units that can still strike
  deal it, gated by hex distance ≤ movement + range) and danger (the shares
  of the actor's hit points the hostile units within their movement + range
  of the candidate's tile deal it); four inputs of the candidate.
- **Combat module** (`Combat`): a small MLP from the attacker's entity row,
  the defender's row and both tiles' rows to the blow's preview (damage
  dealt and taken as shares of hit points, kill, capture). Pretrained on
  the attack candidates' previews (`v2_bench pretrain-combat`), it runs on
  every (decision actor, entity) pair: "what my blow would do to it".

`Measured` (also over the first net, `MeasuredNet`) adds the measurement
switches: the format-10 move-strike columns zeroed in the input
(`drop_move_strike`) and the candidate aux head as a detached probe whose
targets come from the candidate rows (`cand_aux_probe`).
"""

from __future__ import annotations

import math

import torch
import torch.nn as nn
import torch.nn.functional as F
from torch.utils.checkpoint import checkpoint

from . import cand_aux
from .common import EVENT_TABLE, HEAD_PRODUCTION, HEAD_UNIT, Config
from .data import Layout
from .model import CLIP, EventAttention, Net, zero_defer_tag
from .move_strike import FORMAT_10_CANDIDATE

# Hex distance buckets: 0 … 7 each, 8–11, 12–15, 16–23, 24–31, 32+; and a
# bucket for a pair where one side has no position.
_BUCKET_OF = [min(d, 7) if d < 8 else 8 if d < 12 else 9 if d < 16 else 10 if d < 24 else 11 if d < 32 else 12 for d in range(64)]
N_BUCKETS = 14
SUM_FIELDS = 4
NO_POSITION = 13


def relu2(x: torch.Tensor) -> torch.Tensor:
    return F.relu(x).square()


class RMSNorm(nn.Module):
    def __init__(self, d: int, eps: float = 1e-6):
        super().__init__()
        self.weight = nn.Parameter(torch.ones(d))
        self.eps = eps

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        # In float32 under autocast (train_imitation --autocast).
        xf = x.float()
        return (xf * torch.rsqrt(xf.square().mean(-1, keepdim=True) + self.eps) * self.weight.float()).to(x.dtype)


def map_width(nb: torch.Tensor) -> torch.Tensor:
    """The map's width as a 0-d tensor (no device sync): tiles are axial
    (q, r) stored row-major, TileId = r × width + q, the neighbours in the
    kernel's order (q+1, r), (q+1, r−1), (q, r−1), (q−1, r), (q−1, r+1),
    (q, r+1): tile 0's sixth neighbour is `width`. (The synthetic shards'
    odd-q tables put (x, y+1) fourth.)"""
    kernel = nb[0, 0] == 1
    w = torch.where(kernel, nb[0, 5], nb[0, 3])
    return torch.where(w > 0, w, nb.new_tensor(nb.shape[0]))


def tile_coords(nb: torch.Tensor):
    """Axial (q, r) of every tile, float [T] each."""
    w = map_width(nb)
    ids = torch.arange(nb.shape[0], device=nb.device)
    return torch.remainder(ids, w).float(), torch.div(ids, w, rounding_mode="floor").float()


def hex_distance(q1, r1, q2, r2) -> torch.Tensor:
    dq, dr = q1 - q2, r1 - r2
    return ((dq.abs() + dr.abs() + (dq + dr).abs()) * 0.5).round().long()


def distance_bucket(d: torch.Tensor, ok: torch.Tensor) -> torch.Tensor:
    table = torch.as_tensor(_BUCKET_OF, device=d.device)
    return torch.where(ok, table[d.clamp(0, 63)], NO_POSITION)


def dilated_index(nb: torch.Tensor, k: int) -> torch.Tensor:
    """[T × 7]: each tile, then the tile `k` steps along each of the 6
    directions (T: off the map), for an index_select over the states with
    the edge row appended."""
    t = nb.shape[0]
    nbx = torch.where(nb < 0, t, nb)
    ext = torch.cat([nbx, nbx.new_full((1, 6), t)])
    cols = torch.arange(6, device=nb.device)[None, :]
    cur = torch.arange(t, device=nb.device)[:, None].expand(t, 6)
    for _ in range(k):
        cur = ext[cur, cols]
    return torch.cat([torch.arange(t, device=nb.device)[:, None], cur], dim=1).reshape(-1)


class HexBlock(nn.Module):
    """ConvNeXt on the hex grid: depthwise 7-tap convolution at a dilation
    (an off-map tap reads the block's learned edge row), RMSNorm, Linear,
    ReLU², Linear, residual."""

    def __init__(self, dt: int, ratio: int, dilation: int):
        super().__init__()
        self.dilation = dilation
        self.dw = nn.Parameter(torch.randn(7, dt) / math.sqrt(7))
        self.dw_bias = nn.Parameter(torch.zeros(dt))
        self.edge = nn.Parameter(torch.zeros(dt))
        self.norm = RMSNorm(dt)
        self.pw1 = nn.Linear(dt, ratio * dt)
        self.pw2 = nn.Linear(ratio * dt, dt)
        with torch.no_grad():
            self.pw2.weight.mul_(0.1)

    def forward(self, h: torch.Tensor, idx: torch.Tensor) -> torch.Tensor:
        b, t, dt = h.shape
        ext = torch.cat([h, self.edge.expand(b, 1, dt)], dim=1)
        g = ext.index_select(1, idx).view(b, t, 7, dt)
        y = (g * self.dw).sum(2) + self.dw_bias
        return h + self.pw2(relu2(self.pw1(self.norm(y))))


class Block(nn.Module):
    """Pre-RMSNorm attention with QK-norm and the hex-distance bias, then a
    ReLU² FFN; both residual."""

    def __init__(self, cfg: Config):
        super().__init__()
        d = cfg.d_model
        self.heads = cfg.n_attn_heads
        dh = d // self.heads
        self.norm1 = RMSNorm(d)
        self.qkv = nn.Linear(d, 3 * d)
        self.qn = RMSNorm(dh)
        self.kn = RMSNorm(dh)
        self.out = nn.Linear(d, d)
        self.norm2 = RMSNorm(d)
        self.ff1 = nn.Linear(d, cfg.d_ff)
        self.ff2 = nn.Linear(cfg.d_ff, d)
        self.dist_bias = nn.Parameter(torch.zeros(self.heads, N_BUCKETS))

    def forward(self, x: torch.Tensor, mask: torch.Tensor, bucket: torch.Tensor) -> torch.Tensor:
        b, n, d = x.shape
        h = self.heads
        dh = d // h
        q, k, v = self.qkv(self.norm1(x)).split(d, dim=-1)
        q = self.qn(q.view(b, n, h, dh)).transpose(1, 2)
        k = self.kn(k.view(b, n, h, dh)).transpose(1, 2)
        v = v.view(b, n, h, dh).transpose(1, 2)
        bias = self.dist_bias[:, bucket].permute(1, 0, 2, 3)  # [B, H, N, N]
        bias = bias.masked_fill(~mask[:, None, None, :], float("-inf"))
        o = F.scaled_dot_product_attention(q, k, v, attn_mask=bias.to(q.dtype))
        x = x + self.out(o.transpose(1, 2).reshape(b, n, d))
        return x + self.ff2(relu2(self.ff1(self.norm2(x))))


class Combat(nn.Module):
    """The blow of an attacker on a defender (the rules' preview, learnt):
    the numeric columns of the attacker's and defender's entity rows and of
    their tiles' rows (stored / scale, then sign · log(1 + |x|)) → Linear,
    ReLU, Linear, ReLU → [damage dealt / defender's hit points, damage taken
    / attacker's hit points, kills (logit), captures (logit)] and the last
    hidden layer (`pair` returns both, per decision and entity)."""

    OUT = 4

    def __init__(self, layout: Layout, hidden: int = 128, width: int = 64):
        super().__init__()
        from .common import ID_SUFFIX

        for table in ("entity", "tile"):
            names = layout.names[table]
            cols = [i for i, n in enumerate(names) if not n.endswith(ID_SUFFIX)]
            self.register_buffer(f"cols_{table}", torch.as_tensor(cols, dtype=torch.long), persistent=False)
            self.register_buffer(f"inv_{table}", torch.as_tensor([1.0 / layout.scales[table][i] for i in cols], dtype=torch.float32), persistent=False)
        ne, nt = len(self.cols_entity), len(self.cols_tile)
        self.width = width
        self.att = nn.Linear(ne + nt, hidden)
        self.dfd = nn.Linear(ne + nt, hidden, bias=False)
        self.l2 = nn.Linear(hidden, width)
        self.head = nn.Linear(width, self.OUT)

    def feats(self, rows: torch.Tensor, table: str) -> torch.Tensor:
        x = rows[..., getattr(self, f"cols_{table}")] * getattr(self, f"inv_{table}")
        return torch.sign(x) * torch.log1p(x.abs())

    def side(self, ent: torch.Tensor, tile: torch.Tensor) -> torch.Tensor:
        return torch.cat([self.feats(ent, "entity"), self.feats(tile, "tile")], dim=-1)

    def run(self, h1: torch.Tensor):
        h2 = F.relu(self.l2(F.relu(h1)))
        return self.head(h2), h2

    def forward(self, att_ent, att_tile, def_ent, def_tile):
        """One pair per row (pretraining)."""
        return self.run(self.att(self.side(att_ent, att_tile)) + self.dfd(self.side(def_ent, def_tile)))

    def pair(self, att: torch.Tensor, dfd: torch.Tensor):
        """att [D, F] (a decision's actor), dfd [D, E, F] (the entities of
        its turn): (outputs [D, E, 4], hidden [D, E, width])."""
        return self.run(self.att(att)[:, None, :] + self.dfd(dfd))


COMBAT_ENTITY = ("hp", "civilian", "strength", "ranged_strength", "range", "movement", "moves_left", "can_attack", "kind_own_unit",
                 "kind_foreign_unit", "relation_war", "relation_barbarian")
COMBAT_CANDIDATE = ("act_melee", "act_ranged", "preview_damage_to_defender", "preview_damage_to_attacker", "preview_defender_hp", "blow_kills",
                    "blow_captures")


def combat_columns(layout: Layout) -> bool:
    """The layout has the columns the combat module and the sum heads read."""
    return set(COMBAT_ENTITY) <= set(layout.names["entity"]) and set(COMBAT_CANDIDATE) <= set(layout.names["candidate"])


def combat_targets(rows: torch.Tensor, names: list[str], attacker_hp: torch.Tensor) -> torch.Tensor:
    """f32 [N, 4] of candidate rows (stored values; the preview columns'
    scale is 1): damage dealt / the defender's hit points, damage taken /
    the attacker's, kills, captures; NaN on a row that is not a blow with a
    preview."""
    col = {n: i for i, n in enumerate(names)}
    r = lambda n: rows[:, col[n]].float()
    attack = ((r("act_melee") + r("act_ranged")) > 0) & (r("preview_defender_hp") > 0)
    out = torch.stack([(r("preview_damage_to_defender") / r("preview_defender_hp").clamp(min=1.0)).clamp(0, 1),
                       (r("preview_damage_to_attacker") / attacker_hp.float().clamp(min=1.0)).clamp(0, 1),
                       (r("blow_kills") > 0).float(), (r("blow_captures") > 0).float()], dim=-1)
    return torch.where(attack[:, None], out, float("nan"))


def combat_loss(pred: torch.Tensor, tgt: torch.Tensor, ok: torch.Tensor, weight_div: torch.Tensor | None = None) -> torch.Tensor:
    """Squared error of the two shares (the outputs read as they are), BCE
    on logits of kill and capture; the mean of the four per row, summed over
    `ok` rows (each divided by `weight_div` when given)."""
    t = tgt.nan_to_num()
    per = torch.cat([(pred[:, :2] - t[:, :2]) ** 2, F.binary_cross_entropy_with_logits(pred[:, 2:], t[:, 2:], reduction="none")], dim=-1).mean(-1)
    w = ok.to(per.dtype)
    if weight_div is not None:
        w = w / weight_div.clamp(min=1).to(per.dtype)
    return (per * w).sum()


def decision_grid(batch: dict, n_cand: int) -> torch.Tensor:
    """[D, K]: per decision its candidates' rows in order, `n_cand` (a zero
    row) past its list (collate's `pad_index` when padded)."""
    if "pad_index" in batch:
        idx = batch["pad_index"]
        return torch.where(idx >= n_cand, n_cand, idx)
    n_dec = len(batch["dec_b"])
    k_max = batch.get("k_max") or (int(batch["dec_k"].max().item()) if n_dec else 1)
    grid = torch.full((n_dec, k_max), n_cand, dtype=torch.long, device=batch["dec_b"].device)
    grid[batch["cand_dec"], batch["cand_pos"]] = torch.arange(n_cand, device=grid.device)
    return grid


class CandidateAttention(nn.Module):
    """Each candidate attends to its turn's entity tokens (after the
    blocks), from its tile: keys and values gain an embedding of the hex
    distance bucket between the candidate's tile and the entity's and the
    combat pair features of the decision's actor and the entity; scores a
    per-head bias by that bucket; a learned null key and value. Run per
    decision over its candidates as one padded grid [D, K] × [D, E]."""

    def __init__(self, cfg: Config, n_pair: int):
        super().__init__()
        d = cfg.d_model
        self.heads = h = cfg.n_attn_heads
        dh = d // h
        self.norm = RMSNorm(d)
        self.q = nn.Linear(d, d)
        self.kv = nn.Linear(d, 2 * d)
        self.pair_kv = nn.Linear(n_pair, 2 * d) if n_pair else None
        self.qn = RMSNorm(dh)
        self.kn = RMSNorm(dh)
        self.dist_k = nn.Parameter(torch.zeros(h, N_BUCKETS, dh))
        self.dist_v = nn.Parameter(torch.zeros(h, N_BUCKETS, dh))
        self.dist_bias = nn.Parameter(torch.zeros(h, N_BUCKETS))
        self.null_k = nn.Parameter(torch.zeros(h, dh))
        self.null_v = nn.Parameter(torch.zeros(h, dh))
        self.out = nn.Linear(d, d)

    def forward(self, x: torch.Tensor, grid: torch.Tensor, dec_b: torch.Tensor, ent: torch.Tensor, ent_ok: torch.Tensor,
                bucket: torch.Tensor, pair: torch.Tensor | None) -> torch.Tensor:
        """x [N, d] candidates; grid [D, K]; ent [B, E, d]; ent_ok [B, E];
        bucket [D, K, E]; pair [D, E, n_pair] or None → [N, d]."""
        n, d = x.shape
        dd, kk = grid.shape
        h = self.heads
        dh = d // h
        xs = torch.cat([x, x.new_zeros(1, d)]).index_select(0, grid.reshape(-1)).view(dd, kk, d)
        q = self.qn(self.q(self.norm(xs)).view(dd, kk, h, dh)).transpose(1, 2)  # [D, H, K, dh]
        kv = self.kv(ent).index_select(0, dec_b)  # [D, E, 2d]
        if self.pair_kv is not None and pair is not None:
            kv = kv + self.pair_kv(pair)
        k, v = kv.split(d, dim=-1)
        e = k.shape[1]
        k = self.kn(k.view(dd, e, h, dh)).transpose(1, 2)  # [D, H, E, dh]
        v = v.view(dd, e, h, dh).transpose(1, 2)
        scale = 1.0 / math.sqrt(dh)
        bk = bucket[:, None].expand(dd, h, kk, e)
        s = (q @ k.transpose(-1, -2)) * scale  # [D, H, K, E]
        s = s + torch.gather((q @ self.dist_k.transpose(-1, -2)[None]) * scale, 3, bk)  # q · E_k[bucket]
        s = s + self.dist_bias[None, :, None, :].expand(dd, h, kk, N_BUCKETS).gather(3, bk)
        s = s.masked_fill(~ent_ok.index_select(0, dec_b)[:, None, None, :], float("-inf"))
        s_null = (q * self.null_k[None, :, None, :]).sum(-1, keepdim=True) * scale
        p = torch.softmax(torch.cat([s_null, s], dim=-1), dim=-1)
        p_null, p_e = p[..., :1], p[..., 1:]
        o = p_e @ v + p_null * self.null_v[None, :, None, :]
        pb = torch.zeros(dd, h, kk, N_BUCKETS, device=x.device, dtype=p.dtype).scatter_add_(3, bk, p_e)
        o = o + pb @ self.dist_v[None]  # Σ over entities of p · E_v[bucket]
        o = o.transpose(1, 2).reshape(dd * kk, d)
        # Back to candidate rows: each row's (decision, position) in the grid.
        flat = torch.full((n + 1,), dd * kk, dtype=torch.long, device=x.device)
        flat = flat.index_put((grid.reshape(-1),), torch.arange(dd * kk, device=x.device))
        o = torch.cat([o, o.new_zeros(1, d)]).index_select(0, flat[:n])
        return self.out(o)


class Measured:
    """The measurement switches over a net (mixed in before the net's
    class): `drop_move_strike` zeroes the format-10 move-strike columns of
    the candidate rows before any layer reads them (the data stay format
    10, so the arms see the same turns); with `cand_aux` on, `score` puts
    the aux head's targets, read off the candidate rows as `cand_aux.attach`
    reads them, in `batch["cand_aux"]` (the trainer's loss and metrics read
    it there), and `cand_aux_probe` detaches the head's input: a probe that
    measures what the net's own state holds without teaching it."""

    def init_measured(self, layout: Layout):
        names = list(layout.names["candidate"])
        keep = torch.ones(len(names))
        if self.cfg.drop_move_strike:
            for n, _ in FORMAT_10_CANDIDATE:
                if n in names:
                    keep[names.index(n)] = 0.0
        self.register_buffer("cand_keep", keep, persistent=False)
        self.cand_names = names
        self.ent_names = list(layout.names["entity"])
        if self.cfg.cand_aux and self.cfg.cand_aux != cand_aux.N:
            raise ValueError(f"cand_aux must be {cand_aux.N} (cand_aux.TARGETS)")

    def norm(self, x: torch.Tensor, table: str) -> torch.Tensor:
        if table == "candidate" and self.cfg.drop_move_strike:
            x = x * self.cand_keep
        return Net.norm(self, x, table)

    def aux_input(self, hid: torch.Tensor) -> torch.Tensor:
        return hid.detach() if self.cfg.cand_aux_probe else hid

    def put_targets(self, batch: dict):
        """batch["cand_aux"] [N, cand_aux.N] from the candidate rows (NaN
        where a target does not apply; the team targets, which no row holds,
        NaN), unless the batch has them (`cand_aux.attach`)."""
        if not self.cfg.cand_aux or ("cand_aux" in batch and torch.isfinite(batch["cand_aux"]).any()):
            return
        rows = batch["cand_rows"]
        col = {n: i for i, n in enumerate(self.cand_names)}
        if "move_blow_captures" not in col:
            return
        cd = batch["cand_dec"]
        actor = batch["dec_actor"][cd]
        db = batch["dec_b"][cd]
        ents = batch["entities"][db, actor.clamp(min=0)]
        ec = {n: self.ent_names.index(n) for n in ("civilian", "strength", "ranged_strength")}
        fights = (actor >= 0) & (batch["dec_head"][cd] == HEAD_UNIT) & (ents[:, ec["civilian"]] == 0) & ((ents[:, ec["strength"]] > 0) | (ents[:, ec["ranged_strength"]] > 0))
        r = lambda n: rows[:, col[n]]
        move = (r("act_move") > 0) & fights
        attack = ((r("act_melee") + r("act_ranged")) > 0) & (r("preview_defender_hp") > 0)
        where = {"move": move, "attack": attack, "strike": attack | (move & (r("move_strike_targets") > 0))}
        out = rows.new_full((len(rows), cand_aux.N), float("nan"))
        for j, (_, source, _, div, applies) in enumerate(cand_aux.TARGETS):
            if source not in col or not cand_aux.ACTIVE[j]:
                continue
            v = r(source)
            if source == "preview_damage_to_defender":
                v = v * 100.0 / r("preview_defender_hp").clamp(min=1.0)
            out[:, j] = torch.where(where[applies], v / div, float("nan"))
        batch["cand_aux"] = out


class MeasuredNet(Measured, Net):
    """The first net with the measurement switches (small, large arms of
    the v2 comparison)."""

    def __init__(self, cfg: Config, layout: Layout):
        Net.__init__(self, cfg, layout)
        self.init_measured(layout)
        if cfg.cand_aux and cfg.cand_aux_probe:
            self.cand_aux_head = nn.Sequential(Detach(), *self.cand_aux_head)

    def score(self, batch, tile_h, tokens, off):
        self.put_targets(batch)
        return Net.score(self, batch, tile_h, tokens, off)


class Detach(nn.Module):
    def forward(self, x):
        return x.detach()


class NetV2(Measured, Net):
    """The v2 net (module docstring). Shares the first net's interface
    (`embed`, `attend`, `score`, `value`, the memory's GRUs, `per_head`,
    `turn_trace`), so the trainer runs either; `Net.__init__` is not run:
    the layers are v2's, under the same names where they do the same job."""

    def __init__(self, cfg: Config, layout: Layout):
        nn.Module.__init__(self)
        if cfg.engram or cfg.coarse or cfg.privileged_value or getattr(cfg, "intent", 0):
            raise ValueError("v2: engram, coarse, privileged_value and intent are not built")
        if len(cfg.map_dilations) != cfg.n_conv:
            raise ValueError(f"v2: {cfg.n_conv} map blocks, dilations {cfg.map_dilations}")
        self.cfg = cfg
        self.layout_json = layout.to_json()
        self.heads = tuple(layout.heads)
        tables = ("tile", "global", "entity", "candidate") + ((EVENT_TABLE,) if cfg.events else ())
        if cfg.events and not cfg.c_event:
            raise ValueError("events on needs a layout with the event table (format 7)")
        from .common import ACT_BITS, INPUT_SHIFT

        for table in tables:
            mult = torch.as_tensor(layout.input_multipliers(table), dtype=torch.float32)
            self.register_buffer(f"in_{table}", mult / float(1 << (ACT_BITS + INPUT_SHIFT)))
        dt, d = cfg.d_tile, cfg.d_model
        self.id_emb = nn.ModuleDict()
        for table in tables:
            ids = cfg.id_columns(table)
            n = getattr(cfg, f"c_{table}")
            self.register_buffer(f"num_{table}", torch.as_tensor([i for i in range(n) if i not in ids], dtype=torch.long), persistent=False)
            self.register_buffer(f"idc_{table}", torch.as_tensor(ids, dtype=torch.long), persistent=False)
            if ids:
                self.id_emb[table] = nn.Embedding(len(ids) * cfg.id_buckets, cfg.id_dim)
                nn.init.normal_(self.id_emb[table].weight, std=0.5)
        self.tile_embed = nn.Linear(cfg.row_in("tile"), dt)
        self.map_blocks = nn.ModuleList(HexBlock(dt, cfg.map_ratio, k) for k in cfg.map_dilations)
        self.map_norm = RMSNorm(dt)
        self.region = nn.Linear(2 * dt, d)
        self.entity = nn.Linear(cfg.entity_in, d)
        self.glob = nn.Linear(cfg.row_in("global"), d)
        self.blocks = nn.ModuleList(Block(cfg) for _ in range(cfg.n_attn))
        self.final_norm = RMSNorm(d)
        self.tag_emb = nn.Embedding(cfg.n_decision_heads * cfg.tag_buckets, cfg.tag_dim)
        self.param_emb = nn.Embedding(cfg.n_decision_heads * cfg.param_buckets, cfg.param_dim)
        nn.init.normal_(self.tag_emb.weight, std=0.5)
        nn.init.normal_(self.param_emb.weight, std=0.5)
        self.cand = nn.Linear(cfg.candidate_in, d)
        self.head_query = nn.ModuleList(nn.Linear(d, d) for _ in range(cfg.n_decision_heads))
        # The combat module needs the blow previews (format 8) and the
        # entities' fighting columns; a layout without them (the synthetic
        # shards) builds v2 without it.
        self.combat = Combat(layout) if cfg.combat and combat_columns(layout) else None
        self.cand_attn = CandidateAttention(cfg, Combat.OUT + self.combat.width if self.combat is not None else 0)
        # The sum heads (`sum_fields`): focus fire on the candidate's target
        # and the danger at its tile, summed combat-module blows, as four
        # inputs of the candidate; zero at the start.
        if self.combat is not None:
            self.fields_in = nn.Linear(SUM_FIELDS, d)
            nn.init.zeros_(self.fields_in.weight)
        # No FFN per candidate (the design's choice 5): the heads' MLPs read
        # the attention's residual stream after an RMSNorm.
        self.cand_out_norm = RMSNorm(d)
        # The combat preview loss (training): set by `score` when the batch
        # has attack candidates with a preview, else None.
        self.combat_loss = None
        self.head_hidden = nn.ModuleList(nn.Linear(d, cfg.d_head) for _ in range(cfg.n_decision_heads))
        self.head_logit = nn.ModuleList(nn.Linear(cfg.d_head, 1) for _ in range(cfg.n_decision_heads))
        self.value_hidden = nn.Linear(d, cfg.d_head)
        self.value_out = nn.Linear(cfg.d_head, 1)
        self.slot_player = nn.Linear(cfg.mem_player, d)
        self.slot_nation = nn.Linear(cfg.mem_nation, d)
        self.slot_unit = nn.Linear(cfg.mem_unit, d)
        self.gru_player = nn.GRUCell(d, cfg.mem_player)
        self.gru_nation = nn.GRUCell(d, cfg.mem_nation)
        self.gru_unit = nn.GRUCell(d, cfg.mem_unit)
        if cfg.aim:
            self.aim = nn.Linear(cfg.n_aims, d)
        self.aux_binary = list(layout.extra.get("aux", ([], [], []))[2]) if cfg.aux else []
        if cfg.aux and cfg.n_aux:
            self.aux_head = nn.Linear(d, cfg.n_aux)
        if cfg.cand_aux:
            self.cand_aux_head = nn.Sequential(nn.Linear(d, cfg.d_head), nn.ReLU(), nn.Linear(cfg.d_head, cfg.cand_aux))
        self.cand_aux_out = None
        if cfg.trace:
            self.trace = nn.Linear(d, d, bias=False)
            nn.init.zeros_(self.trace.weight)
        if cfg.events:
            self.event = nn.Linear(cfg.event_in, d)
            self.xattn = EventAttention(cfg)
        if cfg.defer:
            from .common import DEFER_BIAS_INIT

            self.defer_bias = nn.Parameter(torch.tensor(DEFER_BIAS_INIT))
            zero_defer_tag(self)
        self.init_measured(layout)
        self.ent_col = {n: i for i, n in enumerate(self.ent_names)}
        self.fields_out = None
        # Recompute each map block in the backward pass (training): the
        # blocks' inner activations are tiles × 2·d_tile × turns, gigabytes
        # for a batch of whole-game turns.
        self.checkpoint_map = True

    def embed(self, batch: dict) -> dict:
        cfg = self.cfg
        tiles = self.norm(batch["tiles"], "tile")
        bsz, n_tiles, _ = tiles.shape
        h = self.tile_embed(tiles)
        nb = batch["neighbors"]
        idx = {k: dilated_index(nb, k) for k in sorted(set(cfg.map_dilations))}
        for blk in self.map_blocks:
            if self.checkpoint_map and torch.is_grad_enabled() and self.training:
                h = checkpoint(blk, h, idx[blk.dilation], use_reentrant=False)
            else:
                h = blk(h, idx[blk.dilation])
        tile_h = self.map_norm(h)
        r = int(batch["n_regions"])
        reg_tok = self.region(self.pool(tile_h, batch["region"], r, batch.get("region_pool")))

        et = batch["ent_tile"]
        has = (et >= 0).to(h.dtype)
        ent_h = torch.gather(tile_h, 1, et.clamp(min=0)[..., None].expand(-1, -1, cfg.d_tile)) * has[..., None]
        kind = F.one_hot(batch["ent_kind"].clamp(min=0, max=cfg.n_kinds), cfg.n_kinds + 1)[..., : cfg.n_kinds]
        kind = kind * ((batch["ent_kind"] >= 0) & (batch["ent_kind"] < cfg.n_kinds))[..., None]
        rel = F.one_hot(batch["ent_rel"].clamp(min=0, max=cfg.n_relations), cfg.n_relations + 1)[..., : cfg.n_relations]
        rel = rel * ((batch["ent_rel"] >= 0) & (batch["ent_rel"] < cfg.n_relations))[..., None]
        ent_in = torch.cat([self.norm(batch["entities"], "entity"), kind.to(h.dtype), rel.to(h.dtype), ent_h, has[..., None]], dim=-1)
        ent_tok = self.entity(ent_in)
        glob_tok = self.glob(self.norm(batch["globals"], "global"))[:, None]
        e_max = ent_tok.shape[1]
        is_unit, is_nation = self.slot_masks(batch)
        ones = torch.ones(bsz, 1, dtype=torch.bool, device=h.device)
        masks = [torch.ones(bsz, r, dtype=torch.bool, device=h.device), batch["ent_mask"], ones, ones]
        if cfg.aim:
            masks.append(ones)
        off = cfg.offsets(r, 0, e_max)
        off["e_max"] = e_max
        # Token positions (axial q, r): the regions' centroids, the
        # entities' tiles (a slot its entity's); none for the rest.
        tq, tr = tile_coords(nb)
        region = batch["region"]
        cnt = torch.zeros(r, device=h.device).index_add_(0, region, torch.ones_like(tq)).clamp(min=1)
        rq = torch.zeros(r, device=h.device).index_add_(0, region, tq) / cnt
        rr = torch.zeros(r, device=h.device).index_add_(0, region, tr) / cnt
        eq, er = tq[et.clamp(min=0)], tr[et.clamp(min=0)]
        e_ok = et >= 0
        z = lambda n: torch.zeros(bsz, n, device=h.device)
        f = lambda n: torch.zeros(bsz, n, dtype=torch.bool, device=h.device)
        n_mid = 2 + (1 if cfg.aim else 0)  # global, player slot, aim
        pos_q = torch.cat([rq.expand(bsz, r), eq, z(n_mid), eq], dim=1)
        pos_r = torch.cat([rr.expand(bsz, r), er, z(n_mid), er], dim=1)
        pos_ok = torch.cat([torch.ones(bsz, r, dtype=torch.bool, device=h.device), e_ok, f(n_mid), e_ok], dim=1)
        return {
            "tile_h": tile_h,
            "prefix": torch.cat([reg_tok, ent_tok, glob_tok], dim=1),
            "ent_tok": ent_tok,
            "aim": self.aim(batch["aim"])[:, None] if cfg.aim else None,
            "is_unit": is_unit,
            "mask": torch.cat(masks + [is_unit | is_nation], dim=1),
            "off": off,
            "pos_q": pos_q,
            "pos_r": pos_r,
            "pos_ok": pos_ok,
        } | (self.event_tokens(batch, reg_tok, ent_tok) if cfg.events else {})

    def attend(self, emb: dict, mem_p: torch.Tensor, mem_n: torch.Tensor, mem_u: torch.Tensor) -> torch.Tensor:
        player_tok = self.slot_player(mem_p)[:, None]
        slot_tok = torch.where(emb["is_unit"][..., None], self.slot_unit(mem_u), self.slot_nation(mem_n)) + emb["ent_tok"]
        parts = [emb["prefix"], player_tok] + ([emb["aim"]] if self.cfg.aim else []) + [slot_tok]
        tokens = torch.cat(parts, dim=1)
        if self.cfg.events:
            tokens = tokens + self.xattn(tokens, emb["ev_tok"], emb["ev_mask"])
        q, r, ok = emb["pos_q"], emb["pos_r"], emb["pos_ok"]
        d = hex_distance(q[:, :, None], r[:, :, None], q[:, None, :], r[:, None, :])
        bucket = distance_bucket(d, ok[:, :, None] & ok[:, None, :])
        mask = emb["mask"]
        for block in self.blocks:
            tokens = block(tokens, mask, bucket)
        return self.final_norm(tokens)

    def score(self, batch: dict, tile_h: torch.Tensor, tokens: torch.Tensor, off: dict) -> torch.Tensor:
        self.put_targets(batch)
        cfg = self.cfg
        ent0 = off["ent"]
        e_max = off["e_max"]
        dec_b, dec_head, actor = batch["dec_b"], batch["dec_head"], batch["dec_actor"]
        uses_actor = ((dec_head == HEAD_UNIT) | (dec_head == HEAD_PRODUCTION)) & (actor >= 0)
        a = actor.clamp(min=0)
        is_unit, is_nation = self.slot_masks(batch)
        has_slot = ((is_unit | is_nation)[dec_b, a] & uses_actor).to(tokens.dtype)
        actor_ctx = tokens[dec_b, ent0 + a] + has_slot[:, None] * tokens[dec_b, off["slots"] + a]
        global_ctx = tokens[dec_b, off["global"]] + tokens[dec_b, off["player"]]
        ctx = torch.where(uses_actor[:, None], actor_ctx, global_ctx)

        cd = batch["cand_dec"]
        cb = dec_b[cd]
        ctile = batch["cand_tile"]
        has_t = (ctile >= 0).to(tokens.dtype)
        th = tile_h[cb, ctile.clamp(min=0)] * has_t[:, None]
        cent = batch["cand_ent"]
        has_e = (cent >= 0).to(tokens.dtype)
        eh = tokens[cb, ent0 + cent.clamp(min=0)] * has_e[:, None]
        inp = torch.cat([self.norm(batch["cand_rows"], "candidate"), self.tag_emb(batch["cand_tag"]).clamp(-CLIP, CLIP),
                         self.param_emb(batch["cand_param"]).clamp(-CLIP, CLIP), th, has_t[:, None], eh, has_e[:, None]], dim=-1)
        enc = self.cand(inp)
        if cfg.trace:
            ctx = ctx + self.turn_trace(batch, enc, tokens.shape[0])
        q = self.per_head(ctx, dec_head, batch.get("dec_groups"), lambda h, x: self.head_query[h](x), cfg.d_model)
        x = enc + q[cd]

        # The candidate's tile: its target tile, else the actor's tile.
        n_cand = len(cd)
        grid = decision_grid(batch, n_cand)
        et = batch["ent_tile"]
        actor_tile = torch.where(actor >= 0, et[dec_b, a], -1)
        pos = torch.where(ctile >= 0, ctile, actor_tile[cd])
        pos = torch.cat([pos, pos.new_full((1,), -1)]).index_select(0, grid.reshape(-1)).view(grid.shape)  # [D, K]
        tq, tr = tile_coords(batch["neighbors"])
        etd = et.index_select(0, dec_b)  # [D, E]
        dist = hex_distance(tq[pos.clamp(min=0)][:, :, None], tr[pos.clamp(min=0)][:, :, None], tq[etd.clamp(min=0)][:, None, :], tr[etd.clamp(min=0)][:, None, :])
        bucket = distance_bucket(dist, (pos >= 0)[:, :, None] & (etd >= 0)[:, None, :])
        pair = self.pair_features(batch, dec_b, actor, et) if self.combat is not None else None
        self.combat_loss = self.preview_loss(batch, pair) if pair is not None and self.training else None
        if self.combat is not None:
            focus, danger = self.sum_fields(batch, dec_b, actor, dist.masked_fill((pos < 0)[:, :, None], 1 << 20))
            danger = danger[cd, batch["cand_pos"]]
            f = focus[cb, cent.clamp(min=0)] * has_e
            self.fields_out = torch.stack([f, danger], dim=-1)
            x = x + self.fields_in(torch.stack([f.clamp(max=4), (f >= 1).to(x.dtype) * has_e, danger.clamp(max=4), (danger >= 1).to(x.dtype)], dim=-1))
        ent = tokens[:, ent0 : ent0 + e_max]
        if self.checkpoint_map and torch.is_grad_enabled() and self.training:
            # Recomputed in the backward pass: the padded [D, H, K, E] grid of
            # a whole-game piece is gigabytes.
            x = x + checkpoint(self.cand_attn, x, grid, dec_b, ent, batch["ent_mask"], bucket, pair, use_reentrant=False)
        else:
            x = x + self.cand_attn(x, grid, dec_b, ent, batch["ent_mask"], bucket, pair)
        hid = self.cand_out_norm(x)
        self.probe_hidden = hid if getattr(self, "keep_probe", False) else None
        if cfg.cand_aux:
            self.cand_aux_out = self.cand_aux_head(self.aux_input(hid))
        logit = lambda h, y: self.head_logit[h](F.relu(self.head_hidden[h](y)))
        out = self.per_head(hid, dec_head[cd], batch.get("cand_groups"), logit, 1).squeeze(-1)
        if cfg.defer:
            out = out + batch["cand_defer"].to(out.dtype) * self.defer_bias
        return out

    def preview_loss(self, batch: dict, pair: torch.Tensor) -> torch.Tensor:
        """The combat module's loss on the batch's attack candidates (the
        design's choice 2: fine-tuned with the net, its preview loss kept):
        Σ over attack candidates with a preview of the mean over the four
        outputs (squared error of the shares, BCE of kill and capture), each
        divided by its decision's candidate count."""
        cd, cent = batch["cand_dec"], batch["cand_ent"]
        dec_b, actor = batch["dec_b"], batch["dec_actor"]
        tgt = combat_targets(batch["cand_rows"], self.cand_names, batch["entities"][dec_b[cd], actor[cd].clamp(min=0), self.ent_names.index("hp")])
        ok = torch.isfinite(tgt[:, 0]) & (cent >= 0) & (actor[cd] >= 0)
        pred = pair[cd, cent.clamp(min=0), : Combat.OUT]
        return combat_loss(pred, tgt, ok, batch["dec_k"][cd])

    def sum_fields(self, batch: dict, dec_b, actor, dist):
        """The two sum heads (an aggregation by sum, not attention's average):
        `focus` [B, E] — per entity, the summed share of its hit points the
        own units that can still strike this turn deal it (the combat
        module's blow of each, gated by hex distance ≤ its movement + range),
        ≥ 1 where the team can kill or take it; `danger` [D, K] — per
        decision candidate tile, the summed share of the acting unit's hit
        points the hostile units within their movement + range of that tile
        deal it next turn. No pathfinding: reach is a hex distance."""
        ents, et = batch["entities"], batch["ent_tile"]
        c = self.ent_col
        col = lambda n: ents[..., c[n]]
        ok = batch["ent_mask"] & (et >= 0)
        fights = ((col("strength") > 0) | (col("ranged_strength") > 0)) & (col("civilian") == 0) & ok
        ready = fights & (col("kind_own_unit") > 0) & (col("moves_left") > 0) & (col("can_attack") > 0)
        hostile = fights & (col("kind_foreign_unit") > 0) & ((col("relation_war") > 0) | (col("relation_barbarian") > 0))
        reach = col("movement").float() + col("range").float().clamp(min=1)
        side = self.side_rows
        h = self.combat.att(side)[:, :, None] + self.combat.dfd(side)[:, None, :]  # [B, attacker, defender, hidden]
        dealt = self.combat.run(h)[0][..., 0].clamp(0, 1)
        tq, tr = tile_coords(batch["neighbors"])
        eq, er = tq[et.clamp(min=0)], tr[et.clamp(min=0)]
        dee = hex_distance(eq[:, :, None], er[:, :, None], eq[:, None, :], er[:, None, :])
        gate = ready[:, :, None] & ok[:, None, :] & (dee.float() <= reach[:, :, None])
        focus = (dealt * gate).sum(1)
        a = actor.clamp(min=0)
        on_actor = dealt.index_select(0, dec_b)[torch.arange(len(dec_b), device=a.device), :, a]  # [D, E]: entity → the actor
        gd = (hostile.index_select(0, dec_b) & (actor >= 0)[:, None])[:, None, :] & (dist.float() <= reach.index_select(0, dec_b)[:, None, :])
        danger = (gd * on_actor[:, None, :]).sum(-1)
        return focus, danger

    def pair_features(self, batch: dict, dec_b, actor, et) -> torch.Tensor:
        """[D, E, 4 + width]: the combat module on (the decision's actor,
        each entity of its turn); zero without an actor or for padding."""
        ents, tiles = batch["entities"], batch["tiles"]
        bsz, e_max = et.shape
        tile_of = torch.gather(tiles, 1, et.clamp(min=0)[..., None].expand(-1, -1, tiles.shape[-1]))  # [B, E, C_tile]
        side = self.combat.side(ents, tile_of)  # [B, E, F]
        self.side_rows = side
        a = actor.clamp(min=0)
        att = side[dec_b, a]  # [D, F]
        out, hid = self.combat.pair(att, side.index_select(0, dec_b))
        ok = (actor >= 0)[:, None] & batch["ent_mask"].index_select(0, dec_b)
        return torch.cat([out, hid], dim=-1) * ok[..., None].to(out.dtype)


def build_net(cfg: Config, layout: Layout) -> Net:
    """The net of a config: v2 (`arch` 2), the first net with the
    measurement switches, or the first net."""
    if cfg.arch == 2:
        return NetV2(cfg, layout)
    if cfg.drop_move_strike or cfg.cand_aux_probe:
        return MeasuredNet(cfg, layout)
    return Net(cfg, layout)
