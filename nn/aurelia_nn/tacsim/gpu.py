"""The economy kept on the GPU (MLX): the state of many empires resident on the device, one end of turn
of every city and every empire per `step` — no JSON, no Python loop over cities or items.

    python -m aurelia_nn.tacsim.gpu DUMP.jsonl|DIR            # parity: one device step from every dumped world
    python -m aurelia_nn.tacsim.gpu DUMP --bench 1000,10000,100000   # city-turns/s, the state on the device

The state (`State`): per world (an empire on its map) the dynamic map planes [W, T] (feature, resource,
improvement, pillaged, owner, owning city, city, route), its technologies, research queue, science,
treasury, culture pool, resource ledger; per city its size, food, kept food, production stock,
buildings [C, NB], queue [C, Q], culture stock, border level and radius, and a local layout: the 127
tiles of radius 6 around it (the radius-3 disc first, in tile order), their static planes and a local
neighbour table — the plot yields are computed on those tiles only.

One `step` (`kernel::systems::turn::end_turn`, the Cities, Gold, Culture, Science and Research steps):
  yields     `city.evaluate_arrays` (every effect row, the citizens, the rates) on the local layouts
  growth     the food difference, the threshold, growth and starvation (as `turn.city_steps`)
  production the stock grows; NeedsBuilding orders leave; then up to K heads a city: a lost order leaves
             (built, a wonder built, a missing building, resources short), a head the stock covers
             completes (units counted, buildings set, their OnBuilt rows: free buildings here and in the
             first cities without one, citizens, free units, free policies), its copies leave the ledger;
             a completed wonder leaves every queue of its empire; two cities completing one wonder in a
             pass: the first in the turn's order keeps it
  borders    the culture into the stock; at the level's price (a table of the kernel's power) the best
             neutral tile next to the territory joins the city (one int64 key, argmin over the layout)
  empire     science (Rationalism, allies, scholars) less a gold deficit, the research queue (the price
             table of the turn), the gold step (upkeep of units, buildings, routes; connections), the
             culture step

What the device step does not order: inside one empire its cities resolve in parallel — a city does
not see what an earlier city of the same turn completed (an owner-wide effect, a wonder completed at a
later pass, the copies an earlier city took when they run short). `parity` counts those apart against
the harness (`turn.py`), which resolves them in order.

Inputs held as the dump gives them (the subsystems not in MLX, as in the harness): beliefs and timed
effects, city-state food, great works, trade routes, landmarks, religious and trade counts, garrisons,
capital connections, the happiness snapshot and the golden age of the turn, the player's Gold
modifiers, unit maintenance percents, the technologies' prices of the turn (met nations' knowledge).
"""

import argparse
import json
import math
import time

import mlx.core as mx

from . import city as C
from . import culture as CU
from . import econ
from . import great as GP
from . import happy as H
from . import ledger as L
from . import player as P
from . import rdebug as R
from . import treasury as TR
from . import turn as TN

I32, I64 = mx.int32, mx.int64
RADIUS = 6
NL = 1 + 3 * RADIUS * (RADIUS + 1)  # 127
K_DRAIN = 5
K_RESEARCH = 3
BIG = 2 ** 30


def layout(w, h, centre):
    """The local layout of a city: the radius-3 disc in tile order (padded to 37), then rings 4..6;
    per slot its ring and its six neighbours as local slots (−1: off the map or the layout)."""
    d3 = C.disc(w, h, centre)
    rest = sorted(set(TN.disc_r(w, h, centre, RADIUS)) - set(d3))
    slots = d3 + [-1] * (C.NCAND - len(d3)) + rest
    slots += [-1] * (NL - len(slots))
    at = {t: i for i, t in enumerate(slots) if t >= 0}
    ring = [TN.hex_distance(w, centre, t) if t >= 0 else 99 for t in slots]
    nbr = [[at.get(n, -1) if (t >= 0 and n >= 0) else -1 for n in TN.neighbours6(w, h, t)] if t >= 0 else [-1] * 6 for t in slots]
    return slots, ring, nbr


class Tables:
    """The rules' tables the step reads (once per model)."""

    def __init__(self, model):
        er = model.er
        nu, nb = model.nu, model.nb
        self.nu, self.nb = nu, nb
        B = er["buildings"]
        U = er["units"]
        self.palace = mx.array([d["kind"] == "Palace" for d in B])
        self.wonder = mx.array([bool(d["wonder"]) for d in B])
        self.national = mx.array([d["kind"] == "NationalWonder" for d in B])
        self.world_wonder = mx.array([d["kind"] == "WorldWonder" for d in B])
        self.maint = mx.array([d["maintenance"] for d in B], dtype=I32)
        self.purchase_only = mx.array([bool(u["purchase_only"]) for u in U] + [False] * nb + [False])
        self.food_unit = mx.array([bool(u["food"]) for u in U] + [False] * nb + [False])
        # Requirement classes: has_class(b, rq) = b == rq or b replaces rq.
        cls = [[float(b == rq or B[b]["replaces"] == rq) for rq in range(nb)] for b in range(nb)]
        self.cls = mx.array(cls, dtype=mx.float32)
        req = [[0.0] * nb for _ in range(nb)]
        emp = [[0.0] * nb for _ in range(nb)]
        for x, d in enumerate(B):
            for rq in d["requires"]:
                req[x][rq] = 1.0
            for rq in d["requires_empire"]:
                emp[x][rq] = 1.0
        self.req_t = mx.array(req, dtype=mx.float32).T  # [rq, x]
        self.emp_t = mx.array(emp, dtype=mx.float32).T
        rt = L.tables(model)
        self.nr = rt.nr
        self.needs = mx.array(rt.req + rt.cons + [[0] * rt.nr], dtype=I32)  # [items + 1, NR]
        ob = TN.on_built(model)
        here = [[False] * nb for _ in range(nb)]
        popg = [0] * nb
        gold = [False] * nb
        freep = [0] * nb
        grant = [[0] * nb for _ in range(nb)]
        funit = [[0] * nu for _ in range(nb)]
        for b, rows in ob.items():
            for kind, arg, count in rows:
                if kind == "here":
                    here[b][arg] = True
                elif kind == "population":
                    popg[b] += arg
                elif kind == "golden_age":
                    gold[b] = True
                elif kind == "cities":
                    grant[b][arg] += count
                elif kind == "free_policies":
                    freep[b] += arg
                elif kind == "free_unit":
                    funit[b][arg] += count
        self.here = mx.array(here)
        self.pop_grant = mx.array(popg, dtype=I32)
        self.ga_grant = mx.array(gold)
        self.free_pol = mx.array(freep, dtype=I32)
        self.grant = mx.array(grant, dtype=I32)
        self.has_grant = any(any(r) for r in grant)
        self.free_unit = mx.array(funit, dtype=I32)
        # Border price per (level, cost percent): the kernel's deterministic power, tabled on the host.
        speed = model.speed["culture_percent"]
        self.border = mx.array([[TN.border_cost(lv, 0, cp, speed) for cp in range(-100, 101)] for lv in range(256)], dtype=I32)
        self.ys_cap = 511


def tables(model):
    if not hasattr(model, "_gpu_tables"):
        model._gpu_tables = Tables(model)
    return model._gpu_tables


class State:
    """Device arrays of W worlds and C cities (see the module doc); built once by `load`."""


