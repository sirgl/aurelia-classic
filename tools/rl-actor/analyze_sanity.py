"""Per-family win rates of the Rust actor's sampled plays and of the twin's, with Wilson 95% intervals; the verdict whether they overlap.
    analyze_sanity.py out.json --rust DIR [DIR...] --twin FILE.json [FILE.json ...]"""
import argparse, collections, json, math

def wilson(w, n, z=1.96):
    if n == 0:
        return (0.0, 0.0, 0.0)
    p = w / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (p, c - h, c + h)

ap = argparse.ArgumentParser()
ap.add_argument("out")
ap.add_argument("--rust", nargs="+", required=True)
ap.add_argument("--twin", nargs="+", required=True)
a = ap.parse_args()
R = collections.defaultdict(lambda: [0, 0])
for d in a.rust:
    for l in open(d + "/rows.jsonl"):
        r = json.loads(l)
        R[r["family"]][0] += int(r["win"])
        R[r["family"]][1] += 1
T = collections.defaultdict(lambda: [0, 0])
for f in a.twin:
    for up in json.load(open(f)):
        P, G = up["P"], up["G"]
        for i, v in enumerate(up["verdict"]):
            fam = up["families"][i % P]
            T[fam][0] += int(v == 1)
            T[fam][1] += 1
rows = []
for fam in sorted(R):
    wr, nr = R[fam]
    wt, nt = T[fam]
    pr, lr, hr = wilson(wr, nr)
    pt, lt, ht = wilson(wt, nt)
    rows.append({"family": fam, "rust": [wr, nr, pr, lr, hr], "twin": [wt, nt, pt, lt, ht], "overlap": not (hr < lt or ht < lr)})
tw, tn = sum(v[0] for v in R.values()), sum(v[1] for v in R.values())
uw, un = sum(v[0] for v in T.values()), sum(v[1] for v in T.values())
out = {"rows": rows, "rust_all": [tw, tn, *wilson(tw, tn)], "twin_all": [uw, un, *wilson(uw, un)], "family_mean_rust": sum(r["rust"][2] for r in rows) / len(rows), "family_mean_twin": sum(r["twin"][2] for r in rows) / len(rows)}
json.dump(out, open(a.out, "w"), indent=1)
for r in rows:
    print(f"{r['family']:18} rust {r['rust'][2]*100:5.1f} [{r['rust'][3]*100:5.1f},{r['rust'][4]*100:5.1f}] n={r['rust'][1]:4}  twin {r['twin'][2]*100:5.1f} [{r['twin'][3]*100:5.1f},{r['twin'][4]*100:5.1f}] n={r['twin'][1]:4}  {'overlap' if r['overlap'] else 'DIFFER'}")
print("all: rust %.1f twin %.1f; family mean rust %.1f twin %.1f" % (out["rust_all"][2]*100, out["twin_all"][2]*100, out["family_mean_rust"]*100, out["family_mean_twin"]*100))
