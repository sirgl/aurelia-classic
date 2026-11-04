//! The AI bench (docs/ai-bench.md): versions of the AI play a suite of
//! games against each other; the report says which is stronger and by how
//! much.
//!
//! `cargo run --release --offline -p aurelia-qa --bin ai-tournament -- [options]`
//!
//! --candidate X        head to head: X against the baseline (default: the
//!                      game AI, the first of --list: `standard`)
//! --baseline Y         the other side (default: the game AI; `allocator` is the classic AI
//!                      of 16 October, `legacy` the one before)
//! --solo X             every seat plays X (for --save / --compare)
//! --suite smoke|standard|duel|pace (default standard)
//! --speed NAME         the game speed: quick, standard (default), epic or
//!                      marathon; the suite's rounds and checkpoints move to
//!                      the same share of the speed's game (the report keeps it)
//! --jobs N             games in parallel (default: the machine's threads)
//! --seed-offset K      an independent suite of the same shape (seeds + 1000·K)
//! --repeat R           head to head on offsets K … K+R−1: the spread between runs
//!                      and every run's seed blocks pooled (20 blocks for a verdict)
//! --save FILE          write the report as JSON
//! --compare FILE       solo: compare with a stored JSON report
//! --matches            print every match
//! --sprt S0,S1         sequential: play seed blocks (every game of a seed)
//!                      one after another from --seed-offset on and stop as
//!                      soon as an SPRT on the points share per block
//!                      accepts H0 (share ≤ S0) or H1 (share ≥ S1); shares
//!                      as fractions (0.45,0.55)
//! --sprt-error A,B     the test's α and β (default 0.05,0.05)
//! --min-games N        sequential: no decision before N games (default 60
//!                      games and at least 4 seed blocks: 80 on standard,
//!                      60 on duel)
//! --max-games N        sequential: undecided after N games (default five
//!                      suites: 400 on standard and duel)
//! --reference FILE     sequential, repeatable: saved reports (--save) of
//!                      another candidate against the same baseline on the
//!                      same suite and speed; a block's value is then the
//!                      candidate's share minus the reference's on the same
//!                      games (S0, S1 are differences: -0.05,0.05)
//! --list               print the variants

use std::time::Instant;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::play::{tournament, Pairing};
use aurelia_qa::bench::report::{comparison_markdown, repeats_markdown, RunInfo, TournamentReport};
use aurelia_qa::bench::sequential::{play_sequential, Plan, Reference, Sprt, DEFAULT_MIN_BLOCKS, DEFAULT_MIN_GAMES};
use aurelia_qa::bench::suite::{Mode, Suite};

struct Options {
    suite: String,
    mode: Mode,
    /// The sides by variant name; None is the game AI.
    candidate: Option<String>,
    baseline: Option<String>,
    jobs: usize,
    seed_offset: u64,
    repeat: u64,
    save: Option<String>,
    compare: Option<String>,
    matches: bool,
    list: bool,
    speed: Option<String>,
    /// The sequential test's hypotheses (`--sprt`); None: a plain run.
    sprt: Option<(f64, f64)>,
    sprt_error: (f64, f64),
    min_games: Option<usize>,
    max_games: Option<usize>,
    references: Vec<String>,
}

fn pair(flag: &str, value: &str) -> Result<(f64, f64), String> {
    let parsed: Vec<f64> = value.split(',').map(str::trim).map(str::parse).collect::<Result<_, _>>().map_err(|e| format!("{flag}: {e}"))?;
    match parsed[..] {
        [a, b] => Ok((a, b)),
        _ => Err(format!("{flag} takes two numbers: A,B")),
    }
}

