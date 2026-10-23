#!/usr/bin/env python3
"""Re-sources the classic rule catalogs (classic/docs/classic/*.json) from Unciv.

Derived from Unciv (https://github.com/yairm210/Unciv), MPL-2.0: the rule files
`android/assets/jsons/Civ V - Gods & Kings/*.json` (Vanilla for what G&K lacks).
Unciv has no Brave New World rule set; entries without an Unciv counterpart keep
their present values and are marked "own value, unverified".

What the script does, per catalog (classic/docs/classic/RESOURCING.md has the method):

  1. matches every entry to an Unciv entry by name (case, a leading "the", and
     punctuation ignored; ALIASES for the renames);
  2. overwrites the scalar rule fields that Unciv has (cost, strength, movement,
     prerequisites, technology, yields, ...); a field Unciv does not have keeps the
     present value;
  3. writes `provenance` on every entry: "unciv: <File>.json, <name>" or
     "own value, unverified" (and `unciv_fields`: the fields taken from Unciv);
  4. drops what must not stay: the verbatim `text` descriptions, the original's
     `game_type` keys, `defines`/`verify*` notes, `edition`; effect `source` labels
     become "unciv: ..." (effect confirmed against Unciv) or "own value, unverified";
  5. compares the typed effects with the claims of the Unciv entry's unique texts (UNIQUE_RULES): equal ->
     `source: unciv: ...`, the same rule with another number -> Unciv's number (listed by --report);
  6. fills the nation name pools of classic/assets/rules/classic/civilizations.ron from
     Unciv's `cities` and `spyNames` where the nation exists there.

Usage (repository root; Python 3.9+, standard library only):
    git clone --depth 1 https://github.com/yairm210/Unciv.git DIR   # read-only checkout
    python3 -I classic/tools/import-unciv.py --unciv DIR            rewrite classic/docs/classic/*.json
    python3 -I classic/tools/import-unciv.py --unciv DIR --report   print the summary only
The result is committed; running it again gives the same bytes (it is an overlay
and every step is idempotent). The Unciv commit is recorded in each file's `notice`.
"""
import json
import os
import re
import subprocess
import sys

RULESETS = {"gk": "Civ V - Gods & Kings", "va": "Civ V - Vanilla"}
ALIASES = {"railroad": "railroads", "penicillin": "pharmaceuticals", "gold": "goldore"}
EDITION = "Civ 5 inspired rule set (Unciv Gods & Kings ruleset plus own extensions)"
NOTICE = ("Derived from Unciv (https://github.com/yairm210/Unciv), MPL-2.0: rule fields of the entries "
          "marked `unciv:` come from Unciv's `Civ V - Gods & Kings` JSON files at commit {commit}; "
          "entries marked `own value, unverified` have no Unciv counterpart. Listed per file in "
          "classic/docs/classic/UNCIV-ATTRIBUTION.md.")
OWN = "own value, unverified"
OVERRIDES = "classic/tools/own-overrides.json"
OWN_CHECKED = "own value"  # an own number that was checked by hand: no external source is named

def relaxed_json(text):
    """Parse JSON with // and /* */ comments and trailing commas (Unciv's dialect)."""
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


class Unciv:
    def __init__(self, root):
        self.root = root
        self.base = os.path.join(root, "android", "assets", "jsons")
        self.commit = subprocess.run(["git", "-C", root, "rev-parse", "HEAD"], capture_output=True,
                                     text=True).stdout.strip()[:8] or "unknown"
        self.cache = {}

    def load(self, rs, name):
        k = (rs, name)
        if k not in self.cache:
            p = os.path.join(self.base, RULESETS[rs], name + ".json")
            self.cache[k] = relaxed_json(open(p, encoding="utf-8").read()) if os.path.exists(p) else []
        return self.cache[k]

    def pool(self, name, flatten=None):
        """{normalised name: (entry, ruleset)}; G&K wins over Vanilla."""
        out = {}
        for rs in ("va", "gk"):
            rows = self.load(rs, name)
            if flatten:
                rows = flatten(rows)
            for r in rows:
                if isinstance(r, str):
                    r = {"name": r}
                if "name" in r:
                    out[key(r["name"])] = (r, rs)
        return out


def flat_techs(cols):
    out = []
    for c in cols:
        for t in c["techs"]:
            t = dict(t)
            t["_col"] = c
            out.append(t)
    return out


def flat_policies(branches):
    out = []
    for b in branches:
        out.append(dict(b, _branch=True))
        for p in b.get("policies", []):
            out.append(dict(p, _of=b["name"]))
    return out


def load_cat(name):
    with open(f"classic/docs/classic/{name}.json", encoding="utf-8") as fh:
        return json.load(fh)


def save_cat(name, d):
    with open(f"classic/docs/classic/{name}.json", "w", encoding="utf-8") as fh:
        fh.write(json.dumps(d, indent=1, ensure_ascii=False) + "\n")


STATS = {"changed": {}, "taken": {}, "own": {}, "own_names": {}, "claims_missing": [],
         "civ_verified": {}, "civ_changed": [], "civ_effects": 0, "civ_skipped": [], "civ_unknown": [],
         "uq_confirmed": 0, "uq_changed": [], "uq_log": [], "uq_promotions": []}


def mark(cat, it, u, ufile, fields):
    """Provenance of one entry."""
    if u is None:
        it["provenance"] = OWN
        it.pop("unciv_fields", None)
        STATS["own"][cat] = STATS["own"].get(cat, 0) + 1
        STATS["own_names"].setdefault(cat, []).append(it["name"])
    else:
        it["provenance"] = f"unciv: {ufile}.json, {u['name']}"
        it["unciv_fields"] = sorted(fields)
        STATS["taken"][cat] = STATS["taken"].get(cat, 0) + 1


def setf(cat, it, field, value, fields):
    """Set a field from Unciv; count a change of value."""
    if field in it and it[field] != value:
        STATS["changed"].setdefault(cat, []).append(f"{it['name']}.{field}: {it[field]!r} -> {value!r}")
    it[field] = value
    fields.append(field)


class Ids:
    """name -> catalog id, for the references Unciv writes by name."""

    def __init__(self):
        self.maps = {}
        for cat, kinds in {"tech": ["techs"], "unit": ["units", "unique_units"],
                           "building": ["buildings", "unique_buildings", "wonders"],
                           "resource": ["resources"], "improvement": ["improvements"],
                           "terrain": ["terrain", "features", "natural_wonders"],
                           "promotion": ["promotions"], "nation": ["civilizations"],
                           "era": ["eras"]}.items():
            m = {}
            for k in kinds:
                for it in load_cat(k)["items"]:
                    m[key(it["name"])] = it["id"]
                    if cat == "era":
                        m[key(it["name"].replace(" Era", " era"))] = it["id"]
            self.maps[cat] = m

    def get(self, cat, name):
        if name is None:
            return None
        return self.maps[cat].get(key(name))


# ----------------------------------------------------------------------------- units
def starting_techs(uc):
    """Technologies Unciv gives at the start: a unit that needs one is available from turn one."""
    out = set()
    for t in uc.pool("Techs", flat_techs).values():
        if "Starting tech" in t[0].get("uniques", []):
            out.add(key(t[0]["name"]))
    return out


def air_types(uc):
    """Unciv unit types that move through the air (our schema keeps their strength elsewhere)."""
    return {t["name"] for t in uc.load("gk", "UnitTypes") if t.get("movementType") == "Air"}


def units(uc, ids, cat):
    pool = uc.pool("Units")
    start = starting_techs(uc)
    air = air_types(uc)
    d = load_cat(cat)
    for it in d["items"]:
        hit = pool.get(key(it["name"]))
        u = hit[0] if hit else None
        fields = []
        if u is not None:
            is_air = u.get("unitType") in air
            if not is_air:
                setf(cat, it, "combat", u.get("strength", 0), fields)
                setf(cat, it, "ranged_combat", u.get("rangedStrength", 0), fields)
                if "movement" in u:
                    setf(cat, it, "moves", u["movement"], fields)
            if "cost" in u:
                setf(cat, it, "cost", u["cost"], fields)
            rt = ids.get("tech", u.get("requiredTech"))
            if u.get("requiredTech") is None:
                if it.get("required_tech") is None:
                    fields.append("required_tech")
            elif key(u["requiredTech"]) in start:
                pass  # a technology Unciv grants at the start: the unit is still gated by it here
            elif rt:
                setf(cat, it, "required_tech", rt, fields)
            ot = ids.get("tech", u.get("obsoleteTech"))
            if u.get("obsoleteTech") is None or ot:
                setf(cat, it, "obsolete_tech", ot, fields)
            up = ids.get("unit", u.get("upgradesTo"))
            if up:
                setf(cat, it, "upgrades_to", [up], fields)
            rr = ids.get("resource", u.get("requiredResource"))
            if is_air:
                pass
            elif u.get("requiredResource") is None:
                setf(cat, it, "required_resources", [], fields)
            elif rr:
                amount = 1
                for old in it.get("required_resources") or []:
                    if old["resource"] == rr:
                        amount = old["amount"]
                setf(cat, it, "required_resources", [{"resource": rr, "amount": amount}], fields)
        mark(cat, it, u, "Units" if hit and hit[1] == "gk" else "Units (Vanilla)", fields)
        if hit and hit[1] == "va":
            it["provenance"] = it["provenance"].replace("Units (Vanilla).json", "Vanilla Units.json")
    return d


# ----------------------------------------------------------------------------- buildings
def building_cost(b, techs, tech_cols, is_wonder):
    """Unciv: an absent cost comes from the column of the required technology."""
    if "cost" in b and b["cost"] != -1:
        return b["cost"]
    t = techs.get(key(b.get("requiredTech", "")))
    if not t:
        return None
    col = t[0]["_col"]
    return col.get("wonderCost" if is_wonder else "buildingCost")


