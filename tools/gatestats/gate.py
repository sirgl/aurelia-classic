#!/usr/bin/env python3
"""The shared gate-statistics tool (standard library only).

  python3 gate.py report --inputs inputs.json --spec spec.json --out DIR    # DIR/report.md and DIR/report.json
  python3 gate.py power  --n 140 [160 ...] --m 40 [--q 0.10] [--alpha 0.05] [--p0 0.8]

See README.md for the formats of inputs.json and spec.json."""
import argparse
import fnmatch
import json
import math
import os
import random
import sys
from operator import itemgetter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rows as R  # noqa: E402
import stats as S  # noqa: E402

DEFAULT_SPEC = {'alpha': 0.05, 'min_n': 10, 'ci_level': 0.95, 'bootstrap_reps': 4000, 'bootstrap_seed': 1, 'seed_z': 2.0}


# ---- one family ----------------------------------------------------------------------------------

def family_stats(pair, seed_z=2.0):
    seeds = pair['seeds']
    n11 = sum(c[0] for c in seeds.values())
    n10 = sum(c[1] for c in seeds.values())
    n01 = sum(c[2] for c in seeds.values())
    n00 = sum(c[3] for c in seeds.values())
    n = n11 + n10 + n01 + n00
    a, b = n11 + n10, n11 + n01
    mc = S.mcnemar_exact(n10, n01)
    fi = S.fisher_exact(a, n, b, n)
    lo, hi = S.newcombe_paired(n11, n10, n01, n00)
    out = {'n': n, 'a_wins': a, 'b_wins': b, 'a_rate': a / n if n else 0.0, 'b_rate': b / n if n else 0.0,
           'a_ci': S.wilson(a, n), 'b_ci': S.wilson(b, n),
           'delta': (b - a) / n if n else 0.0, 'delta_ci': (lo, hi),
           'b_worse': n10, 'b_better': n01, 'p_two': mc['p_two'], 'p_worse': mc['p_worse'], 'p_better': mc['p_better'],
           'fisher_p_two': fi['p_two'], 'dropped': pair['dropped']}
    per = []
    for s in sorted(seeds):
        c = seeds[s]
        ns = sum(c)
        d = (c[2] - c[1]) / ns if ns else 0.0
        var = max((c[1] + c[2] - (c[2] - c[1]) ** 2 / ns) / ns ** 2, 0.0) if ns else 0.0
        per.append({'seed': s, 'n': ns, 'a_rate': (c[0] + c[1]) / ns if ns else 0.0, 'b_rate': (c[0] + c[2]) / ns if ns else 0.0,
                    'delta': d, 'var': var, 'p_two': S.mcnemar_exact(c[1], c[2])['p_two']})
    out['seeds'] = per
    out['seed_disagree'] = False
    out['seed_sd_b'] = None
    if len(per) >= 2:
        hi_s, lo_s = max(per, key=lambda x: x['delta']), min(per, key=lambda x: x['delta'])
        se = math.sqrt(hi_s['var'] + lo_s['var'])
        z = (hi_s['delta'] - lo_s['delta']) / se if se > 0 else (math.inf if hi_s['delta'] > lo_s['delta'] else 0.0)
        out['seed_z'] = z
        out['seed_disagree'] = z > seed_z
        rb = [x['b_rate'] for x in per]
        m = sum(rb) / len(rb)
        out['seed_sd_b'] = math.sqrt(sum((x - m) ** 2 for x in rb) / (len(rb) - 1))
    return out


# ---- the gate ------------------------------------------------------------------------------------

def _match(name, patterns):
    return any(fnmatch.fnmatch(name, p) for p in patterns or [])


def _in(name, slice_name, cfg):
    sl = cfg.get('slices')
    return (sl is None or slice_name in sl) and not _match(name, cfg.get('exclude'))


