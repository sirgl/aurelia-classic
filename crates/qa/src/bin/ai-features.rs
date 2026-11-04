//! The shards of the end-to-end net made from replay logs
//! (`aurelia_qa::export`, classic/docs/ai-net-data.md): each log is replayed
//! command by command with its round hashes checked, and the nations'
//! decisions are written as `<name>.shard/`.
//!
//! `cargo run --release -p aurelia-qa --bin ai-features -- LOG_OR_DIR... --out DIR [options]`
//!
//! LOG_OR_DIR           `.aurlog` files, or directories of them (each with
//!                      its `.json` spec beside it)
//! --out DIR            where the shards go (required)
//! --window K           keep windows of K consecutive player-turns of each
//!                      nation (with --windows-per-player; default: all turns)
//! --windows-per-player M   how many windows a nation keeps (default 4)
//! --turn-sample N      without --window: a deterministic 1-in-N of the
//!                      player-turns (default 1: all)
//! --jobs J             logs replayed at once (default 1)
//! --hindsight-marches  label a unit's moves as the march to where it stops
//!                      moving (`export::features_with`; default off)
//! --e2                 the logs are E2 self-play games (`ai-selfplay`): each
//!                      log's sidecar (`<stem>.e2.bin`, `<stem>.e2.json`) is
//!                      read beside it and the shard holds the learner's own
//!                      decisions with the E2 files (`aurelia_qa::e2`)
//! --minds LIST         keep only the decisions of the seats these bench
//!                      minds played (e.g. `standard`), read from each log's
//!                      `<stem>.e2.json` (`ai-selfplay --learner-mind` against
//!                      other minds): the expert's choices, not its rivals';
//!                      a log without a sidecar (`ai-play`) keeps every seat.
//!                      A learner's seat is kept from the round the learner
//!                      took it (a training ground's war round, a mid-game
//!                      start: `export::scope_of_game`)
//!
//! A log with a sidecar is replayed with its changes outside the commands
//! (a training ground's scene, a mid-game gift), with or without --minds.

use std::path::PathBuf;
use std::time::Instant;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::export::{features_of, parallel, read_log, scope_of_game, write_shard, Scope, Selection};
use aurelia_qa::e2;

struct Options {
    logs: Vec<PathBuf>,
    out: Option<PathBuf>,
    sample: u32,
    window: Option<u32>,
    per_player: u32,
    jobs: usize,
    hindsight_marches: bool,
    e2: bool,
    minds: Option<Vec<String>>,
}

impl Options {
    fn selection(&self) -> Selection {
        match self.window {
            Some(length) => Selection::Windows { length, per_player: self.per_player },
            None => Selection::Sample(self.sample),
        }
    }
}

fn options() -> Result<Options, String> {
    let mut o = Options { logs: Vec::new(), out: None, sample: 1, window: None, per_player: 4, jobs: 1, hindsight_marches: false, e2: false, minds: None };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--out" => o.out = Some(PathBuf::from(value()?)),
            "--turn-sample" => o.sample = value()?.parse().map_err(|e| format!("--turn-sample: {e}"))?,
            "--window" => o.window = Some(value()?.parse().map_err(|e| format!("--window: {e}"))?),
            "--windows-per-player" => o.per_player = value()?.parse().map_err(|e| format!("--windows-per-player: {e}"))?,
            "--hindsight-marches" => o.hindsight_marches = true,
            "--e2" => o.e2 = true,
            "--minds" => o.minds = Some(value()?.split(',').filter(|m| !m.is_empty()).map(str::to_owned).collect()),
            "--jobs" => o.jobs = value()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            path => {
                let path = PathBuf::from(path);
                if path.is_dir() {
                    let mut found: Vec<PathBuf> = std::fs::read_dir(&path)
                        .map_err(|e| format!("{}: {e}", path.display()))?
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| p.extension().is_some_and(|x| x == "aurlog"))
                        .collect();
                    found.sort();
                    o.logs.extend(found);
                } else {
                    o.logs.push(path);
                }
            }
        }
    }
    if o.window == Some(0) || o.per_player == 0 {
        return Err("--window and --windows-per-player need positive numbers".to_owned());
    }
    if o.sample == 0 {
        return Err("--turn-sample needs a positive number".to_owned());
    }
    Ok(o)
}

/// The scope of a self-play log from its `<stem>.e2.json`
/// (`export::scope_of_game`): with `minds` the seats one of them played;
/// every player and no change outside the commands for a log without one,
/// an `ai-play` game of one expert on every seat.
fn scope_of(log: &std::path::Path, minds: Option<&[String]>) -> Result<Scope, String> {
    let meta = log.with_extension("e2.json");
    if !meta.exists() {
        return Ok(Scope::default());
    }
    let text = std::fs::read_to_string(&meta).map_err(|e| format!("{}: {e} (the self-play sidecar)", meta.display()))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", meta.display()))?;
    scope_of_game(&json, minds).map_err(|e| format!("{}: {e}", meta.display()))
}

