//! The recorder: one AI-only game played by the classic AI with every
//! decision, what the AI saw and what came of each command written to a
//! JSON Lines file (`aurelia_qa::record`), a dataset for training a policy.
//!
//! `cargo run --release --offline -p aurelia-qa --bin ai-record -- --out FILE [options]`
//!
//! --out FILE           the file to write (required)
//! --seed N             the game's seed (default 1)
//! --map SCRIPT         the map script (default pangaea)
//! --size NAME          the world size (default tiny)
//! --rounds N           the round of the time victory (default 100)
//! --variant NAME       the AI variant (default standard; `ai-tournament --list`)
//! --every K            full sights on every K-th round only (default 1: all)
//!
//! The file holds no clock: the same options give the same file.

use std::path::PathBuf;
use std::time::Instant;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::record::record_game;
use aurelia_qa::spec::{size_from_name, GameSpec};
use classic_rules::ai::variant::Variant;
use classic_rules::scenario::MapScript;

struct Options {
    seed: u64,
    map: String,
    size: String,
    rounds: u32,
    variant: String,
    every: u32,
    out: Option<PathBuf>,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        seed: 1,
        map: "pangaea".to_owned(),
        size: "tiny".to_owned(),
        rounds: 100,
        variant: "standard".to_owned(),
        every: 1,
        out: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--seed" => options.seed = value()?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--map" => options.map = value()?,
            "--size" => options.size = value()?,
            "--rounds" => options.rounds = value()?.parse().map_err(|e| format!("--rounds: {e}"))?,
            "--variant" => options.variant = value()?,
            "--every" => options.every = value()?.parse().map_err(|e| format!("--every: {e}"))?,
            "--out" => options.out = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown option {other}")),
        }
    }
    if options.every == 0 {
        return Err("--every needs a positive number".to_owned());
    }
    Ok(options)
}

fn main() {
    if let Err(error) = options().and_then(|options| run(&options)) {
        eprintln!("ai-record: {error}");
        std::process::exit(1);
    }
}

fn run(options: &Options) -> Result<(), String> {
    let out = options.out.as_ref().ok_or("--out FILE is required")?;
    let script = MapScript::from_key(&options.map).ok_or_else(|| format!("unknown map script {}", options.map))?;
    let size = size_from_name(&options.size).ok_or_else(|| format!("unknown world size {}", options.size))?;
    let variant = Variant::from_name(&options.variant)
        .ok_or_else(|| format!("unknown variant {} ({})", options.variant, ClassicBench::variants().join(", ")))?;
    let spec = GameSpec::new(script, size, options.seed).max_rounds(options.rounds);
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let rules = ClassicBench::rules();
    let started = Instant::now();
    let summary = record_game(&rules, &spec, variant, options.every, Box::new(file)).map_err(|e| e.to_string())?;
    let seconds = started.elapsed().as_secs_f64();
    println!("{}: {} rounds played, hash {:016x}", spec.label(), summary.rounds, summary.hash);
    let lines: Vec<String> = summary.written.lines.iter().map(|(tag, n)| format!("{tag} {n}")).collect();
    println!("lines: {}", lines.join(", "));
    if summary.written.missed > 0 {
        println!("missed: {} unit decisions the legal list did not hold", summary.written.missed);
    }
    println!("{} bytes, {seconds:.1} s, {}", summary.written.bytes, out.display());
    Ok(())
}
