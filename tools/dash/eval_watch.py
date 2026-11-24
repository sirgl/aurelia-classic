"""Per-epoch PLAY evaluation of the army net.
  python3 -I eval_watch.py watch HOURS   every 2 min: evaluate each unevaluated checkpoint of every run with
                                         ckpt/index.jsonl (newest first, one at a time) through the broker;
                                         append one line per checkpoint to runs/<run>/evals.jsonl
  python3 -I eval_watch.py backfill      final points of the finished variants from their full-eval rows
  python3 -I eval_watch.py teacher       (re)write teacher_ref.json
The quick eval is the one of M-HANDOFF: 12 families, held-out instances, player army-net."""
import glob, json, os, shutil, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
ARMY = os.environ.get('AURELIA_LAB') or REPO + '/lab'  # runs/, data/ of the experiments
RUNS = ARMY + '/runs'
BIN = os.environ.get('AURELIA_EXAM_BIN') or REPO + '/target/release/ai-exam'
RES = REPO + '/tools/res/res'
DAEMON = ARMY + '/eval-watch'  # its eval outputs (out/<run>-e<epoch>); was undefined in the kit: NameError every pass
os.makedirs(ARMY + '/dash', exist_ok=True)
TEACHER_REF = ARMY + '/dash/teacher_ref.json'
SIEGE = ['siege', 'ram', 'assault-heal', 'city-fire', 'hill-archers', 'planned-assault', 'promotion-choice', 'rush']
CAP7 = [f for f in SIEGE if f != 'rush']  # rush is a defence family: no city is taken in it
FAMS12 = SIEGE + ['raid', 'prey-land', 'focus', 'snipe']
QUICK_FAMS = 'siege,ram,assault-heal,city-fire,hill-archers,planned-assault,promotion-choice,rush,raid,prey,focus,snipe'


def fam_of(r):
    if r['family'] == 'prey':
        return 'prey-land' if r['info'].get('context') == 'land' else 'prey-either'
    return r['family']


def read_rows(path):
    return [json.loads(l) for l in open(path) if l.strip()]


def per_family(rows):
    """family -> {n, win, capture, lost, dmg_own, rounds_cap} for rows of one player."""
    by = {}
    for r in rows:
        by.setdefault(fam_of(r), []).append(r)
    out = {}
    for f, rs in by.items():
        n = len(rs)
        m = lambda r, k: r['measures'].get(k)
        tk = [r for r in rs if m(r, 'taken') is not None]
        cap = [r for r in tk if m(r, 'taken') > 0]
        dm = [m(r, 'dmg_own') for r in rs if m(r, 'dmg_own') is not None]
        out[f] = {
            'n': n,
            'win': 100.0 * sum(1 for r in rs if r['win']) / n,
            'capture': 100.0 * len(cap) / len(tk) if tk else None,
            'lost': sum(m(r, 'lost') or 0.0 for r in rs) / n,
            'dmg_own': sum(dm) / len(dm) if dm else None,
            'rounds_cap': sum(r['rounds'] for r in cap) / len(cap) if cap else None,
            'n_cap': len(cap),
        }
    return out


def mean(xs):
    xs = [x for x in xs if x is not None]
    return sum(xs) / len(xs) if xs else None


def teacher_best():
    """family -> (teacher, rate) of the latest table; prey split included."""
    tabs = sorted(glob.glob(RUNS + '/table-*.json'), key=os.path.getmtime, reverse=True)
    for t in tabs:
        try:
            d = json.load(open(t))
        except Exception:
            continue
        b = {r['family']: (r['best'], r['best_rate']) for r in d['rows']}
        if all(f in b for f in FAMS12):
            return b
    raise SystemExit('no table with the 12 families')


def teacher_ref():
    if os.path.exists(TEACHER_REF):
        return json.load(open(TEACHER_REF))
    best = teacher_best()
    rows = read_rows(RUNS + '/eval-base/rows.jsonl') + read_rows(RUNS + '/teach4-hold/rows.jsonl')
    ref = {}
    for f in FAMS12:
        tn, rate = best[f]
        rs = [r for r in rows if r['player'] == tn and fam_of(r) == f]
        pf = per_family(rs).get(f, {})
        pf['teacher'] = tn
        pf['best_rate'] = rate
        ref[f] = pf
    ref['_siege'] = summarise_siege({f: ref[f] for f in SIEGE})
    json.dump(ref, open(TEACHER_REF, 'w'), indent=1)
    return ref


def summarise_siege(pf):
    return {
        'capture': mean([pf[f]['capture'] for f in CAP7 if f in pf]),
        'lost': mean([pf[f]['lost'] for f in SIEGE if f in pf]),
        'rounds_cap': mean([pf[f]['rounds_cap'] for f in CAP7 if f in pf]),
        'dmg_own': mean([pf[f]['dmg_own'] for f in SIEGE if f in pf]),
        'win': mean([pf[f]['win'] for f in SIEGE if f in pf]),
    }


def evaluate_rows(rows, epoch, run, **extra):
    pf = per_family([r for r in rows if r['player'] == 'army-net'])
    best = teacher_best()
    ok12 = [f for f in FAMS12 if f in pf and pf[f]['win'] / 100.0 >= 0.9 * best[f][1]]
    line = {'run': run, 'epoch': epoch, 'families': pf,
            'siege': summarise_siege(pf), 'fam_ok': len(ok12), 'fam_n': len([f for f in FAMS12 if f in pf]),
            'fam_ok_list': ok12}
    line.update(extra)
    return line