def buildings(uc, ids, cat):
    pool = uc.pool("Buildings")
    techs = uc.pool("Techs", flat_techs)
    d = load_cat(cat)
    for it in d["items"]:
        hit = pool.get(key(it["name"]))
        u = hit[0] if hit else None
        fields = []
        if u is not None:
            wonder = it.get("kind") in ("world_wonder", "national_wonder")
            project = it.get("kind") in ("project", "league_project")
            if not project:
                cost = building_cost(u, techs, None, wonder or bool(u.get("isWonder") or u.get("isNationalWonder")))
                if cost is not None and cost > 0:
                    setf(cat, it, "cost", cost, fields)
                if "maintenance" in u or not (wonder or project):
                    setf(cat, it, "maintenance", u.get("maintenance", 0), fields)
                rt = ids.get("tech", u.get("requiredTech"))
                if u.get("requiredTech") is None:
                    if it.get("required_tech") is None:
                        fields.append("required_tech")
                elif key(u["requiredTech"]) in starting_techs(uc):
                    pass
                elif rt:
                    setf(cat, it, "required_tech", rt, fields)
                if "hurryCostModifier" in u:
                    setf(cat, it, "hurry_cost_modifier", u["hurryCostModifier"], fields)
        mark(cat, it, u, "Buildings" if hit and hit[1] == "gk" else "Vanilla Buildings", fields)
    return d


# ----------------------------------------------------------------------------- techs
def techs(uc, ids, cat):
    pool = uc.pool("Techs", flat_techs)
    d = load_cat(cat)
    for it in d["items"]:
        hit = pool.get(key(it["name"]))
        u = hit[0] if hit else None
        fields = []
        if u is not None:
            setf(cat, it, "cost", u.get("cost", u["_col"]["techCost"]), fields)
            pre = [ids.get("tech", p) for p in u.get("prerequisites", [])]
            if all(pre):
                # a prerequisite that is a technology Unciv does not have (The Internet) stays
                unciv_ids = {ids.get("tech", t[0]["name"]) for t in pool.values()}
                pre += [p for p in it["prerequisites"] if p not in unciv_ids]
                # keep the present order when it is the same set
                setf(cat, it, "prerequisites",
                     it["prerequisites"] if sorted(it["prerequisites"]) == sorted(pre) else pre, fields)
            era = ids.get("era", u["_col"]["era"])
            if era:
                setf(cat, it, "era", era, fields)
        mark(cat, it, u, "Techs", fields)
    return d


# ----------------------------------------------------------------------------- eras
def eras(uc, ids, cat):
    pool = uc.pool("Eras")
    d = load_cat(cat)
    for it in d["items"]:
        hit = pool.get(key(it["name"].replace(" Era", " era")))
        u = hit[0] if hit else None
        fields = []
        if u is not None:
            if "researchAgreementCost" in u:
                setf(cat, it, "research_agreement_cost", u["researchAgreementCost"], fields)
            if "embarkDefense" in u:
                setf(cat, it, "embarked_unit_defense", u["embarkDefense"], fields)
        mark(cat, it, u, "Eras", fields)
    return d


# ----------------------------------------------------------------------------- resources
def resources(uc, ids, cat):
    pool = uc.pool("TileResources")
    d = load_cat(cat)
    classes = {"Bonus": "bonus", "Luxury": "luxury", "Strategic": "strategic"}
    for it in d["items"]:
        hit = pool.get(key(it["name"]))
        u = hit[0] if hit else None
        fields = []
        if u is not None:
            if u.get("resourceType") in classes:
                setf(cat, it, "class", classes[u["resourceType"]], fields)
            y = {k2: u[k1] for k1, k2 in (("food", "food"), ("production", "production"), ("gold", "gold"),
                                           ("science", "science"), ("culture", "culture"), ("faith", "faith"))
                 if u.get(k1)}
            setf(cat, it, "yields", y, fields)
            imp = ids.get("improvement", u.get("improvement"))
            if imp:
                setf(cat, it, "improvement", [imp], fields)
            rv = ids.get("tech", u.get("revealedBy"))
            if u.get("revealedBy") is None or rv:
                setf(cat, it, "revealed_by_tech", rv, fields)
        mark(cat, it, u, "TileResources", fields)
    return d


def improvements(uc, ids, cat):
    pool = uc.pool("TileImprovements")
    d = load_cat(cat)
    for it in d["items"]:
        hit = pool.get(key(it["name"]))
        u = hit[0] if hit else None
        fields = []
        if u is not None:
            b = it.get("build")
            if b:
                tr = ids.get("tech", u.get("techRequired"))
                if u.get("techRequired") is None or tr:
                    b["required_tech"] = tr
                    fields.append("build.required_tech")
                if "turnsToBuild" in u:
                    b["work_time"] = u["turnsToBuild"] * 100
                    fields.append("build.work_time")
        mark(cat, it, u, "TileImprovements", fields)
    return d


def terrain(uc, ids, cat):
    pool = uc.pool("Terrains")
    d = load_cat(cat)
    for it in d["items"]:
        hit = pool.get(key(it["name"]))
        u = hit[0] if hit else None
        fields = []
        if u is not None:
            y = {k: u[k] for k in ("food", "production", "gold", "science", "culture", "faith") if u.get(k)}
            # Hills and Mountains are pseudo-terrains in Unciv; the importer handles them as plots.
            # Feature and natural-wonder yields are additive in Unciv's model, not here: kept.
            if cat == "terrain" and it["id"] not in ("core:terrain/hill", "core:terrain/mountain"):
                setf(cat, it, "yields", y, fields)
                if "movementCost" in u:
                    setf(cat, it, "movement_cost", u["movementCost"], fields)
            if cat == "features" and "defenceBonus" in u:
                setf(cat, it, "defense_pct", int(round(u["defenceBonus"] * 100)), fields)
        mark(cat, it, u, "Terrains", fields)
    if cat == "terrain":
        d["rivers"]["river_yield"] = ("none: a river gives no yield of its own; rivers matter for fresh water "
                                      "(farms), building requirements, city defense and combat. Own value, unverified.")
        d.pop("corrections", None)
    return d


def policies(uc, ids, cat):
    pool = uc.pool("Policies", flat_policies)
    d = load_cat(cat)
    for it in d["branches"] + d["items"]:
        nm = it["name"]
        if it["id"].endswith("/finisher"):  # the branch finisher is Unciv's "<Branch> Complete"
            nm += " Complete"
        hit = pool.get(key(nm))
        if hit and it["id"].endswith("/opener") and not hit[0].get("_branch"):
            hit = None  # a branch opener is a branch in Unciv (Aesthetics is a single policy there)
        u = hit[0] if hit else None
        fields = []
        mark(cat, it, u, "Policies", fields)
    return d


def beliefs(uc, ids, cat):
    pool = uc.pool("Beliefs")
    d = load_cat(cat)
    for it in d["items"]:
        hit = pool.get(key(it["name"]))
        u = hit[0] if hit else None
        mark(cat, it, u, "Beliefs", [])
    rel = uc.pool("Religions")
    for r in d["religions"]:
        r["provenance"] = f"unciv: Religions.json, {r['name']}" if key(r["name"]) in rel else OWN
    return d


# Promotions whose catalog name differs from Unciv's (several share one display name, so by id); each
# pair was compared: the same magnitude in Unciv's unique text.
PROMOTION_IDS = {"air-targeting-1": "Air Targeting I", "air-targeting-2": "Air Targeting II",
                 "air-ambush-1": "Ambush I", "air-ambush-2": "Ambush II",
                 "heli-ambush-1": "Anti-Armor I", "heli-ambush-2": "Anti-Armor II",
                 "repair": "Air Repair", "heli-repair": "Air Repair", "range": "Extended Range",
                 "air-range": "Operational Range", "siege": "Besiege", "evasion-i": "Evasion",
                 "jaguar": "[Jaguar] ability", "ski-infantry": "[Norwegian Ski Infantry] ability"}


def promotions(uc, ids, cat):
    pool = uc.pool("UnitPromotions")
    d = load_cat(cat)
    for it in d["items"]:
        hit = pool.get(key(PROMOTION_IDS.get(it["id"].rsplit("/", 1)[-1], it["name"])))
        mark(cat, it, hit[0] if hit else None, "UnitPromotions", [])
    return d


def civilizations(uc, ids, cat):
    pool = uc.pool("Nations")
    d = load_cat(cat)
    for it in d["items"]:
        it.pop("release", None)  # named the original's expansions; nothing reads it
        hit = pool.get(key(it["name"]))
        mark(cat, it, hit[0] if hit else None, "Nations", [])
    return d


def city_states(uc, ids, cat):
    pool = uc.pool("Nations")
    d = load_cat(cat)
    # The influence table's keys were the original's define names; they are plain snake_case names now
    # (nothing reads them). The per-city-state art style field is unused and dropped.
    common = d.get("common", {})
    if isinstance(common.get("influence"), dict):
        common["influence"] = {k.lower() if k.isupper() else k: v for k, v in common["influence"].items()}
    for it in d["items"]:
        it.pop("art_style", None)
        hit = pool.get(key(it["name"]))
        mark(cat, it, hit[0] if hit else None, "Nations", [])
    types = uc.pool("CityStateTypes")
    for t in d["types"]:
        hit = types.get(key(t["name"].replace(" City-States", "")))
        t["provenance"] = f"unciv: CityStateTypes.json, {hit[0]['name']}" if hit else OWN
    return d


def great_people(uc, ids, cat):
    pool = uc.pool("Units")
    d = load_cat(cat)
    for it in d["items"]:
        hit = pool.get(key(it["name"]))
        mark(cat, it, hit[0] if hit else None, "Units", [])
    spec = uc.pool("Specialists")
    for s in d["specialists"]:
        hit = spec.get(key(s["name"].split("|")[0]))
        s["provenance"] = f"unciv: Specialists.json, {hit[0]['name']}" if hit else OWN
    return d


def name_lines(names):
    """The importer's line format: four quoted names per line."""
    q = ['"' + n.replace("\\", "\\\\").replace('"', '\\"') + '"' for n in names]
    return [", ".join(q[i:i + 4]) + "," for i in range(0, len(q), 4)]


