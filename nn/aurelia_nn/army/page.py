"""Roman's page of the army net (Russian): the trainer table, the learning curves as inline SVG, the data, parity
and decision time. python -m aurelia_nn.army.page --spec page.json --out FILE.html

page.json: {"headline": str, "verdict": [str], "nets": [{"name", "curves": path}], "table": path (table.py),
"table_label": str, "extra_tables": [{"label", "path"}], "teachers": {player: decisions}, "facts": [[k, v]], "details": [str]}
"""
import argparse
import html
import json

COLORS = ['#2563eb', '#dc2626', '#16a34a', '#9333ea', '#ea580c']


def curves_svg(nets, key, title, w=560, h=240):
    pad_l, pad_b, pad_t, pad_r = 44, 30, 26, 10
    series = []
    xmax = 0.0
    ymin, ymax = 1e9, -1e9
    for n in nets:
        pts = []
        for l in open(n['curves']):
            r = json.loads(l)
            if 'hold' not in r:
                continue
            v = r['hold'][key] if key in r['hold'] else r.get(key)
            x = r['epoch'] + n.get('epoch0', 0.0)
            pts.append((x, v))
            xmax = max(xmax, x)
            ymin, ymax = min(ymin, v), max(ymax, v)
        series.append((n['name'], pts))
    if ymax <= ymin:
        ymax = ymin + 1
    span = ymax - ymin
    ymin, ymax = ymin - 0.05 * span, ymax + 0.05 * span
    sx = lambda x: pad_l + (w - pad_l - pad_r) * x / max(xmax, 1e-9)
    sy = lambda y: pad_t + (h - pad_t - pad_b) * (1 - (y - ymin) / (ymax - ymin))
    out = [f'<svg viewBox="0 0 {w} {h}" width="{w}" height="{h}" xmlns="http://www.w3.org/2000/svg" font-family="sans-serif" font-size="11">']
    out.append(f'<text x="{pad_l}" y="15" font-size="13" font-weight="bold">{html.escape(title)}</text>')
    for i in range(5):
        y = ymin + (ymax - ymin) * i / 4
        out.append(f'<line x1="{pad_l}" x2="{w - pad_r}" y1="{sy(y):.1f}" y2="{sy(y):.1f}" stroke="#e5e7eb"/>')
        out.append(f'<text x="{pad_l - 4}" y="{sy(y) + 4:.1f}" text-anchor="end" fill="#555">{y:.2f}</text>')
    for i in range(int(xmax) + 1):
        out.append(f'<text x="{sx(i):.1f}" y="{h - 10}" text-anchor="middle" fill="#555">{i}</text>')
    out.append(f'<text x="{w - pad_r}" y="{h - 10}" text-anchor="end" fill="#555">эпохи</text>')
    for k, (name, pts) in enumerate(series):
        c = COLORS[k % len(COLORS)]
        d = ' '.join(f'{"M" if i == 0 else "L"}{sx(x):.1f},{sy(y):.1f}' for i, (x, y) in enumerate(pts))
        out.append(f'<path d="{d}" fill="none" stroke="{c}" stroke-width="2"/>')
        out.append(f'<text x="{w - pad_r - 4}" y="{pad_t + 14 * k + 4}" text-anchor="end" fill="{c}">{html.escape(name)}</text>')
    out.append('</svg>')
    return '\n'.join(out)


def cell(v, best):
    if v is None:
        return '<td class="na">—</td>'
    share = v / best if best else 0
    cls = 'g' if share >= 0.9 else ('y' if share >= 0.5 else 'r')
    return f'<td class="{cls}">{round(100 * v)}</td>'


GEOMETRY = {'choke', 'hold', 'flank', 'kite', 'harass', 'hill-archers', 'surrounded', 'convoy'}


