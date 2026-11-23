"""The gate table of the army net on the held-out instances: per family the best teacher (the recorded trainers'
plays, runs/teach4-hold/rows.jsonl) and each net given, "*" = win >= 90 % of the best teacher.
python tools/m5/py/m_table.py OUT.json NAME=runs/EVAL/rows.jsonl [NAME=...]   (run from the lab dir)"""
import json
import os
import subprocess
import sys

LAB = os.environ.get('AURELIA_LAB') or os.path.join(os.path.dirname(__file__), '..', '..', '..', 'lab')
REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
out = sys.argv[1]
variants = [v.split('=')[0] + '=' + os.path.abspath(v.split('=', 1)[1]) for v in sys.argv[2:]]
teach = os.path.join(LAB, 'runs', 'teach4-hold', 'rows.jsonl')
kept = os.path.join(LAB, 'data', 'kept-m.jsonl')
subprocess.run([sys.executable, '-m', 'aurelia_nn.army.table', '--kept', kept, '--rows', teach, '--variant', *variants,
                '--out', os.path.abspath(out)], cwd=os.path.join(REPO, 'nn'), check=True, stdout=subprocess.DEVNULL)
t = json.load(open(out))
names = [v.split('=')[0] for v in variants]
print(f"{'family':18} {'n':>4} {'best teacher':>20}  " + ' '.join(f'{v:>7}' for v in names))
for r in t['rows']:
    cells = ' '.join(f"{r['rates'].get(v, float('nan')):6.2f}{'*' if r['pass'].get(v) else ' '}" for v in names)
    print(f"{r['family']:18} {r['n']:4} {r['best']:>14}:{r['best_rate']:.2f}  {cells}")
print({v: t['summary'][v]['passed'] for v in names}, 'of', t['summary']['families'], 'families at >= 90 % of the best teacher')
