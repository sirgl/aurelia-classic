#!/bin/sh
# The gates G0-G3 of work/new-net-design-2026-11-22.md section 7, written before any run and not changed after.
# The integrator runs them on the integrated build; every big step is its own command so that each goes
# through the broker (macOS has no `timeout`; the steps are long, run them in the background):
#
#   work/res/res run --cpu 4 --name net2-gates -- sh classic/tools/net2-gates.sh build
#   work/res/res run --cpu 4 --name net2-gates -- sh classic/tools/net2-gates.sh g0
#   work/res/res run --cpu 4 --name net2-gates -- sh classic/tools/net2-gates.sh g1-run     # ~1.5 h on 4 threads (x2 variants)
#   sh classic/tools/net2-gates.sh g1-check
#   work/res/res run --cpu 4 --name net2-gates -- sh classic/tools/net2-gates.sh g2-run     # 800 games
#   sh classic/tools/net2-gates.sh g2-check                                                 # G2 and G3 from the same report
#
# Environment (all optional):
#   CLONE   the integrated checkout's classic/ directory      (default: the directory above tools/)
#   BIN     directory of the built binaries                   (default: $CARGO_TARGET_DIR/release, else $CLONE/../target-own/release)
#   OUT     where the runs and reports go                     (default: $CLONE/../net2-gates-out)
#   POS     the exam's positions: rounds 100-110              (default: stress/runs/positions, two levels above CLONE/..)
#   JOBS    threads of every run                              (default: $RES_THREADS, else 4)
#   CAND    the candidate variant                             (default: net2)
#   BASE    the baseline variant                              (default: nets-x-site)
#
# What is measured, and the pass rules (the design's table, verbatim in the checks below):
#   G0 untouched   --solo standard|pre-nets-x --suite standard --speed quick: the hashes below.
#   G1 exam        ai-stress on CAND and BASE, the same positions and instances. Pass: no (situation, strength)
#                  cell that passes for BASE fails for CAND, and the named moves (A5 nobody_attacked <= -0.15, B4
#                  capital_retaken >= +0.10, B9 strong rival_wins_science <= -0.10, A6/C2 weak_captured delta
#                  >= +0.10, P3 military_added_before_war >= +0.3, P4 script_sold_then_war <= -0.10, D3
#                  peace_round <= -2), each CAND minus BASE, mean over the levels, edited unless it says delta.
#   G2 games       ai-tournament --candidate CAND --baseline BASE --suite pace --speed quick, offsets 1700-1703
#                  (800 games). Pass: victory-weighted share >= 52 % pooled and >= 48 % per offset; real
#                  victories >= BASE's + 5 %; capitals lost <= 1.2 x; risings of rebels per seat <= 1.2 x.
#   G3 turn time   AI ms p50 a turn of the same run: CAND <= 1.15 x BASE.
# G4 (old exams), G5 (replication on offsets 1710-1711 by a fresh reviewer) and G6 (float twin parity,
# `pytest nn/tests/test_net2_parity.py`) are the other lanes' commands and are not scripted here.
#
# The wave-3 situations of the exam are not in this build's ai-stress (26 situations); when they are, add them to
# EXAM_END and EXAM_WINDOW below.

set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
CLONE=${CLONE:-$(cd "$HERE/.." && pwd)}
BIN=${BIN:-${CARGO_TARGET_DIR:+$CARGO_TARGET_DIR/release}}
BIN=${BIN:-$CLONE/../target-own/release}
OUT=${OUT:-$CLONE/../net2-gates-out}
POS=${POS:-$CLONE/../../stress/runs/positions}
JOBS=${JOBS:-${RES_THREADS:-4}}
CAND=${CAND:-net2}
BASE=${BASE:-nets-x-site}
STANDARD_HASH=8460fdf4e1b4bad1
PRE_NETS_X_HASH=d2d736c1273a82bb

# A1 and B9 are played to the end of the game (their outcome is the victory), instances 0-59 (the odd ones 1..59
# are the gate's); every other situation is played on its window, 100 instances. Together they are the 26
# situations of this build; no situation is played in both ways, so no metric mixes a window and an end.
EXAM_END=a1,b9
EXAM_WINDOW=a5,b1,b4,d1,c5,p3,p2,p7,p5,p1,p4,p8,a2,a3,a4,a6,a7,b2,b5,b6,b7,c2,d2,d3

step=${1:-}
mkdir -p "$OUT"

case "$step" in
build)
    cd "$CLONE"
    cargo +1.98.1 build --release -p aurelia-qa --bin ai-tournament --bin ai-stress -j "$JOBS"
    ;;

