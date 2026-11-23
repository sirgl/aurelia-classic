"""The economy on the GPU (MLX), second subsystem: a city's yields, exact to the Rust rules
(`kernel::derived::yields::city_breakdown`): the effects that reach the city and its tiles, the
worked tiles and specialists (`derived::citizens::assign`), the base, the rates; the production cost
of the head of the queue.

    python -m aurelia_nn.tacsim.city DUMP.jsonl      # parity against Rust, every city of every end of turn

Data-oriented (the design lens: branchy rules as flat tables):
- every passive City and Tile effect of the rules is one row of an effect table: its source (nation,
  technology, policy, a building of the city, a building anywhere in the empire), its selector gate
  (capital), two conditions as (kind, argument) codes and its modifier as (kind, channel, amount);
- a city's instances of each row are a gather from the owner's masks (nation, technologies,
  policies, building counts) and the city's building mask; each condition kind is one vectorized
  mask over [cities, rows] (or [cities, tiles, rows] for the tile effects); the stage totals are one
  masked sum;
- the citizens are placed by the greedy rule of `assign` for all cities at once: forced specialists,
  locked tiles, then one citizen per step to the best of (best free tile, best specialist), ties as
  in Rust (lower tile id, lower specialist kind).

Inputs from the subsystems not yet in MLX (from the dump, the same numbers Rust reads): the beliefs
and timed effects that reach the city (rows of the same table with their instance counts), city-state
food, great work culture, trade route yields, landmark culture, the religious counts, the garrison
and the capital connection, the happiness percent and the golden age.
"""

import argparse
import json
import time

import mlx.core as mx

from . import econ
from . import rdebug as R

I32 = mx.int32
NY = 6
RADIUS = 3
NCAND = 37  # tiles of a radius-3 disc
CHAN = {"Food": 0, "Production": 1, "Gold": 2, "Science": 3, "Culture": 4}
WEIGHTS = [12, 8, 5, 6, 6, 5, 4]
FOCUS = {"Default": None, "Food": 0, "Production": 1, "Gold": 2, "Science": 3, "Culture": 4, "ModuleYield": 5, "SpecialistPoints": 6}
FOOD_PER_CITIZEN = 2
MID_KEYS = ("connected", "after")
GOLDEN_AGE_PRODUCTION_PERCENT = 20

# Condition codes (both conditions of a row).
C_ALWAYS, C_TECH, C_NOTECH, C_MINPOP, C_BUILDING, C_PRODUCING, C_GARRISON, C_COASTAL, C_RIVER, C_PEACE, C_CONNECTED, \
    C_ADJFEATURE, C_GOLDEN, C_FOLLOWERS, C_CAPBUILT, C_NEVER = range(16)
T_IMPROVEMENT, T_RESOURCE, T_FEATURE, T_TERRAIN, T_PLOT, T_CLASS, T_FEATURE_UNIMPROVED = range(20, 27)
PLOTS = {"River": 0, "Sea": 1, "Lake": 2, "SeaResource": 3, "NaturalWonder": 4}
NEVER = {"TileTerrainUntil", "OpponentRole", "OpponentDomain", "OpponentClassOf", "NearOwnImprovement", "EventNearSourceCity",
         "LearnedTechnology", "UnitDomain", "UnitRole"}
MODULE_NEVER = {"OpponentIsBarbarian", "OpponentIsCityState", "OpponentHasMoreCities", "NearFollowingCity", "GreatPersonIs"}
# Modifier codes.
M_NONE, M_FLAT, M_PCT, M_PERPOP, M_GROWTH, M_STAT, M_COST, M_FLAT_COUNT, M_PCT_COUNT, M_HAPPY, M_HAPPY_COUNT = range(11)
S_FOODKEPT, S_ANYSPEC, S_SPEC, S_SPECFOOD, S_WONDERCULTURE, S_POPUNHAPPY, S_SPECUNHAPPY, S_NOOCCUPIED = range(1, 9)
# The society and economy stats a player's rows may carry (`happy.py`), by their innermost name.
PLAYER_STATS = ("CityCountUnhappinessPercent", "PopulationUnhappinessPercent", "HappinessPerCity", "NaturalWonderHappinessPercent",
                "GoldenAgeLengthPercent", "ScienceWhileHappyPercent", "HappinessPerLuxury", "AllyScience", "ResearchAgreementPercent", "CapitalScienceBuildingBoost")
COUNTS = {"Population": 0, "WorldWonders": 1, "MajorityFollowers": 2, "ReligionsPresent": 3, "TradePartners": 4, "CityStateRoutes": 5,
          "Policies": 6, "PeacefulForeignFollowers": 7, "ConnectedCities": 8}
COST_INDEX = {"Units": 0, "Buildings": 1, "BorderGrowth": 2, "BorderGrowthExponent": 3}  # the city's cost targets (not the purchases)


def chan(v):
    """Our channel of a `YieldKind` (−1: none of the six)."""
    t = R.tag(v)
    if t in CHAN:
        return CHAN[t]
    if t == "Module":
        return 5
    if t == "Output":
        return 1  # the legacy combined channel counts as production
    raise ValueError(f"yield kind {v}")


class Unsupported(Exception):
    pass


