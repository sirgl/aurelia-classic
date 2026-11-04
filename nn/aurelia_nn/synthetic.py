"""Synthetic shards in the contract's format (classic/docs/ai-net-data.md),
for tests and for developing the trainer before the exporter's data exists.

The expert's choices follow hidden rules over the features (a candidate's own
row, its target tile and that tile's terrain id, the actor's row, the global
row, the parameter id) and one rule that needs memory: a unit is born with a
goal tag, shown only on its first turn (entity column 1), and prefers
candidates of that tag for the rest of its life.

Format 2 (default) has persistent entity ids (entity_meta column 3) and
`windows.npy`, with each player's turns contiguous; format 1 has neither and
turns in round order, like the first exporter. `events=True` adds the event
table of format 7 (`events.npy`, `event_meta.npy`, `turn_events.npy`), drawn
from a generator of its own so the other arrays stay those of the same seed
without events.

Also a stand-in for the feature binary: `python -m aurelia_nn.synthetic
<log> --out <dir> [--turn-sample N]` writes the shard of a synthetic "log"
(a JSON file with seed, width, height, rounds, players).
"""

from __future__ import annotations

import argparse
import json
import math
import os
from pathlib import Path

import numpy as np

KINDS = 6  # own unit, own city, foreign unit, foreign city, city-state, player
RELATIONS = 7
GOAL_COLUMN = 1  # entity column carrying a unit's goal tag on its birth turn
TERRAIN_COLUMN = 2  # tile column `terrain_id_plus_1`
UNIT_KIND_COLUMN = 2  # entity column `unit_kind_id_plus_1`


def default_layout(c_tile=12, c_global=10, c_entity=8, c_candidate=6) -> dict:
    scales = [1, 10, 100, 4]

    def table(prefix, n, ids=()):
        out = [{"name": f"{prefix}_{i}", "scale": scales[i % len(scales)]} for i in range(n)]
        for col, name in ids:
            if col < n:
                out[col] = {"name": name, "scale": 1}
        return out

    return {
        "tile": table("tile", c_tile, [(TERRAIN_COLUMN, "terrain_id_plus_1"), (3, "resource_id_plus_1")]),
        "global": table("global", c_global),
        "entity": table("entity", c_entity, [(UNIT_KIND_COLUMN, "unit_kind_id_plus_1"), (3, "level")]),
        "candidate": table("candidate", c_candidate),
    }


def event_layout() -> list:
    """The synthetic event table: the kind and parameter ids, then numbers."""
    return [{"name": "event_kind_id_plus_1", "scale": 1}, {"name": "event_param_id_plus_1", "scale": 1}, {"name": "age", "scale": 1},
            {"name": "event_value", "scale": 10}, {"name": "event_flag", "scale": 1}]


def hex_neighbors(width: int, height: int) -> np.ndarray:
    """Odd-q offset hex neighbours, −1 off the map (synthetic only: the real
    order is the kernel's)."""
    out = np.full((width * height, 6), -1, np.int32)
    for y in range(height):
        for x in range(width):
            odd = x & 1
            deltas = [(0, -1), (1, -1 + odd), (1, odd), (0, 1), (-1, odd), (-1, -1 + odd)]
            for k, (dx, dy) in enumerate(deltas):
                nx, ny = x + dx, y + dy
                if 0 <= nx < width and 0 <= ny < height:
                    out[y * width + x, k] = ny * width + nx
    return out


