//! A recorded game as a page to watch in a browser (`aurelia_qa::frames`):
//! the log is replayed through the kernel, its frames are written as JSON
//! and, with `--html`, embedded in the self-contained player
//! (`crates/qa/replay/player.html`, opens from `file://`).
//!
//! `ai-replay-frames LOG.aurlog [--out FRAMES.json] [--html PAGE.html] [--whole] [--per-turn|--per-round]`
//! `ai-replay-frames --batch DIR --out-dir OUT [--jobs J] [--force] [--title TEXT]`
//!
//! --whole        a training ground's battle with its whole game, not only the battle
//! --per-turn     a frame after every player-turn (the default of a battle)
//! --per-round    a frame after every round (the default of a whole game)
//! --batch DIR    every `.aurlog` under DIR: `OUT/<path under DIR>/<stem>.html`
//!                (skipped when newer than its log, unless --force), a
//!                `<stem>.summary.json` beside each page, and `OUT/index.html`
//!                listing every page with its outcome

use std::path::{Path, PathBuf};
use std::time::Instant;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::export::parallel;
use aurelia_qa::frames::{frames_of_log, Options};
use serde_json::{json, Value};

const PLAYER: &str = include_str!("../../replay/player.html");
const SLOT: &str = "/*FRAMES*/null";

struct Args {
    log: Option<PathBuf>,
    out: Option<PathBuf>,
    html: Option<PathBuf>,
    batch: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    jobs: usize,
    force: bool,
    title: Option<String>,
    options: Options,
}

fn args() -> Result<Args, String> {
    let mut a = Args { log: None, out: None, html: None, batch: None, out_dir: None, jobs: 1, force: false, title: None, options: Options::default() };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--out" => a.out = Some(PathBuf::from(value()?)),
            "--html" => a.html = Some(PathBuf::from(value()?)),
            "--batch" => a.batch = Some(PathBuf::from(value()?)),
            "--out-dir" => a.out_dir = Some(PathBuf::from(value()?)),
            "--jobs" => a.jobs = value()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            "--title" => a.title = Some(value()?),
            "--force" => a.force = true,
            "--whole" => a.options.whole = true,
            "--per-turn" => a.options.per_turn = Some(true),
            "--per-round" => a.options.per_turn = Some(false),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            path => a.log = Some(PathBuf::from(path)),
        }
    }
    Ok(a)
}

/// The player page with `frames` embedded (`</` escaped so the JSON cannot end the script).
fn page(frames: &Value) -> String {
    let data = serde_json::to_string(frames).unwrap_or_else(|_| "null".to_owned()).replace("</", "<\\/");
    PLAYER.replacen(SLOT, &data, 1)
}

/// What the index lists of a game.
fn summary(frames: &Value) -> Value {
    let scene = &frames["scene"];
    let b = &scene["battle"];
    let list = frames["frames"].as_array().cloned().unwrap_or_default();
    // Rounds as the frames name them (the round a turn is played in; the battle record names the
    // round begun after it).
    let round = |f: Option<&Value>| f.map_or(Value::Null, |f| f["round"].clone());
    let captured = list.iter().flat_map(|f| f["events"].as_array().cloned().unwrap_or_default()).find(|e| e["t"] == "captured" && e["at_i"][0] == scene["target_i"]).map_or(Value::Null, |e| e["r"].clone());
    json!({
        "era": scene["era"],
        "armies": scene["armies"],
        "variety": b["variety"],
        "first_round": round(list.first()),
        "last_round": round(list.last()),
        "captured_at": captured,
        "stem": scene["stem"],
        "kind": scene["kind"],
        "rung": b["rung"],
        "stage": b["stage"],
        "learner": b["learner"],
        "enemy": b["enemy"],
        "captured_round": b["captured_round"],
        "held": b["held"],
        "won_round": b["won_round"],
        "start_round": b["start_round"],
        "end_round": b["end_round"],
        "target_damage": b["target_damage"],
        "reward": b["reward"],
        "final_round": scene["final_round"],
        "standings": scene["standings"],
        "players": frames["players"],
        "frames": frames["frames"].as_array().map_or(0, Vec::len),
    })
}

