"""Reading rows.jsonl files of trainer runs into paired per-family data (standard library only).

A rows file has one play per line: family, era, instance, attempt, player, win, info{...}. Only the kept attempt of
a scene counts (kept.jsonl: family, era, instance, attempt), for instances in [lo, hi). A play is identified by its
unit (era, instance) inside a family; the prey family is split into prey-land / prey-either by info.context, as
aurelia_nn.army.table does. Several rows files of one arm are several seeds: the i-th file is seed i, and the
A and B plays are paired seed by seed."""
import collections
import json
import os

# The families whose scene depends on the scene rule version (one list, shared with the Rust tools: the qa test
# `the_families_that_depend_on_the_rule_version_are_the_listed_ones` proves it). Rows, records and kept sets of these
# are comparable only inside one rule version.
DEPENDENT_JSON = os.environ.get(
    'GATESTATS_RULE_DEPENDENT',
    os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'crates', 'qa', 'src', 'trainers', 'rule_dependent.json'))


# The versions a row carries (`info`), and which families each one makes incomparable. `rule_version`: the scene rule (the
# listed families only). `obs_version`: what the army net observes (1: the full state of the game; 2: only what its side
# knows, `known.rs`): it changes every input of the net, so it applies to every family, but only to the plays of the net.
VERSION_FIELDS = {'rule_version': 'listed', 'obs_version': 'all'}


class RuleVersionError(Exception):
    """Rows (or a kept set) recorded under different scene rule versions (or different observation versions), or without a
    version where the scene (the net's input) depends on it."""


def dependent_families():
    with open(DEPENDENT_JSON) as fh:
        return set(json.load(fh)['families'])


def row_version(r, field='rule_version'):
    """info.<field> of a row as int, or None when the row carries none (a file recorded before the field existed)."""
    v = r.get('info', {}).get(field)
    return None if v is None else int(v)


def arm_versions(paths, legacy=None, player='army-net', families=None, field='rule_version'):
    """-> ({family: version}, {kept_id}) of the plays of `player` in the rows files, for the families `field` makes
    incomparable (rule_version: the listed ones; obs_version: all). A row without the field counts as `legacy` (an explicit
    opt-in that names the version of an old file); with no `legacy` it is an error. Two versions inside one family are an
    error."""
    dep = None if VERSION_FIELDS[field] == 'all' and families is None else (dependent_families() if families is None else set(families))
    out, kept_ids = {}, set()
    for p in paths:
        with open(p) as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                r = json.loads(line)
                if (dep is not None and r.get('family') not in dep) or (player and r.get('player') != player):
                    continue
                v = row_version(r, field)
                if v is None:
                    if legacy is None:
                        flag = '--legacy-version N, or legacy_version' if field == 'rule_version' else '--legacy-obs-version N, or legacy_obs_version'
                        raise RuleVersionError(
                            f"{p}: a {r['family']} row has no {field} (recorded before the field existed). "
                            f"Its scene depends on the {field.replace('_', ' ')}: name the version it was recorded under "
                            f"(legacy version 1 or 2; {flag} in the inputs) or replay it.")
                    v = int(legacy)
                if out.setdefault(r['family'], v) != v:
                    raise RuleVersionError(f"{p}: {r['family']} rows of {field.replace('_', ' ')}s {out[r['family']]} and {v} in one arm")
                if 'kept_id' in r.get('info', {}):
                    kept_ids.add(r['info']['kept_id'])
    return out, kept_ids


def require_same_version(label_a, va, label_b, vb, field='rule_version'):
    """Refuse a comparison of two arms recorded under different versions (`field`) in a family that depends on it."""
    name = field.replace('_', ' ')
    why = ('the same (era, instance, attempt) is another scene' if field == 'rule_version'
           else 'the net saw another input (the full state of the game, or only what its side knows)')
    for fam in sorted(set(va[0]) & set(vb[0])):
        if va[0][fam] != vb[0][fam]:
            raise RuleVersionError(
                f"{fam}: {label_a} was recorded under {name} {va[0][fam]}, {label_b} under {name} {vb[0][fam]}: "
                f"{why}, the rows are not comparable")
    if va[1] and vb[1] and va[1] != vb[1]:
        raise RuleVersionError(f"{label_a} and {label_b} were played from different kept sets (kept_id {sorted(va[1])} vs {sorted(vb[1])})")


def load_kept(path, lo=80, hi=100, versions=None):
    """The kept attempts of the instances in [lo, hi). `versions`: {family: rule version} of the rows the set filters; a
    rule-dependent family whose kept entries name another version is an error (entries of an old file name none: not checked
    here, the rows' own versions are, and the file's name tells the version)."""
    kept = {}
    with open(path) as fh:
        for line in fh:
            k = json.loads(line)
            if k.get('attempt') is not None and lo <= k['instance'] < hi:
                kept[(k['family'], k['era'], k['instance'])] = k['attempt']
                kv = k.get('rule_version')
                if versions and kv is not None and versions.get(k['family'], kv) != kv:
                    raise RuleVersionError(f"{path}: {k['family']} kept under rule version {kv}, the rows were recorded under {versions[k['family']]}")
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
