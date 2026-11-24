"""Reading rows.jsonl files of trainer runs into paired per-family data (standard library only).

A rows file has one play per line: family, era, instance, attempt, player, win, info{...}. Only the kept attempt of
a scene counts (kept.jsonl: family, era, instance, attempt), for instances in [lo, hi). A play is identified by its
unit (era, instance) inside a family; the prey family is split into prey-land / prey-either by info.context, as
aurelia_nn.army.table does. Several rows files of one arm are several seeds: the i-th file is seed i, and the
A and B plays are paired seed by seed."""
import collections
import json


def load_kept(path, lo=80, hi=100):
    kept = {}
    with open(path) as fh:
        for line in fh:
            k = json.loads(line)
            if k.get('attempt') is not None and lo <= k['instance'] < hi:
                kept[(k['family'], k['era'], k['instance'])] = k['attempt']
    return kept


class Loader:
    def __init__(self, kept_path=None, lo=80, hi=100):
        self.kept = load_kept(kept_path, lo, hi) if kept_path else None
        self.lo, self.hi = lo, hi
        self.ctx = {}

    def _rows(self, path):
        with open(path) as fh:
            for line in fh:
                line = line.strip()
                if line:
                    yield json.loads(line)

    def prescan(self, paths):
        """Collect the prey contexts of every file first, so the split does not depend on the file order."""
        for p in paths:
            for r in self._rows(p):
                if r['family'] == 'prey':
                    c = r.get('info', {}).get('context')
                    if c is not None:
                        self.ctx[(r['family'], r['era'], r['instance'])] = c

    def _accept(self, r):
        key = (r['family'], r['era'], r['instance'])
        if self.kept is not None:
            return self.kept.get(key) == r['attempt']
        return self.lo <= r['instance'] < self.hi

    def _family(self, r):
        if r['family'] != 'prey':
            return r['family']
        c = r.get('info', {}).get('context')
        c = c if c is not None else self.ctx.get((r['family'], r['era'], r['instance']))
        return 'prey-land' if c == 'land' else 'prey-either'

    def read_arm(self, paths, player='army-net'):
        """-> {family: {unit: {slot: win}}} where slot = (seed index, running index of the play in its file)."""
        arm = collections.defaultdict(lambda: collections.defaultdict(dict))
        for seed, p in enumerate(paths):
            seen = collections.Counter()
            for r in self._rows(p):
                if r.get('player') != player or not self._accept(r):
                    continue
                fam, unit = self._family(r), (r['era'], r['instance'])
                slot = (seed, seen[(fam, unit)])
                seen[(fam, unit)] += 1
                arm[fam][unit][slot] = bool(r['win'])
        return arm

    def read_teachers(self, paths):
        """-> {family: {teacher: (wins, plays)}} for the players ref-* and unciv."""
        t = collections.defaultdict(lambda: collections.defaultdict(lambda: [0, 0]))
        for p in paths:
            for r in self._rows(p):
                pl = r.get('player', '')
                if not (pl.startswith('ref-') or pl == 'unciv') or not self._accept(r):
                    continue
                c = t[self._family(r)][pl]
                c[0] += bool(r['win'])
                c[1] += 1
        return {f: {k: tuple(v) for k, v in d.items()} for f, d in t.items()}


def best_teacher(teachers):
    """(name, rate) of the best teacher of one family, ties to a reference over unciv; (None, 0.0) if none."""
    if not teachers:
        return (None, 0.0)
    name = max(teachers, key=lambda k: (teachers[k][0] / teachers[k][1], k != 'unciv'))
    return name, teachers[name][0] / teachers[name][1]


def pair_family(arm_a, arm_b):
    """Pair the plays of A and B of one family.
    -> {'clusters': [(a_wins, b_wins, plays)] per unit, 'seeds': {seed: [n11, n10, n01, n00]}, 'dropped': plays without a partner}."""
    clusters, seeds, dropped = [], collections.defaultdict(lambda: [0, 0, 0, 0]), 0
    for unit in sorted(set(arm_a) | set(arm_b)):
        pa, pb = arm_a.get(unit, {}), arm_b.get(unit, {})
        both = set(pa) & set(pb)
        dropped += len(pa) + len(pb) - 2 * len(both)
        aw = bw = 0
        for slot in both:
            a, b = pa[slot], pb[slot]
            aw += a
            bw += b
            seeds[slot[0]][0 if (a and b) else 1 if a else 2 if b else 3] += 1
        if both:
            clusters.append((aw, bw, len(both)))
    return {'clusters': clusters, 'seeds': dict(seeds), 'dropped': dropped}
