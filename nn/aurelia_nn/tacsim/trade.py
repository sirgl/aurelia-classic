"""The economy on the GPU (MLX), part 5: trade routes, exact to the Rust rules (`derived::trade`,
`systems::trade::trade_turn`).

    python -m aurelia_nn.tacsim.trade DUMP.jsonl|DIR    # parity: the values, the moves, the cities' trade yields

The state (the dump's `world.routes`, every running route): owner, origin and destination cities,
domain, kind, the trade unit's kind, the path, the unit's position and direction, the circuits done and
due, the yields of both ends (hundredths: gold, science, food, production).

The Trade step of a player's turn (before the cities), as arrays over [routes] and [routes, path]:
- values of every route at either end of the player (`route_values`): international — the sender's
  base by era, 18 % of the geometric mean of both cities' gold without trade, 25 per luxury or
  strategic resource one end lacks (≤ 5, × the sender's percent), the effects' gold (the cities' and
  the sender's route gold, the target's gold, Merchant Confederacy, Economic Union, Morocco) ×
  (100 + 3·min(L, 15)) % × 150 % at sea; the recipient's share; science per technology the partner
  knows (0.3, ≤ 4.5) and the cultural influence bonus; internal — (200 + 45 era + 15 per citizen) ×
  (100 + 60 at sea + InternalPercent) % of food or production;
- the player's trade units move (3 tiles on land, 5 at sea): one step along the path, turning at either
  end, a circuit at each return to the origin; an enemy combat unit on the tile plunders the route;
- finished routes end and their trade unit comes back to the origin (a unit: its upkeep counts at the
  Gold step);
then the cities read their routes' yields (`city_trade_yields`: both ends summed, whole) after their
percents, and the counts of the effect table (TradePartners, CityStateRoutes).

Inputs from the subsystems MLX does not hold on this side (in a whole-game run they are the other
players' own state): the other end's city (its gold without trade, its luxuries and strategic resources,
its trade stats) and its owner (era, trade stats, ideology), the cultural influence level, the tiles of a
route an enemy combat unit stands on.
"""

import argparse
import json
import math
import time

import mlx.core as mx

I64 = mx.int64
INTERNATIONAL_BASE, INTERNATIONAL_PER_ERA = 80, 25
RECIPIENT_BASE, RECIPIENT_PER_ERA = 25, 10
CITY_GOLD_PERCENT = 18
RESOURCE_GOLD, RESOURCE_GOLD_MAX = 25, 5
DISTANCE_PERCENT_PER_TILE, DISTANCE_PERCENT_MAX_TILES = 3, 15
SEA_GOLD_PERCENT = 150
SCIENCE_PER_TECH, SCIENCE_MAX = 30, 450
INTERNAL_BASE, INTERNAL_PER_ERA, INTERNAL_PER_CITIZEN, INTERNAL_SEA_PERCENT = 200, 45, 15, 60
PLUNDER_BASE, PLUNDER_PER_ERA = 60, 10
LAND_SPEED, SEA_SPEED = 3, 5
INTERNATIONAL, FOOD, PRODUCTION = 0, 1, 2


def routes_of(st):
    """The running routes of a side as dicts (creation order)."""
    out = []
    for r in st["world"]["routes"]:
        owner, origin, dest, domain, kind, unit, path, pos, fwd, done, total, oy, dy = r
        out.append({"owner": owner, "origin": origin, "dest": dest, "sea": domain == "Sea", "kind": kind, "unit": unit, "path": path,
                    "pos": pos, "fwd": fwd, "done": done, "total": total, "oy": list(oy), "dy": list(dy)})
    return out


def isqrt(n):
    return math.isqrt(n)


def science_from(techs, a, b, major_a, major_b):
    if not (major_a and major_b):
        return 0
    gap = sum(1 for x, y in zip(techs[a], techs[b]) if y == "1" and x != "1")
    return min(SCIENCE_PER_TECH * gap, SCIENCE_MAX)


