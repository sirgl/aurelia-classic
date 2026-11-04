#!/usr/bin/env python3
"""Tables of the start census (crates/classic-rules/examples/start_census.rs).

  start-census-report.py report  CSV [CSV ...] --title T --thresholds FILE [--write-thresholds] [--json OUT]
  start-census-report.py compare BEFORE.json AFTER.json

`report` reads the rows of the example and prints a markdown report: the distribution (min, p10, median, p90, max)
of what lies around a start, the share of "bad" starts, the strategic coverage and the spread of a game.
The thresholds of a "bad" start come from the median of the BEFORE run (`--write-thresholds`) and stay fixed for the
AFTER run (`--thresholds FILE`), so both runs are judged by the same numbers.

Definitions (radius 2 = the 18 tiles around the start, radius 3 = the 36; the start tile is left out):
  food / production : sum of the tiles' food / production (real tile yields of the ruleset on turn 0)
  value2 / value3   : food + production in that radius
  bad_food          : food2 < 50 % of the median food2
  bad_prod          : production2 < 25 % of the median production2
  bad_land          : land3 < 50 % of the median land3 (a start on a sliver of land)
  bad               : any of the three
  spread of a game  : value2 of the best start minus value2 of the worst one; ratio = worst / best
"""
import csv
import json
import sys


def pct(values, p):
    v = sorted(values)
    if not v:
        return 0.0
    k = (len(v) - 1) * p / 100
    f = int(k)
    c = min(f + 1, len(v) - 1)
    return v[f] + (v[c] - v[f]) * (k - f)


def five(values):
    return [pct(values, p) for p in (0, 10, 50, 90, 100)]


def fmt(x):
    return f"{x:.0f}" if abs(x - round(x)) < 1e-9 else f"{x:.1f}"


def load(paths):
    rows = []
    for path in paths:
        with open(path) as handle:
            for r in csv.DictReader(handle):
                for k, v in r.items():
                    if k not in ("size", "script"):
                        r[k] = int(v)
                r["value2"] = r["f2"] + r["p2"]
                r["value3"] = r["f3"] + r["p3"]
                rows.append(r)
    return rows


def thresholds_of(rows):
    med = lambda k: pct([r[k] for r in rows], 50)
    return {
        "food2_min": 0.5 * med("f2"),
        "prod2_min": 0.25 * med("p2"),
        "land3_min": 0.5 * med("land3"),
        "median_f2": med("f2"),
        "median_p2": med("p2"),
        "median_land3": med("land3"),
    }


def bad_flags(r, t):
    return (r["f2"] < t["food2_min"], r["p2"] < t["prod2_min"], r["land3"] < t["land3_min"])


def games(rows):
    out = {}
    for r in rows:
        out.setdefault((r["size"], r["script"], r["seed"]), []).append(r)
    return out


def summary(rows, t):
    n = len(rows)
    bad = [bad_flags(r, t) for r in rows]
    strategic = [k for k in rows[0] if k.startswith("s_")]
    early = ["s_horses", "s_iron"]
    out = {"starts": n, "maps": len(games(rows))}
    out["bad_food"] = sum(b[0] for b in bad) / n
    out["bad_prod"] = sum(b[1] for b in bad) / n
    out["bad_land"] = sum(b[2] for b in bad) / n
    out["bad"] = sum(any(b) for b in bad) / n
    out["no_prod_tile"] = sum(1 for r in rows if r["prod1_2"] == 0) / n
    for k in ("horses", "iron"):
        out[f"has_{k}"] = sum(1 for r in rows if r[f"s_{k}"] > 0) / n
    out["has_horses_and_iron"] = sum(1 for r in rows if all(r[k] > 0 for k in early)) / n
    out["strategic_under_centre"] = sum(1 for r in rows if r["under_centre"] > 0) / n
    for k in ("f2", "p2", "value2", "f3", "p3", "value3", "land2", "land3", "g2", "lux3", "bonus3", "fresh2"):
        out[k] = five([r[k] for r in rows])
    g = games(rows)
    spreads, ratios, spreads3 = [], [], []
    for rs in g.values():
        v = [r["value2"] for r in rs]
        spreads.append(max(v) - min(v))
        ratios.append(min(v) / max(v) if max(v) else 1.0)
        v3 = [r["value3"] for r in rs]
        spreads3.append(max(v3) - min(v3))
    out["spread2"] = five(spreads)
    out["spread3"] = five(spreads3)
    out["ratio2"] = five(ratios)
    out["games_below_half"] = sum(1 for x in ratios if x < 0.5) / len(ratios)
    out["games_with_bad_start"] = sum(1 for rs in g.values() if any(any(bad_flags(r, t)) for r in rs)) / len(g)
    out["strategic"] = {k: sum(1 for r in rows if r[k] > 0) / n for k in strategic}
    # What the balance changed (game-level columns, repeated on each row of a game; absent before the balance).
    if "edit_bonus" in rows[0]:
        firsts = [rs[0] for rs in g.values()]
        per_start = sum(len(rs) for rs in g.values()) / len(g)
        for key in ("strategic", "missing", "bonus", "turned", "cleared"):
            out[f"edit_{key}"] = sum(r[f"edit_{key}"] for r in firsts) / len(firsts) / per_start
        out["games_with_edit"] = sum(1 for r in firsts if r["edit_bonus"] + r["edit_turned"] > 0) / len(firsts)
        out["games_missing"] = sum(1 for r in firsts if r["edit_missing"] > 0) / len(firsts)
    return out


