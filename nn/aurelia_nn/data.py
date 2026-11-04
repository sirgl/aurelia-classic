"""Reading the exporter's shards (classic/docs/ai-net-data.md) into turns and
batches.

A shard is one game: `meta.json` plus `.npy` arrays. Feature counts and
scales come from the layout in `meta.json`, never from constants here. The
tile rows of a turn are assembled by one function (`Shard.turn_tiles`), so a
change of how the exporter stores them (static plus dynamic rows, sampled
turns) stays local to it.
"""

from __future__ import annotations

import hashlib
import json
import math
import os
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

from .common import ACT_BITS, DEFER_TAG, ENGRAM_SMALL_NAMES, EVENT_TABLE, HEAD_UNIT, ID_SUFFIX, INPUT_SHIFT, candidate_engram, coarse_of_tiles, entity_engram, tile_engram, NATION_SLOT_KINDS, PRESETS, TABLES, WAIT_TAG, Config, fnv1a64, param_index, tag_index

_TABLE_ALIASES = {
    "tile": ("tile", "tiles"),
    "global": ("global", "globals"),
    "entity": ("entity", "entities"),
    "candidate": ("candidate", "candidates"),
}


@dataclass
class Layout:
    """Feature names and scales per table (`value = stored / scale`)."""

    names: dict[str, list[str]]
    scales: dict[str, list[float]]
    # Training-only tables (aux, truth): names and scales; not in the hash.
    extra: dict = field(default_factory=dict)
    # The decision heads (meta.json `heads`), in the order of the head column.
    heads: tuple = ("unit", "research", "production", "policy")
    # Named tags (meta.json `tags`): name → tag.
    tags: dict = field(default_factory=dict)

    @staticmethod
    def from_meta(meta: dict) -> "Layout":
        raw = meta["layout"]
        names: dict[str, list[str]] = {}
        scales: dict[str, list[float]] = {}
        for table in TABLES:
            entries = None
            for alias in _TABLE_ALIASES[table]:
                if alias in raw:
                    entries = raw[alias]
                    break
            if entries is None:
                raise ValueError(f"meta.json layout has no table {table!r} (keys: {sorted(raw)})")
            if isinstance(entries, dict) and "features" in entries:
                entries = entries["features"]
            names[table], scales[table] = _entries(entries)
        if EVENT_TABLE in raw:
            # Format 7: the event table (in the hash and the weights file).
            names[EVENT_TABLE], scales[EVENT_TABLE] = _entries(raw[EVENT_TABLE])
        extra = {}
        for table in ("aux", "truth"):
            if table in raw:
                entries = [e if isinstance(e, dict) else {"name": e} for e in raw[table]]
                extra[table] = ([str(e["name"]) for e in entries], [float(e.get("scale", 1)) for e in entries], [bool(e.get("binary", False)) for e in entries])
        heads = tuple(raw.get("_heads") or meta.get("heads") or ("unit", "research", "production", "policy"))
        tags = {str(k): int(v) for k, v in (raw.get("_tags") or meta.get("tags") or {}).items()}
        return Layout(names, scales, extra, heads, tags)

    def count(self, table: str) -> int:
        return len(self.names.get(table, ()))

    def tables(self) -> tuple:
        """The tables of the layout: the four, then the event table when it
        has one (format 7)."""
        return TABLES + ((EVENT_TABLE,) if EVENT_TABLE in self.names else ())

    def id_columns(self, table: str) -> tuple:
        """Columns holding an id (`*_id_plus_1`): embedded, not numbers."""
        return tuple(i for i, n in enumerate(self.names.get(table, ())) if n.endswith(ID_SUFFIX))

    def config(self, preset: str = "small", **overrides) -> Config:
        small = tuple(i for i, n in enumerate(self.names["entity"]) if n in ENGRAM_SMALL_NAMES)
        return Config(
            n_decision_heads=len(self.heads),
            engram_entity_small=small,
            n_aux=len(self.extra.get("aux", ([], []))[0]),
            n_truth=len(self.extra.get("truth", ([], []))[0]),
            **{**PRESETS[preset], **overrides},
            c_tile=self.count("tile"),
            c_global=self.count("global"),
            c_entity=self.count("entity"),
            c_candidate=self.count("candidate"),
            c_event=self.count(EVENT_TABLE),
            id_tile=self.id_columns("tile"),
            id_global=self.id_columns("global"),
            id_entity=self.id_columns("entity"),
            id_candidate=self.id_columns("candidate"),
            id_event=self.id_columns(EVENT_TABLE),
        )

    def canonical(self) -> str:
        """The text the layout hash is taken of: per table `table:` then
        `name/scale;` per feature, a newline after each table; a scale is an
        integer when it is one, else Python's float repr."""
        out = []
        for table in self.tables():
            parts = [f"{table}:"]
            for name, scale in zip(self.names[table], self.scales[table]):
                s = str(int(scale)) if float(scale).is_integer() else repr(float(scale))
                parts.append(f"{name}/{s};")
            out.append("".join(parts) + "\n")
        return "".join(out)

    def hash(self) -> int:
        return fnv1a64(self.canonical().encode("utf-8"))

    def input_multipliers(self, table: str) -> np.ndarray:
        """Per feature, the integer multiplier that brings a stored value to
        Q8: x_q = round_shift(stored × mult, INPUT_SHIFT)."""
        out = []
        for scale in self.scales[table]:
            if not scale > 0:
                raise ValueError(f"layout {table}: a scale must be positive, got {scale}")
            out.append(int(round(2.0 ** (ACT_BITS + INPUT_SHIFT) / scale)))
        return np.asarray(out, dtype=np.int64)

    @staticmethod
    def from_json(j: dict) -> "Layout":
        return Layout.from_meta({"layout": j})

    def to_json(self) -> dict:
        out = {t: [{"name": n, "scale": s} for n, s in zip(self.names[t], self.scales[t])] for t in self.tables()}
        for t, (names, scales, binary) in self.extra.items():
            out[t] = [{"name": n, "scale": s, "binary": b} for n, s, b in zip(names, scales, binary)]
        out["_heads"] = list(self.heads)
        out["_tags"] = dict(self.tags)
        return out


