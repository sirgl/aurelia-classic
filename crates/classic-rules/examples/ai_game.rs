//! Headless AI game on the BNW ruleset: every player is an AI, the driver
//! runs the rounds, the world is validated as it goes. Prints per player:
//! cities, population, techs, units by role, wars and peaces, and the
//! totals of the decision trace.
//!
//! `cargo run --release -p classic-rules --example ai_game -- [rounds] [seed] [players] [width] [height]`

use std::time::Instant;

use classic_content::core::core_rules;
use classic_rules::ai::planner::{role_of, AiRole};
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::ids::PlayerId;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, Command};

fn arg(index: usize, default: u64) -> u64 {
    std::env::args()
        .nth(index)
        .and_then(|a| a.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let rounds = arg(1, 300) as u32;
    let config = ScenarioConfig {
        seed: arg(2, 0x5EED_0B5E),
        players: arg(3, 6) as u8,
        width: arg(4, 64) as u32,
        height: arg(5, 48) as u32,
        cities: 0,
        units: 0,
        water_ratio: 20,
    };
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &config);
    let mut driver = AiDriver::new(
        AiSettings::for_rules(&rules, rules.default_difficulty()),
        Vec::new(),
    );
    driver.record_commands();
    driver.set_trace_capacity(200_000);
    let mut round_ms = Vec::new();
    while game.view().auth.turn().round < rounds && game.winner().is_none() {
        let started = Instant::now();
        let report = driver.run(&mut game, &rules, u32::MAX);
        round_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        if report.waiting {
            break;
        }
        if game.view().auth.turn().round % 25 == 0 {
            game.validate(&rules).expect("world must stay valid");
        }
    }
    game.validate(&rules).expect("world must stay valid");
    print_players(&game, &rules, driver.commands(), driver.states());
    let mut sorted = round_ms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
    let p95 = sorted.get(sorted.len() * 95 / 100).copied().unwrap_or(0.0);
    println!(
        "rounds={} winner={:?} round_ms median={median:.2} p95={p95:.2} max={:.2}",
        game.view().auth.turn().round,
        game.winner(),
        sorted.last().copied().unwrap_or(0.0)
    );
    println!("counters: {:?}", driver.counters());
    for (reason, count) in driver.trace().totals() {
        print!("{reason:?}={count} ");
    }
    println!();
    print_rejections(driver.trace());
    if let Ok(player) = std::env::var("AI_TRACE_PLAYER") {
        let player: u8 = player.parse().unwrap_or(0);
        for entry in driver.trace().entries().filter(|e| e.player.0 == player) {
            println!("{entry}");
        }
    }
}

/// Rejected commands grouped by reason and error.
fn print_rejections(trace: &classic_rules::ai::trace::DecisionTrace) {
    let mut groups: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for entry in trace.entries() {
        if let classic_rules::ai::trace::Outcome::Rejected(error) = &entry.outcome {
            *groups
                .entry(format!("{:?}: {error:?}", entry.reason))
                .or_default() += 1;
        }
    }
    for (group, count) in groups {
        println!("rejected {count} × {group}");
    }
}

fn print_players(
    game: &classic_rules::GameState,
    rules: &classic_rules::CompiledRuleset,
    log: &[Command],
    states: &classic_rules::ai::AiStates,
) {
    let auth = game.view().auth;
    for p in 0..auth.players().count() {
        let player = PlayerId(p as u8);
        let cities: Vec<_> = auth
            .cities()
            .iter()
            .filter(|(_, c)| c.owner == player)
            .collect();
        let population: u32 = cities.iter().map(|(_, c)| c.population).sum();
        let buildings: usize = cities.iter().map(|(_, c)| c.buildings.len()).sum();
        let mut roles = [0usize; 5];
        for (_, unit) in auth
            .units()
            .iter_handles()
            .filter(|(_, u)| u.owner == player)
        {
            let index = match role_of(rules, unit.kind) {
                AiRole::Settler => 0,
                AiRole::Worker => 1,
                AiRole::Military => 2,
                AiRole::Naval => 3,
                AiRole::Civilian => 4,
            };
            roles[index] += 1;
        }
        let wars = log
            .iter()
            .filter(|c| matches!(c, Command::DeclareWar { player: p, .. } if *p == player))
            .count();
        let peaces = log
            .iter()
            .filter(|c| matches!(c, Command::MakePeace { player: p, .. } if *p == player))
            .count();
        let improvements = (0..auth.map().tile_count() as u32)
            .map(classic_rules::ids::TileId)
            .filter(|t| {
                auth.map().owner(*t).flatten() == Some(player)
                    && auth.map().improvement(*t).flatten().is_some()
            })
            .count();
        println!(
            "P{p} alive={} cities={} peak={} pop={population} buildings={buildings} techs={} gold={} settlers={} workers={} military={} improvements={improvements} wars_declared={wars} peaces={peaces}",
            auth.players().is_alive(player),
            cities.len(),
            states.get(player).map(|s| s.strategy.peak_cities).unwrap_or(0),
            auth.players().techs_of(player).len(),
            auth.players().treasury_of(player),
            roles[0],
            roles[1],
            roles[2],
        );
    }
}
