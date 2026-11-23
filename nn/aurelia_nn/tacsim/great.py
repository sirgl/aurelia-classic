"""The economy on the GPU (MLX): great person points and births, exact to the Rust rules
(`derived::great_people::{city_points, great_person_threshold}`, `systems::great_people::
accumulate_great_people`, the first step of a city's turn).

    python -m aurelia_nn.tacsim.great DUMP.jsonl|DIR   # parity: every city's points, every threshold

    points     per great person with a specialist: (its specialist's points × the specialists + the
               city's AddGreatPersonPoints of the kind, ≥ 0) × 100 × (100 + the city's and the player's
               GreatPeopleRate + GreatPersonRate(kind)) % — the player's with the friends' rate
               (GreatPeopleRatePerFriend of both, per friend) and the World Congress funding
    threshold  100 × 2^(born of its family, ≤ 24) × the speed's pace (generals and admirals: no pace);
               writers, artists and musicians count apart, the rest together
    the turn   a city with a specialist or a building's points adds its points to its progress; at
               the threshold (in whole points) the progress empties and the great person is born in
               the city (the nation's own unit if it has one), counted for the next threshold

The city's points come from `city.evaluate` (the specialists the citizens' placement made, the
AddGreatPersonPoints rows, the rate rows); the player's rate from the Player rows (`player.py`).
Inputs from the subsystems not in MLX: the friends' rate (declarations of friendship), the funding
resolutions.
"""

import argparse
import json

from . import city as C
from . import player as P
from . import rdebug as R

POINTS_FOR_NEXT = 100
FACTOR = 2
SEPARATE = ("Writer", "Artist", "Musician")
NO_PACE = ("General", "Admiral")
RATE_ALL = P.key("Module(GreatPeopleRate)")


class Defs:
    def __init__(self, er):
        self.defs = []
        units = er["units"]
        for i, d in enumerate(er.get("great_people", [])):
            name = R.tag(R.parse(d["kind"]))
            self.defs.append({"i": i, "kind": name, "k": d["kind_index"], "unit": d["unit"], "spec": d["specialist"],
                              "specialists": R.tag(R.parse(d["source"])) == "Specialists",
                              "rate_key": P.key(f"Module(GreatPersonRate({name}))")})
        self.spec_gpp = [s.get("gpp", 0) for s in er["specialists"]]
        self.units = units

    def unit_for(self, d, nation):
        """`rules.nation_variant`: the nation's unique unit replacing the great person's, else its own."""
        for k, u in enumerate(self.units):
            if u["nation"] == nation and u["replaces"] == d["unit"]:
                return k
        return d["unit"]


def defs(model):
    if not hasattr(model, "_gp_defs"):
        model._gp_defs = Defs(model.er)
    return model._gp_defs


def group(kind):
    return kind if kind in SEPARATE else "people"


def born_of(economy):
    """The birth counters of a player (`created`)."""
    e = economy or {}
    return {"Writer": e.get("writers", 0), "Artist": e.get("artists", 0), "Musician": e.get("musicians", 0), "people": e.get("gp_created", 0)}


