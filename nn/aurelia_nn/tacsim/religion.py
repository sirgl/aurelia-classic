"""The economy on the GPU (MLX), part 5: religion, exact to the Rust rules (`derived::religion`,
`classic::religion_effects`, `systems::religion`).

    python -m aurelia_nn.tacsim.religion DUMP.jsonl|DIR    # parity: beliefs, counts, faith, pressure

The state (the dump's `world.religion`, every city of the world): per city its pressure per creed
(1000 per citizen; creed 0 atheism, 1 the owner's pantheon, 2 + id a founded religion) and the
population it was last reconciled with; the founded religions (founder, holy city, beliefs); per player
its faith, pantheon, religion and prophets; the prophets' RNG stream.

- followers and majority (`followers`, `majority`): one array program over [cities, creeds] — the
  share of the pressure per citizen, the remainders handed out by rank (larger first, the lower creed
  on a tie), the majority as the most followers then the most pressure, at least half of the citizens;
- the beliefs that reach a city (its majority's beliefs, the second religion's pantheon under
  Religious Tolerance, the holy city's effects with their counts) and the founder's beliefs of a player
  with the religion's followers and cities counted: the rows `city.py` and `player.py` read (they
  replace the dump's lists);
- the religious counts of the effect table (MajorityFollowers, ReligionsPresent,
  PeacefulForeignFollowers);
- the Faith step: the cities' faith through the player's Faith modifiers, the city-states' faith, the
  Great Prophet roll on its own xorshift stream;
- the round's pressure (`religious_pressure_round`, at the end of the last player's turn): holy cities,
  every city with a religious majority on the cities within its reach and along its trade routes (the
  origin's owner's route percent), more on friendly city-states (Religious Unity), spies (Underground
  Sect); a city never touched gets its atheists, growth since the last touch goes to the majority.

Inside a turn the followers move with the cities' growth: the founder's counted beliefs at the Gold and
Faith steps are recounted on the populations the cities left (the 250 treasury mismatches of part 4).
"""

import argparse
import json
import re
import time

import mlx.core as mx

from . import rdebug as R

I64 = mx.int64
PRESSURE_PER_POP = 1000
HOLY_CITY_PRESSURE = 5
ADJACENT_CITY_DISTANCE = 10
PROPHET_BASE_CHANCE = 5
FIRST_PROPHET_FAITH, NEXT_PROPHET_FAITH = 200, 100
INDUSTRIAL_ERA = 4
STATS = ("PressureOut", "SpreadDistance", "MissionaryStrength", "ProphetStrength", "ProphetCost", "MissionaryCost",
         "FriendlyCityStatePressure", "OtherReligionErosion", "PressureRetention")
ABILITIES = ("ConvertsBarbarians", "FaithPurchaseAllGreatPeople")
FAITH_INDEX = 6  # the player's yields: Output 0 .. Culture 5, the module's Faith 6
_MOD = re.compile(r"modifier: (.*), duration:")
_PER_CITY = re.compile(r"^Module\(AddYieldPerFollowingCity \{ yield_kind: (.*), amount: (-?\d+), foreign_only: (true|false) \}\)$")
_PER_FOLLOWERS = re.compile(r"^Module\(AddYieldPerFollowers \{ yield_kind: (.*), per: (\d+), foreign_only: (true|false) \}\)$")
_HAPPY_PER_CITY = re.compile(r"^Module\(AddHappinessPerFollowingCity \{ hundredths: (-?\d+) \}\)$")
_SELECTOR = re.compile(r"selector: (Module\(\w+\)|\w+)")


def creed_of_religion(r):
    return 2 + r


