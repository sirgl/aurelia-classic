#!/usr/bin/env python3
"""Victory-weighted share from ai-tournament outputs: a real victory counts 1, a time (score) victory TIME_WEIGHT.
Usage: victory-share.py FILE...   (head-to-head outputs with the "by kind" table)"""
import re, sys
TIME_WEIGHT = 0.25
def kinds(cell):
    return {k: int(n) for k, n in re.findall(r"([a-z]+) (\d+)", cell)}
print("| file | games | real wins cand / base | time wins cand / base | plain share | real-victory share | weighted share |")
print("|---|---:|---|---|---:|---:|---:|")
for path in sys.argv[1:]:
    rows = {}
    for line in open(path):
        m = re.match(r"\| (Candidate|Baseline) \| ([\w-]+) \| (\d+) \| (\d+) \| ([^|]*) \|", line)
        if m:
            side = m.group(1)
            k = kinds(m.group(5))
            r = rows.setdefault(side, {"name": m.group(2), "real": 0, "time": 0})
            r["time"] += k.pop("time", 0)
            r["real"] += sum(k.values())
    if len(rows) < 2:
        print(f"| {path} | no table | | | | | |"); continue
    c, b = rows["Candidate"], rows["Baseline"]
    games = c["real"] + c["time"] + b["real"] + b["time"]
    plain = (c["real"] + c["time"]) / games
    real = c["real"] / (c["real"] + b["real"]) if c["real"] + b["real"] else float("nan")
    wc, wb = c["real"] + TIME_WEIGHT * c["time"], b["real"] + TIME_WEIGHT * b["time"]
    print(f"| {path} ({c['name']}) | {games} | {c['real']} / {b['real']} | {c['time']} / {b['time']} | {plain:.1%} | {real:.1%} | {wc / (wc + wb):.1%} |")
