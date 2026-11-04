"""The float model of the end-to-end net (classic/docs/ai-neural-architecture.md,
classic/docs/ai-net-model.md).

Every operation here has an integer twin in quantize.py (NumPy reference) and
in crates/classic-rules/src/ai/net_e2e/model.rs:

- inputs: stored i16 × the layout's multiplier (Q8), clipped to ±128;
- tile embedding: Linear + ReLU;
- hex convolution × n_conv: self + 6 neighbours (an off-map neighbour reads
  the layer's learned edge row), Linear + ReLU, added to the input
  (residual), so tile states stay ≥ 0;
- regions: mean and max of the tile states per region, Linear;
- entity tokens: Linear on [row, kind one-hot, relation one-hot, tile state,
  has tile]; global token: Linear on the global row;
- attention blocks × n_attn (pre-LayerNorm without affine parameters; the
  following Linear learns the affine part): self-attention over all tokens,
  then a ReLU feed-forward, both residual; a final LayerNorm;
- memory (classic/docs/ai-neural-architecture.md, Memory): a player slot,
  a nation slot per nation or city-state, a unit slot per own unit; each slot
  is a token: Linear(slot) (plus its entity's token for nation and unit
  slots); at the end of the turn a GRU per slot kind updates the slot from its
  token's output;
- heads: per candidate, Linear on [row, tag embedding, parameter embedding,
  target tile state, has tile, target entity token, has entity] plus the
  head's query of the context (the actor's token plus its unit slot's token
  for unit commands and production, the global token plus the player slot's
  token for research and policy), ReLU, Linear + ReLU, Linear → one logit;
- value: on the global token, Linear + ReLU, Linear;
- events (format 7, `events` on): each event of the player's last rounds is
  a token (Linear on [its row, the region token of its tile, has a tile, its
  actor's entity token, has an actor]); before the attention blocks every
  token attends over the event tokens (pre-LayerNorm on both sides, the
  self-attention's heads) and adds the result through `xattn_out`, which
  starts at zero (a net warm-started without events computes what it did).

Token order: regions, entities, global, player slot, then one slot token per
entity position (masked unless the entity has a slot).
"""

from __future__ import annotations

import math
from dataclasses import fields

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F

from .common import ACT_BITS, DEFER_BIAS_INIT, DEFER_TAG, WAIT_TAG, EVENT_TABLE, INPUT_SHIFT, LN_EPS, NATION_SLOT_KINDS, UNIT_SLOT_KINDS, Config, HEAD_PRODUCTION, HEAD_UNIT, tag_index
from .data import Layout

CLIP = float(1 << (15 - ACT_BITS))  # ±128: the i16 Q8 range


class Block(nn.Module):
    def __init__(self, cfg: Config):
        super().__init__()
        d = cfg.d_model
        self.heads = cfg.n_attn_heads
        self.qkv = nn.Linear(d, 3 * d)
        self.out = nn.Linear(d, d)
        self.ff1 = nn.Linear(d, cfg.d_ff)
        self.ff2 = nn.Linear(cfg.d_ff, d)

    def forward(self, x: torch.Tensor, mask: torch.Tensor) -> torch.Tensor:
        b, n, d = x.shape
        h = self.heads
        dh = d // h
        a = F.layer_norm(x, (d,), eps=LN_EPS)
        q, k, v = self.qkv(a).split(d, dim=-1)
        q = q.view(b, n, h, dh).transpose(1, 2)
        k = k.view(b, n, h, dh).transpose(1, 2)
        v = v.view(b, n, h, dh).transpose(1, 2)
        # softmax(q kᵀ / √dh, masked keys −∞) v, as one fused op.
        o = F.scaled_dot_product_attention(q, k, v, attn_mask=mask[:, None, None, :], scale=1.0 / math.sqrt(dh))
        x = x + self.out(o.transpose(1, 2).reshape(b, n, d))
        a = F.layer_norm(x, (d,), eps=LN_EPS)
        return x + self.ff2(F.relu(self.ff1(a)))


class EventAttention(nn.Module):
    """Every token over the turn's event tokens (docs/ai-net-model.md,
    Events): q = Wq·LN(x), k, v = Wkv·LN(e), softmax(q kᵀ / √dh) v over the
    real events per head, then Wout; a turn without events adds nothing.
    Wout starts at zero."""

    def __init__(self, cfg: Config):
        super().__init__()
        d = cfg.d_model
        self.heads = cfg.n_attn_heads
        self.q = nn.Linear(d, d)
        self.kv = nn.Linear(d, 2 * d)
        self.out = nn.Linear(d, d)
        nn.init.zeros_(self.out.weight)
        nn.init.zeros_(self.out.bias)

    def forward(self, x: torch.Tensor, ev: torch.Tensor, mask: torch.Tensor) -> torch.Tensor:
        b, n, d = x.shape
        v_n = ev.shape[1]
        h = self.heads
        dh = d // h
        q = self.q(F.layer_norm(x, (d,), eps=LN_EPS)).view(b, n, h, dh).transpose(1, 2)
        k, v = self.kv(F.layer_norm(ev, (d,), eps=LN_EPS)).split(d, dim=-1)
        k = k.view(b, v_n, h, dh).transpose(1, 2)
        v = v.view(b, v_n, h, dh).transpose(1, 2)
        s = (q @ k.transpose(-1, -2)) * (1.0 / math.sqrt(dh))
        # Padding events: a large finite negative (a turn without events then
        # weighs its padding evenly, and `has` zeroes its output: no NaN).
        s = s.masked_fill(~mask[:, None, None, :], -1e9)
        o = (torch.softmax(s, dim=-1) @ v).transpose(1, 2).reshape(b, n, d)
        has = mask.any(dim=1).to(x.dtype)[:, None, None]
        return self.out(o) * has