def _bootstrap(fams, mean_set, count_set, bars, frac, reps, seed, level):
    """Paired bootstrap over units (instances) inside every family. fams: name -> clusters."""
    rng = random.Random(seed)
    cols = {}
    for name, cl in fams.items():
        cols[name] = ([c[0] for c in cl], [c[1] for c in cl], [c[2] for c in cl], len(cl))
    mean_d, cnt_a, cnt_b = [], [], []
    for _ in range(reps):
        m_sum, ca, cb = 0.0, 0, 0
        for name, (aw, bw, nn, m) in cols.items():
            idx = rng.choices(range(m), k=m)
            if m == 1:
                sa, sb, sn = aw[0], bw[0], nn[0]
            else:
                g = itemgetter(*idx)
                sa, sb, sn = sum(g(aw)), sum(g(bw)), sum(g(nn))
            if name in mean_set:
                m_sum += (sb - sa) / sn
            if name in count_set:
                bar = bars[name]
                ca += sa / sn >= bar - 1e-12
                cb += sb / sn >= bar - 1e-12
        mean_d.append(m_sum / len(mean_set) if mean_set else 0.0)
        cnt_a.append(ca)
        cnt_b.append(cb)
    tail = (1 - level) / 2

    def ci(xs):
        xs = sorted(xs)
        return (xs[int(tail * (len(xs) - 1))], xs[int(math.ceil((1 - tail) * (len(xs) - 1)))])
    out = {}
    if mean_set:
        out['mean_delta_ci'] = ci(mean_d)
    if count_set:
        out['count_a_ci'] = ci(cnt_a)
        out['count_b_ci'] = ci(cnt_b)
        out['count_diff_ci'] = ci([b - a for a, b in zip(cnt_a, cnt_b)])
    return out


