"""The economy on the GPU (MLX): the treasury, exact to the Rust rules (`derived::maintenance::upkeep`,
`derived::income::player_gold`, `systems::economy::collect_gold`, the science step's deficit).

    python -m aurelia_nn.tacsim.treasury DUMP.jsonl|DIR   # parity: the upkeep of every end of turn's world

    gold     the cities' gold (rates + Wealth) through the player's Gold modifiers, never below 0
    units    free = 3 + the difficulty's free units + garrisons (GarrisonFreeMaintenance) + FreeMaintenanceUnits;
             paid = Σ weights − free × 100 (hundredths; a kind weighs 0 with NoMaintenance, else 100 + its
             MaintenancePercent); cost = ⌊(paid/2/100 × (1 + p))^(1 + p/3)⌋, p = round / max_rounds, the
             power in 40-bit fixed point as the kernel; × the difficulty's (AI) unit cost % (Standing Army Tax +20 %)
    buildings Σ maintenance × the difficulty's (AI) building cost %
    routes   road 1, railroad 2 per own tile without a city (hills free with FreeHillsRouteMaintenance)
             × (100 + RouteMaintenancePercent) % × the route cost %
    connections Σ over the cities connected to the capital (−100 + 15 × capital size + 110 × size) hundredths,
             × (100 + ConnectionGoldPercent) %
    treasury + gold − (units + buildings + routes − connections); below zero: 0, the shortfall comes off the
             turn's science

The Gold step runs after the cities: the units they trained, the buildings they completed (and were
granted), their new sizes and the tiles their borders claimed count.

Inputs from the subsystems not in MLX: the player's Gold modifiers (only beliefs carry them: the
founder's religion), each unit kind's MaintenancePercent (the unit effects), the capital connections.
"""

import argparse
import json
import time

from . import city as C
from . import player as P
from . import rdebug as R

GOLD_PER_UNIT_TIMES100 = 50
FREE_UNITS = 3
ROAD, RAILROAD = 1, 2
CONN_BASE, CONN_CAPITAL, CONN_CITY = -100, 15, 110
ARMY_TAX_PERCENT = 20
BITS = 40
ONE = 1 << BITS
LN2 = 762_123_384_786
FREE_MAINT = P.key("Module(FreeMaintenanceUnits)")
GARRISON_FREE = P.key("Module(Rule(GarrisonFreeMaintenance))")
FREE_HILLS = P.key("Module(Rule(FreeHillsRouteMaintenance))")
ROUTE_PCT = P.key("RouteMaintenancePercent")
CONN_PCT = P.key("Module(ConnectionGoldPercent)")


def tdiv(a, b):
    """Integer division toward zero (Rust)."""
    q = abs(a) // abs(b)
    return q if (a >= 0) == (b >= 0) else -q


def _ln(x):
    k = (x.bit_length() - 1) - BITS
    m = x >> k if k >= 0 else x << -k
    z = tdiv((m - ONE) * ONE, m + ONE)
    z2 = tdiv(z * z, ONE)
    term, total, n = z, 0, 1
    while term != 0:
        total += tdiv(term, n)
        term = tdiv(term * z2, ONE)
        n += 2
    return k * LN2 + 2 * total


def _exp(y):
    n = y // LN2  # div_euclid (LN2 > 0)
    r = y - n * LN2
    term, total, i = ONE, ONE, 1
    while term != 0:
        term = tdiv(tdiv(term * r, ONE), i)
        total += term
        i += 1
    return total << min(n, 80) if n >= 0 else total >> min(-n, 127)


