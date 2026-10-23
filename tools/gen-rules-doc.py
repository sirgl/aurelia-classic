#!/usr/bin/env python3
"""Generate classic/docs/classic/rules.md from the project's own data.

Usage (from anywhere inside the repository, Python 3 standard library only):
  python3 -I classic/tools/gen-rules-doc.py           write classic/docs/classic/rules.md
  python3 -I classic/tools/gen-rules-doc.py --check   exit 1 when the committed file is stale
  python3 -I classic/tools/gen-rules-doc.py --stdout  print the document instead of writing it

Inputs:
  * the rule constants of the Rust source: every `pub const NAME: Type = value;`
    of classic/crates/classic-rules/src and platform/crates/kernel/src, with the
    `///` doc comment directly above it (the meaning is its first sentence, the
    source is the first source clause found in it: `Unciv: <file>`,
    `Own design`, `Own formula (BNW-like system; ...)`, `own value` or `own value, ... unverified`);
  * the JSON catalogs classic/docs/classic/*.json (counts, key numbers and the
    `provenance` of every entry);
  * classic/assets/rules/classic/handicaps.ron and projects.ron (difficulty
    levels and projects, which have no JSON catalog).

The output is deterministic (sorted, no timestamps), so `--check` can compare
it byte for byte. Formulas are described in short sentences of our own in the
template below; the numbers in them are looked up from the constants, so they
cannot drift from the code. A formula sentence that names a constant the code
no longer has prints a warning and makes `--check` fail.
"""
import glob
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = subprocess.check_output(["git", "-C", HERE, "rev-parse", "--show-toplevel"], text=True).strip()
OUT = "classic/docs/classic/rules.md"
CATALOGS = "classic/docs/classic"
RON = "classic/assets/rules/classic"
CODE_ROOTS = ["classic/crates/classic-rules/src", "platform/crates/kernel/src"]

# Files whose constants are engine plumbing, not game rules (save format, test
# scaffolding, numeric helpers); they are counted in the header and not listed.
EXCLUDED = [
    "platform/crates/kernel/src/persistence/",
    "platform/crates/kernel/src/test_module.rs",
    "platform/crates/kernel/src/math.rs",
    "platform/crates/kernel/src/module.rs",
    "classic/crates/classic-rules/src/save_codec.rs",
    "classic/crates/classic-rules/src/fixtures.rs",
]

# System of a file: the first prefix (relative to a code root) that matches.
# Order of SYSTEMS is the order of the sections.
SYSTEMS = [
    ("Turn order and game length", ["turn/", "classic/turn.rs", "classic/mod.rs", "classic/systems.rs"]),
    ("Growth and citizens", ["systems/growth.rs", "derived/citizens.rs", "world/citizens.rs"]),
    ("Production, purchase and processes", ["world/city.rs", "world/city_economy.rs", "systems/processes.rs",
                                            "command/", "world/builds.rs", "derived/grants.rs"]),
    ("Gold and maintenance", ["derived/maintenance.rs", "derived/income.rs"]),
    ("Happiness and resources", ["derived/happiness.rs", "derived/resources.rs"]),
    ("Culture, policies and borders", ["derived/culture.rs", "world/culture.rs", "world/territory.rs",
                                       "systems/borders.rs"]),
    ("Science", ["derived/research.rs", "systems/research.rs"]),
    ("Golden ages", ["systems/golden_age.rs"]),
    ("Great people", ["derived/great_people.rs", "systems/great_people.rs"]),
    ("City combat strength", ["combat/city.rs", "systems/combat/city.rs"]),
    ("Combat, experience and healing", ["combat/", "systems/combat/", "systems/plunder.rs", "systems/airlift.rs",
                                        "systems/paradrop.rs", "effects/unit.rs"]),
    ("Movement and vision", ["navigation/", "derived/visibility.rs", "world/unit.rs", "classic/unit.rs"]),
    ("Barbarians and ancient ruins", ["systems/barbarians.rs", "systems/ruins.rs", "derived/ruins.rs",
                                      "scenario/ruins.rs"]),
    ("Religion", ["derived/religion.rs", "systems/religion.rs"]),
    ("City-states", ["systems/city_states/", "world/city_states.rs", "scenario/city_states.rs"]),
    ("Diplomacy", ["systems/diplomacy/"]),
    ("Espionage", ["systems/espionage.rs", "world/espionage.rs"]),
    ("Trade routes", ["derived/trade.rs", "world/trade.rs"]),
    ("Tourism and archaeology", ["derived/tourism.rs", "derived/archaeology.rs", "systems/tourism.rs",
                                "world/tourism.rs"]),
    ("World Congress", ["systems/congress.rs", "world/congress.rs", "systems/league_projects.rs"]),
    ("Victory and score", ["systems/victory.rs", "world/victory.rs"]),
    ("Map, terrain and tile yields", ["world/map.rs", "world/size.rs", "derived/tile_yields.rs", "derived/yields.rs",
                                      "scenario/", "rules/natural_wonders.rs", "classic/yields.rs"]),
    ("Players", ["world/player.rs", "classic/player.rs"]),
    ("Content and effect model", ["classic/", "rules/", "effects/"]),
]
OTHER = "Other rule constants"
AI = "AI"