def run(inputs, spec, legacy=None, legacy_obs=None):
    """`legacy`: the rule version old rows without `rule_version` are taken to be (opt-in, names the version); a slice may carry
    legacy_version_A / legacy_version_B / legacy_version_teachers of its own. `legacy_obs`: the same for `obs_version` (what the
    army net observed: 1 the full state, 2 what its side knows); a slice may carry legacy_obs_A / legacy_obs_B."""
    sp = dict(DEFAULT_SPEC)
    sp.update(spec)
    alpha, min_n = sp['alpha'], sp['min_n']
    fams, pairs, bars, best = {}, {}, {}, {}
    slice_names = [s['name'] for s in inputs['slices']]
    for sl in inputs['slices']:
        lo, hi = sl.get('window', [80, 100])
        # The class guard: arms and teachers of a rule-dependent family are compared only inside one rule version.
        lg = sl.get('legacy_version', inputs.get('legacy_version', legacy))
        va = R.arm_versions(sl['A'], sl.get('legacy_version_A', lg))
        vb = R.arm_versions(sl['B'], sl.get('legacy_version_B', lg))
        R.require_same_version(inputs.get('A_label', 'A'), va, inputs.get('B_label', 'B'), vb)
        # The same guard for the net's input: the plays of the net are comparable only inside one observation version.
        lo_ = sl.get('legacy_obs', inputs.get('legacy_obs', legacy_obs))
        oa = R.arm_versions(sl['A'], sl.get('legacy_obs_A', lo_), field='obs_version')
        ob = R.arm_versions(sl['B'], sl.get('legacy_obs_B', lo_), field='obs_version')
        R.require_same_version(inputs.get('A_label', 'A'), oa, inputs.get('B_label', 'B'), ob, field='obs_version')
        if sl.get('teachers'):
            vt = R.arm_versions(sl['teachers'], sl.get('legacy_version_teachers', lg), player=None)
            R.require_same_version('the teachers', vt, inputs.get('B_label', 'B'), vb)
        ld = R.Loader(sl.get('kept'), lo, hi)
        allp = list(sl['A']) + list(sl['B']) + list(sl.get('teachers', []))
        ld.prescan(allp)
        arm_a, arm_b = ld.read_arm(sl['A']), ld.read_arm(sl['B'])
        teach = ld.read_teachers(sl.get('teachers', []))
        for fam in sorted(set(arm_a) & set(arm_b)):
            pr = R.pair_family(arm_a[fam], arm_b[fam])
            st = family_stats(pr, sp['seed_z'])
            if st['n'] < min_n * max(1, len(pr['seeds'])):
                continue
            name = f"{sl['name']}/{fam}"
            st['slice'], st['family'], st['name'] = sl['name'], fam, name
            bt, br = R.best_teacher(teach.get(fam, {}))
            st['best_teacher'], st['best_teacher_rate'] = bt, br
            fams[name], pairs[name] = st, pr['clusters']
            bars[name] = sp.get('count_at_bar', {}).get('frac', 0.9) * br
            best[name] = br

    # Holm / BH over the harm universe
    harm = sp.get('harm', {})
    uni, seen = [], {}
    for n, s in fams.items():   # a family repeated in several slices with identical plays counts once
        if not _in(n, s['slice'], harm):
            continue
        sig = (s['family'], s['n'], s['a_wins'], s['b_wins'], s['b_worse'], s['b_better'])
        if sig in seen:
            s['duplicate_of'] = seen[sig]
            continue
        seen[sig] = n
        uni.append(n)
    for n, s in fams.items():
        s['in_harm_set'] = n in uni
    for key, adj_key in (('p_two', 'p_two_holm'), ('p_worse', 'p_worse_holm'), ('p_better', 'p_better_holm')):
        for n, a in zip(uni, S.holm([fams[n][key] for n in uni])):
            fams[n][adj_key] = a
    for n, a in zip(uni, S.benjamini_hochberg([fams[n]['p_two'] for n in uni])):
        fams[n]['p_two_bh'] = a
    tol = harm.get('tolerance_pts', 0.0) / 100.0
    for n in uni:
        s = fams[n]
        s['status'] = 'same'
        if s['p_two_holm'] < alpha:
            s['status'] = 'up' if s['delta'] > 0 else 'down'
        s['drop'] = s['delta'] < -tol - 1e-12 and s['p_worse_holm'] < alpha
        if s['drop']:
            s['status'] = 'DROP'
        s['bh_sig'] = s['p_two_bh'] < alpha
    for n, s in fams.items():
        if n not in uni:
            s['status'] = 'same-as-' + s['duplicate_of'] if 'duplicate_of' in s else 'not-in-harm-set'
            s['drop'] = False

    gates = []
    # 1. harm
    if 'harm' in sp:
        drops = [n for n in uni if fams[n]['drop']]
        unstable = [n for n in uni if fams[n]['seed_disagree'] and fams[n]['delta'] < 0]
        tolf = harm.get('tolerance_pts', 0.0) / 100.0
        uncertain = [n for n in uni if fams[n]['delta_ci'][0] < -tolf - 1e-12 and not fams[n]['drop']]
        bad = list(drops)
        if harm.get('mode') == 'noninferiority':   # strict: a family must be shown to be within the tolerance
            bad += [n for n in uncertain]
        if harm.get('unstable_counts_as_drop'):
            bad += [n for n in unstable if n not in bad]
        gates.append({'name': 'harm', 'pass': not bad,
                      'text': f"no family dropped by more than {harm.get('tolerance_pts', 0)} points with Holm-adjusted p < {alpha}, over {len(uni)} families",
                      'detail': {'drops': drops, 'unstable_below': unstable, 'not_shown_within_tolerance': uncertain, 'mode': harm.get('mode', 'significance'), 'families': len(uni)}})
    # 2. required up
    if 'required_up' in sp:
        req = [f for f in sp['required_up'].get('families', [])]
        miss = [f for f in req if f not in fams]
        have = [f for f in req if f in fams]
        adj = S.holm([fams[f]['p_better'] for f in have]) if have else []
        gain = sp['required_up'].get('min_gain_pts', 0.0) / 100.0
        res = {}
        for f, a in zip(have, adj):
            fams[f]['p_better_req_holm'] = a
            res[f] = bool(fams[f]['delta'] > gain and a < alpha)
        gates.append({'name': 'required_up', 'pass': not miss and all(res.values()),
                      'text': f"every required family up by more than {sp['required_up'].get('min_gain_pts', 0)} points with Holm-adjusted p < {alpha} (Holm over the required set)",
                      'detail': {'per_family': res, 'missing': miss}})
    # 2b. rate floor: the candidate's own win rate of the named families is at least min_rate (a point estimate, as the gate lines that say "X >= 60 %")
    if 'rate_floor' in sp:
        fl = sp['rate_floor']
        names = fl.get('families', [])
        miss = [f for f in names if f not in fams]
        rates = {f: fams[f]['b_rate'] for f in names if f in fams}
        gates.append({'name': 'rate_floor', 'pass': not miss and all(r >= fl['min_rate'] - 1e-12 for r in rates.values()),
                      'text': f"the candidate's win rate is at least {100 * fl['min_rate']:.0f} % in: {', '.join(names)}",
                      'detail': {'rates': rates, 'missing': miss}})
    # bootstrap sets
    mean_cfg, cnt_cfg = sp.get('mean'), sp.get('count_at_bar')
    def uniq(cfg, extra=lambda n: True):
        out, sg = set(), set()
        for n, s in fams.items():
            sig = (s['family'], s['n'], s['a_wins'], s['b_wins'], s['b_worse'], s['b_better'])
            if cfg is not None and _in(n, s['slice'], cfg) and extra(n) and sig not in sg:
                sg.add(sig)
                out.add(n)
        return out
    mean_set = uniq(mean_cfg)
    count_set = uniq(cnt_cfg, lambda n: best[n] > 0)
    boot_names = mean_set | count_set
    boot = {}
    if boot_names:
        boot = _bootstrap({n: pairs[n] for n in sorted(boot_names)}, mean_set, count_set, bars, None, sp['bootstrap_reps'],
                          sp['bootstrap_seed'], sp['ci_level'])
    summary = {}
    if mean_cfg is not None:
        ds = [fams[n]['delta'] for n in mean_set]
        md = sum(ds) / len(ds) if ds else 0.0
        ci = boot.get('mean_delta_ci', (0.0, 0.0))
        summary['mean'] = {'families': len(ds), 'a': sum(fams[n]['a_rate'] for n in mean_set) / max(len(ds), 1),
                           'b': sum(fams[n]['b_rate'] for n in mean_set) / max(len(ds), 1), 'delta': md, 'ci': ci}
        gates.append({'name': 'mean', 'pass': bool(ds) and ci[0] > mean_cfg.get('min_gain_pts', 0.0) / 100.0,
                      'text': f"mean over {len(ds)} families up: paired bootstrap {int(sp['ci_level'] * 100)} % CI lower bound above {mean_cfg.get('min_gain_pts', 0.0)} points",
                      'detail': summary['mean']})
    if cnt_cfg is not None:
        def cnt(key):
            return sum(1 for n in count_set if fams[n][key] / fams[n]['n'] >= bars[n] - 1e-12)
        ca, cb = cnt('a_wins'), cnt('b_wins')
        summary['count'] = {'families': len(count_set), 'a': ca, 'b': cb, 'diff': cb - ca, 'frac': cnt_cfg.get('frac', 0.9),
                            'a_ci': boot['count_a_ci'], 'b_ci': boot['count_b_ci'], 'diff_ci': boot['count_diff_ci']}
        okc = cb - ca >= cnt_cfg.get('min_gain', 1)
        if cnt_cfg.get('require_ci'):
            okc = okc and boot['count_diff_ci'][0] > 0
        gates.append({'name': 'count_at_bar', 'pass': okc,
                      'text': f"families at >= {int(100 * cnt_cfg.get('frac', 0.9))} % of the best teacher, of {len(count_set)}: at least {cnt_cfg.get('min_gain', 1)} more than the reference",
                      'detail': summary['count']})
    # power
    power = []
    m_all = max(len(uni), 1)
    for n in sorted(fams):
        s = fams[n]
        q = (s['b_worse'] + s['b_better']) / s['n'] if s['n'] else 0.0
        a_adj = alpha / m_all
        mde_p = S.mde_paired(s['n'], q, a_adj) if q > 0 else None
        mde_u = S.mde_unpaired(s['n'], max(s['a_rate'], 0.5), a_adj)
        s['mde_paired'], s['mde_unpaired'], s['discordant'] = mde_p, mde_u, q
    counts = {'up': 0, 'down': 0, 'same': 0, 'DROP': 0}
    for n in uni:
        counts[fams[n]['status']] += 1
    summary['status_counts'] = counts
    summary['bh'] = {'significant': sum(1 for n in uni if fams[n]['bh_sig']), 'families': len(uni)}
    summary['seed_disagree'] = [n for n in fams if fams[n]['seed_disagree']]
    ok = all(g['pass'] for g in gates) if gates else None
    return {'name': sp.get('name', ''), 'A': inputs.get('A_label', 'A'), 'B': inputs.get('B_label', 'B'), 'spec': sp,
            'slices': slice_names, 'pass': ok, 'gates': gates, 'summary': summary, 'families': fams, 'harm_set': uni}