class Rules:
    """The beliefs' effects by bucket and selector, the religion numbers they change, the policies of
    Religious Tolerance, the buildings' pressure percents, the speed's numbers."""

    def __init__(self, er):
        self.er = er
        nb = len(er.get("beliefs", []))
        self.n_beliefs = nb
        self.city = [[] for _ in range(nb)]  # (text, selector)
        self.tile = [[] for _ in range(nb)]
        self.player = [[] for _ in range(nb)]
        self.module = [[] for _ in range(nb)]
        self.has_passive = False
        for src, sid, bucket, text in er["effects"]:
            if src != "belief":
                continue
            m = _SELECTOR.search(text)
            sel = m.group(1) if m else ""
            if bucket in ("city", "tile", "player", "module"):
                getattr(self, bucket)[sid].append((text, sel))
                if bucket in ("city", "tile"):
                    self.has_passive = True
        self.category = [R.tag(R.body(R.parse(b["category"]))[0]) if b["category"].startswith("Some") else None
                         for b in er.get("beliefs", [])]
        self.second_pantheon = {sid for src, sid, bucket, text in er["effects"] if src == "policy" and "SecondReligionPantheon" in text}
        rr = er.get("religion_rules", {})
        self.adjacent = rr.get("adjacent", 90)
        self.max_religions = rr.get("max_religions", 1)
        self.faith_percent = rr.get("faith_percent", 100)
        self.pressure_pct = {}
        for b, d in enumerate(er["buildings"]):
            n = 0
            for a in d.get("abilities", []):
                v = R.parse(a)
                if R.tag(v) == "ReligiousPressure":
                    n += R.body(v)[0]
            if n:
                self.pressure_pct[b] = n
        self.prophet_unit = next((g["unit"] for g in er["great_people"] if g["source"] == "Faith"), None)

    def religion_mods(self, beliefs, techs):
        """`religion_modifiers`: the percents of the religion's numbers (index of `STATS`), the prophet
        cost percent, the abilities, from its beliefs' module rows whose conditions hold for the
        founder (`techs`: the founder's technology bits)."""
        pct = [0] * len(STATS)
        cost = 0
        ab = [False] * len(ABILITIES)
        for b in beliefs:
            for text, _ in self.module[b]:
                e = R.body(R.parse(text))
                if not all(_holds(e[k], techs) for k in ("condition", "second_condition")):
                    continue
                m = e["modifier"]
                if R.tag(m) != "Module":
                    continue
                inner = R.body(m)[0]
                t = R.tag(inner)
                if t == "ModifyReligion":
                    f = R.body(inner)
                    k = STATS.index(f["stat"])
                    pct[k] += f["percent"]
                    if f["stat"] == "ProphetCost":
                        cost += f["percent"]
                elif t == "GrantReligionAbility":
                    ab[ABILITIES.index(R.body(inner)[0])] = True
        return pct, cost, ab


def _holds(cond, techs):
    """A founder-level condition of a religion row (`effect_holds` with the founder only)."""
    t = R.tag(cond)
    if t == "Always":
        return True
    if t == "HasTechnology":
        return techs[R.arg(R.body(cond)[0])] == "1"
    if t == "LacksTechnology":
        return techs[R.arg(R.body(cond)[0])] != "1"
    raise ValueError(f"religion row condition {cond}")


def world(st):
    """The religion state of a turn line's side as Python structures."""
    wr = st["world"]["religion"]
    cities = []
    for row in wr["cities"]:
        h, owner, tile, pop, pressure, fpop, paid, majority, capital, pressing = row
        cities.append({"h": h, "owner": owner, "tile": tile, "pop": pop, "pressure": {c: a for c, a in pressure}, "fpop": fpop,
                       "paid": paid, "majority": majority, "capital": capital, "pressing": pressing})
    religions = {r[0]: {"id": r[0], "founder": r[1], "holy": r[2], "beliefs": r[3], "enhanced": r[4], "pct": r[5], "cost": r[6], "ab": r[7]}
                 for r in wr["religions"]}
    players = [{"faith": f, "pantheon": pa, "religion": re_, "prophets": pr} for f, pa, re_, pr in wr["players"]]
    return {"cities": cities, "religions": religions, "players": players, "pantheons": wr["pantheons"], "rng": int(wr["rng"]),
            "left": wr["left"], "spies": wr["spies"]}


# ---- followers and majority as one array program over [cities, creeds] ----

def creeds(w):
    """The creed columns of a world: atheism, pantheon, each religion id that has pressure or is founded."""
    ids = set(w["religions"])
    for c in w["cities"]:
        ids |= {k - 2 for k in c["pressure"] if k >= 2}
    return [0, 1] + [2 + r for r in sorted(ids)]