FORMULA = "own formula (expansion-only system; Unciv has no equivalent)"
DESIGN = "own design"
SOURCE_CLASSES = [
    "Unciv",
    DESIGN,
    FORMULA,
    "own value",
    "own value (unverified)",
    "own value (source not recorded)",
]

CONST = re.compile(r"^\s*pub const ([A-Z][A-Z0-9_]*)\s*:\s*(.+?)\s*=\s*(.*)$")
DOC = re.compile(r"^\s*///\s?(.*)$")
ATTR = re.compile(r"^\s*#\[")
CLAUSES = [
    ("differs", re.compile(r"\b[Dd]iffers? from Unciv\b[\s(:,]*")),
    ("Unciv", re.compile(r"\b(?:Unciv:|Source: Unciv)\s*")),
    ("design", re.compile(r"\bOwn design\b")),
    ("formula", re.compile(r"\bOwn formula \(BNW-like system")),
    ("bvalue", re.compile(r"\bOwn value \(BNW-like system")),
    ("own", re.compile(r"\b[Oo]wn values?\b")),
]
SENTENCE_END = re.compile(r"\.(?=\s|$)")


# ---------------------------------------------------------------- constants


def rel_in_root(path):
    for root in CODE_ROOTS:
        if path.startswith(root + "/"):
            return path[len(root) + 1:]
    return path


def system_of(path):
    rel = rel_in_root(path)
    if rel.startswith("ai/"):
        return AI
    for name, prefixes in SYSTEMS:
        if any(rel == p or (p.endswith("/") and rel.startswith(p)) for p in prefixes):
            return name
    return OTHER


def read_value(lines, start, first):
    """The value text of a constant from `first` (the text after `=`) up to the `;` that ends it."""
    text = first
    i = start
    depth = 0
    while True:
        for pos, ch in enumerate(text):
            if ch in "([{":
                depth += 1
            elif ch in ")]}":
                depth -= 1
            elif ch == ";" and depth <= 0:
                return text[:pos], i
        i += 1
        if i >= len(lines):
            return text, i
        depth = 0
        text = text + " " + lines[i].strip()
        # Re-scan the whole accumulated text so the depth count stays exact.


def clean_value(value):
    value = re.sub(r"\s+", " ", value.strip())
    value = re.sub(r",\s*([\]\)])", r"\1", value)
    if len(value) > 90:
        value = value[:87].rstrip() + "..."
    return value


SELF_REF = re.compile(r"\s*\((?:classic/docs/classic/)?rules\.md\)|[;,]?\s*(?:classic/docs/classic/)?rules\.md\b")


def first_sentence(text):
    # A pointer to this very file says nothing here.
    text = SELF_REF.sub("", text)
    m = SENTENCE_END.search(text)
    return text[: m.start() + 1] if m else text


def sentence_at(text, start):
    """The text from `start` to the end of its sentence (without the period)."""
    m = SENTENCE_END.search(text, start)
    return text[start: m.start()] if m else text[start:]


def classify(doc):
    """(source class, source detail) from the first source clause of a doc comment."""
    best = None
    for kind, pattern in CLAUSES:
        m = pattern.search(doc)
        if m and (best is None or m.start() < best[1].start()):
            best = (kind, m)
    if best is None:
        return "own value (source not recorded)", ""
    kind, m = best
    if kind == "differs":
        detail = sentence_at(doc, m.end()).strip().rstrip(",;)")
        if len(detail) > 100:
            detail = detail[:97].rstrip() + "..."
        return "own value", ("differs from Unciv: " + detail) if detail else "differs from Unciv"
    if kind == "design":
        return DESIGN, ""
    if kind == "formula":
        return FORMULA, ""
    if kind == "bvalue":
        return "own value", "expansion-only system; Unciv has no equivalent"
    if kind == "Unciv":
        detail = sentence_at(doc, m.end()).strip().rstrip(",;")
        if len(detail) > 100:
            detail = detail[:97].rstrip() + "..."
        return kind, detail
    clause = sentence_at(doc, m.start())
    # The sentence may start before the match ("This is an own value, unverified").
    if re.search(r"unverified|not compared|not checked", clause, re.I):
        return "own value (unverified)", ""
    return "own value", ""


