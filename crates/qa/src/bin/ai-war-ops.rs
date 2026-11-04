//! The war operations of AI-only games and their outcomes, and the capture chance learned
//! from them (classic/docs/ai-war-fix.md "The capture chance").
//!
//! `cargo run --release -p aurelia-qa --bin ai-war-ops -- [options]`
//!
//! --variant X          every seat plays X (default `war-fix-plain`)
//! --suite NAME         the maps and the rounds of a bench suite (default standard)
//! --seeds A..B         the seeds, B excluded, each on every map of the suite (default
//!                      100001..100011: no bench offset uses them)
//! --jobs N             games in parallel (default 4)
//! --out FILE           one CSV row per operation started: where, its features
//!                      (`operations::CHANCE_FEATURES`), the chance it was admitted with and
//!                      how it ended (`war_probe::EndCause`; `Running` at the game's end)
//! --fit FILE…          fit a logistic regression of «captured» on the features of the rows
//!                      (odd seeds train, even seeds test) and print the integer constants of
//!                      `operations::CHANCE_INTERCEPT` / `CHANCE_WEIGHTS`, the test's log loss
//!                      and calibration against the constant 35 % and the siege model's chance,
//!                      and the same for the constants compiled in now
//! --use A,B,…          the features the fit may use (default all)
//! --ridge R            the ridge on the standardized weights (default 1)
//! --label war|target   predict a city of the operation's enemy taken from its start to 20
//!                      rounds after its end (default), or its own target taken

use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::play::{play_match, Pairing};
use aurelia_qa::bench::suite::{Match, Seating, Suite};
use classic_rules::ai::operations::{learned_chance, CHANCE_FEATURES};
use classic_rules::ai::war_probe;

struct Options {
    variant: String,
    suite: String,
    seeds: std::ops::Range<u64>,
    jobs: usize,
    out: Option<String>,
    fit: Vec<String>,
    /// The features the fit uses (default: all) and its ridge on the standardized weights.
    used: Option<Vec<String>>,
    ridge: f64,
    /// What the fit predicts: `war` (default), a city of the enemy taken in the operation's
    /// war; `target`, its own target taken.
    label: String,
}

