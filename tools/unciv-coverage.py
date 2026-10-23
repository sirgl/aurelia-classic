#!/usr/bin/env python3
"""How much of the Civ V catalogs (classic/docs/classic/*.json) Unciv's rule sets contain.

Unciv (https://github.com/yairm210/Unciv, MPL-2.0) ships "Civ V - Vanilla" and
"Civ V - Gods & Kings" rule sets; there is no Brave New World rule set in the
repository, so BNW-only content cannot be covered. This script counts, per
catalog, the entries whose name exists in Unciv (names are matched after
dropping case, a leading "the " and punctuation, plus the renames in
ALIASES) and, for units, buildings, techs and difficulty levels, whether the
numbers agree.

Usage (run from the repository root; Python 3.9+, standard library only):
    python3 -I classic/tools/unciv-coverage.py --fetch DIR      download Unciv files into DIR, then count
    python3 -I classic/tools/unciv-coverage.py --dir DIR        count with files already in DIR

The files are downloaded from raw.githubusercontent.com at the pinned commit
UNCIV_COMMIT below, so the numbers are reproducible. Unciv's JSON allows
comments and trailing commas; `relaxed_json` reads that dialect.
"""
import json
import os
import re
import sys
import urllib.parse
import urllib.request

UNCIV_COMMIT = "70d0f5c80289950d1073cc9a7c5ce8c1c3f27b0b"  # master on 2026-10-20
RULESETS = {"gk": "Civ V - Gods & Kings", "va": "Civ V - Vanilla"}
FILES = ["Units", "Buildings", "Techs", "Policies", "Beliefs", "TileResources",
         "TileImprovements", "Terrains", "UnitPromotions", "Nations", "Eras",
         "Difficulties", "Religions", "Specialists", "CityStateTypes"]
# Names that differ between the catalogs and Unciv for the same thing.
ALIASES = {"railroad": "railroads", "penicillin": "pharmaceuticals", "gold": "goldore"}


def relaxed_json(text):
    """Parse JSON with // and /* */ comments and trailing commas."""
    out, i, n, in_str = [], 0, len(text), False
    while i < n:
        c = text[i]
        if in_str:
            out.append(c)
            if c == "\\":
                out.append(text[i + 1])
                i += 2
                continue
            if c == '"':
                in_str = False
            i += 1
        elif c == '"':
            in_str = True
            out.append(c)
            i += 1
        elif text.startswith("//", i):
            while i < n and text[i] != "\n":
                i += 1
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2)
            i = n if j < 0 else j + 2
        else:
            out.append(c)
            i += 1
    return json.loads(re.sub(r",(\s*[\]}])", r"\1", "".join(out)))


def key(name):
    k = re.sub(r"[^a-z0-9]", "", re.sub(r"^the ", "", name.lower()))
    return ALIASES.get(k, k)


def fetch(dest):
    for rs, folder in RULESETS.items():
        os.makedirs(os.path.join(dest, rs), exist_ok=True)
        for f in FILES:
            path = f"android/assets/jsons/{folder}/{f}.json"
            url = ("https://raw.githubusercontent.com/yairm210/Unciv/" + UNCIV_COMMIT + "/"
                   + urllib.parse.quote(path))
            try:
                data = urllib.request.urlopen(url, timeout=30).read()
            except Exception:  # file absent in this rule set (Beliefs, Religions in Vanilla)
                continue
            with open(os.path.join(dest, rs, f + ".json"), "wb") as fh:
                fh.write(data)


def load(d, rs, f):
    p = os.path.join(d, rs, f + ".json")
    if not os.path.exists(p):
        return []
    with open(p, encoding="utf-8") as fh:
        return relaxed_json(fh.read())


def catalog(name):
    with open(f"classic/docs/classic/{name}.json", encoding="utf-8") as fh:
        return json.load(fh)


class Pool:
    """Unciv entries of one table in both rule sets, by normalised name."""

    def __init__(self, d, table, flatten=None):
        self.by = {}
        for rs in RULESETS:
            rows = load(d, rs, table)
            if flatten:
                rows = flatten(rows)
            self.by[rs] = {key(r["name"]): r for r in rows if "name" in r}

    def find(self, name, rs=None):
        k = key(name)
        for r in ([rs] if rs else list(RULESETS)):
            if k in self.by[r]:
                return self.by[r][k]
        return None