def _entries(entries) -> tuple[list[str], list[float]]:
    """Names and scales of a table's layout entries (`{name, scale}` or a
    bare name, scale 1)."""
    names, scales = [], []
    for e in entries:
        if isinstance(e, str):
            names.append(e)
            scales.append(1.0)
        else:
            names.append(str(e["name"]))
            scales.append(float(e.get("scale", 1)))
    return names, scales


@dataclass
class Decision:
    head: int
    actor: int  # local entity index, -1 none
    rows: np.ndarray  # i16 [k, C_candidate]
    meta: np.ndarray  # i32 [k, 4]: tag, target tile, target entity (local), parameter
    choice: int
    is_wait: bool = False  # a unit decision whose expert choice is "wait"


@dataclass
class Turn:
    """One player-turn with local indices (entities 0..E of this turn)."""

    tiles: np.ndarray  # i16 [T, C_tile]
    neighbors: np.ndarray  # i32 [T, 6]
    region: np.ndarray  # i32 [T]
    n_regions: int
    global_row: np.ndarray  # i16 [C_global]
    entities: np.ndarray  # i16 [E, C_entity]
    entity_meta: np.ndarray  # i32 [E, 3]: kind, tile, relation
    entity_ids: np.ndarray = None  # i32 [E]: persistent id (memory slot key), −1 none
    decisions: list[Decision] = field(default_factory=list)
    coarse: np.ndarray = None  # i32 [T]: the 16×16 block of each tile
    aux: np.ndarray = None  # f32 [n_aux]: future targets (value = stored / scale)
    truth: np.ndarray = None  # f32 [n_truth]: the truth row (training only)
    value_target: float = float("nan")
    map_key: str = ""
    game: str = ""
    # `decisions` as arrays (Shard.turn sets it; collate reads it, building
    # it from `decisions` when absent): per decision head, actor, choice,
    # wait, k (candidate count); the candidates' rows and meta in order.
    dec_arrays: dict = None
    # Format 7: the events of the player's last rounds, i16 [V, C_event],
    # and their meta i32 [V, 4]: kind, tile, actor (local entity, −1 none),
    # parameter. None: a shard without events.
    events: np.ndarray = None
    event_meta: np.ndarray = None


def _load(path: Path, name: str, in_memory: bool = False) -> np.ndarray:
    return np.load(path / name, mmap_mode=None if in_memory else "r")