fn options() -> Result<Options, String> {
    let mut o = Options { variant: "war-fix-plain".into(), suite: "standard".into(), seeds: 100_001..100_011, jobs: 4, out: None, fit: Vec::new(), used: None, ridge: 1.0, label: "war".into() };
    let mut args = std::env::args().skip(1).peekable();
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--variant" => o.variant = value()?,
            "--suite" => o.suite = value()?,
            "--seeds" => {
                let v = value()?;
                let (a, b) = v.split_once("..").ok_or("--seeds A..B")?;
                o.seeds = a.parse().map_err(|e| format!("{e}"))?..b.parse().map_err(|e| format!("{e}"))?;
            }
            "--jobs" => o.jobs = value()?.parse().map_err(|e| format!("{e}"))?,
            "--out" => o.out = Some(value()?),
            "--use" => o.used = Some(value()?.split(',').map(str::to_owned).collect()),
            "--label" => o.label = value()?,
            "--ridge" => o.ridge = value()?.parse().map_err(|e| format!("{e}"))?,
            "--fit" => {
                o.fit.push(value()?);
                while args.peek().is_some_and(|a| !a.starts_with("--")) {
                    o.fit.push(args.next().unwrap_or_default());
                }
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(o)
}

fn main() {
    let result = options().and_then(|o| if o.fit.is_empty() { play(&o) } else { fit(&o) });
    if let Err(e) = result {
        eprintln!("ai-war-ops: {e}");
        std::process::exit(1);
    }
}

/// One operation and its end.
struct Row {
    label: String,
    seed: u64,
    round: u32,
    player: u8,
    target: u32,
    chance: Option<i64>,
    cause: String,
    end_round: u32,
    /// Cities of the enemy the player took from the start until 20 rounds after the end (or its
    /// next operation): what the war the operation fought brought.
    war_captures: u32,
    features: [i64; 12],
}

fn play(o: &Options) -> Result<(), String> {
    let suite = Suite::named(&o.suite).ok_or("unknown suite")?;
    let matches: Vec<Match> = suite.maps.iter().flat_map(|map| o.seeds.clone().map(move |seed| Match { map: *map, seed, seating: Seating::All })).collect();
    let pairing = Pairing { baseline: o.variant.clone(), candidate: o.variant.clone() };
    let next = AtomicUsize::new(0);
    let rows: Mutex<Vec<(usize, Vec<Row>)>> = Mutex::new(Vec::new());
    let started = std::time::Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..o.jobs.clamp(1, matches.len().max(1)) {
            scope.spawn(|| {
                let rules = ClassicBench::rules();
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(game) = matches.get(at) else { break };
                    war_probe::enable(true);
                    let result = play_match::<ClassicBench>(&rules, &suite, game, &pairing);
                    let probe = war_probe::take();
                    war_probe::enable(false);
                    if let Err(e) = result {
                        eprintln!("{e}");
                        continue;
                    }
                    if std::env::var_os("WAR_OPS_DUMP").is_some() {
                        for s in &probe.starts {
                            eprintln!("start r{} p{} t{} v{}", s.round, s.player.0, s.target, s.value);
                        }
                        for e in &probe.ends {
                            eprintln!("end r{} p{} t{} {:?} marching {} age {}", e.round, e.player.0, e.target, e.cause, e.marching, e.age);
                        }
                    }
                    if std::env::var_os("WAR_OPS_RECALLS").is_some() {
                        for r in &probe.recalls {
                            eprintln!(
                                "recall r{} p{} marching {} age {} life {} value {} expected {} net_saved {} saved {} threat {} guard {} planned {} attacker {} members {} nearest {} reaches {} missing {}/{}",
                                r.round, r.player, r.marching, r.age, r.life, r.value, r.value_expected, r.net_saved, r.saved, r.threat, r.guard, r.planned, r.attacker, r.members, r.nearest_member, r.reaches, r.missing, r.need
                            );
                        }
                    }
                    if std::env::var_os("WAR_OPS_ESTIMATES").is_some() {
                        for e in &probe.estimates {
                            eprintln!("estimate r{} p{} pop {} value {} gain {} cost {} unrest {} worth {} missing {} need {} free {} excess {}", e.round, e.player.0, e.population, e.value, e.gain, e.cost, e.unrest, e.worth, e.missing, e.need, e.free, e.excess);
                        }
                    }
                    let mut out = Vec::new();
                    for (at, start) in probe.starts.iter().enumerate() {
                        let end = probe.ends.iter().find(|e| e.player == start.player && e.target == start.target && e.round >= start.round);
                        let next = probe.starts[at + 1..].iter().find(|s| s.player == start.player).map_or(u32::MAX, |s| s.round);
                        let until = end.map_or(suite.rounds, |e| e.round + 20).min(next);
                        let war_captures = probe.captures.iter().filter(|(round, player, _, from)| *player == start.player && *from == Some(start.enemy) && (start.round..until).contains(round)).count() as u32;
                        out.push(Row {
                            label: game.label(),
                            seed: game.seed,
                            round: start.round,
                            player: start.player.0,
                            target: start.target,
                            chance: start.chance,
                            cause: end.map_or("Running".to_owned(), |e| format!("{:?}", e.cause)),
                            end_round: end.map_or(suite.rounds, |e| e.round),
                            war_captures,
                            features: start.features,
                        });
                    }
                    eprintln!("{} {} operations, {:.0} s", game.label(), out.len(), started.elapsed().as_secs_f64());
                    rows.lock().expect("rows").push((at, out));
                }
            });
        }
    });
    let mut rows = rows.into_inner().map_err(|e| e.to_string())?;
    rows.sort_by_key(|(at, _)| *at);
    let mut text = format!("game,seed,round,player,target,chance,cause,end_round,war_captures,{}\n", CHANCE_FEATURES.join(","));
    let (mut total, mut captured, mut wars_won) = (0, 0, 0);
    for row in rows.iter().flat_map(|(_, r)| r) {
        total += 1;
        captured += usize::from(row.cause == "Captured");
        wars_won += usize::from(row.war_captures > 0);
        let features: Vec<String> = row.features.iter().map(|f| f.to_string()).collect();
        let chance = row.chance.map_or(String::new(), |c| c.to_string());
        text.push_str(&format!("{},{},{},{},{},{},{},{},{},{}\n", row.label, row.seed, row.round, row.player, row.target, chance, row.cause, row.end_round, row.war_captures, features.join(",")));
    }
    println!(
        "{} games, {total} operations, {captured} took their target ({:.1} %), {wars_won} took a city of their enemy ({:.1} %), {:.0} s",
        matches.len(),
        100.0 * captured as f64 / total.max(1) as f64,
        100.0 * wars_won as f64 / total.max(1) as f64,
        started.elapsed().as_secs_f64()
    );
    if let Some(path) = &o.out {
        std::fs::File::create(path).and_then(|mut f| f.write_all(text.as_bytes())).map_err(|e| format!("{path}: {e}"))?;
        println!("wrote {path}");
    }
    Ok(())
}

