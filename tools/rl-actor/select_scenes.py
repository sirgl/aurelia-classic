"""The fixed work of the comparison: for update u the 64 scenes (armyrl's own allocation and draw: P 64, at least 2 per family, the
rest by the family weights at the start, no rate yet), K = 6 sampled rounds per scene (distinct rounds of the scene's Rust length,
`armyrl.play_length`), written for both sides: sel-uN.json (the twin), plan-uN.json + kept-uN.jsonl (the Rust actor)."""
import collections, json, random, sys
from aurelia_nn.tacsim import armyrl as R, armyobs as A

DATA = "/Users/roman.ivanov/civ/work/nets/mlx-army/data"
OUT = "/Users/roman.ivanov/civ/work/nets/rust-actor/data"
ERAS = ["ancient", "classical", "medieval", "renaissance", "industrial", "modern", "atomic", "information"]
P, K, G = 64, 6, 8

def main():
    ups = [int(x) for x in sys.argv[1:]] or [1]
    train_all = [p for p in A.load(f"{DATA}/tr-MBf6c-r.jsonl", families=R.FAMILIES)]
    inst = lambda p: int(p[0]["play"].split("/")[2])
    train_all = [p for p in train_all if inst(p) < 70]
    by_fam = collections.defaultdict(list)
    for p in train_all:
        by_fam[p[0]["play"].split("/")[0]].append(p)
    fams = [f for f in R.FAMILIES if by_fam.get(f)]
    bars = R.bars_from(f"{DATA}/teach-r-train/rows.jsonl", set(fams))
    print({f: len(by_fam[f]) for f in fams}, flush=True)
    kept = {}
    for l in open(f"{DATA}/kept-r.jsonl"):
        d = json.loads(l)
        kept[(d["family"], d["era"], d["instance"])] = d
    for u in ups:
        random.seed(1000 + u)
        n_of, _ = R.allocate(P, fams, {}, bars, 2, 4.0)
        sel = []
        for f in fams:
            for p in random.sample(by_fam[f], min(n_of[f], len(by_fam[f]))):
                L = R.play_length(p)
                sel.append({"name": p[0]["play"], "family": f, "length": L, "rounds": sorted(random.sample(range(L), min(K, L)))})
        plan = {s["name"]: s["rounds"] for s in sel}
        json.dump({"u": u, "P": P, "G": G, "K": K, "scenes": sel}, open(f"{OUT}/sel-u{u}.json", "w"), indent=1)
        json.dump({"seed": 7000 + u, "copies": G, "plan": plan}, open(f"{OUT}/plan-u{u}.json", "w"))
        with open(f"{OUT}/kept-u{u}.jsonl", "w") as fh:
            for s in sel:
                fam, era, i, a = s["name"].split("/")
                k = dict(kept[(fam, ERAS[int(era)], int(i))])
                assert k["attempt"] == int(a), (s["name"], k)
                fh.write(json.dumps(k) + "\n")
        print("update", u, "plays", len(sel), "per family", n_of, "mean length", sum(s["length"] for s in sel) / len(sel), flush=True)

main()