def regions(width: int, height: int) -> np.ndarray:
    rw = math.ceil(width / 4)
    ys, xs = np.divmod(np.arange(width * height), width)
    return ((ys // 4) * rw + xs // 4).astype(np.int32)


def _rows(rng, n, scales, lo=-2.0, hi=2.0):
    vals = rng.uniform(lo, hi, size=(n, len(scales)))
    return np.clip(np.round(vals * np.asarray(scales)[None, :]), -32768, 32767).astype(np.int16)


def write_shard(out: str | os.PathLike, seed: int, width: int = 16, height: int = 12, rounds: int = 10, players: int = 2,
                layout: dict | None = None, turn_sample: int = 1, entities: tuple[int, int] = (0, 5), fmt: int = 2, extras: bool = True, religion: bool = False,
                events: bool = False) -> Path:
    """`entities`: the range of visible foreign units per turn (the rest are
    own units, cities, city-states and nations). `extras`: also the
    training-only tables `aux.npy` (future targets) and `truth.npy`.
    `religion`: a fifth decision head (index 4, tags 600+). `events`: 0 … 6
    events a turn (format 7), some with a tile and an actor (a city-state or
    nation row of the turn)."""
    rng = np.random.default_rng(seed)
    ev_rng = np.random.default_rng(seed + 7919)
    layout = layout or default_layout()
    if events:
        layout = dict(layout) | {"event": event_layout()}
    sc = {t: np.asarray([e["scale"] for e in v], np.float64) for t, v in layout.items() if t != "event"}
    c_ent = len(sc["entity"])
    n_tiles = width * height
    nb = hex_neighbors(width, height)
    reg = regions(width, height)
    static = _rows(rng, n_tiles, sc["tile"])
    if TERRAIN_COLUMN < len(sc["tile"]):
        static[:, TERRAIN_COLUMN] = rng.integers(1, 8, n_tiles)
    if 3 < len(sc["tile"]):
        static[:, 3] = np.where(rng.random(n_tiles) < 0.2, rng.integers(1, 40, n_tiles), 0)

    # Persistent own units per player: id, goal tag, birth round, tile, kind id.
    next_id = [1000 * (p + 1) for p in range(players)]
    units = [[] for _ in range(players)]

    def birth(p, rnd):
        units[p].append({"id": next_id[p], "goal": int(rng.integers(1, 5)), "born": rnd, "tile": int(rng.integers(0, n_tiles)), "kind": int(rng.integers(1, 30))})
        next_id[p] += 1

    for p in range(players):
        for _ in range(3):
            birth(p, -1)

    per_turn = []  # (round, player, turn dict)
    head_names = ("unit", "research", "production", "policy") + (("religion",) if religion else ())
    counts = {h: 0 for h in head_names}
    for rnd in range(rounds):
        for player in range(players):
            units[player] = [u for u in units[player] if rng.random() > 0.08]
            if rng.random() < 0.5 or not units[player]:
                birth(player, rnd)
            for u in units[player]:
                u["tile"] = int(nb[u["tile"], rng.integers(0, 6)]) if rng.random() < 0.7 else u["tile"]
                if u["tile"] < 0:
                    u["tile"] = int(rng.integers(0, n_tiles))
            if turn_sample > 1 and fmt == 1 and (rnd * players + player) % turn_sample:
                continue
            t_tiles = static.copy()
            t_tiles[:, 0] = np.clip(t_tiles[:, 0] + rng.integers(-1, 2, n_tiles), -32768, 32767)
            g = _rows(rng, 1, sc["global"])[0]
            ents, meta = [], []
            for u in units[player]:
                row = _rows(rng, 1, sc["entity"])[0]
                row[GOAL_COLUMN] = u["goal"] * sc["entity"][GOAL_COLUMN] if u["born"] == rnd else 0
                if UNIT_KIND_COLUMN < c_ent:
                    row[UNIT_KIND_COLUMN] = u["kind"]
                ents.append(row)
                meta.append([0, u["tile"], 0, u["id"]])
            for c in range(2):
                row = _rows(rng, 1, sc["entity"])[0]
                row[UNIT_KIND_COLUMN] = 0
                ents.append(row)
                meta.append([1, (player * 37 + c * 11) % n_tiles, 0, 50_000 + 10 * player + c])
            for _ in range(int(rng.integers(*entities))):
                row = _rows(rng, 1, sc["entity"])[0]
                row[UNIT_KIND_COLUMN] = int(rng.integers(1, 30))
                ents.append(row)
                meta.append([2, int(rng.integers(0, n_tiles)), int(rng.integers(1, 4)), -1])
            for cs in range(2):
                row = _rows(rng, 1, sc["entity"])[0]
                row[UNIT_KIND_COLUMN] = 0
                ents.append(row)
                meta.append([4, int((cs * 53 + 7) % n_tiles), 4, 9000 + cs])
            for other in range(players):
                if other != player:
                    row = _rows(rng, 1, sc["entity"])[0]
                    row[UNIT_KIND_COLUMN] = 0
                    ents.append(row)
                    meta.append([5, -1, int(rng.integers(1, 4)), other])
            e_rows = np.asarray(ents, np.int16)
            e_meta = np.asarray(meta, np.int64)
            decs = []  # head, actor (local), rows, cmeta (local entity), choice

            def add(head, actor, rows, cmeta, scores):
                decs.append((head, actor, rows, cmeta, int(np.argmax(scores))))
                counts[head_names[head]] += 1

            tile_val = t_tiles[:, 0] / sc["tile"][0] + (t_tiles[:, TERRAIN_COLUMN] == 3) * 1.0
            e = len(ents)
            for i, u in enumerate(units[player]):
                k = int(rng.integers(2, 8))
                rows = _rows(rng, k, sc["candidate"])
                tags = rng.integers(0, 10, k)
                if rng.random() < 0.7:
                    tags[rng.integers(0, k)] = u["goal"]
                tgt = np.where(rng.random(k) < 0.8, rng.integers(0, n_tiles, k), -1)
                tent = np.where(rng.random(k) < 0.2, rng.integers(0, e, k), -1)
                par = np.where(rng.random(k) < 0.5, rng.integers(0, 20, k), -1)
                cm = np.stack([tags, tgt, tent, par], 1)
                score = rows[:, 0] / sc["candidate"][0] + np.where(tgt >= 0, tile_val[tgt], -1.0) + (tags == u["goal"]) * 2.0
                add(0, i, rows, cm, score)
            city = len(units[player])
            k = int(rng.integers(3, 9))
            rows = _rows(rng, k, sc["candidate"])
            par = rng.choice(200, k, replace=False)
            cm = np.stack([400 + rng.integers(0, 3, k), np.full(k, meta[city][1]), np.full(k, -1), par], 1)
            score = rows[:, 0] / sc["candidate"][0] - (e_rows[city, 0] / sc["entity"][0]) * (rows[:, 2] / sc["candidate"][2])
            add(2, city, rows, cm, score)
            k = int(rng.integers(3, 11))
            rows = _rows(rng, k, sc["candidate"])
            par = rng.choice(80, k, replace=False)
            cm = np.stack([np.full(k, 300), np.full(k, -1), np.full(k, -1), par], 1)
            score = (rows[:, 1] / sc["candidate"][1]) * np.sign(g[0] + 0.5) + (par % 7) / 3.0
            add(1, -1, rows, cm, score)
            if rng.random() < 0.4:
                k = int(rng.integers(2, 7))
                rows = _rows(rng, k, sc["candidate"])
                par = rng.choice(60, k, replace=False)
                cm = np.stack([np.full(k, 500), np.full(k, -1), np.full(k, -1), par], 1)
                score = ((par * 7) % 5) / 2.0 + rows[:, 0] / sc["candidate"][0]
                add(3, -1, rows, cm, score)
            if religion and rng.random() < 0.5:
                k = int(rng.integers(2, 6))
                rows = _rows(rng, k, sc["candidate"])
                par = rng.choice(30, k, replace=False)
                cm = np.stack([600 + rng.integers(0, 3, k), np.full(k, -1), np.full(k, -1), par], 1)
                score = ((par * 3) % 4) / 2.0 + rows[:, 1] / sc["candidate"][1]
                add(4, -1, rows, cm, score)
            ev_rows, ev_meta = [], []
            if events:
                others = [i for i, m in enumerate(meta) if m[0] in (4, 5)]
                for _ in range(int(ev_rng.integers(0, 7))):
                    k = int(ev_rng.integers(0, 20))
                    par = int(ev_rng.integers(-1, 40))
                    tile = int(ev_rng.integers(0, n_tiles)) if ev_rng.random() < 0.6 else -1
                    actor = others[int(ev_rng.integers(0, len(others)))] if others and ev_rng.random() < 0.7 else -1
                    ev_rows.append([k + 1, 0 if par < 0 else 1 + k * 1024 + par, int(ev_rng.integers(0, 10)), int(ev_rng.integers(-50, 300)), int(ev_rng.integers(0, 2))])
                    ev_meta.append([k, tile, actor, par])
            per_turn.append((rnd, player, {"tiles": t_tiles, "global": g, "entities": e_rows, "meta": e_meta, "decisions": decs, "events": ev_rows, "event_meta": ev_meta}))

    if fmt == 2:
        per_turn.sort(key=lambda x: (x[1], x[0]))  # each player's turns contiguous
    turns, tiles, globs, ents_all, emeta_all, decs_all, cands, cmeta_all, windows = [], [], [], [], [], [], [], [], []
    turn_events, ev_all, evmeta_all = [], [], []
    for row, (rnd, player, t) in enumerate(per_turn):
        first = len(ents_all)
        turns.append([rnd, player, first, len(t["entities"])])
        turn_events.append([len(evmeta_all), len(t["event_meta"])])
        ev_all.extend(t["events"])
        evmeta_all.extend([m[0], m[1], m[2] + first if m[2] >= 0 else -1, m[3]] for m in t["event_meta"])
        tiles.append(t["tiles"])
        globs.append(t["global"])
        ents_all.extend(t["entities"])
        emeta_all.extend(m if fmt == 2 else m[:3] for m in t["meta"])
        for head, actor, rows, cm, choice in t["decisions"]:
            cm = np.array(cm)
            cm[:, 2] = np.where(cm[:, 2] >= 0, cm[:, 2] + first, -1)
            decs_all.append([row, head, actor + first if actor >= 0 else -1, len(cands), len(rows), choice])
            cands.extend(rows)
            cmeta_all.extend(cm)
        if fmt == 2 and (not windows or windows[-1][0] != player):
            windows.append([player, row, 0])
        if fmt == 2:
            windows[-1][2] += 1

    shares = rng.dirichlet(np.ones(players))
    if extras:
        layout = dict(layout)
        layout["aux"] = [{"name": "score_in_10", "scale": 100}, {"name": "city_lost_in_10", "scale": 1, "binary": True}]
        layout["truth"] = [{"name": "points_share_now", "scale": 1000}, {"name": "hidden_strength", "scale": 10}]
        aux = np.stack([np.round(100 * (shares[[t[1] for t in turns]] + 0.01 * np.asarray([t[0] for t in turns]))), rng.integers(0, 2, len(turns))], 1)
        truth = np.stack([np.round(1000 * shares[[t[1] for t in turns]]), rng.integers(0, 50, len(turns))], 1)
    meta = {
        "format": 7 if events else fmt,
        "spec": {"seed": seed, "suite": "synthetic", "width": width, "height": height, "players": players, "rounds": rounds, "variant": "standard"},
        "tiles": n_tiles,
        "regions": int(reg.max()) + 1,
        "layout": layout,
        "heads": list(head_names),
        "tags": {"wait": 250, "research": 300, "produce_unit": 400, "adopt_policy": 500}
        | ({"religion_found": 600, "religion_save": 601, "religion_enhance": 602} if religion else {}),
        "standings": [
            {"player": p, "score": int(1000 * shares[p]), "points_share": float(shares[p]), "rank": int(1 + np.sum(shares > shares[p])), "victory": None}
            for p in range(players)
        ],
        "counts": {"decisions": counts, "dropped": {}},
    }
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    with open(out / "meta.json", "w") as f:
        json.dump(meta, f, indent=1)

    def save(name, arr, dtype, cols):
        a = np.asarray(arr, dtype=dtype)
        if a.size == 0 and cols:
            a = a.reshape(0, cols)
        np.save(out / name, a.astype(np.dtype(dtype).newbyteorder("<"), copy=False))

    save("neighbors.npy", nb, "<i4", 6)
    save("region.npy", reg, "<i4", 0)
    save("turns.npy", turns, "<i4", 4)
    save("tiles.npy", np.concatenate(tiles) if tiles else np.zeros((0, len(sc["tile"]))), "<i2", len(sc["tile"]))
    save("globals.npy", globs, "<i2", len(sc["global"]))
    save("entities.npy", ents_all, "<i2", c_ent)
    save("entity_meta.npy", emeta_all, "<i4", 4 if fmt == 2 else 3)
    save("decisions.npy", decs_all, "<i4", 6)
    save("candidates.npy", cands, "<i2", len(sc["candidate"]))
    save("candidate_meta.npy", cmeta_all, "<i4", 4)
    if fmt == 2:
        save("windows.npy", windows, "<i4", 3)
    if extras:
        save("aux.npy", aux, "<i2", 2)
        save("truth.npy", truth, "<i2", 2)
    if events:
        save("turn_events.npy", turn_events, "<i4", 2)
        save("events.npy", ev_all, "<i2", len(event_layout()))
        save("event_meta.npy", evmeta_all, "<i4", 4)
    return out


def write_dataset(root: str | os.PathLike, games: int = 8, sizes=((16, 12), (20, 16)), rounds: int = 10, players: int = 2, seed: int = 0,
                  layout: dict | None = None, fmt: int = 2, religion: bool = False, events: bool = False) -> list[Path]:
    root = Path(root)
    out = []
    for g in range(games):
        w, h = sizes[g % len(sizes)]
        out.append(write_shard(root / f"{seed + g}-synthetic.shard", seed + g, w, h, rounds, players, layout, fmt=fmt, religion=religion, events=events))
    return out


def main():
    ap = argparse.ArgumentParser(description="Write a synthetic shard from a synthetic log (stand-in for ai-features).")
    ap.add_argument("log", help="JSON file: {seed, width, height, rounds, players}")
    ap.add_argument("--out", required=True)
    ap.add_argument("--turn-sample", type=int, default=1)
    a = ap.parse_args()
    with open(a.log) as f:
        spec = json.load(f)
    write_shard(a.out, int(spec.get("seed", 0)), int(spec.get("width", 16)), int(spec.get("height", 12)), int(spec.get("rounds", 10)),
                int(spec.get("players", 2)), turn_sample=a.turn_sample, fmt=1)


if __name__ == "__main__":
    main()
