"""The trainer table of the army net on the held-out instances (DESIGN.md, «Evaluation and the gate»).

python -m aurelia_nn.army.table --kept data/kept.jsonl --rows runs/eval/rows.jsonl [runs/teach-hold/rows.jsonl ...]
    --tac2 RUN/rows.jsonl ... --from 80 --to 100 --out table.json
Per family (prey split by context as the trainers' page does): N held-out kept, win % of army-net, standard, tac2,
unciv, every reference, the best teacher (references and unciv), the net's share of it, and the gate.
"""
import argparse
import collections
import json

ASSAULT = {'assault-heal', 'promotion-choice', 'hill-archers', 'city-fire', 'planned-assault'}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--kept', required=True)
    ap.add_argument('--rows', nargs='+', required=True)
    ap.add_argument('--tac2', nargs='*', default=[])
    ap.add_argument('--from', dest='lo', type=int, default=80)
    ap.add_argument('--to', dest='hi', type=int, default=100)
    ap.add_argument('--out', required=True)
    a = ap.parse_args()
    kept = {}
    for l in open(a.kept):
        k = json.loads(l)
        if k['attempt'] is not None and a.lo <= k['instance'] < a.hi:
            kept[(k['family'], k['era'], k['instance'])] = k['attempt']
    res = collections.defaultdict(lambda: collections.defaultdict(lambda: [0, 0]))
    ctx = {}

    def add(r, name):
        key = (r['family'], r['era'], r['instance'])
        if kept.get(key) != r['attempt']:
            return
        fam = r['family']
        if fam == 'prey':
            c = r['info'].get('context')
            if c is not None:
                ctx[key] = c
            fam = 'prey-land' if ctx.get(key, c) == 'land' else 'prey-either'
        cell = res[fam][name]
        cell[0] += bool(r['win'])
        cell[1] += 1

    for path in a.rows:
        for l in open(path):
            r = json.loads(l)
            add(r, r['player'])
    for path in a.tac2:
        for l in open(path):
            r = json.loads(l)
            if r['info'].get('stage') == 'player' and r['player'] == 'tac2':
                add(r, 'tac2')
    out = []
    for fam in sorted(res):
        cells = res[fam]
        rate = {p: (w / n if n else 0.0, n) for p, (w, n) in cells.items()}
        teachers = {p: v for p, v in rate.items() if p.startswith('ref-') or p == 'unciv'}
        best = max(teachers.items(), key=lambda x: (x[1][0], x[0] != 'unciv')) if teachers else (None, (0.0, 0))
        n = rate.get('army-net', (0, 0))[1]
        net = rate.get('army-net', (0.0, 0))[0]
        share = net / best[1][0] if best[1][0] > 0 else None
        out.append({'family': fam, 'n': n, 'rates': {p: v[0] for p, v in rate.items()}, 'counts': {p: cells[p] for p in cells},
                    'best': best[0], 'best_rate': best[1][0], 'net_share': share, 'pass': share is not None and share >= 0.9 and n >= 10})
    eligible = [r for r in out if r['n'] >= 10 and r['best_rate'] > 0]
    passed = [r for r in eligible if r['pass']]
    summary = {'families': len(eligible), 'passed': len(passed), 'gate_win': len(eligible) > 0 and len(passed) * 2 >= len(eligible)}
    json.dump({'rows': out, 'summary': summary}, open(a.out, 'w'), indent=1)
    for r in out:
        print(f"{r['family']:18} n={r['n']:4} net={r['rates'].get('army-net', 0):.2f} std={r['rates'].get('standard', 0):.2f} tac2={r['rates'].get('tac2', float('nan')):.2f} unciv={r['rates'].get('unciv', 0):.2f} best={r['best']}:{r['best_rate']:.2f} share={r['net_share'] if r['net_share'] is None else round(r['net_share'], 2)} {'PASS' if r['pass'] else ''}")
    print(json.dumps(summary))


if __name__ == '__main__':
    main()