def nation_names(uc, ids, path="classic/assets/rules/classic/civilizations.ron"):
    """Replace the city and spy name pools of the nations Unciv has by Unciv's lists."""
    pool = uc.pool("Nations")
    nation_ids = {v: k for k, v in ids.maps["nation"].items()}
    out, civ, field, replaced = [], None, None, {"city_names": 0, "spy_names": 0}
    with open(path, encoding="utf-8") as fh:
        lines = fh.read().split("\n")
    i = 0
    while i < len(lines):
        line = lines[i]
        t = line.strip()
        m = re.match(r'^id: "(core:nation/[^"]+)",$', t)
        if m:
            civ = m.group(1)
        mm = re.match(r"^(city_names|spy_names): \[$", t)
        if mm and civ:
            field = mm.group(1)
            hit = pool.get(nation_ids.get(civ, ""))
            src = None
            if hit:
                src = hit[0].get("cities" if field == "city_names" else "spyNames")
            j = i + 1
            while lines[j].strip() != "],":
                j += 1
            indent = line[: len(line) - len(line.lstrip())]
            out.append(line)
            if src:
                # Unciv's names first, without case-insensitive duplicates; a list that is too short
                # to found a dozen cities (the Huns: one name) is filled up with our present names.
                names, seen = [], set()
                for n in (x.strip() for x in src):
                    if n and n.lower() not in seen:
                        seen.add(n.lower())
                        names.append(n)
                if len(names) < (10 if field == "city_names" else 3):
                    for old in re.findall(r'"((?:[^"\\]|\\.)*)"', "\n".join(lines[i + 1:j])):
                        if old.lower() not in seen:
                            seen.add(old.lower())
                            names.append(old)
                out.extend(indent + "    " + l for l in name_lines(names))
                replaced[field] += 1
            else:
                out.extend(lines[i + 1:j])
            out.append(lines[j])
            i = j + 1
            continue
        out.append(line)
        i += 1
    with open(path, "w", encoding="utf-8") as fh:
        fh.write("\n".join(out))
    return replaced


def merge_order(old, new):
    """Keep the order of the entries that are still valid, then append the new ones."""
    out = [x for x in old if x in new]
    seen = set(out)
    out += [x for x in new if x not in seen]
    return out


def sync_tech_links(cats):
    """techs.json lists what each tech unlocks and makes obsolete; keep it equal to the
    required_tech / obsolete_tech / revealed_by_tech of the entries."""
    unlock_u, unlock_b, obs_u, obs_b, reveals = {}, {}, {}, {}, {}
    for k in ("units", "unique_units"):
        for it in cats[k]["items"]:
            if it.get("required_tech"):
                unlock_u.setdefault(it["required_tech"], []).append(it["id"])
            if it.get("obsolete_tech"):
                obs_u.setdefault(it["obsolete_tech"], []).append(it["id"])
    for k in ("buildings", "unique_buildings", "wonders"):
        for it in cats[k]["items"]:
            if it.get("kind") in ("project", "league_project"):
                continue
            if it.get("required_tech"):
                unlock_b.setdefault(it["required_tech"], []).append(it["id"])
            if it.get("obsolete_tech"):
                obs_b.setdefault(it["obsolete_tech"], []).append(it["id"])
    for it in cats["resources"]["items"]:
        if it.get("revealed_by_tech"):
            reveals.setdefault(it["revealed_by_tech"], []).append(it["id"])
    for t in cats["techs"]["items"]:
        i = t["id"]
        t["unlocks"]["units"] = merge_order(t["unlocks"]["units"], unlock_u.get(i, []))
        t["unlocks"]["buildings"] = merge_order(t["unlocks"]["buildings"], unlock_b.get(i, []))
        t["unlocks"]["reveals_resources"] = merge_order(t["unlocks"]["reveals_resources"], reveals.get(i, []))
        t["obsoletes"]["units"] = merge_order(t["obsoletes"]["units"], obs_u.get(i, []))
        t["obsoletes"]["buildings"] = merge_order(t["obsoletes"]["buildings"], obs_b.get(i, []))


HANDLERS = {
    "units": units, "unique_units": units, "buildings": buildings, "unique_buildings": buildings,
    "wonders": buildings, "techs": techs, "eras": eras, "resources": resources,
    "improvements": improvements, "terrain": terrain, "features": terrain, "natural_wonders": terrain,
    "policies": policies, "beliefs": beliefs, "promotions": promotions, "civilizations": civilizations,
    "city_states": city_states, "great_people": great_people,
}


# ----------------------------------------------------------------------------- own overrides
def own_overrides(cats):
    """Apply classic/tools/own-overrides.json to the entries Unciv does not have.

    Per entry id: `set` (fields to write), `effects` ("all" or a list of effect indices that count as
    checked by hand), `effect_set` (index -> fields of that effect to correct), `effects_only` (an entry that
    cites Unciv: only the listed effects still `own value, unverified` are marked). The entry's provenance
    becomes `own value` (no external source is named); marked effects get the same `source`. Entries without an
    override stay `own value, unverified`."""
    with open(OVERRIDES, encoding="utf-8") as fh:
        ov = json.load(fh)["entries"]
    seen = set()
    for cat, d in cats.items():
        for lk, lst in d.items():
            if not isinstance(lst, list):
                continue
            for it in lst:
                if not isinstance(it, dict) or it.get("id") not in ov:
                    continue
                o = ov[it["id"]]
                seen.add(it["id"])
                src = OWN_CHECKED
                if o.get("effects_only"):  # an Unciv entry whose Brave New World effects the article states
                    effs = it.get("effects", [])
                    for i in o.get("effects", []):
                        if effs[i].get("source") == OWN:
                            effs[i]["source"] = src
                            STATS["civ_effects"] += 1
                    continue
                if it.get("provenance") != OWN and it.get("provenance") != OWN_CHECKED:
                    STATS["civ_skipped"].append(f"{it['id']}: has {it.get('provenance')}")
                    continue
                for f, v in o.get("set", {}).items():
                    if it.get(f) != v:
                        STATS["civ_changed"].append(f"{it['id']}.{f}: {it.get(f)!r} -> {v!r}")
                    it[f] = v
                it["provenance"] = src
                if lk in ("items", "branches"):  # the entries `mark` counts
                    STATS["civ_verified"][cat] = STATS["civ_verified"].get(cat, 0) + 1
                effs = it.get("effects", [])
                for i, fields in o.get("effect_set", {}).items():
                    e = effs[int(i)]
                    for f, v in fields.items():
                        if e.get(f) != v:
                            STATS["civ_changed"].append(f"{it['id']}.effects[{i}].{f}: {e.get(f)!r} -> {v!r}")
                        e[f] = v
                idx = range(len(effs)) if o.get("effects") == "all" else o.get("effects", [])
                for i in idx:
                    effs[i]["source"] = src
                    STATS["civ_effects"] += 1
                # a nation's effects sit in its abilities: "all" or "<ability>.<effect>" indices
                abl = [(f"{a}.{i}", e) for a, ab in enumerate(it.get("ability", [])) for i, e in enumerate(ab["effects"])]
                abl += [(f"tier{t['tier']}.{i}", e) for t in it.get("reward_tiers", []) for i, e in enumerate(t["effects"])]
                want = o.get("ability_effects", [])
                for ref, e in abl:
                    if want == "all" or ref in want:
                        e["source"] = src
                        STATS["civ_effects"] += 1
    STATS["civ_unknown"] = sorted(set(ov) - seen)


# ----------------------------------------------------------------------------- scrub
PROVENANCE_KEYS = ("verify", "verify_fields", "verify_note", "defines", "text", "cost_formula", "note")


def effect_source(e, ufile):
    """The neutral provenance of one typed effect."""
    # `tag` (nearby_improvement_combat_bonus, attack_bonus_turns, ...) names the effects the importer
    # recognises by role; it is kept as it is in the catalogs.
    e["source"] = OWN


# The original's enum-style names in data values become plain snake_case names (the importer
# reads the new spelling). VICTORY_SPACE_RACE is not renamed: it is part of the compiled ruleset.
ENUM_PREFIXES = ("CIVILIAN_ATTACK_PRIORITY_", "SPECIALUNIT_", "PROCESS_", "LEAGUE_PROJECT_REWARD_")
ENUM_NAMES = {"HURRY_GOLD": "gold", "INVISIBLE_SUBMARINE": "invisible_submarine",
              "GREAT_WORK_PARTHENON_FRIEZE": "parthenon_frieze"}


def plain_name(v):
    if v in ENUM_NAMES:
        return ENUM_NAMES[v]
    for p in ENUM_PREFIXES:
        if v.startswith(p) and v[len(p):].replace("_", "").isalnum():
            return v[len(p):].lower()
    return v


def scrub(o, cat, in_effect=False):
    if isinstance(o, dict):
        if "modifier" in o and "selector" in o:
            effect_source(o, cat)
        for k in list(o):
            if k in PROVENANCE_KEYS:
                del o[k]
            elif isinstance(o[k], str):
                if k == "source" and not ("modifier" in o and "selector" in o):
                    if not o[k].startswith("unciv:"):
                        o[k] = OWN
                else:
                    o[k] = plain_name(o[k])
            elif k == "source" and not ("modifier" in o and "selector" in o):
                if isinstance(o[k], str) and not o[k].startswith("unciv:"):
                    o[k] = OWN
            else:
                scrub(o[k], cat)
    elif isinstance(o, list):
        for i, v in enumerate(o):
            if isinstance(v, str):
                o[i] = plain_name(v)
            else:
                scrub(v, cat)


STAT_NAMES = {"food": "food", "production": "production", "gold": "gold", "science": "science",
              "culture": "culture", "faith": "faith"}


def building_claims(u):
    """The typed effects of one Unciv building entry that this script can translate:
    (modifier, selector kind, amount). Everything else stays unconfirmed."""
    claims = []
    for stat in STAT_NAMES:
        if u.get(stat):
            claims.append((f"yield.{stat}.flat", "this_city", u[stat]))
    if u.get("happiness"):
        claims.append(("happiness.local", "this_city", u["happiness"]))
    for stat, pct in (u.get("percentStatBonus") or {}).items():
        if stat in STAT_NAMES and pct:
            claims.append((f"yield.{stat}.pct", "this_city", pct))
    for uq in u.get("uniques", []):
        m = re.fullmatch(r"\[\+(\d+) (\w+)\] per \[(\d+)\] population \[in this city\]", uq)
        if m and m.group(2).lower() in STAT_NAMES:
            claims.append((f"yield.{m.group(2).lower()}.per_pop", "this_city", int(m.group(1)) / int(m.group(3))))
            continue
        m = re.fullmatch(r"\[\+(\d+)\]% Production when constructing \[[^\]]+\] units \[in this city\]", uq)
        if m:
            claims.append(("production.units.pct", "this_city", int(m.group(1))))
            continue
        m = re.fullmatch(r"\[\+(\d+) (\w+)\] from \[[^\]]+\] tiles \[in this city\]", uq)
        if m and m.group(2).lower() in STAT_NAMES:
            claims.append((f"tile.yield.{m.group(2).lower()}.flat", "tiles_of_city", int(m.group(1))))
            continue
        m = re.fullmatch(r"\[\+(\d+) Happiness\] \[in all cities\]", uq)
        if m:
            claims.append(("happiness.global", "player", int(m.group(1))))
            continue
        m = re.fullmatch(r"New \[[^\]]+\] units start with \[(\d+)\] XP \[in this city\]", uq)
        if m:
            claims.append(("unit.free_xp", "units_trained_here", int(m.group(1))))
    return claims