class Rows:
    """The effect table: one row per (source, compiled effect); extra rows (beliefs, timed effects)
    are appended as the dump names them."""

    def __init__(self, er):
        self.er = er
        self.nb = len(er["buildings"])
        self.nu = len(er["units"])
        self.owner_wide = set(er["owner_wide"])
        self.filters = []  # distinct Producing filters
        self.city, self.tile = [], []  # row dicts
        self.extra = {}  # (bucket, text) -> row index
        for src, sid, bucket, text in er["effects"]:
            if bucket not in ("city", "tile") or src == "belief":
                continue  # beliefs reach a city through the dump's lists (religion is not in MLX yet)
            row = self.parse(text, bucket)
            if row is None:
                continue
            sel = row["sel"]
            if src == "building":
                if sel in ("OwnerCities", "OwnerCityTiles", "Capital"):
                    row["src"] = ("wide", sid) if sid in self.owner_wide else ("none", sid)
                else:
                    row["src"] = ("local", sid)
            else:
                row["src"] = (src, sid)
            (self.city if bucket == "city" else self.tile).append(row)

    def add_extra(self, bucket, text):
        key = (bucket, text)
        if key not in self.extra:
            row = self.parse(text, bucket)
            if row is None:
                row = {"sel": "none", "c": [(C_NEVER, 0, 0)] * 2, "m": (M_NONE, 0, 0, 0, 0, 0)}
            row["src"] = ("extra", len(self.extra))
            rows = self.city if bucket == "city" else self.tile
            self.extra[key] = (len(rows), row["src"][1])
            rows.append(row)
        return self.extra[key]

    def filter_index(self, f):
        key = json.dumps(f, sort_keys=True)
        for i, (k, _) in enumerate(self.filters):
            if k == key:
                return i
        self.filters.append((key, f))
        return len(self.filters) - 1

    def cond(self, c):
        t = R.tag(c)
        b = R.body(c)
        if t == "Always":
            return (C_ALWAYS, 0, 0)
        if t == "HasTechnology":
            return (C_TECH, R.arg(b[0]), 0)
        if t == "LacksTechnology":
            return (C_NOTECH, R.arg(b[0]), 0)
        if t == "MinPopulation":
            return (C_MINPOP, b[0], 0)
        if t == "HasBuilding":
            return (C_BUILDING, R.arg(b[0]), 0)
        if t == "Producing":
            f = b[0]
            if R.tag(f) == "BuiltInCapital":
                return (C_CAPBUILT, 0, 0)
            return (C_PRODUCING, self.filter_index(f), 0)
        if t == "CityGarrisoned":
            return (C_GARRISON, 0, 0)
        if t == "CityCoastal":
            return (C_COASTAL, 0, 0)
        if t == "CityOnRiver":
            return (C_RIVER, 0, 0)
        if t == "AtPeace":
            return (C_PEACE, 0, 0)
        if t == "ConnectedToCapital":
            return (C_CONNECTED, 0, 0)
        if t == "CityAdjacentUnimprovedFeature":
            return (C_ADJFEATURE, R.arg(b["feature"]), b["at_least"])
        if t == "TileImprovement":
            return (T_IMPROVEMENT, R.arg(b[0]), 0)
        if t == "TileResource":
            return (T_RESOURCE, R.arg(b[0]), 0)
        if t == "TileFeature":
            return (T_FEATURE, R.arg(b[0]), 0)
        if t == "TileTerrain":
            code = self.er["terrain_codes"][R.arg(b[0])]
            return (T_TERRAIN, code, 0)
        if t == "TilePlot":
            return (T_PLOT, PLOTS[R.tag(b[0])], 0)
        if t == "TileResourceClass":
            return (T_CLASS, resource_class(R.tag(b[0])), 0)
        if t == "TileFeatureUnimproved":
            return (T_FEATURE_UNIMPROVED, R.arg(b[0]), 0)
        if t in NEVER:
            return (C_NEVER, 0, 0)
        if t == "Module":
            m = b[0]
            mt = R.tag(m)
            if mt == "GoldenAge":
                return (C_GOLDEN, 0, 0)
            if mt == "MinFollowers":
                return (C_FOLLOWERS, R.body(m)[0], 0)
            if mt in MODULE_NEVER:
                return (C_NEVER, 0, 0)
        raise Unsupported(f"condition {c}")

    def modifier(self, m):
        t = R.tag(m)
        b = R.body(m)
        if t == "AddYield":
            return (M_FLAT, chan(b["yield_kind"]), b["amount"], 0, 0, 0)
        if t == "AddYieldPercent":
            return (M_PCT, chan(b["yield_kind"]), b["percent"], 0, 0, 0)
        if t == "AddYieldPerPopulation":
            return (M_PERPOP, chan(b["yield_kind"]), b["hundredths"], 0, 0, 0)
        if t == "ModifyGrowth":
            return (M_GROWTH, 0, b["percent"], 0, 0, 0)
        if t in ("AddYieldPerCount", "AddYieldPercentPerCount"):
            per = R.body(b["per"])
            count = per["count"]
            name = R.tag(count) if R.tag(count) != "Module" else R.tag(R.body(count)[0])
            if name not in COUNTS:
                raise Unsupported(f"count {count}")
            mx_ = R.arg(per["max"])
            amount = b["amount"] if t == "AddYieldPerCount" else b["percent"]
            return (M_FLAT_COUNT if t == "AddYieldPerCount" else M_PCT_COUNT, chan(b["yield_kind"]), amount, COUNTS[name], per["every"],
                    -1 if mx_ is None else abs(mx_))
        if t == "Module":
            inner = b[0]
            it, ib = R.tag(inner), R.body(inner)
            if it == "AddHappiness":
                return (M_HAPPY, 0, ib["amount"], 0, 0, 0)
            if it == "AddHappinessPerCount":
                per = R.body(ib["per"])
                count = per["count"]
                name = R.tag(count) if R.tag(count) != "Module" else R.tag(R.body(count)[0])
                if name not in COUNTS:
                    raise Unsupported(f"count {count}")
                mx_ = R.arg(per["max"])
                return (M_HAPPY_COUNT, 0, ib["amount"], COUNTS[name], per["every"], -1 if mx_ is None else abs(mx_))
            if it == "ModifyCityState" and R.tag(ib["stat"]) in PLAYER_STATS:
                return (M_STAT, 100 + PLAYER_STATS.index(R.tag(ib["stat"])), ib["amount"], 0, 0, 0)
            if it in ("AddHappinessPerFollowingCity", "AddYieldPerFollowers", "AddYieldPerFollowingCity"):
                raise Unsupported(f"unresolved {it}")
            return (M_NONE, 0, 0, 0, 0, 0)
        if t == "AddStat":
            st = b["stat"]
            amount = b["amount"]
            stt = R.tag(st)
            if stt == "FoodKept":
                return (M_STAT, S_FOODKEPT, amount, 0, 0, 0)
            if stt == "AnySpecialistYield":
                return (M_STAT, S_ANYSPEC, amount, chan(R.body(st)[0]), 0, 0)
            if stt == "SpecialistYield":
                sb = R.body(st)
                spec = R.arg(sb["specialist"])
                return (M_STAT, S_SPEC, amount, chan(sb["yield_kind"]), -1 if spec is None else spec, 0)
            if stt == "Module":
                inner = R.body(st)[0]
                if R.tag(inner) == "Society":
                    soc = R.tag(R.body(inner)[0])
                    if soc == "SpecialistFoodPercent":
                        return (M_STAT, S_SPECFOOD, amount, 0, 0, 0)
                    if soc == "WonderCulturePercent":
                        return (M_STAT, S_WONDERCULTURE, amount, 0, 0, 0)
                    if soc == "SpecialistUnhappinessPercent":
                        return (M_STAT, S_SPECUNHAPPY, amount, 0, 0, 0)
                    if soc == "NoOccupiedUnhappiness":
                        return (M_STAT, S_NOOCCUPIED, amount, 0, 0, 0)
                    if soc in PLAYER_STATS:
                        return (M_STAT, 100 + PLAYER_STATS.index(soc), amount, 0, 0, 0)
                if R.tag(inner) == "PopulationUnhappinessPercent":
                    return (M_STAT, S_POPUNHAPPY, amount, 0, 0, 0)
                if R.tag(inner) in PLAYER_STATS:
                    return (M_STAT, 100 + PLAYER_STATS.index(R.tag(inner)), amount, 0, 0, 0)
            return (M_NONE, 0, 0, 0, 0, 0)  # a stat of another system
        if t == "ModifyCost":
            target = R.tag(b["target"])
            if target in COST_INDEX:
                return (M_COST, COST_INDEX[target], b["percent"], 0, 0, 0)
            return (M_NONE, 0, 0, 0, 0, 0)
        if t == "MultiplyYield":
            raise Unsupported("MultiplyYield")
        return (M_NONE, 0, 0, 0, 0, 0)  # combat, happiness, great people, tourism: other systems

    def parse(self, text, bucket):
        e = R.body(R.parse(text))
        if R.tag(e["trigger"]) != "Passive":
            return None
        sel = R.tag(e["selector"])
        if sel == "Module":
            sel = R.tag(R.body(e["selector"])[0])
        return {"sel": sel, "unique": R.tag(e["stacking"]) == "Unique", "c": [self.cond(e["condition"]), self.cond(e["second_condition"])],
                "m": self.modifier(e["modifier"])}


