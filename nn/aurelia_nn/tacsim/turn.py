"""The economy on the GPU (MLX), third subsystem: a player's end of turn for its cities and its
empire, exact to the Rust rules (`kernel::systems::turn::end_turn`: the Happiness, GoldenAge,
Cities (GreatPeople, Growth, Production, Borders), Gold, Culture, Science, Research, Diplomacy
(research agreements) and WorkerJobs steps).

    python -m aurelia_nn.tacsim.turn DUMP.jsonl|DIR    # parity against Rust: the state after every end of turn

Before the cities: the happiness snapshot and the golden age step (`happy.py`), whose excess and
golden age the city step reads. Per city, in the turn's city order, from one snapshot of its yields
(`city.evaluate`; a city reads what the earlier ones completed: owner-wide effects, wonders, free
buildings, a golden age started, the resource copies taken, the great people born):
- great people (`great.py`): the points of its specialists and buildings × the rates into the
  progress; at the threshold one is born (a unit: its upkeep counts);
- growth (`grow_city` with the classic formulas): the food difference (citizens and specialists eat,
  the growth percents on a surplus), the food kept, the threshold (speed, the AI's growth percent
  and its per-era modifier), growth, starvation, «avoid growth»; a settler at the head converts the
  surplus into production instead;
- production: the stock grows by the rate; the queue drains from its head while the stock covers
  the head's cost (lost orders leave first: built already, a wonder built elsewhere, a missing
  building, the resources short in the turn's running ledger (`ledger.py`): what the earlier units
  and buildings of this turn took); a city with an empty queue and a process converts it (wealth,
  research); a completed building's OnBuilt rows (free buildings here or in the first cities without
  one, citizens, a golden age, free units, free policies); a completed wonder leaves every queue of
  the player;
- borders: the culture into the stock; at the level's threshold (the kernel's deterministic power)
  the best neutral tile next to the territory joins the city;
then per player: the Gold step (`treasury.py`: the cities' gold through the player's Gold modifiers
less the upkeep of units, buildings and routes plus the connection gold of the world the cities
left; below zero the treasury empties and the shortfall comes off the science); the Culture step
(`culture.py`: the cities' culture, the city-states, the heritage sites, the excess happiness, the
league's boost; the free policies of completed buildings and of an era entered); the cities' science
through the player's terms (Rationalism, the allied city-states, the scholars) into the stock; the
research queue completes while the stock covers the head's price (`derived::research::
technology_cost`); a research agreement at its end pays a third of the smaller pool; the unlocks
after the research; the worker jobs the turn's rate completes (improvements, removed features and
their production to the nearest city).

Not in MLX yet (the parity counts them apart): the World Congress rewards (golden age points,
science, free policies), the gold per turn of deals settled at the round's end, gold from other
systems inside the turn (natural wonder finds, trade missions), techs stolen or granted by other
systems, a razed city's burning.
"""

import argparse
import json
import time

import mlx.core as mx

from . import city as C
from . import great as GP
from . import happy as H
from . import ledger as L
from . import player as P
from . import rdebug as R
from . import treasury as TR

I32 = mx.int32
I64 = mx.int64
PROCESS_PERCENT = 25
LN_2 = 0.6931471805599453
RESEARCH_POOL_DIVISOR = 3
MEDIAN_TECH_PERCENT = 50
SCHOLARS_PERCENT = 12
KNOWN_DISCOUNT_PERCENT = 30
VERY_UNHAPPY_THRESHOLD = -11
VERY_UNHAPPY_PRODUCTION = -50
UNHAPPY_GROWTH_PERCENT = -75
VERY_UNHAPPY_GROWTH_PERCENT = -100
FOOD_BASE, FOOD_PER_POP = 15, 8
GOLD_INDEX = 3  # `YieldKind::Gold.index()` (Output 0, Food 1, Production 2, Gold 3, Science 4, Culture 5)


def growth_percent(excess):
    if excess <= VERY_UNHAPPY_THRESHOLD:
        return VERY_UNHAPPY_GROWTH_PERCENT
    if excess < 0:
        return UNHAPPY_GROWTH_PERCENT
    return 0


def isqrt(n):
    """⌊√n⌋ for n ≥ 0 (int64)."""
    r = mx.floor(mx.sqrt(n.astype(mx.float32))).astype(I64)
    # Two corrections each way cover the float32 error at these sizes (n < 2^40).
    for _ in range(2):
        r = mx.where(r * r > n, r - 1, r)
        r = mx.where((r + 1) * (r + 1) <= n, r + 1, r)
    return r


