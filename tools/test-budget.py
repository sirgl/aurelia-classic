#!/usr/bin/env python3
"""The time budget of the fast tier (docs/testing.md, "Two tiers").

    python3 tools/test-budget.py [THREADS]      run the default profile and check it
    python3 tools/test-budget.py --junit FILE   check a junit file of an earlier run

Runs `cargo nextest run --profile measure` (the fast tier plus a junit file), then prints the wall
time of the run, the CPU seconds of the test bodies, the 15 slowest tests and the longest chain a run
cannot be shorter than. Exits 1 when a fast-tier test is reported slow (over MAX_TEST seconds) or the work does not fit the
budget at 4 threads (BUDGET seconds). The CPU seconds do not depend on the threads of the run or on
the load of the machine as much as the wall time does: they are what to cut.
"""
import subprocess, sys, time
import xml.etree.ElementTree as ET

BUDGET = 30.0     # seconds of wall time of the fast tier at 4 threads, build not included
THREADS = 4
MAX_TEST = 2.0    # seconds: nextest reports a fast test slow at 2 s and ends it at 6 s (a pin may take longer); a test over this goes to the slow set
JUNIT = "target/nextest/measure/junit.xml"


def main(argv):
    if argv[:1] == ["--junit"]:
        path, wall = argv[1], None
    else:
        threads = argv[0] if argv else str(THREADS)
        start = time.time()
        # nextest's exit status is not looked at here: a failing test is the test run's business, the budget is ours.
        subprocess.run(["cargo", "+1.98.1", "nextest", "run", "--cargo-profile", "fasttest", "--workspace", "--profile", "measure",
                        "--test-threads", threads], check=False)
        wall, path = time.time() - start, JUNIT
    cases = [(float(c.get("time")), c.get("classname"), c.get("name")) for c in ET.parse(path).iter("testcase") if c.find("skipped") is None]
    cases.sort(reverse=True)
    work = sum(c[0] for c in cases)
    print(f"\n{len(cases)} tests, {work:.0f} s of test bodies" + (f", {wall:.0f} s wall including the build" if wall else ""))
    print("slowest:")
    for t, binary, name in cases[:15]:
        print(f"  {t:6.2f} s  {binary}  {name}")
    # A run on THREADS threads takes at least the work divided by the threads, and at least its longest test.
    floor = max(work / THREADS, cases[0][0])
    print(f"floor at {THREADS} threads: {floor:.1f} s (budget {BUDGET:.0f} s); longest test {cases[0][0]:.1f} s (limit {MAX_TEST:.0f} s)")
    slow = [c for c in cases if c[0] > MAX_TEST]
    for t, binary, name in slow:
        print(f"SLOW in the fast tier ({t:.1f} s > {MAX_TEST:.0f} s): {binary} {name}: make it fast or add it to the slow filterset in .config/nextest.toml")
    bad = floor > BUDGET or bool(slow)
    print("OVER THE BUDGET" if bad else "within the budget")
    return 1 if bad else 0


sys.exit(main(sys.argv[1:]))
