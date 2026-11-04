//! A war in the air era, played by the AI against a passive rival, with and
//! without the air arm (classic/docs/ai-air.md «Measured»).
//!
//! The AI (player 0) has three cities, the technologies of the first
//! aircraft, oil under its capital, a modern army and a war chest; the rival
//! (player 1, passive: it trains defenders and keeps its fighter on
//! interception duty) has three cities garrisoned by musketmen, an
//! anti-aircraft gun and a fighter. The AI declares the war and fights it by
//! its operations.
//!
//! `cargo run --release -p classic-rules --example ai_air_war -- <variant> <seed> [rounds]`
//!
//! One line: the round of the end, the cities of both, the aircraft built,
//! lost, their strikes, rebases, sweeps, postures, the aircraft orders the
//! rules refused, exhausted turn budgets.

use std::collections::BTreeSet;

use classic_content::core::core_rules;
use classic_rules::ai::air::wing;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{
    PlayerId, ProductionItem, ResourceId, TechId, TerrainId, TileId, UnitKindId,
};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::trace::{Actor, Outcome, Reason};

const AI: PlayerId = PlayerId(0);
const RIVAL: PlayerId = PlayerId(1);

fn arg(index: usize, default: u64) -> u64 {
    std::env::args()
        .nth(index)
        .and_then(|a| a.parse().ok())
        .unwrap_or(default)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(
        rules
            .units
            .iter()
            .position(|u| u.key == key)
            .unwrap_or_else(|| panic!("{key}")) as u16,
    )
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(
        rules
            .technologies
            .iter()
            .position(|t| t.key == key)
            .unwrap_or_else(|| panic!("{key}")) as u16,
    )
}

fn scene(rules: &CompiledRuleset, seed: u64) -> GameState {
    let mut game = GameState::new(
        rules,
        GameConfig {
            players: 2,
            width: 40,
            height: 24,
            rng_seed: 100 + seed,
        },
    );
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(1))).collect();
    game.debug_set_terrain_batch(rules, &land);
    for (q, r) in [(6, 10), (6, 16), (10, 13)] {
        game.debug_found_city(rules, AI, tile(&game, q, r));
    }
    for (q, r) in [(16, 10), (16, 16), (20, 13)] {
        let at = tile(&game, q, r);
        game.debug_found_city(rules, RIVAL, at);
        let guards = 3 + (seed % 3) as usize;
        for key in [
            "core:unit/musketman",
            "core:unit/musketman",
            "core:unit/musketman",
            "core:unit/musketman",
            "core:unit/rifleman",
        ]
        .iter()
        .take(guards)
        {
            game.debug_spawn_unit(rules, RIVAL, at, kind(rules, key));
        }
    }
    let capital = tile(&game, 20, 13);
    // The air defence of the rival's capital: on odd seeds a gun and a
    // fighter, on even seeds a gun only.
    game.debug_spawn_unit(
        rules,
        RIVAL,
        capital,
        kind(rules, "core:unit/anti-aircraft-gun"),
    );
    if seed % 2 == 1 {
        game.debug_spawn_unit(rules, RIVAL, capital, kind(rules, "core:unit/fighter"));
    }
    // The AI's technologies: the first aircraft and the soldiers of the age.
    let mut stack: Vec<TechId> = [
        "core:tech/radar",
        "core:tech/rifling",
        "core:tech/metallurgy",
    ]
    .iter()
    .map(|k| tech(rules, k))
    .collect();
    let mut known = BTreeSet::new();
    while let Some(t) = stack.pop() {
        if known.insert(t.0) {
            stack.extend(rules.technologies[t.0 as usize].requires.iter().copied());
        }
    }
    for t in &known {
        game.debug_learn_tech(rules, AI, TechId(*t));
    }
    let oil = ResourceId(
        rules
            .resources
            .iter()
            .position(|r| r.key == "core:resource/oil")
            .unwrap() as u16,
    );
    game.debug_set_deposit(rules, tile(&game, 6, 10), oil, 6);
    for (q, r) in [(7, 10), (7, 11), (5, 10), (9, 13)] {
        game.debug_spawn_unit(
            rules,
            AI,
            tile(&game, q, r),
            kind(rules, "core:unit/rifleman"),
        );
    }

    let home = tile(&game, 6, 10);
    // Starting aircraft (argument 5, default 1): none makes the AI build its own.
    if arg(5, 1) == 1 {
        for key in ["core:unit/bomber", "core:unit/bomber", "core:unit/fighter"] {
            game.debug_spawn_unit(rules, AI, home, kind(rules, key));
        }
    }
    // A field army of the rival in sight of the AI's second city.
    if arg(4, 1) == 1 {
        let shift = (seed % 4) as i32;
        for (q, r) in [
            (14, 12 + shift),
            (14, 13 + shift),
            (15, 13 + shift),
            (15, 12 + shift),
            (14, 14 + shift),
            (15, 14 + shift),
        ] {
            game.debug_spawn_unit(
                rules,
                RIVAL,
                tile(&game, q, r),
                kind(rules, "core:unit/musketman"),
            );
        }
    }
    game.debug_meet(AI, RIVAL);
    game.execute(
        rules,
        Command::DeclareWar {
            player: AI,
            enemy: RIVAL,
        },
    )
    .unwrap();
    game.debug_earn(AI, 4_000);
    // Only a conquest ends this game.
    let mut options = classic_rules::world::victory::VictoryOptions::default();
    for kind in classic_rules::world::victory::VictoryKind::ALL {
        options = options.with(
            kind,
            kind == classic_rules::world::victory::VictoryKind::Domination,
        );
    }
    game.debug_set_victory_options(options);
    game
}