class Net(nn.Module):
    def __init__(self, cfg: Config, layout: Layout):
        super().__init__()
        self.cfg = cfg
        self.layout_json = layout.to_json()
        self.heads = tuple(layout.heads)
        tables = ("tile", "global", "entity", "candidate") + ((EVENT_TABLE,) if cfg.events else ())
        if cfg.events and not cfg.c_event:
            raise ValueError("events on needs a layout with the event table (format 7)")
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
        self.edges = nn.Parameter(torch.zeros(cfg.n_conv, dt))
        self.convs = nn.ModuleList(nn.Linear(7 * dt, dt) for _ in range(cfg.n_conv))
        self.region = nn.Linear(2 * dt, d)
        self.entity = nn.Linear(cfg.entity_in, d)
        self.glob = nn.Linear(cfg.row_in("global"), d)
        self.blocks = nn.ModuleList(Block(cfg) for _ in range(cfg.n_attn))
        self.tag_emb = nn.Embedding(cfg.n_decision_heads * cfg.tag_buckets, cfg.tag_dim)
        self.param_emb = nn.Embedding(cfg.n_decision_heads * cfg.param_buckets, cfg.param_dim)
        nn.init.normal_(self.tag_emb.weight, std=0.5)
        nn.init.normal_(self.param_emb.weight, std=0.5)
        self.cand = nn.Linear(cfg.candidate_in, d)
        self.head_query = nn.ModuleList(nn.Linear(d, d) for _ in range(cfg.n_decision_heads))
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
        if cfg.engram:
            self.engram = nn.ModuleDict({site: nn.Embedding(cfg.engram_buckets, cfg.engram_dim) for site in ("tile", "entity", "candidate")})
            for emb in self.engram.values():
                nn.init.normal_(emb.weight, std=0.5)
        if cfg.coarse:
            self.coarse = nn.Linear(2 * dt, d)
        if cfg.aim:
            self.aim = nn.Linear(cfg.n_aims, d)
        self.aux_binary = list(layout.extra.get("aux", ([], [], []))[2]) if cfg.aux else []
        if cfg.aux and cfg.n_aux:
            self.aux_head = nn.Linear(d, cfg.n_aux)
        if cfg.cand_aux:
            # Training only (`cand_aux.py`): what the rules' previews say of
            # each candidate, from the hidden state the head's logit reads.
            self.cand_aux_head = nn.Sequential(nn.Linear(d, cfg.d_head), nn.ReLU(), nn.Linear(cfg.d_head, cfg.cand_aux))
        self.cand_aux_out = None
        if cfg.privileged_value and cfg.n_truth:
            self.truth_in = nn.Linear(cfg.n_truth, cfg.d_head)
        if cfg.trace:
            # W_tr starts at zero: a net warm-started from a checkpoint
            # without the trace computes exactly what it did.
            self.trace = nn.Linear(d, d, bias=False)
            nn.init.zeros_(self.trace.weight)
        if cfg.events:
            self.event = nn.Linear(cfg.event_in, d)
            self.xattn = EventAttention(cfg)
        if cfg.defer:
            # The defer candidate's logit adds this (logits; Q8 in the file).
            self.defer_bias = nn.Parameter(torch.tensor(DEFER_BIAS_INIT))
            zero_defer_tag(self)

    def norm(self, x: torch.Tensor, table: str) -> torch.Tensor:
        """A table's rows as layer input: the numeric columns scaled and
        clipped, then the id columns' embeddings (classic/docs/ai-net-model.md)."""
        num = (x[..., getattr(self, f"num_{table}")] * getattr(self, f"in_{table}")[getattr(self, f"num_{table}")]).clamp(-CLIP, CLIP)
        ids = self.cfg.id_columns(table)
        if not ids:
            return num
        stored = x[..., getattr(self, f"idc_{table}")].long()
        buckets = self.cfg.id_buckets
        offs = torch.arange(len(ids), device=x.device) * buckets
        rows = offs + torch.where(stored <= 0, 0, 1 + torch.remainder(stored - 1, buckets - 1))
        emb = self.id_emb[table](rows).clamp(-CLIP, CLIP)
        return torch.cat([num, emb.flatten(-2)], dim=-1)

    def with_engram(self, x: torch.Tensor, batch: dict, site: str, key: str) -> torch.Tensor:
        """Appends the engram vector (engram on) to a table's layer input."""
        if not self.cfg.engram:
            return x
        return torch.cat([x, self.engram[site](batch[key]).clamp(-CLIP, CLIP)], dim=-1)

    @staticmethod
    def pool(h: torch.Tensor, groups: torch.Tensor, n: int, layout=None):
        """Mean and max of the tile states per group ([B, n, 2 × d_tile]).
        The max is over the group's tiles and a zero (the tile states are
        ≥ 0). `layout` (data.pool_layout, from collate) holds the tiles per
        group and the tiles of each group as rows padded to the largest
        group, so the max is a dense reduction; without it, a scatter."""
        bsz, n_tiles, dt = h.shape
        sums = torch.zeros(bsz, n, dt, device=h.device, dtype=h.dtype).index_add_(1, groups, h)
        if layout is None:
            cnt = torch.bincount(groups, minlength=n).clamp(min=1).to(h.dtype)
            mx = torch.zeros(bsz, n, dt, device=h.device, dtype=h.dtype).scatter_reduce(
                1, groups[None, :, None].expand(bsz, n_tiles, dt), h, reduce="amax", include_self=True
            )
        else:
            # Row n_tiles is the zero every group includes, n_tiles + 1 the padding (−∞).
            cnt, slots = layout
            cnt = cnt.to(h.dtype)
            ext = torch.cat([h, h.new_zeros(bsz, 1, dt), h.new_full((bsz, 1, dt), float("-inf"))], dim=1)
            mx = ext.index_select(1, slots.reshape(-1)).view(bsz, n, slots.shape[1], dt).amax(dim=2)
        return torch.cat([sums / cnt[None, :, None], mx], dim=-1)

    def slot_masks(self, batch: dict):
        kind = batch["ent_kind"]
        is_unit = torch.zeros_like(batch["ent_mask"])
        is_nation = torch.zeros_like(batch["ent_mask"])
        for k in UNIT_SLOT_KINDS:
            is_unit = is_unit | (kind == k)
        for k in NATION_SLOT_KINDS:
            is_nation = is_nation | (kind == k)
        return is_unit & batch["ent_mask"], is_nation & batch["ent_mask"]

    def encode(self, batch: dict):
        """Tile states [B, T, d_tile], tokens after the final LayerNorm
        (Config.offsets: regions, coarse regions, entities, global, player
        slot, aim, entity slots) and the token offsets."""
        cfg = self.cfg
        emb = self.embed(batch)
        bsz, e_max = emb["ent_tok"].shape[:2]
        z = lambda *shape: torch.zeros(*shape, device=emb["ent_tok"].device, dtype=emb["ent_tok"].dtype)
        mem_p = batch.get("mem_player")
        mem_n = batch.get("mem_nation")
        mem_u = batch.get("mem_unit")
        tokens = self.attend(
            emb,
            z(bsz, cfg.mem_player) if mem_p is None else mem_p,
            z(bsz, e_max, cfg.mem_nation) if mem_n is None else mem_n,
            z(bsz, e_max, cfg.mem_unit) if mem_u is None else mem_u,
        )
        return emb["tile_h"], tokens, emb["off"]

    def embed(self, batch: dict) -> dict:
        """The part of the forward that does not read the memory, for every
        turn of the batch at once: the tile states, the token prefix
        (regions, coarse regions, entities, global), the entity tokens, the
        aim token, the slot kinds, the attention mask and the offsets.
        `attend` finishes the turn from its memory; `rows` cuts out turns."""
        cfg = self.cfg
        tiles = self.with_engram(self.norm(batch["tiles"], "tile"), batch, "tile", "tile_engram")
        bsz, n_tiles, _ = tiles.shape
        h = F.relu(self.tile_embed(tiles))
        nb = batch["neighbors"]
        idx = torch.cat([torch.arange(n_tiles, device=nb.device)[:, None], torch.where(nb < 0, n_tiles, nb)], dim=1).reshape(-1)
        for layer, conv in enumerate(self.convs):
            ext = torch.cat([h, self.edges[layer].expand(bsz, 1, cfg.d_tile)], dim=1)
            g = ext.index_select(1, idx).reshape(bsz, n_tiles, 7 * cfg.d_tile)  # self, then the 6 neighbours
            h = h + F.relu(conv(g))
        tile_h = h
        r = int(batch["n_regions"])
        reg_tok = self.region(self.pool(h, batch["region"], r, batch.get("region_pool")))
        rc = int(batch["n_coarse"]) if cfg.coarse else 0
        if cfg.coarse:
            reg_tok = torch.cat([reg_tok, self.coarse(self.pool(h, batch["coarse"], rc, batch.get("coarse_pool")))], dim=1)

        et = batch["ent_tile"]
        has = (et >= 0).to(h.dtype)
        ent_h = torch.gather(tile_h, 1, et.clamp(min=0)[..., None].expand(-1, -1, cfg.d_tile)) * has[..., None]
        kind = F.one_hot(batch["ent_kind"].clamp(min=0, max=cfg.n_kinds), cfg.n_kinds + 1)[..., : cfg.n_kinds]
        kind = kind * ((batch["ent_kind"] >= 0) & (batch["ent_kind"] < cfg.n_kinds))[..., None]
        rel = F.one_hot(batch["ent_rel"].clamp(min=0, max=cfg.n_relations), cfg.n_relations + 1)[..., : cfg.n_relations]
        rel = rel * ((batch["ent_rel"] >= 0) & (batch["ent_rel"] < cfg.n_relations))[..., None]
        ent_in = torch.cat([self.with_engram(self.norm(batch["entities"], "entity"), batch, "entity", "ent_engram"), kind.to(h.dtype), rel.to(h.dtype), ent_h, has[..., None]], dim=-1)
        ent_tok = self.entity(ent_in)
        glob_tok = self.glob(self.norm(batch["globals"], "global"))[:, None]
        e_max = ent_tok.shape[1]
        is_unit, is_nation = self.slot_masks(batch)
        ones = torch.ones(bsz, 1, dtype=torch.bool, device=h.device)
        masks = [torch.ones(bsz, r + rc, dtype=torch.bool, device=h.device), batch["ent_mask"], ones, ones]  # …, global, player slot
        if cfg.aim:
            masks.append(ones)
        off = cfg.offsets(r, rc, e_max)
        off["e_max"] = e_max
        return {
            "tile_h": tile_h,
            "prefix": torch.cat([reg_tok, ent_tok, glob_tok], dim=1),
            "ent_tok": ent_tok,
            "aim": self.aim(batch["aim"])[:, None] if cfg.aim else None,
            "is_unit": is_unit,
            "mask": torch.cat(masks + [is_unit | is_nation], dim=1),
            "off": off,
        } | (self.event_tokens(batch, reg_tok[:, :r], ent_tok) if cfg.events else {})

    def event_tokens(self, batch: dict, reg_tok: torch.Tensor, ent_tok: torch.Tensor) -> dict:
        """The event tokens [B, V, d_model] (Linear on [the event row, the
        region token of its tile, has a tile, its actor's entity token, has
        an actor]; tokens before any attention) and their mask [B, V]."""
        d = self.cfg.d_model
        rg, act = batch["ev_region"], batch["ev_actor"]
        has_r = (rg >= 0).to(reg_tok.dtype)
        has_a = (act >= 0).to(ent_tok.dtype)
        reg = torch.gather(reg_tok, 1, rg.clamp(min=0)[..., None].expand(-1, -1, d)) * has_r[..., None]
        ent = torch.gather(ent_tok, 1, act.clamp(min=0, max=ent_tok.shape[1] - 1)[..., None].expand(-1, -1, d)) * has_a[..., None]
        inp = torch.cat([self.norm(batch["ev_rows"], EVENT_TABLE), reg, has_r[..., None], ent, has_a[..., None]], dim=-1)
        return {"ev_tok": self.event(inp), "ev_mask": batch["ev_mask"]}

    @staticmethod
    def rows(emb: dict, start: int, stop: int) -> dict:
        """The turns start … stop − 1 of an `embed` result (views)."""
        return {k: (v[start:stop] if torch.is_tensor(v) else v) for k, v in emb.items()}

    def attend(self, emb: dict, mem_p: torch.Tensor, mem_n: torch.Tensor, mem_u: torch.Tensor) -> torch.Tensor:
        """The tokens after the final LayerNorm from an `embed` result and the
        memory: player [B, mem_player], nation [B, E, mem_nation], unit
        [B, E, mem_unit]."""
        player_tok = self.slot_player(mem_p)[:, None]
        slot_tok = torch.where(emb["is_unit"][..., None], self.slot_unit(mem_u), self.slot_nation(mem_n)) + emb["ent_tok"]
        parts = [emb["prefix"], player_tok] + ([emb["aim"]] if self.cfg.aim else []) + [slot_tok]
        tokens = torch.cat(parts, dim=1)
        if self.cfg.events:
            tokens = tokens + self.xattn(tokens, emb["ev_tok"], emb["ev_mask"])
        mask = emb["mask"]
        for block in self.blocks:
            tokens = block(tokens, mask)
        return F.layer_norm(tokens, (self.cfg.d_model,), eps=LN_EPS)

    def value(self, batch: dict, tokens: torch.Tensor, off: dict) -> torch.Tensor:
        hidden = self.value_hidden(tokens[:, off["global"]])
        if self.cfg.privileged_value and self.cfg.n_truth:
            hidden = hidden + self.truth_in(batch["truth"])
        return self.value_out(F.relu(hidden)).squeeze(-1)

    def aux(self, tokens: torch.Tensor, off: dict) -> torch.Tensor | None:
        """Auxiliary future targets from the global token (training only)."""
        if not (self.cfg.aux and self.cfg.n_aux):
            return None
        return self.aux_head(tokens[:, off["global"]])

    def update_memory(self, batch: dict, tokens: torch.Tensor, off: dict):
        """The slots after the turn: player [B, mem_player], nation
        [B, E, mem_nation], unit [B, E, mem_unit] (meaningful where the entity
        has that slot)."""
        cfg = self.cfg
        bsz = tokens.shape[0]
        e_max = off["e_max"]
        z = lambda *shape: torch.zeros(*shape, device=tokens.device, dtype=tokens.dtype)
        mem_p = batch.get("mem_player", None)
        mem_p = z(bsz, cfg.mem_player) if mem_p is None else mem_p
        mem_n = batch.get("mem_nation", None)
        mem_n = z(bsz, e_max, cfg.mem_nation) if mem_n is None else mem_n
        mem_u = batch.get("mem_unit", None)
        mem_u = z(bsz, e_max, cfg.mem_unit) if mem_u is None else mem_u
        slots = tokens[:, off["slots"] :]
        new_p = self.gru_player(tokens[:, off["player"]], mem_p)
        new_n = self.gru_nation(slots.reshape(-1, cfg.d_model), mem_n.reshape(-1, cfg.mem_nation)).view(bsz, e_max, cfg.mem_nation)
        new_u = self.gru_unit(slots.reshape(-1, cfg.d_model), mem_u.reshape(-1, cfg.mem_unit)).view(bsz, e_max, cfg.mem_unit)
        return new_p, new_n, new_u

    def score(self, batch: dict, tile_h: torch.Tensor, tokens: torch.Tensor, off: dict) -> torch.Tensor:
        """A logit per candidate row of the batch ([N])."""
        cfg = self.cfg
        ent0 = off["ent"]
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
        inp = torch.cat(
            [
                self.with_engram(self.norm(batch["cand_rows"], "candidate"), batch, "candidate", "cand_engram"),
                self.tag_emb(batch["cand_tag"]).clamp(-CLIP, CLIP),
                self.param_emb(batch["cand_param"]).clamp(-CLIP, CLIP),
                th,
                has_t[:, None],
                eh,
                has_e[:, None],
            ],
            dim=-1,
        )
        enc = self.cand(inp)
        if cfg.trace:
            ctx = ctx + self.turn_trace(batch, enc, tokens.shape[0])
        q = self.per_head(ctx, dec_head, batch.get("dec_groups"), lambda h, x: self.head_query[h](x), cfg.d_model)
        hid = F.relu(enc + q[cd])
        if cfg.cand_aux:
            self.cand_aux_out = self.cand_aux_head(hid)
        logit = lambda h, x: self.head_logit[h](F.relu(self.head_hidden[h](x)))
        out = self.per_head(hid, dec_head[cd], batch.get("cand_groups"), logit, 1).squeeze(-1)
        if cfg.defer:
            # The defer candidates of the unit head (data.collate's cand_defer).
            out = out + batch["cand_defer"].to(out.dtype) * self.defer_bias
        return out

    def turn_trace(self, batch: dict, enc: torch.Tensor, n_turns: int) -> torch.Tensor:
        """The turn trace each decision's context adds ([D, d_model];
        classic/docs/ai-net-model.md, The turn trace): W_tr · clamp(S /
        2^trace_shift, ±128), S the sum of the candidate encodings `enc`
        (clamped to ±128, as the integer net saturates them) of the choices
        made before it in its player-turn by the traced heads (a wait
        excepted). Teacher-forced (the choices are the labels) and
        vectorised: the encodings are put in a table of a row per turn and
        position (data.trace_indices), summed along each turn, read back."""
        d = self.cfg.d_model
        kt = int(batch["trace_kt"])
        u = enc.index_select(0, batch["trace_src"]).clamp(-CLIP, CLIP)
        table = enc.new_zeros(n_turns * kt + 1, d).index_add(0, batch["trace_put"], u)
        sums = table[:-1].view(n_turns, kt, d).cumsum(dim=1).view(n_turns * kt, d)
        s = sums.index_select(0, batch["trace_get"]) * (1.0 / (1 << self.cfg.trace_shift))
        return self.trace(s.clamp(-CLIP, CLIP))

    def per_head(self, x: torch.Tensor, heads: torch.Tensor, groups, fn, width: int) -> torch.Tensor:
        """`fn(head, rows)` applied to the rows of each decision head, zeros
        for rows of no head; `groups` from data.head_groups (computed on the
        host when absent): the rows are gathered head by head once, each head
        runs on a contiguous slice, and no device sync is needed."""
        if groups is None:
            from .data import Uploader, head_groups

            groups = head_groups(heads.cpu().numpy(), self.cfg.n_decision_heads, Uploader(x.device))
        order, spans, inverse = groups
        xs = torch.cat([x, x.new_zeros(1, x.shape[1])]).index_select(0, order)
        outs = [fn(h, xs[start : start + n]) for h, (start, n) in enumerate(spans) if n]
        return torch.cat(outs + [x.new_zeros(1, width)]).index_select(0, inverse)

    def forward(self, batch: dict, with_memory: bool = False, with_aux: bool = False):
        """(logits, value[, memory][, aux])."""
        tile_h, tokens, off = self.encode(batch)
        out = (self.score(batch, tile_h, tokens, off), self.value(batch, tokens, off))
        if with_memory:
            out = out + (self.update_memory(batch, tokens, off),)
        if with_aux:
            out = out + (self.aux(tokens, off),)
        return out