def backfill():
    specs = [('A2', 'evalall-A2', 'netA2'), ('M0', 'evalall-M0', 'netM0'), ('M1', 'evalall-M1', 'netM1'),
             ('MH0', 'evalall-MH0', 'netMH0'), ('MB0', 'evalall-MB0', 'netMB0')]
    for name, ev, net in specs:
        rows = read_rows(f'{RUNS}/{ev}/rows.jsonl')
        cur = [json.loads(l) for l in open(f'{RUNS}/{net}/curves.jsonl') if l.strip()]
        epoch = cur[-1]['epoch'] if cur else None
        rows12 = [r for r in rows if fam_of(r) in FAMS12 and r['player'] == 'army-net']
        line = evaluate_rows(rows12, round(epoch, 3) if epoch else None, name, backfill=True, source=ev)
        # the same count on all 32 families (raw family names, teachers.json of the net)
        tj = json.load(open(f'{RUNS}/{net}/teachers.json'))['best']
        n_ok = 0
        for f, (_, rate) in tj.items():
            sub = [r for r in rows if r['family'] == f and r['player'] == 'army-net']
            if sub and 100.0 * sum(r['win'] for r in sub) / len(sub) / 100.0 >= 0.9 * rate:
                n_ok += 1
        line['fam_ok32'] = n_ok
        line['fam_n32'] = len(tj)
        d = f'{RUNS}/{net}'
        os.makedirs(d, exist_ok=True)
        with open(d + '/evals.jsonl', 'a+') as fh:
            fh.seek(0)
            have = [json.loads(l) for l in fh if l.strip()]
            if any(h.get('backfill') and h['run'] == name for h in have):
                print('skip', name)
                continue
            fh.write(json.dumps(line) + '\n')
        print(name, line['epoch'], line['fam_ok'], line['fam_n'], line['fam_ok32'], line['fam_n32'],
              round(line['siege']['capture'], 1))


def run_one(run, ent):
    path = ent['path']
    if not os.path.isabs(path):
        path = os.path.join(RUNS, run, path) if not os.path.exists(path) else os.path.abspath(path)
    binp = path.rsplit('.', 1)[0] + '.bin'
    if not os.path.exists(binp) or time.time() - os.path.getmtime(binp) < 10:
        return None
    tag = f"{run}-e{ent['epoch']}"
    out = f'{DAEMON}/out/{tag}'
    shutil.rmtree(out, ignore_errors=True)
    cmd = (f"cd {ARMY} && ARMY_NET={binp} {BIN} suite --kept data/kept-m.jsonl --from 80 --instances 100 "
           f"--families {QUICK_FAMS} --players army-net --out {out} --jobs $RES_THREADS")
    t0 = time.time()
    r = subprocess.run([RES, 'run', '--cpu', '2-' + str(max(2, int(subprocess.run(['sysctl', '-n', 'hw.perflevel0.physicalcpu'], capture_output=True, text=True).stdout.strip() or 4) - 1)), '--name', 'eval-watch', '--', 'sh', '-c', cmd],
                       capture_output=True, text=True)
    wall = time.time() - t0
    rp = out + '/rows.jsonl'
    if not os.path.exists(rp):
        print(time.strftime('%H:%M:%S'), 'no rows for', tag, r.stderr[-300:], flush=True)
        return None
    rows = read_rows(rp)
    shutil.rmtree(out, ignore_errors=True)
    return evaluate_rows(rows, ent['epoch'], run, wall_s=round(wall, 1), ckpt=path, backfill=False)


def done_epochs(run):
    p = f'{RUNS}/{run}/evals.jsonl'
    if not os.path.exists(p):
        return set()
    return {json.loads(l)['epoch'] for l in open(p) if l.strip() and not json.loads(l).get('backfill')}


def tick():
    for ip in sorted(glob.glob(RUNS + '/*/ckpt/index.jsonl')):
        run = ip.split('/')[-3]
        ents = [json.loads(l) for l in open(ip) if l.strip()]
        while True:
            done = done_epochs(run)
            todo = sorted([e for e in ents if e['epoch'] not in done], key=lambda e: -e['epoch'])
            tried = False
            for e in todo:
                line = run_one(run, e)
                if line:
                    with open(f'{RUNS}/{run}/evals.jsonl', 'a') as fh:
                        fh.write(json.dumps(line) + '\n')
                    print(time.strftime('%H:%M:%S'), run, 'epoch', e['epoch'], 'fams', line['fam_ok'], '/', line['fam_n'],
                          'siege capture %.1f' % line['siege']['capture'], 'wall', line['wall_s'], flush=True)
                    tried = True
                    break
            if not tried:
                break
            ents = [json.loads(l) for l in open(ip) if l.strip()]


def watch(hours):
    end = time.time() + hours * 3600
    print(time.strftime('%H:%M:%S'), 'eval_watch start, pid', os.getpid(), 'until', time.strftime('%H:%M', time.localtime(end)), flush=True)
    while time.time() < end:
        try:
            tick()
        except Exception as ex:
            print(time.strftime('%H:%M:%S'), 'error', repr(ex), flush=True)
        time.sleep(120)
    print('eval_watch end', flush=True)


if __name__ == '__main__':
    cmd = sys.argv[1] if len(sys.argv) > 1 else 'watch'
    if cmd == 'watch':
        watch(float(sys.argv[2]) if len(sys.argv) > 2 else 12)
    elif cmd == 'backfill':
        teacher_ref()
        backfill()
    elif cmd == 'teacher':
        if os.path.exists(TEACHER_REF):
            os.remove(TEACHER_REF)
        json.dump(teacher_ref(), sys.stdout, indent=1)