def threshold(d, born, pace):
    n = born[group(d["kind"])]
    points = POINTS_FOR_NEXT
    for _ in range(min(n, 24)):
        points = min(points * FACTOR, 2 ** 32 - 1)
    if d["kind"] in NO_PACE:
        return points
    return max(points * pace // 100, 1)


def player_rate(prow4, t):
    """The player's rate as the city step reads it: its stats, the friends' rate (into
    GreatPeopleRate), the World Congress funding (into GreatPersonRate of each kind)."""
    stats, _ = P.stats_and_costs(prow4, t, "pre")
    st = t["pre"]["state"]
    gp = st.get("gp", {})
    stats = dict(stats)
    stats[RATE_ALL] = stats.get(RATE_ALL, 0) + gp.get("friends", 0)
    return stats, gp.get("funding", [])


def city_points(D, spec_n, gp_points, gp_rate, stats, funding):
    """Points per specialist slot (hundredths): `derived::great_people::city_points`."""
    out = [0] * max(len(spec_n), 1 + max([d["spec"] for d in D.defs if d["spec"] is not None] + [0]))
    seen = set()
    shift = {}
    for d in D.defs:
        if d["kind"] not in seen and d["i"] < len(funding) and funding[d["i"]]:
            seen.add(d["kind"])
            shift[d["rate_key"]] = funding[d["i"]]
    for d in D.defs:
        if not d["specialists"] or d["spec"] is None:
            continue
        s = d["spec"]
        count = spec_n[s] if s < len(spec_n) else 0
        buildings = max(gp_points[d["k"]], 0) if d["k"] >= 0 else 0
        base = (D.spec_gpp[s] * count + buildings) * 100
        if base == 0:
            continue
        rate = gp_rate[d["k"]] + stats.get(RATE_ALL, 0) + stats.get(d["rate_key"], 0) + shift.get(d["rate_key"], 0)
        out[s] = min(base * max(100 + rate, 0) // 100, 2 ** 32 - 1)
    return out


def accumulate(D, progress, points, spec_n, gp_points, born, pace, nation):
    """One city's GreatPeople step: the progress after it, the units born (and `born` counted)."""
    if not any(spec_n) and not any(x > 0 for x in gp_points):
        return progress, []
    progress = list(progress)
    units = []
    for d in D.defs:
        s = d["spec"]
        if s is None or s >= len(progress):
            continue
        thr = threshold(d, born, pace)
        progress[s] = min(progress[s] + points[s], 2 ** 32 - 1)
        if progress[s] // 100 < thr:
            continue
        progress[s] = 0
        units.append(D.unit_for(d, nation))
        born[group(d["kind"])] += 1
    return progress, units


def run(path, limit_games=None, chunk=64, show=6):
    """The points of every city against Rust's `city_points` on the turn's world (the pre side's
    yields), and every threshold."""
    tot = {"cities": 0, "points_ok": 0, "thresholds": 0, "thresholds_ok": 0}
    ex = []
    model = None
    for g, game in C.each_game(path, limit_games):
        if model is None:
            model = C.Model(game["tables"], game["rules"])
        prow4 = model.__dict__.setdefault("player_rows4", P.Rows(model.rows, model.er))
        D = defs(model)
        pace = model.speed["pace_percent"]
        turns = P.prepare(game, [t for t in game["turns"] if t["pre"]["state"]["cities"]])
        for k in range(0, len(turns), chunk):
            part = turns[k:k + chunk]
            b = C.Batch(model, game, part, side="pre")
            got = C.evaluate(model, game, b)
            sn, gpp, gpr = got["spec_n"].tolist(), got["gp_points"].tolist(), got["gp_rate"].tolist()
            rates = {}
            for ti, t in enumerate(part):
                rates[ti] = player_rate(prow4, t)
                st = t["pre"]["state"]
                if "gp" in st:
                    born = born_of(st.get("economy"))
                    for d, want in zip(D.defs, st["gp"]["threshold"]):
                        tot["thresholds"] += 1
                        tot["thresholds_ok"] += int(threshold(d, born, pace) == want)
            for ci, (ti, c) in enumerate(b.cities):
                want = c["rust"].get("gp_points")
                if want is None:
                    continue
                stats, funding = rates[ti]
                mine = city_points(D, sn[ci], gpp[ci], gpr[ci], stats, funding)
                tot["cities"] += 1
                ok = mine[:len(want)] == want[:len(mine)] and not any(want[len(mine):]) and not any(mine[len(want):])
                tot["points_ok"] += int(ok)
                if not ok and len(ex) < show:
                    ex.append({"game": g, "round": part[ti]["round"], "player": part[ti]["player"], "city": c["h"], "want": want, "mlx": mine,
                               "spec_n": sn[ci], "gp_points": gpp[ci], "gp_rate": gpr[ci]})
    return {**tot, "examples": ex}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--games", type=int)
    ap.add_argument("--show", type=int, default=6)
    o = ap.parse_args(argv)
    rep = run(o.dump, o.games, show=o.show)
    print(f"great people: city points {rep['points_ok']}/{rep['cities']} city turns as Rust's city_points; thresholds {rep['thresholds_ok']}/{rep['thresholds']}")
    for e in rep["examples"]:
        print("  ex:", json.dumps(e))


if __name__ == "__main__":
    main()