def defer_tag_row(cfg: Config) -> int | None:
    """The tag embedding row of the unit head's defer candidate when no other
    unit tag shares it (the codec tags of unit commands, all below 100 —
    knowledge/actions.rs `order_key`, 0 … 81 —, and wait), else None."""
    row = int(tag_index(HEAD_UNIT, DEFER_TAG, cfg))
    others = {int(tag_index(HEAD_UNIT, t, cfg)) for t in list(range(100)) + [WAIT_TAG]}
    return None if row in others else row


def zero_defer_tag(model: "Net"):
    """The defer candidate's tag row to zero (a new choice, no trained
    meaning yet), where the row is the defer's alone."""
    row = defer_tag_row(model.cfg)
    if row is not None:
        with torch.no_grad():
            model.tag_emb.weight[row].zero_()


class MemoryBank:
    """One player's float memory along a window: the player slot, nation
    slots and unit slots by persistent id."""

    def __init__(self, cfg: Config, device):
        self.cfg = cfg
        self.player = torch.zeros(cfg.mem_player, device=device)
        self.nations: dict[int, torch.Tensor] = {}
        self.units: dict[int, torch.Tensor] = {}

    def detach(self):
        self.player = self.player.detach()
        self.nations = {k: v.detach() for k, v in self.nations.items()}
        self.units = {k: v.detach() for k, v in self.units.items()}