# A building stat (culture, happiness, gold, percent bonus) whose number differs from Unciv's entry takes
# Unciv's number (False: keep ours and only list the difference).
BUILDING_STAT_CORRECTIONS = True


def confirm_building_effects(uc, cats):
    """Mark the effects of buildings that equal what Unciv's entry says as `unciv: ...`; list the
    Unciv claims our effects do not have (not applied: the numbers of the game stay as they are)."""
    pool = uc.pool("Buildings")
    confirmed = total = 0
    for cat in ("buildings", "unique_buildings", "wonders"):
        for it in cats[cat]["items"]:
            hit = pool.get(key(it["name"]))
            total += len(it.get("effects", []))
            if not hit:
                continue
            claims = building_claims(hit[0])
            for e in it.get("effects", []):
                k = (e["modifier"], e["selector"]["kind"], e.get("amount"))
                for c in claims:
                    if c[0] == k[0] and c[1] == k[1] and abs(float(c[2]) - float(k[2] or 0)) < 1e-9:
                        claims.remove(c)
                        e["source"] = f"unciv: Buildings.json, {hit[0]['name']}"
                        confirmed += 1
                        break
            for c in list(claims):  # the same stat with another number: Unciv's number
                own = [e for e in it.get("effects", []) if e.get("source") == OWN and e["modifier"] == c[0]
                       and e["selector"]["kind"] == c[1] and not e.get("condition")]
                if BUILDING_STAT_CORRECTIONS and len(own) == 1 and sum(1 for x in claims if x[:2] == c[:2]) == 1:
                    STATS["uq_changed"].append(f"{it['id']} {c[0]}: {own[0].get('amount')} -> {c[2]} "
                                               f"(unciv: Buildings.json, {hit[0]['name']})")
                    own[0]["amount"] = c[2]
                    own[0]["source"] = f"unciv: Buildings.json, {hit[0]['name']}"
                    claims.remove(c)
                    confirmed += 1
            for c in claims:
                STATS["claims_missing"].append(f"{it['name']}: Unciv has {c[0]} {c[2]}, no such effect here")
    STATS["effects_confirmed"] = confirmed
    STATS["effects_total_buildings"] = total


# ----------------------------------------------------------------------------- unique translation
# Unciv writes an entry's rules as unique texts ("[+15]% Strength <vs [Water] units>"); our catalogs
# use typed effects (modifier, amount, condition). UNIQUE_RULES translates the common patterns into
# claims (modifier, amount, condition tokens, selector kinds or None); an effect whose modifier, condition
# tokens and selector equal a claim is confirmed (`source: unciv: <File>.json, <entry>`); where only the
# amount differs, Unciv's amount is taken (listed in the report). Our condition tokens: the last path
# segment of every condition value ("core:domain/sea" -> "sea"), `key` for a true flag, `key=N` for a number.
UNCIV_TOKEN = {"water": "sea", "armored": "armor", "barbarian": "barbarian", "land": "land", "mounted": "mounted",
               "forest": "forest", "jungle": "jungle", "snow": "snow", "tundra": "tundra", "hill": "hill",
               "bomber": "bomber", "helicopter": "helicopter"}
# Unciv groups that our conditions split into several unit combats (one claim each).
UNCIV_SPLIT = {"air": ("fighter", "bomber"), "water": ("sea", "navalranged", "navalmelee"), "fighter": ("fighter", "triplane")}
SELECTORS = {"all cities": {"all_cities", "player"}, "capital": {"capital"},
             "this city": {"this_city", "following_cities"}, "cities following this religion": {"following_cities"},
             "all cities with a garrison": {"all_cities"}}
YIELDS = {"food", "production", "gold", "science", "culture", "faith", "tourism"}


def eff_tokens(cond):
    out = set()

    def walk(k, v):
        if isinstance(v, dict):
            for kk, vv in v.items():
                walk(kk, vv)
        elif v is True:
            out.add(k)
        elif isinstance(v, str):
            out.add(v.rsplit("/", 1)[-1])
        elif isinstance(v, (int, float)) and not isinstance(v, bool):
            out.add(f"{k}={v:g}")
    walk("", cond or {})
    return frozenset(out)


def _num(s):
    v = float(s)
    return int(v) if v == int(v) else v


def strength_claims(n, conds):
    """'[+N]% Strength <...>' with its conditionals -> claims."""
    att = "when attacking" in conds
    dfn = "when defending" in conds
    toks, terrain, vs = set(), None, None
    for c in conds:
        m = re.fullmatch(r"vs \[(\w+)\] units", c)
        if m:
            vs = m.group(1).lower()
        m = re.fullmatch(r"when fighting in \[([\w ]+)\] tiles", c)
        if m:
            terrain = m.group(1).lower()
        if c == "with a garrison":
            toks.add("city_garrisoned")
        if c == "during a Golden Age":
            toks.add("golden_age")
        if c == "when fighting units from a Civilization with more Cities than you":
            toks.add("civ_with_more_cities")
        if c == "vs [City-States]":
            toks.add("city_state")
    if any(re.fullmatch(r"when adjacent to a \[\w+\] unit", c) for c in conds):
        return [("combat.adjacent_friendly_unit.pct", n, frozenset())]
    if "vs cities" in conds:
        return [("combat.attack_vs_city.pct", n, frozenset(toks))]
    if vs == "wounded":
        return [("combat.attack_vs_wounded.pct", n, frozenset())]
    if vs == "ranged" and dfn:
        return [("combat.defense_vs_ranged.pct", n, frozenset())]
    if terrain in ("open terrain", "rough terrain"):
        t = terrain.split()[0]
        if att:
            return [(f"combat.ranged_attack_{t}_terrain.pct", n, frozenset())]
        return [(f"combat.attack_{t}_terrain.pct", n, frozenset()), (f"combat.defense_{t}_terrain.pct", n, frozenset())]
    if terrain == "friendly land":
        return [("combat.in_friendly_land.pct", n, frozenset())]
    if terrain in UNCIV_TOKEN:
        tk = frozenset({UNCIV_TOKEN[terrain]})
        return [("combat.attack.pct", n, tk), ("combat.defense.pct", n, tk)]
    if terrain and terrain != "foreign land":
        return []
    if terrain == "foreign land":
        return [("combat.outside_friendly_land.pct", n, frozenset())]
    if "when [Embarked]" in conds or "for [Embarked] units" in conds:
        return [("combat.embarked_defense.pct", n, frozenset())] if dfn else []
    mod = "combat.attack.pct" if att else "combat.defense.pct" if dfn else "combat.strength.pct"
    if vs:
        if vs not in UNCIV_TOKEN and vs not in UNCIV_SPLIT:
            return []
        return [(mod, n, frozenset(toks | {t})) for t in UNCIV_SPLIT.get(vs, (UNCIV_TOKEN.get(vs),))]
    return [(mod, n, frozenset(toks))]


