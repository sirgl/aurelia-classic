"""Function-preserving check: are two quick-eval runs the same play by play?
python3 tools/m5/py/same_plays.py A/rows.jsonl B/rows.jsonl  -> equal / total, and the first differences.
A play is compared on its verdict, rounds and every measure except the timings."""
import json, sys
def key(r):
    return (r['family'], r['era'], r['instance'], r['attempt'])
def sig(r):
    m = {k: v for k, v in r['measures'].items()}
    return (r['win'], r['rounds'], tuple(sorted(m.items())))
a = {key(r): r for r in map(json.loads, open(sys.argv[1])) if r['player'] == 'army-net'}
b = {key(r): r for r in map(json.loads, open(sys.argv[2])) if r['player'] == 'army-net'}
common = sorted(set(a) & set(b))
diff = [k for k in common if sig(a[k]) != sig(b[k])]
print(f'equal {len(common) - len(diff)} / {len(common)} (only in A {len(set(a) - set(b))}, only in B {len(set(b) - set(a))})')
for k in diff[:5]:
    print(k, sig(a[k]), sig(b[k]))
sys.exit(1 if diff or set(a) != set(b) else 0)
