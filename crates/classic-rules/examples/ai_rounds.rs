//! AI round time on the perf scenarios (PLAN M0/M6 contract: an AI round
//! ≤ 250 ms on `representative`, ≤ 1 s on `stress`).
//!
//! Method: release build; each run generates the scenario on the BNW
//! ruleset (all players AI), plays `WARMUP` rounds, then times `ROUNDS`
//! rounds (a round = every player's AI turn, EndTurn included, measured
//! around `AiDriver::run`). `RUNS` runs per scenario; the report gives the
//! median and p95 over all timed rounds, the median of the run medians,
//! the longest single driver step (the granularity of frame slicing) and
//! the deterministic counters of the last run.
//!
//! On a shared machine the wall time of a round is inflated by whatever else
//! runs; the report therefore also gives the CPU time of the thread
//! (Linux: `/proc/thread-self/schedstat`, 0 elsewhere), which does not
//! include the time spent waiting for a core. Compare variants by paired
//! runs (alternate them), and quote the load average with the wall numbers.
//!
//! `cargo run --release -p classic-rules --example ai_rounds -- [runs] [rounds] [warmup] [scenario...]`
//!
//! `AI_VARIANT=<name>` plays every player with that AI variant
//! (`ai::variant`, platform/docs/ai-bench.md) instead of the game's AI (`standard`).

use std::time::Instant;

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::{AiSettings, StepOutcome};
use kernel::ai::driver::AiDriver;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset};

struct Sample {
    rounds_ms: Vec<f64>,
    /// CPU time of the thread per timed round (0 when unavailable).
    cpu_ms: Vec<f64>,
    longest_step_ms: f64,
    counters: classic_rules::ai::trace::AiCounters,
    units: usize,
    cities: usize,
}

/// CPU time this thread has used, in ms (Linux: run time of the first field
/// of `/proc/thread-self/schedstat`).
fn thread_cpu_ms() -> f64 {
    std::fs::read_to_string("/proc/thread-self/schedstat")
        .ok()
        .and_then(|text| text.split_whitespace().next().and_then(|ns| ns.parse::<f64>().ok()))
        .map_or(0.0, |ns| ns / 1e6)
}

fn arg(index: usize, default: usize) -> usize {
    std::env::args()
        .nth(index)
        .and_then(|a| a.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let runs = arg(1, 9);
    let rounds = arg(2, 5);
    let warmup = arg(3, 2);
    let names: Vec<String> = std::env::args().skip(4).collect();
    let names = if names.is_empty() {
        vec!["representative".to_string(), "stress".to_string()]
    } else {
        names
    };
    let rules = compile_rules(core_rules()).unwrap();
    for name in names {
        let config = match name.as_str() {
            "representative" => ScenarioConfig::representative_late_game(),
            "stress" => ScenarioConfig::stress(),
            other => panic!("unknown scenario {other}"),
        };
        let variant = std::env::var("AI_VARIANT").ok().map_or(Variant::default(), |name| {
            Variant::from_name(&name).unwrap_or_else(|| panic!("unknown AI variant {name}"))
        });
        let samples: Vec<Sample> = (0..runs).map(|_| run(&rules, &config, warmup, rounds, ClassicMind::new(variant))).collect();
        report(&name, &config, &samples);
    }
}

fn run(rules: &CompiledRuleset, config: &ScenarioConfig, warmup: usize, rounds: usize, mind: ClassicMind) -> Sample {
    let mut game = generate(config, rules);
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), mind);
    driver.set_trace_capacity(0);
    let mut longest_step_ms: f64 = 0.0;
    let mut rounds_ms = Vec::new();
    let mut cpu_ms = Vec::new();
    for round in 0..warmup + rounds {
        let started = Instant::now();
        let cpu_started = thread_cpu_ms();
        loop {
            let step = Instant::now();
            let outcome = driver.step(&mut game, rules);
            longest_step_ms = longest_step_ms.max(step.elapsed().as_secs_f64() * 1000.0);
            if matches!(outcome, StepOutcome::RoundDone | StepOutcome::Waiting) {
                break;
            }
        }
        if round >= warmup {
            rounds_ms.push(started.elapsed().as_secs_f64() * 1000.0);
            cpu_ms.push(thread_cpu_ms() - cpu_started);
        }
    }
    let auth = game.view().auth;
    Sample {
        rounds_ms,
        cpu_ms,
        longest_step_ms,
        counters: *driver.counters(),
        units: auth.units().iter_handles().count(),
        cities: auth.cities().len(),
    }
}

fn percentile(sorted: &[f64], p: usize) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = (sorted.len() * p / 100).min(sorted.len() - 1);
    sorted[index]
}

fn report(name: &str, config: &ScenarioConfig, samples: &[Sample]) {
    let mut all: Vec<f64> = samples
        .iter()
        .flat_map(|s| s.rounds_ms.iter().copied())
        .collect();
    all.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut medians: Vec<f64> = samples
        .iter()
        .map(|s| {
            let mut r = s.rounds_ms.clone();
            r.sort_by(|a, b| a.partial_cmp(b).unwrap());
            percentile(&r, 50)
        })
        .collect();
    medians.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let longest = samples
        .iter()
        .map(|s| s.longest_step_ms)
        .fold(0.0, f64::max);
    let last = samples.last().unwrap();
    println!(
        "{name}: {}x{} players {} | runs {} rounds/run {} | round ms median {:.1} p95 {:.1} max {:.1} | median of run medians {:.1} (min {:.1} max {:.1}) | longest step {:.2} ms | end units {} cities {}",
        config.width,
        config.height,
        config.players,
        samples.len(),
        last.rounds_ms.len(),
        percentile(&all, 50),
        percentile(&all, 95),
        all.last().copied().unwrap_or(0.0),
        percentile(&medians, 50),
        medians.first().copied().unwrap_or(0.0),
        medians.last().copied().unwrap_or(0.0),
        longest,
        last.units,
        last.cities,
    );
    let mut cpu: Vec<f64> = samples.iter().flat_map(|s| s.cpu_ms.iter().copied()).collect();
    cpu.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "{name} cpu ms (thread time, no waiting for a core): median {:.1} p95 {:.1} max {:.1}",
        percentile(&cpu, 50),
        percentile(&cpu, 95),
        cpu.last().copied().unwrap_or(0.0)
    );
    println!("{name} counters (last run): {:?}", last.counters);
}