fn train(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    let idle: Vec<_> = game
        .view()
        .auth
        .cities()
        .iter()
        .filter(|(_, c)| c.owner == player && c.queue.is_empty())
        .map(|(h, _)| h)
        .collect();
    for city in idle {
        let item = ProductionItem::Unit(kind(rules, "core:unit/musketman"));
        let _ = game.execute(rules, Command::EnqueueProduction { player, city, item });
    }
    // The fighter keeps watch.
    let fighters: Vec<_> = game
        .view()
        .auth
        .units()
        .iter_handles()
        .filter(|(_, c)| c.owner == player && c.kind == kind(rules, "core:unit/fighter"))
        .map(|(h, _)| h)
        .collect();
    for unit in fighters {
        let _ = game.execute(rules, Command::Intercept { unit });
    }
}

fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "standard".into());
    let variant = Variant::from_name(&name).unwrap_or_else(|| panic!("unknown variant {name}"));
    let seed = arg(2, 1);
    let rounds = arg(3, 150) as u32;
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules, seed);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, vec![RIVAL], ClassicMind::new(variant));
    driver.set_trace_capacity(2_000_000);
    let mut alive: BTreeSet<u64> = BTreeSet::new();
    let mut air_ids: BTreeSet<u64> = BTreeSet::new();
    let (mut built, mut lost) = (0u32, 0u32);
    let mut ground_lost = 0u32;
    let mut ground_alive: BTreeSet<u64> = BTreeSet::new();
    let mut rival_units_min = usize::MAX;
    let mut first_capture = None;
    let mut op_rounds = 0u32;
    let mut peace_at = None;
    while game.view().auth.turn().round < rounds && game.winner().is_none() {
        let active = game.view().auth.active_player();
        if active == AI {
            driver.run(&mut game, &rules, u32::MAX);
        } else {
            train(&mut game, &rules, active);
            game.execute(&rules, Command::EndTurn { player: active })
                .unwrap();
        }
        let auth = game.view().auth;
        if driver
            .states()
            .states()
            .iter()
            .any(|st| !st.operations().is_empty())
        {
            op_rounds += 1;
        }
        if peace_at.is_none() && !auth.diplomacy().at_war(AI, RIVAL) {
            peace_at = Some(auth.turn().round);
        }
        let mut now = BTreeSet::new();
        for (_, core) in auth.units().iter_handles() {
            if core.owner == AI && wing(&rules, core.kind).is_some() {
                now.insert(core.id.0 as u64);
                air_ids.insert(core.id.0 as u64);
            }
        }
        if std::env::var("AIR_TRACE").is_ok() {
            for (h, core) in auth.units().iter_handles() {
                if wing(&rules, core.kind).is_some() && core.owner == AI {
                    let _ = h;
                    eprintln!(
                        "round {} unit {} {} hp={} at {:?} stance={:?}",
                        auth.turn().round,
                        core.id.0,
                        rules.units[core.kind.0 as usize].name,
                        core.hp,
                        auth.map()
                            .dimensions
                            .to_coord(core.tile)
                            .map(|c| (c.q, c.r)),
                        core.stance
                    );
                }
            }
            for gone in alive.difference(&now) {
                eprintln!("round {} LOST unit {gone}", auth.turn().round);
            }
        }
        if std::env::var("AIR_DEMAND").is_ok() && auth.turn().round % 20 == 5 && active == AI {
            let obs = game.ai_observation(&rules, AI);
            if let Some(st) = driver.states().get(AI) {
                eprintln!(
                    "round {} wanted_strikers={} shortfall_strikers={} fighters={} seen_cities={} treasury={} income={} relations={:?}",
                    auth.turn().round,
                    classic_rules::ai::air::wanted_strikers(&obs, st),
                    classic_rules::ai::air::shortfall(&obs, st, &rules, classic_rules::ai::air::Wing::Striker),
                    classic_rules::ai::air::shortfall(&obs, st, &rules, classic_rules::ai::air::Wing::Fighter),
                    st.seen_cities().len(),
                    obs.economy.treasury,
                    obs.economy.income.gold,
                    obs.relations.iter().map(|r| (r.other.0, r.at_war, r.kind)).collect::<Vec<_>>()
                );
            }
        }
        let mut ground_now = BTreeSet::new();
        for (_, core) in auth.units().iter_handles() {
            if core.owner == AI && wing(&rules, core.kind).is_none() {
                ground_now.insert(core.id.0 as u64);
            }
        }
        ground_lost += ground_alive.difference(&ground_now).count() as u32;
        ground_alive = ground_now;
        rival_units_min = rival_units_min.min(auth.indexes().units_of(RIVAL).len());
        built += now.difference(&alive).count() as u32;
        lost += alive.difference(&now).count() as u32;
        alive = now;
        if first_capture.is_none() && auth.indexes().cities_of(RIVAL).len() < 3 {
            first_capture = Some(auth.turn().round);
        }
    }
    let (mut strikes, mut rebases, mut sweeps, mut posts, mut rejected, mut ordered) =
        (0, 0, 0, 0, 0, 0);
    for e in driver.trace().entries() {
        let air_actor = matches!(e.actor, Actor::Unit(id) if air_ids.contains(&(id.0 as u64)));
        if air_actor && std::env::var("AIR_TRACE").is_ok() {
            eprintln!(
                "  trace r{} {:?} {:?} {:?} {:?}",
                e.round, e.actor, e.reason, e.command, e.outcome
            );
        }
        let ok = e.outcome == Outcome::Applied;
        match &e.command {
            Some(Command::RangedAttack { .. }) if ok && air_actor => strikes += 1,
            Some(Command::RangedAttack { unit, .. }) if ok && e.actor == Actor::Player => {
                // The generic advisor's shot: count it when the shooter is an aircraft (by the live unit, if any).
                if game
                    .view()
                    .auth
                    .units()
                    .get(*unit)
                    .is_some_and(|u| wing(&rules, u.core.kind).is_some())
                {
                    strikes += 1;
                }
            }
            Some(Command::Rebase { .. }) if ok => rebases += 1,
            Some(Command::AirSweep { .. }) if ok => sweeps += 1,
            Some(Command::Intercept { .. }) if ok => posts += 1,
            Some(Command::EnqueueProduction {
                item: ProductionItem::Unit(k),
                ..
            }) if ok && wing(&rules, *k).is_some() => ordered += 1,
            Some(_) if !ok && air_actor => rejected += 1,
            _ => {}
        }
    }
    let exhausted = driver
        .trace()
        .entries()
        .filter(|e| e.reason == Reason::TurnBudgetExhausted)
        .count();
    let auth = game.view().auth;
    println!(
        "{name} seed={seed} end={} winner={:?} victory={:?} first_capture={first_capture:?} op_rounds={op_rounds} peace_at={peace_at:?} cities={}/{} ordered={ordered} built={built} lost={lost} ground_lost={ground_lost} rival_units_min={rival_units_min} strikes={strikes} rebases={rebases} sweeps={sweeps} postures={posts} air_rejected={rejected} budget_exhausted={exhausted} rejected={}",
        auth.turn().round,
        game.winner(),
        game.outcome().map(|o| o.kind),
        auth.indexes().cities_of(AI).len(),
        auth.indexes().cities_of(RIVAL).len(),
        driver.counters().commands_rejected
    );
}
