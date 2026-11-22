//! The situation exam (`aurelia_qa::stress`; classic/docs/ai-stress.md).
//!
//! `ai-stress run --positions DIR --out DIR [--situations a5,b1] [--instances 100] [--round 100] [--jitter 10]
//!   [--jobs 2] [--no-end]`
//!   plays every (situation, instance) not yet in DIR/rows.jsonl: the logs of DIR (`ai-play --stop-round`)
//!   replayed to a round, the edit at three strengths, the control once; the edited starts go to DIR/scenes/.
//!   Situations that cannot be built on a position go to DIR/unbuilt.jsonl.
//! `ai-stress one --positions DIR --situation a5 --instance 3 [--level 2]`
//!   plays one instance with a line a round on stderr (`STRESS_TRACE`), prints the row.
//! `ai-stress rebuild --scene DIR/scenes/a5-003-L2.json`
//!   builds a saved edited start again and checks its world hash.
//! `ai-stress table --variant NAME --out DIR`
//!   the suite's table from the rows DIR/NAME/ and DIR/standard/ hold, with no play.
//! `ai-stress random --positions DIR --out DIR --samples 200 [--seed 1] [--jobs 2]`
//!   random combinations of the primitives at random strengths on the positions, each played edited
//!   against control; DIR/random.jsonl and DIR/ranking.md, ranked by the damage to the seat's outcome.
//! `ai-stress gates`
//!   the gates as the document's table.
//! `ai-stress report --out DIR`
//!   the result table against the gates (Markdown).
//! `ai-stress suite --variant NAME --positions DIR --out DIR [--situations all] [--instances 100] [--jobs 2] [--no-end]`
//!   the whole exam on the AI variant NAME (every seat played on by it) and on `standard`, the
//!   reference: DIR/NAME/ and DIR/standard/ as `run` writes them (a rerun plays only what is missing),
//!   and DIR/suite-NAME.md, the table "situation x strength: pass/fail, outcome delta, behaviour".
//!   `--variant` also works for `run` and `one`.
//!
//! A measurement: it changes no rule and no AI.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::stress::{self, gates, position, Row, Saved, Situation};
use classic_rules::ai::variant::Variant;

fn main() {
    if let Err(e) = real() {
        eprintln!("ai-stress: {e}");
        std::process::exit(1);
    }
}

#[derive(Clone)]
struct Opts {
    cmd: String,
    positions: PathBuf,
    out: PathBuf,
    scene: Option<PathBuf>,
    situations: Vec<Situation>,
    instances: usize,
    instance: usize,
    round: u32,
    jitter: u32,
    jobs: usize,
    level: Option<u8>,
    /// Play each arm out to the end of the game after its window (the end-of-game outcome).
    to_end: bool,
    samples: usize,
    seed: u64,
    /// The AI that plays on (the log's own when none).
    variant: Option<String>,
}

fn opts() -> Result<Opts, String> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().ok_or("run | one | rebuild | report")?;
    let mut o = Opts { cmd, positions: PathBuf::from("positions"), out: PathBuf::from("stress"), scene: None, situations: vec![Situation::A5, Situation::B1], instances: 100, instance: 0, round: 100, jitter: 10, jobs: 2, level: None, to_end: true, samples: 200, seed: 1, variant: None };
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} needs a value"));
        let number = |v: String| v.parse::<usize>().map_err(|e| format!("{a}: {e}"));
        match a.as_str() {
            "--positions" => o.positions = PathBuf::from(value()?),
            "--out" => o.out = PathBuf::from(value()?),
            "--scene" => o.scene = Some(PathBuf::from(value()?)),
            "--situations" => {
                let list = value()?;
                o.situations = if list == "all" { Situation::all() } else { list.split(',').map(Situation::parse).collect::<Result<_, _>>()? };
            }
            "--variant" => o.variant = Some(value()?),
            "--situation" => o.situations = vec![Situation::parse(&value()?)?],
            "--instances" => o.instances = number(value()?)?,
            "--instance" => o.instance = number(value()?)?,
            "--round" => o.round = number(value()?)? as u32,
            "--jitter" => o.jitter = number(value()?)? as u32,
            "--jobs" => o.jobs = number(value()?)?,
            "--samples" => o.samples = number(value()?)?,
            "--seed" => o.seed = number(value()?)? as u64,
            "--no-end" => o.to_end = false,
            "--level" => o.level = Some(number(value()?)? as u8),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(o)
}

fn real() -> Result<(), String> {
    let o = opts()?;
    let rules = ClassicBench::rules();
    match o.cmd.as_str() {
        "run" => run(&o, &rules),
        "one" => one(&o, &rules),
        "rebuild" => {
            let path = o.scene.as_ref().ok_or("--scene FILE")?;
            let saved: Saved = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            stress::rebuild(&rules, &saved)?;
            println!("rebuilt {} {} level {}: the world hash {} matches", saved.situation, saved.instance, saved.level, saved.world);
            Ok(())
        }
        "report" => report(&o),
        "suite" => suite(&o, &rules),
        "table" => table(&o),
        "random" => random(&o, &rules),
        "gates" => {
            print!("{}\n{}\n{}", gates::markdown(), stress::humans::gates::markdown(), stress::humans::gates::offence_markdown());
            Ok(())
        }
        other => Err(format!("unknown command {other}")),
    }
}