def load(model, game, turns):
    """The state of the dumped turn lines (each one world, its pre side with the turn's inputs), as the
    harness reads them; plus what the comparison needs."""
    er = model.er
    tb = tables(model)
    gt = game["tables"]
    w, h = gt["map_w"], gt["map_h"]
    T = w * h
    prow = model.__dict__.setdefault("player_rows", H.PlayerRows(model.rows, er))
    prow4 = model.__dict__.setdefault("player_rows4", P.Rows(model.rows, er))
    P.prepare(game, turns)
    hap = H.happiness(model, prow, game, turns)
    turns = [dict(t, _golden=bool(x["ga"] and x["ga"]["turns"] > 0), _unhappy=TN.VERY_UNHAPPY_PRODUCTION if x["excess"] <= TN.VERY_UNHAPPY_THRESHOLD else 0,
                  _excess=x["excess"], _ga=x["ga"]) for t, x in zip(turns, hap)]
    b = C.Batch(model, game, turns, side="pre", mid=True)
    S = State()
    S.turns, S.batch, S.n, S.W, S.T = turns, b, b.n, len(turns), T
    A = C.column_arrays(model, b.cols, b.n, b.x_city, b.x_tile)
    S.static = {k: A[k] for k in ("nation", "counts", "landmark", "focus", "forced", "gw", "csfood", "unhappy", "after", "era", "owner_cities",
                                  "garrison", "coastal", "river", "peace", "connected", "golden", "locked", "avoid", "resist", "ai", "handicap",
                                  "xc", "xt", "wonder_res")}
    S.pop, S.bld = A["pop"], A["bld"]
    cities = [c for _, c in b.cities]
    S.cw = mx.array([ti for ti, _ in b.cities], dtype=I32)
    S.handle = mx.array([c["h"] for c in cities], dtype=I32)
    S.food = mx.array([c["food"] for c in cities], dtype=I64)
    S.kept = mx.array([c["kept"] for c in cities], dtype=I64)
    S.prod = mx.array([c["prod"] for c in cities], dtype=I64)
    S.culture = mx.array([c["culture"] for c in cities], dtype=I32)
    S.acquired = mx.array([c["acquired"] for c in cities], dtype=I32)
    S.radius = mx.array([c["radius"] for c in cities], dtype=I32)
    Q = max([len(c["queue"]) for c in cities] + [1])
    S.queue = mx.array([[C.item_of(q, model.nu) for q in c["queue"]] + [-1] * (Q - len(c["queue"])) for c in cities], dtype=I32)
    # 1 Wealth, 2 Research (a quarter converted), 3 a World Congress project (the production goes to it whole).
    S.process = mx.array([1 if "Wealth" in c["process"] else 2 if "Research" in c["process"] else 0 if c["process"] == "None" else 3
                          for c in cities], dtype=I32)
    # Local layouts.
    lay = [layout(w, h, c["tile"]) for c in cities]
    S.lt = mx.array([x[0] for x in lay], dtype=I32)
    S.ring = mx.array([x[1] for x in lay], dtype=I32)
    S.lnb = mx.array([x[2] for x in lay], dtype=I32)
    S.centre_slot = mx.array([x[0].index(c["tile"]) for x, c in zip(lay, cities)], dtype=I32)
    S.centre = mx.array([c["tile"] for c in cities], dtype=I32)
    lti = mx.maximum(S.lt, 0)
    off = S.lt < 0
    S.st_static = {k: mx.where(off, 0 if k in ("kind", "wonder") else False, mx.take(mx.array(gt["tiles"][k], dtype=I32 if k in ("kind", "wonder") else mx.bool_), lti))
                   for k in ("kind", "mountain", "water", "lake", "river", "wonder")}
    S.st_static["kind"] = mx.where(off, -1, S.st_static["kind"])
    S.st_static["wonder"] = mx.where(off, -1, S.st_static["wonder"])
    S.terrain = mx.where(off, -1, mx.take(mx.array(game["rules"]["terrain"], dtype=I32), lti))
    S.unworked = mx.array([[tt in set(c["unworked"]) for tt in x[0][:C.NCAND]] for x, c in zip(lay, cities)])
    # World planes (the pre side of each line).
    pl = [t["pre_planes"] for t in turns]

    def plane(i, dt=I32):
        return mx.array([p[i] if len(p) > i else [0] * T for p in pl], dtype=dt)

    S.feature, S.resource, S.improvement = plane(0), plane(1), plane(2)
    S.pillaged = plane(3) > 0
    S.owner, S.ocity, S.cityh, S.route = plane(4), plane(5), plane(6), plane(8)
    S.player = mx.array([t["player"] for t in turns], dtype=I32)
    S.hills = mx.array(gt["tiles"].get("hills") or [False] * T)
    S.mw = mx.arange(S.W, dtype=I32)  # each world's row of the map planes (replicas share one)
    # Per world.
    sts = [t["pre"]["state"] for t in turns]
    S.techs = mx.array([[ch == "1" for ch in s["techs"]] for s in sts])
    NT = len(er["techs"])
    npol = A["pol"].shape[1]
    S.pol = mx.array([[i in set(s["policies"]) for i in range(npol)] for s in sts])
    S.science = mx.array([s["science"] for s in sts], dtype=I64)
    S.treasury = mx.array([s["treasury"] for s in sts], dtype=I64)
    QR = max([len(s["queue"]) for s in sts] + [1])
    S.rqueue = mx.array([list(s["queue"]) + [-1] * (QR - len(s["queue"])) for s in sts], dtype=I32)
    S.tech_cost = mx.array([[TN.tech_cost(game["rules"], game, t, k) for k in range(NT)] for t in turns], dtype=I64)
    S.tech_req = mx.array([[r in d["requires"] for r in range(NT)] for d in er["techs"]])  # [NT, NT]
    sci_pct, ally, scholars, gold_stage, culture_in = [], [], [], [], []
    for t in turns:
        st = t["pre"]["state"]
        _, stats = H.player_terms(prow, t, game["rules"])
        sci_pct.append(stats.get("ScienceWhileHappyPercent", 0))
        ally.append(st.get("allied_science", 0) if stats.get("AllyScience", 0) > 0 else 0)
        gp = game["rules"]["players"]
        scholars.append([bool(st.get("scholars")) and any(o != t["player"] and gp[o]["major"] and st["alive"][o] and st["all_techs"][o][k] == "1"
                                                          for o in range(len(gp))) for k in range(NT)])
        gold_stage.append(list(TR.stage(st["rust"]["player_yields"][TN.GOLD_INDEX])))
    S.sci_pct = mx.array(sci_pct, dtype=I64)
    # Research agreements ending this turn (the Diplomacy step after the research): a third of the
    # smaller pool × (50 + ResearchAgreementPercent) %.
    ra = []
    for t in turns:
        st = t["pre"]["state"]
        _, stats = H.player_terms(prow, t, game["rules"])
        ra.append(sum(min(mine, theirs) // TN.RESEARCH_POOL_DIVISOR * max(TN.MEDIAN_TECH_PERCENT + stats.get("ResearchAgreementPercent", 0), 0) // 100
                      for other, until, mine, theirs in st.get("research_agreements", []) if t["round"] >= until))
    S.ra_pay = mx.array(ra, dtype=I64)
    # Korea (CapitalScienceBuildingBoost): a science building completed in the capital adds half the
    # median price of the technologies researchable at the turn's start.
    kb = []
    for t in turns:
        st = t["pre"]["state"]
        _, stats = H.player_terms(prow, t, game["rules"])
        v = 0
        if stats.get("CapitalScienceBuildingBoost", 0) > 0:
            known0 = [ch == "1" for ch in st["techs"]]
            costs = sorted(TN.tech_cost(game["rules"], game, t, k) for k, d in enumerate(er["techs"]) if not known0[k] and all(known0[q] for q in d["requires"]))
            v = costs[len(costs) // 2] * TN.MEDIAN_TECH_PERCENT // 100 if costs else 0
        kb.append(v)
    S.korea = mx.array(kb, dtype=I64)
    S.sci_bld = mx.array([b in TN.science_buildings(model) for b in range(model.nb)])
    S.ally = mx.array(ally, dtype=I64)
    S.scholars = mx.array(scholars)
    S.gold_stage = mx.array(gold_stage, dtype=I64)
    S.excess = mx.array([t["_excess"] for t in turns], dtype=I64)
    # Wonders built anywhere (the game) before the turn.
    S.wonders = mx.array([[x in set(s.get("wonders_built", [])) for x in range(model.nb)] for s in sts])
    # The resource ledger of the turn's world (the cities' ExtraLuxuryCopies from the city table).
    gx = C.evaluate(model, game, b)
    ev = gx["extra_lux"].tolist()
    elx = {(ti, c["h"]): ev[i] for i, (ti, c) in enumerate(b.cities) if ev[i]}
    base = L.base(model, prow4, game, turns, "pre", elx)
    S.avail = mx.array([[tt - uu for tt, uu in zip(tot, used)] for tot, used in base], dtype=I32)
    # Upkeep inputs.
    S.unit_counts = mx.array([[dict((k, n) for k, n in s["units"]).get(k, 0) for k in range(model.nu)] for s in sts], dtype=I32)
    S.weights = mx.array([[dict(s.get("unit_weights", [])).get(k, dict(t["post"]["state"].get("unit_weights", [])).get(k, 100)) for k in range(model.nu)]
                          for s, t in zip(sts, turns)], dtype=I64)
    S.puppet = mx.array([c["status"] == "Puppet" for c in cities])
    load_jobs(model, game, S)
    load_great(model, game, S)
    load_culture(model, game, S)
    load_routes(model, game, S)
    return S


def load_routes(model, game, S):
    """The route network of every world as a compact graph (`derived::connections`): its tiles that a
    connection may cross (a route, a city, the nation's road features on its own land; the player's
    or nobody's), their neighbours in the graph, each city's node and the seas next to it."""
    gt = game["tables"]
    w, h = gt["map_w"], gt["map_h"]
    area = gt["tiles"].get("water_area") or [-1] * (w * h)
    nodes, nbrs, city_node = [], [], []
    for ti, t in enumerate(S.turns):
        pl = t["pre_planes"]
        p = t["player"]
        woods = set(game["rules"]["players"][p].get("road_features", []))
        route = pl[8] if len(pl) > 8 else [0] * (w * h)
        ok = [(pl[4][x] in (-1, p)) and (route[x] != 0 or pl[6][x] >= 0 or (bool(woods) and pl[4][x] == p and pl[0][x] in woods)) for x in range(w * h)]
        idx = {x: i for i, x in enumerate(x for x in range(w * h) if ok[x])}
        nodes.append(sorted(idx, key=idx.get))
        nbrs.append([[idx.get(n, -1) if n >= 0 else -1 for n in TR.neighbours(w, h, x)] for x in nodes[-1]])
    Pn = max([len(n) for n in nodes] + [1])
    S.r_nbr = mx.array([nb + [[-1] * 6] * (Pn - len(nb)) for nb in nbrs], dtype=I32)  # [W, P, 6]
    at = [{x: i for i, x in enumerate(n)} for n in nodes]
    S.city_node = mx.array([at[ti].get(c["tile"], -1) for ti, c in S.batch.cities], dtype=I32)
    seas = [sorted({area[n] for n in TR.neighbours(w, h, c["tile"]) if n >= 0 and area[n] >= 0}) for _, c in S.batch.cities]
    S.n_areas = 1 + max([a for v in seas for a in v] + [0])
    S.city_seas = mx.array([(v + [-1] * 6)[:6] for v in seas], dtype=I32)
    S.water_routes = mx.array([bool(d.get("water_routes")) for d in model.er["buildings"]])


def connections(S, bld, capital):
    """The cities each world's capital reaches (`compute_connected_cities`) on the device: a flood over
    the route graph, then the harbors: a reached harbor city reaches the own harbor cities on its seas,
    the flood goes on from them; until nothing changes."""
    W, Pn = S.r_nbr.shape[0], S.r_nbr.shape[1]
    cw = S.cw
    has_cap = seg_sum(capital.astype(I32), cw, W) > 0
    flat = cw * Pn + mx.maximum(S.city_node, 0)
    start = (mx.zeros((W * Pn + 1,), dtype=I32).at[mx.where(capital & (S.city_node >= 0), flat, W * Pn)].add(1)[:-1] > 0).reshape(W, Pn)
    harbor = mx.any(bld & S.water_routes[None], axis=1) & (S.city_node >= 0)
    nb_ok = S.r_nbr >= 0
    nb = mx.maximum(S.r_nbr, 0).reshape(W, -1)
    reach = start
    while True:
        for _ in range(8):
            reach = reach | mx.any(nb_ok & mx.take_along_axis(reach, nb, axis=1).reshape(W, Pn, 6), axis=2)
        # the harbors
        got = mx.take(reach.reshape(-1), flat) & (S.city_node >= 0)
        on = mx.zeros((W * S.n_areas + 1,), dtype=I32).at[mx.where((got & harbor)[:, None] & (S.city_seas >= 0), cw[:, None] * S.n_areas + S.city_seas, W * S.n_areas).reshape(-1)].add(1)[:-1]
        on = on.reshape(W, S.n_areas) > 0
        touch = mx.any((S.city_seas >= 0) & mx.take_along_axis(mx.take(on, cw, axis=0), mx.maximum(S.city_seas, 0), axis=1), axis=1)
        more = harbor & touch & ~got
        add = (mx.zeros((W * Pn + 1,), dtype=I32).at[mx.where(more, flat, W * Pn)].add(1)[:-1] > 0).reshape(W, Pn)
        step_ = reach | add
        nxt = step_ | mx.any(nb_ok & mx.take_along_axis(step_, nb, axis=1).reshape(W, Pn, 6), axis=2)
        changed = bool(mx.any(nxt & ~reach).item())
        reach = nxt
        if not changed:
            break
    return mx.take(reach.reshape(-1), flat) & (S.city_node >= 0) & mx.take(has_cap, cw)


GROUPS = ("Writer", "Artist", "Musician", "people")


def load_great(model, game, S):
    """Great people: each city's progress per specialist, the empire's birth counters, the player's
    rate per kind (its stats, the friends' rate, the funding: inputs of the turn), the definitions."""
    D = GP.defs(model)
    prow4 = model.__dict__["player_rows4"]
    ns = max([model.ns] + [len(c.get("gpp", [])) for _, c in S.batch.cities])
    S.gpp = mx.array([(list(c.get("gpp", [])) + [0] * ns)[:ns] for _, c in S.batch.cities], dtype=I64)
    S.has_gpp = mx.array(["gpp" in c for _, c in S.batch.cities])
    S.born = mx.array([[GP.born_of(t["pre"]["state"].get("economy"))[g] for g in GROUPS] for t in S.turns], dtype=I32)
    NK = model.n_gp_kinds
    rates, units = [], []
    for t in S.turns:
        stats, funding = GP.player_rate(prow4, t)
        row = [0] * len(D.defs)
        seen = {}
        for d in D.defs:
            if d["i"] < len(funding) and funding[d["i"]] and d["kind"] not in seen:
                seen[d["kind"]] = funding[d["i"]]
        for d in D.defs:
            row[d["i"]] = stats.get(GP.RATE_ALL, 0) + stats.get(d["rate_key"], 0) + seen.get(d["kind"], 0)
        rates.append(row)
        nation = game["rules"]["players"][t["player"]]["nation"]
        units.append([D.unit_for(d, nation) for d in D.defs])
    S.gp_prate = mx.array(rates, dtype=I64)  # [W, ND]
    S.gp_unit = mx.array(units, dtype=I32)  # [W, ND]
    S.gp_defs = [(d["spec"] if (d["specialists"] and d["spec"] is not None) else -1, d["k"], GROUPS.index(GP.group(d["kind"])), d["kind"] not in GP.NO_PACE)
                 for d in D.defs]
    S.spec_gpp = mx.array(D.spec_gpp, dtype=I64)


def load_culture(model, game, S):
    """The culture step's per-world terms: the pool, the lifetime, the city-states' bonus, the heritage
    sites, the league's boost, HappinessToCulturePercent (inputs as the turn's world has them)."""
    prow4 = model.__dict__["player_rows4"]
    rows = []
    for t in S.turns:
        st = t["pre"]["state"]
        c = st.get("culture")
        stats, _ = P.stats_and_costs(prow4, t, "pre")
        if c is None:
            rows.append([0, 0, 0, 0, 0, 0, 0])
            continue
        rows.append([c["pool"], c["lifetime"], max(c["cs"], 0), int(bool(st.get("heritage"))), int(c["boost_turns"] > 0),
                     max(stats.get(CU.HAPPY_CULTURE, 0), 0), 1])
    S.cul_in = mx.array(rows, dtype=I64)


def load_jobs(model, game, S):
    """The worker jobs of every world as rows (`turn.finish_turns`' rule): the tile, the improvement it
    places (−1), the feature it removes, the city a removed feature's production goes to (its global
    row) and the amount; the device completes the rows whose work left the world's rate covers."""
    er = model.er
    w = game["tables"]["map_w"]
    pace = model.speed["pace_percent"]
    row_of = {(ti, c["h"]): i for i, (ti, c) in enumerate(S.batch.cities)}
    rows = []
    rates = []
    for ti, t in enumerate(S.turns):
        st = t["pre"]["state"]
        rates.append(st.get("work_rate", 0))
        if "jobs" not in st:
            continue
        p, rate = t["player"], st["work_rate"]
        feat, post = t["pre_planes"][0], t["post_planes"]
        changed = {}
        cities = [(c["h"], c["tile"]) for c in st["cities"]]
        for tile, target, left, site_ok in st["jobs"]:
            v = R.parse(target)
            tag = R.tag(v)
            done = left <= rate
            imp, removed, city, amount = -1, -1, -1, 0
            f = None
            if tag == "Improvement":
                imp = R.arg(R.body(v)[0]) if site_ok else -1
                if site_ok:
                    f = changed.get(tile, {}).get("feature", feat[tile])
                    if not (f >= 0 and f not in er["improvement_features"][imp]):
                        f = None
            elif tag == "Chop":
                f = R.arg(R.body(v)[0])
                if changed.get(tile, {}).get("feature", feat[tile]) != f:
                    f = None
            if f is not None:
                removed = f
                prod = er["feature_removal"][f]
                if prod > 0 and post[4][tile] == p and cities:
                    oc = post[5][tile]
                    if oc < 0:
                        oc = min(cities, key=lambda hc: (TN.hex_distance(w, tile, hc[1]), hc[0]))[0]
                    ctile = next((ct for h, ct in cities if h == oc), None)
                    if ctile is not None:
                        amount = prod * pace // 100 - max(TN.hex_distance(w, tile, ctile) - 2, 0) * 5
                        city = row_of.get((ti, oc), -1)
            if done:
                if removed >= 0:
                    changed.setdefault(tile, {})["feature"] = -1
                if imp >= 0:
                    changed.setdefault(tile, {})["improvement"] = imp
            rows.append((ti, tile, left, imp, removed, city if amount > 0 else -1, max(amount, 0)))
    S.rate = mx.array(rates, dtype=I32)
    if not rows:
        rows = [(0, 0, 2 ** 30, -1, -1, -1, 0)]
    cols = list(zip(*rows))
    S.j_world, S.j_tile, S.j_left, S.j_imp, S.j_removed, S.j_city, S.j_amount = (mx.array(c, dtype=I32) for c in cols)


def jobs_step(S, prod):
    """The WorkerJobs step on the device: the jobs the rate completes; a removed feature's production
    into its city's stock. Returns the stock and the planes after."""
    done = S.j_left <= mx.take(S.rate, S.j_world)
    add = mx.where(done & (S.j_city >= 0), S.j_amount, 0)
    prod = prod + mx.zeros((S.n,), dtype=I32).at[mx.maximum(S.j_city, 0)].add(add).astype(I64)
    flat = mx.take(S.mw, S.j_world) * S.T + S.j_tile
    M = S.feature.shape[0]
    feature = S.feature.reshape(-1)
    kill = mx.zeros((M * S.T + 1,), dtype=I32).at[mx.where(done & (S.j_removed >= 0), flat, M * S.T)].add(1)[:-1] > 0
    feature = mx.where(kill, -1, feature).reshape(M, S.T)
    imp = S.improvement.reshape(-1)
    put = mx.full((M * S.T + 1,), -1, dtype=I32).at[mx.where(done & (S.j_imp >= 0), flat, M * S.T)].maximum(mx.where(done, S.j_imp, -1))[:-1]
    imp = mx.where(put >= 0, put, imp).reshape(M, S.T)
    return prod, feature, imp


def seg_sum(x, seg, n):
    """Σ of the rows of `x` per segment (the GPU scatters 32-bit values: the sums here fit)."""
    dt = x.dtype
    if dt == mx.bool_ or dt == I64:
        x = x.astype(I32)
    out = mx.zeros((n,) + x.shape[1:], dtype=x.dtype).at[seg].add(x)
    return out.astype(I64) if dt == I64 else out


def compact(queue, drop):
    """The queue rows without the `drop` entries (order kept, −1 after)."""
    Q = queue.shape[1]
    pos = mx.arange(Q)[None]
    key = mx.where(drop | (queue == -1), Q + pos, pos)
    order = mx.argsort(key, axis=1)
    q = mx.take_along_axis(queue, order, axis=1)
    d = mx.take_along_axis(drop | (queue == -1), order, axis=1)
    return mx.where(d, -1, q)


def step(model, S, in_order=True):
    """One end of turn of every city and world of `S`. The cities of an empire resolve in parallel
    (pass one); with `in_order`, every city that an earlier city of its turn affected (a completed
    building: owner-wide effects, wonders, national wonders; a golden age; resource copies taken)
    resolves again against what those earlier cities completed (pass two, on those cities only:
    exclusive segmented scans of pass one), as the kernel's city loop orders them."""
    tb = tables(model)
    Cn, W = S.n, S.W
    cw = S.cw
    pre = prepare(model, S)
    zero = {"wide_add": mx.zeros((Cn, model.nb), dtype=I32), "golden_add": mx.zeros((Cn,), dtype=mx.bool_),
            "capbld_add": mx.zeros((Cn, model.nb), dtype=mx.bool_), "took_before": mx.zeros((Cn, tb.nr), dtype=I32), "first": True}
    r = city_pass(model, S, pre, None, zero)
    r["again"] = mx.zeros((Cn,), dtype=mx.bool_)
    if in_order:
        E = excl_scan(r["built_any"].astype(I32), cw, W)
        ov = {"wide_add": E, "golden_add": excl_scan(r["ga_c"].astype(I32)[:, None], cw, W)[:, 0] > 0,
              "capbld_add": excl_scan((r["built_any"] & r["capital"][:, None]).astype(I32), cw, W) > 0,
              "took_before": excl_scan(r["took"], cw, W), "first": False}
        grp = mx.array([g for _, _, g, _ in S.gp_defs], dtype=I32)
        bgrp = mx.sum(r["births"].astype(I32)[:, :, None] * (grp[None, :, None] == mx.arange(4)[None, None, :]), axis=1)  # [C, 4]
        ov["born_before"] = excl_scan(bgrp, cw, W).astype(I64)
        again = mx.any(E > 0, axis=1) | ov["golden_add"] | mx.any(ov["took_before"] > 0, axis=1) | mx.any(ov["born_before"] > 0, axis=1)
        n2 = int(mx.sum(again).item())
        if n2:
            idx = mx.argsort((~again).astype(I32))[:n2]
            ov = {k: (mx.take(v, idx, axis=0) if k != "first" else v) for k, v in ov.items()}
            r2 = city_pass(model, S, pre, idx, ov)
            pos = mx.full((Cn,), -1, dtype=I32).at[idx].maximum(mx.arange(n2, dtype=I32))
            on = pos >= 0
            pz = mx.maximum(pos, 0)
            for k, v in r2.items():
                if hasattr(v, "shape") and v.ndim >= 1 and v.shape[0] == n2 and k in r and r[k].shape[0] == Cn:
                    r[k] = mx.where(on.reshape((-1,) + (1,) * (v.ndim - 1)), mx.take(v, pz, axis=0), r[k])
            r["again"] = again
    return empire(model, S, r)


def prepare(model, S):
    """What both passes read and neither changes: the local planes and their plot yields (the map does
    not move during the cities), the empire's aggregates of the turn's start."""
    tb = tables(model)
    mt = model.tb
    Cn, W, T = S.n, S.W, S.T
    cw = S.cw
    techs = mx.take(S.techs, cw, axis=0)
    flat = mx.take(S.mw, cw)[:, None] * T + mx.maximum(S.lt, 0)
    off = S.lt < 0

    def loc(p, fill):
        return mx.where(off, fill, mx.take(p.reshape(-1), flat))

    feat, res, imp = loc(S.feature, -1), loc(S.resource, -1), loc(S.improvement, -1)
    pill = loc(S.pillaged, False)
    ocity, owner = loc(S.ocity, -1), loc(S.owner, -1)
    st = dict(S.st_static)
    st.update({"feature": feat, "resource": res, "improvement": imp, "pillaged": pill, "owner": mx.full(feat.shape, -1, dtype=I32),
               "city": mx.zeros(feat.shape, dtype=mx.bool_), "techs": techs[:, None, :], "viewer": mx.zeros((Cn,), dtype=I32)})
    raw = econ.plot_yields(mt, S.lnb, st, finish=False)  # [C, NL, 6]
    capital = mx.any(S.bld & tb.palace[None], axis=1)
    has = (S.bld.astype(mx.float32) @ tb.cls) > 0.5  # [C, NB] classes the city has
    return {"techs": techs, "feat": feat, "res": res, "imp": imp, "pill": pill, "ocity": ocity, "owner": owner, "raw": raw, "off": off,
            "capital": capital, "has": has,
            "wide_w": seg_sum(S.bld.astype(I32), cw, W),
            "cap_w": seg_sum((S.bld & capital[:, None]).astype(I32), cw, W) > 0,
            "emp_has_w": seg_sum(mx.where(S.puppet[:, None], 0, (~has).astype(I32)), cw, W) == 0}


def excl_scan(x, seg, n):
    """Per row: Σ of the rows before it in its segment (segments contiguous, in order)."""
    return mx.cumsum(x, axis=0) - x - mx.take(seg_first_cumsum(x, seg, n), seg, axis=0)


def city_pass(model, S, pre, idx, ov):
    """Yields, growth, production and borders of the cities `idx` (None: all), each against `ov` (what
    the earlier cities of its turn completed: none in the first pass)."""
    tb = tables(model)
    nu, nb = model.nu, model.nb
    W = S.W

    def sel(x):
        return x if idx is None else mx.take(x, idx, axis=0)

    cw = sel(S.cw)
    Cn = cw.shape[0]
    A = {k: sel(v) for k, v in S.static.items()}
    A["pop"], A["bld"] = sel(S.pop), sel(S.bld)
    queue0 = sel(S.queue)
    head = queue0[:, 0]
    A["head"] = head
    A["handle"] = sel(S.handle)
    A["wide"] = mx.take(pre["wide_w"], cw, axis=0) + ov["wide_add"]
    A["golden"] = A["golden"] | ov["golden_add"]
    capital = sel(pre["capital"])
    A["capital"] = capital
    cap_bld = mx.take(pre["cap_w"], cw, axis=0) | ov["capbld_add"]
    hb = mx.where(head >= nu, head - nu, 0)
    A["capbuilt"] = (head >= nu) & mx.take_along_axis(cap_bld, hb[:, None], axis=1)[:, 0]
    A["techs"] = sel(pre["techs"])
    A["pol"] = mx.take(S.pol, cw, axis=0)
    feat, res, imp, pill, ocity, owner, raw, off = (sel(pre[k]) for k in ("feat", "res", "imp", "pill", "ocity", "owner", "raw", "off"))
    lt, ring, lnb = sel(S.lt), sel(S.ring), sel(S.lnb)
    river = sel(S.st_static["river"])
    K = C.NCAND
    A["cand"] = lt[:, :K]
    A["craw"] = raw[:, :K]
    A["c_feature"], A["c_resource"], A["c_imp"], A["c_pill"], A["c_ocity"] = feat[:, :K], res[:, :K], imp[:, :K], pill[:, :K], ocity[:, :K]
    A["c_terrain"] = sel(S.terrain)[:, :K]
    for k in ("river", "water", "lake"):
        A["c_" + k] = sel(S.st_static[k])[:, :K]
    A["c_wonder"] = sel(S.st_static["wonder"])[:, :K]
    A["is_centre"] = A["cand"] == sel(S.centre)[:, None]
    A["unworked"] = sel(S.unworked)
    cn = mx.take_along_axis(lnb, sel(S.centre_slot)[:, None, None], axis=1)[:, 0]  # [C, 6] local slots
    A["cn_on"] = cn >= 0
    A["n_feature"] = mx.where(cn >= 0, mx.take_along_axis(feat, mx.maximum(cn, 0), axis=1), -1)
    A["n_imp"] = mx.where(cn >= 0, mx.take_along_axis(imp, mx.maximum(cn, 0), axis=1), -1)
    got = C.evaluate_arrays(model, A)
    rates = got["rates"]
    # --- great people (`accumulate_great_people`, first in the city's turn).
    gpp, births = great_step(model, S, got, sel(S.gpp), sel(S.has_gpp) & ~A["resist"], cw, ov)
    # --- growth (`turn.city_steps`).
    pop = A["pop"].astype(I64)
    spec_count = mx.minimum(mx.sum(got["spec_n"], axis=1).astype(I64), pop)
    eaters = mx.maximum(pop * 100 + spec_count * got["spec_food_pct"].astype(I64), 0)
    food = rates[:, 0].astype(I64) * 100
    surplus = food - 2 * eaters
    hi = mx.where(head < 0, tb.food_unit.shape[0] - 1, head)
    producing_food = mx.take(tb.food_unit, hi)
    excess_c = mx.take(S.excess, cw)
    emp = mx.where(excess_c <= TN.VERY_UNHAPPY_THRESHOLD, TN.VERY_UNHAPPY_GROWTH_PERCENT, mx.where(excess_c < 0, TN.UNHAPPY_GROWTH_PERCENT, 0))
    pct = mx.maximum(100 + got["growth_percent"].astype(I64) + emp, 0)
    diff = mx.where(surplus > 0, surplus * pct // 100, surplus)
    diff = mx.where(producing_food, 0, diff)
    hc = A["handicap"].astype(I64)
    thr = TN.threshold(pop, model.speed["pace_percent"], A["ai"], hc[:, 5], hc[:, 4], A["era"].astype(I64))
    kept_pct = mx.clip(got["food_kept_stat"].astype(I64), 0, 100)
    food0, kept0, prod0 = sel(S.food), sel(S.kept), sel(S.prod)
    store = food0 + diff
    kept = mx.clip(kept0 + TN.trunc_div(diff, 100), 0, thr * kept_pct // 100)
    full = store >= thr * 100
    avoid = A["avoid"]
    used = mx.maximum(thr - kept, 0)
    new_food = mx.where(full & avoid, thr * 100, mx.where(full, mx.maximum(store - used * 100, 0), mx.where(store < 0, 0, store)))
    new_pop = mx.where(full & ~avoid, pop + 1, mx.where((store < 0) & (pop > 1), pop - 1, pop))
    resist = A["resist"]
    new_food = mx.where(resist, food0, new_food)
    kept = mx.where(resist, kept0, kept)
    new_pop = mx.where(resist, pop, new_pop)
    # --- production.
    production = mx.maximum(rates[:, 1], 0).astype(I64) + mx.where(producing_food, TN.food_production(surplus) // 100, 0)
    process = sel(S.process)
    proc = (head == -1) & (process > 0) & ~resist
    conv = mx.maximum(production, 0) * TN.PROCESS_PERCENT // 100
    gold_p = mx.where(proc & (process == 1), conv, 0)
    sci_p = mx.where(proc & (process == 2), conv, 0)
    drains = ~resist & ~proc
    stock = mx.where(drains, prod0 + production, prod0)
    queue = queue0
    bld = A["bld"]
    own_pre = bld.astype(I32)
    others0 = mx.take(pre["wide_w"], cw, axis=0) - own_pre + ov["wide_add"]  # other cities of the empire with it
    # What the earlier cities completed has left this city's queue (the sweep at their completion).
    if not ov["first"]:
        gone0 = (ov["wide_add"] > 0) & (tb.wonder | tb.national)[None]
        qb = mx.where(queue >= nu, queue - nu, 0)
        queue = compact(queue, (queue >= nu) & mx.take_along_axis(gone0, qb, axis=1))
    # NeedsBuilding orders leave first (`sweep_city_unbuildable`).
    has = sel(pre["has"])
    emp_has = pre["emp_has_w"]
    miss = (((~has).astype(mx.float32) @ tb.req_t) > 0.5) | ((((~mx.take(emp_has, cw, axis=0)).astype(mx.float32)) @ tb.emp_t) > 0.5)  # [C, NB]
    qb = mx.where(queue >= nu, queue - nu, 0)
    needs_q = (queue >= nu) & mx.take_along_axis(miss, qb, axis=1)
    queue = mx.where(drains[:, None], compact(queue, needs_q), queue)
    avail = mx.take(S.avail, cw, axis=0) - ov["took_before"]
    wonders = mx.take(S.wonders, cw, axis=0) | ((ov["wide_add"] > 0) & tb.wonder[None])
    stopped = ~drains
    trained = mx.zeros((Cn, nu), dtype=I32)
    pop_add = mx.zeros((Cn,), dtype=I64)
    ga_c = mx.zeros((Cn,), dtype=mx.bool_)
    freep = mx.zeros((Cn,), dtype=I32)
    built_any = mx.zeros((Cn, nb), dtype=mx.bool_)
    took_all = mx.zeros((Cn, tb.nr), dtype=I32)
    rows = mx.arange(Cn)
    cost_pct = got["cost_pct"]
    for _ in range(K_DRAIN):
        head = queue[:, 0]
        isb = head >= nu
        b_ = mx.where(isb, head - nu, 0)
        hi = mx.where(head < 0, tb.needs.shape[0] - 1, head)
        need = mx.take(tb.needs, hi, axis=0)
        short = mx.any((need > 0) & (avail < need), axis=1)

        def g1(m):
            return mx.take_along_axis(m, b_[:, None], axis=1)[:, 0]

        lost = (isb & (g1(bld) | (mx.take(tb.wonder, b_) & g1(wonders)) | (mx.take(tb.national, b_) & (g1(others0) > 0)) | g1(miss))) | short
        active = ~stopped & (head >= 0)
        purch = mx.take(tb.purchase_only, hi)
        cost = C.production_cost_arrays(model, A, head, [cost_pct[:, 0], cost_pct[:, 1]]).astype(I64)
        do_lost = active & lost
        do_build = active & ~lost & ~purch & (stock >= cost)
        if ov["first"]:
            # Two cities of an empire completing one wonder in a pass: the first in the turn's order.
            uniq = isb & (mx.take(tb.wonder, b_) | mx.take(tb.national, b_)) & do_build
            key = cw * nb + b_
            first = mx.full((W * nb + 1,), Cn, dtype=I32).at[mx.where(uniq, key, W * nb)].minimum(mx.where(uniq, rows, Cn))
            late = uniq & (mx.take(first, key) != rows)
            do_lost = do_lost | late
            do_build = do_build & ~late
        stock = mx.where(do_build, stock - cost, stock)
        popq = do_lost | do_build
        queue = mx.where(popq[:, None], mx.concatenate([queue[:, 1:], mx.full((Cn, 1), -1, dtype=I32)], axis=1), queue)
        stopped = stopped | (active & ~popq) | (head < 0)
        # Completions.
        unit_done = do_build & ~isb
        b_done = do_build & isb
        trained = trained + ((mx.arange(nu)[None] == head[:, None]) & unit_done[:, None]).astype(I32)
        onehot_b = (mx.arange(nb)[None] == b_[:, None]) & b_done[:, None]
        here = mx.any(onehot_b[:, :, None] & tb.here[None], axis=1)
        new_b = onehot_b | (here & ~bld)
        bld = bld | new_b
        built_any = built_any | new_b
        extra_b = (new_b & ~onehot_b).astype(I32)
        trained_free = mx.sum(mx.where(onehot_b[:, :, None], tb.free_unit[None], 0), axis=1)  # [C, NU]
        took = (mx.where(do_build[:, None], need, 0) + mx.sum(extra_b[:, :, None] * tb.needs[None, nu:nu + nb], axis=1)
                + mx.sum(trained_free[:, :, None] * tb.needs[None, :nu], axis=1))
        avail = avail - took
        took_all = took_all + took
        trained = trained + trained_free
        pop_add = pop_add + mx.sum(mx.where(onehot_b, tb.pop_grant[None], 0), axis=1).astype(I64)
        ga_c = ga_c | mx.any(onehot_b & tb.ga_grant[None], axis=1)
        freep = freep + mx.sum(mx.where(onehot_b, tb.free_pol[None], 0), axis=1)
        wdone = new_b & (tb.wonder | tb.national)[None]
        wonders = wonders | (wdone & tb.wonder[None])
        qb = mx.where(queue >= nu, queue - nu, 0)
        queue = compact(queue, (queue >= nu) & mx.take_along_axis(wdone, qb, axis=1))
    new_pop = new_pop + mx.where(resist, 0, pop_add)
    # --- borders.
    cul = mx.maximum(rates[:, 4], 0).astype(I32)
    culture0 = sel(S.culture)
    cstock = mx.where(resist, culture0, culture0 + cul)
    mine = ocity == A["handle"][:, None]
    level = mx.sum(mine & (ring >= 2) & (ring <= 5), axis=1)
    cp = got["cost_pct"]
    need_c = mx.take(tb.border.reshape(-1), mx.clip(level, 0, 255) * 201 + mx.clip(cp[:, 2] + 100, 0, 200))
    claim = ~resist & (cstock >= need_c)
    nb_ok = lnb >= 0
    lnb = mx.maximum(lnb, 0).reshape(Cn, -1)
    nmine = mx.any(nb_ok & (mx.take_along_axis(mine.astype(I32), lnb, axis=1).reshape(Cn, NL, 6) > 0), axis=2)
    adj_res = mx.any(nb_ok & (mx.take_along_axis((res >= 0).astype(I32), lnb, axis=1).reshape(Cn, NL, 6) > 0), axis=2)
    cand_ok = (ring >= 1) & (ring <= 5) & (owner < 0) & nmine & ~off
    ys = mx.minimum(mx.sum(mx.maximum(raw, 0), axis=2), tb.ys_cap).astype(I64)
    k1 = ((res < 0).astype(I64) * 2 + (~adj_res).astype(I64)) * 2 + (~river).astype(I64)
    key = ((k1 * 512 + (tb.ys_cap - ys)) * 8 + ring.astype(I64)) * 65536 + mx.maximum(lt, 0).astype(I64)
    key = mx.where(cand_ok, -key, -(2 ** 62))  # argmax of −key = the least key
    best = mx.argmax(key, axis=1)
    found = mx.any(cand_ok, axis=1) & claim
    tile = mx.take_along_axis(lt, best[:, None], axis=1)[:, 0]
    dist = mx.take_along_axis(ring, best[:, None], axis=1)[:, 0]
    return {"got": got, "rates": rates, "pop": new_pop, "food": new_food, "kept": kept, "prod": stock, "queue": queue, "bld": bld,
            "gpp": gpp, "births": births,
            "built_any": built_any, "took": took_all, "trained": trained, "ga_c": ga_c, "freep": freep, "capital": capital,
            "culture": mx.where(claim, cstock - need_c, cstock), "acquired": mx.where(claim, sel(S.acquired) + 1, sel(S.acquired)),
            "radius": mx.where(found, mx.maximum(sel(S.radius), dist), sel(S.radius)), "claim_tile": mx.where(found, tile, -1),
            "cul": mx.where(resist, 0, cul), "gold_c": mx.where(resist, 0, rates[:, 2].astype(I64) + gold_p),
            "sci_c": mx.where(resist, 0, rates[:, 3].astype(I64) + sci_p)}


def great_step(model, S, got, progress, on, cw, ov):
    """Every city's points into its progress; at the threshold of its kind (the empire's births so far,
    the earlier cities' of this turn in the second pass) the progress empties and one is born.
    Returns the progress and the births [C, definitions]."""
    pace = model.speed["pace_percent"]
    spec_n = got["spec_n"].astype(I64)
    gpts = got["gp_points"].astype(I64)
    grate = got["gp_rate"].astype(I64)
    active = on & (mx.any(spec_n > 0, axis=1) | mx.any(gpts > 0, axis=1))
    born = mx.take(S.born, cw, axis=0).astype(I64) + ov.get("born_before", 0)  # [C, 4]
    prate = mx.take(S.gp_prate, cw, axis=0)
    out = progress
    births = []
    for i, (spec, k, grp, paced) in enumerate(S.gp_defs):
        if spec < 0:
            births.append(mx.zeros(on.shape, dtype=mx.bool_))
            continue
        count = spec_n[:, spec]
        bpts = mx.maximum(gpts[:, k], 0) if k >= 0 else 0
        base = (S.spec_gpp[spec] * count + bpts) * 100
        rate = (grate[:, k] if k >= 0 else 0) + prate[:, i]
        pts = mx.where(base == 0, 0, mx.minimum(base * mx.maximum(100 + rate, 0) // 100, 2 ** 32 - 1))
        n = mx.minimum(born[:, grp], 24)
        thr = mx.left_shift(mx.full(n.shape, GP.POINTS_FOR_NEXT, dtype=I64), n)
        if paced:
            thr = mx.maximum(thr * pace // 100, 1)
        cur = mx.where(active, mx.minimum(out[:, spec] + pts, 2 ** 32 - 1), out[:, spec])
        b = active & (cur // 100 >= thr)
        cur = mx.where(b, 0, cur)
        out = mx.where(mx.arange(out.shape[1])[None] == spec, cur[:, None], out)
        born = born + mx.where((mx.arange(4)[None] == grp) & b[:, None], 1, 0)
        births.append(b)
    return out, mx.stack(births, axis=1)


def empire(model, S, r):
    """After the cities: the wonders completed this turn leave every queue of their empire; free
    buildings in the first cities without one; the worker jobs; the empire's sums."""
    tb = tables(model)
    nu = model.nu
    Cn, W = S.n, S.W
    cw = S.cw
    queue, bld = r["queue"], r["bld"]
    wdone = mx.take(seg_sum((r["built_any"] & (tb.wonder | tb.national)[None]).astype(I32), cw, W) > 0, cw, axis=0)
    qb = mx.where(queue >= nu, queue - nu, 0)
    queue = compact(queue, (queue >= nu) & mx.take_along_axis(wdone, qb, axis=1))
    if tb.has_grant:
        gcount = mx.take(seg_sum(mx.sum(r["built_any"][:, :, None].astype(I32) * tb.grant[None], axis=1), cw, W), cw, axis=0)  # [C, NB]
        lack = (~bld).astype(I32)
        rank = mx.cumsum(lack, axis=0) - mx.take(seg_first_cumsum(lack, cw, W), cw, axis=0)
        bld = bld | ((lack > 0) & (rank <= gcount))
    stock, feature_after, imp_after = jobs_step(S, r["prod"])
    sci_w = seg_sum(r["sci_c"], cw, W)
    gs = S.gold_stage
    gross = mx.maximum((seg_sum(r["gold_c"], cw, W) + gs[:, 0]) * (100 + gs[:, 1]), 0) // 100
    bm = seg_sum(mx.sum(mx.where(bld, tb.maint[None], 0), axis=1).astype(I64), cw, W)
    out = dict(r)
    out.update({"queue": queue, "bld": bld, "prod": stock, "feature": feature_after, "improvement": imp_after, "gross": gross, "bmaint": bm,
                "sci_w": sci_w, "cul_w": seg_sum(r["cul"], cw, W), "ga_w": seg_sum(r["ga_c"].astype(I32), cw, W) > 0,
                "freep_w": seg_sum(r["freep"], cw, W)})
    # Science before the deficit (`player_science`, scholars).
    hp = mx.where((S.sci_pct > 0) & (sci_w > 0) & (S.excess >= 0), sci_w * S.sci_pct // 100, 0)
    sci = mx.maximum(sci_w, 0) + hp + S.ally
    head_t = S.rqueue[:, 0]
    sch = (head_t >= 0) & mx.take_along_axis(S.scholars, mx.maximum(head_t, 0)[:, None], axis=1)[:, 0]
    sci = mx.where(sch, sci * (100 + TN.SCHOLARS_PERCENT) // 100, sci)
    out["sci_pre_deficit"] = mx.maximum(sci, 0)
    return out


def seg_first_cumsum(x, seg, n):
    """Per segment the inclusive cumsum of `x` just before the segment's first row (segments contiguous)."""
    cs = mx.cumsum(x, axis=0)
    first = mx.full((n,), x.shape[0], dtype=I32).at[seg].minimum(mx.arange(x.shape[0], dtype=I32))
    before = mx.where((first > 0)[:, None], mx.take(cs, mx.maximum(first - 1, 0), axis=0), 0)
    return before


def load_treasury(model, game, S):
    """The treasury's per-world terms: the difficulty's percents, the player's stats (as the turn's
    world reads them before its end turn), garrisons and capital connections (the turn's inputs)."""
    er = game["rules"]
    prow4 = model.__dict__["player_rows4"]
    rows = []
    for t in S.turns:
        st = t["pre"]["state"]
        hc = TR.handicap(er, st, t["player"])
        stats, _ = P.stats_and_costs(prow4, t, "pre")
        ai = st.get("ai", er["players"][t["player"]]["ai"])
        rows.append([hc["ai_unit_cost_percent"] if ai else hc["unit_cost_percent"], hc["ai_building_cost_percent"] if ai else hc["building_cost_percent"],
                     hc["route_cost_percent"], max(hc["gold_free_units"], 0), max(stats.get(TR.FREE_MAINT, 0), 0), int(stats.get(TR.GARRISON_FREE, 0) > 0),
                     int(stats.get(TR.FREE_HILLS, 0) > 0), stats.get(TR.ROUTE_PCT, 0), stats.get(TR.CONN_PCT, 0), int(bool(st.get("army_tax"))),
                     st.get("round", t["round"]), int("upkeep" in st)])
    S.tr = mx.array(rows, dtype=I64)
    own = S.owner == S.player[:, None]
    hills_free = (S.tr[:, 6] > 0)[:, None] & S.hills[None]
    rv = mx.where(S.route == 1, TR.ROAD, mx.where(S.route == 2, TR.RAILROAD, 0))
    S.route_base = mx.sum(mx.where(own & (S.cityh < 0) & ~hills_free, rv, 0), axis=1).astype(I64)
    post = []
    for ti, c in S.batch.cities:
        pc = {x["h"]: x for x in S.turns[ti]["post"]["state"]["cities"]}.get(c["h"])
        post.append(((pc or c)["inputs"]["garrison"], (pc or c)["inputs"]["connected"]))
    S.garrison_post = mx.array([g for g, _ in post])
    S.connected_post = mx.array([k for _, k in post])
    S.max_rounds = TR.max_rounds(er)
    # ConnectionGoldPercent of an owner-wide building (Machu Picchu) completed in the turn counts at
    # the Gold step: per building its contribution (the player table's rows of that building).
    contrib = [0] * model.nb
    for row in prow4.static:
        if row["src"][0] == "wide":
            pm = P._parsed(row)
            if pm and pm[0] == "stat" and pm[1] == TR.CONN_PCT:
                contrib[row["src"][1]] += pm[2]
    S.conn_pct_of = mx.array(contrib, dtype=I64)


def finish(model, game, S, out):
    """The empire's steps after the cities, on the device: the Gold step (the upkeep of the world the
    cities left: units with those trained, buildings, routes with the claimed tiles, connections with
    the new sizes; the unit cost's fixed-point power on the host, one value per empire), the science
    less a deficit, the research queue."""
    if not hasattr(S, "tr"):
        load_treasury(model, game, S)
    tb = tables(model)
    W, T = S.W, S.T
    cw = S.cw
    tr = S.tr
    # The units the cities trained and the great people born in them.
    born_kind = mx.take(S.gp_unit, cw, axis=0)  # [C, ND]
    born_units = mx.sum(((mx.arange(model.nu)[None, None, :] == born_kind[:, :, None]) & out["births"][:, :, None]).astype(I32), axis=1)
    units = S.unit_counts + seg_sum(out["trained"] + born_units, cw, W)
    grp = mx.array([g for _, _, g, _ in S.gp_defs], dtype=I32)
    out["born"] = S.born + seg_sum(mx.sum(out["births"].astype(I32)[:, :, None] * (grp[None, :, None] == mx.arange(4)[None, None, :]), axis=1), cw, W)
    wsum = mx.sum(units.astype(I64) * S.weights, axis=1)
    has_cities = seg_sum(mx.ones((S.n,), dtype=I32), cw, W) > 0
    garrisons = mx.where(tr[:, 5] > 0, seg_sum(S.garrison_post.astype(I32), cw, W).astype(I64), 0)
    free = TR.FREE_UNITS + tr[:, 3] + garrisons + tr[:, 4]
    paid = mx.maximum(wsum - free * 100, 0)
    support = paid * TR.GOLD_PER_UNIT_TIMES100 // 100
    # Routes: the empire's tiles (with the ones its borders claimed this turn) without a city.
    ct = out["claim_tile"]
    flat = mx.take(S.mw, cw) * T + mx.maximum(ct, 0)
    rt_ = mx.take(S.route.reshape(-1), flat)
    free_c = (mx.take(tr[:, 6], cw) > 0) & mx.take(S.hills, mx.maximum(ct, 0))
    add_c = mx.where((ct >= 0) & (mx.take(S.cityh.reshape(-1), flat) < 0) & ~free_c, mx.where(rt_ == 1, TR.ROAD, mx.where(rt_ == 2, TR.RAILROAD, 0)), 0)
    rsum = S.route_base + seg_sum(add_c, cw, W).astype(I64)
    rsum = rsum * mx.maximum(100 + tr[:, 7], 0) // 100
    routes = rsum * mx.maximum(tr[:, 2], 0) // 100
    buildings = out["bmaint"] * mx.maximum(tr[:, 1], 0) // 100
    # Connections: the cities reached from the capital, sizes after the growth.
    cap = mx.any(out["bld"] & tb.palace[None], axis=1)
    pop = out["pop"].astype(I64)
    cap_pop = seg_sum(mx.where(cap, pop, 0), cw, W)
    has_cap = seg_sum(cap.astype(I32), cw, W) > 0
    g = TR.CONN_BASE + mx.take(cap_pop, cw) * TR.CONN_CAPITAL + pop * TR.CONN_CITY
    had = seg_sum(S.bld.astype(I32), cw, W) > 0
    new_w = (seg_sum(out["bld"].astype(I32), cw, W) > 0) & ~had  # first of its kind in the empire this turn
    cp_w = tr[:, 8] + mx.sum(mx.where(new_w, S.conn_pct_of[None], 0), axis=1)
    cp = mx.take(cp_w, cw)
    g = mx.where(cp == 0, g, TN.trunc_div(g * (100 + cp), 100))
    reached = connections(S, out["bld"], cap)
    out["reached"] = reached
    conn = seg_sum(mx.where(reached & ~cap & mx.take(has_cap, cw), g, 0), cw, W)
    conn = TN.trunc_div(conn, 100)
    # The unit cost's power, per empire on the host (cached by its arguments).
    mx.eval(support)
    sl, rl = support.tolist(), tr[:, 10].tolist()
    uc = mx.array([_unit_cost(s_, r_, S.max_rounds) for s_, r_ in zip(sl, rl)], dtype=I64)
    uc = uc * mx.maximum(tr[:, 0], 0) // 100
    uc = mx.where(tr[:, 9] > 0, uc * (100 + TR.ARMY_TAX_PERCENT) // 100, uc)
    uc = mx.where(has_cities, mx.minimum(uc, 2 ** 31 - 1), 0)
    net = out["gross"] - (uc + buildings + routes - conn)
    after = S.treasury + net
    treasury = mx.where(tr[:, 11] > 0, mx.clip(after, 0, 2 ** 32 - 1), S.treasury)
    deficit = mx.where(tr[:, 11] > 0, mx.minimum(after, 0), 0)
    sci = mx.maximum(out["sci_pre_deficit"] + deficit, 0)
    stock = S.science + sci
    boost = seg_sum(mx.any(out["built_any"] & cap[:, None] & S.sci_bld[None], axis=1).astype(I32), cw, W) > 0
    stock = stock + mx.where(boost, S.korea, 0)
    known = S.techs
    rq = S.rqueue
    for _ in range(K_RESEARCH):
        head = rq[:, 0]
        hk = mx.maximum(head, 0)
        is_known = (head >= 0) & mx.take_along_axis(known, hk[:, None], axis=1)[:, 0]
        prereq = mx.all(~mx.take(S.tech_req, hk, axis=0) | known, axis=1)
        cost = mx.take_along_axis(S.tech_cost, hk[:, None], axis=1)[:, 0]
        learn = (head >= 0) & ~is_known & prereq & (stock >= cost)
        stock = mx.where(learn, stock - cost, stock)
        known = known | ((mx.arange(known.shape[1])[None] == hk[:, None]) & learn[:, None])
        popq = is_known | learn
        rq = mx.where(popq[:, None], mx.concatenate([rq[:, 1:], mx.full((W, 1), -1, dtype=I32)], axis=1), rq)
    # The Culture step: the cities' culture, the city-states, the heritage sites, the excess
    # happiness; the league's boost.
    ci = S.cul_in
    wonders = seg_sum(mx.sum((out["bld"] & tb.world_wonder[None]).astype(I32), axis=1), cw, W).astype(I64)
    happy = mx.where(S.excess > 0, S.excess * ci[:, 5] // 100, 0)
    total = out["cul_w"].astype(I64) + ci[:, 2] + mx.where(ci[:, 3] > 0, game["rules"].get("heritage_per_wonder", 2) * wonders, 0) + happy
    total = total * (100 + mx.where(ci[:, 4] > 0, game["rules"].get("boost_percent", 75), 0)) // 100
    stock = stock + S.ra_pay
    out.update({"science": stock, "techs": known, "rqueue": rq, "treasury": treasury, "upkeep": [uc, buildings, routes, conn], "units": units,
                "culture_pool": ci[:, 0] + total, "culture_lifetime": ci[:, 1] + total})
    return out


def advance(S, out):
    """The state after the step (a closed loop: the next step reads it; the turn's inputs stay)."""
    S.pop = out["pop"].astype(I32)
    S.food, S.kept, S.prod = out["food"], out["kept"], out["prod"]
    S.culture, S.acquired, S.radius = out["culture"], out["acquired"], out["radius"]
    S.queue, S.bld = out["queue"], out["bld"]
    S.feature, S.improvement = out["feature"], out["improvement"]
    M = S.ocity.shape[0]
    flat = mx.take(S.mw, S.cw) * S.T + mx.maximum(out["claim_tile"], 0)
    at = mx.where(out["claim_tile"] >= 0, flat, M * S.T)
    S.ocity = mx.concatenate([S.ocity.reshape(-1), mx.array([-1], dtype=I32)]).at[at].maximum(mx.where(out["claim_tile"] >= 0, S.handle, -1))[:-1].reshape(M, S.T)
    S.owner = mx.concatenate([S.owner.reshape(-1), mx.array([-1], dtype=I32)]).at[at].maximum(mx.where(out["claim_tile"] >= 0, mx.take(S.player, S.cw), -1))[:-1].reshape(M, S.T)
    S.techs, S.science, S.rqueue, S.treasury = out["techs"], out["science"], out["rqueue"], out["treasury"]
    S.unit_counts = out["units"]
    S.gpp, S.born = out["gpp"], out["born"]
    S.cul_in = mx.concatenate([out["culture_pool"][:, None], out["culture_lifetime"][:, None], S.cul_in[:, 2:]], axis=1)
    S.avail = S.avail - seg_sum(out["took"], S.cw, S.W)
    return S


def unit_part(hc, stats, w, weights, max_rounds, army_tax):
    if not w.cities:
        return 0
    garrisons = sum(1 for c in w.cities if c["garrison"]) if stats.get(TR.GARRISON_FREE, 0) > 0 else 0
    free = TR.FREE_UNITS + max(hc["gold_free_units"], 0) + garrisons + max(stats.get(TR.FREE_MAINT, 0), 0)
    paid = max(weights - free * 100, 0)
    cost = _unit_cost(paid * TR.GOLD_PER_UNIT_TIMES100 // 100, w.round, max_rounds)
    pct = hc["ai_unit_cost_percent"] if w.ai else hc["unit_cost_percent"]
    cost = cost * max(pct, 0) // 100
    if army_tax:
        cost = cost * (100 + TR.ARMY_TAX_PERCENT) // 100
    return min(cost, 2 ** 31 - 1)


_COST = {}


def _unit_cost(support, turn, end):
    k = (support, turn, end)
    if k not in _COST:
        _COST[k] = TR.unit_cost_at(support, turn, end)
    return _COST[k]


def compare(model, S, out):
    """Per city and per world: the device step against the dump's post (and the harness's causes)."""
    nu = model.nu
    keys = ("pop", "food", "kept", "prod", "culture", "acquired", "radius")
    vals = {k: out[k].tolist() for k in keys}
    ql, bl = out["queue"].tolist(), out["bld"].tolist()
    gl = out["gpp"].tolist()
    s = {"cities": 0, "cities_ok": 0, "worlds": 0, "worlds_ok": 0}
    fields = {}
    ex = []
    for i, (ti, c) in enumerate(S.batch.cities):
        t = S.turns[ti]
        post = {x["h"]: x for x in t["post"]["state"]["cities"]}.get(c["h"])
        if post is None:
            continue
        s["cities"] += 1
        want = {k: post[k] for k in keys}
        want["queue"] = [C.item_of(q, nu) for q in post["queue"]]
        want["buildings"] = sorted(post["buildings"])
        mine = {k: vals[k][i] for k in keys}
        mine["queue"] = [q for q in ql[i] if q != -1]
        mine["buildings"] = [b for b, v in enumerate(bl[i]) if v]
        if "gpp" in post:
            want["gpp"] = post["gpp"]
            mine["gpp"] = gl[i][:len(post["gpp"])]
        ok = True
        for k in want:
            e = want[k] == mine[k]
            fields[k] = fields.get(k, 0) + int(e)
            ok &= e
        s["cities_ok"] += int(ok)
        if not ok and len(ex) < 6:
            ex.append({"round": t["round"], "player": t["player"], "city": c["h"], "bad": [k for k in want if want[k] != mine[k]],
                       "want": {k: want[k] for k in want if want[k] != mine[k]}, "mlx": {k: mine[k] for k in want if want[k] != mine[k]}})
    sci, tr = out["science"].tolist(), out["treasury"].tolist()
    techs = out["techs"].tolist()
    rq = out["rqueue"].tolist()
    cpool, clife = out["culture_pool"].tolist(), out["culture_lifetime"].tolist()
    up = [u.tolist() for u in out["upkeep"]]
    reached = out["reached"].tolist()
    for i, (ti, c) in enumerate(S.batch.cities):
        st = S.turns[ti]["pre"]["state"]
        if "reached" in st:
            s["reach_cities"] = s.get("reach_cities", 0) + 1
            s["reach_ok"] = s.get("reach_ok", 0) + int(reached[i] == (c["h"] in st["reached"]))
    for wi, t in enumerate(S.turns):
        post = t["post"]["state"]
        s["worlds"] += 1
        ok = {"science": post["science"] == sci[wi], "techs": post["techs"] == "".join("1" if x else "0" for x in techs[wi]),
              "rqueue": post["queue"] == [q for q in rq[wi] if q >= 0], "treasury": post["treasury"] == tr[wi]}
        if "culture" in post:
            ok["culture"] = (post["culture"]["pool"], post["culture"]["lifetime"]) == (cpool[wi], clife[wi])
        if not all(ok.values()) and len([e for e in ex if "world" in e]) < 4:
            ex.append({"world": wi, "round": t["round"], "player": t["player"], "bad": [k for k, v in ok.items() if not v],
                       "treasury": [t["pre"]["state"]["treasury"], post["treasury"], tr[wi]], "science": [post["science"], sci[wi]],
                       "upkeep": [post.get("upkeep"), [u[wi] for u in up]]})
        for k, v in ok.items():
            fields["p_" + k] = fields.get("p_" + k, 0) + int(v)
        s["worlds_ok"] += int(all(ok.values()))
    return s, fields, ex


def run(path, limit_games=None, chunk=64, show=8):
    tot, fields, ex = {}, {}, []
    model = None
    t_step = 0.0
    for g, game in C.each_game(path, limit_games):
        if model is None:
            model = C.Model(game["tables"], game["rules"])
        turns = [t for t in game["turns"] if t["pre"]["state"]["cities"]]
        for k in range(0, len(turns), chunk):
            S = load(model, game, turns[k:k + chunk])
            t1 = time.time()
            out = step(model, S)
            out = finish(model, game, S, out)
            mx.eval(out["pop"], out["queue"], out["science"])
            t_step += time.time() - t1
            s, f, e = compare(model, S, out)
            s["second_pass"] = int(mx.sum(out["again"]).item())
            for key, v in s.items():
                tot[key] = tot.get(key, 0) + v
            for key, v in f.items():
                fields[key] = fields.get(key, 0) + v
            for x in e:
                if len(ex) < show:
                    x["game"] = g
                    ex.append(x)
    return {**tot, "fields": fields, "examples": ex, "step_s": round(t_step, 2)}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--games", type=int)
    ap.add_argument("--show", type=int, default=8)
    ap.add_argument("--bench")
    o = ap.parse_args(argv)
    if o.bench:
        print(json.dumps(bench(o.dump, [int(x) for x in o.bench.split(",")]), indent=1))
        return
    rep = run(o.dump, o.games, show=o.show)
    print(f"device step: cities {rep.get('cities_ok', 0)}/{rep.get('cities', 0)}, worlds {rep.get('worlds_ok', 0)}/{rep.get('worlds', 0)} equal; "
          f"cities reached from the capital (on the turn's world) {rep.get('reach_ok', 0)}/{rep.get('reach_cities', 0)}; "
          f"cities resolved again in order (pass two) {rep.get('second_pass', 0)}; "
          f"fields {rep['fields']}; step {rep['step_s']} s")
    for e in rep["examples"]:
        print("  ex:", json.dumps(e))


CITY_KEYS = ("city_node", "city_seas", "gpp", "has_gpp", "pop", "bld", "handle", "food", "kept", "prod", "culture", "acquired", "radius", "queue", "process", "lt", "ring", "lnb",
             "centre_slot", "centre", "terrain", "unworked", "puppet", "garrison_post", "connected_post")
WORLD_KEYS = ("korea", "route_base", "mw", "ra_pay", "r_nbr", "born", "gp_prate", "gp_unit", "cul_in", "player", "techs", "pol", "science",
              "treasury", "rqueue", "tech_cost", "sci_pct", "ally", "scholars", "gold_stage", "excess", "wonders", "avail", "unit_counts",
              "weights", "rate", "tr")


def replicate(S, r):
    """`r` copies of the state side by side on the device (independent empires): the bench's sizes
    without loading the dump again."""
    R_ = State()
    R_.__dict__.update(S.__dict__)
    for k in CITY_KEYS:
        setattr(R_, k, mx.concatenate([getattr(S, k)] * r, axis=0))
    for k in WORLD_KEYS:
        setattr(R_, k, mx.concatenate([getattr(S, k)] * r, axis=0))
    R_.static = {k: mx.concatenate([v] * r, axis=0) for k, v in S.static.items()}
    R_.st_static = {k: mx.concatenate([v] * r, axis=0) for k, v in S.st_static.items()}
    R_.cw = mx.concatenate([S.cw + i * S.W for i in range(r)])
    R_.j_world = mx.concatenate([S.j_world + i * S.W for i in range(r)])
    R_.j_city = mx.concatenate([mx.where(S.j_city >= 0, S.j_city + i * S.n, -1) for i in range(r)])
    for k in ("j_tile", "j_left", "j_imp", "j_removed", "j_amount"):
        setattr(R_, k, mx.concatenate([getattr(S, k)] * r))
    R_.n, R_.W = S.n * r, S.W * r
    R_.turns = S.turns * r
    return R_


def bench(path, sizes, game_index=0, steps=5, in_order=True, chunk=25000):
    """City-turns per second with the state on the device: the turn lines of one game replicated to
    `sizes` cities, loaded once; then `steps` end turns in a closed loop (each step reads the state
    the previous one left: sizes, stocks, queues, buildings, borders, research, treasury)."""
    for i, (g, game) in enumerate(C.each_game(path)):
        if i == game_index:
            break
    model = C.Model(game["tables"], game["rules"])
    turns = [t for t in game["turns"] if t["pre"]["state"]["cities"]]
    turns = turns[len(turns) // 2:]  # the second half of the game: grown empires
    need, k = min(sizes), 0
    while k < len(turns) and sum(len(t["pre"]["state"]["cities"]) for t in turns[:k]) < need:
        k += 1
    turns = turns[:k]
    t0 = time.time()
    base = load(model, game, turns)
    load_treasury(model, game, base)
    mx.eval(base.pop, base.queue, base.avail)
    t_load = time.time() - t0
    out_rows = []
    for n in sizes:
        r = max(1, round(n / base.n))
        # The state in resident parts of at most `chunk` cities (the peak memory of one step's
        # temporaries), each stepped in turn.
        per = max(1, min(r, chunk // base.n))
        parts = [replicate(base, min(per, r - i)) for i in range(0, r, per)]
        for S in parts:
            mx.eval(S.pop, S.queue, S.lt)
        times = []
        for s_ in range(steps + 1):
            t1 = time.time()
            for j, S in enumerate(parts):
                out = step(model, S, in_order=in_order)
                out = finish(model, game, S, out)
                parts[j] = advance(S, out)
                mx.eval(parts[j].pop, parts[j].food, parts[j].prod, parts[j].queue, parts[j].bld, parts[j].culture, parts[j].ocity, parts[j].science,
                        parts[j].treasury, parts[j].techs)
            times.append(time.time() - t1)
        dt = sum(times[1:]) / steps
        cities = sum(S.n for S in parts)
        out_rows.append({"cities": cities, "empires": sum(S.W for S in parts), "parts": len(parts), "load_s (once, JSON)": round(t_load, 2),
                         "step_s": round(dt, 4), "city_turns_per_s": round(cities / dt), "steps": [round(x, 4) for x in times]})
    return out_rows


if __name__ == "__main__":
    main()
