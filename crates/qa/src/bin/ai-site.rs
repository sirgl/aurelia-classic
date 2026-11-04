//! The site dataset: AI-only games played by the classic AI with every city
//! it founded written to a CSV file (`aurelia_qa::site`): the site's
//! features when it was chosen and the city's state 25, 50 and 100 rounds
//! later, a dataset for a learned site-value model; and how well the
//! settling planner's own score orders the cities, the baseline.
//!
//! `cargo run --release --offline -p aurelia-qa --bin ai-site -- --out FILE [options]`
//!
//! --out FILE           the CSV file to write (required)
//! --seeds A..B         the games' seeds, B excluded (default 1..21); A..=B includes B
//! --map SCRIPT         the map script (default pangaea)
//! --size NAME          the world size (default small)
//! --rounds N           rounds to play (the round of the time victory; default 300)
//! --variant NAME       the AI variant (default standard)
//! --jobs J             games played at once (default: the machine's threads)
//! --window W           rounds of a choice window for the baseline (default 20)
//! --stats              print the dataset's statistics and the baseline
//!
//! The file holds no clock: the same options give the same file, the games'
//! rows in seed order.

use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::site::{group_of, probe_game, write_csv, Agreement, Cost, Probed, Row, HORIZONS};
use aurelia_qa::spec::{size_from_name, GameSpec};
use classic_rules::ai::variant::Variant;
use classic_rules::scenario::MapScript;

struct Options {
    seeds: (u64, u64),
    map: String,
    size: String,
    rounds: u32,
    variant: String,
    jobs: usize,
    window: u32,
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
        rounds: 300,
        variant: "standard".to_owned(),
        jobs: std::thread::available_parallelism().map_or(1, |n| n.get()),
        window: 20,
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
            "--rounds" => options.rounds = value()?.parse().map_err(|e| format!("--rounds: {e}"))?,
            "--variant" => options.variant = value()?,
            "--jobs" => options.jobs = value()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            "--window" => options.window = value()?.parse().map_err(|e| format!("--window: {e}"))?,
            "--stats" => options.stats = true,
            "--out" => options.out = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown option {other}")),
        }
    }
    if options.window == 0 {
        return Err("--window needs a positive number".to_owned());
    }
    Ok(options)
}

fn main() {
    if let Err(error) = options().and_then(|options| run(&options)) {
        eprintln!("ai-site: {error}");
        std::process::exit(1);
    }
}

fn run(options: &Options) -> Result<(), String> {
    let out = options.out.as_ref().ok_or("--out FILE is required")?;
    let script = MapScript::from_key(&options.map).ok_or_else(|| format!("unknown map script {}", options.map))?;
    let size = size_from_name(&options.size).ok_or_else(|| format!("unknown world size {}", options.size))?;
    let variant = Variant::from_name(&options.variant)
        .ok_or_else(|| format!("unknown variant {} ({})", options.variant, ClassicBench::variants().join(", ")))?;
    let specs: Vec<GameSpec> = (options.seeds.0..options.seeds.1)
        .map(|seed| GameSpec::new(script, size, seed).max_rounds(options.rounds))
        .collect();
    let started = Instant::now();
    let results = play(&specs, options.jobs, |rules, spec| probe_game(rules, spec, variant));
    let seconds = started.elapsed().as_secs_f64();
    let mut games = Vec::new();
    for (spec, result) in specs.iter().zip(results) {
        let game = result.map_err(|error| format!("{}: {error}", spec.label()))?;
        println!("{}: {} rounds, {} rows, hash {:016x}", spec.label(), game.rounds, game.rows.len(), game.hash);
        games.push(game);
    }

    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut writer = BufWriter::new(file);
    let rows: Vec<Row> = games.iter().flat_map(|g| g.rows.iter().cloned()).collect();
    write_csv(&mut writer, &rows).and_then(|_| writer.flush()).map_err(|e| format!("{}: {e}", out.display()))?;
    let cost = games.iter().fold(Cost::default(), |a, g| a.plus(g.cost));
    let failed: u64 = games.iter().map(|g| g.failed).sum();
    println!("{} rows, {seconds:.1} s, {}", rows.len(), out.display());
    println!(
        "features: {} sites, {:.1} us a site, longest {:.1} us; {failed} founding orders without a city",
        cost.sites,
        cost.nanos as f64 / cost.sites.max(1) as f64 / 1e3,
        cost.max_nanos as f64 / 1e3
    );
    if options.stats {
        print_stats(&rows, options.window);
    }
    Ok(())
}

/// Probes `specs` on `jobs` threads; the results come in the specs' order.
fn play(specs: &[GameSpec], jobs: usize, probe: impl Fn(&classic_rules::CompiledRuleset, &GameSpec) -> Result<Probed, String> + Sync) -> Vec<Result<Probed, String>> {
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

fn print_stats(rows: &[Row], window: u32) {
    let lost = rows.iter().filter(|r| r.lost_round.is_some()).count();
    let capitals = rows.iter().filter(|r| r.feature("own_cities") == Some(0)).count();
    let illegal = rows.iter().filter(|r| r.feature("heuristic_legal") == Some(0)).count();
    println!("rows {}, capitals {capitals}, cities lost {lost}, heuristic illegal {illegal}", rows.len());
    for horizon in HORIZONS {
        let reached = rows.iter().filter(|r| r.state_at(horizon).is_some()).count();
        let lost_before = rows.iter().filter(|r| r.lost_round.is_some_and(|l| l <= r.round + horizon)).count();
        println!("+{horizon}: {reached} rows with a state, {lost_before} lost before it");
    }
    println!("baseline: the heuristic site score against the city's outcome (groups: game, player, {window}-round window)");
    for horizon in HORIZONS {
        let points = |outcome: fn(&aurelia_qa::site::CityState) -> f64| -> Vec<(u64, f64, f64)> {
            rows.iter()
                .filter_map(|r| {
                    let state = r.state_at(horizon)?;
                    let score = f64::from(r.feature("heuristic_score")?);
                    Some((group_of(r, window), score, outcome(&state)))
                })
                .collect()
        };
        let population = Agreement::of(&points(|s| f64::from(s.population)));
        let yields = Agreement::of(&points(|s| s.total_yields() as f64));
        println!("  +{horizon} population    {population}");
        println!("  +{horizon} total yields  {yields}");
        let founded = rows.iter().filter(|r| r.feature("own_cities") != Some(0));
        let points: Vec<(u64, f64, f64)> = founded
            .filter_map(|r| Some((group_of(r, window), f64::from(r.feature("heuristic_score")?), f64::from(r.state_at(horizon)?.population))))
            .collect();
        println!("  +{horizon} population, capitals left out  {}", Agreement::of(&points));
    }
}
