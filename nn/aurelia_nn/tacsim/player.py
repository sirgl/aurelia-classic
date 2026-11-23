"""The player's modifiers in MLX's economy, every stat by its name: the Player-scope rows of the effect
table (nation, technologies, policies, owner-wide buildings; the founder's beliefs and timed effects as
the dump lists them) summed per stat, exact to `kernel::effects::player::player_modifiers().stats`
and its cost percents.

    stats(prow, t)  ->  {stat key: amount}   (key: the stat's Debug text, canonical: `key("ResourceCopies(ResourceId(0))")`)
    costs(prow, t)  ->  {cost target key: percent}

The happiness rows and the named society stats stay in `happy.player_terms`; this is the general
table every later subsystem (resources, treasury, culture, great people) reads.
"""

import json
import re

from . import city as C
from . import happy as H
from . import rdebug as R

_STAT = re.compile(r"modifier: AddStat \{ stat: (.*), amount: (-?\d+) \}, duration")
_COST = re.compile(r"modifier: ModifyCost \{ target: (.*), percent: (-?\d+) \}, duration")


def key(text):
    """The canonical key of a stat (or cost target) Debug text."""
    return json.dumps(R.parse(text), sort_keys=True)


def _parsed(row):
    if "_pm" not in row:
        text = row.get("raw", "")
        m = _STAT.search(text)
        c = _COST.search(text)
        row["_pm"] = (("stat", key(m.group(1)), int(m.group(2))) if m else ("cost", key(c.group(1)), int(c.group(2))) if c else None)
    return row["_pm"]


def _rows(prow, t, st, p, extra_buildings):
    er_players = t["_players"]
    peace = not any(st["war"][o] and er_players[o]["major"] and st["alive"][o] and o != p for o in range(len(er_players)))
    st = dict(st, _peace=peace)
    golden = st["golden_age"]
    counts_b = {}
    for c in st["cities"]:
        for b in c["buildings"]:
            counts_b[b] = counts_b.get(b, 0) + 1
    for b in extra_buildings:
        counts_b[b] = counts_b.get(b, 0) + 1
    nation = er_players[p]["nation"]
    pol = set(st["policies"])
    out = []
    for row in prow.static:
        kind, sid = row["src"]
        if kind == "nation":
            n = int(nation == sid)
        elif kind == "tech":
            n = int(st["techs"][sid] == "1")
        elif kind == "policy":
            n = int(sid in pol)
        else:
            cnt = counts_b.get(sid, 0)
            n = min(cnt, 1) if row["unique"] else cnt
        if n and all(H.holds(c, st, golden) for c in row["c"]):
            out.append((row, n))
    for text, n in st["founder"] + st["timed"]:
        row = prow.parse_extra(text)
        if row is not None:
            row.setdefault("raw", text)
            if all(H.holds(c, st, golden) for c in row["c"]):
                out.append((row, n))
    return out


def stats_and_costs(prow, t, side="pre", extra_buildings=()):
    """({stat key: amount}, {cost key: percent}) of the player's rows."""
    if "_players" not in t:
        raise KeyError("turn line without _players (set by `prepare`)")
    stats, costs = {}, {}
    for row, n in _rows(prow, t, t[side]["state"], t["player"], extra_buildings):
        pm = _parsed(row)
        if pm is None:
            continue
        kind, k, amount = pm
        d = stats if kind == "stat" else costs
        d[k] = d.get(k, 0) + amount * n
    return stats, costs


def rust_stats(text):
    """Rust's `StatTotals { entries: [(stat, n), ...] }` as {stat key: n}."""
    v = R.body(R.parse(text))
    out = {}
    for e in v["entries"]:
        stat, n = e["()"]
        k = json.dumps(stat, sort_keys=True)
        out[k] = out.get(k, 0) + n
    return out


def prepare(game, turns):
    """Attach the seats to each turn line (the player rows read them)."""
    for t in turns:
        t["_players"] = game["rules"]["players"]
    return turns


class Rows(H.PlayerRows):
    """`happy.PlayerRows` that keeps each row's text (the stats are read by name)."""

    def __init__(self, rows, er):
        super().__init__(rows, er)
        # The same selection as the parent, with the texts.
        self.static = []
        for src, sid, bucket, text in er["effects"]:
            if bucket != "player" or src == "belief":
                continue
            row = rows.parse(text, "player")
            if row is None:
                continue
            if src == "building":
                if row["sel"] not in ("Owner", "OwnerCities", "OwnerCityTiles", "Capital") or sid not in rows.owner_wide:
                    continue
                row["src"] = ("wide", sid)
            else:
                row["src"] = (src, sid)
            row["raw"] = text
            self.static.append(row)


def get(stats, text, default=0):
    return stats.get(key(text), default)
