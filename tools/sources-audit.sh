#!/bin/sh
# Counts the records of rules.md and the provenance of the content catalogs (classic/docs/classic/SOURCING-POLICY.md).
#   sh classic/tools/sources-audit.sh            prints the counts
#   sh classic/tools/sources-audit.sh --strict   exits 1 while any row has no recorded source (the release gate)
set -eu
cd "$(dirname "$0")/.."
python3 -I - "${1:-}" <<'PY'
import json, glob, os, re, sys
strict = sys.argv[1] == "--strict"
rows = [l for l in open("docs/classic/rules.md", encoding="utf-8") if l.startswith("| `")]
cls = {"Unciv": 0, "own design": 0, "common knowledge": 0, "own value, source not recorded": 0, "own value, unverified": 0, "other": 0}
for l in rows:
    cells = [c.strip() for c in l.split("|")]
    src = cells[4] if len(cells) > 4 else ""
    if src.startswith("Unciv"): cls["Unciv"] += 1
    elif src.startswith("own design"): cls["own design"] += 1
    elif src.startswith("common knowledge"): cls["common knowledge"] += 1
    elif src.startswith("own value") and "not recorded" in src: cls["own value, source not recorded"] += 1
    elif src.startswith("own value"): cls["own value, unverified"] += 1
    else: cls["other"] += 1
print(f"rules.md: {len(rows)} constants")
for k, v in cls.items(): print(f"  {v:5}  {k}")
missing = {}
for f in sorted(glob.glob("docs/classic/*.json")):
    try: d = json.load(open(f, encoding="utf-8"))
    except Exception: continue
    items = d.get("items") if isinstance(d, dict) else None
    if not isinstance(items, list) or not items or not isinstance(items[0], dict): continue
    def ok(t):
        v = str(t.get("provenance", "")).strip()
        return v.startswith(("unciv", "own design", "common knowledge"))
    n = len(items); no = sum(1 for t in items if not ok(t))
    missing[os.path.basename(f)] = (n, no)
print("catalogs: items whose provenance is not unciv / own design / common knowledge")
for f, (n, no) in missing.items(): print(f"  {no:4} of {n:4}  {f}")
bad = cls["own value, source not recorded"] + cls["own value, unverified"] + sum(no for _, no in missing.values())
print(f"unresolved: {bad}")
if strict and bad: sys.exit(1)
PY