/// A row read back: its seed, the chance it was admitted with, captured, the features.
struct Sample {
    seed: u64,
    captured: bool,
    features: [f64; 12],
    raw: [i64; 12],
}

fn read(paths: &[String], war_label: bool) -> Result<Vec<Sample>, String> {
    let mut out = Vec::new();
    for path in paths {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        for line in text.lines().skip(1) {
            let cells: Vec<&str> = line.split(',').collect();
            if cells.len() < 9 + 12 {
                continue;
            }
            let mut raw = [0i64; 12];
            for (i, cell) in cells[9..21].iter().enumerate() {
                raw[i] = cell.parse().map_err(|e| format!("{path}: {e}"))?;
            }
            // An operation still running at the game's end has no outcome.
            if cells[6] == "Running" {
                continue;
            }
            let captured = if war_label { cells[8].parse::<u32>().unwrap_or(0) > 0 } else { cells[6] == "Captured" };
            out.push(Sample { seed: cells[1].parse().unwrap_or(0), captured, features: raw.map(|x| x as f64), raw });
        }
    }
    Ok(out)
}

fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}

/// Logistic regression by Newton's method on standardized features with a small ridge.
fn logistic(samples: &[&Sample], used: [bool; 12], ridge: f64) -> (f64, [f64; 12]) {
    let n = samples.len().max(1) as f64;
    let mut mean = [0.0; 12];
    let mut sd = [0.0; 12];
    for s in samples {
        for j in 0..12 {
            mean[j] += s.features[j] / n;
        }
    }
    for s in samples {
        for j in 0..12 {
            sd[j] += (s.features[j] - mean[j]).powi(2) / n;
        }
    }
    let sd = sd.map(|v| v.sqrt().max(1e-9));
    let x: Vec<[f64; 13]> = samples
        .iter()
        .map(|s| {
            let mut row = [1.0; 13];
            for j in 0..12 {
                row[j + 1] = if used[j] { (s.features[j] - mean[j]) / sd[j] } else { 0.0 };
            }
            row
        })
        .collect();
    let y: Vec<f64> = samples.iter().map(|s| f64::from(u8::from(s.captured))).collect();
    let mut beta = [0.0; 13];
    for _ in 0..50 {
        let mut grad = [0.0; 13];
        let mut hess = [[0.0; 13]; 13];
        for (row, yi) in x.iter().zip(&y) {
            let p = sigmoid(row.iter().zip(&beta).map(|(a, b)| a * b).sum());
            let w = (p * (1.0 - p)).max(1e-9);
            for a in 0..13 {
                grad[a] += (yi - p) * row[a];
                for b in 0..13 {
                    hess[a][b] += w * row[a] * row[b];
                }
            }
        }
        for a in 1..13 {
            grad[a] -= ridge * beta[a];
            hess[a][a] += ridge;
        }
        let step = solve(hess, grad);
        for a in 0..13 {
            beta[a] += step[a];
        }
        if step.iter().map(|s| s.abs()).fold(0.0, f64::max) < 1e-8 {
            break;
        }
    }
    let mut weights = [0.0; 12];
    let mut intercept = beta[0];
    for j in 0..12 {
        weights[j] = beta[j + 1] / sd[j];
        intercept -= beta[j + 1] * mean[j] / sd[j];
    }
    (intercept, weights)
}