def strip_markdown(text):
    return text.replace("|", "\\|")


def scan_constants():
    files = subprocess.check_output(
        ["git", "-C", ROOT, "ls-files", "--", *[r + "/*.rs" for r in CODE_ROOTS]], text=True
    ).split()
    # Uncommitted files are read too (the generator runs before a commit).
    for root in CODE_ROOTS:
        for path in glob.glob(os.path.join(ROOT, root, "**", "*.rs"), recursive=True):
            files.append(os.path.relpath(path, ROOT))
    constants = []
    excluded = 0
    for path in sorted(set(files)):
        full = os.path.join(ROOT, path)
        if not os.path.exists(full):
            continue
        with open(full, encoding="utf-8") as handle:
            lines = handle.read().split("\n")
        in_tests = False
        for i, line in enumerate(lines):
            if re.match(r"^\s*#\[cfg\(test\)\]", line) and i + 1 < len(lines) \
                    and re.match(r"^\s*(pub\s+)?mod\s", lines[i + 1]):
                in_tests = True
            m = CONST.match(line)
            if not m or in_tests:
                continue
            if any(path.startswith(e) for e in EXCLUDED):
                excluded += 1
                continue
            name, ty, rest = m.groups()
            value, _ = read_value(lines, i, rest)
            doc_lines = []
            j = i - 1
            while j >= 0 and (DOC.match(lines[j]) or ATTR.match(lines[j])):
                d = DOC.match(lines[j])
                if d:
                    doc_lines.append(d.group(1).strip())
                j -= 1
            doc = " ".join(reversed(doc_lines)).strip()
            doc = re.sub(r"\s+", " ", doc)
            meaning = first_sentence(doc) if doc else ""
            if (not doc and i > 0 and CONST.match(lines[i - 1]) and constants and constants[-1]["path"] == path
                    and re.search(r"\b[Oo]wn values\b", constants[-1]["doc"])):
                # A run of constants without blank lines under a comment that says "own values" (plural)
                # shares that comment. Other sources are never inherited: a neighbour is not
                # necessarily covered by its predecessor's citation.
                head = constants[-1]
                doc = head["doc"]
                meaning = "(with `%s`) %s" % (head["group"], head["head_meaning"]) if head["head_meaning"] else ""
                group = head.get("group", head["name"])
            else:
                group = name
            source, detail = classify(doc)
            constants.append({
                "name": name,
                "type": ty.strip(),
                "value": clean_value(value),
                "path": path,
                "line": i + 1,
                "system": system_of(path),
                "meaning": meaning,
                "doc": doc,
                "group": group,
                "head_meaning": first_sentence(doc) if doc else "",
                "source": source,
                "detail": detail,
            })
    return constants, excluded


# ---------------------------------------------------------------- catalogs


def load(name):
    with open(os.path.join(ROOT, CATALOGS, name), encoding="utf-8") as handle:
        return json.load(handle)


def provenance_class(text):
    t = (text or "").strip().lower()
    if t.startswith("unciv"):
        return "Unciv"
    if t.startswith("own value") and ("unverified" in t or "not compared" in t):
        return "own (unverified)"
    if t.startswith("own"):
        return "own"
    return "not recorded"


PROV_ORDER = ["Unciv", "own", "own (unverified)", "not recorded"]


def prov_counts(items):
    counts = {}
    for item in items:
        k = provenance_class(item.get("provenance"))
        counts[k] = counts.get(k, 0) + 1
    return counts


def prov_short(item):
    return {"Unciv": "Unciv", "own": "own",
            "own (unverified)": "own, unverified", "not recorded": "not recorded"}[
        provenance_class(item.get("provenance"))]


def short_id(ident):
    if not ident:
        return ""
    return ident.split("/")[-1]


def names_by_id(*catalogs):
    result = {}
    for items in catalogs:
        for item in items:
            result[item["id"]] = item.get("name", short_id(item["id"]))
    return result


def yields_text(yields):
    if not yields:
        return ""
    return ", ".join("%s %s" % (k, v) for k, v in sorted(yields.items()))


def table(header, rows):
    out = ["| " + " | ".join(header) + " |", "|" + "|".join("---" for _ in header) + "|"]
    for row in rows:
        out.append("| " + " | ".join(strip_markdown(str(c)) if c is not None else "" for c in row) + " |")
    return out