g0)
    for pair in "standard:$STANDARD_HASH" "pre-nets-x:$PRE_NETS_X_HASH"; do
        v=${pair%%:*}
        want=${pair##*:}
        "$BIN/ai-tournament" --solo "$v" --suite standard --speed quick --jobs "$JOBS" >"$OUT/g0-$v.md" 2>&1
        if grep -q "$want" "$OUT/g0-$v.md"; then
            echo "G0 $v: $want  PASS"
        else
            echo "G0 $v: wanted $want, report says: $(grep -o 'result hash `[0-9a-f]*`' "$OUT/g0-$v.md" | head -1)  FAIL"
            exit 1
        fi
    done
    ;;

g1-run)
    for v in "$CAND" "$BASE"; do
        "$BIN/ai-stress" run --variant "$v" --positions "$POS" --out "$OUT/exam/$v" --situations "$EXAM_END" --instances 60 --jobs "$JOBS"
        "$BIN/ai-stress" suite --variant "$v" --positions "$POS" --out "$OUT/exam" --situations "$EXAM_WINDOW" --instances 100 --no-end --jobs "$JOBS"
    done
    ;;

g1-check)
    python3 -I - "$OUT/exam" "$CAND" "$BASE" <<'PY'
import json, sys
root, cand, base = sys.argv[1:4]

def cells(variant):
    # suite-NAME.md: | situation | strength | n | NAME | standard | ...; the verdict of NAME is column 4.
    out = {}
    for line in open(f"{root}/suite-{variant}.md"):
        c = [x.strip() for x in line.strip().strip("|").split("|")]
        if len(c) > 4 and c[0] and c[0] != "situation" and not c[0].startswith("-"):
            out[(c[0], c[1])] = c[3].startswith("pass")
    return out

mine, theirs = cells(cand), cells(base)
lost = sorted(k for k, ok in theirs.items() if ok and not mine.get(k, False))
print(f"G1 cells: {sum(mine.values())}/{len(mine)} pass for {cand}, {sum(theirs.values())}/{len(theirs)} for {base}")
print("G1 cells that pass for", base, "and fail for", cand + ":", lost if lost else "none")
failed = bool(lost)

def rows(variant):
    return [json.loads(l) for l in open(f"{root}/{variant}/rows.jsonl") if l.strip()]

WAVE_ONE = {"a5", "b1", "a1", "b4", "b9", "d1", "c5"}

def mean(rs, situation, metric, delta=False, level=None):
    # Wave 1 situations: the odd instances 1..59; wave 2: every instance.
    v = []
    for r in rs:
        if r["situation"] != situation or (level is not None and r["level"] != level):
            continue
        if situation in WAVE_ONE and not (r["instance"] % 2 == 1 and r["instance"] <= 59):
            continue
        if metric not in r["edited"]:
            continue
        v.append(r["edited"][metric] - (r["control"].get(metric, 0) if delta else 0))
    return (sum(v) / len(v), len(v)) if v else (float("nan"), 0)

moves = [
    ("A5 nobody_attacked", "a5", "nobody_attacked", False, None, lambda d: d <= -0.15, "<= -0.15"),
    ("B4 capital_retaken", "b4", "capital_retaken", False, None, lambda d: d >= 0.10, ">= +0.10"),
    ("B9 strong rival_wins_science", "b9", "rival_wins_science", False, 2, lambda d: d <= -0.10, "<= -0.10"),
    ("A6 weak_captured (delta)", "a6", "weak_captured", True, None, lambda d: d >= 0.10, ">= +0.10"),
    ("C2 weak_captured (delta)", "c2", "weak_captured", True, None, lambda d: d >= 0.10, ">= +0.10"),
    ("P3 military_added_before_war", "p3", "military_added_before_war", False, None, lambda d: d >= 0.3, ">= +0.3"),
    ("P4 script_sold_then_war", "p4", "script_sold_then_war", False, None, lambda d: d <= -0.10, "<= -0.10"),
    ("D3 peace_round", "d3", "peace_round", False, None, lambda d: d <= -2, "<= -2"),
]
a, b = rows(cand), rows(base)
print()
print(f"| named move | {cand} | {base} | difference | rule | pass |")
print("|---|---:|---:|---:|---|---|")
for name, sit, metric, delta, level, ok, text in moves:
    x, n = mean(a, sit, metric, delta, level)
    y, m = mean(b, sit, metric, delta, level)
    d = x - y
    good = n > 0 and m > 0 and ok(d)
    failed |= not good
    print(f"| {name} | {x:.3f} (n {n}) | {y:.3f} (n {m}) | {d:+.3f} | {text} | {'yes' if good else 'NO'} |")
