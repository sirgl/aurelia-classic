#!/usr/bin/env python3 -I
"""A live page of the army net's training (Roman, 23 Nov 2026: «онлайн чарт по обучению: сколько шагов,
на чём тренируются, какие параметры»). Serves http://localhost:8765/ ; the page reloads itself every 15 s.

Reads, every request, `<runs>/*/curves.jsonl` (one JSON line per evaluation: step, epoch, train_loss,
hold{loss, main, acc_ptr, acc_act, acc_both}, dps, wall_s), `<runs>/*/config.json` if the trainer writes
it, `<runs>/*/teachers.json`, the trainer's launch script and the logs' last lines. Read-only.
"""

import html
import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
LAB = Path(os.environ.get("AURELIA_LAB") or REPO / "lab")  # runs/, logs/, data/ of the experiments
ROOT = LAB
RUNS = LAB / "runs"
LOGS = LAB / "logs"
DEFAULTS = {"bs": 512, "lr": 1e-3, "d": 64, "layers": 3, "heads": 4, "ff": 128, "lost-weight": 0.1,
            "best-factor": 2.0, "eval-every": 1000, "seed": 1, "map": False}
COLORS = ["#2f6fb3", "#c0632f", "#3f8a4a", "#8a4fb3", "#b3324f", "#5f7f8f"]


def read_curves(d: Path) -> list[dict]:
    p = d / "curves.jsonl"
    if not p.exists():
        return []
    rows = []
    for line in p.read_text().splitlines():
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    return rows


def launch_args() -> dict[str, dict]:
    """The options of each run from the launch scripts (`--out ../../runs/NAME ...`)."""
    out: dict[str, dict] = {}
    for sh in RUNS.glob("*.sh"):
        for line in sh.read_text().splitlines():
            if "army.train" not in line or "--out" not in line:
                continue
            words = line.split()
            opts = dict(DEFAULTS)
            name = None
            i = 0
            while i < len(words):
                w = words[i]
                if w.startswith("--"):
                    key = w[2:]
                    nxt = words[i + 1] if i + 1 < len(words) else ""
                    if nxt and not nxt.startswith("--") and nxt not in (">", "2>&1"):
                        opts[key] = nxt
                        i += 1
                    else:
                        opts[key] = True
                    if key == "out":
                        name = Path(opts[key]).name
                i += 1
            if name:
                out[name] = opts
    return out


def human_bytes(n: float) -> str:
    for unit in ("Б", "КБ", "МБ", "ГБ"):
        if n < 1024:
            return f"{n:.0f} {unit}" if unit == "Б" else f"{n:.1f} {unit}"
        n /= 1024
    return f"{n:.1f} ТБ"


def ru(x: float, d: int = 1) -> str:
    """Russian number format: comma decimals, space thousands, never scientific."""
    return f"{x:,.{d}f}".replace(",", "\u00a0").replace(".", ",")


def ru_pct(x: float, d: int = 0) -> str:
    return ru(x * 100, d) + "\u00a0%"


def ru_tys(x: float, d: int = 1) -> str:
    return ru(x / 1000, d) + "\u00a0тыс."


def nice_step(span: float, n: int = 4) -> float:
    raw = span / n
    e = 10 ** int(f"{raw:e}".split("e")[1])
    for m in (1, 2, 2.5, 5, 10):
        if raw <= m * e:
            return m * e
    return 10 * e


def chart(title: str, series: list[tuple[str, str, list[tuple[float, float]], dict]], kind="loss",
          target: tuple[float, str] | None = None, legend=False, note="", rlabel: int = 0) -> str:
    """series: (name, colour, points (epoch, y), style{thin, grey}). kind: loss / pct / tys.
    The run's name is written at the end of its line; a legend only when asked (many runs)."""
    series = [x for x in series if x[2]]
    if not series:
        return f"<div class='chart'><h3>{html.escape(title)}</h3><p class='mu'>данных пока нет</p></div>"
    W, H, L, B, T, R = 560, 260, 54, 34, 14, 96 if not legend else 14
    if rlabel and not legend:
        R = rlabel
    pts = [p for _, _, s, _ in series for p in s]
    live = [p for _, _, s, st in series if not st.get("ref") for p in s] or pts
    x0, x1 = 0.0, max(p[0] for p in live) or 1.0
    ys = [p[1] for p in pts] + ([target[0]] if target else [])
    lo, hi = min(ys), max(ys)
    if kind == "pct":
        lo, hi = max(0.0, (int(lo * 20) / 20) - 0.05), min(1.0, int(hi * 20) / 20 + 0.05)
        step = 0.05 if hi - lo <= 0.3 else 0.1
    elif kind == "tys":
        lo, hi = 0.0, hi * 1.1
        step = nice_step(hi - lo)
    elif kind == "int":
        lo, hi = 0.0, float(max(int(hi + 0.999), 1))
        step = 1.0 if hi <= 12 else 2.0
    else:
        step = nice_step((hi - lo) * 1.1 or 1.0)
        lo = int(lo / step) * step if lo > 0 else lo
        hi = hi + step * 0.2
    if hi <= lo:
        hi = lo + 1e-9
    def sx(x): return L + (x - x0) / (x1 - x0) * (W - L - R)
    def sy(y): return T + (1 - (y - lo) / (hi - lo)) * (H - T - B)
    def fy(v):
        return ru_pct(v) if kind == "pct" else (ru_tys(v) if kind == "tys" else (ru(v, 0) if kind == "int" else ru(v, 2)))
    g = [f"<svg viewBox='0 0 {W} {H}' role='img' aria-label='{html.escape(title)}'>"]
    k = 0
    while lo + step * k <= hi + 1e-12 and k < 20:
        yv = lo + step * k
        g.append(f"<line x1='{L}' x2='{W-R}' y1='{sy(yv):.1f}' y2='{sy(yv):.1f}' class='grid'/>"
                 f"<text x='{L-6}' y='{sy(yv)+4:.1f}' text-anchor='end' class='ax'>{fy(yv)}</text>")
        k += 1
    xstep = nice_step(x1 - x0, 5)
    xv = 0.0
    while xv <= x1 + 1e-9:
        g.append(f"<text x='{sx(xv):.1f}' y='{H-16}' text-anchor='middle' class='ax'>{ru(xv, 0 if xstep >= 1 else 1)}</text>")
        xv += xstep
    g.append(f"<text x='{(L+W-R)/2:.0f}' y='{H-3}' text-anchor='middle' class='ax'>эпохи</text>")
    if target:
        ty = sy(target[0])
        g.append(f"<line x1='{L}' x2='{W-R}' y1='{ty:.1f}' y2='{ty:.1f}' stroke='#b3324f' stroke-width='1.5' stroke-dasharray='6 4'/>"
                 f"<text x='{W-R-4}' y='{ty-4:.1f}' text-anchor='end' class='ax' fill='#b3324f' style='fill:#b3324f'>{html.escape(target[1])}</text>")
    ends = []
    for name, color, s, st in series:
        d = " ".join(f"{'M' if i == 0 else 'L'}{sx(x):.1f},{sy(y):.1f}" for i, (x, y) in enumerate(s))
        w = 1.5 if st.get("thin") else 2.6
        x, y = s[-1]
        if st.get("ref"):  # a finished net: one point, drawn as a flat reference line over the whole chart
            g.append(f"<line x1='{L}' x2='{W-R}' y1='{sy(y):.1f}' y2='{sy(y):.1f}' stroke='{color}' stroke-width='1.2' stroke-dasharray='2 3'/>")
            cx = W - R
        else:
            g.append(f"<path d='{d}' fill='none' stroke='{color}' stroke-width='{w}' stroke-linejoin='round'/>")
            cx = sx(x)
        g.append(f"<circle cx='{cx:.1f}' cy='{sy(y):.1f}' r='{2.5 if st.get('thin') else 3.5}' fill='{color}'>"
                 f"<title>{html.escape(name)}: {fy(y)} после {ru(x, 2)} эпох</title></circle>")
        ends.append([sy(y), name, color, st])
    if not legend:
        ends.sort(key=lambda e: e[0])
        for i in range(1, len(ends)):  # push labels apart so they do not collide
            if ends[i][0] - ends[i - 1][0] < 13:
                ends[i][0] = ends[i - 1][0] + 13
        for y, name, color, st in ends:
            g.append(f"<text x='{W-R+6}' y='{y+4:.1f}' font-size='11.5' font-weight='{400 if st.get('thin') else 600}' "
                     f"style='fill:{color}'>{html.escape(name)}</text>")
    g.append("</svg>")
    lg = ""
    if legend:
        lg = "<div>" + " ".join(f"<span class='lg'><i style='background:{c}'></i>{html.escape(n)}</span>" for n, c, s, _ in series) + "</div>"
    seen = f"<p class='see'><b>Что видно:</b> {html.escape(note)}</p>" if note else ""
    return f"<div class='chart'><h3>{html.escape(title)}</h3>{''.join(g)}{lg}{seen}</div>"


