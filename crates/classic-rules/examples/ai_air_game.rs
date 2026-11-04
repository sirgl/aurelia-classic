//! Headless AI games in the air era: every player is an AI of one variant,
//! all players are taught the technologies up to the first aircraft at a
//! chosen round, and the report counts what the aircraft did.
//!
//! `cargo run --release -p classic-rules --example ai_air_game -- <variant> <seed> [rounds] [teach_round] [players] [width] [height]`
//!
//! Prints one line per game: aircraft built and lost, strikes, rebases,
//! sweeps, interception postures, rejected commands, exhausted turn budgets,
//! cities and the winner. The world is read (not the AI) only to count.

use std::collections::{BTreeMap, BTreeSet};

use classic_content::core::core_rules;
use classic_rules::ai::air::wing;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{PlayerId, TechId};
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, ClassicGame, Command};
use kernel::ai::driver::AiDriver;
use kernel::ai::trace::{Actor, Outcome, Reason};

fn arg(index: usize, default: u64) -> u64 {
    std::env::args()
        .nth(index)
        .and_then(|a| a.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "standard".into());
    let variant = Variant::from_name(&name).unwrap_or_else(|| panic!("unknown variant {name}"));
    let seed = arg(2, 1);
    let rounds = arg(3, 400) as u32;
    let teach = arg(4, 150) as u32;
    let config = ScenarioConfig {
        seed: 0xA1A1_0000 + seed,
        players: arg(5, 4) as u8,
        width: arg(6, 56) as u32,
        height: arg(7, 40) as u32,
        cities: 0,
        units: 0,
        water_ratio: 20,
    };
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &config);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(variant));
    driver.set_trace_capacity(4_000_000);
    // The technologies the first aircraft need: the closure of the
    // prerequisites of the techs that unlock an air unit.
    let mut want: BTreeSet<u16> = BTreeSet::new();
    // Only the cheapest aircraft: the unlockers no deeper than the shallowest
    // plus `spread` (teaching the whole tail would also teach a victory).
    let unlockers: Vec<(usize, u8)> = rules
        .technologies
        .iter()
        .enumerate()
        .filter(|(_, t)| t.unlocks_units.iter().any(|k| wing(&rules, *k).is_some()))
        .map(|(i, t)| (i, t.era))
        .collect();
    let shallowest = unlockers.iter().map(|(_, e)| *e).min().unwrap_or(0);
    let spread = arg(8, 1) as u8;
    let mut stack: Vec<TechId> = unlockers
        .iter()
        .filter(|(_, e)| *e <= shallowest + spread)
        .map(|(i, _)| TechId(*i as u16))
        .collect();
    if std::env::var("AIR_LIST").is_ok() {
        for (i, e) in &unlockers {
            eprintln!("unlocker {} era {e}", rules.technologies[*i].key);
        }
    }
    while let Some(t) = stack.pop() {
        if want.insert(t.0) {
            stack.extend(rules.technologies[t.0 as usize].requires.iter().copied());
        }
    }
    let mut taught = false;
    let mut op_rounds = 0u32;
    let mut ops_seen = 0usize;
    let mut seen: BTreeMap<u64, (u8, u16)> = BTreeMap::new();
    let mut alive: BTreeSet<u64> = BTreeSet::new();
    let (mut built, mut lost) = (0u32, 0u32);
    let mut air_ids: BTreeSet<u64> = BTreeSet::new();
    while game.view().auth.turn().round < rounds && game.winner().is_none() {
        if !taught && game.view().auth.turn().round >= teach {
            taught = true;
            let players = game.view().auth.players().count();
            // The strategic resource the aircraft need (the harness gives
            // every nation one source, the same for every variant).
            let oil = classic_rules::ids::ResourceId(
                rules
                    .resources
                    .iter()
                    .position(|r| r.key == "core:resource/oil")
                    .unwrap() as u16,
            );
            let mut spots = Vec::new();
            for p in 0..players as u8 {
                let auth = game.view().auth;
                let mut own: Vec<_> = auth
                    .indexes()
                    .cities_of(PlayerId(p))
                    .iter()
                    .filter_map(|c| auth.cities().get(*c))
                    .map(|c| (c.id, c.tile))
                    .collect();
                own.sort();
                if let Some((_, at)) = own.first() {
                    spots.push((*at, oil));
                }
            }
            game.debug_set_resource_batch(&rules, &spots);
            // Wars the AIs would not start by themselves in the time: the
            // same two pairs for every variant (0-1 and 2-3).
            if arg(9, 1) == 1 {
                for (a, b) in [(0u8, 1u8), (2, 3)] {
                    if (b as usize) < players {
                        game.debug_meet(PlayerId(a), PlayerId(b));
                        game.debug_force_war(&rules, PlayerId(a), PlayerId(b));
                    }
                }
            }
            for p in 0..players as u8 {
                for t in &want {
                    game.debug_learn_tech(&rules, PlayerId(p), TechId(*t));
                }
            }
        }
        driver.run(&mut game, &rules, u32::MAX);
        let ops: usize = driver
            .states()
            .states()
            .iter()
            .map(|s| s.operations().len())
            .sum();
        if ops > 0 {
            op_rounds += 1;
        }
        ops_seen = ops_seen.max(ops);
        let auth = game.view().auth;
        let mut now = BTreeSet::new();
        for (_, core) in auth.units().iter_handles() {
            if wing(&rules, core.kind).is_some() {
                let id = core.id.0 as u64;
                now.insert(id);
                seen.entry(id).or_insert((core.owner.0, core.kind.0));
                air_ids.insert(id);
            }
        }
        built += now.difference(&alive).count() as u32;
        lost += alive.difference(&now).count() as u32;
        alive = now;
    }
    let (mut strikes, mut rebases, mut sweeps, mut posts, mut rejected, mut ordered, mut wars) =
        (0, 0, 0, 0, 0, 0, 0);
    let mut by_reason = BTreeMap::new();
    for e in driver.trace().entries() {
        let air_actor = matches!(e.actor, Actor::Unit(id) if air_ids.contains(&(id.0 as u64)));
        match (&e.command, e.outcome == Outcome::Applied) {
            (Some(Command::RangedAttack { unit, .. }), true) => {
                if let Some(core) = game.view().auth.units().get(*unit) {
                    if wing(&rules, core.core.kind).is_some() {
                        strikes += 1;
                    }
                } else if air_actor {
                    strikes += 1;
                }
            }
            (
                Some(Command::EnqueueProduction {
                    item: classic_rules::ids::ProductionItem::Unit(k),
                    ..
                }),
                true,
            ) if wing(&rules, *k).is_some() => ordered += 1,
            (Some(Command::DeclareWar { .. }), true) => wars += 1,
            (Some(Command::Rebase { .. }), true) => rebases += 1,
            (Some(Command::AirSweep { .. }), true) => sweeps += 1,
            (Some(Command::Intercept { .. }), true) => posts += 1,
            (Some(_), false) if air_actor => rejected += 1,
            _ => {}
        }
        if e.command.is_none() {
            *by_reason.entry(format!("{:?}", e.reason)).or_insert(0u32) += 1;
        }
    }
    let exhausted = driver
        .trace()
        .entries()
        .filter(|e| e.reason == Reason::TurnBudgetExhausted)
        .count();
    let auth = game.view().auth;
    let cities: Vec<usize> = (0..auth.players().count() as u8)
        .map(|p| auth.indexes().cities_of(PlayerId(p)).len())
        .collect();
    println!(
        "{name} seed={seed} rounds={} wars={wars} op_rounds={op_rounds} max_ops={ops_seen} ordered={ordered} built={built} lost={lost} strikes={strikes} rebases={rebases} sweeps={sweeps} postures={posts} air_rejected={rejected} budget_exhausted={exhausted} cities={cities:?} winner={:?} commands_rejected={}",
        auth.turn().round,
        game.winner(),
        driver.counters().commands_rejected
    );
}