# (pattern over the unique without its conditionals, claim builder (match, conditionals) -> claims)
UNIQUE_RULES = [
    (r"\[([+-]\d+)\]% Strength", lambda m, c: strength_claims(_num(m[1]), c)),
    (r"\[([+-]\d+)\]% Strength for cities", lambda m, c: [("city.ranged_strike.pct", _num(m[1]),
                                                            frozenset({"city_garrisoned"} if "with a garrison" in c else ()))]
     if "when attacking" in c else []),
    (r"\[([+-]\d+)\] Sight", lambda m, c: [("unit.embarked_sight.flat", _num(m[1]), frozenset())]
     if c in (["when [Embarked]"], ["for [Embarked] units"]) else []),
    (r"\[\+(\d+)\] Movement", lambda m, c: [("unit.naval_moves.flat", _num(m[1]), frozenset())]
     if c in (["for [{Military} {Water}] units"], ["for [Water] units"]) else
     [("unit.embarked_moves.flat", _num(m[1]), frozenset())] if c == ["for [Embarked] units"] else
     [("unit.moves.flat", _num(m[1]), frozenset({"golden_age"}))] if c == ["for [All] units", "during a Golden Age"] else []),
    (r"\[([+-]\d+)\]% Gold cost of acquiring tiles \[in ([\w ]+)\]",
     lambda m, c: [("tile_purchase.cost.pct", _num(m[1]), frozenset(), SELECTORS.get(m[2]))]),
    (r"\[([+-]\d+)\]% Culture cost of natural border growth \[in (all cities|this city)\]",
     lambda m, c: [("border.culture_cost.pct", _num(m[1]), frozenset(), SELECTORS.get(m[2]))]),
    (r"\[([^\]]+)\] from \[[\w ]+\] tiles(?: without \[[\w ]+\])? \[in this city\]",
     lambda m, c: [x for x in stat_claims(m[1], "from", None, c) if x[0].startswith("tile.")]),
    (r"\[(\d+)\]% Food is carried over after population increases \[in this city\]",
     lambda m, c: [("growth.food_kept.pct", _num(m[1]), frozenset())]),
    (r"Provides \[(\d+)\] \[\w+\]", lambda m, c: [("resource.provide", _num(m[1]), frozenset())]),
    (r"Population loss from nuclear attacks \[([+-]\d+)\]% \[in this city\]",
     lambda m, c: [("city.nuke_damage.pct", _num(m[1]), frozenset())]),
    (r"\[([+-]\d+)\]% enemy spy effectiveness \[in ([\w ]+)\]",
     lambda m, c: [("espionage.enemy_steal_rate.pct", _num(m[1]), frozenset(), SELECTORS.get(m[2]))]),
    (r"\[\+(\d+)\]% \[Gold\] from Trade Routes", lambda m, c: [("city_connection.gold.pct", _num(m[1]), frozenset())]),
    (r"Science gained from research agreements \[\+(\d+)\]%", lambda m, c: [("research_agreement.science.pct", _num(m[1]), frozenset())]),
    (r"\[([+-]\d+)\]% Culture cost of adopting new Policies", lambda m, c: [("policy.cost.pct", _num(m[1]), frozenset())]),
    (r"Gain an extra spy", lambda m, c: [("espionage.spies.flat", 1, frozenset())]),
    (r"New spies start with \[(\d+)\] level\(s\)", lambda m, c: [("espionage.new_spy_rank.flat", _num(m[1]), frozenset())]),
    (r"Promotes all spies \[(\d+)\] time\(s\)", lambda m, c: [("espionage.spy_rank.flat", _num(m[1]), frozenset())]),
    (r"\[\+(\d+)\] population \[in all cities\]", lambda m, c: [("city.population.flat", _num(m[1]), frozenset())]),
    (r"\[\+(\d+) Happiness\] \[in all cities\]", lambda m, c: [("happiness.per_city", _num(m[1]), frozenset())]),
    (r"Gain a free \[[\w ]+\] \[in all cities\]", lambda m, c: [("grant.building", 1, frozenset())]),
    (r"\[\+(\d+)\]% Natural religion spread \[in this city\]", lambda m, c: [("religion.pressure_out.pct", _num(m[1]), frozenset())]),
    (r"Gain \[(\d+)\] \[Gold\]", lambda m, c: [("great_person_expended.gold.flat", _num(m[1]), frozenset())]
     if "upon expending a [Great Person] unit" in c else []),
    (r"Enables nuclear weapon", lambda m, c: [("project.allows_nukes", True, frozenset())]),
    (r"Empire enters golden age", lambda m, c: [("golden_age.start", True, frozenset())]),
    (r"Free Great Person", lambda m, c: [("grant.great_person_choice", 1, frozenset())]),
    (r"Removes extra unhappiness from annexed cities", lambda m, c: [("building.no_occupied_unhappiness", True, frozenset())]),
    (r"Connects trade routes over water", lambda m, c: [("building.allows_water_routes", True, frozenset())]),
    (r"All newly-trained \[[^\]]+\] units \[in this city\] receive the \[[\w ]+\] promotion",
     lambda m, c: [("grant.promotion", 1, frozenset())]),
    (r"\[([+-]\d+)\]% \[\w+\] resource production", lambda m, c: [("resource.quantity.pct", _num(m[1]), frozenset())]),
    (r"Earn \[(\d+)\]% of killed \[Military\] unit's \[Strength\] as \[Culture\]",
     lambda m, c: [("culture.on_kill.pct_of_strength", _num(m[1]), frozenset())]),
    (r"When conquering an encampment, earn \[\d+\] Gold and recruit a Barbarian unit",
     lambda m, c: [("trait.land_barbarian_conversion_percent", _num(re.search(r"\[(\d+)\]", c[0])[1]), frozenset())] if c else []),
    (r"\[([+-]\d+)\]% maintenance costs", lambda m, c: [("unit.maintenance.pct", _num(m[1]), frozenset())]),
    (r"\[([+-]\d+)\]% unhappiness from the number of cities", lambda m, c: [("unhappiness.from_cities.pct", _num(m[1]), frozenset())]),
    (r"\[([+-]\d+)\]% Unhappiness from \[Population\] \[in all cities\]",
     lambda m, c: [("unhappiness.from_population.pct", _num(m[1]), frozenset())]),
    (r"\[([+-]\d+)\]% Golden Age length", lambda m, c: [("golden_age.duration.pct", _num(m[1]), frozenset())]),
    (r"\[([+-]\d+)\]% Production towards any buildings that already exist in the Capital",
     lambda m, c: [("trait.capital_building_modifier", _num(m[1]), frozenset())]),
    (r"\[\w[\w ]*\] is earned \[(\d+)\]% faster", lambda m, c: [("great_people.rate.pct", _num(m[1]), frozenset(), {"all_cities"})]),
    (r"\[([+-]\d+)\]% maintenance on road & railroads", lambda m, c: [("route.maintenance.pct", _num(m[1]), frozenset())]),
    (r"\[\+(\d+) Gold\] for discovering a Natural Wonder \(bonus enhanced to \[\+(\d+) Gold\] if first to discover it\)",
     lambda m, c: [("gold.on_discover_natural_wonder", _num(m[1]), frozenset()),
                   ("gold.on_discover_natural_wonder_first", _num(m[2]), frozenset())]),
    (r"\[\+(\d+)\]% Yield from every \[Natural Wonder\]", lambda m, c: [("tile.yield.any.pct", _num(m[1]), frozenset({"natural_wonder"}))]),
    (r"Retain \[(\d+)\]% of the happiness from a luxury after the last copy has been traded away",
     lambda m, c: [("trait.luxury_happiness_retention", _num(m[1]), frozenset())]),
    (r"Gain \[(\d+)\] Influence with a \[Great Person\] gift to a City-State",
     lambda m, c: [("trait.great_person_gift_influence", _num(m[1]), frozenset())]),
    (r"When declaring friendship, both parties gain a \[(\d+)\]% boost to great person generation",
     lambda m, c: [("trait.dof_great_person_modifier", _num(m[1]), frozenset())]),
    (r"Cities are razed \[(\d+)\] times as fast", lambda m, c: [("trait.raze_speed_modifier", (int(m[1]) - 1) * 100, frozenset())]),
    (r"May capture killed \[Water\] units", lambda m, c: [("promotion.capture_defeated_enemy", True, frozenset())]),
    (r"Hovering unit", lambda m, c: [("promotion.hovering_unit", True, frozenset())]),
    (r"\[(\d+)\]% chance to intercept air attacks", lambda m, c: [("air.intercept_chance.flat", _num(m[1]), frozenset())]),
    (r"Can carry \[(\d+)\] \[\w+\] units", lambda m, c: [("unit.cargo.flat", _num(m[1]), frozenset())]),
    (r"Can see invisible \[Submarine\] units", lambda m, c: [("promotion.see_invisible", 1, frozenset())]),
    (r"All tiles cost 1 movement", lambda m, c: [("promotion.flat_movement_cost", True, frozenset())]),
    (r"Unable to capture cities", lambda m, c: [("promotion.no_capture", True, frozenset())]),
    (r"Must set up to ranged attack", lambda m, c: [("promotion.must_set_up_to_ranged_attack", True, frozenset())]),
    (r"Rough terrain penalty", lambda m, c: [("promotion.rough_terrain_ends_turn", True, frozenset())]),
    (r"May enter foreign tiles without open borders.*", lambda m, c: [("promotion.rival_territory", True, frozenset())]),
    (r"May enter foreign tiles without open borders, but loses \[(\d+)\] religious strength each turn it ends there",
     lambda m, c: []),
    (r"May Paradrop to \[Land\] tiles up to \[(\d+)\] tiles away", lambda m, c: [("unit.paradrop_range.flat", _num(m[1]), frozenset())]),
    (r"\[\+(\d+)\] HP when healing", lambda m, c: [] if c else [
        (f"heal.in_{w}_land.flat", _num(m[1]), frozenset()) for w in ("enemy", "neutral", "friendly")]),
    (r"\[([+-]\d+)\] (Sight|Movement|Range)", lambda m, c: [] if [x for x in c if not x.startswith("for [")] else [(
        {"Sight": "unit.sight.flat", "Movement": "unit.moves.flat", "Range": "unit.range.flat"}[m[2]], _num(m[1]), frozenset())]),
    (r"\[\+(\d+)\] HP when healing", lambda m, c: [("heal.in_enemy_land.flat", _num(m[1]), frozenset()),
                                                   ("heal.in_neutral_land.flat", _num(m[1]), frozenset())]
     if c == ["in [Foreign Land] tiles"] else []),
    (r"All adjacent units heal \[\+(\d+)\] HP when healing", lambda m, c: [("heal.flat", _num(m[1]), frozenset())]),
    (r"\[(\d+)\] additional attacks per turn", lambda m, c: [("unit.attacks_per_turn.flat", _num(m[1]), frozenset())]),
    (r"Can move after attacking", lambda m, c: [("promotion.can_move_after_attacking", True, frozenset())]),
    (r"Unit will heal every turn, even if it performs an action", lambda m, c: [("promotion.always_heal", True, frozenset())]),
    (r"May heal outside of friendly territory", lambda m, c: [("promotion.heal_outside_friendly", True, frozenset())]),
    (r"Double movement in \[(\w+)\]", lambda m, c: [("unit.double_move", True, frozenset({m[1].lower()}))]),
    (r"\[\+(\d+)\]% Damage when intercepting", lambda m, c: [("air.interception_strength.pct", _num(m[1]), frozenset())]),
    (r"\[\+(\d+)\]% Strength when performing Air Sweep", lambda m, c: [("air.sweep_strength.pct", _num(m[1]), frozenset())]),
    (r"Can carry \[(\d+)\] extra \[\w+\] units", lambda m, c: [("unit.cargo.flat", _num(m[1]), frozenset())]),
    (r"Earn \[(\d+)\]% of the damage done to \[City\] units as \[Gold\]",
     lambda m, c: [("combat.city_attack_gold.pct_of_damage", _num(m[1]), frozenset())]),
    (r"\[Great General\] is earned \[(\d+)\]% faster", lambda m, c: [
        ("combat.great_general_bonus.pct", _num(m[1]), frozenset(), {"this_unit"}),
        ("great_people.rate.pct", _num(m[1]), frozenset(), {"player"})]),
    (r"\[\+(\d+)\]% XP gained from combat", lambda m, c: [("unit.xp_gain.pct", _num(m[1]), frozenset())]),
    (r"Damage taken from interception reduced by \[(\d+)\]%",
     lambda m, c: [("air.damage_taken_from_interception.pct", -_num(m[1]), frozenset()), ("air.evasion.pct", _num(m[1]), frozenset())]),
    (r"\[(\d+)\] extra interceptions may be made per turn", lambda m, c: [("air.interceptions_per_turn.flat", _num(m[1]), frozenset())]),
    (r"Withdraws before melee combat", lambda m, c: [("combat.withdraw_chance.pct", _num(re.search(r"\[(\d+)\]", c[0])[1]), frozenset())]
     if c else []),
    (r"Ranged attacks may be performed over obstacles", lambda m, c: [("promotion.range_attack_ignore_los", True, frozenset())]),
    (r"Eliminates combat penalty for attacking over a river", lambda m, c: [("promotion.amphib", True, frozenset())]),
    (r"Heals \[(\d+)\] damage if it kills a unit", lambda m, c: [("heal.on_kill.flat", _num(m[1]), frozenset())]),
    (r"\[([+-]\d+)\]% Strength for enemy \[Military\] units in adjacent \[All\] tiles",
     lambda m, c: [("combat.strength.pct", _num(m[1]), frozenset()), ("aura.range", 1, frozenset())]),
    # yields, happiness, production
    (r"\[([^\]]+)\] \[in ([\w ]+)\]", lambda m, c: stat_claims(m[1], "", SELECTORS.get(m[2]), c,
                                                              "city_garrisoned" if "garrison" in m[2] else None)),
    (r"\[([^\]]+)\] from every \[([\w' ]+)\]", lambda m, c: stat_claims(m[1], "from", None, c)),
    (r"\[([^\]]+)\] from all \[[\w ]+\] buildings", lambda m, c: stat_claims(m[1], "from", None, c)),
    (r"\[([+-]\d+)\]% \[(\w+)\] \[in ([\w ]+)\]", lambda m, c: [(f"yield.{m[2].lower()}.pct", _num(m[1]), frozenset(),
                                                                 SELECTORS.get(m[3]))] if m[2].lower() in YIELDS else []),
    (r"\[([+-]\d+)\]% \[(\w+)\] from every \[[\w ]+\]", lambda m, c: [(f"yield.{m[2].lower()}.pct", _num(m[1]), frozenset())]
     if m[2].lower() in YIELDS else []),
    (r"\[([+-]\d+)\]% Production when constructing \[([^\]]+)\] (units|buildings|wonders) \[in ([\w ]+)\]",
     lambda m, c: production_claims(_num(m[1]), m[2], m[3], SELECTORS.get(m[4]))),
    (r"\[([+-]\d+)\]% growth \[in ([\w ]+)\]", lambda m, c: [("growth.rate.pct", _num(m[1]), frozenset(), SELECTORS.get(m[2]))]),
    (r"\[([+-]\d+)\]% Great Person generation \[in ([\w ]+)\]",
     lambda m, c: [("great_people.rate.pct", _num(m[1]), frozenset(), SELECTORS.get(m[2]))]),
    (r"\[([+-]\d+)\] Happiness from each type of luxury resource", lambda m, c: [("happiness.per_luxury.flat", _num(m[1]), frozenset())]),
    (r"\[\+1 Happiness\] per \[(\d+)\] population \[in ([\w ]+)\]", lambda m, c: [("happiness.per_x_pop_in_city", _num(m[1]), frozenset())]),
    (r"\[\+1 Gold, \+1 Happiness\] per \[(\d+)\] population \[in capital\]",
     lambda m, c: [("yield.gold.per_pop", 1 / int(m[1]), frozenset(), {"capital"})]),
    (r"\[\+1 Happiness\] \[in all cities connected to capital\]", lambda m, c: [("happiness.per_city_connection", 1, frozenset())]),
    (r"\[([+-]\d+)\]% Unhappiness from \[Population\] \[in [\w -]+\]", lambda m, c: [("unhappiness.pct", _num(m[1]), frozenset())]),
    (r"Each city founded increases culture cost of policies \[(\d+)\]% less than normal",
     lambda m, c: [("policy.cost_per_city.pct", -_num(m[1]), frozenset())]),
    (r"Provides a \[(Aqueduct)\] in your first \[(\d+)\] cities for free",
     lambda m, c: [("policy.num_cities_free_food_building", _num(m[2]), frozenset())]),
    (r"Provides the cheapest \[Culture\] building in your first \[(\d+)\] cities for free",
     lambda m, c: [("policy.num_cities_free_culture_building", _num(m[1]), frozenset())]),
    (r"Can build \[all\] improvements at a \[\+(\d+)\]% rate", lambda m, c: [("worker.speed.pct", _num(m[1]), frozenset())]),
    (r"\[([+-]\d+)\]% Gold cost of upgrading", lambda m, c: [("unit.upgrade_cost.pct", _num(m[1]), frozenset())]),
    (r"\[Gold\] cost of purchasing items in cities \[([+-]\d+)\]%", lambda m, c: [("purchase.cost.pct", _num(m[1]), frozenset())]),
    (r"\[([+-]\d+)\]% City-State Influence degradation", lambda m, c: [("city_state.influence_decay.pct", _num(m[1]), frozenset())]),
    (r"\[([+-]\d+)\]% Culture cost of natural border growth \[in cities following this religion\]",
     lambda m, c: [("border.culture_cost.pct", _num(m[1]), frozenset())]),
    (r"\[\+(\d+) Science\] from every specialist \[in all cities\]",
     lambda m, c: [("specialist.yield.science.flat", _num(m[1]), frozenset())]),
    (r"\[\+(\d+) (\w+)\] from each Trade Route", lambda m, c: [(f"trade.route.yield.{m[2].lower()}.flat", _num(m[1]), frozenset())]),
    (r"Free \[[\w ]+\] appears", lambda m, c: [("grant.unit", 1, frozenset())]),
    (r"\[(\d+)\] free \[[\w ]+\] units appear", lambda m, c: [("grant.unit", _num(m[1]), frozenset())]),
    (r"Free Technology", lambda m, c: [("grant.tech", 1, frozenset())]),
    (r"\[(\d+)\] Free Technologies", lambda m, c: [("grant.tech", _num(m[1]), frozenset())]),
    (r"\[Faith\] cost of purchasing items in cities \[([+-]\d+)\]%", lambda m, c: [("faith_purchase.cost.pct", _num(m[1]), frozenset())]),
    (r"Free Social Policy", lambda m, c: [("grant.policy", 1, frozenset())]),
    (r"Gain a free \[[\w ]+\] \[in this city\]", lambda m, c: [("grant.building", 1, frozenset())]),
    (r"New \[[\w ]+\] units start with \[(\d+)\] XP \[in ([\w ]+)\]", lambda m, c: [("unit.free_xp", _num(m[1]), frozenset())]),
    (r"Earn \[(\d+)\]% of killed \[Barbarian\] unit's \[Strength\] as \[(\w+)\]",
     lambda m, c: [(f"{m[2].lower()}.on_kill.pct_of_strength", _num(m[1]), frozenset({"barbarian"}))]),
    (r"Earn \[(\d+)\]% of \[Military\] unit's \[Strength\] as \[Faith\] when killed within 4 tiles of a city following this religion",
     lambda m, c: [("faith.on_kill.pct_of_strength", _num(m[1]), frozenset({"max_distance=4"}))]),
    (r"\[([+-]\d+)\]% Spread Religion Strength", lambda m, c: [(
        {"for [Great Prophet] units": "prophet.strength.pct", "for [Missionary] units": "missionary.strength.pct"}.get(
            c[0] if c else "", "-"), _num(m[1]), frozenset())]),
    (r"\[([+-]\d+)\]% Faith cost of generating Great Prophet equivalents",
     lambda m, c: [("prophet.faith_threshold.pct", _num(m[1]), frozenset())]),
    (r"\[Faith\] cost of purchasing \[Missionary\] units \[([+-]\d+)\]%",
     lambda m, c: [("faith_purchase.missionary_inquisitor.cost.pct", _num(m[1]), frozenset())]),
    (r"\[\+(\d+)\]% Natural religion spread \[in City-State cities\]",
     lambda m, c: [("religion.pressure_to_friendly_city_states.pct", _num(m[1]), frozenset())]),
    (r"Resting point for Influence with City-States following this religion \[\+(\d+)\]",
     lambda m, c: [("city_state.influence_resting_point.flat", _num(m[1]), frozenset({"city_state_follows_religion"}))]),
    (r"Gain \[(\d+)\] \[Faith\]", lambda m, c: [("faith.on_great_person_expended", _num(m[1]), frozenset())]
     if "upon expending a [Great Person] unit" in c else []),
    (r"When spreading religion to a city, gain \[(\d+)\] times the amount of followers of other religions as \[Science\]",
     lambda m, c: [("science.on_spread_per_other_religion_follower", _num(m[1]), frozenset())]),
    (r"\[\+(\d+) Gold\] for each global city following this religion",
     lambda m, c: [("yield.gold.per_following_city", _num(m[1]), frozenset())]),
    (r"\[\+(\d+) Faith\] for each global city following this religion",
     lambda m, c: [("yield.faith.per_foreign_following_city", _num(m[1]), frozenset())]),
    (r"\[\+1 Gold\] from every \[(\d+)\] global followers \[in all cities\]",
     lambda m, c: [("yield.gold.per_x_followers", _num(m[1]), frozenset())]),
    (r"\[\+1 Culture\] from every \[(\d+)\] global followers \[in foreign cities\]",
     lambda m, c: [("yield.culture.per_x_foreign_followers", _num(m[1]), frozenset())]),
    (r"\[\+(\d+) Gold\] when a city adopts this religion for the first time",
     lambda m, c: [("gold.on_city_first_converted", _num(m[1]), frozenset())]),
    (r"\[\+1\]% \[Production\] from every follower, up to \[(\d+)\]%",
     lambda m, c: [("yield.production.pct_per_follower_max", _num(m[1]), frozenset())]),
    (r"\[\+(\d+) Faith\] from every \[Wonder\]", lambda m, c: [("world_wonder.yield.faith.flat", _num(m[1]), frozenset())]),
    (r"\[All\] Units adjacent to this city heal \[\+(\d+)\] HP per turn when healing",
     lambda m, c: [("heal.flat", _num(m[1]), frozenset({"adjacent_to_friendly_city"}))]),
    (r"\[\+(\d+) Happiness\] in cities on \[River\] tiles", lambda m, c: [("happiness.local", _num(m[1]), frozenset({"city_on_river"}))]),
    (r"Units in cities cost no Maintenance", lambda m, c: [("policy.garrison_free_maintenance", True, frozenset())]),
    (r"Notified of new Barbarian encampments", lambda m, c: [("policy.always_see_barb_camps", True, frozenset())]),
    (r"\[-50\]% Food consumption by \[Specialists\] \[in all cities\]", lambda m, c: [("policy.half_specialist_food", True, frozenset())]),
]


