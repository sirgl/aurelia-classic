//! The danger dataset: AI-only games played by the classic AI with every
//! military land unit's features at the end of its owner's turn and the
//! damage it took before the next written to a CSV file
//! (`aurelia_qa::danger`), a dataset for a learned danger model, and the
//! quality of the tactical search's own estimate on it, the baseline. With
//! `--battles` the seeds are synthetic battles (`aurelia_qa::battle`): the
//! game of each seed is played to its war, then the battle is probed for
//! `--rounds` rounds from its start and the AI memory of the rounds before.
//!
//! `cargo run --release --offline -p aurelia-qa --bin ai-danger -- --out FILE [options]`
//!
//! --out FILE           the CSV file to write (required)
//! --seeds A..B         the games' seeds, B excluded (default 1..21); A..=B includes B
//! --map SCRIPT         the map script (default pangaea)
//! --size NAME          the world size (default small)
//! --rounds N           rounds to play (and the round of the time victory; default 200;
//!                      a battle's rounds after its war begins, default 30)
//! --battles            the seeds are battles (pangaea of --size); a seed whose battle
//!                      cannot be set is skipped
//! --variant NAME       the AI variant (default standard)
//! --jobs J             games played at once (default: the machine's threads)
//! --stats              print the dataset's statistics and the baseline's quality
//!
//! The file holds no clock: the same options give the same file, the games'
//! rows in seed order.

use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::battle::BattleSpec;
use aurelia_qa::danger::{probe_battle, probe_game, write_csv, Cost, Probed, Quality, Row};
use aurelia_qa::record::new_game;
use aurelia_qa::spec::{size_from_name, GameSpec};
use classic_rules::ai::variant::Variant;
use classic_rules::scenario::MapScript;
use classic_rules::CompiledRuleset;
use kernel::ai::plan::AiSettings;

struct Options {
    seeds: (u64, u64),
    map: String,
    size: String,
    rounds: Option<u32>,
    battles: bool,
    variant: String,
    jobs: usize,
    stats: bool,
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
        rounds: None,
        battles: false,
        variant: "standard".to_owned(),
        jobs: std::thread::available_parallelism().map_or(1, |n| n.get()),
        stats: false,
        out: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--seeds" => options.seeds = seeds(&value()?)?,
            "--map" => options.map = value()?,
            "--size" => options.size = value()?,
            "--rounds" => options.rounds = Some(value()?.parse().map_err(|e| format!("--rounds: {e}"))?),
            "--battles" => options.battles = true,
            "--variant" => options.variant = value()?,
            "--jobs" => options.jobs = value()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            "--stats" => options.stats = true,
            "--out" => options.out = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

fn main() {
    if let Err(error) = options().and_then(|options| run(&options)) {
        eprintln!("ai-danger: {error}");
        std::process::exit(1);
    }
}

fn run(options: &Options) -> Result<(), String> {
    let out = options.out.as_ref().ok_or("--out FILE is required")?;
    let script = MapScript::from_key(&options.map).ok_or_else(|| format!("unknown map script {}", options.map))?;
    let size = size_from_name(&options.size).ok_or_else(|| format!("unknown world size {}", options.size))?;
    let variant = Variant::from_name(&options.variant)
        .ok_or_else(|| format!("unknown variant {} ({})", options.variant, ClassicBench::variants().join(", ")))?;
    let seeds: Vec<u64> = (options.seeds.0..options.seeds.1).collect();
    let started = Instant::now();
    let (labels, results): (Vec<String>, Vec<Result<Probed, String>>) = if options.battles {
        let rounds = options.rounds.unwrap_or(30);
        let specs: Vec<BattleSpec> = seeds.iter().map(|seed| BattleSpec::new(*seed, size)).collect();
        let results = play(&specs, options.jobs, |rules, spec| probe_battle(rules, spec, rounds, variant));
        (specs.iter().map(|s| s.label()).collect(), results)
    } else {
        let rounds = options.rounds.unwrap_or(200);
        let specs: Vec<GameSpec> = seeds.iter().map(|seed| GameSpec::new(script, size, *seed).max_rounds(rounds)).collect();
        let results = play(&specs, options.jobs, |rules, spec| {
            let game = new_game(rules, spec);
            let settings = AiSettings::for_rules(rules, &spec.difficulty);
            probe_game(rules, game, rounds, settings, variant, spec.seed)
        });
        (specs.iter().map(|s| s.label()).collect(), results)
    };
    let seconds = started.elapsed().as_secs_f64();
    let mut games = Vec::new();
    for (label, result) in labels.iter().zip(results) {
        match result {
            Ok(game) => {
                println!("{label}: {} rounds, {} rows, hash {:016x}", game.rounds, game.rows.len(), game.hash);
                games.push(game);
            }
            Err(error) if options.battles => println!("{label}: skipped, {error}"),
            Err(error) => return Err(format!("{label}: {error}")),
        }
    }

    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut writer = BufWriter::new(file);
    let rows: Vec<Row> = games.iter().flat_map(|g| g.rows.iter().cloned()).collect();
    write_csv(&mut writer, &rows).and_then(|_| writer.flush()).map_err(|e| format!("{}: {e}", out.display()))?;
    let cost = games.iter().fold(Cost::default(), |a, g| a.plus(g.cost));
    println!("{} rows, {seconds:.1} s, {}", rows.len(), out.display());
    println!(
        "features: {} player turns, {} units; {:.1} us a player turn, {:.2} us a unit, longest turn {:.1} ms",
        cost.turns,
        cost.units,
        cost.nanos as f64 / cost.turns.max(1) as f64 / 1e3,
        cost.nanos as f64 / cost.units.max(1) as f64 / 1e3,
        cost.max_nanos as f64 / 1e6
    );
    if options.stats {
        print_stats(&rows);
    }
    Ok(())
}

/// Probes `specs` on `jobs` threads; the results come in the specs' order.
fn play<S: Sync>(specs: &[S], jobs: usize, probe: impl Fn(&CompiledRuleset, &S) -> Result<Probed, String> + Sync) -> Vec<Result<Probed, String>> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Result<Probed, String>>>> = Mutex::new(vec![None; specs.len()]);
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, specs.len().max(1)) {
            scope.spawn(|| {
                let rules = ClassicBench::rules();
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(spec) = specs.get(at) else { break };
                    let result = probe(&rules, spec);
                    results.lock().expect("no thread panicked holding the results")[at] = Some(result);
                }
            });
        }
    });
    let results = results.into_inner().expect("no thread panicked holding the results");
    results.into_iter().map(|r| r.expect("every spec probed")).collect()
}

fn print_stats(rows: &[Row]) {
    let threatened = |r: &&Row| r.threats() > 0;
    let nations = |r: &&Row| r.side == 0;
    println!("all rows:");
    println!("{}", Quality::of(rows.iter()));
    println!("rows with at least one threat:");
    println!("{}", Quality::of(rows.iter().filter(threatened)));
    println!("rows without a threat:");
    println!("{}", Quality::of(rows.iter().filter(|r| r.threats() == 0)));
    println!("nations' rows with at least one threat:");
    println!("{}", Quality::of(rows.iter().filter(nations).filter(threatened)));
    let healed = rows.iter().filter(|r| r.healed > 0).count();
    println!("rows healed at the end of the turn: {healed}");
}
