"""The economy on the GPU (MLX), fourth subsystem: an empire's happiness and its golden age, exact to
the Rust rules (`derived::happiness::player_happiness`, `systems::golden_age`).

    python -m aurelia_nn.tacsim.happy DUMP.jsonl     # parity: the report of every end of turn, the golden age after it

The cities' part comes from `city.evaluate` (their local happiness, their unhappiness stats, the
specialists the citizens' placement makes); the player's part from the Player-scope rows of the same
effect table (nation, technologies, policies, owner-wide buildings; the founder's beliefs and timed
effects as the dump lists them): its happiness, its stats (city-count and population unhappiness
percents, happiness per city, golden age length).

    happiness   = handicap + luxuries + Σ min(city happiness, pop)⁺ + buildings + cities × per city
                + natural wonders + league
    unhappiness = ⌊(Σ count + Σ population + Σ occupied) × AI % / 100 / 100⌋ + public opinion
    golden age  : the meter fills with the excess (never below 0); at the threshold
                  (500 + 250·ages, +1 % per city, × pace) a golden age of 10 × (100 + length %) × speed % starts

Inputs from the subsystems not yet in MLX (the report's own lines): luxuries, natural wonders, the
World Congress, public opinion.
"""

import argparse
import json
import time

import mlx.core as mx

from . import city as C
from . import rdebug as R

UNHAPPINESS_FROM_CITY = 300
UNHAPPINESS_FROM_ANNEXED_CITY = 500
OCCUPIED_CITY_UNHAPPINESS = 200
UNHAPPINESS_FROM_CITIZENS = 100
UNHAPPINESS_FROM_ANNEXED_CITIZENS = 200
GA_BASE, GA_STEP, GA_CITY_PERCENT, GA_TURNS = 500, 250, 1, 10


class PlayerRows:
    """The Player-scope rows of the effect table (parsed by the city table's parser)."""

    def __init__(self, rows, er):
        self.rows = rows
        self.er = er
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
            self.static.append(row)
        self.cache = {}

    def parse_extra(self, text):
        if text not in self.cache:
            row = self.rows.parse(text, "player")
            self.cache[text] = row
        return self.cache[text]


def holds(c, st, golden):
    kind, a, _ = c
    known = st["techs"]
    if kind == C.C_ALWAYS:
        return True
    if kind == C.C_TECH:
        return known[a] == "1"
    if kind == C.C_NOTECH:
        return known[a] != "1"
    if kind == C.C_PEACE:
        return st["_peace"]
    if kind == C.C_GOLDEN:
        return golden
    return False  # a city or tile condition never holds at the player's level


