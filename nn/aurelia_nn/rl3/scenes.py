"""The scenes of an update: the pool (kept scenes of instances below the split, every family), the family weights, the plan of the
sampled rounds, and the files the Rust actor reads (`--kept`, `ARMY_SAMPLE_PLAN`)."""
import collections
import json
import random

ERAS = ["ancient", "classical", "medieval", "renaissance", "industrial", "modern", "atomic", "information"]


class Pool:
    """kept scenes by family, their greedy lengths (`lengths_rows`: rows of a greedy pass of the start net over the pool)."""

    def __init__(self, kept_path, lengths_rows, split=60):
        self.kept = collections.defaultdict(list)
        for line in open(kept_path):
            k = json.loads(line)
            if k["instance"] < split and k.get("attempt") is not None:
                self.kept[k["family"]].append(k)
        self.length, self.start_win = {}, collections.defaultdict(lambda: [0, 0])
        for line in open(lengths_rows):
            r = json.loads(line)
            if r.get("player") != "army-net":
                continue
            key = (r["family"], r["era"], r["instance"], r["attempt"])
            self.length[key] = max(int(r["rounds"]), 1)
            self.start_win[r["family"]][0] += int(bool(r["win"]))
            self.start_win[r["family"]][1] += 1
        self.families = sorted(f for f in self.kept if any((k["family"], k["era"], k["instance"], k["attempt"]) in self.length for k in self.kept[f]))
        for f in self.families:
            self.kept[f] = [k for k in self.kept[f] if (k["family"], k["era"], k["instance"], k["attempt"]) in self.length]

    def start_rate(self, f):
        w, n = self.start_win[f]
        return w / max(n, 1)


def draw(pool, n_of, K, G, seed, out_dir):
    """The scenes of an update: n_of[f] per family, K distinct rounds each (of the scene's greedy length). Writes `plan.json` and `kept.jsonl`
    in `out_dir`; returns the scenes [(key, family, rounds)]."""
    rng = random.Random(seed)
    sel = []
    for f in pool.families:
        n = min(n_of.get(f, 0), len(pool.kept[f]))
        for k in rng.sample(pool.kept[f], n):
            L = pool.length[(k["family"], k["era"], k["instance"], k["attempt"])]
            rounds = sorted(rng.sample(range(L), min(K, L)))
            sel.append((f"{f}/{ERAS.index(k['era'])}/{k['instance']}/{k['attempt']}", k, rounds))
    json.dump({"seed": seed, "copies": G, "plan": {key: rounds for key, _, rounds in sel}}, open(f"{out_dir}/plan.json", "w"))
    with open(f"{out_dir}/kept.jsonl", "w") as fh:
        for _, k, _ in sel:
            fh.write(json.dumps(k) + "\n")
    return sel