EVAL_STRONG = ["#2f6fb3", "#c0632f", "#3f8a4a", "#8a4fb3", "#b3324f", "#5f7f8f"]
EVAL_REFS = {"A2": "#6f6e69", "M1": "#a09e97"}   # finished variants, grey
EVAL_HIDDEN = {"M0", "MH0"}                      # backfilled, but not drawn (not the base of the B line)
EVAL_NAME = {"assault-heal": "штурм с лечением", "planned-assault": "плановый штурм"}


def read_evals() -> tuple[list[tuple[str, list[dict]]], dict[str, dict]]:
    """B-line runs with their per-checkpoint points (a backfilled run: one point) and the grey references."""
    live, refs = [], {}
    for p in sorted(RUNS.glob("net*/evals.jsonl")):
        name = p.parent.name[3:]
        pts = []
        for line in p.read_text().splitlines():
            try:
                pts.append(json.loads(line))
            except json.JSONDecodeError:
                pass
        if not pts or name in EVAL_HIDDEN:
            continue
        if name in EVAL_REFS:
            refs[name] = pts[-1]
        else:
            live.append((name, sorted(pts, key=lambda r: r["epoch"] or 0)))
    return live, refs


def evals_html() -> str:
    live, refs = read_evals()
    try:
        tref = json.loads((Path(__file__).resolve().parent / "teacher_ref.json").read_text())
    except (OSError, ValueError):
        tref = {}
    if not live and not refs:
        return "<p class='mu'>Проверки игрой ещё не было.</p>"
    col = {n: EVAL_STRONG[i % len(EVAL_STRONG)] for i, (n, _) in enumerate(live)}
    ts = tref.get("_siege", {})

    def get(r, fam, key):
        if fam is None:
            v = r.get("siege", {}).get(key)
        elif key == "fam_ok":
            v = r.get(key)
        else:
            v = r.get("families", {}).get(fam, {}).get(key)
        return v

    def series(fam, key, scale=1.0, label_suffix="", only_ref=False, thin_live=False):
        out = []
        for n, pts in live:
            xy = [(float(r["epoch"]), get(r, fam, key) * scale) for r in pts if r.get("epoch") is not None and get(r, fam, key) is not None]
            out.append((n + label_suffix, col[n], xy, {"thin": True} if thin_live else {}))
        for n, r in refs.items():
            v = get(r, fam, key)
            if v is not None and r.get("epoch") is not None:
                out.append((n + label_suffix, EVAL_REFS[n], [(float(r["epoch"]), v * scale)], {"thin": True, "ref": True}))
        return out

    def last(n_pts, fam, key, scale=1.0):
        for r in reversed(n_pts):
            v = get(r, fam, key)
            if v is not None:
                return v * scale, r["epoch"], r
        return None

    def note(fam, key, fmt, scale, teacher, what, higher_better=True, fam_label=""):
        bits = []
        for n, pts in live:
            ps = [r for r in pts if get(r, fam, key) is not None and not r.get("backfill")]
            if len(ps) >= 2:
                a, b = get(ps[0], fam, key) * scale, get(ps[-1], fam, key) * scale
                up = (b > a) == higher_better
                bits.append(f"{n}: {fmt(a)} {after(ps[0]['epoch'])} → {fmt(b)} {after(ps[-1]['epoch'])}, "
                            + ("идёт в нужную сторону" if abs(b - a) > 1e-9 and up else ("не улучшается" if abs(b - a) <= 1e-9 else "ушло не в ту сторону")))
            else:
                lv = last(pts, fam, key, scale)
                if lv:
                    bits.append(f"{n}: {fmt(lv[0])}" + (f" {after(lv[1])}" if lv[1] else "") + (" (пока одна точка)" if len(ps) < 2 else ""))
        for n, r in refs.items():
            v = get(r, fam, key)
            if v is not None:
                bits.append(f"{n}: {fmt(v * scale)}")
        t = f"; учитель {fmt(teacher)}" if teacher is not None else ""
        return (what + " " if what else "") + "; ".join(bits) + t + "."

    def after(e):
        if abs(e - round(e)) < 0.05:
            n = round(e)
            return f"после {n}\u00a0" + ("эпохи" if n % 10 == 1 and n % 100 != 11 else "эпох")
        return f"после {ru(e, 1)}\u00a0эпохи"

    pct = lambda v: ru(v, 0) + "\u00a0%"
    num = lambda v: ru(v, 2)
    tcap = ts.get("capture")
    parts = []
    for n, pts in live:
        lv = pts[-1]
        parts.append(f"{n}: {lv['fam_ok']} из {lv['fam_n']}" + (f" (на полной проверке 32 семейств: {lv['fam_ok32']})" if lv.get("fam_ok32") is not None else "")
                     + (f" {after(lv['epoch'])}" if lv.get("epoch") else ""))
    for n, r in refs.items():
        parts.append(f"{n}: {r['fam_ok']} из {r['fam_n']}" + (f" (на полной: {r['fam_ok32']} из {r['fam_n32']})" if r.get("fam_ok32") is not None else ""))
    ok_note = "; ".join(parts) + ". Уровень учителя — 90 % побед лучшего учителя семейства; считаем по быстрой проверке (12 семейств: 8 осадных, набег, добыча на суше, два дозорных)."

    cap = [(a, b, [(x, y / 100) for x, y in c], d) for a, b, c, d in series(None, "capture")]
    for fam in ("assault-heal", "planned-assault"):
        for n, pts in live:
            xy = [(float(r["epoch"]), r["families"][fam]["capture"] / 100) for r in pts
                  if r.get("epoch") is not None and r.get("families", {}).get(fam, {}).get("capture") is not None]
            cap.append((f"{n} {EVAL_NAME[fam]}", col[n], xy, {"thin": True}))
    cap_note = note(None, "capture", pct, 1.0, tcap, "Среднее по семи осадным семействам, где город берут (без rush: там город защищают):")
    hard = []
    for fam in ("assault-heal", "planned-assault"):
        for n, pts in live:
            v = last(pts, fam, "capture")
            if v:
                hard[len(hard):] = [f"{n}, {EVAL_NAME[fam]}: {pct(v[0])} (учитель {pct(tref.get(fam, {}).get('capture') or 0)})"]
    cap_note += (" Самые трудные: " + "; ".join(hard) + ".") if hard else ""

    ok = []
    for n, pts in live:
        ok.append((n, col[n], [(float(r["epoch"]), float(r["fam_ok"])) for r in pts if r.get("epoch") is not None], {}))
    for n, r in refs.items():
        ok.append((n, EVAL_REFS[n], [(float(r["epoch"]), float(r["fam_ok"]))], {"thin": True, "ref": True}))

    lost = series(None, "lost")
    lost_note = note(None, "lost", num, 1.0, ts.get("lost"), "Сколько своих юнитов теряем за партию в осадных семействах (меньше — лучше):", higher_better=False)
    rnd = series(None, "rounds_cap")
    rnd_note = note(None, "rounds_cap", lambda v: ru(v, 1), 1.0, ts.get("rounds_cap"), "Среднее число раундов до взятия города, где взяли (меньше — быстрее):", higher_better=False)
    fld = []
    for n, pts in live:
        fld.append((f"{n} набег", col[n], [(float(r["epoch"]), r["families"]["raid"]["win"] / 100) for r in pts if r.get("epoch") is not None and "raid" in r.get("families", {})], {}))
        fld.append((f"{n} добыча", col[n], [(float(r["epoch"]), r["families"]["prey-land"]["win"] / 100) for r in pts if r.get("epoch") is not None and "prey-land" in r.get("families", {})], {"thin": True}))
    for n, r in refs.items():
        for fam, lab in (("raid", "набег"), ("prey-land", "добыча")):
            if fam in r.get("families", {}) and r.get("epoch") is not None:
                fld.append((f"{n} {lab}", EVAL_REFS[n], [(float(r["epoch"]), r["families"][fam]["win"] / 100)], {"thin": True, "ref": True}))
    bits = []
    for n, pts in live:
        r = pts[-1]
        bits.append(f"{n}: набег {pct(r['families']['raid']['win'])}, добыча на суше {pct(r['families']['prey-land']['win'])}")
    for n, r in refs.items():
        bits.append(f"{n}: набег {pct(r['families']['raid']['win'])}, добыча {pct(r['families']['prey-land']['win'])}")
    fld_note = "Толстая линия — набег, тонкая — добыча на суше (там всего 9 партий, шум большой). " + "; ".join(bits) + "; учитель 100\u00a0%."

    ch = [
        chart("Захват городов, % партий (осадные семейства)", cap, "pct", target=(tcap / 100, f"учитель {ru(tcap, 0)}\u00a0%") if tcap is not None else None, note=cap_note, rlabel=150),
        chart("Семейств на уровне учителя (из 12)", ok, "int", target=(12.0, "все 12"), note=ok_note, rlabel=60),
        chart("Потери за партию, юнитов (осадные семейства)", lost, "loss", target=(ts["lost"], f"учитель {ru(ts['lost'], 2)}") if ts.get("lost") is not None else None, note=lost_note, rlabel=60),
        chart("Раундов до взятия города", rnd, "loss", target=(ts["rounds_cap"], f"учитель {ru(ts['rounds_cap'], 1)}") if ts.get("rounds_cap") is not None else None, note=rnd_note, rlabel=60),
        chart("Поле: набег и добыча на суше, % побед", fld, "pct", target=(1.0, "учитель 100\u00a0%"), note=fld_note, rlabel=100),
    ]
    npts = sum(1 for _, pts in live for r in pts if not r.get("backfill"))
    intro = (f"<p class='mu'>Каждая контрольная точка обучения (каждые пол-эпохи) проверяется игрой: 12 семейств, отложенные сцены, "
             f"быстрая проверка из M-HANDOFF. Точек по ходу обучения сейчас: {npts}; у законченных сетей — одна точка (их последняя эпоха), "
             f"серая пунктирная линия — A2 и M1.</p>")
    return f"<h2>Как играет сеть по ходу обучения</h2>{intro}<div class='grid2'>{''.join(ch)}</div>"