fn main() {
    if let Err(error) = options().and_then(|o| run(&o)) {
        eprintln!("ai-features: {error}");
        std::process::exit(1);
    }
}

fn run(o: &Options) -> Result<(), String> {
    let out = o.out.as_ref().ok_or("--out DIR is required")?;
    if o.logs.is_empty() {
        return Err("no logs given".to_owned());
    }
    let rules = ClassicBench::rules();
    if o.e2 {
        return run_e2(o, out, &rules);
    }
    let started = Instant::now();
    let results = parallel(&o.logs, o.jobs, |path| {
        let clock = Instant::now();
        let (spec, journal) = read_log(path)?;
        let scope = scope_of(path, o.minds.as_deref())?;
        let mut shard = features_of(&rules, &spec, &journal, o.selection(), o.hindsight_marches, &scope)?;
        // A self-play log is named by its stem, as `--e2` names it: the games of one
        // directory share world specs (a ladder's varied scenes redraw the same worlds),
        // and the spec's name would write their shards over each other.
        if path.with_extension("e2.json").exists() {
            shard.name = path.file_stem().and_then(|s| s.to_str()).unwrap_or(&shard.name).to_owned();
        }
        let secs = clock.elapsed().as_secs_f64();
        let (dir, bytes) = write_shard(out, &shard, &rules).map_err(|e| format!("{}: {e}", spec.name()))?;
        let windows = shard.windows.len();
        Ok::<_, String>((dir, bytes, secs, shard.counts, windows))
    });
    let mut failed = 0;
    for (path, result) in o.logs.iter().zip(results) {
        match result {
            Ok((dir, bytes, secs, counts, windows)) => {
                let kept: u64 = counts.decisions.values().sum();
                let dropped: u64 = counts.dropped.values().sum();
                println!(
                    "{}: {:.1} MB, {secs:.1} s ({:.1} ms a player-turn), {} turns in {windows} windows, {kept} decisions, {dropped} dropped, {} round hashes checked",
                    dir.display(),
                    bytes as f64 / 1e6,
                    secs * 1000.0 / counts.exported_turns.max(1) as f64,
                    counts.exported_turns,
                    counts.checkpoints
                );
            }
            Err(error) => {
                failed += 1;
                eprintln!("{}: {error}", path.display());
            }
        }
    }
    println!("{} shards in {:.1} s", o.logs.len() - failed, started.elapsed().as_secs_f64());
    if failed > 0 {
        return Err(format!("{failed} logs failed"));
    }
    Ok(())
}

/// `--e2`: the shard of each self-play log's learner decisions.
fn run_e2(o: &Options, out: &std::path::Path, rules: &classic_rules::CompiledRuleset) -> Result<(), String> {
    let started = Instant::now();
    let results = parallel(&o.logs, o.jobs, |path| {
        let clock = Instant::now();
        let (spec, journal) = read_log(path)?;
        let (trace, game) = e2::read_sidecar(path)?;
        let name = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| format!("{}: no file name", path.display()))?.to_owned();
        let (shard, arrays, counts) = e2::shard_of_game(rules, &spec, &name, &journal, &trace, o.selection(), &game).map_err(|e| format!("{name}: {e}"))?;
        let secs = clock.elapsed().as_secs_f64();
        let (dir, bytes) = e2::write(out, &shard, &arrays, &counts, &game, rules).map_err(|e| format!("{name}: {e}"))?;
        Ok::<_, String>((dir, bytes, secs, counts))
    });
    let mut failed = 0;
    let mut dropped_all = 0;
    for (path, result) in o.logs.iter().zip(results) {
        match result {
            Ok((dir, bytes, secs, counts)) => {
                let dropped: u64 = counts.dropped.values().sum();
                dropped_all += dropped;
                println!(
                    "{}: {:.1} MB, {secs:.1} s, {} of {} turns, {} of {} decisions aligned, {dropped} dropped {:?}, {} empty, unit commands sent {} / not sent {}",
                    dir.display(),
                    bytes as f64 / 1e6,
                    counts.exported_turns,
                    counts.turns,
                    counts.aligned,
                    counts.decisions,
                    counts.dropped,
                    counts.empty,
                    counts.unit_commands_sent,
                    counts.unit_commands_not_sent
                );
            }
            Err(error) => {
                failed += 1;
                eprintln!("{}: {error}", path.display());
            }
        }
    }
    println!("{} E2 shards in {:.1} s, {dropped_all} decisions dropped", o.logs.len() - failed, started.elapsed().as_secs_f64());
    if failed > 0 {
        return Err(format!("{failed} logs failed"));
    }
    Ok(())
}