class Shard:
    """One game's arrays, memory-mapped, or read into memory when
    `in_memory` (a shard materialised into a temporary directory that is
    deleted after loading, see materialize.py)."""

    def __init__(self, path: str | os.PathLike, in_memory: bool = False):
        self.path = Path(path)
        load = lambda name: _load(self.path, name, in_memory)
        with open(self.path / "meta.json") as f:
            self.meta = json.load(f)
        fmt = self.meta.get("format", 1)
        # 1: no ids, no windows; 2: ids and windows.npy; 3: the research
        # lookahead features (layout only); 4: aux.npy and truth.npy; 5: the
        # religion and citizens heads; 6: demand, speed and city religion
        # columns (layout only); 7: the events (events.npy, event_meta.npy,
        # turn_events.npy); 8: the blow columns, and turns.npy's fifth
        # column, the entity rows at the turn's start; 9: a unit decision may
        # end with the defer candidate (tag DEFER_TAG; layout as format 8);
        # 10: the move candidates' strike columns (layout only).
        if fmt not in (1, 2, 3, 4, 5, 6, 7, 8, 9, 10):
            raise ValueError(f"{self.path}: shard format {fmt}, the reader knows 1 … 10")
        self.layout = Layout.from_meta(self.meta)
        self.neighbors = np.ascontiguousarray(load("neighbors.npy"), dtype=np.int32)
        self.region = np.ascontiguousarray(load("region.npy"), dtype=np.int32)
        self.turns = np.asarray(load("turns.npy"), dtype=np.int64)
        self.tiles = load("tiles.npy")
        self.globals = load("globals.npy")
        self.entities = load("entities.npy")
        self.entity_meta = np.asarray(load("entity_meta.npy"), dtype=np.int64)
        self.decisions = np.asarray(load("decisions.npy"), dtype=np.int64)
        self.candidates = load("candidates.npy")
        self.candidate_meta = np.asarray(load("candidate_meta.npy"), dtype=np.int64)
        self.n_tiles = int(self.neighbors.shape[0])
        self.n_regions = int(self.region.max()) + 1 if self.n_tiles else 0
        digest = hashlib.sha1(self.neighbors.tobytes() + self.region.tobytes()).hexdigest()[:12]
        self.map_key = f"{self.n_tiles}-{digest}"
        self.game = self.path.name
        order = np.argsort(self.decisions[:, 0], kind="stable") if len(self.decisions) else np.zeros(0, np.int64)
        self._dec_order = order
        turn_of = self.decisions[order, 0] if len(order) else np.zeros(0, np.int64)
        self._dec_start = np.searchsorted(turn_of, np.arange(len(self.turns) + 1))
        self._shares = self._standings_shares()
        self.wait_tag = int(self.meta.get("tags", {}).get("wait", WAIT_TAG))
        self.windows = self._windows(load)
        spec = self.meta.get("spec", {})
        width = spec.get("width") or spec.get("map_width")
        self.coarse = coarse_of_tiles(int(width), self.n_tiles) if width else None
        has_events = EVENT_TABLE in self.layout.names and (self.path / "turn_events.npy").exists()
        self.turn_events = np.asarray(load("turn_events.npy"), dtype=np.int64).reshape(-1, 2) if has_events else None
        self.events = load("events.npy") if has_events else None
        self.event_meta = np.asarray(load("event_meta.npy"), dtype=np.int64).reshape(-1, 4) if has_events else None
        self.aux = load("aux.npy") if (self.path / "aux.npy").exists() and "aux" in self.layout.extra else None
        self.truth = load("truth.npy") if (self.path / "truth.npy").exists() and "truth" in self.layout.extra else None

    def _windows(self, load) -> list[np.ndarray]:
        """Runs of consecutive turns of one player (turn rows in order):
        `windows.npy` (player, first turn row, length) when the shard has it,
        else built from turns.npy: a player's turns by round, split where a
        round is skipped."""
        if (self.path / "windows.npy").exists():
            w = np.asarray(load("windows.npy"), dtype=np.int64).reshape(-1, 3)
            return [np.arange(first, first + n) for _p, first, n in w if n > 0]
        out = []
        for player in np.unique(self.turns[:, 1]) if len(self.turns) else []:
            rows = np.nonzero(self.turns[:, 1] == player)[0]
            rows = rows[np.argsort(self.turns[rows, 0], kind="stable")]
            rounds = self.turns[rows, 0]
            cuts = np.nonzero(np.diff(rounds) != 1)[0] + 1
            out.extend(r for r in np.split(rows, cuts) if len(r))
        return out

    def entity_ids(self, first: int, count: int, kinds: np.ndarray) -> np.ndarray:
        """Persistent ids of a turn's entities: entity_meta column 3 (format
        2); format 1 has none, so units get −1 (a fresh slot every turn) and a
        nation or city-state its kind and rank among the turn's rows of that
        kind (provisional: holds while the exporter lists them in a stable
        order)."""
        if self.entity_meta.shape[1] >= 4:
            return np.asarray(self.entity_meta[first : first + count, 3], dtype=np.int32)
        ids = np.full(count, -1, np.int32)
        for kind in NATION_SLOT_KINDS:
            sel = np.nonzero(kinds == kind)[0]
            ids[sel] = 1_000_000 + kind * 10_000 + np.arange(len(sel))
        return ids

    def _standings_shares(self) -> dict[int, float]:
        st = self.meta.get("standings")
        out: dict[int, float] = {}
        if isinstance(st, dict):
            st = [dict(v, player=int(k)) if isinstance(v, dict) else {"player": int(k), "points_share": v} for k, v in st.items()]
        for i, row in enumerate(st or []):
            if not isinstance(row, dict):
                continue
            share = row.get("points_share", row.get("share"))
            if share is None:
                continue
            out[int(row.get("player", i))] = float(share)
        return out

    def __len__(self) -> int:
        return len(self.turns)

    def decision_count(self, t: int) -> int:
        return int(self._dec_start[t + 1] - self._dec_start[t])

    def turn_tiles(self, t: int) -> np.ndarray:
        """The tile rows of turn `t` (i16 [tiles, C_tile]). The one place
        that knows how the exporter stores them."""
        n = self.n_tiles
        return np.asarray(self.tiles[t * n : (t + 1) * n], dtype=np.int16)

    def start_count(self, t: int) -> int:
        """The entity rows of turn `t` at its start: the rows the playing
        net reads (it encodes a turn once, at its start; a row the turn
        appended later, a unit or city it named, reaches it as no entity).
        Format 8 stores it (turns.npy column 4). Before it, the start's rows
        end with the nations and city-states (kinds 4, 5) and the appended
        ones are units and cities (kinds 0–3): the rows up to the last
        nation or city-state, else up to the first row whose kind is below
        its predecessor's, else all."""
        first, count = int(self.turns[t, 2]), int(self.turns[t, 3])
        if self.turns.shape[1] >= 5:
            return int(self.turns[t, 4])
        kinds = np.asarray(self.entity_meta[first : first + count, 0])
        nations = np.nonzero(kinds >= 4)[0]
        if len(nations):
            return int(nations[-1]) + 1
        drops = np.nonzero(np.diff(kinds) < 0)[0]
        return int(drops[0]) + 1 if len(drops) else count

    def turn(self, t: int) -> Turn:
        """Turn `t` as the playing net read it: the entity rows of the
        turn's start (`start_count`); a decision's actor or target among
        the rows appended later is −1, as the player's cache has no row
        for it (`model.rs`, `n_entities`)."""
        _round, player, first, _all = (int(v) for v in self.turns[t, :4])
        count = self.start_count(t)
        ents = np.asarray(self.entities[first : first + count], dtype=np.int16)
        emeta = np.array(self.entity_meta[first : first + count, :3], dtype=np.int32)
        # The turn's decisions with a legal choice, their candidates gathered
        # at once; entity references made local (−1 outside the turn).
        dec = self.decisions[self._dec_order[self._dec_start[t] : self._dec_start[t + 1]]]
        _turn, head, actor, cfirst, ccount, choice = dec.T if len(dec) else np.zeros((6, 0), np.int64)
        keep = (ccount > 0) & (choice >= 0) & (choice < ccount)
        head, actor, cfirst, ccount, choice = head[keep], actor[keep], cfirst[keep], ccount[keep], choice[keep]
        starts = np.cumsum(ccount) - ccount
        cand = np.repeat(cfirst - starts, ccount) + np.arange(int(ccount.sum()))
        rows = np.asarray(self.candidates[cand], dtype=np.int16).reshape(len(cand), self.candidates.shape[1])
        meta = np.array(self.candidate_meta[cand], dtype=np.int32).reshape(len(cand), self.candidate_meta.shape[1])
        ent = meta[:, 2].astype(np.int64) - first
        meta[:, 2] = np.where((meta[:, 2] >= 0) & (ent >= 0) & (ent < count), ent, -1)
        local_actor = np.where((actor >= 0) & (actor - first >= 0) & (actor - first < count), actor - first, -1)
        is_wait = (head == 0) & (meta[starts + choice, 0] == self.wait_tag) if len(head) else np.zeros(0, bool)
        arrays = {"head": head, "actor": local_actor, "choice": choice, "wait": is_wait, "k": ccount, "rows": rows, "meta": meta}
        decisions = [
            Decision(int(h), int(a), rows[o : o + k], meta[o : o + k], int(c), bool(w))
            for h, a, o, k, c, w in zip(head, local_actor, starts, ccount, choice, is_wait)
        ]
        events = event_meta = None
        if self.turn_events is not None:
            v0, nv = (int(v) for v in self.turn_events[t])
            events = np.asarray(self.events[v0 : v0 + nv], dtype=np.int16).reshape(nv, self.layout.count(EVENT_TABLE))
            event_meta = np.array(self.event_meta[v0 : v0 + nv], dtype=np.int32).reshape(nv, 4)
            a = event_meta[:, 2].astype(np.int64) - first
            event_meta[:, 2] = np.where((event_meta[:, 2] >= 0) & (a >= 0) & (a < count), a, -1)
        return Turn(
            tiles=self.turn_tiles(t),
            neighbors=self.neighbors,
            region=self.region,
            n_regions=self.n_regions,
            global_row=np.asarray(self.globals[t], dtype=np.int16),
            entities=ents,
            entity_meta=emeta,
            entity_ids=self.entity_ids(first, count, emeta[:, 0]),
            decisions=decisions,
            dec_arrays=arrays,
            coarse=self.coarse,
            aux=None if self.aux is None else np.asarray(self.aux[t], np.float32) / np.asarray(self.layout.extra["aux"][1], np.float32),
            truth=None if self.truth is None else np.asarray(self.truth[t], np.float32) / np.asarray(self.layout.extra["truth"][1], np.float32),
            value_target=self._shares.get(player, float("nan")),
            map_key=self.map_key,
            game=self.game,
            events=events,
            event_meta=event_meta,
        )