fn logs_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            logs_under(&p, out);
        } else if p.extension().is_some_and(|x| x == "aurlog") {
            out.push(p);
        }
    }
}

fn newer(page: &Path, log: &Path) -> bool {
    let time = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    matches!((time(page), time(log)), (Some(a), Some(b)) if a >= b)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The index page of a batch: one row per game, the newest folder (iteration) first.
fn index(out_dir: &Path, rows: &[(PathBuf, Value)], title: &str) -> String {
    let mut rows: Vec<(String, String, &Value)> = rows
        .iter()
        .map(|(path, s)| {
            let rel = path.strip_prefix(out_dir).unwrap_or(path);
            let group = rel.parent().map(|p| p.display().to_string()).unwrap_or_default();
            (group, rel.display().to_string(), s)
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let name_of = |s: &Value, p: &Value| -> String {
        let id = p.as_u64().unwrap_or(0) as usize;
        let player = s["players"].get(id);
        let name = player.and_then(|x| x["name"].as_str()).filter(|n| !n.is_empty()).map_or_else(|| format!("игрок {id}"), str::to_owned);
        let mind = player.map_or(String::new(), |x| match x["mind"].as_str().unwrap_or("") {
            "learner" | "" if x["learner"] == true => "сеть".to_owned(),
            // A frozen net (`net:<path>/iter_0004.bin`): its checkpoint's name.
            m if m.starts_with("net:") => format!("сеть {}", m.rsplit('/').next().unwrap_or(m).trim_end_matches(".bin")),
            m => m.to_owned(),
        });
        if mind.is_empty() { name } else { format!("{name} ({mind})") }
    };
    let kind_name = |k: &str| match k {
        "siege" => "осада".to_owned(),
        "defence" => "оборона".to_owned(),
        "field" => "полевое сражение".to_owned(),
        "game" => "вся партия".to_owned(),
        other => other.to_owned(),
    };
    let iteration = |g: &str| g.strip_prefix("iter_").and_then(|n| n.split('-').next()?.parse::<u32>().ok()).map_or_else(|| g.to_owned(), |n| n.to_string());
    // A batch of whole games (books, benches) has no battle columns.
    let battles = rows.iter().any(|(_, _, s)| !s["learner"].is_null());
    let mut body = String::new();
    let (mut total, mut taken) = (0, 0);
    for (group, rel, s) in &rows {
        let kind = s["kind"].as_str().unwrap_or("");
        let (scene, outcome, ok) = if s["learner"].is_null() {
            let best = s["standings"].as_array().and_then(|v| v.iter().find(|x| x["rank"] == 1)).cloned().unwrap_or(Value::Null);
            let how = best["victory"].as_str().map_or_else(
                || "по очкам".to_owned(),
                |v| match v {
                    "time" => "победа по времени".to_owned(),
                    "science" => "научная победа".to_owned(),
                    "domination" => "победа завоеванием".to_owned(),
                    "culture" => "культурная победа".to_owned(),
                    "diplomatic" => "дипломатическая победа".to_owned(),
                    other => format!("победа: {other}"),
                },
            );
            let mut outcome = format!("первое место — {}, {how}", esc(&name_of(s, &best["player"])));
            // The learner's seat (a book, the net): its place among the majors.
            let seat = s["players"].as_array().and_then(|v| v.iter().find(|x| x["learner"] == true)).map(|x| x["id"].clone());
            let standing = seat.as_ref().and_then(|id| s["standings"].as_array()?.iter().find(|x| x["player"] == *id).cloned());
            if let (Some(id), Some(st)) = (seat, standing) {
                let majors = s["standings"].as_array().map_or(0, Vec::len);
                outcome = format!("<b>{}</b>: место {} из {majors}, {} очков; {outcome}", esc(&name_of(s, &id)), st["rank"], st["score"]);
            }
            (kind_name(kind), outcome, None)
        } else {
            total += 1;
            let captured = !s["captured_round"].is_null();
            let held = s["held"].as_bool().unwrap_or(false);
            let won = !s["won_round"].is_null();
            let ok = if kind == "siege" { captured } else { held || captured || won };
            taken += usize::from(ok);
            let scene = format!("{}: {} — {}", kind_name(kind), esc(&name_of(s, &s["learner"])), esc(&name_of(s, &s["enemy"])));
            let outcome = if kind == "siege" {
                if captured {
                    format!("взят на ходу {}", if s["captured_at"].is_null() { &s["captured_round"] } else { &s["captured_at"] })
                } else {
                    format!("устоял, урон {:.0} %", s["target_damage"].as_f64().unwrap_or(0.0) * 100.0)
                }
            } else if won {
                format!("победа на ходу {}", s["won_round"])
            } else if ok {
                "задача выполнена".to_owned()
            } else {
                "задача не выполнена".to_owned()
            };
            (scene, outcome, Some(ok))
        };
        let mark = match ok {
            Some(true) => "<span class=\"ok\">●</span> ",
            Some(false) => "<span class=\"no\">●</span> ",
            None => "",
        };
        let rounds = match (s["first_round"].as_u64(), s["last_round"].as_u64().filter(|_| !s["learner"].is_null())) {
            (Some(a), Some(b)) => format!("{a}–{b}"),
            _ => format!("{}", s["final_round"]),
        };
        let rung = s["rung"].as_u64().map_or_else(String::new, |r| r.to_string());
        let (era_index, era) = (s["era"]["index"].as_u64().unwrap_or(0), s["era"]["name"].as_str().unwrap_or("—"));
        let army = |side: &str| {
            s["armies"][side].as_array().map_or_else(String::new, |v| {
                v.iter().filter_map(|x| Some(format!("{} × {}", x[1].as_u64()?, x[0].as_str()?))).collect::<Vec<_>>().join(", ")
            })
        };
        let forces = if s["armies"].is_null() {
            String::new()
        } else {
            let enemy = army("enemy");
            format!("<b>{}</b><br><span class=\"muted\">против:</span> {}", esc(&army("learner")), if enemy.is_empty() { "только город".to_owned() } else { esc(&enemy) })
        };
        if !battles {
            body += &format!(
                "<tr data-era=\"{era_index}\" data-ok=\"\"><td>{}</td><td>{}</td><td><a href=\"{}\">{outcome}</a></td><td>{rounds}</td></tr>",
                esc(&iteration(group)),
                esc(era),
                esc(rel)
            );
            continue;
        }
        body += &format!(
            "<tr data-era=\"{era_index}\" data-ok=\"{}\"><td>{}</td><td>{}</td><td>{rung}</td><td>{mark}<a href=\"{}\">{outcome}</a></td><td>{forces}</td><td>{scene}</td><td>{rounds}</td></tr>",
            ok.map_or("", |o| if o { "1" } else { "0" }),
            esc(&iteration(group)),
            esc(era),
            esc(rel)
        );
    }
    let lead = if total > 0 { format!("<p class=\"lead\">Цель взята или удержана в {taken} из {total} сражений.</p>") } else { String::new() };
    format!(
        r#"<!doctype html><html lang="ru"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{t}</title>
<style>:root{{--bg:#fbfaf7;--fg:#1f1d1a;--muted:#6b665e;--line:#e3ded5;--card:#fff;--a:#2c5d8a;--ok:#2f7d4f;--no:#b4442f}}
@media (prefers-color-scheme: dark){{:root:not([data-theme="light"]){{--bg:#16161a;--fg:#e8e6e1;--muted:#a09a90;--line:#33322f;--card:#1e1e23;--a:#7fb2e0;--ok:#6cc28f;--no:#e58a76}}}}
body{{background:var(--bg);color:var(--fg);font:15px/1.5 -apple-system,sans-serif;margin:0;padding:20px 16px 48px}}main{{max-width:900px;margin:0 auto}}
h1{{font-size:22px;margin:0 0 6px}}a{{color:var(--a)}}.lead{{background:var(--card);border:1px solid var(--line);border-radius:6px;padding:10px 14px}}
.wrap{{overflow-x:auto}}table{{border-collapse:collapse;width:100%;font-size:14px}}td,th{{border-bottom:1px solid var(--line);padding:5px 6px;text-align:left;vertical-align:top}}
th{{color:var(--muted);font-weight:normal}}td:nth-child(1),td:nth-child(3),td:nth-child(7){{font-variant-numeric:tabular-nums;white-space:nowrap}}td:nth-child(4){{min-width:140px}}td:nth-child(5){{min-width:220px}}
.ok{{color:var(--ok)}}.no{{color:var(--no)}}.muted{{color:var(--muted)}}
.filters{{display:flex;flex-wrap:wrap;gap:6px;margin:12px 0}}.filters button{{font:inherit;font-size:14px;color:var(--fg);background:var(--card);border:1px solid var(--line);border-radius:14px;padding:3px 12px;cursor:pointer}}
.filters button.on{{border-color:var(--a);color:var(--a);font-weight:600}}.filters label{{font-size:14px;margin-left:8px;align-self:center}}tr.group td{{font-weight:600;background:var(--card);padding-top:10px}}</style></head><body><main>
<h1>{t}</h1><p class="muted">{intro}</p>{lead}
<div class="filters" id="filters"></div>
<div class="wrap"><table id="games"><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table></div></main>
<script>{script}</script></body></html>"#,
        script = INDEX_SCRIPT,
        intro = if battles {
            "Нажмите на итог, чтобы посмотреть сражение по ходам: карта, отряды, атаки."
        } else {
            "Нажмите на итог, чтобы посмотреть партию: карта по ходам и решения игрока — исследования, постройки, политики, вера."
        },
        head = if battles {
            "<th>круг</th><th>эпоха</th><th>ступень</th><th>итог</th><th>силы в начале</th><th>стороны</th><th>ходы</th>"
        } else {
            "<th>группа</th><th>эпоха в конце</th><th>итог</th><th>ходов</th>"
        },
        t = esc(title)
    )
}

/// The index's era filter and grouping: one button per era present (with its capture share), and
/// "group by era" that orders the rows by era under a heading each.
const INDEX_SCRIPT: &str = r##"
(function () {
  const body = document.querySelector("#games tbody"); if (!body) return;
  const rows = Array.from(body.querySelectorAll("tr[data-era]"));
  const eras = new Map();
  for (const r of rows) { const k = +r.dataset.era; const name = r.children[1].textContent;
    const e = eras.get(k) || { name, n: 0, ok: 0, known: 0 }; e.n++; if (r.dataset.ok !== "") e.known++; if (r.dataset.ok === "1") e.ok++; eras.set(k, e); }
  const cols = document.querySelectorAll("#games thead th").length;
  const keys = Array.from(eras.keys()).sort((a, b) => a - b);
  const box = document.getElementById("filters");
  let only = null, grouped = false;
  const share = (e) => e.known ? Math.round(100 * e.ok / e.known) + " %" : "";
  function render() {
    body.querySelectorAll("tr.group").forEach((g) => g.remove());
    const list = rows.filter((r) => only === null || +r.dataset.era === only);
    rows.forEach((r) => r.style.display = list.includes(r) ? "" : "none");
    if (grouped) {
      const order = list.slice().sort((a, b) => (+a.dataset.era) - (+b.dataset.era));
      let last = null;
      for (const r of order) { const k = +r.dataset.era;
        if (k !== last) { const g = document.createElement("tr"); g.className = "group"; const e = eras.get(k);
          g.innerHTML = '<td colspan="' + cols + '">' + e.name + (e.known ? " — взято " + e.ok + " из " + e.known : " — " + e.n) + "</td>"; body.appendChild(g); last = k; }
        body.appendChild(r); }
    } else rows.forEach((r) => body.appendChild(r));
    box.querySelectorAll("button").forEach((b) => b.classList.toggle("on", (b.dataset.era === "" ? null : +b.dataset.era) === only));
  }
  const btn = (label, era) => { const b = document.createElement("button"); b.textContent = label; b.dataset.era = era === null ? "" : era;
    b.onclick = () => { only = era; render(); }; box.appendChild(b); };
  btn("все эпохи · " + rows.length, null);
  for (const k of keys) { const e = eras.get(k); btn(e.name + " · " + e.n + (e.known ? ", взято " + share(e) : ""), k); }
  const l = document.createElement("label"); l.innerHTML = '<input type="checkbox"> группировать по эпохам';
  l.querySelector("input").onchange = (ev) => { grouped = ev.target.checked; render(); }; box.appendChild(l);
  render();
})();
"##;

fn run() -> Result<(), String> {
    let a = args()?;
    let rules = ClassicBench::rules();
    if let Some(dir) = &a.batch {
        let out_dir = a.out_dir.clone().ok_or("--batch needs --out-dir")?;
        let mut logs = Vec::new();
        logs_under(dir, &mut logs);
        let start = Instant::now();
        let results = parallel(&logs, a.jobs.max(1), |log| -> Result<(PathBuf, Value, bool), String> {
            let rel = log.strip_prefix(dir).unwrap_or(log);
            let html = out_dir.join(rel).with_extension("html");
            let summary_path = out_dir.join(rel).with_extension("summary.json");
            if !a.force && newer(&html, log) && summary_path.exists() {
                let s: Value = serde_json::from_slice(&std::fs::read(&summary_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                return Ok((html, s, false));
            }
            let frames = frames_of_log(&rules, log, a.options).map_err(|e| format!("{}: {e}", log.display()))?;
            std::fs::create_dir_all(html.parent().unwrap_or(&out_dir)).map_err(|e| e.to_string())?;
            std::fs::write(&html, page(&frames)).map_err(|e| format!("{}: {e}", html.display()))?;
            let s = summary(&frames);
            std::fs::write(&summary_path, serde_json::to_vec_pretty(&s).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            Ok((html, s, true))
        });
        let mut rows = Vec::new();
        let mut built = 0;
        for r in results {
            match r {
                Ok((html, s, fresh)) => {
                    built += usize::from(fresh);
                    rows.push((html, s));
                }
                Err(e) => eprintln!("skipped: {e}"),
            }
        }
        let title = a.title.clone().unwrap_or_else(|| format!("Записи партий: {}", dir.file_name().and_then(|s| s.to_str()).unwrap_or("")));
        let index_path = out_dir.join("index.html");
        std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
        std::fs::write(&index_path, index(&out_dir, &rows, &title)).map_err(|e| e.to_string())?;
        eprintln!("{} pages ({built} built) in {:.1}s; index {}", rows.len(), start.elapsed().as_secs_f64(), index_path.display());
        return Ok(());
    }
    let log = a.log.clone().ok_or("usage: ai-replay-frames LOG.aurlog [--out FRAMES.json] [--html PAGE.html] | --batch DIR --out-dir OUT")?;
    let start = Instant::now();
    let frames = frames_of_log(&rules, &log, a.options)?;
    if let Some(out) = &a.out {
        std::fs::write(out, serde_json::to_vec(&frames).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    let html = a.html.clone().or_else(|| a.out.is_none().then(|| log.with_extension("html")));
    if let Some(html) = &html {
        std::fs::write(html, page(&frames)).map_err(|e| format!("{}: {e}", html.display()))?;
    }
    eprintln!(
        "{} frames, {} commands, {} round hashes checked in {:.1}s{}",
        frames["frames"].as_array().map_or(0, Vec::len),
        frames["scene"]["commands"],
        frames["scene"]["checked"],
        start.elapsed().as_secs_f64(),
        html.map(|h| format!("; page {}", h.display())).unwrap_or_default()
    );
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("ai-replay-frames: {e}");
        std::process::exit(1);
    }
}