def parse_ron_records(path, list_key):
    """Top-level records of a RON list as dicts of their scalar `key: value` fields."""
    with open(os.path.join(ROOT, path), encoding="utf-8") as handle:
        text = "\n".join(l for l in handle.read().split("\n") if not l.strip().startswith("//"))
    start = text.index(list_key + ":")
    start = text.index("[", start) + 1
    records = []
    depth = 0
    current = None
    i = start
    while i < len(text):
        ch = text[i]
        if ch == "(":
            depth += 1
            if depth == 1:
                current = i + 1
        elif ch == ")":
            if depth == 1:
                body = text[current:i]
                record = {}
                # Only fields at the record's own level: drop nested groups.
                flat = body
                while True:
                    reduced = re.sub(r"\([^()]*\)|\[[^\[\]]*\]", "", flat)
                    if reduced == flat:
                        break
                    flat = reduced
                for m in re.finditer(r"(\w+)\s*:\s*(\"[^\"]*\"|-?[\w.]+)", flat):
                    v = m.group(2)
                    record[m.group(1)] = v[1:-1] if v.startswith('"') else v
                for m in re.finditer(r"(\w+)\s*:\s*Some\(\s*(\"[^\"]*\"|-?[\w.]+)\s*\)", body):
                    v = m.group(2)
                    record.setdefault(m.group(1), v[1:-1] if v.startswith('"') else v)
                records.append(record)
            depth -= 1
        elif ch == "]" and depth == 0:
            break
        i += 1
    return records


# ---------------------------------------------------------------- formulas


class Lookup:
    def __init__(self, constants):
        self.values = {}
        for c in constants:
            self.values.setdefault(c["name"], c)
        self.missing = []

    def __call__(self, name):
        c = self.values.get(name)
        if c is None:
            self.missing.append(name)
            return "[missing constant `%s`]" % name
        return "%s (`%s`)" % (c["value"], name)


def formulas(v):
    """The main formulas in our own words; every number comes from a constant."""
    return [
        "## Formulas",
        "",
        "Short descriptions in our own words of the main formulas; the code is the authority "
        "(the function named in each item). Every number below is the value of the named constant.",
        "",
        "- **Growth** (`systems::growth::growth_threshold`). A city of size n needs %s food plus %s for "
        "every citizen beyond the first, plus the integer square root of the cube of (n - 1). "
        "An AI player's threshold is then scaled by its difficulty level's growth percent."
        % (v("FOOD_TO_NEXT_POPULATION_BASE"), v("FOOD_TO_NEXT_POPULATION_PER_POP")),
        "- **Technology cost** (`derived::research::tech_cost`). The technology's catalog cost is scaled by "
        "the difficulty level's research percent and the map size's research percent, multiplied by "
        "(1 + the map size's per-city percent times (cities - 1)), counting the most cities the player ever "
        "had, and divided by (1 + %s percent times the share of living civilizations that the player has met "
        "and that know the technology); the result is truncated." % v("KNOWN_DISCOUNT_PERCENT"),
        "- **Policy cost** (`derived::culture::policy_cost`). %s culture plus (%s times the policies already "
        "paid) raised to the power %s; the cost modifiers (never more than %s percent off) and the difficulty "
        "percent apply, the result is multiplied by (1 + the map size's per-city percent times (cities - 1)), "
        "rounded to the nearest whole and then down to a multiple of %s."
        % (v("POLICY_CULTURE_COST_BASE"), v("POLICY_CULTURE_COST_PER_POLICY"), v("POLICY_CULTURE_COST_EXPONENT"),
           v("POLICY_COST_MAX_DISCOUNT_PERCENT").replace("-", ""), v("POLICY_CULTURE_COST_ROUND_TO")),
        "- **Border growth by culture** (`derived::culture::border_cost`, table `kernel::world::city::CULTURE_COSTS`). "
        "A city that has claimed t tiles beyond its first ring needs %s times (t + %s) raised to the power %s "
        "culture for its next tile, rounded to the nearest whole. Modifiers can lower "
        "the cost by at most %s percent."
        % (v("CULTURE_TO_NEXT_TILE_FACTOR"), v("CULTURE_TO_NEXT_TILE_OFFSET"),
           v("CULTURE_TO_NEXT_TILE_EXPONENT"), v("CULTURE_TO_NEXT_TILE_MAX_DISCOUNT_PERCENT").replace("-", "")),
        "- **Tile purchase** (`kernel::world::territory::tile_purchase_offer`). A tile at distance d (at most %s) "
        "from the city costs %s gold times (d - 1) plus %s gold for every tile the city has claimed beyond its "
        "first ring (by culture or purchase); the city's tile purchase modifiers then apply, and a tile is never free."
        % (v("PURCHASE_RADIUS_MAX"), v("PLOT_BASE_COST"), v("PLOT_COST_PER_PLOT_BOUGHT")),
        "- **Item gold price** (`kernel::world::city`). The gold price of a unit or building grows with "
        "(%s times its production cost) raised to the power 0.75, rounded down to a multiple of %s; a partly "
        "built item costs less." % (v("GOLD_PER_PRODUCTION"), v("PURCHASE_DIVISOR")),
        "- **Combat damage** (`combat::damage`). The stronger side's strength divided by the weaker's gives "
        "a ratio r; the damage factor is the mean of ((r + 3) / 4) to the fourth power and 1 (its inverse for "
        "the weaker side). A unit takes that factor times a base of %s hundredths of a hit point plus a "
        "random part up to %s, out of %s hit points, rounded to the nearest hit point with no minimum. A "
        "wounded unit's damage drops by a percentage equal to %s percent of its missing hit points (cities "
        "never weaken). A civilian that is attacked loses %s hit points."
        % (v("MELEE_EQUAL_STRENGTH_BASE_DAMAGE"), v("MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE"),
           v("UNIT_MAX_HP"), v("WOUNDED_DAMAGE_PERCENT"), v("DAMAGE_TO_CIVILIAN_UNIT")),
        "- **Healing** (`systems::combat::heal`). A unit that rests heals %s hit points in a city, %s on its "
        "own or a friendly player's land, %s on neutral land and %s on enemy land, plus the bonuses of its "
        "promotions and of nearby units."
        % (v("HEALING_IN_CITY_CENTER"), v("HEALING_FRIENDLY_TERRITORY"), v("HEALING_NEUTRAL_TERRITORY"),
           v("HEALING_ENEMY_TERRITORY")),
        "- **Unit upkeep** (`derived::maintenance::unit_maintenance`). Every player keeps %s units free, plus "
        "the difficulty's, the policies' and the garrison free units. With n paid units and p the turn over "
        "%s (at most 1), the gold per turn is (%s hundredths times n times (1 + %s p)) raised to the power "
        "(1 + p / %s), then the difficulty's unit cost percent applies."
        % (v("FREE_UNITS"), v("ESTIMATED_END_TURN"), v("GOLD_PER_UNIT_TIMES100"), v("UNIT_COST_GROWTH_FACTOR"),
           v("UNIT_COST_EXPONENT_DIVISOR")),
        "- **Golden age** (`systems::golden_age::golden_age_threshold`). The first golden age needs %s "
        "happiness, every later one %s more; the threshold grows by %s percent per city, truncated. A "
        "golden age lasts %s turns before modifiers."
        % (v("GOLDEN_AGE_BASE_THRESHOLD"), v("GOLDEN_AGE_THRESHOLD_INCREASE"),
           v("GOLDEN_AGE_THRESHOLD_CITY_PERCENT"), v("UNMODIFIED_GOLDEN_AGE_TURNS")),
        "",
    ]