PHASES = [
    # (id, title, what it does)
    ("imit", "1. Подражание (A0, B0)", "A0 без карты, B0 с картой 24×24; учатся повторять лучшего учителя задачи на данных эталонов"),
    ("dag", "2. DAgger (A1, A2)", "сеть играет сама, лучший эталон размечает её решения; дообучение по кругу"),
    ("mem", "3. Память (M0, M1)", "сеть помнит прошлые ходы и факты о карте; M1 — память, дообученная на своих партиях"),
    ("lineb", "4. Линия B: MB → MBH → MBN", "MB: карта + память; MBH: + история ходов; MBN: + заметки; вариант MBS (Mamba)"),
    ("hints", "5. Подсказки и ценность", "подсказки учителя в виде входов и оценка позиции (ценность) как вторая голова"),
    ("enemy", "6. Прогноз врага и намерения", "сеть предсказывает ходы врага и свои намерения на несколько ходов вперёд"),
    ("select", "7. Отбор лучших прогонов", "из своих партий берём лучшие и учимся на них; по кругу"),
    ("grow", "8. Размеры и выращивание", "большие сети: растим из малой, клонируем знания, сравниваем размеры"),
]


def phase_state() -> dict[str, tuple[str, str]]:
    """(state, note) per phase from the files: done / running / waiting."""
    st: dict[str, tuple[str, str]] = {}

    def run_note(name: str) -> tuple[str, str] | None:
        d = RUNS / name
        c = read_curves(d) if d.exists() else []
        if not c:
            return None
        age = time.time() - (d / "curves.jsonl").stat().st_mtime
        trained = (d / "weights.safetensors").exists() and age > 600
        return ("done" if trained else "running", f"{name[3:]}: шаг {c[-1].get('step', 0)}" + ("" if trained else " (идёт)"))

    def phase(names: list[str], table_glob: str) -> tuple[str, str]:
        runs = [x for x in (run_note(n) for n in names) if x]
        tab = any(RUNS.glob(table_glob))
        if not runs and not tab:
            return ("waiting", "")
        note = ", ".join(n for _, n in runs[-3:])
        if any(s == "running" for s, _ in runs):
            return ("running", note)
        if tab:
            return ("done", note)
        return ("running", (note + "; " if note else "") + "ждёт замера")

    st["imit"] = phase(["netA", "netB"], "table-B0.json")
    st["dag"] = phase(["netA-dag1", "netA2", "netA2-nomemory"], "table-A2.json")
    st["mem"] = phase(["netM0", "netM1", "netM1b"], "table-M-final.json")
    # line B is done only when its last variant (MBN) has a table; before that it runs while any MB net exists.
    mb = sorted(p.name for p in RUNS.glob("netMB*") if (p / "curves.jsonl").exists())
    mbn = any(RUNS.glob("table-MBN*.json"))
    s4, n4 = phase(mb, "table-MBN*.json") if mb else ("waiting", "")
    if mb and not mbn and s4 == "done":
        s4 = "running"
    if mb and not any(RUNS.glob("table-MB*.json")):
        n4 = (n4 + "; " if n4 else "") + "таблицы MB* ещё нет"
    st["lineb"] = (s4, n4)
    for pid in ("hints", "enemy", "select", "grow"):
        st[pid] = ("waiting", "")
    return st