def say(label, items, pool, namefn=lambda it: it["name"]):
    gk = sum(1 for it in items if key(namefn(it)) in pool.by["gk"])
    va = sum(1 for it in items if key(namefn(it)) in pool.by["va"])
    miss = [namefn(it) for it in items if pool.find(namefn(it)) is None]
    print(f"{label}: catalog {len(items)}; in Unciv G&K {gk}; in Unciv Vanilla {va}; "
          f"in either {len(items) - len(miss)}; in neither {len(miss)}")
    if miss:
        print("    in neither: " + ", ".join(miss))


def flat_techs(cols):
    return [t for c in cols for t in c["techs"]]


def flat_policies(branches):
    out = []
    for b in branches:
        out.append(b)
        out += b.get("policies", [])
    return out


def main(argv):
    if len(argv) != 3 or argv[1] not in ("--fetch", "--dir"):
        print(__doc__)
        return 2
    d = argv[2]
    if argv[1] == "--fetch":
        fetch(d)

    units = Pool(d, "Units")
    blds = Pool(d, "Buildings")
    techs = Pool(d, "Techs", flat_techs)
    pols = Pool(d, "Policies", flat_policies)
    bels = Pool(d, "Beliefs")
    ress = Pool(d, "TileResources")
    imps = Pool(d, "TileImprovements")
    terr = Pool(d, "Terrains")
    prom = Pool(d, "UnitPromotions")
    nats = Pool(d, "Nations")
    eras = Pool(d, "Eras")

    print(f"Unciv commit {UNCIV_COMMIT}")
    print("== names present in Unciv ==")
    say("units (default unit of each class, great people, ship parts)", catalog("units")["items"], units)
    say("unique units", catalog("unique_units")["items"], units)
    say("buildings", catalog("buildings")["items"], blds)
    say("unique buildings", catalog("unique_buildings")["items"], blds)
    wonders = catalog("wonders")["items"]
    for kind in ("world_wonder", "national_wonder", "project", "league_project"):
        say("wonders/" + kind, [w for w in wonders if w["kind"] == kind], blds)
    say("technologies", catalog("techs")["items"], techs)
    pc = catalog("policies")
    say("policy branches (incl. ideologies)", pc["branches"] if isinstance(pc["branches"], list) else [],
        pols)
    say("policies (role policy)", [p for p in pc["items"] if p["role"] == "policy"], pols)
    say("policy openers/finishers/tenets", [p for p in pc["items"] if p["role"] != "policy"], pols)
    say("beliefs", catalog("beliefs")["items"], bels)
    say("resources", catalog("resources")["items"], ress)
    say("improvements", catalog("improvements")["items"], imps)
    say("terrain", catalog("terrain")["items"], terr)
    say("features", catalog("features")["items"], terr)
    say("natural wonders", catalog("natural_wonders")["items"], terr)
    say("promotions", catalog("promotions")["items"], prom)
    say("civilizations", catalog("civilizations")["items"], nats)
    say("city-states", catalog("city_states")["items"], nats)
    say("eras", catalog("eras")["items"], eras)

    print("== numbers that agree, for entries present in Unciv G&K ==")
    # Units: strength, ranged strength, movement, cost.
    for label, cat in (("units", "units"), ("unique units", "unique_units")):
        tot = same = 0
        diff = {}
        for it in catalog(cat)["items"]:
            u = units.find(it["name"], "gk")
            if u is None:
                continue
            tot += 1
            ok = True
            for ours, theirs in (("combat", "strength"), ("ranged_combat", "rangedStrength"),
                                 ("moves", "movement"), ("cost", "cost")):
                a, b = it.get(ours) or 0, u.get(theirs, 0) or 0
                if ours == "cost" and a == -1:
                    continue
                if ours in ("combat", "ranged_combat", "moves") and it["domain"] == "core:domain/air":
                    continue  # air units are modelled differently (see SOURCES.md)
                if a != b:
                    ok = False
                    diff[ours] = diff.get(ours, 0) + 1
            same += ok
        print(f"{label}: compared {tot}; strength, ranged strength, movement and cost all equal in "
              f"{same}; differing fields {diff}")
    # Buildings: cost and maintenance. Unciv gives most buildings no cost of their own: the cost
    # is the building (or wonder) cost of the column of the required technology in Techs.json.
    columns = {}
    for col in load(d, "gk", "Techs"):
        for t in col["techs"]:
            columns[key(t["name"])] = col
    for label, cat in (("buildings", "buildings"), ("unique buildings", "unique_buildings"),
                       ("wonders", "wonders")):
        tot = same = 0
        for it in catalog(cat)["items"]:
            b = blds.find(it["name"], "gk")
            if b is None or it.get("cost") in (None, -1):
                continue
            cost = b.get("cost")
            col = columns.get(key(b.get("requiredTech", "")))
            if cost is None and col is not None:
                cost = col["wonderCost"] if b.get("isWonder") or b.get("isNationalWonder") else col["buildingCost"]
            if cost is None:
                continue
            tot += 1
            same += it["cost"] == cost and (it.get("maintenance") or 0) == (b.get("maintenance") or 0)
        print(f"{label}: compared {tot} with a cost in Unciv (own or by tech column); cost and "
              f"maintenance equal in {same}")
    # Techs: prerequisites.
    tot = same = 0
    for it in catalog("techs")["items"]:
        t = techs.find(it["name"], "gk")
        if t is None:
            continue
        tot += 1
        ours = sorted(key(p.split("/")[-1].replace("-", " ")) for p in it["prerequisites"])
        theirs = sorted(key(p) for p in t.get("prerequisites", []))
        same += ours == theirs
    print(f"techs: compared {tot}; prerequisite sets equal in {same}")
    # Difficulty levels: 15 fields per level, see SOURCES.md for the mapping.
    ron_civs = open("classic/assets/rules/classic/civilizations.ron", encoding="utf-8").read()
    ron = open("classic/assets/rules/classic/handicaps.ron", encoding="utf-8").read()
    rows = {}
    for name, body in re.findall(r'id: "core:handicap/(\w+)",(.*?)\n        \),', ron, re.S):
        rows[name] = {k: int(v) for k, v in re.findall(r"(\w+): (-?\d+),", body)}
    mapping = {"happiness_default": ("baseHappiness", 1),
               "extra_happiness_per_luxury": ("extraHappinessPerLuxury", 1),
               "num_cities_unhappiness_mod": ("unhappinessModifier", 100),
               "population_unhappiness_mod": ("unhappinessModifier", 100),
               "production_free_units": ("unitSupplyBase", 1),
               "production_free_units_per_city": ("unitSupplyPerCity", 1),
               "unit_cost_percent": ("unitCostModifier", 100),
               "building_cost_percent": ("buildingCostModifier", 100),
               "research_percent": ("researchCostModifier", 100),
               "policy_percent": ("policyCostModifier", 100),
               "ai_growth_percent": ("aiCityGrowthModifier", 100),
               "ai_train_percent": ("aiUnitCostModifier", 100),
               "ai_construct_percent": ("aiBuildingCostModifier", 100),
               "ai_world_construct_percent": ("aiWonderCostModifier", 100),
               "ai_unit_supply_percent": ("aiUnitSupplyModifier", 100)}
    tot = same = 0
    diffs = []
    for lvl in load(d, "gk", "Difficulties"):
        row = rows.get(lvl["name"].lower())
        if not row:
            continue
        for ours, (theirs, scale) in mapping.items():
            tot += 1
            if row[ours] == round(lvl.get(theirs, 0) * scale):
                same += 1
            else:
                diffs.append(f"{lvl['name']}.{ours} {row[ours]} vs {round(lvl.get(theirs, 0) * scale)}")
    print(f"difficulty levels: {tot // len(mapping)} levels x {len(mapping)} fields = {tot} comparisons; "
          f"equal {same}; differing: {', '.join(diffs)}")
    # City and spy name pools of civilizations (classic/assets/rules/classic/civilizations.ron).
    for field, theirs_key in (("city_names", "cities"), ("spy_names", "spyNames")):
        civs = covered = names = names_in = 0
        for block in re.split(r"\n        \(\n", ron_civs)[1:]:
            m = re.search(r'name: "([^"]+)"', block)
            pool = re.search(field + r": \[(.*?)\]", block, re.S)
            if not m or not pool:
                continue
            ours = re.findall(r'"([^"]*)"', pool.group(1))
            civs += 1
            u = nats.find(m.group(1), "gk")
            if u is None or theirs_key not in u:
                continue
            covered += 1
            names += len(ours)
            names_in += sum(1 for n in ours if n in u[theirs_key])
        print(f"{field}: {civs} civilizations with a pool; Unciv G&K has a list for {covered}; "
              f"{names_in} of their {names} names are in Unciv's list")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