CLASSES = {}


def resource_class(name):
    return CLASSES.setdefault(name, len(CLASSES))


def item_of(text, nu):
    """`Building(BuildingKindId(3))` -> nu + 3; `Unit(UnitKindId(7))` -> 7; a project -> −2."""
    v = R.parse(text)
    t = R.tag(v)
    if t == "Unit":
        return R.arg(R.body(v)[0])
    if t == "Building":
        return nu + R.arg(R.body(v)[0])
    return -2


def handicap_fields(text):
    return R.body(R.parse(text))


def filter_matches(er, f, item):
    """`production::item_matches` for an item index (units first, then buildings)."""
    nu = len(er["units"])
    t, b = R.tag(f), R.body(f)
    if item < 0:
        return False
    if item < nu:
        u = er["units"][item]
        role = R.arg(R.parse(u["role"]))
        domain = u["domain"]
        if t == "Units":
            return True
        if t == "UnitDomain":
            return domain == R.tag(b[0])
        if t == "UnitRole":
            return role == R.tag(b[0])
        if t == "UnitClassOf":
            c = er["units"][R.arg(b[0])]["class"]
            return c is not None and c == u["class"]
        if t == "MilitaryUnits":
            return role is not None and isinstance(role, str) and role not in ("Civilian", "SpaceshipPart")
        if t == "SpaceshipParts":
            return role == "SpaceshipPart"
        return False
    bd = er["buildings"][item - nu]
    if t == "Buildings":
        return True
    if t == "WorldWonders":
        before = R.arg(b["before_era"])
        if bd["kind"] != "WorldWonder":
            return False
        if before is None:
            return True
        return bd["era"] < er["eras"][before] if bd["era"] >= 0 else True
    if t == "BuildingClassOf":
        c = er["buildings"][R.arg(b[0])]["class"]
        return c is not None and c == bd["class"]
    return False


class Model:
    """The rules as MLX tables, once per dump (the same content in every game)."""

    def __init__(self, tables, er):
        self.er = er
        self.tb = econ.Tables(tables)
        self.rows = Rows(er)
        self.nb, self.nu = len(er["buildings"]), len(er["units"])
        self.ns = len(er["specialists"])
        self.default_spec = er["default_specialist"]
        b = er["buildings"]
        slots = [[0] * self.ns for _ in range(self.nb)]
        for i, d in enumerate(b):
            if d["slots"]:
                slots[i][d["slots"][0]] += d["slots"][1]
        self.slots = mx.array(slots, dtype=I32)  # [NB, S]
        self.wonder = mx.array([d["wonder"] for d in b])
        self.spec_yields = mx.array([s["yields"] for s in er["specialists"]], dtype=I32)
        self.spec_points = mx.array([s["points"] for s in er["specialists"]], dtype=I32)
        # A world wonder's own flat culture (`wonder_culture_bonus`).
        own = [0] * self.nb
        for src, sid, bucket, text in er["effects"]:
            if src != "building" or bucket != "city" or not b[sid]["wonder"]:
                continue
            e = R.body(R.parse(text))
            m = e["modifier"]
            if R.tag(e["selector"]) == "SourceCity" and R.tag(e["condition"]) == "Always" and R.tag(m) == "AddYield" \
                    and R.tag(R.body(m)["yield_kind"]) == "Culture":
                own[sid] += R.body(m)["amount"]
        self.wonder_culture = mx.array(own, dtype=I32)
        self.speed = R.body(R.parse(er["speed"]))
        self.res_wonder = [r["wonder"] for r in er["resources"]]
        self.res_class = [resource_class(R.tag(R.arg(R.parse(r["class"])))) if R.arg(R.parse(r["class"])) is not None else -1
                          for r in er["resources"]]

    def tables(self):
        """Row arrays (after every extra row is known)."""
        def arr(rows):
            src_kind = {"nation": 0, "tech": 1, "policy": 2, "local": 3, "wide": 4, "extra": 5, "none": 6}
            n = max(1, len(rows))
            pad = rows + [{"src": ("none", 0), "sel": "none", "unique": True, "c": [(C_NEVER, 0, 0)] * 2, "m": (M_NONE,) * 6}] * (n - len(rows))
            return {
                "src": mx.array([src_kind[r["src"][0]] for r in pad], dtype=I32),
                "sid": mx.array([r["src"][1] for r in pad], dtype=I32),
                "capital": mx.array([r["sel"] == "Capital" for r in pad]),
                "unique": mx.array([bool(r.get("unique", True)) for r in pad]),
                "c": mx.array([[list(c) for c in r["c"]] for r in pad], dtype=I32),  # [R, 2, 3]
                "m": mx.array([list(r["m"]) for r in pad], dtype=I32),  # [R, 6]
            }
        self.city_rows, self.tile_rows = arr(self.rows.city), arr(self.rows.tile)
        nf = max(1, len(self.rows.filters))
        nitems = self.nu + self.nb
        self.match = mx.array([[filter_matches(self.er, f, it) for it in range(nitems)] + [False] for _, f in self.rows.filters]
                              if self.rows.filters else [[False] * (nitems + 1)])  # [F, items + 1] (last: no item)
        del nf


def disc(w, h, centre):
    q0, r0 = centre % w, centre // w
    out = []
    for dq in range(-RADIUS, RADIUS + 1):
        for dr in range(max(-RADIUS, -dq - RADIUS), min(RADIUS, RADIUS - dq) + 1):
            q, r = q0 + dq, r0 + dr
            if 0 <= q < w and 0 <= r < h:
                out.append(r * w + q)
    return sorted(out)


def split_dump(path, out_dir):
    """One file per game (the games of a multi-threaded dump interleave), so that a run holds one game
    at a time: `out_dir/<game>.jsonl`."""
    import os
    os.makedirs(out_dir, exist_ok=True)
    files = {}
    with open(path) as f:
        for line in f:
            at = line.find('"game":"')
            if at < 0:
                continue
            g = line[at + 8: line.index('"', at + 8)]
            if g not in files:
                files[g] = open(os.path.join(out_dir, g + ".jsonl"), "w")
            files[g].write(line)
    for fh in files.values():
        fh.close()
    return sorted(files)


def each_game(path, limit_games=None):
    """(name, game) one at a time: from a directory of per-game files (`split_dump`), or a dump file."""
    import os
    if os.path.isdir(path):
        names = sorted(n for n in os.listdir(path) if n.endswith(".jsonl"))
        for n in names[:limit_games] if limit_games else names:
            yield from games_of(os.path.join(path, n)).items()
    else:
        yield from games_of(path, limit_games).items()