def pipeline_svg() -> str:
    st = phase_state()
    fill = {"done": "#3f8a4a", "running": "#2f6fb3", "waiting": "#9a978f"}
    label = {"done": "готово", "running": "идёт", "waiting": "ждёт"}
    W, bw, bh, gap = 1125, 250, 112, 20
    cols = 4
    rows = (len(PHASES) + cols - 1) // cols
    H = rows * (bh + 46) + 10
    g = [f"<svg viewBox='0 0 {W} {H}' role='img' aria-label='Пайплайн обучения'>",
         "<defs><marker id='ar' viewBox='0 0 10 10' refX='9' refY='5' markerWidth='7' markerHeight='7' orient='auto-start-reverse'>"
         "<path d='M0,0 L10,5 L0,10 z' fill='#8a877f'/></marker></defs>"]
    pos = {}
    for i, (pid, title, what) in enumerate(PHASES):
        r, c = divmod(i, cols)
        if r % 2 == 1:
            c = cols - 1 - c  # snake: the second row runs right to left
        x = 10 + c * (bw + gap + 15)
        y = 10 + r * (bh + 46)
        pos[pid] = (x, y)
        s, note = st.get(pid, ("waiting", ""))
        g.append(f"<rect x='{x}' y='{y}' width='{bw}' height='{bh}' rx='10' fill='var(--box)' stroke='{fill[s]}' stroke-width='{3 if s == 'running' else 2}'/>")
        g.append(f"<rect x='{x}' y='{y}' width='{bw}' height='26' rx='10' fill='{fill[s]}'/><rect x='{x}' y='{y+14}' width='{bw}' height='12' fill='{fill[s]}'/>")
        g.append(f"<text x='{x+10}' y='{y+18}' fill='#fff' font-size='13' font-weight='600'>{html.escape(title)}</text>")
        g.append(f"<text x='{x+bw-10}' y='{y+18}' fill='#fff' font-size='11' text-anchor='end'>{label[s]}</text>")
        # wrap the description into lines of ~38 chars
        words, line, lines = what.split(), "", []
        for w in words:
            if len(line) + len(w) + 1 > 38:
                lines.append(line)
                line = w
            else:
                line = f"{line} {w}".strip()
        lines.append(line)
        for k, ln in enumerate(lines[:4]):
            g.append(f"<text x='{x+10}' y='{y+44+k*15}' font-size='11.5' class='tx'>{html.escape(ln)}</text>")
        if note:
            g.append(f"<text x='{x+10}' y='{y+bh-8}' font-size='11' class='nt'>{html.escape(note)}</text>")
    ids = [p[0] for p in PHASES]
    for a, b in zip(ids, ids[1:]):
        (xa, ya), (xb, yb) = pos[a], pos[b]
        if ya == yb:
            if xb > xa:
                g.append(f"<line x1='{xa+bw}' y1='{ya+bh/2}' x2='{xb-2}' y2='{yb+bh/2}' stroke='#8a877f' stroke-width='2' marker-end='url(#ar)'/>")
            else:
                g.append(f"<line x1='{xa}' y1='{ya+bh/2}' x2='{xb+bw+2}' y2='{yb+bh/2}' stroke='#8a877f' stroke-width='2' marker-end='url(#ar)'/>")
        else:
            g.append(f"<line x1='{xa+bw/2}' y1='{ya+bh}' x2='{xb+bw/2}' y2='{yb-2}' stroke='#8a877f' stroke-width='2' marker-end='url(#ar)'/>")
    g.append("</svg>")
    legend = ("<p class='mu'>Зелёный — готово, синий — идёт сейчас, серый — ждёт. Каждый этап меряется на тренажёрах (Rust) на одних и тех "
              "же сценах; лучший вариант идёт дальше. Полные партии в этом пайплайне не мерим: только тренажёры.</p>")
    return "".join(g) + legend