# ---- markdown ------------------------------------------------------------------------------------

def _pct(x, d=1):
    return f'{100 * x:.{d}f}'


def _p(x):
    return '<0.001' if x < 0.001 else f'{x:.3f}'


def markdown(rep):
    A, B = rep['A'], rep['B']
    L = [f"# Gate report: {B} vs {A}" + (f" ({rep['name']})" if rep['name'] else ''), '']
    verdict = {True: 'PASS', False: 'FAIL', None: 'no gates in the spec'}[rep['pass']]
    L += [f"**Verdict: {verdict}**  (reference A = {A}, candidate B = {B}; delta = B minus A; alpha {rep['spec']['alpha']}, Holm across families)", '']
    L += ['| gate | result | what |', '|---|---|---|']
    for g in rep['gates']:
        L.append(f"| {g['name']} | {'pass' if g['pass'] else 'FAIL'} | {g['text']} |")
    L.append('')
    sm = rep['summary']
    if 'mean' in sm:
        m = sm['mean']
        L.append(f"- Mean over {m['families']} families: {_pct(m['a'])} -> {_pct(m['b'])}, delta {100 * m['delta']:+.1f} points, paired bootstrap CI [{100 * m['ci'][0]:+.1f}, {100 * m['ci'][1]:+.1f}].")
    if 'count' in sm:
        c = sm['count']
        L.append(f"- Families at >= {int(100 * c['frac'])} % of the best teacher, of {c['families']}: {c['a']} (CI {c['a_ci'][0]}-{c['a_ci'][1]}) -> {c['b']} (CI {c['b_ci'][0]}-{c['b_ci'][1]}); difference {c['diff']:+d}, CI [{c['diff_ci'][0]:+d}, {c['diff_ci'][1]:+d}]. The bar is fixed at the point estimate of the best teacher.")
    sc = sm['status_counts']
    L.append(f"- Families (harm set, {len(rep['harm_set'])}): up {sc['up']}, down {sc['down']} (of which beyond tolerance: DROP {sc['DROP']}), unchanged {sc['same']}. Benjamini-Hochberg at {rep['spec']['alpha']}: {sm['bh']['significant']} families differ.")
    if sm['seed_disagree']:
        L.append(f"- Seeds disagree on: {', '.join(sm['seed_disagree'])}.")
    for g in rep['gates']:
        d = g['detail']
        if g['name'] == 'harm' and d['drops']:
            L.append(f"- DROP: {', '.join(d['drops'])}.")
        if g['name'] == 'harm' and d['not_shown_within_tolerance']:
            L.append(f"- Not shown to be within the tolerance (the 95 % CI of the delta reaches below -{rep['spec']['harm'].get('tolerance_pts', 0)} points; {'these FAIL the gate' if d['mode'] == 'noninferiority' else 'informational'}): {len(d['not_shown_within_tolerance'])} of {d['families']}: {', '.join(d['not_shown_within_tolerance'])}.")
        if g['name'] == 'harm' and d['unstable_below']:
            L.append(f"- Unstable and below (seeds disagree, delta < 0): {', '.join(d['unstable_below'])}.")
        if g['name'] == 'rate_floor':
            L.append('- Rate floor: ' + ', '.join(f"{k} {100 * v:.1f} %" for k, v in d['rates'].items()) + (f"; missing: {d['missing']}" if d['missing'] else '') + '.')
        if g['name'] == 'required_up':
            L.append('- Required families: ' + ', '.join(f"{k} {'up' if v else 'NOT up'}" for k, v in d['per_family'].items()) + (f"; missing: {d['missing']}" if d['missing'] else '') + '.')
    L.append('')
    multi = any(s['seeds'] and len(s['seeds']) > 1 for s in rep['families'].values())
    L += ['## Families', '', 'b = plays A won and B lost, c = the reverse. p = exact McNemar (two-sided); Holm = adjusted across the harm set; p-worse is one-sided (H1: B worse). MDE = smallest drop the paired test finds at 80 % power at level alpha/m (Bonferroni bound of Holm), given the observed discordance; - = too few discordant plays to find any drop.', '']
    hdr = '| family | n | A % | B % | delta (CI) | b / c | p | p Holm | p-worse Holm | MDE | status |' + (' seeds (delta per seed) |' if multi else '')
    L += [hdr, '|' + '---|' * (hdr.count('|') - 1)]
    order = sorted(rep['families'].values(), key=lambda s: s['delta'])
    for s in order:
        mde = '-' if s['mde_paired'] is None else f"{100 * s['mde_paired']:.1f}"
        row = (f"| {s['name']} | {s['n']} | {_pct(s['a_rate'])} | {_pct(s['b_rate'])} | {100 * s['delta']:+.1f} [{100 * s['delta_ci'][0]:+.1f}, {100 * s['delta_ci'][1]:+.1f}] "
               f"| {s['b_worse']} / {s['b_better']} | {_p(s['p_two'])} | {_p(s.get('p_two_holm', 1.0)) if s['in_harm_set'] else '-'} | {_p(s.get('p_worse_holm', 1.0)) if s['in_harm_set'] else '-'} | {mde} | {s['status']}{' (seeds disagree)' if s['seed_disagree'] else ''}{f" [{s['dropped']} plays unpaired]" if s['dropped'] else ''} |")
        if multi:
            row += ' ' + ', '.join(f"{100 * x['delta']:+.1f}" for x in s['seeds']) + ' |'
        L.append(row)
    L.append('')
    return '\n'.join(L)