def threshold(pop, pace, ai, growth_pct, per_era, era):
    """`player_growth_threshold` [C]."""
    n = (mx.maximum(pop, 1) - 1).astype(I64)
    t = 15 + n * 8 + isqrt(n * n * n)
    t = mx.where(pace == 100, t, mx.maximum(t * pace // 100, 1))
    ta = t * growth_pct // 100
    ta = ta * mx.maximum(per_era * era + 100, 0) // 100
    ta = mx.clip(ta, 1, 2 ** 32 - 1)
    return mx.where(ai, ta, t).astype(I64)


def food_production(surplus):
    """`food_production_times100` (hundredths)."""
    return mx.where(surplus <= 0, 0, mx.where(surplus <= 200, surplus, mx.where(surplus <= 400, 200 + (surplus - 200) // 2,
                                                                                  300 + (surplus - 400) // 4)))


def trunc_div(a, b):
    q = mx.abs(a) // b
    return mx.where(a < 0, -q, q)


def item_cost(model, er, cols, ci, item, cost_pct):
    """`CityModifiers::production_cost` of `item` (an item index) in city `ci` (Python ints)."""
    nu = model.nu
    sp = model.speed["production_percent"]
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

    if item < nu:
        d = er["units"][item]
        base = (d["cost"] if sp == 100 else max(d["cost"] * sp // 100, 1)) if d["founder"] else handicap(d["cost"], 0)
        p = cost_pct[0]
    else:
        d = er["buildings"][item - nu]
        base = handicap(d["cost"] + max(d["cpc"], 0) * cols["owner_cities"][ci], 2 if d["wonder"] else 1)
        p = cost_pct[1]
    return max(base * (100 + p), 100) // 100


def has_class(er, buildings, kind):
    return kind in buildings or any(er["buildings"][b]["replaces"] == kind for b in buildings)


def on_built(model):
    """The OnBuilt rows of the buildings (once per model): building -> [(kind, argument, count)]:
    ("here", building) a free building in the event city, ("cities", building, count) free
    buildings in the owner's first cities without one, ("population", n), ("golden_age", turns)."""
    if hasattr(model, "_on_built"):
        return model._on_built
    out = {}
    for src, sid, bucket, text in model.er["effects"]:
        if bucket != "triggered" or src != "building":
            continue
        e = R.body(R.parse(text))
        if R.tag(e["trigger"]) != "OnBuilt":
            continue
        if R.tag(e["condition"]) != "Always" or R.tag(e["second_condition"]) != "Always":
            raise C.Unsupported(f"conditional OnBuilt {text}")
        m, sel = e["modifier"], R.tag(e["selector"])
        t, mb = R.tag(m), R.body(m)
        if t == "FreeBuilding" and sel in ("EventCity", "SourceCity"):
            out.setdefault(sid, []).append(("here", R.arg(mb[0]), 1))
        elif t == "FreeBuildingInCities":
            out.setdefault(sid, []).append(("cities", R.arg(mb["building"]), mb["count"]))
        elif t == "Grant" and R.tag(mb[0]) == "Population" and sel in ("EventCity", "SourceCity"):
            out.setdefault(sid, []).append(("population", R.body(mb[0])[0], 1))
        elif t == "Module" and R.tag(mb[0]) == "FreePolicies":
            out.setdefault(sid, []).append(("free_policies", R.body(mb[0])["count"], 1))
        elif t == "FreeUnit" and sel in ("EventCity", "SourceCity"):
            out.setdefault(sid, []).append(("free_unit", R.arg(mb["unit"]), mb["count"]))
        elif t == "Grant" and R.tag(mb[0]) == "Module" and R.tag(R.body(mb[0])[0]) == "GoldenAge":
            out.setdefault(sid, []).append(("golden_age", R.body(R.body(mb[0])[0])["turns"], 1))
    model._on_built = out
    return out


def grants_to(t, ti, results):
    """{city: [buildings]} the turn's FreeBuildingInCities grants give, so far: a grant made when
    the j-th city completed its building goes to the first cities without it at that moment (the
    cities up to j as they resolved, the later ones as they stood)."""
    hs = [c["h"] for c in t["pre"]["state"]["cities"]]
    pre = {c["h"]: c["buildings"] for c in t["pre"]["state"]["cities"]}
    given = {}
    for j, h in enumerate(hs):
        r = results.get((ti, h))
        for building, count in (r or {}).get("grants", []):
            left = count
            for i, h2 in enumerate(hs):
                if left == 0:
                    break
                r2 = results.get((ti, h2))
                have = (r2["buildings"] if (i <= j and r2) else pre[h2]) + given.get(h2, [])
                if building not in have:
                    given.setdefault(h2, []).append(building)
                    left -= 1
    return given


def make_variant(t, k, results, ti, er):
    """The turn line as its k-th city sees it: the cities before it with what they completed this
    turn (their buildings, the world wonders), the later ones as before; only that city evaluated."""
    st = dict(t["pre"]["state"])
    cities = [dict(c) for c in st["cities"]]
    done = set()
    for c in cities[:k]:
        r = results.get((ti, c["h"]))
        if r is not None:
            c["buildings"] = list(r["buildings"])
            done |= {x for x in r["built"] if er["buildings"][x]["wonder"]}
    # Free buildings the earlier cities' completions gave the owner's first cities without one.
    for c in cities:
        for building in grants_to(t, ti, results).get(c["h"], []):
            if building not in c["buildings"]:
                c["buildings"] = list(c["buildings"]) + [building]
    st["cities"] = cities
    st["wonders_built"] = sorted(set(st.get("wonders_built", [])) | done)
    v = dict(t)
    v["pre"] = {"tiles": t["pre"]["tiles"], "state": st}
    v["_only"] = cities[k]["h"]
    # A golden age an earlier city's completion started (OnBuilt) is on for this city.
    if any((results.get((ti, c["h"])) or {}).get("golden_age") for c in cities[:k]):
        v["_golden"] = True
    return v


def transition(model, game, turns):
    """The state after each turn line's end of turn, as MLX computes it (per city and per player).
    The cities of a turn resolve in order, each from its own snapshot: a city reads what the earlier
    ones completed (owner-wide effects, wonders); the turns where an earlier city completed something
    are evaluated again city by city."""
    er = model.er
    # The Happiness and GoldenAge steps run before the cities: the snapshot's excess (the growth and
    # production penalties) and the golden age after its step (tile gold, +20 % production).
    prow = model.__dict__.setdefault("player_rows", H.PlayerRows(model.rows, er))
    P.prepare(game, turns)
    hap = H.happiness(model, prow, game, turns)
    turns = [dict(t, _golden=bool(h["ga"] and h["ga"]["turns"] > 0), _unhappy=VERY_UNHAPPY_PRODUCTION if h["excess"] <= VERY_UNHAPPY_THRESHOLD else 0,
                  _excess=h["excess"], _ga=h["ga"]) for t, h in zip(turns, hap)]
    b, city_out = city_steps(model, game, turns)
    if city_out is None:
        return b, None, None, None
    results = {(ti, res["h"]): res for ti, res in city_out}
    order = {ti: [c["h"] for c in t["pre"]["state"]["cities"]] for ti, t in enumerate(turns)}
    tb = L.tables(model)

    def takes(r):
        return r["built"] or r.get("born") or any(tb.needs(u) for u in r["trained"])

    for k in range(1, max(len(v) for v in order.values())):
        variants, idx, runs = [], [], []
        for ti, hs in order.items():
            if k < len(hs) and any(takes(results[(ti, h)]) for h in hs[:k] if (ti, h) in results):
                # The city's own buildings may have grown by an earlier city's grant: its row reads them.
                variants.append(make_variant(turns[ti], k, results, ti, er))
                idx.append(ti)
                # The ledger as the earlier cities left it (their units trained, buildings completed).
                run = b.ledgers[ti].copy()
                for h in hs[:k]:
                    r = results.get((ti, h))
                    for item in (r or {}).get("taken", []):
                        run.take(item)
                    for grp in (r or {}).get("born_groups", []):
                        run.born[grp] += 1
                runs.append(run)
        if variants:
            _, out = city_steps(model, game, variants, ledgers=runs)
            for vi, res in out:
                results[(idx[vi], res["h"])] = res
    # The turn's grants to its cities (the later ones read them already as their own buildings).
    for ti, hs in order.items():
        for h, extra in grants_to(turns[ti], ti, results).items():
            r = results.get((ti, h))
            if r:
                r["buildings"] = r["buildings"] + [x for x in extra if x not in r["buildings"]]
    # A completed world (national) wonder leaves every queue of the player (`sweep_lost_orders`).
    for ti, hs in order.items():
        gone = set()
        for h in hs:
            r = results.get((ti, h))
            if r:
                gone |= {model.nu + x for x in r["built"] if er["buildings"][x]["wonder"] or er["buildings"][x]["kind"] == "NationalWonder"}
        if gone:
            for h in hs:
                r = results.get((ti, h))
                if r:
                    r["queue"] = [q for q in r["queue"] if q not in gone]
    city_out = [(ti, results[(ti, h)]) for ti, hs in order.items() for h in hs if (ti, h) in results]
    per_turn = {}
    for ti, res in city_out:
        pt = per_turn.setdefault(ti, {"science": 0, "gold": 0})
        pt["science"] += res["science_add"]
        pt["gold"] += res["gold_add"]
    return finish_turns(model, game, b, turns, city_out, per_turn)


def city_steps(model, game, turns, ledgers=None):
    """Growth and production of the cities of `turns` (each city from the snapshot of its turn line).
    `ledgers`: the resource ledger each turn line's (variant's) city starts from; None: the turns'
    own, from their worlds (`ledger.base`), updated as their cities resolve in order."""
    er = model.er
    nu = model.nu
    b = C.Batch(model, game, turns, side="pre", mid=True)
    if b.n == 0:
        return b, None
    got = C.evaluate(model, game, b)
    tb = L.tables(model)
    if ledgers is None:
        prow4 = model.__dict__.setdefault("player_rows4", P.Rows(model.rows, er))
        elx_v = got["extra_lux"].tolist()
        elx = {(ti, c["h"]): elx_v[i] for i, (ti, c) in enumerate(b.cities) if elx_v[i]}
        copies = L.building_copies(model, prow4)
        ledgers = [L.Running(tb, tot, used, copies) for tot, used in L.base(model, prow4, game, turns, "pre", elx)]
        for run, t in zip(ledgers, turns):
            run.born = GP.born_of(t["pre"]["state"].get("economy"))
        b.ledgers = [r.copy() for r in ledgers]
    cols = b.cols
    n = b.n
    rates = got["rates"]
    pop = mx.array(cols["pop"], dtype=I64)
    spec_count = mx.minimum(mx.sum(got["spec_n"], axis=1).astype(I64), pop)
    spec_food = got["spec_food_pct"].astype(I64)
    eaters = mx.maximum(pop * 100 + spec_count * spec_food, 0)
    consumption = 2 * eaters
    food = rates[:, 0].astype(I64) * 100
    surplus = food - consumption
    heads = cols["head"]
    producing_food = mx.array([h >= 0 and h < nu and er["units"][h]["food"] for h in heads])
    excess = [b.turns[ti].get("_excess", b.turns[ti]["post"]["state"]["excess"]) for ti, _ in b.cities]
    emp = mx.array([growth_percent(e) for e in excess], dtype=I64)
    pct = mx.maximum(100 + got["growth_percent"].astype(I64) + emp, 0)
    diff = mx.where(surplus > 0, surplus * pct // 100, surplus)
    diff = mx.where(producing_food, 0, diff)
    pace = model.speed["pace_percent"]
    hcs = cols["handicap"]
    players = game["rules"]["players"]  # per game (the seats)
    growth_ai = mx.array([h[5] for h in hcs], dtype=I64)
    per_era = mx.array([h[4] for h in hcs], dtype=I64)
    era = mx.array(cols["era"], dtype=I64)
    ai = mx.array(cols["ai"])
    thr = threshold(pop, pace, ai, growth_ai, per_era, era)
    kept_pct = mx.clip(got["food_kept_stat"].astype(I64), 0, 100)
    store0 = mx.array([c["food"] for _, c in b.cities], dtype=I64)
    kept0 = mx.array([c["kept"] for _, c in b.cities], dtype=I64)
    avoid = mx.array(cols["avoid"])
    store = store0 + diff
    kept = mx.clip(kept0 + trunc_div(diff, 100), 0, thr * kept_pct // 100)
    full = store >= thr * 100
    used = mx.maximum(thr - kept, 0)
    new_food = mx.where(full & avoid, thr * 100, mx.where(full, mx.maximum(store - used * 100, 0), mx.where(store < 0, 0, store)))
    new_pop = mx.where(full & ~avoid, pop + 1, mx.where((store < 0) & (pop > 1), pop - 1, pop))
    # Production.
    prod = mx.maximum(rates[:, 1], 0).astype(I64) + mx.where(producing_food, food_production(surplus) // 100, 0)
    resist = mx.array(cols["resist"])
    mx.eval(new_food, new_pop, kept, prod)
    out = {"food": mx.where(resist, store0, new_food).tolist(), "kept": mx.where(resist, kept0, kept).tolist(),
           "pop": mx.where(resist, pop, new_pop).tolist(), "prod": prod.tolist(), "rates": rates.tolist()}
    # The queue drains (Python over the few items; the costs and checks are the rules' tables).
    cost_pct = got["cost_pct"].tolist()
    # The GreatPeople step (first in a city's turn): the points from the city's snapshot.
    prow4 = model.__dict__.setdefault("player_rows4", P.Rows(model.rows, er))
    D = GP.defs(model)
    gp_sn, gp_pts, gp_rt = got["spec_n"].tolist(), got["gp_points"].tolist(), got["gp_rate"].tolist()
    gp_rates = {}
    city_out = []
    for ci, (ti, c) in enumerate(b.cities):
        t = b.turns[ti]
        st = t["pre"]["state"]
        res = {"h": c["h"], "pop": out["pop"][ci], "food": out["food"][ci], "kept": out["kept"][ci], "rates": out["rates"][ci], "cost_pct": cost_pct[ci]}
        res["born"], res["born_groups"] = [], []
        if "gpp" in c and c["resist"] == 0:
            if ti not in gp_rates:
                gp_rates[ti] = GP.player_rate(prow4, t)
            stats_, funding_ = gp_rates[ti]
            pts = GP.city_points(D, gp_sn[ci], gp_pts[ci], gp_rt[ci], stats_, funding_)
            before = dict(ledgers[ti].born)
            res["gpp"], res["born"] = GP.accumulate(D, c["gpp"], pts, gp_sn[ci], gp_pts[ci], ledgers[ti].born, model.speed["pace_percent"],
                                                    game["rules"]["players"][t["player"]]["nation"])
            res["born_groups"] = [k for k in before for _ in range(ledgers[ti].born[k] - before[k])]
        elif "gpp" in c:
            res["gpp"] = list(c["gpp"])
        buildings = list(c["buildings"])
        queue = [C.item_of(q, nu) for q in c["queue"]]
        stock = c["prod"]
        gold_p = sci_p = 0
        trained = []
        built = []
        grants = []
        taken = []
        freed = []
        if c["resist"] > 0:
            res.update({"prod": stock, "queue": queue, "buildings": buildings})
        else:
            production = out["prod"][ci]
            process = c["process"]
            if not queue and process not in ("None",):
                conv = max(production, 0) * PROCESS_PERCENT // 100
                if "Wealth" in process:
                    gold_p = conv
                elif "Research" in process:
                    sci_p = conv
            else:
                stock += production
                run = ledgers[ti]
                wonders = set(st.get("wonders_built", []))
                own_other = [o for o in st["cities"] if o["h"] != c["h"]]

                def lost(item, at):
                    if 0 <= item < nu:
                        return run.blocked(item)
                    k = item - nu
                    d = er["buildings"][k]
                    if k in buildings:
                        return True
                    if d["wonder"] and k in wonders:
                        return True
                    if d["kind"] == "NationalWonder" and any(k in o["buildings"] for o in own_other):
                        return True
                    if any(not has_class(er, buildings, rq) for rq in d["requires"]):
                        return True
                    if d["requires_empire"] and not all(all(has_class(er, o["buildings"], rq) for rq in d["requires_empire"])
                                                        for o in st["cities"] if o["status"] != "Puppet"):
                        return True
                    return run.blocked(item)

                # sweep_city_unbuildable: NeedsBuilding orders anywhere.
                def needs(item):
                    if item < 0 or item < nu:
                        return False
                    d = er["buildings"][item - nu]
                    return any(not has_class(er, buildings, rq) for rq in d["requires"]) or bool(
                        d["requires_empire"] and not all(all(has_class(er, o["buildings"], rq) for rq in d["requires_empire"])
                                                         for o in st["cities"] if o["status"] != "Puppet"))

                orig = list(range(len(queue)))
                keep = [i for i in orig if not needs(queue[i])]
                queue = [queue[i] for i in keep]
                pos = keep
                while queue:
                    item = queue[0]
                    if item < 0:
                        res["project"] = True  # a project: its cost and its completion are not in MLX yet
                        break
                    if lost(item, pos[0]):
                        queue.pop(0)
                        pos.pop(0)
                        continue
                    if item < nu and er["units"][item]["purchase_only"]:
                        break
                    cost = item_cost(model, er, cols, ci, item, cost_pct[ci])
                    if stock < cost:
                        break
                    stock -= cost
                    queue.pop(0)
                    pos.pop(0)
                    run.take(item)
                    taken.append(item)
                    if item < nu:
                        trained.append(item)
                    else:
                        buildings.append(item - nu)
                        built.append(item - nu)
                        d = er["buildings"][item - nu]
                        if d["wonder"] or d["kind"] == "NationalWonder":
                            queue = [q for q in queue if q != item]
                        # OnBuilt of the building (the trigger table): a free building here, free
                        # buildings in the owner's first cities without one, citizens, a golden age.
                        for kind, arg, count in on_built(model).get(item - nu, []):
                            if kind == "here" and arg not in buildings:
                                buildings.append(arg)
                                run.take(nu + arg)
                                taken.append(nu + arg)
                            elif kind == "free_unit":
                                for _ in range(count):
                                    run.take(arg)
                                    taken.append(arg)
                                freed.append((arg, count))
                            elif kind == "cities":
                                grants.append((arg, count))
                            elif kind == "population":
                                res["pop"] += arg
                            elif kind == "golden_age":
                                res["golden_age"] = arg
            res.update({"prod": stock, "queue": queue, "buildings": buildings})
        res["trained"], res["built"], res["grants"], res["taken"] = trained, built, grants, taken
        res["freed"] = freed
        res["science_add"] = out["rates"][ci][3] + sci_p if c["resist"] == 0 else 0
        res["gold_add"] = out["rates"][ci][2] + gold_p if c["resist"] == 0 else 0
        city_out.append((ti, res))
    b.raw = got["raw"]
    b.cost_pct = cost_pct
    return b, city_out


def ln(x):
    """`numeric::ln` (the deterministic series of the kernel, bit for bit in IEEE doubles)."""
    import struct
    bits = struct.unpack("<Q", struct.pack("<d", x))[0]
    k = ((bits >> 52) & 0x7ff) - 1023
    m = struct.unpack("<d", struct.pack("<Q", (bits & 0x000fffffffffffff) | (1023 << 52)))[0]
    z = (m - 1.0) / (m + 1.0)
    z2 = z * z
    term, total, n = z, 0.0, 1.0
    while n < 80.0:
        total += term / n
        term *= z2
        n += 2.0
    return 2.0 * total + k * LN_2


def exp(y):
    q = y / LN_2
    n = float(int(q + 0.5)) if q >= 0 else -float(int(-q + 0.5))  # f64::round: half away from zero
    r = y - n * LN_2
    term, total, i = 1.0, 1.0, 1.0
    while i < 30.0:
        term = term * r / i
        total += term
        i += 1.0
    v, step = total, (2.0 if n >= 0 else 0.5)
    for _ in range(int(abs(n))):
        v *= step
    return v


def kpow(x, e):
    if x <= 0.0:
        return 1.0 if e == 0.0 else 0.0
    return exp(e * ln(x))


def border_cost(level, exponent_percent, cost_percent, speed_percent):
    """`derived::culture::speed_border_cost`."""
    exponent = 1.3
    if exponent_percent != 0:
        exponent = exponent * float(100 + exponent_percent) / 100.0
    cost = 6.0 * kpow(float(level) + 1.4813, exponent)
    if cost_percent != 0:
        cost = cost * float(100 + max(cost_percent, -85)) / 100.0
    if speed_percent != 100:
        cost = cost * float(speed_percent) / 100.0
    import math
    return max(int(math.floor(cost + 0.5)), 1)


def borders(model, game, b, turns, city_out):
    """The Borders step of each city (in the turn's city order): its culture into the stock; at the
    level's threshold the stock pays it, the level grows, and the best neutral tile next to the
    territory within 5 joins the city (no resource, no resource next to it, no river, higher plot
    yields, nearer, lower id: in that order of priority)."""
    w, h = game["tables"]["map_w"], game["tables"]["map_h"]
    res_of = {(ti, r["h"]): r for ti, r in city_out}
    raw = None
    speed = model.speed["culture_percent"]
    claims = {}
    river = game["tables"]["tiles"]["river"]
    for ti, t in enumerate(turns):
        st = t["pre"]["state"]
        planes = t["pre_planes"]
        owner = list(planes[4])
        ocity = list(planes[5])
        resource = planes[1]
        claimed_now = []
        for c in st["cities"]:
            r = res_of.get((ti, c["h"]))
            if r is None or c["resist"] > 0:
                continue
            stock = c["culture"] + max(r["rates"][4], 0)
            centre = c["tile"]
            disc5 = disc_r(w, h, centre, 5)
            ring1 = set(disc_r(w, h, centre, 1))
            level = sum(1 for x in disc5 if x not in ring1 and ocity[x] == c["h"])
            cp = r["cost_pct"]
            need = border_cost(level, cp[3], cp[2], speed)
            r["culture"], r["acquired"], r["radius"] = stock, c["acquired"], c["radius"]
            if stock < need:
                continue
            if raw is None:
                raw = b.raw.tolist()
            best = None
            for x in disc5:
                if owner[x] != -1:
                    continue
                if not any(n >= 0 and ocity[n] == c["h"] for n in neighbours6(w, h, x)):
                    continue
                has_res = resource[x] >= 0
                adj_res = any(n >= 0 and resource[n] >= 0 for n in neighbours6(w, h, x))
                ys = sum(max(v, 0) for v in raw[ti][x])
                key = (int(not has_res), int(not adj_res), int(not river[x]), -ys, hex_distance(w, centre, x), x)
                if best is None or key < best[0]:
                    best = (key, x)
            r["culture"] = stock - need
            r["acquired"] = c["acquired"] + 1
            if best is not None:
                x = best[1]
                r["radius"] = max(c["radius"], hex_distance(w, centre, x))
                owner[x] = t["player"]
                ocity[x] = c["h"]
                claimed_now.append((x, c["h"]))
        claims[ti] = claimed_now
    return claims


def disc_r(w, h, centre, radius):
    q0, r0 = centre % w, centre // w
    out = []
    for dq in range(-radius, radius + 1):
        for dr in range(max(-radius, -dq - radius), min(radius, radius - dq) + 1):
            q, r = q0 + dq, r0 + dr
            if 0 <= q < w and 0 <= r < h:
                out.append(r * w + q)
    return out


def neighbours6(w, h, t):
    q, r = t % w, t // w
    return [(r + dr) * w + q + dq if 0 <= q + dq < w and 0 <= r + dr < h else -1 for dq, dr in C.econ.DIRS]


def finish_turns(model, game, b, turns, city_out, per_turn):
    """After the cities: the worker jobs, the science and the research, the unlocks."""
    er = model.er
    prow = model.__dict__.setdefault("player_rows", H.PlayerRows(model.rows, er))
    claims = borders(model, game, b, turns, city_out)
    # Worker jobs (the WorkerJobs step, after the cities): the jobs the turn's rate completes put their
    # improvement on the map (a feature it is not valid on goes first) or remove their feature; a
    # removed feature's production goes to the city of the tile (else the nearest), less 5 a tile
    # beyond 2.
    by_city = {(ti, res["h"]): res for ti, res in city_out}
    jobs_out = {}
    pace = model.speed["pace_percent"]
    w = game["tables"]["map_w"]
    for ti, t in enumerate(b.turns):
        st = t["pre"]["state"]
        if "jobs" not in st:
            continue
        p = t["player"]
        rate = st["work_rate"]
        feat = t["pre_planes"][0]
        post_planes = t["post_planes"]
        changed = {}
        cities = [(c["h"], c["tile"]) for c in st["cities"]]

        def remove_feature(tile, f):
            if changed.get(tile, {}).get("feature", feat[tile]) != f:
                return
            changed.setdefault(tile, {})["feature"] = -1
            prod = er["feature_removal"][f]
            if prod <= 0:
                return
            prod = prod * pace // 100
            if post_planes[4][tile] != p:
                return
            oc = post_planes[5][tile]
            if oc < 0:
                if not cities:
                    return
                oc = min(cities, key=lambda hc: (hex_distance(w, tile, hc[1]), hc[0]))[0]
            ctile = next((ct for h, ct in cities if h == oc), None)
            if ctile is None:
                return
            amount = prod - max(hex_distance(w, tile, ctile) - 2, 0) * 5
            if amount > 0 and (ti, oc) in by_city:
                by_city[(ti, oc)]["prod"] += amount
                by_city[(ti, oc)]["chopped"] = by_city[(ti, oc)].get("chopped", 0) + amount

        for tile, target, left, site_ok in st["jobs"]:
            if left > rate:
                continue
            v = R.parse(target)
            tag = R.tag(v)
            if tag == "Improvement":
                imp = R.arg(R.body(v)[0])
                if site_ok:
                    f = changed.get(tile, {}).get("feature", feat[tile])
                    if f >= 0 and f not in er["improvement_features"][imp]:
                        remove_feature(tile, f)
                    changed.setdefault(tile, {})["improvement"] = imp
            elif tag == "Chop":
                remove_feature(tile, R.arg(R.body(v)[0]))
        jobs_out[ti] = changed
    # Science and research per player.
    players_out = {}
    for ti, t in enumerate(b.turns):
        st, post = t["pre"]["state"], t["post"]["state"]
        p = t["player"]
        cities_sci = per_turn.get(ti, {"science": 0})["science"]
        _, stats = H.player_terms(prow, t, game["rules"])
        happy_pct = stats.get("ScienceWhileHappyPercent", 0)
        sci = max(cities_sci, 0)
        if happy_pct > 0 and cities_sci > 0 and t.get("_excess", post["excess"]) >= 0:
            sci += cities_sci * happy_pct // 100
        if stats.get("AllyScience", 0) > 0:
            sci += st.get("allied_science", 0)
        queue = list(st["queue"])
        if st.get("scholars") and queue:
            head = queue[0]
            gp = game["rules"]["players"]
            if any(o != p and gp[o]["major"] and st["alive"][o] and st["all_techs"][o][head] == "1" for o in range(len(gp))):
                sci = sci * (100 + SCHOLARS_PERCENT) // 100
        # The Gold step (after the cities, before the science): the treasury, a deficit off the science.
        gold = gold_step(model, game, t, ti, by_city, per_turn.get(ti, {"gold": 0})["gold"], claims.get(ti, []))
        sci = max(sci + min(gold["deficit"], 0), 0)
        stock = st["science"] + sci
        # Korea (CapitalScienceBuildingBoost): a science building completed in the capital this turn
        # adds half the median price of the technologies researchable now (during the city step).
        if stats.get("CapitalScienceBuildingBoost", 0) > 0:
            for c in st["cities"]:
                r = by_city.get((ti, c["h"]))
                for x in (r or {}).get("built", []):
                    if c["capital"] and x in science_buildings(model):
                        known0 = [ch == "1" for ch in st["techs"]]
                        costs = sorted(tech_cost(game["rules"], game, t, k) for k, d in enumerate(er["techs"])
                                       if not known0[k] and all(known0[q] for q in d["requires"]))
                        if costs:
                            stock += costs[len(costs) // 2] * MEDIAN_TECH_PERCENT // 100
        known = [ch == "1" for ch in st["techs"]]
        learned = []
        while queue:
            head = queue[0]
            if known[head]:
                queue.pop(0)
                continue
            if not all(known[r] for r in er["techs"][head]["requires"]):
                break
            cost = tech_cost(game["rules"], game, t, head)
            if stock < cost:
                break
            stock -= cost
            queue.pop(0)
            known[head] = True
            learned.append(head)
        # The Diplomacy step (after the research): a research agreement accumulates this turn's
        # science, and pays at its end a third of the smaller pool × (50 + the player's percent) %.
        for other, until, mine, theirs in st.get("research_agreements", []):
            if t["round"] >= until:
                stock += min(mine, theirs) // RESEARCH_POOL_DIVISOR * max(MEDIAN_TECH_PERCENT + stats.get("ResearchAgreementPercent", 0), 0) // 100
        # The golden age after the turn: its step (before the cities), then the ones a completion
        # started (OnBuilt grants: a running golden age is lengthened, a new one counts).
        ga = dict(t["_ga"]) if t.get("_ga") else None
        if ga is not None:
            speed_ga = model.speed["golden_age_percent"]
            lp = stats.get("GoldenAgeLengthPercent", 0)
            for c in st["cities"]:
                r = by_city.get((ti, c["h"]))
                base = (r or {}).get("golden_age")
                if not base:
                    continue
                length = base if lp <= 0 else base * (100 + lp) // 100
                if not (lp <= 0 and speed_ga == 100):
                    length = min(max(length * speed_ga // 100, 1), 65535)
                if ga["turns"] == 0:
                    ga["count"] += 1
                ga["turns"] += length
        # The Culture step: the cities' culture (Borders) and the player's sources into the pool.
        cul = None
        if "culture" in st:
            rs = [by_city.get((ti, c["h"])) for c in st["cities"]]
            csum = sum(max(r["rates"][4], 0) for r, c in zip(rs, st["cities"]) if r is not None and c["resist"] == 0)
            wonders = sum(1 for r in rs if r for x in r["buildings"] if er["buildings"][x]["kind"] == "WorldWonder")
            from . import culture as CU
            cul = CU.collect(model, model.__dict__["player_rows4"], game, t, csum, t.get("_excess", post["excess"]), wonders, len(st["cities"]))
            for c in st["cities"]:
                r = by_city.get((ti, c["h"]))
                for x in (r or {}).get("built", []):
                    for kind, arg, count in on_built(model).get(x, []):
                        if kind == "free_policies":
                            cul["free_policies"] += arg
            # OnEraEntered (a technology of a later era learned this turn): the nation's free policies.
            era0 = st["era"]
            era1 = max([er["techs"][k]["era"] for k, ch in enumerate(players_out_techs(st["techs"], learned))] + [era0]) if learned else era0
            if era1 > era0:
                cul["free_policies"] += era_policies(model).get(game["rules"]["players"][p]["nation"], 0)
        players_out[ti] = {"ga": ga, "science": stock, "treasury": gold["treasury"], "gold": gold, "culture": cul, "queue": queue, "techs": "".join("1" if k else "0" for k in known), "learned": learned,
                           "cities_science": cities_sci, "collected": sci}
    # The unlocks after the research: the kernel's gates (not buildable, another nation's, replaced
    # by the nation's unique unit) and the required technology, one gather per item.
    if players_out:
        nu_ = model.nu
        items = [(u["unbuildable"], u["nation"], u["tech"]) for u in er["units"]] + [(d["unbuildable"], d["nation"], d["tech"]) for d in er["buildings"]]
        replaced = {}
        for u in er["units"]:
            if u["nation"] is not None and u["replaces"] is not None:
                replaced.setdefault(u["nation"], set()).add(u["replaces"])
        tis = sorted(players_out)
        nations = [game["rules"]["players"][b.turns[ti]["player"]]["nation"] for ti in tis]
        gate = mx.array([[not unb and (nat is None or nat == n) and not (i < nu_ and i in replaced.get(n, ()))
                          for i, (unb, nat, _) in enumerate(items)] for n in nations])
        req = mx.array([-1 if tech is None else tech for _, _, tech in items], dtype=I32)
        known = mx.array([[ch == "1" for ch in players_out[ti]["techs"]] for ti in tis])
        has = (req[None] < 0) | mx.take(known, mx.maximum(req, 0), axis=1)
        unlocked = (gate & has).tolist()
        for ti, row in zip(tis, unlocked):
            players_out[ti]["unlocked"] = "".join("1" if x else "0" for x in row)
    for ti, cl in claims.items():
        if ti in players_out:
            players_out[ti]["claims"] = cl
    return b, city_out, players_out, jobs_out


def players_out_techs(bits, learned):
    """The indices of the technologies known after the research (the pre bits and the learned)."""
    return sorted({k for k, ch in enumerate(bits) if ch == "1"} | set(learned))


def era_policies(model):
    """{nation: free policies} of the nations' OnEraEntered rows (FreePolicies)."""
    if not hasattr(model, "_era_policies"):
        out = {}
        for src, sid, bucket, text in model.er["effects"]:
            if bucket != "triggered" or src != "nation":
                continue
            e = R.body(R.parse(text))
            m = e["modifier"]
            if R.tag(e["trigger"]) == "OnEraEntered" and R.tag(m) == "Module" and R.tag(R.body(m)[0]) == "FreePolicies":
                out[sid] = out.get(sid, 0) + R.body(R.body(m)[0])["count"]
        model._era_policies = out
    return model._era_policies


def gold_step(model, game, t, ti, by_city, cities_gold, claims):
    """The Gold step: the cities' gold through the player's Gold modifiers, less the upkeep of the
    world the cities left (units trained, buildings completed, sizes, the tiles claimed)."""
    er = game["rules"]
    st, post = t["pre"]["state"], t["post"]["state"]
    if "upkeep" not in st:
        return {"treasury": None, "deficit": 0}
    prow4 = model.__dict__.setdefault("player_rows4", P.Rows(model.rows, model.er))
    w = TR.world_of(model, game, t, "pre")
    post_c = {c["h"]: c for c in post["cities"]}
    built = []
    for c in w.cities:
        r = by_city.get((ti, c["h"]))
        if r is None:
            continue
        built += [x for x in r["buildings"] if x not in c["buildings"]]
        c["buildings"], c["pop"] = r["buildings"], r["pop"]
        c["capital"] = TR.is_capital(model, c["buildings"])
        pc = post_c.get(c["h"])
        if pc is not None:
            c["garrison"] = pc["inputs"]["garrison"]
        for u in r["trained"]:
            w.units[u] = w.units.get(u, 0) + 1
        for u, n in r.get("freed", []):
            w.units[u] = w.units.get(u, 0) + n
        for u in r.get("born", []):
            w.units[u] = w.units.get(u, 0) + 1
    # The connections as the Gold step computes them: the turn's world with its completed harbors.
    reach = set(TR.connected(model, game, t, "pre", {h: r["buildings"] for (ti_, h), r in by_city.items() if ti_ == ti}) or [])
    for c in w.cities:
        c["connected"] = c["h"] in reach
    for k, v in post.get("unit_weights", []):
        w.weights.setdefault(k, v)
    if claims:
        w.owned_tiles = sorted(set(w.owned_tiles) | {x for x, _ in claims})
    stats, _ = P.stats_and_costs(prow4, t, "pre", extra_buildings=built)
    hc = TR.handicap(er, st, t["player"])
    up = TR.upkeep(model, er, hc, stats, w, TR.max_rounds(er), st.get("army_tax", False))
    gross = TR.evaluate(cities_gold, TR.stage(st["rust"]["player_yields"][GOLD_INDEX]))
    net = gross - (up[0] + up[1] + up[2] - up[3])
    after = st["treasury"] + net
    if after >= 0:
        return {"treasury": min(after, 2 ** 32 - 1), "deficit": 0, "upkeep": up, "gross": gross}
    return {"treasury": 0, "deficit": max(after, -2 ** 31), "upkeep": up, "gross": gross}


def science_buildings(model):
    """`systems::research::is_science_building`: a building whose own (SourceCity) effects add
    science to its city."""
    if not hasattr(model, "_science_buildings"):
        out = set()
        for src, sid, bucket, text in model.er["effects"]:
            if src != "building" or bucket != "city":
                continue
            e = R.body(R.parse(text))
            m = e["modifier"]
            t, mb = R.tag(m), R.body(m)
            if R.tag(e["selector"]) != "SourceCity" or t not in ("AddYield", "AddYieldPercent", "AddYieldPerPopulation"):
                continue
            amount = mb.get("amount", mb.get("percent", mb.get("hundredths", 0)))
            if R.tag(mb["yield_kind"]) == "Science" and amount > 0:
                out.add(sid)
        model._science_buildings = out
    return model._science_buildings


def hex_distance(w, a, b):
    dq, dr = a % w - b % w, a // w - b // w
    return (abs(dq) + abs(dr) + abs(dq + dr)) // 2


def tech_cost(er, game, t, tech):
    """`derived::research::technology_cost` (the player's ModifyCost(Technologies) stage is neutral in the content)."""
    st = t["pre"]["state"]
    p = t["player"]
    players = er["players"]
    hc = R.body(R.parse(st.get("handicap", players[p]["handicap"])))
    base = er["techs"][tech]["cost"]
    living = known = 0
    for o in range(len(players)):
        if not players[o]["major"] or not st["alive"][o]:
            continue
        living += 1
        if o != p and "all_techs" in st and st["all_techs"][o][tech] == "1" and st["met"][o]:
            known += 1
    speed = R.body(R.parse(er["speed_rules"]))["science_percent"]
    num = base * max(hc["research_percent"], 1) * max(er["world"]["research_percent"], 1) * max(speed, 1) * 100 * max(living, 1)
    den = 100 * 100 * 100 * (100 * max(living, 1) + KNOWN_DISCOUNT_PERCENT * known)
    num *= max(10000 + er["world"]["per_city"] * (st["effective_cities"] - 1), 0)
    den *= 10000
    return min(max(num // den, 1), 2 ** 32 - 1)


def compare(b, city_out, players_out, jobs_out, er, nu):
    """Per city and per player: the MLX state against the dump's `post`."""
    stats = {"cities": 0, "cities_ok": 0, "players": 0, "players_ok": 0, "job_tiles": 0, "job_tiles_ok": 0}
    for ti, changed in jobs_out.items():
        post = b.turns[ti]["post_planes"]
        for tile, ch in changed.items():
            stats["job_tiles"] += 1
            ok = all(post[0 if k == "feature" else 2][tile] == v for k, v in ch.items())
            stats["job_tiles_ok"] += int(ok)
            if not ok and len([e for e in examples if "job" in e]) < 8:
                st_ = b.turns[ti]["pre"]["state"]
                examples.append({"round": b.turns[ti]["round"], "player": b.turns[ti]["player"], "job": tile, "mlx": ch,
                                 "post": [post[0][tile], post[2][tile]], "pre": [b.turns[ti]["pre_planes"][0][tile], b.turns[ti]["pre_planes"][2][tile]],
                                 "jobs": [j for j in st_["jobs"] if j[0] == tile], "rate": st_["work_rate"]})
    fields = {}
    examples = []
    for ti, res in city_out:
        t = b.turns[ti]
        post = {c["h"]: c for c in t["post"]["state"]["cities"]}
        c = post.get(res["h"])
        if c is None:
            continue
        stats["cities"] += 1
        want = {"pop": c["pop"], "food": c["food"], "kept": c["kept"], "prod": c["prod"], "queue": [C.item_of(q, nu) for q in c["queue"]],
                "buildings": sorted(c["buildings"]), "culture": c["culture"], "acquired": c["acquired"], "radius": c["radius"]}
        mine = {"pop": res["pop"], "food": res["food"], "kept": res["kept"], "prod": res["prod"], "queue": res["queue"],
                "buildings": sorted(res["buildings"]), "culture": res.get("culture", c["culture"]), "acquired": res.get("acquired", c["acquired"]),
                "radius": res.get("radius", c["radius"])}
        ok = True
        for k in want:
            e = want[k] == mine[k]
            fields[k] = fields.get(k, 0) + int(e)
            ok &= e
        stats["cities_ok"] += int(ok)
        if "gpp" in c and "gpp" in res:
            stats["gp_cities"] = stats.get("gp_cities", 0) + 1
            gok = c["gpp"] == res["gpp"]
            stats["gp_cities_ok"] = stats.get("gp_cities_ok", 0) + int(gok)
            if not gok and len([e for e in examples if "gpp" in e]) < 3:
                examples.append({"round": t["round"], "player": t["player"], "city": res["h"], "gpp": [t["pre"]["state"]["cities"] and
                                 {x["h"]: x for x in t["pre"]["state"]["cities"]}[res["h"]]["gpp"], c["gpp"], res["gpp"]], "born": res.get("born")})
        if not ok and len([e for e in examples if "bad" in e]) < 8:
            pre = {x["h"]: x for x in t["pre"]["state"]["cities"]}[res["h"]]
            examples.append({"round": t["round"], "player": t["player"], "city": res["h"],
                             "bad": [k for k in want if want[k] != mine[k]], "want": want, "mlx": mine,
                             "pre": {k: pre[k] for k in ("pop", "food", "kept", "prod", "queue", "status", "resist")},
                             "rates_rust_pre": pre["rust"]["rates"], "built": res["built"], "trained": res["trained"]})
    for ti, pr in players_out.items():
        t = b.turns[ti]
        post = t["post"]["state"]
        stats["players"] += 1
        ok = post["science"] == pr["science"] and post["techs"] == pr["techs"] and post["queue"] == pr["queue"]
        for tile, h in pr.get("claims", []):
            stats["claims"] = stats.get("claims", 0) + 1
            pp = t["post_planes"]
            stats["claims_ok"] = stats.get("claims_ok", 0) + int(pp[4][tile] == t["player"] and pp[5][tile] == h)
        if pr.get("ga") is not None and post.get("economy"):
            pe = post["economy"]
            stats["golden_ages"] = stats.get("golden_ages", 0) + 1
            stats["golden_ages_ok"] = stats.get("golden_ages_ok", 0) + int((pe["ga_turns"], pe["ga_meter"], pe["ga_count"]) == (pr["ga"]["turns"], pr["ga"]["meter"], pr["ga"]["count"]))
        if pr.get("treasury") is not None:
            stats["treasury"] = stats.get("treasury", 0) + 1
            tok = post["treasury"] == pr["treasury"]
            stats["treasury_ok"] = stats.get("treasury_ok", 0) + int(tok)
            if not tok:
                # Why (the inputs MLX does not derive): the deals' gold settled at the round's end, the
                # founder's belief gold that grew with its followers inside the turn (religion), a city-state.
                pre_st = t["pre"]["state"]
                if t["post"].get("round") != t["round"] and pre_st.get("gold_deals"):
                    cause = "deals at the round's end"
                elif t.get("_next_gold") is not None and t["_next_gold"] != pre_st["rust"]["player_yields"][GOLD_INDEX]:
                    cause = "founder's belief gold changed (religion)"
                elif not t["_players"][t["player"]]["major"]:
                    cause = "city-state"
                else:
                    cause = "other"
                causes = stats.setdefault("treasury_causes", {})
                causes[cause] = causes.get(cause, 0) + 1
            if not tok and len([e for e in examples if "treasury" in e]) < 4:
                examples.append({"round": t["round"], "player": t["player"], "treasury": [t["pre"]["state"]["treasury"], post["treasury"], pr["treasury"]],
                                 "gold": pr["gold"], "upkeep_pre_post": [t["pre"]["state"]["upkeep"], post["upkeep"]], "income_pre": t["pre"]["state"]["income_gold"],
                                 "deals": t["pre"]["state"].get("gold_deals"), "post_round": t["post"].get("round")})
        if pr.get("culture") is not None and "culture" in post:
            stats["culture"] = stats.get("culture", 0) + 1
            want = post["culture"]
            mine = pr["culture"]
            cok = all(want[k] == mine[k] for k in ("pool", "lifetime", "recent", "max_cities", "free_policies"))
            stats["culture_ok"] = stats.get("culture_ok", 0) + int(cok)
            if not cok and len([e for e in examples if "culture" in e]) < 4:
                examples.append({"round": t["round"], "player": t["player"], "culture": {k: [t["pre"]["state"]["culture"][k], want[k], mine[k]]
                                                                                        for k in ("pool", "lifetime", "recent", "max_cities", "free_policies")},
                                 "cs": want["cs"], "boost": want["boost_turns"]})
        if post["treasury"] == 0:
            stats["empty_treasury"] = stats.get("empty_treasury", 0) + 1
            stats["empty_treasury_science_ok"] = stats.get("empty_treasury_science_ok", 0) + int(post["science"] == pr["science"])
        if "unlocked" in post:
            stats["unlocks"] = stats.get("unlocks", 0) + 1
            stats["unlocks_ok"] = stats.get("unlocks_ok", 0) + int(post["unlocked"] == pr["unlocked"])
        stats["players_ok"] += int(ok)
        for k in ("science", "techs", "queue"):
            fields["p_" + k] = fields.get("p_" + k, 0) + int(post[k] == pr[k])
        if not ok and len([e for e in examples if "science" in e and "upkeep_pre_post" not in e]) < 6:
            examples.append({"round": t["round"], "player": t["player"], "science": [post["science"], pr["science"]],
                             "pre_science": t["pre"]["state"]["science"], "collected": pr["collected"], "cities_science": pr["cities_science"],
                             "rust_pre_cities_science": t["pre"]["state"]["rust"]["cities_science"],
                             "treasury": [t["pre"]["state"]["treasury"], post["treasury"]], "queue": [t["pre"]["state"]["queue"], post["queue"], pr["queue"]],
                             "learned": pr["learned"]})
    return stats, fields, examples


def run(path, limit_games=None, chunk=64, show=8):
    t0 = time.time()
    ctx = C.each_game(path, limit_games)
    t_load = 0.0
    n_games = 0
    model = None
    tot = {"cities": 0, "cities_ok": 0, "players": 0, "players_ok": 0, "job_tiles": 0, "job_tiles_ok": 0}
    fields = {}
    examples = []
    t_run = 0.0
    for g, game in ctx:
        n_games += 1
        if model is None:
            model = C.Model(game["tables"], game["rules"])
        turns = [t for t in game["turns"] if t["pre"]["state"]["cities"]]
        # Each line with the player's next line's Gold modifiers (a cause of a treasury mismatch).
        nxt = {}
        for t in reversed(game["turns"]):
            t["_next_gold"] = nxt.get(t["player"])
            if "rust" in t["pre"]["state"]:
                nxt[t["player"]] = t["pre"]["state"]["rust"]["player_yields"][GOLD_INDEX]
        for k in range(0, len(turns), chunk):
            t1 = time.time()
            b, city_out, players_out, jobs_out = transition(model, game, turns[k:k + chunk])
            t_run += time.time() - t1
            if city_out is None:
                continue
            s, f, e = compare(b, city_out, players_out, jobs_out, model.er, model.nu)
            for key in s:
                if isinstance(s[key], dict):
                    d = tot.setdefault(key, {})
                    for k2, v2 in s[key].items():
                        d[k2] = d.get(k2, 0) + v2
                else:
                    tot[key] = tot.get(key, 0) + s[key]
            for key in f:
                fields[key] = fields.get(key, 0) + f[key]
            for x in e:
                if len(examples) < show:
                    x["game"] = g
                    examples.append(x)
    return {"games": n_games, **tot, "fields": fields, "examples": examples, "load_s": round(t_load, 2), "run_s": round(t_run, 2)}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--games", type=int)
    ap.add_argument("--show", type=int, default=8)
    o = ap.parse_args(argv)
    rep = run(o.dump, o.games, show=o.show)
    print(f"end of turn: cities {rep['cities_ok']}/{rep['cities']}, players {rep['players_ok']}/{rep['players']} equal, "
          f"tiles of completed worker jobs {rep['job_tiles_ok']}/{rep['job_tiles']}, unlocks {rep.get('unlocks_ok', 0)}/{rep.get('unlocks', 0)}, border tiles claimed {rep.get('claims_ok', 0)}/{rep.get('claims', 0)}, golden age {rep.get('golden_ages_ok', 0)}/{rep.get('golden_ages', 0)}, treasury {rep.get('treasury_ok', 0)}/{rep.get('treasury', 0)}, culture pool {rep.get('culture_ok', 0)}/{rep.get('culture', 0)} (treasury causes {rep.get('treasury_causes', {})}), great person progress {rep.get('gp_cities_ok', 0)}/{rep.get('gp_cities', 0)}, science on turns that end with an empty treasury {rep.get('empty_treasury_science_ok', 0)}/{rep.get('empty_treasury', 0)}; fields {rep['fields']}; "
          f"load {rep['load_s']} s, run {rep['run_s']} s")
    for e in rep["examples"]:
        print("  ex:", json.dumps(e))


if __name__ == "__main__":
    main()
