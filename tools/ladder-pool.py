#!/usr/bin/env python3
"""A pool of solvable siege ladder scenes (`ai-selfplay --ladder-pool FILE`,
`selfplay::LadderPool`, classic/docs/ai-net-data.md «Solvable scenes»).

Per rung, ai-selfplay plays GAMES scenes (one game a group, from seed offset
OFFSET + 1000 × rung) with the shallow search teacher on the learner's seat
(`search:depth=0,rollouts=1`); a scene is solvable when the teacher takes the
city in the learner's first turn where the scene allows one by construction
(`same_turn`), else at any time in the war. The solvable ones go to the pool
as (seed offset, index); an existing pool file is extended (rungs replaced).
Prints per rung the yield (solvable / played) and the seconds a scene.

usage: ladder-pool.py --bin AI_SELFPLAY --out POOL.json --work DIR [--rungs 16-40] [--games 192] [--jobs 4] [--offset 900000]
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

# The battle settings of the siege ladder runs (e2_loop with --siege-ladder --ladder-vary).
SUITE, SIZES, SPEED, ROUNDS, ARMY = "standard", ["small"], "quick", (20, 40), (6, 10)


def rungs_of(text: str) -> list[int]:
    out = []
    for part in text.split(","):
        a, _, b = part.partition("-")
        out += list(range(int(a), int(b or a) + 1))
    return out


def solvable(game: dict) -> bool:
    b = game["ground"]["battle"]
    if b.get("same_turn"):
        return b.get("captured_round") == b.get("start_round")
    return b.get("captured_round") is not None


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--rungs", default="16-40")
    ap.add_argument("--games", type=int, default=192)
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--offset", type=int, default=900000)
    a = ap.parse_args(argv)
    out = Path(a.out)
    speed_key = None
    pool = json.loads(out.read_text()) if out.exists() else None
    for rung in rungs_of(a.rungs):
        d = Path(a.work) / f"r{rung}"
        offset = a.offset + 1000 * rung
        cmd = [a.bin, "--learner-mind", "search:depth=0,rollouts=1", "--suite", SUITE, "--sizes", ",".join(SIZES), "--speed", SPEED,
               "--battle", "siege", "--battle-rounds", f"{ROUNDS[0]}-{ROUNDS[1]}", "--battle-army", f"{ARMY[0]}-{ARMY[1]}", "--battle-difficulty", "1",
               "--battle-rung", str(rung), "--battle-minimal", "--battle-vary", "--games", str(a.games), "--group", "1", "--opponents", "standard",
               "--jobs", str(a.jobs), "--seed-offset", str(offset), "--out", str(d)]
        with open(Path(a.work) / f"r{rung}.log", "w") as f:
            subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT)
        games = [json.loads(p.read_text()) for p in sorted(d.glob("*.e2.json"))]
        if not games:
            print(f"rung {rung}: no game played", flush=True)
            continue
        speed_key = speed_key or games[0]["spec"].get("speed", "")
        good = sorted(int(g["group"]) for g in games if solvable(g))
        same = [g for g in games if g["ground"]["battle"].get("same_turn")]
        secs = sum(g["seconds"] for g in games) / len(games)
        print(f"rung {rung}: {len(good)} of {len(games)} solvable ({len(good) / len(games):.3f}; same-turn scenes {sum(solvable(g) for g in same)} of {len(same)}, "
              f"the others {len(good) - sum(solvable(g) for g in same)} of {len(games) - len(same)}), {secs:.2f} s a scene", flush=True)
        if pool is None:
            pool = {"suite": SUITE, "sizes": SIZES, "speed": speed_key, "rounds": list(ROUNDS), "army": list(ARMY), "minimal": True, "vary": True, "rungs": {}}
        pool["rungs"][str(rung)] = [[offset, i] for i in good]
        out.write_text(json.dumps(pool))
    return 0


if __name__ == "__main__":
    sys.exit(main())