fn options() -> Result<Options, String> {
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut options = Options {
        suite: "standard".to_owned(),
        mode: Mode::Versus,
        candidate: None,
        baseline: None,
        jobs,
        seed_offset: 0,
        repeat: 1,
        save: None,
        compare: None,
        matches: false,
        list: false,
        speed: None,
        sprt: None,
        sprt_error: (0.05, 0.05),
        min_games: None,
        max_games: None,
        references: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--candidate" => options.candidate = Some(value()?),
            "--baseline" => options.baseline = Some(value()?),
            "--solo" => {
                let variant = value()?;
                options.mode = Mode::Solo;
                options.candidate = Some(variant.clone());
                options.baseline = Some(variant);
            }
            "--suite" => options.suite = value()?,
            "--jobs" => options.jobs = value()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            "--seed-offset" => options.seed_offset = value()?.parse().map_err(|e| format!("--seed-offset: {e}"))?,
            "--repeat" => options.repeat = value()?.parse().map_err(|e| format!("--repeat: {e}"))?,
            "--save" => options.save = Some(value()?),
            "--compare" => options.compare = Some(value()?),
            "--matches" => options.matches = true,
            "--list" => options.list = true,
            "--speed" => options.speed = Some(value()?),
            "--sprt" => options.sprt = Some(pair("--sprt", &value()?)?),
            "--sprt-error" => options.sprt_error = pair("--sprt-error", &value()?)?,
            "--min-games" => options.min_games = Some(value()?.parse().map_err(|e| format!("--min-games: {e}"))?),
            "--max-games" => options.max_games = Some(value()?.parse().map_err(|e| format!("--max-games: {e}"))?),
            "--reference" => options.references.push(value()?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    if options.repeat > 1 && (options.save.is_some() || options.mode == Mode::Solo) {
        return Err("--repeat is for head to head without --save".to_owned());
    }
    let sequential_only = options.min_games.is_some() || options.max_games.is_some() || !options.references.is_empty();
    match options.sprt {
        None if sequential_only => return Err("--min-games, --max-games and --reference are for a sequential run (--sprt)".to_owned()),
        Some(_) if options.mode == Mode::Solo || options.repeat > 1 || options.compare.is_some() => {
            return Err("--sprt is for head to head, without --repeat and --compare".to_owned())
        }
        Some((s0, s1)) if s0 >= s1 => return Err("--sprt S0,S1 needs S0 < S1".to_owned()),
        _ => {}
    }
    let (alpha, beta) = options.sprt_error;
    if !(0.0 < alpha && alpha < 0.5 && 0.0 < beta && beta < 0.5) {
        return Err("--sprt-error: α and β between 0 and 0.5".to_owned());
    }
    Ok(options)
}

fn main() {
    if let Err(error) = options().and_then(|options| run::<ClassicBench>(&options)) {
        eprintln!("ai-tournament: {error}");
        std::process::exit(1);
    }
}

fn run<R: BenchRules>(options: &Options) -> Result<(), String> {
    if options.list {
        println!("{} variants: {}", R::NAME, R::variants().join(", "));
        return Ok(());
    }
    let name = &options.suite;
    let suite = Suite::named(name).ok_or_else(|| format!("unknown suite {name} ({})", Suite::NAMES.join(", ")))?;
    let suite = match &options.speed {
        None => suite,
        Some(speed) => {
            let rules = R::rules();
            let def = rules.speed_named(speed).ok_or_else(|| format!("unknown speed {speed}"))?;
            suite.at_speed(&def.key, &def.values)
        }
    };
    let game_ai = R::variants()[0];
    let side = |name: &Option<String>| name.as_deref().unwrap_or(game_ai).to_owned();
    let pairing = Pairing { baseline: side(&options.baseline), candidate: side(&options.candidate) };
    if let Some((s0, s1)) = options.sprt {
        let sprt = Sprt { s0, s1, alpha: options.sprt_error.0, beta: options.sprt_error.1 };
        return run_sequential::<R>(options, &pairing, &suite, sprt);
    }
    let mut reports = Vec::new();
    for offset in options.seed_offset..options.seed_offset + options.repeat.max(1) {
        let report = play_suite::<R>(options, &pairing, &suite, offset)?;
        println!("{}", report.markdown(options.matches));
        reports.push(report);
    }
    if reports.len() > 1 {
        println!("{}", repeats_markdown(&reports));
    }
    let report = &reports[0];
    if let Some(path) = &options.save {
        save(report, path)?;
    }
    if let Some(path) = &options.compare {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let stored: TournamentReport = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
        println!("{}", comparison_markdown(report, &stored)?);
    }
    Ok(())
}

fn play_suite<R: BenchRules>(options: &Options, pairing: &Pairing, suite: &Suite, offset: u64) -> Result<TournamentReport, String> {
    let matches = suite.matches(options.mode, offset);
    let started = Instant::now();
    let results = tournament::<R>(suite, &matches, pairing, options.jobs).map_err(|failed| failed.join("\n"))?;
    let run = RunInfo {
        rules: R::NAME,
        suite,
        mode: options.mode,
        pairing,
        seed_offset: offset,
        jobs: options.jobs,
        wall_seconds: started.elapsed().as_secs_f64(),
    };
    Ok(TournamentReport::new(&run, &results))
}

/// A sequential run (`--sprt`): seed blocks until the test decides; the
/// report of the games played with the test's line.
fn run_sequential<R: BenchRules>(options: &Options, pairing: &Pairing, suite: &Suite, sprt: Sprt) -> Result<(), String> {
    let block_games = suite.maps.len() * 2;
    let max_blocks = options.max_games.map_or(5 * suite.seeds.len(), |g| (g / block_games).max(1));
    let min_blocks = options.min_games.map_or(DEFAULT_MIN_BLOCKS.max(DEFAULT_MIN_GAMES.div_ceil(block_games)), |g| g.div_ceil(block_games)).clamp(2, max_blocks.max(2));
    let plan = Plan { min_blocks, max_blocks };
    let reference = if options.references.is_empty() { None } else { Some(Reference::load(&options.references, suite, pairing)?) };
    let started = Instant::now();
    let run = play_sequential::<R>(suite, pairing, options.seed_offset, sprt, plan, reference.as_ref(), options.jobs).map_err(|failed| failed.join("\n"))?;
    let info = RunInfo {
        rules: R::NAME,
        suite,
        mode: options.mode,
        pairing,
        seed_offset: options.seed_offset,
        jobs: options.jobs,
        wall_seconds: started.elapsed().as_secs_f64(),
    };
    let mut report = TournamentReport::new(&info, &run.results);
    report.sequential = Some(run.outcome);
    println!("{}", report.markdown(options.matches));
    if let Some(path) = &options.save {
        save(&report, path)?;
    }
    Ok(())
}

fn save(report: &TournamentReport, path: &str) -> Result<(), String> {
    let json = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    std::fs::write(path, json + "\n").map_err(|e| format!("{path}: {e}"))?;
    println!("saved {path}");
    Ok(())
}