def table_html(path, label):
    t = json.load(open(path))
    variants = t['summary']['variants']
    rows = []
    for r in t['rows']:
        if r['n'] < 10:
            continue
        rt = r['rates']
        b = r['best_rate']
        refs = ', '.join(f'{p[4:]} {round(100 * v)}' for p, v in sorted(rt.items()) if p.startswith('ref-'))
        nets = ''.join(cell(rt.get(v), b) for v in variants)
        shares = ''.join(f"<td>{'—' if r['share'][v] is None else str(round(100 * r['share'][v])) + ' %'} {'✔' if r['pass'][v] else '✘'}</td>" for v in variants)
        fam = f"<b>{r['family']}</b>" if r['family'] in GEOMETRY else r['family']
        rows.append(f"<tr><td>{fam}</td><td>{r['n']}</td>{nets}{cell(rt.get('standard'), b)}{cell(rt.get('tac2'), b)}"
                    f"{cell(rt.get('unciv'), b)}<td>{round(100 * b)} <small>{html.escape(str(r['best']))}</small></td>{shares}<td><small>{html.escape(refs)}</small></td></tr>")
    s = t['summary']
    head = ('<tr><th>семейство</th><th>N</th>' + ''.join(f'<th>{html.escape(v)}</th>' for v in variants) +
            '<th>standard</th><th>tac2</th><th>Unciv</th><th>лучший учитель</th>' +
            ''.join(f'<th>доля {html.escape(v)} от лучшего</th>' for v in variants) + '<th>эталоны</th></tr>')
    verdict = '; '.join(f'{v}: порог 90 % пройден в {s[v]["passed"]} из {s["families"]} семейств' for v in variants)
    return f'<h3>{html.escape(label)} — {verdict}</h3><table>{head}{"".join(rows)}</table>'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--spec', required=True)
    ap.add_argument('--out', required=True)
    a = ap.parse_args()
    sp = json.load(open(a.spec))
    parts = ['<!DOCTYPE html><html lang="ru"><head><meta charset="utf-8"><title>Сеть управления армией</title><style>',
             'body{font-family:-apple-system,sans-serif;max-width:1100px;margin:24px auto;padding:0 16px;color:#111}',
             'table{border-collapse:collapse;font-size:13px;margin:8px 0}td,th{border:1px solid #ddd;padding:3px 7px;text-align:right}',
             'td:first-child,th:first-child{text-align:left}.g{background:#bbf7d0}.y{background:#fef08a}.r{background:#fecaca}.na{color:#999}',
             'h1{font-size:22px}.lead{font-size:17px;background:#f1f5f9;padding:10px 14px;border-left:4px solid #2563eb}',
             'details{margin:14px 0}summary{cursor:pointer;font-weight:bold}</style></head><body>',
             f'<h1>Сеть управления армией на тренажёрах боя — 23 ноября</h1><p class="lead">{sp["headline"]}</p>']
    parts.append('<ul>' + ''.join(f'<li>{v}</li>' for v in sp['verdict']) + '</ul>')
    parts.append(table_html(sp['table'], sp['table_label']))
    parts.append('<p><small>Цвет — доля от лучшего учителя в семействе: зелёный ≥ 90 %, жёлтый 50–90 %, красный &lt; 50 %. '
                 'Жирным — семейства геометрии (проход, фланг, кайт, холмы, окружение, конвой). '
                 'Отложенные экземпляры 80–99 каждой клетки (на них сеть не училась). tac2 — прежние прогоны тренажёров на тех же сценах.</small></p>')
    for t in sp.get('extra_tables', []):
        parts.append('<details><summary>' + html.escape(t['label']) + '</summary>' + table_html(t['path'], t['label']) + '</details>')
    parts.append('<h2>Кривые обучения (отложенные решения)</h2>')
    parts.append(curves_svg(sp['nets'], 'acc_both', 'Точность: верный юнит и верное действие'))
    parts.append(curves_svg(sp['nets'], 'acc_act', 'Точность действия при верном юните'))
    parts.append(curves_svg(sp['nets'], 'main', 'Потеря имитации (указатель + действие)'))
    parts.append('<h2>Данные и скорость</h2><table>' + ''.join(f'<tr><td>{html.escape(k)}</td><td>{html.escape(str(v))}</td></tr>' for k, v in sp['facts']) + '</table>')
    parts.append('<details><summary>Решения по учителям</summary><table>' + ''.join(f'<tr><td>{html.escape(k)}</td><td>{v}</td></tr>' for k, v in sorted(sp['teachers'].items(), key=lambda x: -x[1])) + '</table></details>')
    parts.append('<details><summary>Подробности: устройство сети, коммиты, как запустить</summary><ul>' + ''.join(f'<li>{d}</li>' for d in sp['details']) + '</ul></details>')
    parts.append('</body></html>')
    open(a.out, 'w').write('\n'.join(parts))


if __name__ == '__main__':
    main()