# ---------------------------------------------------------------- document


def slug(text):
    text = re.sub(r"[`*_]", "", text.strip().lower())
    text = re.sub(r"[^\w\s-]", "", text)
    return re.sub(r"\s", "-", text.strip())


def source_cell(c):
    if c["source"] == "own value" and c["detail"]:
        return "own value (%s)" % c["detail"]
    if c["source"] == "Unciv":
        return "%s: %s" % (c["source"], c["detail"]) if c["detail"] else c["source"]
    return c["source"]


def constants_section(constants, systems):
    out = []
    for name in systems:
        group = [c for c in constants if c["system"] == name]
        if not group:
            continue
        counts = {}
        for c in group:
            counts[c["source"]] = counts.get(c["source"], 0) + 1
        summary = ", ".join("%s %d" % (k, counts[k]) for k in SOURCE_CLASSES if k in counts)
        files = sorted({c["path"] for c in group})
        out += ["### " + name, "", "%d constants (%s). Files: %s." % (
            len(group), summary, ", ".join("`%s`" % f for f in files)), ""]
        rows = []
        for c in sorted(group, key=lambda c: (c["path"], c["line"])):
            rows.append(("`%s`" % c["name"], "`%s`" % c["value"], c["meaning"], source_cell(c),
                         "`%s`" % os.path.basename(c["path"])))
        out += table(["Name", "Value", "Meaning", "Source", "File"], rows)
        out.append("")
    return out


