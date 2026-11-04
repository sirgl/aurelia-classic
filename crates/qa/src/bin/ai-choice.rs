//! The choice dataset: AI-only games played by the classic AI with
//! exploration (`classic_rules::ai::choice`), every production and research
//! choice written with the rows of its set and what followed
//! (`aurelia_qa::choice`), a dataset for the choice net.
//!
//! `cargo run --release --offline -p aurelia-qa --bin ai-choice -- --out PREFIX [options]`
//!
//! --out PREFIX         writes PREFIX.production.csv and PREFIX.research.csv (required)
//! --seeds A..B         the games' seeds, B excluded (default 1..21); A..=B includes B
//! --map SCRIPT         the map script (default pangaea)
//! --size NAME          the world size (default small)
//! --rounds N           rounds to play (the round of the time victory; default 300)
//! --variant NAME       the AI variant (default standard)
//! --explore P          chance of a random choice among the top, per mille (default 150; 0: the variant's own choice)
//! --top K              the random choice is among the best K (default 4)
//! --jobs J             games played at once (default: the machine's threads)
//!
//! The files hold no clock: the same options give the same files, the
//! games' lines in seed order. A set's id is the seed × 1 000 000 plus its
//! number in the game.

use std::collections::BTreeMap;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::choice::{header, lines, probe_game};
use aurelia_qa::spec::{size_from_name, GameSpec};
use classic_rules::ai::choice_net::Kind;
use classic_rules::ai::variant::Variant;
use classic_rules::scenario::MapScript;

struct Options {
    seeds: (u64, u64),
    map: String,
    size: String,
    rounds: u32,
    variant: String,
    explore: u32,
    top: usize,
    jobs: usize,
    out: Option<PathBuf>,
}

fn seeds(text: &str) -> Result<(u64, u64), String> {
    let parse = |s: &str| s.parse::<u64>().map_err(|e| format!("--seeds {text}: {e}"));
    let (from, to) = match text.split_once("..=") {
        Some((a, b)) => (parse(a)?, parse(b)? + 1),
        None => match text.split_once("..") {
            Some((a, b)) => (parse(a)?, parse(b)?),
            None => (parse(text)?, parse(text)? + 1),
        },
    };
    if to <= from {
        return Err(format!("--seeds {text}: no seed"));
    }
    Ok((from, to))
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        seeds: (1, 21),
        map: "pangaea".to_owned(),
        size: "small".to_owned(),
        rounds: 300,
        variant: "standard".to_owned(),
        explore: 150,
        top: 4,
        jobs: std::thread::available_parallelism().map_or(1, |n| n.get()),
        out: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--seeds" => options.seeds = seeds(&value()?)?,
            "--map" => options.map = value()?,
            "--size" => options.size = value()?,
            "--rounds" => options.rounds = value()?.parse().map_err(|e| format!("--rounds: {e}"))?,
            "--variant" => options.variant = value()?,
            "--explore" => options.explore = value()?.parse().map_err(|e| format!("--explore: {e}"))?,
            "--top" => options.top = value()?.parse().map_err(|e| format!("--top: {e}"))?,
            "--jobs" => options.jobs = value()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            "--out" => options.out = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown option {other}")),
        }
    }
    if options.explore > 1000 || options.top == 0 {
        return Err("--explore is per mille (0..=1000), --top positive".to_owned());
    }
    Ok(options)
}

fn main() {
    if let Err(error) = options().and_then(|options| run(&options)) {
        eprintln!("ai-choice: {error}");
        std::process::exit(1);
    }
}

/// The games written so far, those done and waiting for an earlier one,
/// the two files, and every game written.
type Pending = (usize, BTreeMap<usize, Result<Done, String>>, Vec<BufWriter<std::fs::File>>, Vec<Done>);

/// One game's lines and counts.
struct Done {
    label: String,
    text: [Vec<u8>; 2],
    /// Sets and explored sets by kind.
    sets: [u64; 2],
    explored: [u64; 2],
    rounds: u32,
    hash: u64,
    victory: Option<&'static str>,
    seconds: f64,
    snapshot_seconds: f64,
}