def player_terms(prow, t, er):
    """The player's modifiers that the happiness and the golden age read: (happiness, stats by name)."""
    st = t["pre"]["state"]
    p = t["player"]
    players = er["players"]
    st["_peace"] = not any(st["war"][o] and players[o]["major"] and st["alive"][o] and o != p for o in range(len(players)))
    golden = st["golden_age"]
    counts_b = {}
    for c in st["cities"]:
        for b in c["buildings"]:
            counts_b[b] = counts_b.get(b, 0) + 1
    pk = st.get("player_counts", {})
    counts = {6: len(st["policies"]), 7: pk.get("PeacefulForeignFollowers", 0), 8: pk.get("ConnectedCities", 0),
              4: pk.get("TradePartners", 0), 5: pk.get("CityStateRoutes", 0)}
    nation = players[p]["nation"]
    pol = set(st["policies"])
    rows = []
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
        if n:
            rows.append((row, n))
    for text, n in st["founder"] + st["timed"]:
        row = prow.parse_extra(text)
        if row is not None:
            rows.append((row, n))
    happiness = 0
    stats = {}
    for row, n in rows:
        if not all(holds(c, st, golden) for c in row["c"]):
            continue
        m = row["m"]
        kind = m[0]
        if kind == C.M_HAPPY:
            happiness += m[2] * n
        elif kind == C.M_HAPPY_COUNT:
            cnt = counts.get(m[3], 0)
            v = m[2] * (cnt // max(m[4], 1))
            if m[5] >= 0 and abs(v) > m[5]:
                v = m[5] if v > 0 else -m[5]
            happiness += v * n
        elif kind == C.M_STAT and m[1] >= 100:
            name = C.PLAYER_STATS[m[1] - 100]
            stats[name] = stats.get(name, 0) + m[2] * n
        elif kind == C.M_STAT and m[1] == C.S_POPUNHAPPY:
            stats["PopulationUnhappinessPercent"] = stats.get("PopulationUnhappinessPercent", 0) + m[2] * n
        elif kind in (C.M_FLAT, C.M_PCT) and m[1] == 3:
            raise C.Unsupported("a player science modifier")
    return happiness, stats


def report_fields(text):
    return R.body(R.parse(text))


def happiness(model, prow, game, turns):
    """Per turn line: the MLX report and the golden age after the turn."""
    er = game["rules"]  # the seats and the map size are the game's (the content is the model's)
    b = C.Batch(model, game, turns, side="pre")
    got = C.evaluate(model, game, b) if b.n else None
    vals = {k: got[k].tolist() for k in ("happiness", "pop_unhappy_pct", "spec_unhappy_pct", "no_occupied", "spec_n")} if got else {}
    by_turn = {}
    for ci, (ti, c) in enumerate(b.cities):
        by_turn.setdefault(ti, []).append((ci, c))
    out = []
    for ti, t in enumerate(turns):
        st = t["pre"]["state"]
        p = t["player"]
        players = er["players"]
        hc = R.body(R.parse(st.get("handicap", players[p]["handicap"])))
        mod = R.body(hc["module"])
        rep = report_fields(st["rust"]["happiness"])
        buildings, stats = player_terms(prow, t, er)
        local = 0
        share = {"count": 0, "population": 0, "occupied": 0}
        pc = {"city_count": max(100 + stats.get("CityCountUnhappinessPercent", 0), 0), "handicap_cities": mod["num_cities_unhappiness_mod"],
              "world": er["world"].get("city_unhappiness_percent", 100), "population": max(100 + stats.get("PopulationUnhappinessPercent", 0), 0),
              "handicap_population": mod["population_unhappiness_mod"]}

        def count(base):
            return base * pc["city_count"] // 100 * pc["handicap_cities"] // 100 * pc["world"] // 100

        for ci, c in by_turn.get(ti, []):
            local += max(min(vals["happiness"][ci], c["pop"]), 0)
            pop = c["pop"]
            pct = vals["spec_unhappy_pct"][ci]
            if pct < 0:
                specialists = sum(vals["spec_n"][ci])
                pop = max(pop - (specialists * -pct + 99) // 100, 0)
            occupied = c["status"] == "Razing" or (c["status"] == "Occupied" and vals["no_occupied"][ci] <= 0)
            if occupied:
                share["occupied"] += count(UNHAPPINESS_FROM_ANNEXED_CITY) + OCCUPIED_CITY_UNHAPPINESS + pop * UNHAPPINESS_FROM_ANNEXED_CITIZENS * pc["handicap_population"] // 100
            else:
                own = max(100 + vals["pop_unhappy_pct"][ci], 0)
                people = pop * UNHAPPINESS_FROM_CITIZENS * own // 100
                share["count"] += count(UNHAPPINESS_FROM_CITY)
                share["population"] += people * pc["population"] // 100 * pc["handicap_population"] // 100
        ai = mod["ai_unhappiness_percent"] if st.get("ai", players[p]["ai"]) else 100

        def whole(v):
            return v * ai // 100 // 100

        mine = {"handicap": mod["happiness_default"], "luxuries": rep["luxuries"], "local": local, "buildings": buildings,
                "per_city": len(st["cities"]) * stats.get("HappinessPerCity", 0), "natural_wonders": rep["natural_wonders"], "league": rep["league"],
                "from_cities": whole(share["count"]), "from_population": whole(share["population"]), "from_occupied": whole(share["occupied"]),
                "from_opinion": rep["from_opinion"]}
        mine["unhappiness_total"] = whole(share["count"] + share["population"] + share["occupied"]) + rep["from_opinion"]
        excess = sum(mine[k] for k in ("handicap", "luxuries", "local", "buildings", "per_city", "natural_wonders", "league")) - mine["unhappiness_total"]
        # The golden age step (after the happiness snapshot).
        e = dict(st["economy"]) if st.get("economy") else None
        ga = None
        if e is not None:
            ages = e["ga_count"]
            thr = GA_BASE + ages * GA_STEP
            thr += thr * len(st["cities"]) * GA_CITY_PERCENT // 100
            speed = R.body(R.parse(er["speed_rules"]))
            thr = thr * speed["pace_percent"] // 100
            lp = stats.get("GoldenAgeLengthPercent", 0)
            turns_ = GA_TURNS if lp <= 0 else GA_TURNS * (100 + lp) // 100
            if not (lp <= 0 and speed["golden_age_percent"] == 100):
                turns_ = min(max(turns_ * speed["golden_age_percent"] // 100, 1), 65535)
            if e["ga_turns"] == 0:
                meter = min(max(e["ga_meter"] + excess, 0), 2 ** 32 - 1)
                if meter >= thr:
                    meter -= thr
                    e["ga_count"] += 1
                    e["ga_turns"] += turns_
                e["ga_meter"] = meter
            else:
                e["ga_turns"] -= 1
            ga = {"turns": e["ga_turns"], "meter": e["ga_meter"], "count": e["ga_count"], "threshold": thr, "length": turns_}
        out.append({"report": mine, "excess": excess, "ga": ga})
    return out


def run(path, limit_games=None, chunk=64, show=6):
    t0 = time.time()
    ctx = C.each_game(path, limit_games)
    t_load = 0.0
    n_games = 0
    model = prow = None
    tot = {"turns": 0, "report_ok": 0, "excess_ok": 0, "ga_ok": 0, "ga_turns": 0}
    lines = {}
    examples = []
    t_run = 0.0
    for g, game in ctx:
        n_games += 1
        if model is None:
            model = C.Model(game["tables"], game["rules"])
            prow = PlayerRows(model.rows, model.er)
        turns = [t for t in game["turns"] if "happiness" in t["pre"]["state"].get("rust", {})]
        for k in range(0, len(turns), chunk):
            t1 = time.time()
            res = happiness(model, prow, game, turns[k:k + chunk])
            t_run += time.time() - t1
            for t, r in zip(turns[k:k + chunk], res):
                st, post = t["pre"]["state"], t["post"]["state"]
                rep = report_fields(st["rust"]["happiness"])
                tot["turns"] += 1
                ok_lines = {key: rep[key] == v for key, v in r["report"].items()}
                for key, v in ok_lines.items():
                    lines[key] = lines.get(key, 0) + int(v)
                rok = all(ok_lines.values())
                tot["report_ok"] += int(rok)
                eok = r["excess"] == post["excess"]
                tot["excess_ok"] += int(eok)
                gok = True
                if r["ga"] is not None and post.get("economy"):
                    pe = post["economy"]
                    tot["ga_turns"] += 1
                    gok = (pe["ga_turns"], pe["ga_meter"], pe["ga_count"]) == (r["ga"]["turns"], r["ga"]["meter"], r["ga"]["count"])
                    gok &= r["ga"]["threshold"] == st["rust"]["ga_threshold"] and r["ga"]["length"] == st["rust"]["ga_length"]
                    tot["ga_ok"] += int(gok)
                if not (rok and eok and gok) and len(examples) < show:
                    examples.append({"game": g, "round": t["round"], "player": t["player"],
                                     "bad": [key for key, v in ok_lines.items() if not v] + ([] if eok else ["excess"]) + ([] if gok else ["ga"]),
                                     "rust": rep, "mlx": r["report"], "excess": [post["excess"], r["excess"], st["rust"]["excess_now"]],
                                     "ga": [post.get("economy"), r["ga"], st.get("economy"), st["rust"]["ga_threshold"], st["rust"]["ga_length"]]})
    return {"games": n_games, **tot, "lines": lines, "examples": examples, "load_s": round(t_load, 2), "run_s": round(t_run, 2)}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--games", type=int)
    ap.add_argument("--show", type=int, default=6)
    o = ap.parse_args(argv)
    rep = run(o.dump, o.games, show=o.show)
    print(f"happiness: report {rep['report_ok']}/{rep['turns']} end-turns equal on every line, excess (the turn's snapshot) {rep['excess_ok']}/{rep['turns']}, "
          f"golden age after the turn {rep['ga_ok']}/{rep['ga_turns']}; lines {rep['lines']}; load {rep['load_s']} s, run {rep['run_s']} s")
    for e in rep["examples"]:
        print("  ex:", json.dumps(e))


if __name__ == "__main__":
    main()