FAMILY_RU = {
    "airstrike": "Ударить бомбардировщиками по стеку врага",
    "assault-heal": "Осада с ротацией: раненых уводить лечиться и возвращать",
    "bait": "Не клюнуть на приманку и убить засаду",
    "bombard": "Обстрелять и взять прибрежный город кораблями",
    "choke": "Прорвать узкий проход, который держит человек",
    "citadel": "Прорвать человека, стоящего у цитадели",
    "city-fire": "Подойти к городу, не попав под его огонь",
    "convoy": "Провести поселенца мимо налётчика и основать город",
    "counter": "Не дать налётчикам разорить свою землю",
    "duel": "Стычка в поле: N своих против M чужих",
    "flank": "Окружить и убить сильного врага за 3 хода",
    "focus": "Добить раненых: 3 из 4 за 4 хода",
    "general": "Бой с аурой Великого полководца, полководец жив",
    "guard": "Сберечь рабочего или полководца от налётчиков",
    "harass": "Пехотой против конных лучников человека (кешики)",
    "hill-archers": "Поставить лучников на холмы, чтобы доставали до города",
    "hold": "Удержать горный проход против силы в 1,5 раза больше",
    "kite": "Бей-и-беги: конные стрелки против медленной пехоты, без потерь",
    "planned-assault": "Штурм целиком: подход, холмы, ротация раненых, взятие",
    "prey-land": "Слабый новый город соседа рядом по суше: брать",
    "prey-either": "Слабый город за морем или под охраной: можно и не брать",
    "promotion-choice": "Выбрать повышение под план (лечение, против городов)",
    "raid": "Набег: разорить 3 улучшения у вражеского города",
    "ram": "Взять столицу тараном или осадной башней",
    "retreat": "Спасти армию от вдвое большей силы",
    "rush": "Удержать свою столицу против раннего наскока с тараном",
    "siege": "Взять город со стенами осадными орудиями",
    "sink": "Потопить войска врага на воде при высадке",
    "snipe": "Поймать гражданского или полководца под охраной",
    "surrounded": "Сильный юнит в окружении: вырваться или выжить",
    "wounded": "Спасти своих раненых от добивания",
}


def fam_cell(fam: str) -> str:
    return f"<td>{html.escape(FAMILY_RU.get(fam, fam))} <span class='mu'>{html.escape(fam)}</span></td>"


SIEGE = ["siege", "ram", "assault-heal", "city-fire", "hill-archers", "planned-assault", "promotion-choice", "rush"]


def ablation(v: str) -> bool:
    return v.endswith("z") or v.endswith("nf")


def table_variant(stem: str, variants: list[str]) -> str:
    """The variant a table's round introduced: the one named like the table, else the newest non-ablation one."""
    name = stem.replace("table-", "")
    if name in variants:
        return name
    if "-" in name and name.split("-")[0] in variants:
        return name.split("-")[0]
    real = [v for v in variants if not ablation(v)]
    return (real or variants or ["?"])[-1]


# Old names of A0 and B0 in the first tables (table-A, table-B0, table-bc): the same nets, shown once as A0 / B0.
LEGACY = {"A", "B"}


def siege_html() -> str:
    """Siege families, one column per variant (oldest first), each from the latest table that has it."""
    tabs = sorted((t for t in RUNS.glob("table-*.json") if t.name != "table-base.json"), key=lambda p: p.stat().st_mtime)
    order: list[str] = []          # variants by first appearance
    src: dict[str, dict] = {}      # variant -> latest table's data
    best: dict[str, dict] = {}
    for t in tabs:
        try:
            d = json.loads(t.read_text())
        except json.JSONDecodeError:
            continue
        for v in d.get("summary", {}).get("variants") or []:
            if v in LEGACY:
                continue
            if v not in order:
                order.append(v)
            src[v] = {r["family"]: r["rates"].get(v) for r in d.get("rows", []) if r["family"] in SIEGE}
        for r in d.get("rows", []):
            if r["family"] in SIEGE:
                best[r["family"]] = {"best": r.get("best_rate", 0), "std": r["rates"].get("standard", 0), "unciv": r["rates"].get("unciv", 0)}
    if not order:
        return ""
    main = [v for v in order if not ablation(v)]
    checks = [v for v in order if ablation(v)]

    def block(vs: list[str], extra: bool) -> str:
        head = "".join(f"<th>{html.escape(v)}</th>" for v in vs)
        body = ""
        for fam in SIEGE:
            if fam not in best:
                continue
            cells = "".join(f"<td>{src[v][fam]*100:.0f} %</td>" if src[v].get(fam) is not None else "<td>—</td>" for v in vs)
            tail = (f"<td>{best[fam]['best']*100:.0f} %</td><td>{best[fam]['std']*100:.0f} %</td><td>{best[fam]['unciv']*100:.0f} %</td>") if extra else ""
            body += f"<tr>{fam_cell(fam)}{cells}{tail}</tr>"
        tail_h = "<th>лучший учитель</th><th>standard</th><th>Unciv</th>" if extra else ""
        return f"<div class='wrap'><table><tr><th>задача</th>{head}{tail_h}</tr>{body}</table></div>"
    out = ("<h3>Осада: доля побед сети по вариантам (слева направо — по времени; каждый вариант из последнего замера, где он есть)</h3>"
           + block(main, True))
    if checks:
        out += ("<h3>Проверки: те же варианты без памяти (z) или без фактов (nf)</h3>" + block(checks, False))
    return out