fn run(options: &Options) -> Result<(), String> {
    let prefix = options.out.as_ref().ok_or("--out PREFIX is required")?;
    let script = MapScript::from_key(&options.map).ok_or_else(|| format!("unknown map script {}", options.map))?;
    let size = size_from_name(&options.size).ok_or_else(|| format!("unknown world size {}", options.size))?;
    let variant = Variant::from_name(&options.variant)
        .ok_or_else(|| format!("unknown variant {} ({})", options.variant, ClassicBench::variants().join(", ")))?;
    let specs: Vec<GameSpec> = (options.seeds.0..options.seeds.1)
        .map(|seed| GameSpec::new(script, size, seed).max_rounds(options.rounds))
        .collect();
    let path = |kind: Kind| PathBuf::from(format!("{}.{}.csv", prefix.display(), kind.name()));
    let mut writers = Vec::new();
    for kind in [Kind::Production, Kind::Research] {
        let file = std::fs::File::create(path(kind)).map_err(|e| format!("{}: {e}", path(kind).display()))?;
        let mut writer = BufWriter::new(file);
        writeln!(writer, "{}", header(kind)).map_err(|e| e.to_string())?;
        writers.push(writer);
    }
    let started = Instant::now();
    // The games' lines are written in seed order as they come.
    let pending: Mutex<Pending> = Mutex::new((0, BTreeMap::new(), writers, Vec::new()));
    let next = AtomicUsize::new(0);
    let error: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.clamp(1, specs.len().max(1)) {
            scope.spawn(|| {
                let rules = ClassicBench::rules();
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(spec) = specs.get(at) else { break };
                    let done = play(&rules, spec, variant, options);
                    let mut guard = pending.lock().expect("no thread panicked holding the files");
                    let (written, waiting, writers, finished) = &mut *guard;
                    waiting.insert(at, done);
                    while let Some(done) = waiting.remove(written) {
                        *written += 1;
                        match done {
                            Ok(mut done) => {
                                for (writer, text) in writers.iter_mut().zip(&mut done.text) {
                                    if let Err(e) = writer.write_all(text) {
                                        *error.lock().expect("no panic") = Some(e.to_string());
                                    }
                                    *text = Vec::new();
                                }
                                println!(
                                    "{}: {} rounds, {} production sets ({} explored), {} research sets ({} explored), {}, {:.1} s (snapshots {:.1} s), hash {:016x}",
                                    done.label,
                                    done.rounds,
                                    done.sets[0],
                                    done.explored[0],
                                    done.sets[1],
                                    done.explored[1],
                                    done.victory.unwrap_or("no victory"),
                                    done.seconds,
                                    done.snapshot_seconds,
                                    done.hash
                                );
                                finished.push(done);
                            }
                            Err(e) => *error.lock().expect("no panic") = Some(e),
                        }
                    }
                }
            });
        }
    });
    if let Some(error) = error.into_inner().expect("no panic") {
        return Err(error);
    }
    let (_, _, writers, finished) = pending.into_inner().expect("no thread panicked holding the files");
    for mut writer in writers {
        writer.flush().map_err(|e| e.to_string())?;
    }
    let seconds = started.elapsed().as_secs_f64();
    let sum = |f: &dyn Fn(&Done) -> u64| finished.iter().map(f).sum::<u64>();
    let game_seconds: f64 = finished.iter().map(|d| d.seconds).sum();
    let snapshot_seconds: f64 = finished.iter().map(|d| d.snapshot_seconds).sum();
    let mut victories: BTreeMap<&str, u32> = BTreeMap::new();
    for done in &finished {
        *victories.entry(done.victory.unwrap_or("none")).or_default() += 1;
    }
    println!(
        "{} games, {seconds:.1} s wall ({:.2} s a game on one thread, of it snapshots {:.2} s); production: {} sets, {} explored; research: {} sets, {} explored; victories {victories:?}",
        finished.len(),
        game_seconds / finished.len().max(1) as f64,
        snapshot_seconds / finished.len().max(1) as f64,
        sum(&|d| d.sets[0]),
        sum(&|d| d.explored[0]),
        sum(&|d| d.sets[1]),
        sum(&|d| d.explored[1]),
    );
    println!("{}, {}", path(Kind::Production).display(), path(Kind::Research).display());
    Ok(())
}

fn play(rules: &classic_rules::CompiledRuleset, spec: &GameSpec, variant: Variant, options: &Options) -> Result<Done, String> {
    let game = probe_game(rules, spec, variant, options.explore, options.top).map_err(|e| format!("{}: {e}", spec.label()))?;
    let mut text = [Vec::new(), Vec::new()];
    let (mut sets, mut explored) = ([0u64; 2], [0u64; 2]);
    for (number, row) in game.rows.iter().enumerate() {
        let k = usize::from(row.record.kind == Kind::Research);
        sets[k] += 1;
        explored[k] += u64::from(row.record.explored);
        let set = spec.seed * 1_000_000 + number as u64;
        lines(row, set, &game.end, &mut text[k]).map_err(|e| e.to_string())?;
    }
    Ok(Done {
        label: spec.label(),
        text,
        sets,
        explored,
        rounds: game.rounds,
        hash: game.hash,
        victory: game.end.victory,
        seconds: game.seconds,
        snapshot_seconds: game.snapshot_seconds,
    })
}