# ---- CLI -----------------------------------------------------------------------------------------

def cmd_power(a):
    qs = a.q
    print(f"alpha {a.alpha}, m = {a.m} families -> per-family one-sided level {a.alpha / a.m:.5f} (Bonferroni bound of Holm), power {a.power}")
    print('smallest drop in points found with that power (paired exact McNemar at discordance q; unpaired normal approx at baseline p0):')
    print(f"{'n plays':>8} " + ' '.join(f'q={q:<5}' for q in qs) + f"  unpaired(p0={a.p0})")
    for n in a.n:
        cells = []
        for q in qs:
            d = S.mde_paired(n, q, a.alpha / a.m, a.power)
            cells.append(f"{'n/a':>7}" if d is None else f"{100 * d:7.1f}")
        print(f"{n:8d} " + ' '.join(cells) + f"  {100 * S.mde_unpaired(n, a.p0, a.alpha / a.m, a.power):7.1f}")


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest='cmd', required=True)
    r = sub.add_parser('report')
    r.add_argument('--inputs', required=True)
    r.add_argument('--spec', required=True)
    r.add_argument('--out', required=True)
    r.add_argument('--legacy-version', type=int, choices=(1, 2), default=None,
                   help='opt-in: rows without rule_version of a rule-dependent family (guard, convoy) were recorded under this version')
    r.add_argument('--legacy-obs-version', type=int, choices=(1, 2), default=None,
                   help='opt-in: rows without obs_version (the net observed this version: 1 = the full state, 2 = what its side knows)')
    p = sub.add_parser('power')
    p.add_argument('--n', type=int, nargs='+', required=True)
    p.add_argument('--m', type=int, default=1)
    p.add_argument('--q', type=float, nargs='+', default=[0.05, 0.10, 0.20])
    p.add_argument('--alpha', type=float, default=0.05)
    p.add_argument('--power', type=float, default=0.8)
    p.add_argument('--p0', type=float, default=0.8)
    a = ap.parse_args()
    if a.cmd == 'power':
        return cmd_power(a)
    try:
        rep = run(json.load(open(a.inputs)), json.load(open(a.spec)), a.legacy_version, a.legacy_obs_version)
    except R.RuleVersionError as e:
        print(f'REFUSED: {e}', file=sys.stderr)
        return 2
    os.makedirs(a.out, exist_ok=True)
    json.dump(rep, open(os.path.join(a.out, 'report.json'), 'w'), indent=1, default=list)
    md = markdown(rep)
    open(os.path.join(a.out, 'report.md'), 'w').write(md)
    print(md)
    return 0


if __name__ == '__main__':
    sys.exit(main())
