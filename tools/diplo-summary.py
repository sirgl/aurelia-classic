#!/usr/bin/env python3
"""Summary of the probe of lane A (AURELIA_DIPLO_PROBE, crates/qa/src/bench/diplo_probe.rs) by variant:
the gates S1, S2 and C1-C4 of classic/docs/ai-strategist-defects.md. A mirror run has one variant on both
sides; its seats are pooled. Usage: diplo-summary.py FILE.jsonl..."""
import json
import sys

rows = []
for path in sys.argv[1:]:
    rows += [json.loads(line) for line in open(path) if line.strip()]

print("| variant | seats | C1 cs wars / seat | C2 cs cities / seat | tributes / seat | C4 quests done / given | C3 allies end | allies r132 | friends end | parts seats | S1 lacking | S2 blocked rounds / parts seat |")
print("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
for variant in sorted({r["variant"] for r in rows}):
    seats = [r for r in rows if r["variant"] == variant]
    n = len(seats)
    mean = lambda key: sum(r[key] or 0 for r in seats) / n
    given = sum(r["quests_given"] for r in seats)
    done = sum(r["quests_done"] for r in seats)
    mid = [r["allies_132"] for r in seats if r["allies_132"] is not None]
    parts = [r for r in seats if r["parts_round"] is not None]
    lacking = sum(1 for r in parts if r["parts_lack"])
    blocked = sum(r["blocked"] for r in parts)
    share = f"{lacking / len(parts):.1%} ({lacking})" if parts else "-"
    per = f"{blocked / len(parts):.2f}" if parts else "-"
    quests = f"{done / given:.1%} ({done}/{given})" if given else "-"
    mid_mean = f"{sum(mid) / len(mid):.2f}" if mid else "-"
    print(f"| {variant} | {n} | {mean('cs_wars'):.3f} | {mean('cs_cities'):.3f} | {mean('tributes'):.2f} | {quests} | {mean('allies'):.2f} | {mid_mean} | {mean('friends'):.2f} | {len(parts)} | {share} | {per} |")