def catalog_section():
    out = ["## Data catalogs", "",
           "Generated from the JSON catalogs in `classic/docs/classic/` (the input of the content importer) "
           "and, for difficulty levels and projects, from the compiled files in `classic/assets/rules/classic/`. "
           "The provenance column counts what each entry's `provenance` field says.", ""]
    catalogs = [
        ("Units", "units.json"), ("Unique units", "unique_units.json"), ("Buildings", "buildings.json"),
        ("Unique buildings", "unique_buildings.json"), ("Wonders", "wonders.json"), ("Technologies", "techs.json"),
        ("Policies", "policies.json"), ("Beliefs", "beliefs.json"), ("Promotions", "promotions.json"),
        ("Resources", "resources.json"), ("Improvements", "improvements.json"), ("Terrain", "terrain.json"),
        ("Terrain features", "features.json"), ("Natural wonders", "natural_wonders.json"), ("Eras", "eras.json"),
        ("Great people", "great_people.json"), ("Civilizations", "civilizations.json"),
        ("City-states", "city_states.json"),
    ]
    data = {f: load(f) for _, f in catalogs}
    rows = []
    for title, f in catalogs:
        items = data[f]["items"]
        c = prov_counts(items)
        rows.append((title, "`%s`" % f, len(items)) + tuple(c.get(k, 0) for k in PROV_ORDER))
    for title, f, key in [("Policy branches", "policies.json", "branches"), ("Religions", "beliefs.json", "religions"),
                          ("Specialists", "great_people.json", "specialists"),
                          ("City-state types", "city_states.json", "types")]:
        items = data[f][key]
        c = prov_counts(items)
        rows.append((title, "`%s` (%s)" % (f, key), len(items)) + tuple(c.get(k, 0) for k in PROV_ORDER))
    out += ["### Overview", ""]
    out += table(["Catalog", "File", "Entries", "Unciv", "own", "own, unverified", "not recorded"], rows)
    out.append("")

    names = names_by_id(*(data[f]["items"] for _, f in catalogs), data["policies.json"]["branches"])

    def unit_rows(items):
        return [(u["name"], u.get("role", ""), short_id(u.get("era")), u.get("combat") or "",
                 u.get("ranged_combat") or "", u.get("range") or "", u.get("moves"), u.get("cost"),
                 names.get(u.get("required_tech"), ""), prov_short(u)) for u in items]

    for title, f in [("Units", "units.json"), ("Unique units", "unique_units.json")]:
        out += ["### " + title, ""]
        out += table(["Unit", "Role", "Era", "Strength", "Ranged", "Range", "Moves", "Cost", "Technology",
                      "Provenance"], unit_rows(data[f]["items"]))
        out.append("")
    for title, f in [("Buildings", "buildings.json"), ("Unique buildings", "unique_buildings.json"),
                     ("Wonders", "wonders.json")]:
        out += ["### " + title, ""]
        rows = [(b["name"], b.get("kind", ""), short_id(b.get("era")), b.get("cost"), b.get("maintenance"),
                 names.get(b.get("required_tech"), ""), prov_short(b)) for b in data[f]["items"]]
        out += table(["Building", "Kind", "Era", "Cost", "Upkeep", "Technology", "Provenance"], rows)
        out.append("")
    out += ["### Technologies", ""]
    rows = [(t["name"], short_id(t.get("era")), t.get("cost"),
             ", ".join(names.get(p, short_id(p)) for p in t.get("prerequisites", [])), prov_short(t))
            for t in data["techs.json"]["items"]]
    out += table(["Technology", "Era", "Cost", "Requires", "Provenance"], rows)
    out.append("")
    out += ["### Policy branches", ""]
    branches = data["policies.json"]["branches"]
    per_branch = {}
    for p in data["policies.json"]["items"]:
        per_branch[p.get("branch")] = per_branch.get(p.get("branch"), 0) + 1
    rows = [(b["name"], short_id(b.get("era_prerequisite")) or "", per_branch.get(b["id"], 0), prov_short(b))
            for b in branches]
    out += table(["Branch", "Era required", "Policies (opener and finisher included)", "Provenance"], rows)
    out.append("")
    out += ["### Beliefs", ""]
    cats = {}
    for b in data["beliefs.json"]["items"]:
        key = b.get("category", "")
        cats.setdefault(key, {})
        k = provenance_class(b.get("provenance"))
        cats[key][k] = cats[key].get(k, 0) + 1
    rows = [(k, sum(v.values())) + tuple(v.get(p, 0) for p in PROV_ORDER) for k, v in sorted(cats.items())]
    out += table(["Category", "Beliefs", "Unciv", "own", "own, unverified", "not recorded"], rows)
    out.append("")
    out += ["### Resources", ""]
    rows = [(r["name"], r.get("class", ""), yields_text(r.get("yields")), r.get("happiness") or "",
             names.get(r.get("revealed_by_tech"), ""), prov_short(r)) for r in data["resources.json"]["items"]]
    out += table(["Resource", "Class", "Yields", "Happiness", "Revealed by", "Provenance"], rows)
    out.append("")
    out += ["### Improvements", ""]
    rows = [(i["name"], yields_text(i.get("yields")), names.get((i.get("build") or {}).get("required_tech")
                                                                 if isinstance(i.get("build"), dict) else None, ""),
             prov_short(i)) for i in data["improvements.json"]["items"]]
    out += table(["Improvement", "Yields", "Technology", "Provenance"], rows)
    out.append("")
    for title, f in [("Terrain", "terrain.json"), ("Terrain features", "features.json"),
                     ("Natural wonders", "natural_wonders.json")]:
        out += ["### " + title, ""]
        rows = [(t["name"], yields_text(t.get("yields")), t.get("movement_cost"), t.get("defense_pct"),
                 "yes" if t.get("impassable") else "", prov_short(t)) for t in data[f]["items"]]
        out += table(["Name", "Yields", "Movement", "Defence %", "Impassable", "Provenance"], rows)
        out.append("")
    out += ["### Eras", ""]
    rows = [(e["name"], e.get("index"), e.get("research_agreement_cost"), e.get("embarked_unit_defense"),
             e.get("starting_gold"), e.get("research_percent"), prov_short(e)) for e in data["eras.json"]["items"]]
    out += table(["Era", "Index", "Research agreement cost", "Embarked defence", "Starting gold", "Research %",
                  "Provenance"], rows)
    out.append("")
    out += ["### Great people", ""]
    gp = data["great_people.json"]
    rows = [(g["name"], names.get(g.get("specialist"), short_id(g.get("specialist"))) or "",
             g.get("points_source", ""), prov_short(g)) for g in gp["items"]]
    out += table(["Great person", "Specialist", "Points from", "Provenance"], rows)
    out.append("")
    spec_names = {s["id"]: s["name"].split("|")[0] for s in gp["specialists"]}
    rows = [(spec_names[s["id"]], yields_text(s.get("yields")), s.get("great_person_points_per_turn"), prov_short(s))
            for s in gp["specialists"]]
    out += table(["Specialist", "Yields", "Great person points", "Provenance"], rows)
    out.append("")
    if gp.get("thresholds"):
        out += ["Great person thresholds: `%s`." % json.dumps(gp["thresholds"], sort_keys=True), ""]
    out += ["### Civilizations", ""]
    rows = [(c["name"], (c.get("leader") or {}).get("name", ""),
             ", ".join(a.get("name", "") for a in c.get("ability", [])), prov_short(c))
            for c in data["civilizations.json"]["items"]]
    out += table(["Civilization", "Leader", "Ability", "Provenance"], rows)
    out.append("")
    out += ["### City-states", ""]
    cs = data["city_states.json"]
    per_type = {}
    for c in cs["items"]:
        per_type[c.get("type")] = per_type.get(c.get("type"), 0) + 1
    rows = [(t["name"], per_type.get(t["id"], 0), (t.get("thresholds") or {}).get("friends", ""),
             (t.get("thresholds") or {}).get("allies", ""), prov_short(t)) for t in cs["types"]]
    out += table(["Type", "City-states", "Friends at", "Allies at", "Provenance"], rows)
    out.append("")
    out += ["### Promotions", "", "%d promotions; provenance in the overview table." %
            len(data["promotions.json"]["items"]), ""]

    out += ["### Difficulty levels", "",
            "From `%s/handicaps.ron` (fields Unciv carries come from its difficulty file; the file header names "
            "the rest)." % RON, ""]
    levels = parse_ron_records(RON + "/handicaps.ron", "handicaps")
    fields = [("happiness_default", "Base happiness"), ("num_cities_unhappiness_mod", "City unhappiness %"),
              ("population_unhappiness_mod", "Citizen unhappiness %"), ("unit_cost_percent", "Unit cost %"),
              ("building_cost_percent", "Building cost %"), ("research_percent", "Research %"),
              ("policy_percent", "Policy %"), ("ai_growth_percent", "AI growth %"),
              ("ai_train_percent", "AI train %"), ("ai_construct_percent", "AI construct %")]
    rows = [(lvl.get("name", ""),) + tuple(lvl.get(k, "") for k, _ in fields) for lvl in levels]
    out += table(["Level"] + [t for _, t in fields], rows)
    out.append("")
    out += ["### Projects", "", "From `%s/projects.ron`." % RON, ""]
    projects = parse_ron_records(RON + "/projects.ron", "projects")
    rows = [(p.get("name", ""), p.get("kind", ""), p.get("cost", ""), names.get(p.get("required_tech"), ""))
            for p in projects]
    out += table(["Project", "Kind", "Cost", "Technology"], rows)
    out.append("")
    return out