def games_of(path, limit_games=None):
    """Per game: tables, econ_rules and its turn lines with the dynamic planes before (and after)
    each end of turn, reconstructed from the round lines and the turn diffs."""
    ctx = {}
    with open(path) as f:
        for line in f:
            try:
                d = json.loads(line)
            except json.JSONDecodeError:
                continue  # a line still being written
            if not isinstance(d, dict):
                continue
            t = d["t"]
            g = d.get("game")
            if t == "tables":
                if limit_games and len(ctx) >= limit_games:
                    continue
                ctx[g] = {"tables": d, "planes": None, "turns": []}
            elif g not in ctx:
                continue
            elif t == "econ_rules":
                ctx[g]["rules"] = d
            elif t == "round":
                ctx[g]["planes"] = [list(p) for p in zip(d["feature"], d["resource"], d["improvement"], [int(x) for x in d["pillaged"]],
                                                          d["owner"], d["ocity"], d["cityh"])]
            elif t == "turn":
                c = ctx[g]
                for tile, *p in d["pre"]["tiles"]:
                    c["planes"][tile] = p
                pre = [list(col) for col in zip(*c["planes"])]
                for tile, *p in d["post"]["tiles"]:
                    c["planes"][tile] = p
                d["pre_planes"] = pre
                d["post_planes"] = [list(col) for col in zip(*c["planes"])]
                c["turns"].append(d)
    return ctx


def tech_bits(s):
    return [ch == "1" for ch in s]


class Batch:
    """City rows of a list of turn lines of one game (pre-states)."""

    def __init__(self, model, game, turns, side="pre", mid=False):
        """`mid`: the inputs a turn's city step reads that change before it (the happiness percent,
        the golden age, the capital connection, the trade routes) from the turn's `post` side."""
        er, tables = game["rules"], game["tables"]
        w, h = tables["map_w"], tables["map_h"]
        nb, nu, ns = model.nb, model.nu, model.ns
        rows = model.rows
        players = er["players"]
        self.cities = []  # (turn index, city dict)
        cols = {k: [] for k in ("pop", "capital", "coastal", "river", "connected", "garrison", "golden", "peace", "resist", "avoid", "head",
                                "capbuilt", "csfood", "gw", "after", "counts", "techs", "policies", "nation", "bld", "wide", "cand", "landmark",
                                "locked", "forced", "focus", "row", "unhappy", "owner_cities", "ai", "era", "handicap", "wonder_res",
                                "centre", "unworked")}
        x_city, x_tile = [], []  # (city row, extra index, times)
        for ti, t in enumerate(turns):
            s = t[side]["state"]
            p = t["player"]
            inputs_state = t["post"]["state"] if mid else s
            inputs_cities = {c["h"]: c for c in inputs_state["cities"]}
            techs = tech_bits(s["techs"])
            majors = [x["major"] for x in players]
            peace = not any(s["war"][o] and majors[o] and s["alive"][o] and o != p for o in range(len(players)))
            counts_b = [0] * nb
            capital_buildings = set()
            for c in s["cities"]:
                for b in c["buildings"]:
                    counts_b[b] += 1
                if c["capital"]:
                    capital_buildings |= set(c["buildings"])
            # The seat's handicap and AI flag as the turn reads them (the game's settings at play).
            hc = handicap_fields(s.get("handicap", players[p]["handicap"]))
            is_ai = s.get("ai", players[p]["ai"])
            for c in s["cities"]:
                if "_only" in t and c["h"] != t["_only"]:
                    continue
                ic = dict(c["inputs"])
                if mid and c["h"] in inputs_cities:
                    for key in MID_KEYS:
                        ic[key] = inputs_cities[c["h"]]["inputs"][key]
                r = len(self.cities)
                self.cities.append((ti, c))
                cols["row"].append(ti)
                cols["pop"].append(c["pop"])
                cols["capital"].append(c["capital"])
                cols["coastal"].append(c["coastal"])
                cols["river"].append(c["river"])
                cols["connected"].append(ic["connected"])
                cols["garrison"].append(ic["garrison"])
                # The turn's golden age and unhappiness penalty as the city step reads them: MLX's
                # own (`turn.transition` sets them from the happiness and golden age steps), else the
                # dump's side.
                golden = t.get("_golden", inputs_state["golden_age"])
                cols["golden"].append(golden)
                cols["unhappy"].append(t.get("_unhappy", inputs_state["unhappy_percent"]))
                cols["peace"].append(peace)
                cols["resist"].append(c["resist"] > 0)
                cols["avoid"].append(c["avoid"])
                head = item_of(c["queue"][0], nu) if c["queue"] else -1  # −2: a project (not in MLX yet)
                cols["head"].append(head)
                cols["capbuilt"].append(head >= nu and (head - nu) in capital_buildings)
                cols["csfood"].append(ic["base"][0])
                cols["gw"].append(ic["great_works"])
                cols["after"].append(ic["after"])
                k = ic["counts"]
                pk = s.get("player_counts", {})
                cols["counts"].append([0, 0, k["MajorityFollowers"], k["ReligionsPresent"], k["TradePartners"], k["CityStateRoutes"],
                                       len(s["policies"]), pk.get("PeacefulForeignFollowers", 0), pk.get("ConnectedCities", 0)])
                cols["techs"].append(techs)
                cols["policies"].append(s["policies"])
                cols["nation"].append(players[p]["nation"])
                bm = [False] * nb
                for b in c["buildings"]:
                    bm[b] = True
                cols["bld"].append(bm)
                cols["wide"].append(counts_b)
                cols["owner_cities"].append(len(s["cities"]))
                cols["ai"].append(is_ai)
                cols["era"].append(s["era"])
                cols["handicap"].append([hc["ai_train_percent"], hc["ai_construct_percent"], hc["ai_world_construct_percent"],
                                         hc["ai_create_percent"], hc["ai_per_era_modifier"], hc["ai_growth_percent"]])
                cols["wonder_res"].append(ic["wonder_res"])
                tiles = disc(w, h, c["tile"])
                cols["cand"].append(tiles + [-1] * (NCAND - len(tiles)))
                lm = dict((tt, v) for tt, v in ic["landmarks"])
                cols["landmark"].append([lm.get(tt, 0) for tt in tiles] + [0] * (NCAND - len(tiles)))
                locked = set(c["locked"])
                cols["locked"].append([tt in locked for tt in tiles] + [False] * (NCAND - len(tiles)))
                fs = [0] * ns
                for kind, n in c["specs"]:
                    fs[kind] += n
                cols["forced"].append(fs)
                f = FOCUS[c["focus"]]
                wv = list(WEIGHTS)
                if f is not None:
                    wv[f] *= 3
                cols["focus"].append(wv)
                cols["centre"].append(c["tile"])
                cols["unworked"].append(set(c["unworked"]))
                for text, n in ic["beliefs_city"] + ic["timed"]:
                    ri, xi = rows.add_extra("city", text)
                    x_city.append((r, xi, n))
                for text, n in ic["beliefs_tile"]:
                    ri, xi = rows.add_extra("tile", text)
                    x_tile.append((r, xi, n))
        self.n = len(self.cities)
        self.cols = cols
        self.x_city, self.x_tile = x_city, x_tile
        self.w, self.h = w, h
        self.turns = turns
        self.side = side