def row5(name, values):
    return f"| {name} | " + " | ".join(fmt(x) for x in values) + " |"


def report(rows, t, title):
    s = summary(rows, t)
    L = [f"# {title}", ""]
    L.append(f"Starts: {s['starts']} on {s['maps']} maps. Thresholds of a bad start (fixed from the BEFORE medians): "
             f"food2 < {t['food2_min']:.1f}, production2 < {t['prod2_min']:.2f}, land3 < {t['land3_min']:.1f}.")
    L += ["", "## Distribution per start", "", "| what | min | p10 | median | p90 | max |", "|---|---|---|---|---|---|"]
    names = [("f2", "food, radius 2"), ("p2", "production, radius 2"), ("g2", "gold, radius 2"),
             ("value2", "food + production, radius 2"), ("f3", "food, radius 3"), ("p3", "production, radius 3"),
             ("value3", "food + production, radius 3"), ("land2", "land tiles, radius 2 (of 18)"),
             ("land3", "land tiles, radius 3 (of 36)"), ("fresh2", "fresh-water land tiles, radius 2"),
             ("lux3", "luxury deposits, radius 3"), ("bonus3", "bonus deposits, radius 3")]
    for k, name in names:
        L.append(row5(name, s[k]))
    L += ["", "## Bad starts", "", "| share of starts | value |", "|---|---|",
          f"| bad (any of the three) | {100 * s['bad']:.1f} % |",
          f"| bad food | {100 * s['bad_food']:.1f} % |", f"| bad production | {100 * s['bad_prod']:.1f} % |",
          f"| bad land | {100 * s['bad_land']:.1f} % |",
          f"| no tile with production within radius 2 | {100 * s['no_prod_tile']:.1f} % |",
          f"| games with at least one bad start | {100 * s['games_with_bad_start']:.1f} % |"]
    L += ["", "## Strategic resources within radius 3, not under the start tile", "",
          "| share of starts with | value |", "|---|---|",
          f"| horses | {100 * s['has_horses']:.1f} % |", f"| iron | {100 * s['has_iron']:.1f} % |",
          f"| horses and iron | {100 * s['has_horses_and_iron']:.1f} % |"]
    for k, v in s["strategic"].items():
        if k in ("s_horses", "s_iron"):
            continue
        L.append(f"| {k[2:]} | {100 * v:.1f} % |")
    L.append(f"| a strategic deposit under the start tile (should be 0) | {100 * s['strategic_under_centre']:.1f} % |")
    if "edit_bonus" in s:
        L += ["", "## What the balance changed (per start, mean)", "", "| edit | per start |", "|---|---|",
              f"| guaranteed strategic deposits put | {s['edit_strategic']:.2f} |",
              f"| strategic deposits that could not be put | {s['edit_missing']:.3f} |",
              f"| bonus deposits put to top a start up | {s['edit_bonus']:.2f} |",
              f"| tiles turned to another terrain | {s['edit_turned']:.3f} |",
              f"| features (jungle, forest) cleared for a deposit | {s['edit_cleared']:.3f} |",
              f"| games with a top-up edit | {100 * s['games_with_edit']:.1f} % |"]
    L += ["", "## Spread inside a game (value2 of the best start minus the worst)", "",
          "| what | min | p10 | median | p90 | max |", "|---|---|---|---|---|---|",
          row5("spread, radius 2", s["spread2"]), row5("spread, radius 3", s["spread3"]),
          row5("worst / best, radius 2", [x for x in s["ratio2"]]).replace("| 0 |", "| 0.0 |"),
          "", f"Games where the worst start is under half of the best: {100 * s['games_below_half']:.1f} %.", ""]
    # by setup
    L += ["## By setup", "", "| setup | starts | bad | horses+iron | median spread2 | median worst/best |", "|---|---|---|---|---|---|"]
    groups = {}
    for r in rows:
        groups.setdefault((r["size"], r["script"]), []).append(r)
    for (size, script), rs in sorted(groups.items()):
        g = summary(rs, t)
        L.append(f"| {size} {script} | {g['starts']} | {100 * g['bad']:.1f} % | {100 * g['has_horses_and_iron']:.1f} % | "
                 f"{fmt(g['spread2'][2])} | {g['ratio2'][2]:.2f} |")
    return "\n".join(L) + "\n", s