/// Gaussian elimination with partial pivoting.
fn solve(mut a: [[f64; 13]; 13], mut b: [f64; 13]) -> [f64; 13] {
    for col in 0..13 {
        let pivot = (col..13).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs())).unwrap_or(col);
        a.swap(col, pivot);
        b.swap(col, pivot);
        let d = a[col][col];
        if d.abs() < 1e-12 {
            continue;
        }
        for row in 0..13 {
            if row != col {
                let f = a[row][col] / d;
                for k in col..13 {
                    a[row][k] -= f * a[col][k];
                }
                b[row] -= f * b[col];
            }
        }
    }
    std::array::from_fn(|i| if a[i][i].abs() < 1e-12 { 0.0 } else { b[i] / a[i][i] })
}

/// Log loss and calibration (five bins by the prediction) of percent predictions.
fn report(name: &str, samples: &[&Sample], predict: &dyn Fn(&Sample) -> f64) {
    let mut loss = 0.0;
    let mut bins = [(0usize, 0.0f64, 0usize); 5];
    for s in samples {
        let p = (predict(s) / 100.0).clamp(0.005, 0.995);
        loss -= if s.captured { p.ln() } else { (1.0 - p).ln() };
        let bin = ((p * 5.0) as usize).min(4);
        bins[bin].0 += 1;
        bins[bin].1 += p;
        bins[bin].2 += usize::from(s.captured);
    }
    let mean: f64 = samples.iter().map(|s| predict(s)).sum::<f64>() / samples.len().max(1) as f64;
    let cells: Vec<String> = bins
        .iter()
        .enumerate()
        .map(|(i, (n, p, c))| if *n == 0 { format!("{}–{} %: none", i * 20, i * 20 + 20) } else { format!("{}–{} %: {n} predicted {:.0} % taken {:.0} %", i * 20, i * 20 + 20, 100.0 * p / *n as f64, 100.0 * *c as f64 / *n as f64) })
        .collect();
    println!("| {name} | {:.4} | {mean:.1} % | {} |", loss / samples.len().max(1) as f64, cells.join("; "));
}

fn fit(o: &Options) -> Result<(), String> {
    let samples = read(&o.fit, o.label != "target")?;
    let used: [bool; 12] = std::array::from_fn(|j| o.used.as_ref().is_none_or(|u| u.iter().any(|n| n == CHANCE_FEATURES[j])));
    let train: Vec<&Sample> = samples.iter().filter(|s| s.seed % 2 == 1).collect();
    let test: Vec<&Sample> = samples.iter().filter(|s| s.seed % 2 == 0).collect();
    let rate = |set: &[&Sample]| 100.0 * set.iter().filter(|s| s.captured).count() as f64 / set.len().max(1) as f64;
    println!("{} operations with an outcome: train {} ({:.1} % captured), test {} ({:.1} % captured)\n", samples.len(), train.len(), rate(&train), test.len(), rate(&test));
    let (intercept, weights) = logistic(&train, used, o.ridge);
    let int_intercept = (intercept * 1000.0).round() as i64;
    let int_weights: Vec<i64> = weights.iter().map(|w| (w * 1e6).round() as i64).collect();
    let fitted = |s: &Sample| {
        let logit = int_intercept + s.raw.iter().zip(&int_weights).map(|(x, w)| x * w).sum::<i64>() / 1000;
        classic_rules::ai::operations::logistic_percent(logit) as f64
    };
    println!("| predictor (test) | log loss | mean prediction | calibration |\n|---|---:|---:|---|");
    let base = rate(&train);
    report("the training rate, a constant", &test, &|_| base);
    report("the constant 35 %", &test, &|_| 35.0);
    report("the siege model (`siege_chance`)", &test, &|s| s.raw[1] as f64);
    report("the compiled `learned_chance`", &test, &|s| learned_chance(&s.raw) as f64);
    report("this fit (integer)", &test, &fitted);
    println!("\nweights (per unit of the feature, logit):");
    for (name, w) in CHANCE_FEATURES.iter().zip(&weights) {
        println!("  {name:>14}: {w:+.5}");
    }
    println!("\npub const CHANCE_INTERCEPT: i64 = {int_intercept};");
    println!("pub const CHANCE_WEIGHTS: [i64; 12] = [{}];", int_weights.iter().map(|w| w.to_string()).collect::<Vec<_>>().join(", "));
    Ok(())
}