def evaluate(model, game, batch):
    """MLX: the yields of every city of the batch. Returns a dict of arrays."""
    if batch.n == 0:
        return None
    model.tables()
    er = model.er
    cols = batch.cols
    C = batch.n
    nb, nu, ns = model.nb, model.nu, model.ns
    tb = model.tb
    tables = game["tables"]
    w, h = batch.w, batch.h
    T = w * h
    # --- the plot yields of every tile for the turn's player, whole map, per turn line.
    turns = batch.turns
    B = len(turns)
    st = {k: mx.broadcast_to((mx.array(tables["tiles"][k], dtype=I32) if k in ("kind", "wonder") else mx.array(tables["tiles"][k]))[None], (B, T))
          for k in ("kind", "mountain", "water", "lake", "river", "wonder")}
    planes = [t["pre_planes"] if batch.side == "pre" else t["post_planes"] for t in turns]
    feature = mx.array([p[0] for p in planes], dtype=I32)
    resource = mx.array([p[1] for p in planes], dtype=I32)
    improvement = mx.array([p[2] for p in planes], dtype=I32)
    pillaged = mx.array([p[3] for p in planes]) > 0
    ocity = mx.array([p[5] for p in planes], dtype=I32)
    st.update({"feature": feature, "resource": resource, "improvement": improvement, "pillaged": pillaged,
               "owner": mx.full((B, T), -1, dtype=I32), "city": mx.zeros((B, T), dtype=mx.bool_),
               "techs": mx.array([[tech_bits(t[batch.side]["state"]["techs"])] for t in turns]), "viewer": mx.zeros((B,), dtype=I32)})
    nbr = econ.neighbours(w, h)
    raw = econ.plot_yields(tb, nbr, st, finish=False)  # [B, T, 6]
    # --- per city gathers.
    row = mx.array(cols["row"], dtype=I32)
    cand = mx.array(cols["cand"], dtype=I32)  # [C, K]
    on = cand >= 0
    cidx = mx.maximum(cand, 0)
    flat_idx = (row[:, None] * T + cidx).reshape(-1)

    def per_cand(a):
        return mx.take(a.reshape(B * T, *a.shape[2:]), flat_idx, axis=0).reshape(C, NCAND, *a.shape[2:])

    craw = per_cand(raw)
    c_feature, c_resource, c_imp, c_pill = per_cand(feature), per_cand(resource), per_cand(improvement), per_cand(pillaged)
    c_ocity = per_cand(ocity)
    terrain = mx.array(game["rules"]["terrain"], dtype=I32)  # per game (the map)
    c_terrain = mx.take(terrain, cidx)
    c_river = mx.take(mx.array(tables["tiles"]["river"]), cidx)
    c_water = mx.take(mx.array(tables["tiles"]["water"]), cidx)
    c_lake = mx.take(mx.array(tables["tiles"]["lake"]), cidx)
    c_wonder = mx.take(mx.array(tables["tiles"]["wonder"], dtype=I32), cidx)
    centre = mx.array(cols["centre"], dtype=I32)
    handle = mx.array([c["h"] for _, c in batch.cities], dtype=I32)
    is_centre = cand == centre[:, None]
    unworked = mx.array([[tt in uw for tt in cs] for cs, uw in zip(cols["cand"], cols["unworked"])])
    workable = on & ~is_centre & (c_ocity == handle[:, None]) & ~unworked
    techs = mx.array(cols["techs"])  # [C, NT]
    NT = techs.shape[1]
    # The resource as the owner sees it (its revealing technology known).
    r_tech = mx.take(tb.r_tech, mx.where(c_resource < 0, tb.nr, c_resource))
    seen_r = (c_resource >= 0) & ((r_tech < 0) | mx.take_along_axis(techs, mx.maximum(r_tech, 0), axis=1))
    res_class = mx.array(model.res_class + [-1], dtype=I32)
    c_class = mx.where(seen_r, mx.take(res_class, mx.where(c_resource < 0, tb.nr, c_resource)), -1)
    working_imp = mx.where(c_pill, -1, c_imp)
    # --- the city's masks.
    pop = mx.array(cols["pop"], dtype=I32)
    capital = mx.array(cols["capital"])
    bld = mx.array(cols["bld"])  # [C, NB]
    wide = mx.array(cols["wide"], dtype=I32)
    npol = 1 + max([max(p) for p in cols["policies"] if p] + [r["src"][1] for r in model.rows.city + model.rows.tile if r["src"][0] == "policy"] + [0])
    pol = mx.array([[i in set(p) for i in range(npol)] for p in cols["policies"]])
    nation = mx.array(cols["nation"], dtype=I32)
    NX = max(1, len(model.rows.extra))
    xc = [[0] * NX for _ in range(C)]
    for r, xi, n in batch.x_city:
        xc[r][xi] += n
    xt = [[0] * NX for _ in range(C)]
    for r, xi, n in batch.x_tile:
        xt[r][xi] += n
    xc, xt = mx.array(xc, dtype=I32), mx.array(xt, dtype=I32)
    head = mx.array(cols["head"], dtype=I32)
    counts = mx.array(cols["counts"], dtype=I32)
    world_wonders = mx.sum(bld & model.wonder[None], axis=1).astype(I32)
    counts = mx.concatenate([pop[:, None], world_wonders[:, None], counts[:, 2:]], axis=1)  # [C, 9] (`COUNTS`)
    NCOUNT = counts.shape[1]
    flags = {C_GARRISON: mx.array(cols["garrison"]), C_COASTAL: mx.array(cols["coastal"]), C_RIVER: mx.array(cols["river"]),
             C_PEACE: mx.array(cols["peace"]), C_CONNECTED: mx.array(cols["connected"]), C_GOLDEN: mx.array(cols["golden"]),
             C_CAPBUILT: mx.array(cols["capbuilt"])}
    # Unimproved features next to the centre (CityAdjacentUnimprovedFeature).
    cn = mx.take(nbr, centre, axis=0)  # [C, 6]
    cn_on = cn >= 0
    cn_flat = (row[:, None] * T + mx.maximum(cn, 0)).reshape(-1)
    n_feature = mx.take(feature.reshape(-1), cn_flat).reshape(C, 6)
    n_imp = mx.take(improvement.reshape(-1), cn_flat).reshape(C, 6)

    def instances(rt, extra):
        """[C, R]: instances of each row (source and capital gate)."""
        src, sid = rt["src"], rt["sid"]
        n_rows = src.shape[0]
        t_nation = (nation[:, None] == sid[None]).astype(I32)
        t_tech = mx.take(techs.astype(I32), mx.clip(sid, 0, NT - 1), axis=1)
        t_pol = mx.take(pol.astype(I32), mx.clip(sid, 0, pol.shape[1] - 1), axis=1)
        t_local = mx.take(bld.astype(I32), mx.clip(sid, 0, nb - 1), axis=1)
        cnt = mx.take(wide, mx.clip(sid, 0, nb - 1), axis=1)
        t_wide = mx.where(rt["unique"][None], mx.minimum(cnt, 1), cnt)
        t_extra = mx.take(extra, mx.clip(sid, 0, NX - 1), axis=1)
        out = mx.zeros((C, n_rows), dtype=I32)
        for k, v in ((0, t_nation), (1, t_tech), (2, t_pol), (3, t_local), (4, t_wide), (5, t_extra)):
            out = mx.where(src[None] == k, v, out)
        return mx.where(rt["capital"][None] & ~capital[:, None], 0, out)

    def city_cond(c):
        """[C, R] bool for the conditions c [R, 3] (city scope)."""
        kind, a, b = c[:, 0][None], c[:, 1][None], c[:, 2][None]
        out = kind == C_ALWAYS
        out = out | ((kind == C_TECH) & mx.take(techs, mx.clip(a[0], 0, NT - 1), axis=1))
        out = out | ((kind == C_NOTECH) & ~mx.take(techs, mx.clip(a[0], 0, NT - 1), axis=1))
        out = out | ((kind == C_MINPOP) & (pop[:, None] >= a))
        out = out | ((kind == C_BUILDING) & mx.take(bld, mx.clip(a[0], 0, nb - 1), axis=1))
        hi = mx.where(head < 0, model.match.shape[1] - 1, head)
        prod = mx.take(model.match, mx.clip(a[0], 0, model.match.shape[0] - 1), axis=0)  # [R, items+1]
        out = out | ((kind == C_PRODUCING) & mx.take(prod, hi, axis=1).T)
        for code, flag in flags.items():
            out = out | ((kind == code) & flag[:, None])
        out = out | ((kind == C_FOLLOWERS) & (counts[:, 2:3] >= a))
        adj = mx.sum(cn_on[:, None, :] & (n_feature[:, None, :] == a[..., None]) & (n_imp[:, None, :] < 0), axis=2)
        out = out | ((kind == C_ADJFEATURE) & (adj >= b))
        return out

    happy_parts = []

    def amounts(m, inst):
        """Per row its contribution to (flat [C, 6], percent [C, 6]) and the scalars."""
        kind, ch, amt, k, every, mxv = (m[:, i][None] for i in range(6))
        n = inst
        cnt = mx.take(counts, mx.clip(k[0], 0, NCOUNT - 1), axis=1)  # [C, R]
        times = cnt // mx.maximum(every, 1)
        scaled = amt * times
        scaled = mx.where((mxv >= 0) & (mx.abs(scaled) > mxv), mx.where(scaled < 0, -mxv, mxv), scaled)
        # Rust caps with `max` itself (sign of the content's max); the content's max is positive.
        perpop = trunc_div(amt * n * pop[:, None], 100)
        flat_v = mx.where(kind == M_FLAT, amt * n, 0) + mx.where(kind == M_PERPOP, perpop, 0) + mx.where(kind == M_FLAT_COUNT, scaled * n, 0)
        pct_v = mx.where(kind == M_PCT, amt * n, 0) + mx.where(kind == M_PCT_COUNT, scaled * n, 0)
        happy_v = mx.where(kind == M_HAPPY, amt * n, 0) + mx.where(kind == M_HAPPY_COUNT, scaled * n, 0)
        happy_parts.append(mx.sum(happy_v, axis=1))
        chs = mx.arange(NY)[None, None, :] == ch[..., None]
        flat = mx.sum(mx.where(chs, flat_v[..., None], 0), axis=1)
        pct = mx.sum(mx.where(chs, pct_v[..., None], 0), axis=1)
        return flat, pct, kind, ch, amt * n, k, every

    # --- City-scope effects.
    rt = model.city_rows
    inst = instances(rt, xc)
    holds = city_cond(rt["c"][:, 0]) & city_cond(rt["c"][:, 1])
    inst = mx.where(holds, inst, 0)
    flat, pct, mkind, mch, mamt, mk, mevery = amounts(rt["m"], inst)
    growth_percent = mx.sum(mx.where(mkind == M_GROWTH, mamt, 0), axis=1)
    city_happiness = happy_parts[0]
    stat = mx.where(mkind == M_STAT, mch, 0)  # the stat code sits in the channel column
    food_kept = mx.sum(mx.where(stat == S_FOODKEPT, mamt, 0), axis=1)
    spec_food_pct = mx.sum(mx.where(stat == S_SPECFOOD, mamt, 0), axis=1)
    pop_unhappy_pct = mx.sum(mx.where(stat == S_POPUNHAPPY, mamt, 0), axis=1)
    spec_unhappy_pct = mx.sum(mx.where(stat == S_SPECUNHAPPY, mamt, 0), axis=1)
    no_occupied = mx.sum(mx.where(stat == S_NOOCCUPIED, mamt, 0), axis=1)
    wonder_pct = mx.sum(mx.where(stat == S_WONDERCULTURE, mamt, 0), axis=1)
    any_spec = mx.sum(mx.where((stat == S_ANYSPEC)[..., None] & (mx.arange(NY)[None, None] == mk[..., None]), mamt[..., None], 0), axis=1)
    # SpecialistYield: (kind, channel): the column `every` holds the specialist (−1: all but the default).
    spec_kind = mevery
    spec_add = []
    for s in range(ns):
        applies = (spec_kind == s) | ((spec_kind == -1) & (s != (model.default_spec if model.default_spec is not None else -99)))
        sel = (stat == S_SPEC) & applies
        spec_add.append(mx.sum(mx.where(sel[..., None] & (mx.arange(NY)[None, None] == mk[..., None]), mamt[..., None], 0), axis=1))
    spec_table = model.spec_yields[None] + mx.stack(spec_add, axis=1)  # [C, S, 6]
    cost_pct = [mx.sum(mx.where((mkind == M_COST) & (mch == i), mamt, 0), axis=1) for i in range(len(COST_INDEX))]
    prod_item_pct = mx.sum(mx.where((mkind == M_PCT) & (mch == 1) & ((rt["c"][:, 0, 0] == C_PRODUCING) | (rt["c"][:, 0, 0] == C_CAPBUILT))[None],
                                    mamt, 0), axis=1)
    # Marble: a wonder resource of the city's territory speeds world wonders.
    res_pct = []
    for ci, (h_, wr) in enumerate(zip(cols["head"], cols["wonder_res"])):
        v = 0
        if h_ >= nu:
            bd = er["buildings"][h_ - nu]
            if bd["kind"] == "WorldWonder":
                for r_ in er["wonder_resources"]:
                    bonus = model.res_wonder[r_]
                    if bonus is None or r_ not in wr:
                        continue
                    before = bonus[1]
                    if before < 0 or bd["era"] < 0 or bd["era"] < before:
                        v += bonus[0]
        res_pct.append(v)
    res_pct = mx.array(res_pct, dtype=I32)
    pct = pct + mx.where(mx.arange(NY)[None] == 1, res_pct[:, None], 0)
    item_pct = prod_item_pct + res_pct
    # --- Tile-scope effects on the candidate tiles.
    tr = model.tile_rows
    tinst = instances(tr, xt)  # [C, Rt]

    def tile_cond(c):
        kind, a = c[:, 0][None, None], c[:, 1][None, None]
        out = kind == C_ALWAYS
        out = out | ((kind == T_IMPROVEMENT) & (working_imp[..., None] == a))
        out = out | ((kind == T_RESOURCE) & (c_resource[..., None] == a))
        out = out | ((kind == T_FEATURE) & (c_feature[..., None] == a))
        out = out | ((kind == T_TERRAIN) & (c_terrain[..., None] == a))
        out = out | ((kind == T_FEATURE_UNIMPROVED) & (c_feature[..., None] == a) & (c_imp[..., None] < 0))
        out = out | ((kind == T_CLASS) & (c_class[..., None] == a))
        plot = mx.stack([c_river, c_water & ~c_lake, c_lake, c_water & (c_class >= 0), c_wonder >= 0], axis=-1)  # [C, K, 5]
        out = out | ((kind == T_PLOT) & mx.take_along_axis(plot, mx.broadcast_to(mx.clip(a, 0, 4), (C, NCAND, a.shape[2])), axis=2))
        # City-scope conditions of a tile effect (a belief's second condition, the golden age).
        out = out | ((kind < 20) & (kind != C_ALWAYS) & city_cond(c)[:, None, :])
        return out

    th = tile_cond(tr["c"][:, 0]) & tile_cond(tr["c"][:, 1])  # [C, K, Rt]
    tn = mx.where(th, tinst[:, None, :], 0)
    tk, tch, tamt = tr["m"][:, 0], tr["m"][:, 1], tr["m"][:, 2]
    chs = mx.arange(NY)[None, :] == tch[:, None]  # [Rt, 6]
    t_flat = mx.sum(mx.where(((tk == M_FLAT)[:, None] & chs)[None, None], (tn * tamt[None, None])[..., None], 0), axis=2)  # [C, K, 6]
    t_pct = mx.sum(mx.where(((tk == M_PCT)[:, None] & chs)[None, None], (tn * tamt[None, None])[..., None], 0), axis=2)
    y = trunc_div((craw + t_flat) * (100 + t_pct), 100)
    y = y + mx.where(mx.arange(NY)[None, None] == 4, mx.array(cols["landmark"], dtype=I32)[..., None], 0)
    y = mx.where(is_centre[..., None], mx.maximum(y, mx.array(econ.CITY_MIN, dtype=I32)), y)
    y = mx.maximum(y, 0)
    golden = mx.array(cols["golden"])
    y = y + mx.where((mx.arange(NY)[None, None] == 2) & golden[:, None, None] & (y[..., 2:3] >= 1), 1, 0)
    # --- citizens.
    wts = mx.array(cols["focus"], dtype=I32)  # [C, 7]
    slots = mx.sum(mx.where(bld[:, :, None], model.slots[None], 0), axis=1)  # [C, S]
    forced = mx.array(cols["forced"], dtype=I32)
    citizens = pop
    spec_n = mx.zeros((C, ns), dtype=I32)
    for s in range(ns):
        n = mx.minimum(mx.minimum(forced[:, s], slots[:, s]), citizens)
        spec_n = spec_n + mx.where(mx.arange(ns)[None] == s, n[:, None], 0)
        slots = slots - mx.where(mx.arange(ns)[None] == s, n[:, None], 0)
        citizens = citizens - n
    locked = mx.array(cols["locked"]) & workable
    rank = mx.cumsum(locked.astype(I32), axis=1)
    take_locked = locked & (rank <= citizens[:, None])
    citizens = citizens - mx.sum(take_locked.astype(I32), axis=1)
    taken = take_locked | is_centre
    avoid = mx.array(cols["avoid"])
    food_needed = FOOD_PER_CITIZEN * pop + mx.where(avoid, 0, FOOD_PER_CITIZEN)
    pos = mx.arange(NCAND)[None]
    default = model.default_spec
    kinds = mx.arange(ns)[None]
    spec_score_base = mx.sum(spec_table * wts[:, None, :6], axis=2) + model.spec_points[None] * wts[:, 6:7]  # [C, S] not hungry
    spec_food = spec_table[..., 0]
    tile_score_base = mx.sum(y * wts[:, None, :6], axis=2)
    steps = int(mx.max(citizens).item()) if C else 0
    worked_mask = taken
    for step in range(steps):
        active = citizens > step
        food_now = mx.sum(mx.where(worked_mask, y[..., 0], 0), axis=1) + mx.sum(spec_n * spec_food, axis=1)
        hungry = food_now < food_needed
        ts = tile_score_base + mx.where(hungry[:, None], y[..., 0] * wts[:, 0:1], 0)
        free = workable & ~worked_mask
        tkey = mx.where(free, ts * 64 + (63 - pos), -(2 ** 30))
        bt = mx.argmax(tkey, axis=1)
        has_tile = mx.any(free, axis=1)
        bts = mx.take_along_axis(ts, bt[:, None], axis=1)[:, 0]
        avail = (slots > 0) | (kinds == (default if default is not None else -1))
        ss = spec_score_base + mx.where(hungry[:, None], spec_food * wts[:, 0:1], 0)
        skey = mx.where(avail, ss * 16 + (15 - kinds), -(2 ** 30))
        bs = mx.argmax(skey, axis=1)
        has_spec = mx.any(avail, axis=1)
        bss = mx.take_along_axis(ss, bs[:, None], axis=1)[:, 0]
        pick_spec = active & has_spec & (~has_tile | (bss > bts))
        pick_tile = active & has_tile & ~pick_spec
        worked_mask = worked_mask | (pick_tile[:, None] & (pos == bt[:, None]))
        one = pick_spec[:, None] & (kinds == bs[:, None])
        spec_n = spec_n + one.astype(I32)
        slots = slots - (one & (kinds != (default if default is not None else -1))).astype(I32)
    tile_yields = mx.sum(mx.where(worked_mask[..., None], y, 0), axis=1)
    spec_yields = mx.sum(spec_n[..., None] * spec_table, axis=1)
    base = tile_yields + spec_yields
    has_spec = mx.sum(spec_n, axis=1) > 0
    base = base + mx.where(has_spec[:, None], any_spec, 0)
    # The module's base: city-state food, a science per citizen, great works, wonders' culture.
    wonder_bonus = trunc_div(mx.sum(mx.where(bld, model.wonder_culture[None], 0), axis=1) * wonder_pct, 100)
    gw = mx.array(cols["gw"], dtype=I32)
    module_base = mx.stack([mx.array(cols["csfood"], dtype=I32), mx.zeros((C,), dtype=I32), mx.zeros((C,), dtype=I32), pop,
                            gw + wonder_bonus, mx.zeros((C,), dtype=I32)], axis=1)
    base = base + module_base
    unhappy = mx.array(cols["unhappy"], dtype=I32)
    emp_pct = mx.where(golden, GOLDEN_AGE_PRODUCTION_PERCENT + unhappy, unhappy)
    pct_all = pct + mx.where(mx.arange(NY)[None] == 1, emp_pct[:, None], 0)
    after = mx.array(cols["after"], dtype=I32)
    hundredths = (base + flat) * (100 + pct_all)
    rates = mx.maximum(hundredths, 0) // 100 + after
    resist = mx.array(cols["resist"])
    rates = mx.where(resist[:, None], 0, rates)
    base_out = mx.where(resist[:, None], 0, base)
    # --- the production cost of the head of the queue.
    head_cost = production_cost(model, cols, head, cost_pct)
    return {"worked": worked_mask, "cand": cand, "y": y, "spec_n": spec_n, "tile_yields": tile_yields, "spec_yields": spec_yields,
            "base": base_out, "rates": rates, "growth_percent": growth_percent, "item_percent": item_pct, "food_kept_stat": food_kept,
            "spec_food_pct": spec_food_pct, "head_cost": head_cost, "flat": flat, "pct": pct_all, "cost_pct": mx.stack(cost_pct, axis=1), "raw": raw,
            "happiness": city_happiness, "pop_unhappy_pct": pop_unhappy_pct, "spec_unhappy_pct": spec_unhappy_pct, "no_occupied": no_occupied}