def trainers_html() -> str:
    """The trainer tables `<runs>/table-*.json` (the net as a player in the Rust trainers, held-out instances)."""
    out = []
    for t in sorted(RUNS.glob("table-*.json"), key=lambda p: p.stat().st_mtime, reverse=True):
        if t.name == "table-base.json":
            continue
        try:
            d = json.loads(t.read_text())
        except json.JSONDecodeError:
            continue
        var = table_variant(t.stem, d.get("summary", {}).get("variants") or [])
        summ = d.get("summary", {}).get(var, {})
        rows = sorted(d.get("rows", []), key=lambda r: r["rates"].get(var, 0) - r.get("best_rate", 0))
        def cell(v, best=None):
            pct = v * 100
            cls = "" if best is None else (" class='good'" if v >= 0.9 * best else (" class='mid'" if v >= 0.7 * best else " class='bad'"))
            return f"<td{cls}>{pct:.0f} %</td>"
        body = "".join(
            f"<tr>{fam_cell(r['family'])}<td>{r['n']}</td>{cell(r['rates'].get(var, 0), r.get('best_rate', 0))}"
            f"<td>{r.get('best_rate', 0)*100:.0f} % <span class='mu'>{html.escape(str(r.get('best', '')))}</span></td>"
            f"{cell(r['rates'].get('standard', 0))}{cell(r['rates'].get('tac2', 0))}{cell(r['rates'].get('unciv', 0))}</tr>" for r in rows)
        out.append(f"<h3>{html.escape(t.stem.replace('table-', 'Сеть '))}: на уровне ≥ 90 % лучшего учителя — {summ.get('passed', '?')} задач из {len(rows)}</h3>"
                   f"<div class='wrap'><table><tr><th>задача</th><th>сцен</th><th>сеть</th><th>лучший учитель</th><th>standard</th><th>tac2</th><th>Unciv</th></tr>{body}</table></div>")
    return "".join(out) or "<p class='mu'>замера на тренажёрах пока нет</p>"


def alerts_html() -> str:
    p = LAB / "dash" / "alerts.json"
    if not p.exists():
        return "<p class='mu'>Сторож обучения не запущен.</p>"
    try:
        d = json.loads(p.read_text())
    except json.JSONDecodeError:
        return ""
    age = time.time() - d.get("time", 0)
    items = d.get("alerts", [])
    if not items:
        return f"<p class='ok'>Сторож обучения: всё в порядке (проверено {age:.0f} с назад).</p>"
    lis = "".join(f"<li>{html.escape(a)}</li>" for a in items)
    return f"<div class='warn'><b>Сторож обучения ({age:.0f} с назад):</b><ul>{lis}</ul></div>"


def campaign_html() -> str:
    """The overnight campaign (tools/m5/campaign.py): the status file and the last results."""
    d = RUNS / "campaign"
    st = d / "status.json"
    if not st.exists():
        return ""
    try:
        s = json.loads(st.read_text())
    except json.JSONDecodeError:
        return ""
    rows = []
    res = d / "results.jsonl"
    if res.exists():
        for line in res.read_text().splitlines()[-12:]:
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            rows.append("<tr>" + "".join(f"<td>{html.escape(str(r.get(k, '')))}</td>" for k in ("job", "kind", "status", "minutes", "families_90", "dps", "ms_per_turn", "peak_ram_gb")) + "</tr>")
    eta = s.get("eta_h")
    return (f"<h2>Campaign</h2><div class='lead'>{html.escape(str(s.get('state', '')))}: {s.get('done', 0)} of {s.get('total', 0)} jobs finished; "
            f"now: {html.escape(str(s.get('current', '-')))}; estimate left: {eta if eta is not None else '?'} h; updated {html.escape(str(s.get('updated', '')))}</div>"
            "<div class='wrap'><table><tr><th>job</th><th>kind</th><th>status</th><th>min</th><th>families &ge; 90 %</th><th>dec/s</th><th>ms/turn</th><th>peak RAM GB</th></tr>"
            + "".join(rows) + "</table></div>")