def pressure_array(w, cols):
    at = {c: i for i, c in enumerate(cols)}
    rows = []
    for c in w["cities"]:
        r = [0] * len(cols)
        for k, a in c["pressure"].items():
            r[at[k]] = a
        rows.append(r)
    return mx.array(rows, dtype=I64) if rows else mx.zeros((0, len(cols)), dtype=I64)


def followers(P, pop):
    """`derived::religion::followers` for every city: [C, K] followers (creeds without pressure 0)."""
    pop = pop.astype(I64)
    total = mx.sum(P, axis=1)
    per = mx.maximum(total // mx.maximum(pop, 1), 1)
    n = P // per[:, None]
    rem = P - n * per[:, None]
    short = pop - mx.sum(n, axis=1)
    K = P.shape[1]
    idx = mx.arange(K)
    # Rank among the creeds with a remainder: larger first, the lower creed on a tie.
    ahead = (rem[:, None, :] > rem[:, :, None]) | ((rem[:, None, :] == rem[:, :, None]) & (idx[None, None, :] < idx[None, :, None]))
    ahead = ahead & (rem[:, None, :] > 0)
    rank = mx.sum(ahead.astype(I64), axis=2)
    extra = (rem > 0) & (rank < short[:, None])
    n = mx.minimum(n + extra.astype(I64), pop[:, None])
    atheists = mx.where(idx[None, :] == 0, pop[:, None], 0)
    n = mx.where((total == 0)[:, None], atheists, n)
    return mx.where((pop > 0)[:, None], n, 0)


def majority(P, N):
    """`derived::religion::majority`: the column of the majority creed, −1 without one."""
    K = P.shape[1]
    idx = mx.arange(K)
    has = N > 0
    # The first creed with the largest (followers, pressure) among those with followers.
    key_n = mx.where(has, N, -1)
    best_n = mx.max(key_n, axis=1, keepdims=True)
    cand = has & (N == best_n)
    best_p = mx.max(mx.where(cand, P, -1), axis=1, keepdims=True)
    cand = cand & (P == best_p)
    first = mx.min(mx.where(cand, idx[None, :], K), axis=1)
    total = mx.sum(N, axis=1)
    n_best = mx.squeeze(best_n, 1)
    ok = (first < K) & (first != 0) & (n_best >= total // 2)
    return mx.where(ok, first, -1)


def facts(w):
    """Followers and majority of every city of the world (Python lists, creed codes)."""
    cols = creeds(w)
    if not w["cities"]:
        return cols, [], []
    P = pressure_array(w, cols)
    pop = mx.array([c["pop"] for c in w["cities"]], dtype=I64)
    N = followers(P, pop)
    M = majority(P, N)
    Nl, Ml = N.tolist(), M.tolist()
    fol = [{cols[k]: n for k, n in enumerate(row) if n > 0} for row in Nl]
    maj = [cols[m] if m >= 0 else -1 for m in Ml]
    return cols, fol, maj


def religion_counts(w, fol, maj, religion):
    """`religion_counts`: (cities, followers, foreign cities, foreign followers)."""
    r = w["religions"].get(religion)
    if r is None:
        return 0, 0, 0, 0
    creed = 2 + religion
    out = [0, 0, 0, 0]
    for c, f, m in zip(w["cities"], fol, maj):
        n = f.get(creed, 0)
        foreign = c["owner"] != r["founder"]
        out[1] += n
        if foreign:
            out[3] += n
        if m == creed:
            out[0] += 1
            if foreign:
                out[2] += 1
    return tuple(out)


def counted_text(text, counts):
    """`religion_effects::counted`: a count-based modifier as its flat amount (the effect's Debug text)."""
    m = _MOD.search(text)
    mod = m.group(1)
    cities, foll, fcities, ffoll = counts
    a = _PER_CITY.match(mod)
    if a:
        new = f"AddYield {{ yield_kind: {a.group(1)}, amount: {int(a.group(2)) * (fcities if a.group(3) == 'true' else cities)} }}"
    else:
        a = _PER_FOLLOWERS.match(mod)
        if a:
            n = ffoll if a.group(3) == "true" else foll
            new = f"AddYield {{ yield_kind: {a.group(1)}, amount: {n // max(int(a.group(2)), 1)} }}"
        else:
            a = _HAPPY_PER_CITY.match(mod)
            if not a:
                return text
            h = int(a.group(1)) * cities
            new = f"Module(AddHappiness {{ amount: {int(h / 100)} }})"
    return text[:m.start(1)] + new + text[m.end(1):]


def beliefs_of(w, majority_code, owner):
    if majority_code == 1:
        p = w["players"][owner]["pantheon"]
        return [p] if p >= 0 else []
    if majority_code >= 2:
        r = w["religions"].get(majority_code - 2)
        return list(r["beliefs"]) if r else []
    return []


def city_beliefs(rules, w, fol, maj, ci, policies):
    """`visit_city_beliefs` of world city `ci`: (city rows, tile rows) as [(text, 1)]."""
    out_c, out_t = [], []
    if not rules.has_passive or (w["pantheons"] == 0 and not w["religions"]):
        return out_c, out_t
    c = w["cities"][ci]
    m = maj[ci]
    bel = beliefs_of(w, m, c["owner"])
    if rules.second_pantheon and m >= 2 and rules.second_pantheon & set(policies):
        best = None
        for creed, n in sorted(fol[ci].items()):
            if creed < 2 or creed == m:
                continue
            pr = c["pressure"].get(creed, 0)
            if best is None or n > best[1] or (n == best[1] and pr > best[2]):
                best = (creed, n, pr)
        if best is not None:
            r = w["religions"].get(best[0] - 2)
            if r is not None:
                extra = next((b for b in r["beliefs"] if rules.category[b] == "Pantheon"), None)
                if extra is not None and extra not in bel:
                    bel.append(extra)
    for b in bel:
        for text, sel in rules.city[b]:
            if sel in ("Module(FollowingCities)", "Module(FollowingCityTiles)"):
                out_c.append((text, 1))
        for text, sel in rules.tile[b]:
            if sel in ("Module(FollowingCities)", "Module(FollowingCityTiles)"):
                out_t.append((text, 1))
    holy = next((r for r in w["religions"].values() if r["holy"] == c["h"]), None)
    if holy is not None:
        counts = religion_counts(w, fol, maj, holy["id"])
        for b in holy["beliefs"]:
            for text, sel in rules.city[b]:
                if sel == "Module(HolyCity)":
                    out_c.append((counted_text(text, counts), 1))
            for text, sel in rules.tile[b]:
                if sel == "Module(HolyCity)":
                    out_t.append((counted_text(text, counts), 1))
    return out_c, out_t


def founder_beliefs(rules, w, fol, maj, player):
    """`visit_founder_beliefs`: the founder rows of the player's religion, counted."""
    rid = w["players"][player]["religion"]
    r = w["religions"].get(rid) if rid >= 0 else None
    if r is None:
        return []
    counts = religion_counts(w, fol, maj, rid)
    out = []
    for b in r["beliefs"]:
        for text, sel in rules.player[b]:
            if sel == "Module(Founder)":
                out.append((counted_text(text, counts), 1))
    return out


def city_counts(fol, maj, ci):
    """MajorityFollowers, ReligionsPresent of world city `ci`."""
    m = maj[ci]
    return (fol[ci].get(m, 0) if m >= 0 else 0), sum(1 for k, n in fol[ci].items() if k >= 2 and n > 0)


def peaceful_foreign_followers(w, fol, player, war):
    rid = w["players"][player]["religion"]
    if rid < 0 or rid not in w["religions"]:
        return 0
    creed = 2 + rid
    return sum(f.get(creed, 0) for c, f in zip(w["cities"], fol) if c["owner"] != player and not war[c["owner"]])


def regrown(w, pops):
    """The world with the populations the player's cities left (`pops`: {city handle: population})."""
    out = dict(w)
    out["cities"] = [dict(c, pop=pops.get(c["h"], c["pop"])) for c in w["cities"]]
    return out


# ---- the Faith step ----

def speed_faith(rules, faith):
    p = rules.faith_percent
    return faith if p == 100 else (faith * p + 50) // 100


def prophet_number(n):
    cost = FIRST_PROPHET_FAITH
    for k in range(2, max(n, 1) + 1):
        cost += NEXT_PROPHET_FAITH * (k - 1)
    return cost


def prophet_cost(rules, w, player, techs):
    """`prophet_cost` (`techs`: the founder's bits, every player's as `all_techs`)."""
    st = w["players"][player]
    base = speed_faith(rules, prophet_number(st["prophets"] + 1))
    r = w["religions"].get(st["religion"]) if st["religion"] >= 0 else None
    pct = rules.religion_mods(r["beliefs"], techs[r["founder"]])[1] if r else 0
    return max(base * (100 + pct) // 100, 1)


def xorshift_below(state, n):
    m = (1 << 64) - 1
    x = state
    x ^= (x << 13) & m
    x ^= x >> 7
    x ^= (x << 17) & m
    x = max(x, 1)
    return x, x % max(n, 1)


def faith_step(rules, w, player, era, faith_income, techs):
    """`collect_faith` then `roll_prophet`: (faith, prophets, rng state, born) after the step."""
    st = w["players"][player]
    faith = min(st["faith"] + faith_income, 2 ** 32 - 1)
    prophets, rng, born = st["prophets"], w["rng"], False
    if era >= INDUSTRIAL_ERA or rules.prophet_unit is None:
        return faith, prophets, rng, born
    if st["religion"] < 0 and w["left"] == 0:
        return faith, prophets, rng, born
    cost = prophet_cost(rules, w, player, techs)
    if faith < cost:
        return faith, prophets, rng, born
    chance = PROPHET_BASE_CHANCE + (faith - cost)
    rng, roll = xorshift_below(rng, 100)
    if roll >= chance:
        return faith, prophets, rng, born
    if not any(c["owner"] == player for c in w["cities"]):
        return faith, prophets, rng, born
    return 0, prophets + 1, rng, True


# ---- the round's pressure ----

def touch(pressure, fpop, pop, maj_at):
    """`derived::religion::touch` on a pressure dict: (pressure, reconciled population)."""
    p = dict(pressure)
    if not p:
        p[0] = pop * PRESSURE_PER_POP
        return p, pop
    if pop > fpop:
        k = maj_at if maj_at >= 0 else 0
        p[k] = p.get(k, 0) + (pop - fpop) * PRESSURE_PER_POP
    return p, pop


def single_majority(pressure, pop):
    """The majority creed of one city (the array program on one row)."""
    if pop <= 0:
        return -1
    cols = sorted(set(pressure) | {0, 1})
    P = mx.array([[pressure.get(k, 0) for k in cols]], dtype=I64)
    N = followers(P, mx.array([pop]))
    m = majority(P, N).item()
    return cols[m] if m >= 0 else -1


def hex_distance(w_, a, b):
    """`MapDimensions::distance`: tiles are axial (q = index mod width, r = index div width)."""
    dq, dr = a % w_ - b % w_, a // w_ - b // w_
    return (abs(dq) + abs(dr) + abs(dq + dr)) // 2


def pressure_round(rules, w, routes, seats, standings, map_w, techs, distance=None):
    """`religious_pressure_round` on world `w`: the pressure and reconciled population of every city
    after it ({city handle: (pressure, fpop)}), the cities that converted to a religion (first time)."""
    if not w["religions"]:
        return {}, []
    distance = distance or (lambda a, b: hex_distance(map_w, a, b))
    cols, fol, maj = facts(w)
    links = [(r[1], r[2]) for r in routes]
    origins = {o for o, _ in links}
    friends = {(s[0], s[1]): s[10] for s in standings}
    adjacent = rules.adjacent
    sources = []
    for c, m in zip(w["cities"], maj):
        if m < 2:
            continue
        rid = m - 2
        r = w["religions"].get(rid)
        if r is None:
            continue
        pct, _, _ = rules.religion_mods(r["beliefs"], techs[r["founder"]])
        dist = max(pct[STATS.index("SpreadDistance")], 0)
        reach = ADJACENT_CITY_DISTANCE * (100 + dist) // 100
        own = sum(rules.pressure_pct.get(b, 0) for b in c["pressing"])
        percent = max(pct[STATS.index("PressureOut")] + own, -100)
        pressure = adjacent * (100 + percent) // 100
        route_pct = max(100 + seats[c["owner"]][2], 0) if c["h"] in origins else 100
        sources.append({"h": c["h"], "tile": c["tile"], "rid": rid, "founder": r["founder"], "reach": reach, "pressure": pressure,
                        "fcs": pct[STATS.index("FriendlyCityStatePressure")], "route": route_pct})
    spies = []
    for p_ in range(len(w["players"])):
        rid = w["players"][p_]["religion"]
        if rid < 0 or not seats[p_][5] or not seats[p_][7]:
            continue
        times = seats[p_][3]
        if times <= 0:
            continue
        for sp, city in w["spies"]:
            if sp == p_:
                spies.append((city, rid, adjacent * times))
    out, converted = {}, []
    for ci, c in enumerate(w["cities"]):
        gains = []
        holy = next((r for r in w["religions"].values() if r["holy"] == c["h"]), None)
        if holy is not None:
            gains.append((holy["id"], adjacent * HOLY_CITY_PRESSURE))
        for s in sources:
            if s["h"] == c["h"]:
                continue
            within = distance(s["tile"], c["tile"]) <= s["reach"]
            sends = (s["h"], c["h"]) in links
            receives = (c["h"], s["h"]) in links
            if not (within or sends or receives):
                continue
            pr = s["pressure"]
            if s["fcs"] != 0 and seats[c["owner"]][6] and friends.get((c["owner"], s["founder"]), False):
                pr = pr * (100 + max(s["fcs"], -100)) // 100
            if sends:
                pr = pr * s["route"] // 100
            gains.append((s["rid"], pr))
        for city, rid, amount in spies:
            if city == c["h"]:
                gains.append((rid, amount))
        p, fpop = dict(c["pressure"]), c["fpop"]
        changed = False
        for rid, amount in gains:
            if amount == 0:
                continue
            before = single_majority(p, c["pop"])
            p, fpop = touch(p, fpop, c["pop"], single_majority(p, max(fpop, 1)))
            k = 2 + rid
            p[k] = min(p.get(k, 0) + amount, 2 ** 32 - 1)
            if 1 in p:
                p[1] = max(p[1] - amount, 0)
            p = {k_: v for k_, v in p.items() if v > 0}
            changed = True
            after = single_majority(p, c["pop"])
            if after != before and after >= 2 and not c["paid"]:
                converted.append((c["h"], after - 2))
        if changed:
            out[c["h"]] = (p, fpop)
    return out, converted


# ---- parity ----

def run(path, limit_games=None, show=8):
    from . import city as C
    t0 = time.time()
    tot = {"lines": 0, "cities": 0, "beliefs_ok": 0, "counts_ok": 0, "founder_ok": 0, "pff_ok": 0, "majority_ok": 0, "majority": 0,
           "faith": 0, "faith_ok": 0, "prophet_cost": 0, "prophet_cost_ok": 0, "mods": 0, "mods_ok": 0,
           "rounds": 0, "pressure_cities": 0, "pressure_ok": 0, "post_cities": 0, "post_beliefs_ok": 0}
    examples = []
    rules = None
    for g, game in C.each_game(path, limit_games):
        er = game["rules"]
        rules = Rules(er)  # per game: the seats and the map size are the game's
        map_w = game["tables"]["map_w"]
        for t in game["turns"]:
            pre = t["pre"]["state"]
            if "world" not in pre:
                continue
            p = t["player"]
            tot["lines"] += 1
            for side in ("pre", "post"):
                st = t[side]["state"]
                w = world(st)
                cols, fol, maj = facts(w)
                for c, m in zip(w["cities"], maj):
                    tot["majority"] += 1
                    tot["majority_ok"] += int(m == c["majority"])
                for r in w["religions"].values():
                    tot["mods"] += 1
                    ok = rules.religion_mods(r["beliefs"], st["all_techs"][r["founder"]]) == (r["pct"], r["cost"], r["ab"])
                    tot["mods_ok"] += int(ok)
                at = {c["h"]: i for i, c in enumerate(w["cities"])}
                for c in st["cities"]:
                    ci = at.get(c["h"])
                    if ci is None:
                        continue
                    bc, bt = city_beliefs(rules, w, fol, maj, ci, st["policies"])
                    ok = sorted(map(tuple, c["inputs"]["beliefs_city"])) == sorted(bc) and sorted(map(tuple, c["inputs"]["beliefs_tile"])) == sorted(bt)
                    mf, rp = city_counts(fol, maj, ci)
                    cok = (mf, rp) == (c["inputs"]["counts"]["MajorityFollowers"], c["inputs"]["counts"]["ReligionsPresent"])
                    if side == "pre":
                        tot["cities"] += 1
                        tot["beliefs_ok"] += int(ok)
                        tot["counts_ok"] += int(cok)
                    else:
                        tot["post_cities"] += 1
                        tot["post_beliefs_ok"] += int(ok and cok)
                    if not (ok and cok) and len(examples) < show:
                        examples.append({"game": g, "round": t["round"], "player": p, "side": side, "city": c["h"],
                                         "rust": [c["inputs"]["beliefs_city"], c["inputs"]["beliefs_tile"], c["inputs"]["counts"]],
                                         "mlx": [bc, bt, [mf, rp]], "fol": fol[ci], "maj": maj[ci], "pressure": w["cities"][ci]["pressure"]})
                if side == "pre":
                    fb = founder_beliefs(rules, w, fol, maj, p)
                    fok = sorted(fb) == sorted(map(tuple, st["founder"]))
                    tot["founder_ok"] += int(fok)
                    if not fok and len(examples) < show:
                        examples.append({"game": g, "round": t["round"], "player": p, "founder": [st["founder"], fb]})
                    pff = peaceful_foreign_followers(w, fol, p, st["war"])
                    tot["pff_ok"] += int(pff == st["player_counts"].get("PeacefulForeignFollowers", 0))
                    ex = st.get("extra", {})
                    if "prophet_cost" in ex:
                        tot["prophet_cost"] += 1
                        tot["prophet_cost_ok"] += int(prophet_cost(rules, w, p, st["all_techs"]) == ex["prophet_cost"])
            # The round's pressure: the last player's line carries the round's end.
            if t["post"].get("round") != t["round"]:
                wpre = world(pre)
                wpost = world(t["post"]["state"])
                new, _ = pressure_round(rules, wpre, pre["world"]["routes"], pre["world"]["seats"], pre["world"]["cs"]["standings"], map_w, pre["all_techs"])
                tot["rounds"] += 1
                post_at = {c["h"]: c for c in wpost["cities"]}
                for c in wpre["cities"]:
                    q = post_at.get(c["h"])
                    if q is None:
                        continue
                    tot["pressure_cities"] += 1
                    mine = new.get(c["h"], (c["pressure"], c["fpop"]))
                    ok = mine[0] == q["pressure"] and mine[1] == q["fpop"]
                    tot["pressure_ok"] += int(ok)
                    if not ok and len([e for e in examples if "pressure_round" in e]) < 4:
                        examples.append({"game": g, "round": t["round"], "pressure_round": c["h"], "pre": [c["pressure"], c["fpop"], c["pop"]],
                                         "post": [q["pressure"], q["fpop"], q["pop"]], "mlx": mine})
    return {**tot, "examples": examples, "s": round(time.time() - t0, 1)}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--games", type=int)
    ap.add_argument("--show", type=int, default=8)
    o = ap.parse_args(argv)
    r = run(o.dump, o.games, o.show)
    print(f"religion: majority {r['majority_ok']}/{r['majority']} city sides; beliefs reaching the city {r['beliefs_ok']}/{r['cities']} "
          f"(after the turn {r['post_beliefs_ok']}/{r['post_cities']}), counts {r['counts_ok']}/{r['cities']}; founder's beliefs "
          f"{r['founder_ok']}/{r['lines']}, peaceful foreign followers {r['pff_ok']}/{r['lines']}; religion numbers {r['mods_ok']}/{r['mods']}; "
          f"prophet price {r['prophet_cost_ok']}/{r['prophet_cost']}; round pressure {r['pressure_ok']}/{r['pressure_cities']} cities "
          f"({r['rounds']} rounds); {r['s']} s")
    for e in r["examples"]:
        print("  ex:", json.dumps(e)[:1500])


if __name__ == "__main__":
    main()