def find_shards(root: str | os.PathLike) -> list[Path]:
    root = Path(root)
    if (root / "meta.json").exists():
        return [root]
    return sorted(p.parent for p in root.glob("*/meta.json"))


def load_shards(root: str | os.PathLike, in_memory: bool = False) -> list[Shard]:
    """The shards under `root`, memory-mapped, or read into memory when
    `in_memory` (thousands of small shards — a training ground's scenes —
    would hold a file descriptor per mapped array)."""
    shards = [Shard(p, in_memory=in_memory) for p in find_shards(root)]
    if shards:
        h = shards[0].layout.hash()
        for s in shards[1:]:
            if s.layout.hash() != h:
                raise ValueError(f"{s.path}: its layout differs from {shards[0].path}")
    return shards


def split_by_game(shards: list[Shard], val_fraction: float = 0.15) -> tuple[list[Shard], list[Shard]]:
    """Held-out games by a hash of the shard's name (stable across runs);
    at least one game on each side when there are two or more."""
    def h(s: Shard) -> float:
        return int(hashlib.sha1(s.game.encode()).hexdigest()[:8], 16) / 0xFFFFFFFF

    val = [s for s in shards if h(s) < val_fraction]
    train = [s for s in shards if h(s) >= val_fraction]
    if len(shards) >= 2 and (not val or not train):
        ranked = sorted(shards, key=h)
        n_val = max(1, min(len(shards) - 1, round(len(shards) * val_fraction)))
        val, train = ranked[:n_val], ranked[n_val:]
    return train, val