def page() -> str:
    args = launch_args()
    RUNS.mkdir(parents=True, exist_ok=True)
    runs = sorted([d for d in RUNS.iterdir() if d.is_dir() and (d / "curves.jsonl").exists()],
                  key=lambda d: (d / "curves.jsonl").stat().st_mtime, reverse=True)
    main_runs = [d for d in runs if not d.name.startswith("smoke")] or runs
    color = {d.name: COLORS[i % len(COLORS)] for i, d in enumerate(main_runs)}
    curves = {d.name: read_curves(d) for d in main_runs}

    # Headline: the freshest run's last point.
    head = ""
    if main_runs:
        d = main_runs[0]
        c = curves[d.name]
        if c:
            last = c[-1]
            hold = last.get("hold", {})
            age = time.time() - (d / "curves.jsonl").stat().st_mtime
            head = (f"<b>{html.escape(d.name)}</b>: шаг {last.get('step', 0):,} · эпоха {last.get('epoch', 0):.2f} · "
                    f"точность «юнит + действие» на отложенных сценах <b>{hold.get('acc_both', 0)*100:.1f} %</b> · "
                    f"{last.get('dps', 0):,.0f} решений/с · идёт {last.get('wall_s', 0)/60:.1f} мин · "
                    f"обновлено {age:.0f} с назад").replace(",", " ")

    def is_speed(n): return n.startswith("speed")
    def is_ablation(n): return any(t in n for t in ("-z", "nomemory", "-nf")) or n.endswith("nf")
    def is_b(n): return n.startswith("netMB")

    def ser(names, key, sub=None, alt=None, colors=None, style=None):
        out = []
        for n in names:
            pts = []
            for r in curves[n]:
                v = r.get(sub, {}).get(key) if sub else r.get(key)
                if v is None and alt:
                    v = r.get(alt)
                if isinstance(v, (int, float)) and isinstance(r.get("epoch"), (int, float)):
                    pts.append((float(r["epoch"]), float(v)))
            out.append((n[3:] if n.startswith("net") else n, (colors or {}).get(n, "#777"), pts, (style or {}).get(n, {})))
        return out

    def last_of(n, key, sub=None):
        for r in reversed(curves.get(n, [])):
            v = r.get(sub, {}).get(key) if sub else r.get(key)
            if isinstance(v, (int, float)):
                return float(v), float(r.get("epoch", 0)), r
        return None

    def epochs_ru(ep):
        if abs(ep - round(ep)) < 0.05:
            n = round(ep)
            return f"{n}\u00a0" + ("эпохи" if n % 10 == 1 and n % 100 != 11 else "эпох")
        return f"{ru(ep, 1)}\u00a0эпохи"

    def status(n):
        lp = last_of(n, "epoch")
        d = RUNS / n
        age = time.time() - (d / "curves.jsonl").stat().st_mtime
        try:
            target = float(json.loads((d / "config.json").read_text()).get("epochs", 0))
        except (OSError, ValueError):
            target = 0.0
        ep = lp[0] if lp else 0.0
        wall = curves[n][-1].get("wall_s", 0) / 60 if curves[n] else 0
        if age < 180:
            return f"идёт, {ru(wall, 0)}\u00a0мин"
        if target and ep < target * 0.97:
            return f"обучение остановлено на пределе {ru(wall, 0)}\u00a0мин (план {ru(target, 0)}\u00a0эп.), дообучение впереди"
        return "обучение закончено"

    BASE = {"netA2": "A2", "netM1": "M1"}
    b_all = sorted([d.name for d in main_runs if is_b(d.name) and not is_speed(d.name) and not is_ablation(d.name)])
    b_runs = b_all[-4:]
    base_runs = [n for n in BASE if n in curves]
    STRONG = ["#2f6fb3", "#c0632f", "#3f8a4a", "#8a4fb3"]
    col_main = {n: STRONG[i] for i, n in enumerate(b_runs)}
    col_main.update({"netA2": "#6f6e69", "netM1": "#a09e97"})
    thin = {n: {"thin": True} for n in base_runs}
    names_main = b_runs + base_runs

    def compare(n, key, sub, higher_better, fmt):
        me = last_of(n, key, sub)
        if not me:
            return ""
        bits = []
        for b in base_runs:
            lb = last_of(b, key, sub)
            if lb:
                better = (me[0] > lb[0]) == higher_better and abs(me[0] - lb[0]) > 1e-9
                bits.append((better, f"{BASE[b]} ({fmt(lb[0])})"))
        if not bits:
            return ""
        if all(x for x, _ in bits):
            return ("лучше " if higher_better is None else ("выше " if higher_better else "ниже ")) + " и ".join(t for _, t in bits)
        if not any(x for x, _ in bits):
            return ("хуже " if higher_better is None else ("ниже " if higher_better else "выше ")) + " и ".join(t for _, t in bits)
        return "; ".join((("лучше " if x else "хуже ") + t) for x, t in bits)

    acc_notes, loss_notes, spd_notes = [], [], []
    for n in b_runs:
        sh = n[3:]
        a = last_of(n, "acc_both", "hold")
        if a:
            cmp_ = compare(n, "acc_both", "hold", True, lambda v: ru_pct(v))
            acc_notes.append(f"{sh}: {ru_pct(a[0], 1)} на отложенных после {epochs_ru(a[1])}" + (f" — {cmp_} (итоги эталонов)" if cmp_ else "") + f"; {status(n)}")
        l = last_of(n, "main", "hold")
        if l:
            cmp_ = compare(n, "main", "hold", False, lambda v: ru(v, 2))
            loss_notes.append(f"{sh}: потери {ru(l[0], 2)} на отложенных" + (f" — {cmp_} (чем меньше, тем лучше)" if cmp_ else ""))
        v = last_of(n, "dps_mean")
        if v:
            tgt = 5000
            rel = "выше" if v[0] >= tgt else "ниже"
            spd_notes.append(f"скорость {sh} {ru_tys(v[0])}/с — {rel} цели {ru_tys(tgt, 0)}")
    acc_note = "; ".join(acc_notes) or "прогонов линии B пока нет"
    loss_note = "; ".join(loss_notes) or "прогонов линии B пока нет"
    spd_note = "; ".join(spd_notes) or "прогонов линии B пока нет"

    charts = [
        chart("Точность на отложенных: юнит и действие вместе", ser(names_main, "acc_both", "hold", colors=col_main, style=thin), "pct", note=acc_note),
        chart("Потери на отложенных (меньше — лучше)", ser(names_main, "main", "hold", colors=col_main, style=thin), "loss", note=loss_note),
        chart("Скорость обучения, тысяч решений в секунду", ser(b_runs, "dps", colors=col_main), "tys", target=(5000, "цель 5 тыс."), note=spd_note),
    ]

    # Archive: every run, same charts plus the pointer / action / train-loss ones; unique colours, a legend instead of end labels.
    arch_names = [d.name for d in main_runs]
    col_arch = {n: f"hsl({(i * 137.5) % 360:.0f},62%,45%)" for i, n in enumerate(arch_names)}
    arch_charts = [
        chart("Точность: юнит и действие вместе", ser(arch_names, "acc_both", "hold", colors=col_arch), "pct", legend=True),
        chart("Потери на отложенных", ser(arch_names, "main", "hold", colors=col_arch), "loss", legend=True),
        chart("Потери: обучение", ser(arch_names, "train_loss", alt="loss", colors=col_arch), "loss", legend=True),
        chart("Точность: какой юнит ходит (указатель)", ser(arch_names, "acc_ptr", "hold", colors=col_arch), "pct", legend=True),
        chart("Точность: какое действие", ser(arch_names, "acc_act", "hold", colors=col_arch), "pct", legend=True),
        chart("Скорость обучения, тысяч решений в секунду", ser(arch_names, "dps", colors=col_arch), "tys", legend=True),
    ]
    graphs = (f"<p class='mu'>Сейчас показана линия B ({', '.join(n[3:] for n in b_runs) or 'нет прогонов'}) цветом и два эталона тонкой серой линией: "
              f"A2 и M1. Название прогона подписано на конце его линии. Остальные прогоны — в архиве ниже.</p>"
              f"<div class='grid2'>{''.join(charts)}</div>"
              f"<details><summary>Архив прогонов ({len(arch_names)}, включая замеры скорости и проверки без памяти)</summary>"
              f"<div class='grid2'>{''.join(arch_charts)}</div></details>")

    # Runs table: options and data.
    rows = []
    for d in main_runs:
        cfg = {}
        if (d / "config.json").exists():
            try:
                cfg = json.loads((d / "config.json").read_text())
            except json.JSONDecodeError:
                cfg = {}
        opts = {**DEFAULTS, **args.get(d.name, {}), **cfg}
        c = curves[d.name]
        last = c[-1] if c else {}
        train = opts.get("train", "")
        tsize = ""
        for t in (train if isinstance(train, list) else str(train).split()):
            p = (REPO / "nn" / t).resolve() if not str(t).startswith("/") else Path(t)
            if p.exists():
                tsize = human_bytes(p.stat().st_size)
        params = opts.get("params", "")
        rows.append("<tr>" + "".join(f"<td>{html.escape(str(x))}</td>" for x in [
            d.name, "да" if str(opts.get("map")) in ("True", "true", "1") else "нет", opts.get("d"), opts.get("layers"),
            opts.get("heads"), opts.get("ff"), params, opts.get("bs"), opts.get("lr"), opts.get("epochs", ""),
            opts.get("decisions", tsize), f"{last.get('step', 0)}", f"{last.get('epoch', 0):.2f}" if last else "",
        ]) + "</tr>")
    table = ("<table><tr><th>прогон</th><th>карта</th><th>ширина</th><th>слои</th><th>головы внимания</th><th>FFN</th>"
             "<th>параметров</th><th>пачка</th><th>скорость обучения</th><th>эпох</th><th>данные</th><th>шаг</th><th>эпоха</th></tr>"
             + "".join(rows) + "</table>")

    # Teachers.
    teach = ""
    for d in main_runs + runs:
        p = d / "teachers.json"
        if p.exists():
            try:
                best = json.loads(p.read_text()).get("best", {})
            except json.JSONDecodeError:
                best = {}
            teach = "<table><tr><th>задача</th><th>учитель</th><th>доля побед учителя на своих сценах</th></tr>" + "".join(
                f"<tr><td>{html.escape(k)}</td><td>{html.escape(str(v[0]))}</td><td>{v[1]}</td></tr>"
                for k, v in sorted(best.items())) + "</table>"
            break

    logs = ""
    for p in sorted(LOGS.glob("net*.log"), key=lambda p: p.stat().st_mtime, reverse=True)[:3]:
        tail = "\n".join(p.read_text(errors="replace").splitlines()[-12:])
        logs += f"<h3>{html.escape(p.name)}</h3><pre>{html.escape(tail)}</pre>"

    data = []
    for name in ("teach-train.bin", "teach-hold.bin"):
        p = RUNS / name
        if p.exists():
            data.append(f"{name}: {human_bytes(p.stat().st_size)}")
    for name in ("teach-train", "teach-hold"):
        p = RUNS / name / "rows.jsonl"
        if p.exists():
            n = sum(1 for _ in p.open())
            data.append(f"{name}: {n:,} сыгранных сцен учителей".replace(",", " "))

    return f"""<!doctype html><html lang="ru"><head><meta charset="utf-8"><meta http-equiv="refresh" content="15">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Обучение сети войск — онлайн</title>
<style>
:root{{--bg:#f7f6f2;--fg:#1d1d1b;--mu:#6b6a66;--line:#d9d6ce;--box:#fff}}
@media (prefers-color-scheme:dark){{:root{{--bg:#1b1b1a;--fg:#ecebe6;--mu:#a3a19a;--line:#3d3b37;--box:#262624}}}}
body{{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 -apple-system,"Segoe UI",Roboto,sans-serif}}
main{{max-width:1120px;margin:0 auto;padding:16px}} h1{{font-size:22px;margin:0 0 8px}} h2{{font-size:18px;margin:24px 0 8px}}
h3{{font-size:14px;margin:0 0 4px}} .lead{{background:var(--box);border:1px solid var(--line);padding:10px 12px;border-radius:8px}}
.grid2{{display:grid;grid-template-columns:repeat(auto-fit,minmax(340px,1fr));gap:12px}}
.chart{{background:var(--box);border:1px solid var(--line);border-radius:8px;padding:8px}} svg{{width:100%;height:auto}}
svg .grid{{stroke:var(--line)}} svg .tx{{fill:var(--fg)}} svg .nt{{fill:var(--mu);font-style:italic}} svg .ax{{fill:var(--mu);font-size:10px}} .lg{{font-size:12px;margin-right:10px}}
.lg i{{display:inline-block;width:10px;height:10px;border-radius:2px;margin-right:4px}}
table{{border-collapse:collapse;width:100%;font-size:13px;background:var(--box)}} td,th{{border-bottom:1px solid var(--line);padding:4px 6px;text-align:left}}
.wrap{{overflow-x:auto}} pre{{background:var(--box);border:1px solid var(--line);padding:8px;font-size:12px;overflow-x:auto}} .mu{{color:var(--mu)}} td.good{{color:#3f8a4a;font-weight:600}} td.mid{{color:#b07a1f}} td.bad{{color:#b5513f}} .ok{{color:#3f8a4a}} .warn{{background:#fff4e5;color:#6b3d00;border:1px solid #e0a050;border-radius:8px;padding:8px 12px;margin:10px 0}} @media (prefers-color-scheme:dark){{.warn{{background:#3a2a10;color:#ffd9a0}}}}
.see{{margin:6px 2px 2px;font-size:13px}} details{{margin-top:14px}} summary{{cursor:pointer;font-weight:600;padding:6px 0}}
</style></head><body><main>
<h1>Обучение сети войск — онлайн</h1>
<div class="lead">{head or "Полное обучение ещё не началось: ждёт очереди к видеокарте. Ниже — пробные прогоны."}</div>
{alerts_html()}{campaign_html()}<h2>Пайплайн обучения</h2><div class="wrap">{pipeline_svg()}</div>
<h2>Как сеть играет на тренажёрах (Rust, отложенные сцены)</h2>{siege_html()}{trainers_html()}{evals_html()}<h2>Графики (ось X — эпохи обучения)</h2>{graphs}
<h2>Прогоны и параметры</h2><div class="wrap">{table}</div>
<p class="mu">Модель: токены юнитов, городов и цели → трансформер; указатель «кто ходит» и оценка допустимых действий; вариант B добавляет свёрточную карту 24×24, посчитанную раз за ход. Обучение — подражание лучшему учителю задачи (вес «лучшего» ×2), Adam.</p>
<h2>На чём учится</h2><p>{' · '.join(data) or 'нет данных'}</p><div class="wrap">{teach}</div>
<h2>Последние строки журналов</h2>{logs or '<p class="mu">журналов полного обучения пока нет</p>'}
<p class="mu">Страница перечитывает файлы каждые 15 с: {html.escape(str(RUNS))}. Сделано {time.strftime('%H:%M:%S')}.</p>
</main></body></html>"""


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        try:
            body = page().encode()
            self.send_response(200)
        except Exception as e:  # noqa: BLE001 (a status page shows its own error instead of dying)
            body = f"<pre>{html.escape(repr(e))}</pre>".encode()
            self.send_response(500)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
