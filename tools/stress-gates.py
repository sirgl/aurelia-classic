#!/usr/bin/env python3
"""Gates T1-T5 of classic/docs/ai-strategist-defects.md: the exam rows of `standard` and of the candidate
(ai-stress run with STRESS_VARIANT), means over the three levels, on the instances of one parity.
Usage: stress-gates.py STANDARD_ROWS CANDIDATE_ROWS odd|even|all"""
import json, sys
def load(path, parity):
    rows = [json.loads(l) for l in open(path) if l.strip()]
    keep = {"odd": 1, "even": 0}.get(parity)
    return [r for r in rows if keep is None or r["instance"] % 2 == keep]
def mean(rows, situation, metric, delta):
    rs = [r for r in rows if r["situation"] == situation]
    if not rs: return float("nan"), 0
    v = [r["edited"].get(metric, 0) - (r["control"].get(metric, 0) if delta else 0) for r in rs]
    return sum(v) / len(v), len(rs)
std, cand = load(sys.argv[1], sys.argv[3]), load(sys.argv[2], sys.argv[3])
gates = [("T1", "b9", "rival_wins_science", False, lambda s, c: c <= s - 0.10, "cand <= std - 0.10"),
         ("T2", "b9", "rival_cities_lost", True, lambda s, c: c >= s + 0.30, "cand >= std + 0.30"),
         ("T3", "a1", "wars_on_leader", True, lambda s, c: c >= s + 0.20, "cand >= std + 0.20"),
         ("T4", "a1", "denouncements_of_leader", True, lambda s, c: c >= s + 0.50, "cand >= std + 0.50"),
         ("T5a", "a1", "end_seat_win_weight", True, lambda s, c: c >= s - 0.03, "cand >= std - 0.03"),
         ("T5b", "b9", "end_seat_win_weight", True, lambda s, c: c >= s - 0.03, "cand >= std - 0.03")]
print("| gate | situation | metric | standard | candidate | rows | test | pass |")
print("|---|---|---|---:|---:|---:|---|---|")
for g, sit, m, d, ok, text in gates:
    s, n = mean(std, sit, m, d); c, _ = mean(cand, sit, m, d)
    print(f"| {g} | {sit} | {m}{' (delta)' if d else ''} | {s:.3f} | {c:.3f} | {n} | {text} | {'yes' if ok(s, c) else 'no'} |")