def memory_batch(banks: list[MemoryBank], batch: dict, cfg: Config) -> dict:
    """Adds the banks' slots to a collated batch (`mem_player`, `mem_nation`,
    `mem_unit`), looked up by the entities' persistent ids."""
    ids = batch["ent_id"]
    kinds = batch["ent_kind"].cpu().numpy()
    bsz, e_max = ids.shape
    device = batch["entities"].device
    out = dict(batch)
    out["mem_player"] = torch.stack([b.player for b in banks])
    for name, table, width, kset in (("mem_nation", "nations", cfg.mem_nation, NATION_SLOT_KINDS), ("mem_unit", "units", cfg.mem_unit, UNIT_SLOT_KINDS)):
        bi, ei, vals = [], [], []
        for b, bank in enumerate(banks):
            store = getattr(bank, table)
            for e in range(e_max):
                if kinds[b, e] in kset and ids[b, e] >= 0 and int(ids[b, e]) in store:
                    bi.append(b)
                    ei.append(e)
                    vals.append(store[int(ids[b, e])])
        mem = torch.zeros(bsz, e_max, width, device=device)
        if vals:
            mem = mem.index_put((torch.tensor(bi, device=device), torch.tensor(ei, device=device)), torch.stack(vals))
        out[name] = mem
    return out