class WindowIndex:
    """All windows (runs of consecutive turns of one player) of the shards,
    grouped by map, cut into pieces of at most `max_len` turns when given."""

    def __init__(self, shards: list[Shard], max_len: int = 0):
        self.shards = shards
        self.groups: dict[str, list[tuple[int, np.ndarray]]] = {}
        for si, s in enumerate(shards):
            for w in s.windows:
                pieces = [w] if not max_len else [w[i : i + max_len] for i in range(0, len(w), max_len)]
                for piece in pieces:
                    self.groups.setdefault(s.map_key, []).append((si, piece))

    def __len__(self) -> int:
        return sum(len(v) for v in self.groups.values())

    def turns(self) -> int:
        return sum(len(w) for v in self.groups.values() for _, w in v)

    def batches(self, windows_per_batch: int, rng: np.random.Generator | None = None):
        """Lists of windows of one map; a window is (shard, turn rows)."""
        out = []
        for items in self.groups.values():
            items = list(items)
            if rng is not None:
                rng.shuffle(items)
            for i in range(0, len(items), windows_per_batch):
                out.append(items[i : i + windows_per_batch])
        if rng is not None:
            rng.shuffle(out)
        for chunk in out:
            yield [(self.shards[si], rows) for si, rows in chunk]


class TurnIndex:
    """All (shard, turn) pairs with at least one decision, grouped by map."""

    def __init__(self, shards: list[Shard]):
        self.shards = shards
        self.groups: dict[str, list[tuple[int, int]]] = {}
        for si, s in enumerate(shards):
            for t in range(len(s)):
                if s.decision_count(t) > 0:
                    self.groups.setdefault(s.map_key, []).append((si, t))

    def __len__(self) -> int:
        return sum(len(v) for v in self.groups.values())

    def batches(self, turns_per_batch: int, rng: np.random.Generator | None = None):
        """Lists of turns of one map size (one neighbour table)."""
        out = []
        for key, items in self.groups.items():
            items = list(items)
            if rng is not None:
                rng.shuffle(items)
            for i in range(0, len(items), turns_per_batch):
                out.append(items[i : i + turns_per_batch])
        if rng is not None:
            rng.shuffle(out)
        for chunk in out:
            yield [self.shards[si].turn(t) for si, t in chunk]


class Uploader:
    """Host arrays to a device. A blocking copy to MPS waits until all work
    queued on the device is done, so a trainer that must not wait copies
    with `blocking=False`: the copies are queued, the host tensors kept
    until `sync` (call it before dropping the uploader)."""

    def __init__(self, device="cpu", blocking: bool = True):
        import torch

        self.device = torch.device(device)
        self.blocking = blocking or self.device.type == "cpu"
        self.keep = []

    def __call__(self, a, dtype=None):
        import torch

        a = np.ascontiguousarray(a)
        x = torch.from_numpy(a if a.flags.writeable else a.copy())
        if self.device.type != "cpu":
            if not self.blocking:
                self.keep.append(x)
            x = x.to(self.device, non_blocking=not self.blocking)
        return x if dtype is None or x.dtype == dtype else x.to(dtype)

    def sync(self):
        import torch

        if self.keep:
            if self.device.type == "mps":
                torch.mps.synchronize()
            elif self.device.type == "cuda":
                torch.cuda.synchronize(self.device)
            self.keep = []


def decision_arrays(turn: Turn, cfg: Config) -> dict:
    """A turn's decisions as arrays (Turn.dec_arrays), built from
    `decisions` when the turn has none."""
    if turn.dec_arrays is not None:
        return turn.dec_arrays
    ds = turn.decisions
    return {
        "head": np.asarray([d.head for d in ds], np.int64),
        "actor": np.asarray([d.actor for d in ds], np.int64),
        "choice": np.asarray([d.choice for d in ds], np.int64),
        "wait": np.asarray([d.is_wait for d in ds], bool),
        "k": np.asarray([len(d.rows) for d in ds], np.int64),
        "rows": np.concatenate([d.rows for d in ds]) if ds else np.zeros((0, cfg.c_candidate), np.int16),
        "meta": np.concatenate([d.meta for d in ds]) if ds else np.zeros((0, 4), np.int32),
    }


class _Pending(tuple):
    """(array, dtype): a tensor not yet copied (Deferred)."""


class Deferred:
    """An Uploader that copies nothing: `collate(…, upload=Deferred())`
    builds a batch on the host only (in a worker thread, say), and
    `upload_batch` copies it to the device later."""

    def __call__(self, a, dtype=None):
        return _Pending((np.ascontiguousarray(a), dtype))


def upload_batch(obj, upload: Uploader):
    """`obj` (a dict, list or tuple, nested) with every Deferred array copied
    by `upload`; everything else as is."""
    if isinstance(obj, _Pending):
        return upload(*obj)
    if isinstance(obj, dict):
        return {k: upload_batch(v, upload) for k, v in obj.items()}
    if isinstance(obj, (list, tuple)):
        return type(obj)(upload_batch(v, upload) for v in obj)
    return obj