def cond_tokens(conds):
    out = set()
    for c in conds:
        m = re.fullmatch(r"in cities with at least \[(\d+)\] \[(Population|Followers of this Religion)\]", c)
        if m:
            out.add(("min_population=" if m[2] == "Population" else "min_followers=") + m[1])
    return out


def stat_claims(stats, kind, sels, conds, flag=None):
    """'[+1 Culture, +2 Faith]' -> claims; kind "from" = from every building / tile ('building.' or 'tile.' prefix)."""
    out = []
    toks = cond_tokens(conds) | ({flag} if flag else set())
    for part in stats.split(", "):
        m = re.fullmatch(r"([+-]\d+) (\w+)", part)
        if not m:
            return []
        s, n = m[2].lower(), _num(m[1])
        if s == "happiness":
            mods = ["building.happiness.flat"] if kind == "from" else ["happiness.local", "happiness.global"]
        elif s in YIELDS:
            mods = [f"building.yield.{s}.flat", f"tile.yield.{s}.flat"] if kind == "from" else [f"yield.{s}.flat"]
        else:
            return []
        for mod in mods:
            out.append((mod, n, frozenset(toks), sels))
    return out


def production_claims(n, what, kind, sels):
    if what == "Spaceship part":
        return [("production.spaceship.pct", n, frozenset(), sels)]
    if kind == "wonders":
        return [("production.wonders.pct", n, frozenset(), sels)]
    if kind == "buildings":
        return [("production.buildings.pct" if what == "All" else "production.building_class.pct", n, frozenset(), sels)]
    if what == "Military":
        return [("production.military_units.pct", n, frozenset(), sels)]
    return [("production.units.pct", n, frozenset(), sels)]