print()
print("G1:", "FAIL" if failed else "PASS")
sys.exit(1 if failed else 0)
PY
    ;;

g2-run)
    "$BIN/ai-tournament" --candidate "$CAND" --baseline "$BASE" --suite pace --speed quick --seed-offset 1700 --repeat 4 --jobs "$JOBS" >"$OUT/g2.md" 2>&1
    echo "report: $OUT/g2.md"
    ;;

g2-check)
    python3 -I - "$OUT/g2.md" "$CAND" "$BASE" <<'PY'
import re, sys
path, cand, base = sys.argv[1:4]
text = open(path).read()

def table(header_part):
    """Every markdown table whose header contains `header_part`: a list of (header cells, rows of cells)."""
    out, lines = [], text.splitlines()
    for i, line in enumerate(lines):
        if line.startswith("|") and header_part in line:
            head = [c.strip() for c in line.strip().strip("|").split("|")]
            rows = []
            for row in lines[i + 2:]:
                if not row.startswith("|"):
                    break
                rows.append([c.strip() for c in row.strip().strip("|").split("|")])
            out.append((head, rows))
    return out

failed = False
def check(name, value, ok, rule):
    global failed
    failed |= not ok
    print(f"| {name} | {value} | {rule} | {'yes' if ok else 'NO'} |")

print("| gate | value | rule | pass |")
print("|---|---|---|---|")

# G2: the repeats table, one row per offset: victory-weighted share and real victories.
head, rows = table("victory-weighted share")[0]
share = [float(r[head.index("victory-weighted share")].rstrip(" %")) for r in rows]
real = [tuple(int(x) for x in r[head.index("real victories")].split("/")) for r in rows]
games = [int(r[head.index("candidate wins")].split("/")[1]) for r in rows]
pooled = sum(s * g for s, g in zip(share, games)) / sum(games)
check("G2 victory-weighted share, pooled", f"{pooled:.1f} % over {sum(games)} games", pooled >= 52.0, ">= 52 %")
check("G2 victory-weighted share, worst offset", f"{min(share):.1f} % (offsets {share})", min(share) >= 48.0, ">= 48 % each")
rc, rb = sum(r[0] for r in real), sum(r[1] for r in real)
check("G2 real victories", f"{cand} {rc}, {base} {rb}", rc >= 1.05 * rb, ">= 1.05 x baseline")

# G2: capitals lost, from the headline "Capitals lost: `A` n, `B` m."
m = re.findall(r"Capitals lost: `([^`]+)` (\d+), `([^`]+)` (\d+)", text)
if m:
    caps = {m[-1][0]: int(m[-1][1]), m[-1][2]: int(m[-1][3])}
    c, b = caps[cand], caps[base]
    check("G2 capitals lost", f"{cand} {c}, {base} {b}", c <= 1.2 * b, "<= 1.2 x baseline")
else:
    check("G2 capitals lost", "not found in the report", False, "<= 1.2 x baseline")

# G2: risings of rebels per seat, summed over the per-offset tables; G3: AI ms p50, the median over the offsets.
risings = {}
for head, rows in table("risings of rebels"):
    for r in rows:
        risings.setdefault(r[1], []).append(float(r[head.index("risings per seat")]))
if cand in risings and base in risings:
    c, b = sum(risings[cand]) / len(risings[cand]), sum(risings[base]) / len(risings[base])
    check("G2 risings of rebels per seat", f"{cand} {c:.3f}, {base} {b:.3f}", c <= 1.2 * b, "<= 1.2 x baseline")
else:
    check("G2 risings of rebels per seat", "not found in the report", False, "<= 1.2 x baseline")

ms = {}
for head, rows in table("AI ms p50"):
    for r in rows:
        ms.setdefault(r[1], []).append(float(r[head.index("AI ms p50")]))
def median(v):
    v = sorted(v)
    return v[len(v) // 2]
if cand in ms and base in ms:
    c, b = median(ms[cand]), median(ms[base])
    check("G3 AI ms p50 a turn", f"{cand} {c:.2f}, {base} {b:.2f} ({c / b:.2f} x)", c <= 1.15 * b, "<= 1.15 x baseline")
else:
    check("G3 AI ms p50 a turn", "not found in the report", False, "<= 1.15 x baseline")
print()
print("G2/G3:", "FAIL" if failed else "PASS")
sys.exit(1 if failed else 0)
PY
    ;;

*)
    echo "usage: net2-gates.sh build | g0 | g1-run | g1-check | g2-run | g2-check   (see the header)" >&2
    exit 2
    ;;
esac
