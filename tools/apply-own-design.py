#!/usr/bin/env python3
"""Apply the own-design tables (classic/tools/own-design.json) to the classic catalogs.

Unciv has no rule set for the systems that exist only in the expansion (trade routes, tourism, great works,
ideologies, the World Congress, archaeology). Their rows in the catalogs are our own design; the tables in
`own-design.json` are the one place where those rows are written down, per system, with the reason. This script
applies them to `classic/docs/classic/*.json` (idempotent: running it twice changes nothing) and marks each changed
or added typed effect `source: "own design: <system>"`. `classic-import` then compiles the catalogs to the RON files.

Operations (each names an entry by catalog and display name):
  set     {modifier, [nth], [match], amount}         change the amount of the nth effect with this modifier (all if nth is absent)
  remove  {modifier, [match]}                        delete every effect with this modifier
  `match` is an optional dict compared with the selector filter of the effect (e.g. {"building": "core:building/hospital"}).
  add     {effect}                                   append a typed effect (skipped when an equal one is there)
  field   {field, value}                             set a field of the entry itself
  replace {effects}                                  replace the whole effect list of the entry
  ability {key, value}                               set one key of the entry's `abilities` table (units)
  tier    {tier, modifier, amount}                   change the amount of a reward effect of a World Congress project tier

Usage: python3 -I classic/tools/apply-own-design.py [--check]
  --check   only report what would change (exit 1 when something would)
"""
import copy
import json
import os
import sys

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
CATALOGS = os.path.join(ROOT, "classic", "docs", "classic")
DESIGN = os.path.join(ROOT, "classic", "tools", "own-design.json")


def load(cat):
    with open(os.path.join(CATALOGS, cat + ".json"), encoding="utf-8") as fh:
        return json.load(fh)


def save(cat, data):
    with open(os.path.join(CATALOGS, cat + ".json"), "w", encoding="utf-8") as fh:
        fh.write(json.dumps(data, indent=1, ensure_ascii=False) + "\n")


def find(data, name):
    for key in ("items", "branches"):
        for it in data.get(key, []):
            if it.get("name") == name or it.get("id") == name:
                return it
    raise SystemExit(f"entry not found: {name}")


def main():
    check = "--check" in sys.argv
    with open(DESIGN, encoding="utf-8") as fh:
        design = json.load(fh)
    cats = {}
    changed = []
    for system, body in design["systems"].items():
        tag = f"own design: {system}"
        for op in body["ops"]:
            cat = op["catalog"]
            data = cats.setdefault(cat, load(cat))
            entry = find(data, op["entry"])
            effects = entry.setdefault("effects", [])
            kind = op["op"]
            before = copy.deepcopy(entry)
            def matches(e):
                want = op.get("match")
                return all(e["selector"].get("filter", {}).get(k) == v for k, v in want.items()) if want else True

            if kind == "set":
                hits = [e for e in effects if e["modifier"] == op["modifier"] and matches(e)]
                if "nth" in op:
                    hits = hits[op["nth"] : op["nth"] + 1]
                if not hits:
                    raise SystemExit(f"{op['entry']}: no effect {op['modifier']}")
                for e in hits:
                    e["amount"] = op["amount"]
                    e["source"] = tag
            elif kind == "remove":
                entry["effects"] = [e for e in effects if not (e["modifier"] == op["modifier"] and matches(e))]
            elif kind == "add":
                new = dict(op["effect"])
                new["source"] = tag
                if not any({**e, "source": ""} == {**new, "source": ""} for e in effects):
                    effects.append(new)
            elif kind == "replace":
                entry["effects"] = [dict(e, source=tag) for e in op["effects"]]
            elif kind == "tier":
                hits = [
                    e
                    for t in entry["reward_tiers"]
                    if t["tier"] == op["tier"]
                    for e in t["effects"]
                    if e["modifier"] == op["modifier"]
                ]
                if not hits:
                    raise SystemExit(f"{op['entry']}: no tier effect {op['modifier']}")
                for e in hits:
                    e["amount"] = op["amount"]
                    e["source"] = tag
            elif kind == "ability":
                entry.setdefault("abilities", {})[op["key"]] = op["value"]
            elif kind == "field":
                entry[op["field"]] = op["value"]
            else:
                raise SystemExit(f"unknown op {kind}")
            if entry != before:
                changed.append(f"{system}: {cat}/{op['entry']} {kind} {op.get('modifier', op.get('field', ''))}")
    if check:
        for line in changed:
            print(line)
        sys.exit(1 if changed else 0)
    for cat, data in cats.items():
        save(cat, data)
    print(f"applied: {len(changed)} changes in {len(cats)} catalogs")


if __name__ == "__main__":
    main()