def compare(before, after):
    def pctv(x):
        return f"{100 * x:.1f} %"

    rows = [
        ("bad starts (food, production or land)", pctv(before["bad"]), pctv(after["bad"])),
        ("bad food", pctv(before["bad_food"]), pctv(after["bad_food"])),
        ("bad production", pctv(before["bad_prod"]), pctv(after["bad_prod"])),
        ("bad land", pctv(before["bad_land"]), pctv(after["bad_land"])),
        ("no tile with production within radius 2", pctv(before["no_prod_tile"]), pctv(after["no_prod_tile"])),
        ("games with at least one bad start", pctv(before["games_with_bad_start"]), pctv(after["games_with_bad_start"])),
        ("median spread inside a game, radius 2 (food + production)", fmt(before["spread2"][2]), fmt(after["spread2"][2])),
        ("p90 spread inside a game, radius 2", fmt(before["spread2"][3]), fmt(after["spread2"][3])),
        ("median worst / best start, radius 2", f"{before['ratio2'][2]:.2f}", f"{after['ratio2'][2]:.2f}"),
        ("p10 worst / best start, radius 2", f"{before['ratio2'][1]:.2f}", f"{after['ratio2'][1]:.2f}"),
        ("games where the worst start is under half of the best", pctv(before["games_below_half"]), pctv(after["games_below_half"])),
        ("starts with horses within radius 3", pctv(before["has_horses"]), pctv(after["has_horses"])),
        ("starts with iron within radius 3", pctv(before["has_iron"]), pctv(after["has_iron"])),
        ("starts with horses and iron", pctv(before["has_horses_and_iron"]), pctv(after["has_horses_and_iron"])),
        ("min food, radius 2 (min / p10)", f"{fmt(before['f2'][0])} / {fmt(before['f2'][1])}", f"{fmt(after['f2'][0])} / {fmt(after['f2'][1])}"),
        ("min production, radius 2 (min / p10)", f"{fmt(before['p2'][0])} / {fmt(before['p2'][1])}", f"{fmt(after['p2'][0])} / {fmt(after['p2'][1])}"),
        ("median food / production, radius 2", f"{fmt(before['f2'][2])} / {fmt(before['p2'][2])}", f"{fmt(after['f2'][2])} / {fmt(after['p2'][2])}"),
        ("median land tiles, radius 3 (of 36)", fmt(before["land3"][2]), fmt(after["land3"][2])),
        ("median luxury deposits, radius 3", fmt(before["lux3"][2]), fmt(after["lux3"][2])),
    ]
    L = ["| measure | before | after |", "|---|---|---|"]
    L += [f"| {a} | {b} | {c} |" for a, b, c in rows]
    return "\n".join(L) + "\n"


def main():
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return
    mode, rest = args[0], args[1:]
    if mode == "compare":
        b = json.load(open(rest[0]))
        a = json.load(open(rest[1]))
        print(compare(b, a))
        return
    opts = {"--title": "Start census", "--thresholds": None, "--json": None}
    write = False
    paths = []
    i = 0
    while i < len(rest):
        if rest[i] == "--write-thresholds":
            write = True
        elif rest[i] in opts:
            opts[rest[i]] = rest[i + 1]
            i += 1
        else:
            paths.append(rest[i])
        i += 1
    rows = load(paths)
    if write:
        t = thresholds_of(rows)
        json.dump(t, open(opts["--thresholds"], "w"), indent=1)
    else:
        t = json.load(open(opts["--thresholds"]))
    text, s = report(rows, t, opts["--title"])
    if opts["--json"]:
        json.dump(s, open(opts["--json"], "w"), indent=1)
    print(text)


if __name__ == "__main__":
    main()
