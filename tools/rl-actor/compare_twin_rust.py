import json, sys
from aurelia_nn.army.data import map_file, skip_play_at
ERAS = ["ancient", "classical", "medieval", "renaissance", "industrial", "modern", "atomic", "information"]
twin = json.load(open(sys.argv[1])); plan = json.load(open(sys.argv[3]))["plan"]
buf = map_file(sys.argv[2]); at = 0
dp, da, n, miss, bad = [], [], 0, 0, 0
while at < len(buf):
    meta, at, *_ = skip_play_at(buf, at)
    name = f"{meta['family']}/{ERAS.index(meta['era'])}/{meta['instance']}/{meta['attempt']}"
    rounds = set(plan[name])
    tw = [t for t in twin.get(name, []) if t[0] in rounds]
    rs = meta["sample"]
    if len(tw) != len(rs):
        bad += 1; print("length differs", name, len(tw), len(rs)); continue
    for t, r in zip(tw, rs):
        dp.append(abs(t[3] - r[1]))
        if r[2] is not None and t[4] is not None: da.append(abs(t[4] - r[2]))
print("plays with different sampled-decision count:", bad, "| decisions compared", len(dp), "max pick", max(dp, default=0), "max action", max(da, default=0), "share >1e-4", sum(1 for x in dp + da if x > 1e-4) / max(1, len(dp + da)), "mean", sum(dp + da) / max(1, len(dp + da)))