def render():
    constants, excluded = scan_constants()
    lookup = Lookup(constants)
    rules = [c for c in constants if c["system"] != AI]
    ai = [c for c in constants if c["system"] == AI]
    order = [name for name, _ in SYSTEMS] + [OTHER]

    def counts(group):
        result = {k: 0 for k in SOURCE_CLASSES}
        for c in group:
            result[c["source"]] += 1
        return result

    rc, ac = counts(rules), counts(ai)
    out = [
        "# Game rules of the classic set",
        "",
        "> **Generated file, do not edit.** Made by `classic/tools/gen-rules-doc.py` from the project's own "
        "data: the rule constants of the Rust source (`classic/crates/classic-rules/src`, "
        "`platform/crates/kernel/src`) with their doc comments, and the JSON catalogs in `classic/docs/classic/`. "
        "Regenerate with `python3 -I classic/tools/gen-rules-doc.py`; `--check` fails when this file is stale "
        "(`tools/check-all.sh` runs it).",
        ">",
        "> **Sources.** The classic set is a Civ V-style rule set. Its rules and numbers may come only from "
        "(1) Unciv (open source, MPL-2.0; cited as `Unciv: <file>`) or (2) our own design "
        "(`own value`; `own design` for the systems Unciv lacks (BNW-DESIGN.md); "
        "`own value (unverified)` where it was not compared with a public source). See "
        "[SOURCES.md](SOURCES.md) and [UNCIV-ATTRIBUTION.md](UNCIV-ATTRIBUTION.md). The source column of every "
        "constant below is read from the constant's own doc comment; a comment without a recognised source "
        "clause is listed as `own value (source not recorded)`.",
        ">",
        "> **Formulas** are described in short sentences in our own words, with the numbers taken from the "
        "constants; the code is the authority. This file holds no pseudo-code or text of the original game.",
        "",
        "## Summary",
        "",
        "Rule constants: %d (AI constants: %d; %d constants of engine plumbing in %s are not listed)."
        % (len(rules), len(ai), excluded, ", ".join("`%s`" % e for e in EXCLUDED)),
        "",
    ]
    out += table(["Source", "Rule constants", "AI constants"],
                 [(k, rc[k], ac[k]) for k in SOURCE_CLASSES] + [("total", len(rules), len(ai))])
    out += ["", "Sections:", ""]
    out.append("- [Formulas](#formulas)")
    out.append("- [Rule constants by system](#rule-constants-by-system)")
    for name in order:
        if any(c["system"] == name for c in rules):
            out.append("  - [%s](#%s)" % (name, slug(name)))
    out.append("- [AI constants](#ai-constants)")
    out.append("- [Data catalogs](#data-catalogs)")
    out.append("")
    out += formulas(lookup)
    out += ["## Rule constants by system", "",
            "Every `pub const` of the rule code, by system. Value as written in the code; meaning is the first "
            "sentence of the doc comment; the source is the doc comment's first source clause.", ""]
    out += constants_section(rules, order)
    out += ["## AI constants", "",
            "Tuning numbers of the computer players (`classic/crates/classic-rules/src/ai`). They are heuristics "
            "of our own: generic, derived from rule data, no cheats.", ""]
    for c in ai:
        c["system"] = "AI: `%s`" % os.path.basename(c["path"])
    ai_order = sorted({c["system"] for c in ai})
    out += [l.replace("### AI: ", "### ") for l in constants_section(ai, ai_order)]
    out += catalog_section()
    text = "\n".join(out).rstrip("\n") + "\n"
    return text, lookup.missing, rc, ac


def main(argv):
    text, missing, rc, ac = render()
    for name in sorted(set(missing)):
        print("gen-rules-doc: formula names a missing constant: %s" % name, file=sys.stderr)
    path = os.path.join(ROOT, OUT)
    if "--stdout" in argv:
        sys.stdout.write(text)
        return 0
    if "--check" in argv:
        try:
            with open(path, encoding="utf-8") as handle:
                current = handle.read()
        except FileNotFoundError:
            current = ""
        if current != text or missing:
            print("gen-rules-doc: %s is stale; run python3 -I classic/tools/gen-rules-doc.py" % OUT, file=sys.stderr)
            return 1
        print("gen-rules-doc: ok (%s up to date)" % OUT)
        return 0
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)
    print("gen-rules-doc: wrote %s (%d lines); rule constants %s; AI constants %s" % (
        OUT, text.count("\n"), ", ".join("%s %d" % (k, rc[k]) for k in SOURCE_CLASSES),
        ", ".join("%s %d" % (k, ac[k]) for k in SOURCE_CLASSES)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