def trunc_div(a, b):
    """Integer division toward zero (Rust), for b > 0."""
    q = mx.abs(a) // b
    return mx.where(a < 0, -q, q)


def production_cost(model, cols, head, cost_pct):
    """`CityModifiers::production_cost` of each city's head (−1 without one)."""
    er = model.er
    nu = model.nu
    sp = model.speed["production_percent"]
    out = []
    for ci, h in enumerate(cols["head"]):
        if h < 0:
            out.append(-1)
            continue
        hc = cols["handicap"][ci]
        era_pct = max(hc[4] * cols["era"][ci] + 100, 0)

        def scale(p):
            return max(max(p, 0) * era_pct // 100, 1)

        def handicap(cost, which):
            if sp != 100:
                cost = max(cost * sp // 100, 1)
            if cols["ai"][ci]:
                cost = max(cost * scale(hc[which]) // 100, 1)
            return cost

        if h < nu:
            d = er["units"][h]
            if d["founder"]:
                base = d["cost"] if sp == 100 else max(d["cost"] * sp // 100, 1)
            else:
                base = handicap(d["cost"], 0)
            target = 0
        else:
            d = er["buildings"][h - nu]
            cost = d["cost"] + max(d["cpc"], 0) * cols["owner_cities"][ci]
            base = handicap(cost, 2 if d["wonder"] else 1)
            target = 1
        out.append(base)
    base = mx.array(out, dtype=I32)
    target = mx.where(head >= nu, 1, 0)
    p = mx.where(target == 1, cost_pct[1], cost_pct[0])
    cost = mx.maximum(base * (100 + p), 100) // 100
    return mx.where(head < 0, -1, cost)


def compare(model, game, batch, got):
    """Per city: equal on every part of the breakdown?"""
    worked = got["worked"].tolist()
    cand = got["cand"].tolist()
    yv = got["y"].tolist()
    arrs = {k: got[k].tolist() for k in ("spec_n", "tile_yields", "spec_yields", "base", "rates", "growth_percent", "item_percent", "head_cost")}
    res = []
    for ci, (ti, c) in enumerate(batch.cities):
        r = c["rust"]
        mine = sorted((cand[ci][k], yv[ci][k]) for k in range(NCAND) if worked[ci][k])
        theirs = sorted((t, y) for t, y in r["worked"])
        parts = {
            "worked": mine == theirs,
            "specialists": arrs["spec_n"][ci] == r["specialists"][:model.ns],
            "base": arrs["base"][ci] == r["base"],
            "rates": arrs["rates"][ci] == r["rates"],
            "growth": arrs["growth_percent"][ci] == r["growth_percent"],
            "item": arrs["item_percent"][ci] == r["item_percent"],
            "cost": arrs["head_cost"][ci] == (r["head_cost"] if r["head_cost"] is not None else -1),
        }
        res.append((ti, c, parts, mine, theirs, {k: arrs[k][ci] for k in arrs}))
    return res


def run(path, limit_games=None, chunk=128, show=4):
    t0 = time.time()
    ctx = each_game(path, limit_games)
    t_load = 0.0
    n_games = 0
    model = None
    tot = {"cities": 0, "ok": 0}
    parts_ok = {}
    examples = []
    t_gpu = 0.0
    for g, game in ctx:
        n_games += 1
        if model is None:
            model = Model(game["tables"], game["rules"])
        turns = [t for t in game["turns"] if t["pre"]["state"]["cities"]]
        for k in range(0, len(turns), chunk):
            b = Batch(model, game, turns[k:k + chunk])
            t1 = time.time()
            got = evaluate(model, game, b)
            mx.eval(got)
            t_gpu += time.time() - t1
            for ti, c, parts, mine, theirs, vals in compare(model, game, b, got):
                tot["cities"] += 1
                for p, v in parts.items():
                    parts_ok[p] = parts_ok.get(p, 0) + int(v)
                if all(parts.values()):
                    tot["ok"] += 1
                elif len(examples) < show:
                    t = b.turns[ti]
                    examples.append({"game": g, "round": t["round"], "player": t["player"], "city": c["h"], "bad": [p for p, v in parts.items() if not v],
                                     "pop": c["pop"], "rust": {k: c["rust"][k] for k in ("base", "rates", "specialists", "growth_percent", "item_percent", "head_cost")},
                                     "mlx": vals, "worked_rust": theirs, "worked_mlx": mine})
    t_load = time.time() - t0 - t_gpu
    return {"games": n_games, "cities": tot["cities"], "cities_ok": tot["ok"], "parts": parts_ok, "examples": examples,
            "load_s": round(t_load, 2), "gpu_s": round(t_gpu, 3)}


def bench(path, sizes=(1000, 10000, 50000), game_index=0):
    """City yields per second: the turn lines of one game replicated to `sizes` cities, evaluated
    (all effects, tile effects, the citizens' placement, rates, costs). The batch's host side (JSON
    to arrays) is timed apart: a simulator keeps its state on the GPU."""
    for i, (g, game) in enumerate(each_game(path)):
        if i == game_index:
            break
    model = Model(game["tables"], game["rules"])
    turns = [t for t in game["turns"] if t["pre"]["state"]["cities"]]
    per_turn = sum(len(t["pre"]["state"]["cities"]) for t in turns) / len(turns)
    out = []
    for n in sizes:
        k = max(1, int(n / per_turn))
        reps = (turns * (k // len(turns) + 1))[:k]
        t0 = time.time()
        b = Batch(model, game, reps)
        t_host = time.time() - t0
        got = evaluate(model, game, b)
        mx.eval(got)
        t1 = time.time()
        got = evaluate(model, game, b)
        mx.eval(got)
        t_eval = time.time() - t1
        out.append({"cities": b.n, "turn_lines": len(reps), "host_s": round(t_host, 3), "evaluate_s": round(t_eval, 3),
                    "city_turns_per_s": round(b.n / t_eval)})
    return out


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--games", type=int)
    ap.add_argument("--show", type=int, default=4)
    ap.add_argument("--bench", action="store_true")
    o = ap.parse_args(argv)
    if o.bench:
        print(json.dumps(bench(o.dump), indent=1))
        return
    rep = run(o.dump, o.games, show=o.show)
    print(f"city yields: {rep['cities_ok']}/{rep['cities']} city end-turns of {rep['games']} games equal on every part; "
          f"parts {rep['parts']}; load {rep['load_s']} s, GPU {rep['gpu_s']} s")
    for e in rep["examples"]:
        print("  ex:", json.dumps(e))


if __name__ == "__main__":
    main()