def bucket(n: int, lo: int = 16) -> int:
    """`n` rounded up to one of a few sizes (at least `lo`, then four per
    octave, ≤ 25 % more): MPS compiles a kernel per tensor shape, so the
    trainer pads its batches to these sizes and the kernels are reused."""
    if n <= lo:
        return lo
    step = 1 << max(0, n.bit_length() - 3)
    return -(-n // step) * step


def collate(turns: list[Turn], cfg: Config, device="cpu", upload: Uploader | Deferred | None = None, pad: bool = False):
    """Tensors of a batch of turns of one map (see model.Net.forward);
    `upload` copies them to the device (default: blocking copies).

    `pad` (the trainer): the entity, decision and candidate counts and the
    longest candidate list are padded to `bucket` sizes, so most batches
    share shapes. Padding entities are masked; padding decisions belong to
    no head, their `padded_logits` row is [0, −∞, …] with choice 0
    (cross-entropy 0, no gradient), and `np` leaves them out; padding
    candidates belong to no head and no decision's list (`pad_index` maps a
    decision's positions to candidate rows). `np` holds the real counts
    (`n_dec`, `n_cand`)."""
    import torch

    upload = upload or Uploader(device)

    b = len(turns)
    first = turns[0]
    e_max = max(1, max(len(t.entities) for t in turns))
    e_max = bucket(e_max) if pad else e_max
    ents = np.zeros((b, e_max, cfg.c_entity), np.int16)  # stored i16, a float on the device
    ekind = np.full((b, e_max), -1, np.int64)
    eid = np.full((b, e_max), -1, np.int64)
    erel = np.full((b, e_max), -1, np.int64)
    etile = np.full((b, e_max), -1, np.int64)
    emask = np.zeros((b, e_max), bool)
    for i, t in enumerate(turns):
        e = len(t.entities)
        ents[i, :e] = t.entities
        ekind[i, :e] = t.entity_meta[:, 0]
        eid[i, :e] = t.entity_ids if t.entity_ids is not None else -1
        etile[i, :e] = np.where((t.entity_meta[:, 1] >= 0) & (t.entity_meta[:, 1] < len(t.tiles)), t.entity_meta[:, 1], -1)
        erel[i, :e] = t.entity_meta[:, 2]
        emask[i, :e] = True
    arrs = [decision_arrays(t, cfg) for t in turns]
    cat = lambda key, empty: np.concatenate([a[key] for a in arrs]) if arrs else empty
    dec_head = cat("head", np.zeros(0, np.int64)).astype(np.int64)
    dec_actor = cat("actor", np.zeros(0, np.int64)).astype(np.int64)
    dec_choice = cat("choice", np.zeros(0, np.int64)).astype(np.int64)
    dec_wait = cat("wait", np.zeros(0, bool)).astype(bool)
    dec_k = cat("k", np.zeros(0, np.int64)).astype(np.int64)
    dec_b = np.repeat(np.arange(b, dtype=np.int64), [len(a["head"]) for a in arrs])
    rows = cat("rows", np.zeros((0, cfg.c_candidate), np.int16)).astype(np.int16, copy=False)
    metas = cat("meta", np.zeros((0, 4), np.int32)).astype(np.int32, copy=False)
    # The candidate aux head's targets (cand_aux.attach; NaN where a turn has none).
    cand_aux = np.concatenate([a["cand_aux"] if "cand_aux" in a else np.full((len(a["rows"]), cfg.cand_aux), np.nan, np.float32) for a in arrs]) if cfg.cand_aux and arrs else None
    starts = np.cumsum(dec_k) - dec_k
    cand_dec = np.repeat(np.arange(len(dec_k), dtype=np.int64), dec_k)
    cand_pos = np.arange(len(cand_dec), dtype=np.int64) - starts[cand_dec]
    dec_tag = metas[starts + dec_choice, 0].astype(np.int64) if len(dec_k) else np.zeros(0, np.int64)
    heads_of_cand = dec_head[cand_dec]
    n_dec, n_cand = len(dec_k), len(cand_dec)
    # The expert's chosen candidate row per decision (`--decision-weight column:NAME`).
    dec_row = rows[starts + dec_choice] if len(dec_k) else np.zeros((0, cfg.c_candidate), np.int16)
    host = {"dec_head": dec_head, "dec_wait": dec_wait, "dec_tag": dec_tag, "dec_row": dec_row, "n_dec": n_dec, "n_cand": n_cand}
    k_max = int(dec_k.max()) if n_dec else 1
    pad_index = None
    if pad and n_dec:
        # Padding rows: decisions of head n_decision_heads (none), candidates
        # of decision 0 at position 0 that no list points to.
        d_pad, c_pad, k_max = bucket(n_dec) - n_dec, bucket(n_cand) - n_cand, bucket(k_max, 8)
        more = lambda a, n, v: np.concatenate([a, np.full((n,) + a.shape[1:], v, a.dtype)])
        nh = cfg.n_decision_heads
        dec_head, dec_actor, dec_choice, dec_wait, dec_tag = more(dec_head, d_pad, nh), more(dec_actor, d_pad, -1), more(dec_choice, d_pad, 0), more(dec_wait, d_pad, False), more(dec_tag, d_pad, -1)
        dec_b, dec_k = more(dec_b, d_pad, 0), more(dec_k, d_pad, 1)
        rows, metas = more(rows, c_pad, 0), more(metas, c_pad, -1)
        if cand_aux is not None:
            cand_aux = more(cand_aux, c_pad, np.nan)
        cand_dec, cand_pos, heads_of_cand = more(cand_dec, c_pad, 0), more(cand_pos, c_pad, 0), more(heads_of_cand, c_pad, nh)
        # padded_logits: [decision, position] → candidate row, len(rows) for
        # −∞, len(rows) + 1 for the 0 of a padding decision's only position.
        pad_index = np.full((len(dec_k), k_max), len(rows), np.int64)
        pad_index[cand_dec[:n_cand], cand_pos[:n_cand]] = np.arange(n_cand)
        pad_index[n_dec:, 0] = len(rows) + 1
    # A wait and a deferral stay out of the turn trace.
    untraced = dec_wait | ((dec_head == HEAD_UNIT) & (dec_tag == DEFER_TAG))
    trace = trace_indices(cfg, b, n_dec, starts, dec_b, dec_head, dec_choice, untraced, pad) if cfg.trace else None
    n_tiles = len(first.tiles)
    ctile = metas[:, 1].astype(np.int64)
    ctile = np.where((ctile >= 0) & (ctile < n_tiles), ctile, -1)

    # Copied to the device as stored (i16 rows: half the bytes of f32), then
    # converted there; the conversion is exact.
    t = lambda a, dt=torch.float32: upload(a, dt)

    emb_head = np.minimum(heads_of_cand, cfg.n_decision_heads - 1)  # padding candidates: bucket 0 of head n − 1
    value_np = np.asarray([x.value_target for x in turns], np.float32)
    out = {
        # Host copies for the trainer's bookkeeping (no device round trip).
        "np": host | {"value_target": value_np},
        # The longest candidate list (padded_logits) and the rows grouped by
        # decision head (Net.score), known here without a device sync.
        "k_max": k_max,
        "dec_groups": head_groups(dec_head, cfg.n_decision_heads, upload, pad),
        "cand_groups": head_groups(heads_of_cand, cfg.n_decision_heads, upload, pad),
        "region_pool": pool_layout(first.region, first.n_regions, upload),
    }
    if cfg.engram:
        out["tile_engram"] = t(np.stack([tile_engram(cfg, x.tiles) for x in turns]), torch.long)
        ent_e = np.zeros((b, e_max), np.int64)
        cand_e = []
        for i, x in enumerate(turns):
            ent_e[i, : len(x.entities)] = entity_engram(cfg, x.entities, x.entity_meta[:, 0])
            for d in x.decisions:
                ak = int(x.entity_meta[d.actor, 0]) + 1 if 0 <= d.actor < len(x.entities) else 0
                cand_e.append(candidate_engram(cfg, d.head, d.meta, ak, x.entity_meta[:, 0], n_tiles))
        out["ent_engram"] = t(ent_e, torch.long)
        out["cand_engram"] = t(np.concatenate(cand_e + [np.zeros(len(rows) - n_cand, np.int64)]), torch.long)
    if cfg.coarse:
        if first.coarse is None:
            raise ValueError("coarse regions need the map width (meta.json spec.width)")
        out["coarse"] = t(first.coarse, torch.long)
        out["n_coarse"] = int(first.coarse.max()) + 1
        out["coarse_pool"] = pool_layout(first.coarse, out["n_coarse"], upload)
    if cfg.aim:
        out["aim"] = t(np.zeros((b, cfg.n_aims), np.float32))
    if cfg.aux and cfg.n_aux:
        out["aux"] = t(np.stack([x.aux if x.aux is not None else np.full(cfg.n_aux, np.nan, np.float32) for x in turns]))
    if cfg.privileged_value and cfg.n_truth:
        out["truth"] = t(np.stack([x.truth if x.truth is not None else np.zeros(cfg.n_truth, np.float32) for x in turns]))
    if trace is not None:
        src, put, get, kt = trace
        out |= {"trace_src": t(src, torch.long), "trace_put": t(put, torch.long), "trace_get": t(get, torch.long), "trace_kt": kt}
    if cfg.events:
        ev_rows, ev_region, ev_actor, ev_mask = event_arrays(turns, cfg, pad)
        out |= {"ev_rows": t(ev_rows), "ev_region": t(ev_region, torch.long), "ev_actor": t(ev_actor, torch.long), "ev_mask": t(ev_mask, torch.bool)}
    return out | {
        "tiles": t(np.stack([x.tiles for x in turns])),
        "neighbors": t(first.neighbors, torch.long),
        "region": t(first.region, torch.long),
        "n_regions": first.n_regions,
        "globals": t(np.stack([x.global_row for x in turns])),
        "entities": t(ents),
        "ent_kind": t(ekind, torch.long),
        "ent_id": eid,
        "ent_rel": t(erel, torch.long),
        "ent_tile": t(etile, torch.long),
        "ent_mask": t(emask, torch.bool),
        "dec_b": t(dec_b, torch.long),
        "dec_head": t(dec_head, torch.long),
        "dec_actor": t(dec_actor, torch.long),
        "dec_choice": t(dec_choice, torch.long),
        "dec_k": t(dec_k, torch.long),
        "dec_wait": t(dec_wait, torch.bool),
        "dec_tag": t(dec_tag, torch.long),  # the tag of the expert's choice
        "cand_rows": t(rows),
        "cand_tag": t(tag_index(emb_head, metas[:, 0], cfg), torch.long),
        # Format 9: the unit head's defer candidates (their logit adds Net.defer_bias).
        "cand_defer": t((heads_of_cand == HEAD_UNIT) & (metas[:, 0] == DEFER_TAG), torch.bool),
        "cand_param": t(param_index(emb_head, metas[:, 3], cfg), torch.long),
        "cand_tile": t(ctile, torch.long),
        "cand_ent": t(metas[:, 2].astype(np.int64), torch.long),
        "cand_dec": t(cand_dec, torch.long),
        "cand_pos": t(cand_pos, torch.long),
        "value_target": t(value_np),
    } | ({"pad_index": t(pad_index, torch.long)} if pad_index is not None else {}) | ({"cand_aux": t(cand_aux)} if cand_aux is not None else {})


def event_arrays(turns: list[Turn], cfg: Config, pad: bool = False):
    """The events of a batch of turns for Net.embed (docs/ai-net-model.md,
    Events): rows [B, V, C_event] (stored), the region of each event's tile
    [B, V] (−1 none), its actor's local entity [B, V] (−1 none) and the mask
    of real events [B, V]; V the most events of a turn (at least 1; a
    `bucket` size when `pad`). A turn without events has none masked in."""
    b = len(turns)
    v = max([0] + [0 if x.event_meta is None else len(x.event_meta) for x in turns])
    v = bucket(max(v, 1), 8) if pad else max(v, 1)
    rows = np.zeros((b, v, cfg.c_event), np.int16)
    region = np.full((b, v), -1, np.int64)
    actor = np.full((b, v), -1, np.int64)
    mask = np.zeros((b, v), bool)
    for i, x in enumerate(turns):
        if x.event_meta is None or not len(x.event_meta):
            continue
        n = len(x.event_meta)
        rows[i, :n] = x.events
        tile = x.event_meta[:, 1].astype(np.int64)
        ok = (tile >= 0) & (tile < len(x.region))
        region[i, :n] = np.where(ok, np.asarray(x.region, np.int64)[np.where(ok, tile, 0)], -1)
        a = x.event_meta[:, 2].astype(np.int64)
        actor[i, :n] = np.where((a >= 0) & (a < len(x.entities)), a, -1)
        mask[i, :n] = True
    return rows, region, actor, mask


def trace_indices(cfg: Config, n_turns: int, n_dec: int, starts, dec_b, dec_head, dec_choice, dec_wait, pad: bool):
    """The turn trace's indices for Net.score (classic/docs/ai-net-model.md,
    The turn trace), teacher-forced: a turn's decisions are in the order
    made (decisions.npy), each choice the label. Per decision (padding
    included): `src` its choice's candidate row; `put` where its encoding
    goes in a table of `n_turns × kt` rows (turn × kt + its position in the
    turn + 1; the table's last row, n_turns × kt, for a decision outside
    the trace: another head, a wait or a deferral (`dec_wait` holds both),
    padding); `get` the row of the
    table's running sum along each turn it reads: turn × kt + its position,
    which holds the choices before it. `kt` > the most decisions of a turn
    (a bucket size when `pad`)."""
    n_all = len(dec_b)
    real = np.arange(n_all) < n_dec
    src = np.zeros(n_all, np.int64)
    src[:n_dec] = starts + dec_choice[:n_dec]
    counts = np.bincount(dec_b[:n_dec], minlength=n_turns)
    pos = np.zeros(n_all, np.int64)
    pos[:n_dec] = np.arange(n_dec) - (np.cumsum(counts) - counts)[dec_b[:n_dec]]
    kt = int(counts.max(initial=0)) + 1
    kt = bucket(kt, 8) if pad else kt
    heads = np.asarray(dec_head, np.int64)
    traced = real & (heads >= 0) & (heads < 31) & (((cfg.trace_heads >> np.clip(heads, 0, 30)) & 1) == 1) & ~np.asarray(dec_wait, bool)
    turn = np.asarray(dec_b, np.int64) * kt
    put = np.where(traced, turn + pos + 1, n_turns * kt)
    get = np.where(real, turn + pos, 0)
    return src, put, get, kt


def pool_layout(groups: np.ndarray, n: int, upload=None):
    """For Net.pool: the tiles per group (clamped to ≥ 1) and a [n, M + 1]
    table of each group's tiles, padded with T + 1 (−∞) to the largest group
    M, then T (the zero every group's max includes); T = the tile count."""
    groups = np.asarray(groups, np.int64)
    n_tiles = len(groups)
    cnt = np.bincount(groups, minlength=n)[:n]
    order = np.argsort(groups, kind="stable")
    start = np.concatenate([[0], np.cumsum(cnt)[:-1]])
    width = int(cnt.max()) if n else 0
    slots = np.full((n, width + 1), n_tiles + 1, np.int64)
    slots[groups[order], np.arange(n_tiles) - start[groups[order]]] = order
    slots[:, width] = n_tiles
    upload = upload or Uploader()
    return upload(np.maximum(cnt, 1)), upload(slots)


def head_groups(heads: np.ndarray, n_heads: int, upload=None, pad: bool = False):
    """Rows grouped by decision head, for Net.per_head: (order, spans,
    inverse). `order` lists, head after head, the rows of heads 0 …
    n_heads − 1 (stable), each head's run padded to `bucket` size when `pad`
    with len(heads) (a zero row); `spans` per head the (start, count) of its
    run (Python ints: no device sync); `inverse` the position of each row in
    `order`, len(order) (zero) for rows of no head."""
    heads = np.asarray(heads, np.int64)
    n = len(heads)
    order = np.argsort(heads, kind="stable")
    bounds = np.searchsorted(heads[order], np.arange(n_heads + 1))
    runs, spans, at = [], [], 0
    inverse = np.full(n, -1, np.int64)
    for h in range(n_heads):
        rows = order[bounds[h] : bounds[h + 1]]
        size = bucket(len(rows)) if pad and len(rows) else len(rows)
        runs.append(np.concatenate([rows, np.full(size - len(rows), n, np.int64)]))
        inverse[rows] = at + np.arange(len(rows))
        spans.append((at, size))
        at += size
    order = np.concatenate(runs) if runs else np.zeros(0, np.int64)
    inverse[inverse < 0] = len(order)
    upload = upload or Uploader()
    return upload(order), tuple(spans), upload(inverse)