def store_memory(banks: list[MemoryBank], batch: dict, new_p, new_n, new_u):
    """Writes the updated slots back: the player slot, the nation slots of the
    turn's nations (others kept), the unit slots of the turn's units (units
    gone are dropped); in entity order, so a repeated id keeps the last."""
    ids = batch["ent_id"]
    kinds = batch["ent_kind"].cpu().numpy()
    mask = batch["ent_mask"].cpu().numpy()
    for b, bank in enumerate(banks):
        bank.player = new_p[b]
        units = {}
        for e in range(ids.shape[1]):
            if not mask[b, e] or ids[b, e] < 0:
                continue
            if kinds[b, e] in NATION_SLOT_KINDS:
                bank.nations[int(ids[b, e])] = new_n[b, e]
            elif kinds[b, e] in UNIT_SLOT_KINDS:
                units[int(ids[b, e])] = new_u[b, e]
        bank.units = units


class WindowMemory:
    """The memory of a batch of windows (lists of `data.Turn`, one player
    each) as device tensors: the training twin of MemoryBank, memory_batch
    and store_memory, with the same reads and writes but no per-entity Python
    tensors and no device sync.

    Per window a player row and two tables of slots keyed by persistent id,
    nation slots and unit slots; row 0 of each window's table stays zero. The
    host knows the whole batch in advance, so the rows each turn reads and
    writes are planned here once:

    - a nation slot is read when the entity has a nation kind, an id ≥ 0 and
      that id was written on an earlier turn of the window;
    - a unit slot is read when the entity has a unit kind, an id ≥ 0 and that
      id was written on the window's previous turn (units gone are dropped);
    - a turn writes the slot of every entity of its kind with an id ≥ 0, a
      repeated id from its last entity."""

    def __init__(self, windows: list, cfg: Config, pad: bool = False):
        from .data import bucket

        self.cfg = cfg
        self.n_windows = len(windows)
        n_ids, u_ids = [], []
        for w in windows:
            n_set, u_set = set(), set()
            for t in w:
                ids, kinds = self._ids(t)
                n_set.update(ids[self._of(kinds, NATION_SLOT_KINDS) & (ids >= 0)].tolist())
                u_set.update(ids[self._of(kinds, UNIT_SLOT_KINDS) & (ids >= 0)].tolist())
            n_ids.append(np.array(sorted(n_set), np.int64))
            u_ids.append(np.array(sorted(u_set), np.int64))
        self.n_stride = 1 + max((len(x) for x in n_ids), default=0)
        self.u_stride = 1 + max((len(x) for x in u_ids), default=0)
        # Table rows: the windows' slots, then (padded) a last row for padding writes.
        self.pad = pad
        size = lambda n: bucket(n + 1) if pad else n + 1
        self.n_rows, self.u_rows = size(self.n_windows * self.n_stride), size(self.n_windows * self.u_stride)
        # plan[(k, i)] = (read nation, read unit, (entity, row) writes nation, unit);
        # rows are global table rows (window × stride + slot).
        self.plan = {}
        for i, w in enumerate(windows):
            n_seen = np.zeros(len(n_ids[i]), bool)
            u_prev = np.zeros(len(u_ids[i]), bool)
            for k, t in enumerate(w):
                ids, kinds = self._ids(t)
                is_n = self._of(kinds, NATION_SLOT_KINDS) & (ids >= 0)
                is_u = self._of(kinds, UNIT_SLOT_KINDS) & (ids >= 0)
                n_slot = np.where(is_n, np.searchsorted(n_ids[i], ids), -1)
                u_slot = np.where(is_u, np.searchsorted(u_ids[i], ids), -1)
                read_n = np.where(is_n & n_seen[np.maximum(n_slot, 0)] if len(n_ids[i]) else False, 1 + n_slot, 0) + i * self.n_stride
                read_u = np.where(is_u & u_prev[np.maximum(u_slot, 0)] if len(u_ids[i]) else False, 1 + u_slot, 0) + i * self.u_stride
                write_n = self._last(np.nonzero(is_n)[0], ids)
                write_u = self._last(np.nonzero(is_u)[0], ids)
                self.plan[(k, i)] = (read_n, read_u, (write_n, 1 + n_slot[write_n] + i * self.n_stride), (write_u, 1 + u_slot[write_u] + i * self.u_stride))
                n_seen[n_slot[write_n]] = True
                u_prev[:] = False
                u_prev[u_slot[write_u]] = True
        self.player = self.nation = self.unit = None

    @staticmethod
    def _ids(turn):
        kinds = np.asarray(turn.entity_meta[:, 0], np.int64)
        ids = np.full(len(kinds), -1, np.int64) if turn.entity_ids is None else np.asarray(turn.entity_ids, np.int64)
        return ids, kinds

    @staticmethod
    def _of(kinds: np.ndarray, kset) -> np.ndarray:
        out = kinds == kset[0]
        for k in kset[1:]:
            out |= kinds == k
        return out

    @staticmethod
    def _last(ents: np.ndarray, ids: np.ndarray) -> np.ndarray:
        """Of entities sharing an id, the last one (in entity order)."""
        if len(ents) == 0:
            return ents
        rev = ents[::-1]
        _, first = np.unique(ids[rev], return_index=True)
        return np.sort(rev[first])

    def segment(self, steps: list[tuple[int, list[int]]], e_max: int, upload):
        """Index tensors for a run of turns, each (k, active windows),
        stacked in that order as one collated batch padded to `e_max`
        entities: the read rows of the nation and unit tables [rows, e_max],
        the writes (source row in a turn's flattened [n × e_max] slots, table
        row) and per turn its span of each. `upload`: a data.Uploader, or
        data.Deferred to build them on the host (data.upload_batch later);
        `views` cuts out the turns. A turn's writes are padded to a
        data.bucket size with writes of row 0 to the tables' last row, which
        is never read (fewer tensor shapes)."""
        from .data import bucket

        n_total = sum(len(act) for _, act in steps)
        read_n = np.zeros((n_total, e_max), np.int64)
        read_u = np.zeros((n_total, e_max), np.int64)
        out, row = [], 0
        wn_src, wn_dst, wu_src, wu_dst, cuts = [], [], [], [], [(0, 0)]
        for k, act in steps:
            start, before = row, (len(wn_dst), len(wu_dst))
            for local, i in enumerate(act):
                rn, ru, (en, dn), (eu, du) = self.plan[(k, i)]
                read_n[row] = i * self.n_stride
                read_u[row] = i * self.u_stride
                read_n[row, : len(rn)] = rn
                read_u[row, : len(ru)] = ru
                wn_src.append(local * e_max + en)
                wn_dst.append(dn)
                wu_src.append(local * e_max + eu)
                wu_dst.append(du)
                row += 1
            for src, dst, trash, b in ((wn_src, wn_dst, self.n_rows - 1, before[0]), (wu_src, wu_dst, self.u_rows - 1, before[1])):
                n = sum(map(len, dst[b:]))  # this turn's writes
                if n and self.pad:
                    src.append(np.zeros(bucket(n) - n, np.int64))
                    dst.append(np.full(bucket(n) - n, trash, np.int64))
            cuts.append((sum(map(len, wn_src)), sum(map(len, wu_src))))
            out.append((k, act, start, row))
        cat = lambda xs: upload(np.concatenate(xs) if xs else np.zeros(0, np.int64))
        turns = []
        for j, (k, act, start, stop) in enumerate(out):
            act_t = None if act == list(range(self.n_windows)) else upload(np.asarray(act, np.int64))
            turns.append({"start": start, "stop": stop, "active": act_t, "n": (cuts[j][0], cuts[j + 1][0]), "u": (cuts[j][1], cuts[j + 1][1])})
        return {"read_n": upload(read_n), "read_u": upload(read_u), "wn_src": cat(wn_src), "wn_dst": cat(wn_dst),
                "wu_src": cat(wu_src), "wu_dst": cat(wu_dst), "turns": turns}

    @staticmethod
    def views(seg: dict) -> list[dict]:
        """Per turn of an uploaded `segment`: its row span, active windows,
        reads and writes (views of the segment's tensors)."""
        out = []
        for t in seg["turns"]:
            (n0, n1), (u0, u1), (a, b) = t["n"], t["u"], (t["start"], t["stop"])
            out.append({"start": a, "stop": b, "active": t["active"], "read_n": seg["read_n"][a:b], "read_u": seg["read_u"][a:b],
                        "wn": (seg["wn_src"][n0:n1], seg["wn_dst"][n0:n1]), "wu": (seg["wu_src"][u0:u1], seg["wu_dst"][u0:u1])})
        return out

    def start(self, device):
        """Zero memory on `device` (the windows' first turn)."""
        z = lambda *shape: torch.zeros(*shape, device=device)
        self.player = z(self.n_windows, self.cfg.mem_player)
        self.nation = z(self.n_rows, self.cfg.mem_nation)
        self.unit = z(self.u_rows, self.cfg.mem_unit)

    def read(self, turn: dict):
        """(player, nation, unit) memory of a turn's windows, as
        memory_batch puts in a batch."""
        n, e_max = turn["read_n"].shape
        mem_p = self.player if turn["active"] is None else self.player.index_select(0, turn["active"])
        mem_n = self.nation.index_select(0, turn["read_n"].reshape(-1)).view(n, e_max, self.cfg.mem_nation)
        mem_u = self.unit.index_select(0, turn["read_u"].reshape(-1)).view(n, e_max, self.cfg.mem_unit)
        return mem_p, mem_n, mem_u

    def write(self, model: "Net", turn: dict, tokens: torch.Tensor, off: dict, mem):
        """The memory update at the end of a turn (Net.update_memory on the
        slots that are kept, store_memory)."""
        cfg = self.cfg
        mem_p, mem_n, mem_u = mem
        new_p = model.gru_player(tokens[:, off["player"]], mem_p)
        self.player = new_p if turn["active"] is None else self.player.index_put((turn["active"],), new_p)
        slots = tokens[:, off["slots"] :].reshape(-1, cfg.d_model)
        src, dst = turn["wn"]
        if len(dst):
            new_n = model.gru_nation(slots.index_select(0, src), mem_n.reshape(-1, cfg.mem_nation).index_select(0, src))
            self.nation = self.nation.index_put((dst,), new_n)
        src, dst = turn["wu"]
        if len(dst):
            new_u = model.gru_unit(slots.index_select(0, src), mem_u.reshape(-1, cfg.mem_unit).index_select(0, src))
            self.unit = self.unit.index_put((dst,), new_u)

    def detach(self):
        self.player, self.nation, self.unit = self.player.detach(), self.nation.detach(), self.unit.detach()