def values(route, terms, techs):
    """`route_values` of one route from its ends' terms: (origin yields, destination yields)."""
    o, d = terms["origin"], terms["destination"]
    if o is None or d is None:
        return [0, 0, 0, 0], [0, 0, 0, 0]
    L = max(len(route["path"]) - 1, 0)
    if route["kind"] == INTERNATIONAL:
        majors = o["major"] and d["major"]
        city_term = (isqrt(o["gold_wo"] * d["gold_wo"]) * CITY_GOLD_PERCENT) % (1 << 32) // 100
        sea = route["sea"]
        sp = o["player"]
        own = o["city"][1 if sea else 0] + sp[1 if sea else 0]
        target = d["city"][2] if majors else 0
        if d["city_state"]:
            policy = sp[2]
        elif o["ideology"] is not None and o["ideology"] == d["ideology"]:
            policy = sp[3]
        else:
            policy = 0
        incoming = d["player"][4] if d["major"] else 0
        extra = policy + incoming
        ours, theirs = set(o["res"]), set(d["res"])
        differ = len(ours - theirs) + len(theirs - ours)
        res_gold = min(differ, RESOURCE_GOLD_MAX) * RESOURCE_GOLD * max(100 + sp[5], 0) // 100
        scale = (100 + DISTANCE_PERCENT_PER_TILE * min(L, DISTANCE_PERCENT_MAX_TILES)) * (SEA_GOLD_PERCENT if sea else 100) // 100
        value = INTERNATIONAL_BASE + INTERNATIONAL_PER_ERA * o["era"] + city_term + (max(own, 0) + max(target, 0) + max(extra, 0)) * 100 + res_gold
        origin_gold = value * scale // 100
        recipient = max(d["city"][3], 0) if majors else 0
        dest_gold = (RECIPIENT_BASE + RECIPIENT_PER_ERA * d["era"] + city_term // 2 + recipient * 100) * scale // 100
        osci = science_from(techs, o["owner"], d["owner"], o["major"], d["major"]) + terms.get("influence", 0)
        dsci = science_from(techs, d["owner"], o["owner"], d["major"], o["major"])
        return [origin_gold, osci, 0, 0], [dest_gold, dsci, 0, 0]
    base = INTERNAL_BASE + INTERNAL_PER_ERA * d["era"] + INTERNAL_PER_CITIZEN * d["pop"]
    pct = max(100 + (INTERNAL_SEA_PERCENT if route["sea"] else 0) + d["player"][6], 0)
    amount = base * pct // 100
    return [0, 0, 0, 0], ([0, 0, amount, 0] if route["kind"] == FOOD else [0, 0, 0, amount])


def step(route):
    last = max(len(route["path"]) - 1, 0)
    if route["fwd"] and route["pos"] >= last:
        route["fwd"] = False
    elif not route["fwd"] and route["pos"] == 0:
        route["fwd"] = True
    if route["fwd"]:
        route["pos"] = min(route["pos"] + 1, last)
    else:
        route["pos"] = max(route["pos"] - 1, 0)
        if route["pos"] == 0:
            route["done"] += 1


def trade_turn(routes, player, owner_of, terms_by_index, techs, enemies_by_index, eras):
    """`trade_turn`: the routes after the player's Trade step, the units that came back
    ([(owner, origin, unit kind)]), the plunders ([(plunderer, gold)]). `owner_of`: city -> owner."""
    if not routes:
        return routes, [], []
    routes = [dict(r, oy=list(r["oy"]), dy=list(r["dy"])) for r in routes]
    for i, r in enumerate(routes):
        r["_i"] = i
        if r["owner"] != player and owner_of.get(r["dest"]) != player:
            continue
        terms = terms_by_index.get(i)
        if terms is None:
            continue
        o, d = values(r, terms, techs)
        r["oy"], r["dy"] = o, d
    plunders = []
    owned = [i for i, r in enumerate(routes) if r["owner"] == player]
    for i in reversed(owned):
        r = routes[i]
        steps = SEA_SPEED if r["sea"] else LAND_SPEED
        enemies = enemies_by_index.get(r["_i"], {})
        for _ in range(steps):
            if r["done"] >= r["total"]:
                break
            step(r)
            if r["pos"] in enemies:
                era = eras[r["owner"]]
                gold = (PLUNDER_BASE + PLUNDER_PER_ERA * era) * (SEA_GOLD_PERCENT if r["sea"] else 100) // 100
                plunders.append((enemies[r["pos"]], gold))
                routes.pop(i)
                break
    back = []
    kept = []
    for r in routes:
        if r["done"] >= r["total"]:
            if owner_of.get(r["origin"]) == r["owner"]:
                back.append((r["owner"], r["origin"], r["unit"]))
        else:
            kept.append(r)
    return kept, back, plunders


def city_yields(routes, city):
    """`city_trade_yields`: whole (gold, science, food, production) of the city's routes."""
    s = [0, 0, 0, 0]
    for r in routes:
        if r["origin"] == city:
            y = r["oy"]
        elif r["dest"] == city:
            y = r["dy"]
        else:
            continue
        for k in range(4):
            s[k] += y[k]
    return [v // 100 for v in s]


def partners(routes, player, owner_of):
    out = set()
    for r in routes:
        d = owner_of.get(r["dest"])
        if d is None:
            continue
        if r["owner"] == player:
            q = d
        elif d == player:
            q = r["owner"]
        else:
            continue
        if q != player:
            out.add(q)
    return len(out)


def city_state_routes(routes, player, owner_of, is_cs):
    return sum(1 for r in routes if r["owner"] == player and is_cs.get(owner_of.get(r["dest"]), False))


# ---- the same step as arrays over [routes, path] (the device form) ----

def move_arrays(pos, fwd, done, total, length, steps, enemy):
    """`move_route` for every route at once: `enemy` [R, Lmax] marks the path tiles an enemy holds.
    Returns (pos, fwd, done, plundered)."""
    plundered = mx.zeros(pos.shape, dtype=mx.bool_)
    for k in range(SEA_SPEED):
        live = (k < steps) & (done < total) & ~plundered
        turn_back = fwd & (pos >= length)
        turn_fwd = (~fwd) & (pos == 0)
        f = mx.where(turn_back, False, mx.where(turn_fwd, True, fwd))
        np_ = mx.where(f, mx.minimum(pos + 1, length), mx.maximum(pos - 1, 0))
        nd = done + ((~f) & (np_ == 0)).astype(done.dtype)
        pos = mx.where(live, np_, pos)
        fwd = mx.where(live, f, fwd)
        done = mx.where(live, nd, done)
        hit = mx.take_along_axis(enemy, mx.clip(pos, 0, enemy.shape[1] - 1)[:, None].astype(mx.int32), axis=1)[:, 0]
        plundered = plundered | (live & hit)
    return pos, fwd, done, plundered


# ---- parity ----

def run(path, limit_games=None, show=8):
    from . import city as C
    t0 = time.time()
    tot = {"lines": 0, "values": 0, "values_ok": 0, "routes_after": 0, "routes_after_ok": 0, "after": 0, "after_ok": 0,
           "counts": 0, "counts_ok": 0, "back": 0, "plunders": 0, "own_terms": 0, "own_terms_ok": 0}
    examples = []
    for g, game in C.each_game(path, limit_games):
        for t in game["turns"]:
            pre = t["pre"]["state"]
            if "world" not in pre:
                continue
            p = t["player"]
            tot["lines"] += 1
            routes = routes_of(pre)
            owner_of = {row[0]: row[1] for row in pre["world"]["religion"]["cities"]}
            is_cs = {k: s[6] for k, s in enumerate(pre["world"]["seats"])}
            eras = [s[0] for s in pre["world"]["seats"]]
            terms = {x["i"]: x for x in pre["extra"]["routes"]}
            enemies = {x["i"]: {k: e for k, e in x["enemies"]} for x in pre["extra"]["routes"]}
            # The formula on the dumped terms against Rust's values on the same world.
            for i, x in terms.items():
                tot["values"] += 1
                o, d = values(routes[i], x, pre["all_techs"])
                ok = [o, d] == x["values"]
                tot["values_ok"] += int(ok)
                if not ok and len(examples) < show:
                    examples.append({"game": g, "round": t["round"], "values": [o, d], "rust": x["values"], "route": {k: routes[i][k] for k in ("kind", "sea", "path")},
                                     "terms": x})
            # The step against the routes after the turn (values recomputed by Rust at the step's world).
            kept, back, plunders = trade_turn(routes, p, owner_of, {i: dict(x, values=None) for i, x in terms.items()}, pre["all_techs"], enemies, eras)
            # Rust's values at the step can differ from the pre world's (the happiness and golden age
            # steps run first); the moves and the ends are compared, and the values where the pre
            # world's equal the post's.
            post = routes_of(t["post"]["state"])
            tot["back"] += len(back)
            tot["plunders"] += len(plunders)
            tot["routes_after"] += 1
            key = lambda r: (r["owner"], r["origin"], r["dest"], r["pos"], r["fwd"], r["done"], r["total"])
            ok = [key(r) for r in kept] == [key(r) for r in post]
            tot["routes_after_ok"] += int(ok)
            if not ok and len([e for e in examples if "moves" in e]) < 4:
                examples.append({"game": g, "round": t["round"], "player": p, "moves": [[key(r) for r in kept], [key(r) for r in post]], "back": back})
            # The cities' trade yields after the step (what the city step reads) against the dump's.
            pc = {c["h"]: c for c in t["post"]["state"]["cities"]}
            for c in pre["cities"]:
                q = pc.get(c["h"])
                if q is None:
                    continue
                tot["after"] += 1
                y = city_yields(kept, c["h"])
                want = q["inputs"]["after"]
                ok = [want[2], want[3], want[0], want[1]] == y
                tot["after_ok"] += int(ok)
                if not ok and len([e for e in examples if "after" in e]) < 4:
                    examples.append({"game": g, "round": t["round"], "player": p, "city": c["h"], "after": [y, want]})
                tot["counts"] += 1
                k = q["inputs"]["counts"]
                cok = (k["TradePartners"], k["CityStateRoutes"]) == (partners(kept, p, owner_of), city_state_routes(kept, p, owner_of, is_cs))
                tot["counts_ok"] += int(cok)
    return {**tot, "examples": examples, "s": round(time.time() - t0, 1)}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--games", type=int)
    ap.add_argument("--show", type=int, default=8)
    o = ap.parse_args(argv)
    r = run(o.dump, o.games, o.show)
    print(f"trade: route values {r['values_ok']}/{r['values']} (the formula on the world before the turn); routes after the step "
          f"{r['routes_after_ok']}/{r['routes_after']} lines (positions, directions, circuits, ends; {r['back']} units back, {r['plunders']} plunders); "
          f"cities' trade yields after the step {r['after_ok']}/{r['after']}; partners and city-state routes {r['counts_ok']}/{r['counts']}; {r['s']} s")
    for e in r["examples"]:
        print("  ex:", json.dumps(e)[:1500])


if __name__ == "__main__":
    main()