def unique_claims(uq):
    conds = re.findall(r"<([^<>]*(?:<[^<>]*>[^<>]*)*)>", uq)
    body = re.sub(r"\s*<[^<>]*(?:<[^<>]*>[^<>]*)*>", "", uq).strip()
    out = []
    for pat, fn in UNIQUE_RULES:
        m = re.fullmatch(pat, body)
        if m:
            for c in fn(m, conds):
                out.append(c if len(c) == 4 else c + (None,))
    return out


def _same(a, b):
    if isinstance(a, bool) or isinstance(b, bool):
        return a is b or a == b
    try:
        return abs(float(a) - float(b)) < 1e-9
    except (TypeError, ValueError):
        return False


def confirm_unique_effects(uc, cats):
    """Every effect still `own value, unverified` of an entry that cites Unciv is compared with the
    claims of that Unciv entry's uniques (one claim confirms one effect)."""
    pools = {"UnitPromotions": uc.pool("UnitPromotions"), "Policies": uc.pool("Policies", flat_policies),
             "Beliefs": uc.pool("Beliefs"), "Buildings": uc.pool("Buildings"), "Nations": uc.pool("Nations"),
             "TileImprovements": uc.pool("TileImprovements"), "TileResources": uc.pool("TileResources"),
             "Techs": uc.pool("Techs", flat_techs), "Units": uc.pool("Units")}
    for cat, d in cats.items():
        for lk, lst in d.items():
            if not isinstance(lst, list):
                continue
            for it in lst:
                if not isinstance(it, dict):
                    continue
                m = re.fullmatch(r"unciv: (\w+)\.json, (.+)", str(it.get("provenance", "")))
                if not m or m[1] not in pools:
                    continue
                hit = pools[m[1]].get(key(m[2]))
                if not hit:
                    continue
                src = f"unciv: {m[1]}.json, {hit[0]['name']}"
                claims = [c for uq in hit[0].get("uniques", []) for c in unique_claims(uq)]
                effs = list(it.get("effects", []))
                effs += [e for ab in it.get("ability", []) for e in ab.get("effects", [])]
                match_claims(it, effs, claims, src)
    # Promotions Unciv does not have as promotions are often the abilities of the units that carry them
    # (Unciv writes those as unit uniques, or as a promotion of the unit): compared with every carrier.
    upro = pools["UnitPromotions"]
    by_id = {p["id"]: p for p in cats["promotions"]["items"] if p.get("provenance") == OWN}
    for cat in ("units", "unique_units"):
        for u in cats[cat]["items"]:
            hit = pools["Units"].get(key(u["name"]))
            if not hit:
                continue
            uqs = list(hit[0].get("uniques", []))
            for pn in hit[0].get("promotions", []):
                uqs += (upro.get(key(pn)) or ({}, ""))[0].get("uniques", [])
            claims = [c for uq in uqs for c in unique_claims(uq)]
            for pid in u.get("free_promotions", []):
                if pid in by_id:
                    p = by_id[pid]
                    match_claims(p, p.get("effects", []), list(claims), f"unciv: Units.json, {hit[0]['name']}")
    # ... or of the wonder, policy or nation that grants them (an effect naming the promotion).
    for cat, d in cats.items():
        for lk, lst in d.items():
            if not isinstance(lst, list) or cat == "promotions":
                continue
            for it in lst:
                if not isinstance(it, dict):
                    continue
                m = re.fullmatch(r"unciv: (\w+)\.json, (.+)", str(it.get("provenance", "")))
                if not m or m[1] not in pools:
                    continue
                hit = pools[m[1]].get(key(m[2]))
                named = set(re.findall(r'"(core:promotion/[\w-]+)"', json.dumps(it))) & set(by_id)
                if not hit or not named:
                    continue
                claims = [c for uq in hit[0].get("uniques", []) for c in unique_claims(uq)]
                for pid in sorted(named):
                    p = by_id[pid]
                    match_claims(p, p.get("effects", []), list(claims), f"unciv: {m[1]}.json, {hit[0]['name']}")
    for p in by_id.values():
        srcs = {e.get("source") for e in p.get("effects", [])}
        if p.get("effects") and len(srcs) == 1 and srcs != {OWN}:
            p["provenance"] = srcs.pop()
            STATS["uq_promotions"].append(p["id"])


def match_claims(it, effs, claims, src):
    own = [e for e in effs if e.get("source") == OWN]

    def fits(c, e):
        return (c[0] == e["modifier"] and c[2] == eff_tokens(e.get("condition"))
                and (c[3] is None or e["selector"]["kind"] in c[3]))
    for e in own:  # exact matches first
        for c in claims:
            if fits(c, e) and _same(c[1], e.get("amount")):
                claims.remove(c)
                e["source"] = src
                STATS["uq_confirmed"] += 1
                STATS["uq_log"].append((it["id"], e["modifier"], e.get("amount"), e["selector"]["kind"]))
                break
    # One Unciv rule that our catalogs split by a selector filter (the same effect for land and sea units,
    # for two resources, ...): the copies that differ only in the filter share the confirmation.
    done = [e for e in own if e["source"] == src]
    for e in [e for e in own if e["source"] == OWN]:
        for d in done:
            if (d["modifier"] == e["modifier"] and _same(d.get("amount"), e.get("amount"))
                    and d.get("condition") == e.get("condition") and d["selector"]["kind"] == e["selector"]["kind"]
                    and d["selector"] != e["selector"]):
                e["source"] = src
                STATS["uq_confirmed"] += 1
                break
    for e in [e for e in own if e["source"] == OWN]:  # the same rule with another number: Unciv's number
        cand = [c for c in claims if fits(c, e)]
        rivals = [x for x in own if x["source"] == OWN and x["modifier"] == e["modifier"]
                  and eff_tokens(x.get("condition")) == eff_tokens(e.get("condition"))]
        if len(cand) == 1 and len(rivals) == 1 and not isinstance(cand[0][1], bool) \
                and isinstance(e.get("amount"), (int, float)) and not isinstance(e.get("amount"), bool):
            STATS["uq_changed"].append(f"{it['id']} {e['modifier']}: {e['amount']} -> {cand[0][1]} ({src})")
            e["amount"] = cand[0][1]
            claims.remove(cand[0])
            e["source"] = src
            STATS["uq_confirmed"] += 1


def vocabulary(cats):
    """classic/docs/classic/effect_vocabulary.json: every modifier / selector / trigger / condition key the
    catalogs use (our own typed-effect names; no column names of any source)."""
    mods, sels, trig, conds = {}, {}, {}, {}

    def walk(o, f):
        if isinstance(o, dict):
            if "modifier" in o and "scope" in o:
                m = mods.setdefault(o["modifier"], {"count": 0, "scopes": set(), "triggers": set(),
                                                    "needs_mechanic": set(), "files": set()})
                m["count"] += 1
                m["scopes"].add(o["scope"])
                m["triggers"].add(o["trigger"])
                m["files"].add(f)
                if o.get("needs_mechanic"):
                    m["needs_mechanic"].add(o["needs_mechanic"])
                k = o["selector"]["kind"]
                sels[k] = sels.get(k, 0) + 1
                trig[o["trigger"]] = trig.get(o["trigger"], 0) + 1
                for c in (o.get("condition") or {}):
                    conds[c] = conds.get(c, 0) + 1
            for v in o.values():
                walk(v, f)
        elif isinstance(o, list):
            for v in o:
                walk(v, f)

    for name, d in sorted(cats.items()):
        walk(d, name + ".json")

    def unit(m):
        if m.endswith(".pct") or ".pct_" in m or m.endswith("_pct"):
            return "percent"
        if m.endswith(".flat") or ".flat_" in m:
            return "flat"
        return "see_modifier"

    return {"schema": "classic-spec/1", "catalog": "effect_vocabulary",
            "edition": EDITION,
            "note": "Every modifier / selector / trigger / condition key used by the catalogs. Modifiers with an "
                    "entity prefix (policy., trait., belief., promotion., building., tech., improvement., project.) "
                    "carry needs_mechanic: a dedicated mechanic must interpret them.",
            "modifiers": {k: {"count": v["count"], "unit": unit(k), "scopes": sorted(v["scopes"]),
                              "triggers": sorted(v["triggers"]), "needs_mechanic": sorted(v["needs_mechanic"]),
                              "used_in": sorted(v["files"])} for k, v in sorted(mods.items())},
            "selector_kinds": dict(sorted(sels.items())), "triggers": dict(sorted(trig.items())),
            "condition_keys": dict(sorted(conds.items()))}