def pow_floor(num, den, exp_num, exp_den):
    if num == 0 or den == 0:
        return 0
    if exp_num == exp_den:
        return min(num // den, 2 ** 64 - 1)
    x = (num << BITS) // den
    y = tdiv(_ln(x) * exp_num, exp_den)
    return max(0, min(_exp(y) >> BITS, 2 ** 64 - 1))


def unit_cost_at(support_times100, turn, end):
    end = max(end, 1)
    turn = min(turn, end)
    num = support_times100 * (end + turn)
    den = end * 100
    exp_den = end * 3
    return pow_floor(num, den, exp_den + turn, exp_den)


def handicap(er, st, p):
    return R.body(R.parse(st.get("handicap", er["players"][p]["handicap"])))


def stage(text):
    """A Rust `StageTotals` Debug text as (flat, percent, count, product)."""
    b = R.body(R.parse(text))
    return b["flat"], b["percent"], b["multiplier_count"], b["multiplier_product"]


def evaluate(base, totals, at_least=0):
    """`StageTotals::evaluate(base, Caps::at_least(0))`."""
    flat, pct, count, product = totals
    h = (base + flat) * (100 + pct)
    if count:
        h = tdiv(h * product, 100 ** count)
    h = max(h, at_least * 100)
    return tdiv(h, 100)


class World:
    """What the upkeep reads of one player's world (a side of a turn line, or the Gold step's)."""

    def __init__(self, units, weights, cities, owned_tiles, routes, city_tiles, hills, round_, ai):
        self.units = units  # {kind: count}
        self.weights = weights  # {kind: weight}
        self.cities = cities  # [{"h", "buildings", "pop", "garrison", "connected", "capital"}]
        self.owned_tiles = owned_tiles
        self.routes, self.city_tiles, self.hills = routes, city_tiles, hills
        self.round, self.ai = round_, ai


def upkeep(model, er, hc, stats, w, max_rounds, army_tax):
    """[units, buildings, routes, connections] of `w` (`maintenance::upkeep`)."""
    b = model.er["buildings"]
    if not w.cities:
        units = 0
    else:
        garrisons = sum(1 for c in w.cities if c["garrison"]) if stats.get(GARRISON_FREE, 0) > 0 else 0
        free = FREE_UNITS + max(hc["gold_free_units"], 0) + garrisons + max(stats.get(FREE_MAINT, 0), 0)
        weights = sum(w.weights.get(k, 100) * n for k, n in w.units.items())
        paid = max(weights - free * 100, 0)
        cost = unit_cost_at(paid * GOLD_PER_UNIT_TIMES100 // 100, w.round, max_rounds)
        pct = hc["ai_unit_cost_percent"] if w.ai else hc["unit_cost_percent"]
        cost = cost * max(pct, 0) // 100
        if army_tax:
            cost = cost * (100 + ARMY_TAX_PERCENT) // 100
        units = min(cost, 2 ** 31 - 1)
    total = sum(b[x]["maintenance"] for c in w.cities for x in c["buildings"])
    pct = hc["ai_building_cost_percent"] if w.ai else hc["building_cost_percent"]
    buildings = tdiv(total * max(pct, 0), 100)
    free_hills = stats.get(FREE_HILLS, 0) > 0
    rt = 0
    for t in w.owned_tiles:
        if w.city_tiles[t] or (free_hills and w.hills[t]):
            continue
        rt += ROAD if w.routes[t] == 1 else RAILROAD if w.routes[t] == 2 else 0
    rt = tdiv(rt * max(100 + stats.get(ROUTE_PCT, 0), 0), 100)
    routes = tdiv(rt * max(hc["route_cost_percent"], 0), 100)
    cap = next((c for c in w.cities if c["capital"]), None)
    conn = 0
    if cap is not None:
        cpct = stats.get(CONN_PCT, 0)
        for c in w.cities:
            if c is cap or not c["connected"]:
                continue
            g = CONN_BASE + cap["pop"] * CONN_CAPITAL + c["pop"] * CONN_CITY
            conn += g if cpct == 0 else tdiv(g * (100 + cpct), 100)
    return [units, buildings, routes, tdiv(conn, 100)]


def connected(model, game, t, side, buildings_of=None):
    """`derived::connections::compute_connected_cities` on a side's world: from the capital over tiles
    with a route (a city tile counts; the nation's road features on its own land) that belong to the
    player or to nobody; a reached city with a harbor reaches every own harbor city on one of its
    seas, and the flood goes on from there. `buildings_of`: {city: buildings} as the Gold step sees
    them (completed this turn). Returns the reached cities (the capital included), or None without a
    capital."""
    st = t[side]["state"]
    planes = t[side + "_planes"]
    gt = game["tables"]
    w, h = gt["map_w"], gt["map_h"]
    p = t["player"]
    b = model.er["buildings"]
    bl = {c["h"]: (buildings_of or {}).get(c["h"], c["buildings"]) for c in st["cities"]}
    tile_of = {c["h"]: c["tile"] for c in st["cities"]}
    cap = next((c["h"] for c in st["cities"] if is_capital(model, bl[c["h"]])), None)
    if cap is None:
        return None
    owner, feature, cityh = planes[4], planes[0], planes[6]
    route = planes[8] if len(planes) > 8 else [0] * len(owner)
    woods = set(game["rules"]["players"][p].get("road_features", []))
    area = gt["tiles"].get("water_area") or [-1] * len(owner)
    own_city_at = {c["tile"]: c["h"] for c in st["cities"]}
    harbor = {hh for hh, bs in bl.items() if any(b[x].get("water_routes") for x in bs)}

    def seas(hh):
        return {area[n] for n in neighbours(w, h, tile_of[hh]) if n >= 0 and area[n] >= 0}

    reached = [cap]
    seen = set()
    frontier = [tile_of[cap]]
    while frontier:
        stack = frontier
        frontier = []
        while stack:
            x = stack.pop()
            if x in seen:
                continue
            seen.add(x)
            if x in own_city_at and own_city_at[x] not in reached:
                reached.append(own_city_at[x])
            for n in neighbours(w, h, x):
                if n < 0 or n in seen:
                    continue
                ok = owner[n] in (-1, p)
                wood = bool(woods) and owner[n] == p and feature[n] in woods
                if ok and (route[n] != 0 or wood or cityh[n] >= 0):
                    stack.append(n)
        on = set()
        for hh in reached:
            if hh in harbor:
                on |= seas(hh)
        if on:
            for c in st["cities"]:
                hh = c["h"]
                if hh not in reached and hh in harbor and seas(hh) & on:
                    reached.append(hh)
                    frontier.append(tile_of[hh])
    return reached


def neighbours(w, h, t):
    from .econ import DIRS
    q, r = t % w, t // w
    return [(r + dr) * w + q + dq if 0 <= q + dq < w and 0 <= r + dr < h else -1 for dq, dr in DIRS]


def world_of(model, game, t, side):
    """The upkeep's world of a turn line's side, as the dump has it."""
    st = t[side]["state"]
    planes = t[side + "_planes"]
    hs = {c["h"] for c in st["cities"]}
    owned = [x for x, oc in enumerate(planes[5]) if oc in hs]
    routes = planes[8] if len(planes) > 8 else [0] * len(planes[0])
    city_tiles = [h >= 0 for h in planes[6]]
    hills = game["tables"]["tiles"].get("hills") or [False] * len(planes[0])
    weights = {k: v for k, v in st.get("unit_weights", [])}
    reach = set(connected(model, game, t, side) or [])
    cities = [{"h": c["h"], "buildings": c["buildings"], "pop": c["pop"], "garrison": c["inputs"]["garrison"],
               "connected": c["h"] in reach, "capital": is_capital(model, c["buildings"])} for c in st["cities"]]
    return World(dict((k, n) for k, n in st["units"]), weights, cities, owned, routes, city_tiles, hills, st.get("round", t["round"]),
                 st.get("ai", game["rules"]["players"][t["player"]]["ai"]))


def is_capital(model, buildings):
    b = model.er["buildings"]
    return any(b[x]["kind"] == "Palace" for x in buildings)


def max_rounds(er):
    return er.get("max_rounds", 500)


def run(path, limit_games=None, show=6):
    tot = {"worlds": 0, "ok": 0, "parts": [0, 0, 0, 0], "income": 0, "income_ok": 0}
    ex = []
    model = prow = None
    t0 = time.time()
    for g, game in C.each_game(path, limit_games):
        if model is None:
            model = C.Model(game["tables"], game["rules"])
            prow = P.Rows(model.rows, model.er)
        er = game["rules"]
        for t in P.prepare(game, game["turns"]):
            for side in ("pre", "post"):
                st = t[side]["state"]
                if "upkeep" not in st:
                    continue
                stats, _ = P.stats_and_costs(prow, t, side)
                hc = handicap(er, st, t["player"])
                w = world_of(model, game, t, side)
                mine = upkeep(model, er, hc, stats, w, max_rounds(er), st.get("army_tax", False))
                want = st["upkeep"]
                if "reached" in st:
                    tot["reached"] = tot.get("reached", 0) + 1
                    tot["reached_ok"] = tot.get("reached_ok", 0) + int(sorted(connected(model, game, t, side) or []) == sorted(st["reached"]))
                tot["worlds"] += 1
                ok = mine == want
                tot["ok"] += int(ok)
                for i in range(4):
                    tot["parts"][i] += int(mine[i] == want[i])
                if not ok and len(ex) < show:
                    ex.append({"game": g, "round": t["round"], "player": t["player"], "side": side, "want": want, "mlx": mine,
                               "units": st["units"], "weights": st.get("unit_weights"), "cities": len(st["cities"])})
    return {**tot, "examples": ex, "run_s": round(time.time() - t0, 2)}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--games", type=int)
    ap.add_argument("--show", type=int, default=6)
    o = ap.parse_args(argv)
    rep = run(o.dump, o.games, o.show)
    print(f"upkeep: {rep['ok']}/{rep['worlds']} worlds (before and after every end of turn) equal on all four parts; "
          f"units, buildings, routes, connections {rep['parts']}; "
          f"cities connected to the capital {rep.get('reached_ok', 0)}/{rep.get('reached', 0)} worlds; run {rep['run_s']} s")
    for e in rep["examples"]:
        print("  ex:", json.dumps(e))


if __name__ == "__main__":
    main()
