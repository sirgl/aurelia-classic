"""A slim copy of an army obs dump for the twin's own play (training and evaluation, `compare` off).

    python -m aurelia_nn.tacsim.armyslim IN.jsonl OUT.jsonl

The own play reads of a play's moments only: the first moment (ids, handles, the wounded modifier, the city strength,
the far cities' token rows), the first moment of each round (the economy's schedule) and the most units any moment
holds (the template's slots). The slim copy keeps the scene line, the end line and exactly those moments, in their
order; the event lines and the other moments are dropped. Streams the input line by line.
"""

import json
import sys


def slim(src, dst):
    n_in = n_out = plays = 0

    def flush(out, p):
        if p is None:
            return 0
        scene, moments, end = p
        keep = set()
        seen = set()
        for i, (rnd, units, _) in enumerate(moments):
            if i == 0 or rnd not in seen:
                keep.add(i)
            seen.add(rnd)
        if moments:
            keep.add(max(range(len(moments)), key=lambda i: (moments[i][1], -i)))
        out.write(scene)
        for i in sorted(keep):
            out.write(moments[i][2])
        if end is not None:
            out.write(end)
        return len(keep)

    with open(src) as fh, open(dst, "w") as out:
        live = {}
        for line in fh:
            n_in += 1
            d = json.loads(line)
            t = d["type"]
            if t == "ev":
                continue
            name = d["play"]
            if t == "scene":
                live[name] = [line, [], None]
                plays += 1
            elif t == "moment":
                live[name][1].append((d["round"], len(d["state"]["units"]), line))
            else:
                p = live.pop(name)
                p[2] = line
                n_out += flush(out, p)
        for p in live.values():
            n_out += flush(out, p)
    return plays, n_in, n_out


if __name__ == "__main__":
    p, a, b = slim(sys.argv[1], sys.argv[2])
    print(f"{p} plays; {a} lines in, {b} moments kept", flush=True)