HANDICAP_MAP = {
    "happiness_default": ("baseHappiness", 1), "extra_happiness_per_luxury": ("extraHappinessPerLuxury", 1),
    "num_cities_unhappiness_mod": ("unhappinessModifier", 100),
    "population_unhappiness_mod": ("unhappinessModifier", 100),
    "production_free_units": ("unitSupplyBase", 1), "production_free_units_per_city": ("unitSupplyPerCity", 1),
    "unit_cost_percent": ("unitCostModifier", 100), "building_cost_percent": ("buildingCostModifier", 100),
    "research_percent": ("researchCostModifier", 100), "policy_percent": ("policyCostModifier", 100),
    "ai_unhappiness_percent": ("aiUnhappinessModifier", 100), "ai_growth_percent": ("aiCityGrowthModifier", 100),
    "ai_train_percent": ("aiUnitCostModifier", 100), "ai_construct_percent": ("aiBuildingCostModifier", 100),
    "ai_world_construct_percent": ("aiWonderCostModifier", 100),
    "ai_unit_supply_percent": ("aiUnitSupplyModifier", 100),
    "ai_building_cost_percent": ("aiBuildingMaintenanceModifier", 100),
    "ai_unit_cost_percent": ("aiUnitMaintenanceModifier", 100),
}
HANDICAP_HEADER = """// Difficulty levels. index 0..7 = Settler..Deity, 8 = the AI default level, the level every
// AI player plays at.
// Derived from Unciv (https://github.com/yairm210/Unciv), MPL-2.0: Civ V - Gods & Kings/Difficulties.json
// (classic/tools/import-unciv.py rewrites the fields below from it; commit {commit}). Fields Unciv does not
// carry (route cost, the free-unit percent of population, gold-free units, ai_create, ai_world_train/create,
// ai_unit_upgrade, the per-era modifier, the AI default level) are own values, unverified.
"""


# Values kept although Unciv differs, with the reason (none since the Prince AI unit upkeep percent
# was adopted: classic/docs/classic/RESOURCING.md, "Kept differences").
HANDICAP_KEEP = {}


def handicaps(uc, path="classic/assets/rules/classic/handicaps.ron"):
    """Rewrite the fields of handicaps.ron that Unciv's Difficulties.json carries."""
    levels = {key(l["name"]): l for l in uc.load("gk", "Difficulties")}
    text = open(path, encoding="utf-8").read()
    body_start = text.index("(\n    handicaps:")
    head, body = text[:body_start], text[body_start:]
    changed = []

    def level(m):
        name, rest = m.group(1), m.group(2)
        u = levels.get(key(name))
        if not u:
            return m.group(0)
        for ours, (theirs, scale) in HANDICAP_MAP.items():
            if theirs not in u or (name, ours) in HANDICAP_KEEP:
                continue
            new = int(round(u[theirs] * scale))
            mm = re.search(rf"\n( +){ours}: (-?\d+),", rest)
            if mm and int(mm.group(2)) != new:
                changed.append(f"{name}.{ours}: {mm.group(2)} -> {new}")
                rest = rest.replace(mm.group(0), f"\n{mm.group(1)}{ours}: {new},", 1)
        return f'id: "core:handicap/{name}",{rest}\n        ),'

    body = re.sub(r'id: "core:handicap/(\w+)",(.*?)\n        \),', level, body, flags=re.S)
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(HANDICAP_HEADER.format(commit=uc.commit) + body)
    STATS["changed"]["handicaps"] = changed


SOURCES_OF = {
    "units": ["Units.json", "UnitTypes.json"], "unique_units": ["Units.json"],
    "buildings": ["Buildings.json", "Techs.json"], "unique_buildings": ["Buildings.json", "Techs.json"],
    "wonders": ["Buildings.json", "Techs.json"], "techs": ["Techs.json", "Eras.json"],
    "eras": ["Eras.json"], "resources": ["TileResources.json"], "improvements": ["TileImprovements.json"],
    "terrain": ["Terrains.json"], "features": ["Terrains.json"], "natural_wonders": ["Terrains.json"],
    "policies": ["Policies.json"], "beliefs": ["Beliefs.json", "Religions.json"],
    "promotions": ["UnitPromotions.json"], "civilizations": ["Nations.json"],
    "city_states": ["Nations.json", "CityStateTypes.json"], "great_people": ["Units.json", "Specialists.json"],
}


def attribution(uc):
    """classic/docs/classic/UNCIV-ATTRIBUTION.md: each derived file and the Unciv file it came from."""
    rows = []
    for cat, files in SOURCES_OF.items():
        n = STATS["taken"].get(cat, 0)
        c = STATS["civ_verified"].get(cat, 0)
        m = STATS["own"].get(cat, 0) - min(c, STATS["own"].get(cat, 0))
        rows.append(f"| `classic/docs/classic/{cat}.json` | {', '.join('`' + f + '`' for f in files)} | {n} | {c} | {m} |")
    return f"""# Unciv attribution

The classic rule data is derived from **Unciv** (<https://github.com/yairm210/Unciv>), licensed under the
Mozilla Public License 2.0 (<https://www.mozilla.org/MPL/2.0/>). Unciv's source files are available at the
repository above; the files used are the JSON rule files of `android/assets/jsons/Civ V - Gods & Kings/`
(and `Civ V - Vanilla/` for what the first lacks) at commit `{uc.commit}`.

Derived from Unciv (https://github.com/yairm210/Unciv), MPL-2.0. This applies to the files below. Files that
contain Unciv data stay under MPL-2.0; the other files of the project are separate files under the project's
own terms.

What was taken: numbers and references of the entries that Unciv has (cost, strength, movement, technology
prerequisites, maintenance, yields, name pools), matched by name. Not taken: Unciv's descriptive texts,
quotes and introduction texts (they come from the original game and are not covered by
the licence). Entries Unciv does not have are marked `own value, unverified` in their `provenance` field.

Generated by `classic/tools/import-unciv.py` (reproducible: the same Unciv commit gives the same bytes).

| Derived file | Unciv file(s) | Entries taken from Unciv | Entries checked by hand (own value) | Entries own / unverified |
|---|---|---:|---:|---:|
{chr(10).join(rows)}
| `classic/assets/rules/classic/civilizations.ron` (`city_names`, `spy_names`) | `Nations.json` (`cities`, `spyNames`) | see below | | nations Unciv lacks keep their lists |
| `classic/assets/rules/classic/*.ron` (compiled by `classic-import`) | all of the above | derived from the catalogs | | |
| `classic/docs/classic/effect_vocabulary.json` | none (our own typed-effect names) | | | |

`classic/assets/rules/classic/handicaps.ron` is derived from `Difficulties.json` (see its header).

Typed effects: {STATS.get("effects_confirmed", 0)} of the {STATS.get("effects_total_buildings", 0)} effects of buildings and wonders equal a stat,
percent, per-population, tile, happiness or experience claim of the Unciv entry and carry
`source: "unciv: Buildings.json, <entry>"`. {STATS.get("uq_confirmed", 0)} more effects (of policies, beliefs, promotions,
nations, buildings, wonders, techs and improvements) equal a claim of the Unciv entry's unique texts as the
translation table `UNIQUE_RULES` of `classic/tools/import-unciv.py` reads them and carry
`source: "unciv: <File>.json, <entry>"`; {len(STATS.get("uq_promotions", []))} promotions Unciv has no promotion for cite the Unciv unit,
wonder, policy or nation whose uniques state all their effects.

Entries Unciv does not have and whose numbers were checked by hand are listed in
`classic/tools/own-overrides.json`: such an entry carries `provenance: "own value"`
({sum(STATS["civ_verified"].values())} entries; the count per file is in the table above), a checked effect
`source: "own value"` ({STATS["civ_effects"]} effects). Every other effect is `own value, unverified`.
"""


def main(argv):
    args = [a for a in argv[1:] if not a.startswith("--")]
    if "--unciv" not in argv:
        print(__doc__)
        return 2
    root = argv[argv.index("--unciv") + 1]
    report_only = "--report" in argv
    uc = Unciv(root)
    ids = Ids()
    cats = {}
    for cat, fn in HANDLERS.items():
        d = fn(uc, ids, cat)
        scrub(d, cat)
        d["edition"] = EDITION
        d["notice"] = NOTICE.format(commit=uc.commit)
        cats[cat] = d
    sync_tech_links(cats)
    confirm_building_effects(uc, cats)
    confirm_unique_effects(uc, cats)
    own_overrides(cats)
    if not report_only:
        for cat, d in cats.items():
            save_cat(cat, d)
        with open("classic/docs/classic/UNCIV-ATTRIBUTION.md", "w", encoding="utf-8") as fh:
            fh.write(attribution(uc))
        with open("classic/docs/classic/effect_vocabulary.json", "w", encoding="utf-8") as fh:
            fh.write(json.dumps(vocabulary(cats), indent=1, ensure_ascii=False) + "\n")
    if not report_only:
        handicaps(uc)
        r = nation_names(uc, ids)
        print(f"name pools replaced from Unciv: city_names {r['city_names']}, spy_names {r['spy_names']}")
    print(f"Unciv commit {uc.commit}")
    for cat in HANDLERS:
        print(f"{cat}: from Unciv {STATS['taken'].get(cat, 0)}, own/unverified {STATS['own'].get(cat, 0)}, "
              f"changed values {len(STATS['changed'].get(cat, []))}")
    print(f"building effects confirmed against Unciv: {STATS['effects_confirmed']} of {STATS['effects_total_buildings']}; "
          f"Unciv claims without a matching effect: {len(STATS['claims_missing'])}")
    print(f"checked by hand (own value): {sum(STATS['civ_verified'].values())} entries "
          f"({', '.join(f'{k} {v}' for k, v in STATS['civ_verified'].items())}), {STATS['civ_effects']} effects, "
          f"{len(STATS['civ_changed'])} values corrected")
    print(f"other effects confirmed against Unciv's uniques: {STATS['uq_confirmed']}, "
          f"{len(STATS['uq_changed'])} amounts taken from Unciv; promotions confirmed through their units: "
          f"{len(STATS['uq_promotions'])}")
    for r in STATS["uq_changed"]:
        print("    unciv amount:", r)
    if "--uq-log" in argv:
        for r in STATS["uq_log"]:
            print("    uq:", *r)
    for r in STATS["civ_changed"]:
        print("    corrected:", r)
    for r in STATS["civ_skipped"] + [f"{i}: no such entry" for i in STATS["civ_unknown"]]:
        print("    override not applied:", r)
    if "--residual" in argv:
        for row in STATS["claims_missing"]:
            print("   ", row)
        print("-- entries without an Unciv counterpart (own value, unverified)")
        for cat, names in STATS["own_names"].items():
            print(f"{cat} ({len(names)}): " + ", ".join(names))
    for cat, rows in STATS["changed"].items():
        print(f"-- changed in {cat}")
        for r in rows:
            print("   ", r)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