def padded_logits(batch: dict, logits: torch.Tensor) -> torch.Tensor:
    """[D, K_max] with −inf where a decision has fewer candidates (a padded
    batch, collate `pad`: the padding decisions' rows are [0, −∞, …])."""
    if "pad_index" in batch:
        ends = torch.cat([logits.new_full((1,), float("-inf")), logits.new_zeros(1)])
        return torch.cat([logits, ends]).index_select(0, batch["pad_index"].reshape(-1)).view(batch["pad_index"].shape)
    n_dec = len(batch["dec_b"])
    k_max = batch.get("k_max") or (int(batch["dec_k"].max().item()) if n_dec else 1)
    out = torch.full((n_dec, k_max), float("-inf"), device=logits.device, dtype=logits.dtype)
    out[batch["cand_dec"], batch["cand_pos"]] = logits
    return out


def parameter_count(model: nn.Module) -> int:
    return sum(p.numel() for p in model.parameters())


def save_checkpoint(path, model: Net, extra: dict | None = None):
    torch.save({"config": model.cfg.as_dict(), "layout": model.layout_json, "state": model.state_dict(), **(extra or {})}, path)


def load_checkpoint(path, map_location="cpu") -> Net:
    """The net of a checkpoint. Config keys this code does not know (a newer
    tree's training-only heads, e.g. `intent`) are dropped, and with them the
    state of the layers they built; every layer of the net built here must
    still be in the checkpoint."""
    ck = torch.load(path, map_location=map_location, weights_only=False)
    known = {f.name for f in fields(Config)}
    conf = {k: tuple(v) if isinstance(v, list) else v for k, v in ck["config"].items() if k in known}
    from .model_v2 import build_net  # v2 (`arch` 2) and the measurement switches

    model = build_net(Config(**conf), Layout.from_json(ck["layout"]))
    if len(conf) == len(ck["config"]):
        model.load_state_dict(ck["state"])
    else:
        missing, _ = model.load_state_dict(ck["state"], strict=False)
        if missing:
            raise ValueError(f"{path}: the checkpoint lacks {missing}")
    return model