fn read_rows(out: &std::path::Path) -> Vec<Row> {
    let text = std::fs::read_to_string(out.join("rows.jsonl")).unwrap_or_default();
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn variant_of(o: &Opts) -> Result<Option<Variant>, String> {
    match &o.variant {
        Some(name) => Variant::from_name(name).map(Some).ok_or_else(|| format!("no AI variant {name}")),
        None => Ok(None),
    }
}

fn run(o: &Opts, rules: &classic_rules::CompiledRuleset) -> Result<(), String> {
    std::fs::create_dir_all(&o.out).map_err(|e| e.to_string())?;
    let variant = variant_of(o)?;
    let logs = position::logs(&o.positions)?;
    let done: std::collections::BTreeSet<(String, usize)> = read_rows(&o.out).into_iter().map(|r| (r.situation, r.instance)).collect();
    let open = |name: &str| std::fs::OpenOptions::new().create(true).append(true).open(o.out.join(name)).map_err(|e| e.to_string());
    let rows = Mutex::new(open("rows.jsonl")?);
    let unbuilt = Mutex::new(open("unbuilt.jsonl")?);
    let scenes = o.out.join("scenes");
    let indexes: Vec<usize> = (0..o.instances.min(logs.len())).collect();
    let started = std::time::Instant::now();
    let count = std::sync::atomic::AtomicUsize::new(0);
    aurelia_qa::export::parallel(&indexes, o.jobs, |&i| {
        let todo: Vec<Situation> = o.situations.iter().copied().filter(|s| !done.contains(&(s.name().to_owned(), i))).collect();
        if todo.is_empty() {
            return;
        }
        let log = std::fs::canonicalize(&logs[i]).unwrap_or_else(|_| logs[i].clone());
        let result: Result<stress::Done, String> = Ok(stress::run_log(rules, &log, i, &todo, (o.round, o.jitter), variant, Some(&scenes), o.to_end));
        let note = |situation: &str, error: &str| {
            let line = serde_json::json!({"situation": situation, "instance": i, "error": error});
            let _ = writeln!(unbuilt.lock().expect("one writer at a time"), "{line}");
        };
        match result {
            Ok(done) => {
                for row in &done.rows {
                    let line = serde_json::to_string(row).expect("a row serialises");
                    let _ = writeln!(rows.lock().expect("one writer at a time"), "{line}");
                }
                for (situation, error) in &done.unbuilt {
                    note(situation, error);
                }
                let n = count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                println!("{n} instances, {:.0} s", started.elapsed().as_secs_f64());
            }
            Err(e) => note("position", &e),
        }
    });
    println!("done in {:.0} s", started.elapsed().as_secs_f64());
    Ok(())
}

fn one(o: &Opts, rules: &classic_rules::CompiledRuleset) -> Result<(), String> {
    std::env::set_var("STRESS_TRACE", "1");
    let logs = position::logs(&o.positions)?;
    let log = logs.get(o.instance).ok_or("no such instance")?;
    let done = stress::run_log(rules, log, o.instance, &o.situations, (o.round, o.jitter), variant_of(o)?, None, o.to_end);
    for row in done.rows.iter().filter(|r| o.level.is_none_or(|l| r.level == l)) {
        println!("{}", serde_json::to_string_pretty(row).map_err(|e| e.to_string())?);
    }
    for (s, e) in &done.unbuilt {
        println!("unbuilt {s}: {e}");
    }
    Ok(())
}

fn report(o: &Opts) -> Result<(), String> {
    let rows = read_rows(&o.out);
    let results = gates::judge(&rows);
    println!("{} rows\n\n{}", rows.len(), gates::results_markdown(&results));
    Ok(())
}

/// The whole exam on a variant and on `standard`, and the table of the two.
fn suite(o: &Opts, rules: &classic_rules::CompiledRuleset) -> Result<(), String> {
    let name = o.variant.clone().ok_or("--variant NAME")?;
    variant_of(o)?;
    let situations = if o.situations == [Situation::A5, Situation::B1] { Situation::all() } else { o.situations.clone() };
    let mut arms = vec![name.clone()];
    if name != "standard" {
        arms.push("standard".to_owned());
    }
    for arm in &arms {
        let each = Opts { cmd: "run".to_owned(), out: o.out.join(arm), variant: Some(arm.clone()), situations: situations.clone(), positions: o.positions.clone(), scene: None, ..o.clone() };
        println!("suite: {arm}");
        run(&each, rules)?;
    }
    let candidate = read_rows(&o.out.join(&name));
    let reference = if name == "standard" { None } else { Some(read_rows(&o.out.join("standard"))) };
    let table = stress::suite::table(&name, &candidate, reference.as_deref());
    let path = o.out.join(format!("suite-{name}.md"));
    std::fs::write(&path, &table).map_err(|e| e.to_string())?;
    println!("{table}\n{}", path.display());
    Ok(())
}

/// The suite's table from the rows a suite (or runs into `OUT/NAME/`, `OUT/standard/`) wrote, with
/// no play: a suite played over several sets of positions is tabled once.
fn table(o: &Opts) -> Result<(), String> {
    let name = o.variant.clone().ok_or("--variant NAME")?;
    let candidate = read_rows(&o.out.join(&name));
    let reference = if name == "standard" { None } else { Some(read_rows(&o.out.join("standard"))) };
    let table = stress::suite::table(&name, &candidate, reference.as_deref());
    let path = o.out.join(format!("suite-{name}.md"));
    std::fs::write(&path, &table).map_err(|e| e.to_string())?;
    println!("{table}\n{}", path.display());
    Ok(())
}

/// The damage of a sample: how much the seat's outcome fell, edited against control (the end of the
/// game's score share and victory weight when played out, else the window's score share).
fn damage(row: &Row) -> f64 {
    let d = |k: &str| row.control.get(k).copied().unwrap_or(0.0) - row.edited.get(k).copied().unwrap_or(0.0);
    if row.edited.contains_key("end_seat_score_share") {
        d("end_seat_score_share") + 0.25 * d("end_seat_win_weight")
    } else {
        d("seat_score_share")
    }
}

/// The share of the score the edit took at once (before any play).
fn start_damage(row: &Row) -> f64 {
    row.control.get("start_seat_score_share").copied().unwrap_or(0.0) - row.edited.get("start_seat_score_share").copied().unwrap_or(0.0)
}

/// The ranking of the samples by damage, and the worst sample of each of the first `distinct`
/// combinations (the sorted kinds of their primitives) in that order.
fn ranking(rows: &mut [Row], distinct: usize) -> String {
    rows.sort_by(|a, b| damage(b).total_cmp(&damage(a)));
    let combination = |r: &Row| r.info.get("combination").cloned().unwrap_or_default();
    let mut md = String::from("| rank | damage | at once | after the edit | sample | position | combination | recipe |\n|---|---|---|---|---|---|---|---|\n");
    for (rank, r) in rows.iter().enumerate().take(30) {
        md.push_str(&format!("| {} | {:+.3} | {:+.3} | {:+.3} | {} | {} r{} | {} | {} |\n", rank + 1, damage(r), start_damage(r), damage(r) - start_damage(r), r.instance, r.position, r.round, combination(r), r.info.get("recipe").map_or("", String::as_str)));
    }
    let mut seen: Vec<String> = Vec::new();
    md.push_str("\nThe worst distinct combinations (each at its worst sample; mean damage over all its samples):\n\n| combination | worst damage | samples | mean damage | mean after the edit |\n|---|---|---|---|---|\n");
    for r in rows.iter() {
        let c = combination(r);
        if seen.contains(&c) || seen.len() == distinct {
            continue;
        }
        let same: Vec<&Row> = rows.iter().filter(|x| combination(x) == c).collect();
        let mean = same.iter().map(|x| damage(x)).sum::<f64>() / same.len() as f64;
        let after = same.iter().map(|x| damage(x) - start_damage(x)).sum::<f64>() / same.len() as f64;
        md.push_str(&format!("| {c} | {:+.3} | {} | {mean:+.3} | {after:+.3} |\n", damage(r), same.len()));
        seen.push(c);
    }
    md
}

fn random(o: &Opts, rules: &classic_rules::CompiledRuleset) -> Result<(), String> {
    std::fs::create_dir_all(&o.out).map_err(|e| e.to_string())?;
    let variant = variant_of(o)?;
    let logs = position::logs(&o.positions)?;
    let path = o.out.join("random.jsonl");
    let done: std::collections::BTreeSet<usize> = std::fs::read_to_string(&path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Row>(l).ok()).map(|r| r.instance).collect();
    let file = Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?);
    let samples: Vec<usize> = (0..o.samples).filter(|s| !done.contains(s)).collect();
    aurelia_qa::export::parallel(&samples, o.jobs, |&i| {
        let at = (position::mix(o.seed ^ 0xface ^ i as u64) % logs.len() as u64) as usize;
        let log = std::fs::canonicalize(&logs[at]).unwrap_or_else(|_| logs[at].clone());
        let loaded = position::load(rules, &log, position::round_of(at, o.round, o.jitter));
        let row = loaded.and_then(|mut p| {
            if let Some(v) = variant {
                p.variant = v;
            }
            stress::run_random(rules, &p, i, o.seed, o.to_end)
        });
        match row {
            Ok(row) => {
                let line = serde_json::to_string(&row).expect("a row serialises");
                let _ = writeln!(file.lock().expect("one writer at a time"), "{line}");
            }
            Err(e) => eprintln!("sample {i}: {e}"),
        }
    });
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut rows: Vec<Row> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let md = ranking(&mut rows, 10);
    std::fs::write(o.out.join("ranking.md"), &md).map_err(|e| e.to_string())?;
    println!("{} samples\n\n{md}", rows.len());
    Ok(())
}
