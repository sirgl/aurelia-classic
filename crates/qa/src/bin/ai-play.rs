//! The dataset of the end-to-end net: AI-only games of the scripted mind
//! kept as replay logs (`aurelia_qa::export`, classic/docs/ai-net-data.md).
//!
//! `cargo run --release -p aurelia-qa --bin ai-play -- --out DIR [options]`
//!
//! --out DIR            where the logs go (required): `<name>.aurlog` (the
//!                      kernel's command journal) and `<name>.json` (the spec)
//! --suite NAME         the bench suite whose games are played (default smoke)
//! --games N            how many games (default 1): the suite's games from
//!                      the seed offset on, offset after offset
//! --seed-offset K      the first seed offset (default 0; seeds move by
//!                      `SEED_STRIDE` per offset)
//! --rounds R           the last round (default: the suite's, scaled to the
//!                      speed: 330 for a suite of 500 at Quick)
//! --speed NAME         the game speed: quick, standard (default), epic or
//!                      marathon (kept in each log's `.json`)
//! --jobs J             games played at once (default 1)
//! --variant NAME       the expert on every seat (default the net's base,
//!                      `NET_BASE`: `pre-puppet`, the standard before the
//!                      eighth promotion; kept in each log's `.json`).
//!                      The exporter replays a log by its commands and reads
//!                      the rows with `NET_BASE` whoever played: the rows are
//!                      those the net reads at play, the choices this
//!                      expert's (classic/docs/ai-net-data.md "The expert")

use std::path::PathBuf;
use std::time::Instant;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::export::{parallel, play_log, suite_specs_with, write_log};
use classic_rules::ai::variant::{Variant, NET_BASE};

struct Options {
    out: Option<PathBuf>,
    suite: String,
    games: usize,
    offset: u64,
    rounds: Option<u32>,
    jobs: usize,
    speed: Option<String>,
    variant: Variant,
}

fn options() -> Result<Options, String> {
    let mut o = Options { out: None, suite: "smoke".to_owned(), games: 1, offset: 0, rounds: None, jobs: 1, speed: None, variant: NET_BASE };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--out" => o.out = Some(PathBuf::from(value()?)),
            "--suite" => o.suite = value()?,
            "--games" => o.games = value()?.parse().map_err(|e| format!("--games: {e}"))?,
            "--seed-offset" => o.offset = value()?.parse().map_err(|e| format!("--seed-offset: {e}"))?,
            "--rounds" => o.rounds = Some(value()?.parse().map_err(|e| format!("--rounds: {e}"))?),
            "--jobs" => o.jobs = value()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            "--speed" => o.speed = Some(value()?),
            "--variant" => {
                let name = value()?;
                o.variant = Variant::from_name(&name).ok_or_else(|| format!("--variant: no AI variant {name}"))?;
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(o)
}

fn main() {
    if let Err(error) = options().and_then(|o| run(&o)) {
        eprintln!("ai-play: {error}");
        std::process::exit(1);
    }
}

fn run(o: &Options) -> Result<(), String> {
    let out = o.out.as_ref().ok_or("--out DIR is required")?;
    let suite = Suite::named(&o.suite).ok_or_else(|| format!("unknown suite {} ({})", o.suite, Suite::NAMES.join(", ")))?;
    let rules = ClassicBench::rules();
    let mut specs = suite_specs_with(&suite, o.games, o.offset, o.rounds, o.variant);
    println!("expert {} (the net's base: {})", o.variant.name(), NET_BASE.name());
    if let Some(name) = &o.speed {
        let speed = rules.speed_named(name).ok_or_else(|| format!("unknown speed {name}"))?;
        for spec in &mut specs {
            if !speed.values.is_standard() {
                spec.speed = speed.key.clone();
            }
            if o.rounds.is_none() {
                spec.rounds = speed.values.game_round(suite.rounds);
            }
        }
        println!("speed {}: the last round {}", speed.name, o.rounds.unwrap_or(speed.values.game_round(suite.rounds)));
    }
    let started = Instant::now();
    let results = parallel(&specs, o.jobs, |spec| {
        let clock = Instant::now();
        let log = play_log(&rules, spec)?;
        let path = write_log(out, spec, &log).map_err(|e| format!("{}: {e}", spec.name()))?;
        Ok::<_, String>((path, log.journal.len(), log.rounds, clock.elapsed().as_secs_f64()))
    });
    let mut failed = 0;
    let mut bytes = 0;
    for (spec, result) in specs.iter().zip(results) {
        match result {
            Ok((path, size, rounds, secs)) => {
                bytes += size;
                println!("{}: {rounds} rounds, {:.1} KB, {secs:.1} s", path.display(), size as f64 / 1024.0);
            }
            Err(error) => {
                failed += 1;
                eprintln!("{}: {error}", spec.name());
            }
        }
    }
    let played = specs.len() - failed;
    println!("{played} games, {:.1} KB per log, {:.1} s in all", bytes as f64 / 1024.0 / played.max(1) as f64, started.elapsed().as_secs_f64());
    if failed > 0 {
        return Err(format!("{failed} games failed"));
    }
    Ok(())
}
