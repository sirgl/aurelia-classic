//! Orders judged by their effect (`ai::effects`, classic/docs/ai-effects-checked.md):
//! a walk the game applies but that leaves the unit on its tile is a failed
//! attempt of its task and is not ordered again; workers are wanted for the
//! jobs the job finder offers; a job is ranked and kept by the unit's own
//! path; a work boat is built only for a deposit it can sail to.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::effects::{EffectChecks, Walks};
use classic_rules::ai::planner::{observe, Decision, PlanTools, TurnBrief, UnitView};
use classic_rules::ai::planner_map::MapScratch;
use classic_rules::ai::planner_workers::{decide_worker, open_jobs};
use classic_rules::ai::sea_work::sails_to;
use classic_rules::ai::state::{AiState, Strategy, UnitTask};
use classic_rules::ai::trace::{Actor, AiCounters, Reason};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{PlayerId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;

const US: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);
/// The codes of the map `GameState::new` builds.
const GRASSLAND: u16 = 1;
const WATER: u16 = 4;

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    let full = format!("core:unit/{key}");
    UnitKindId(rules.units.iter().position(|u| u.key == full).unwrap() as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    let full = format!("core:tech/{key}");
    TechId(rules.technologies.iter().position(|t| t.key == full).unwrap() as u16)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// Open grassland 30 × 20, every tile known to us, `water` tiles water.
fn world(rules: &CompiledRuleset, water: impl Fn(i32, i32) -> bool) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 });
    let dims = game.view().auth.map().dimensions;
    let terrain: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32)
        .map(|t| {
            let c = dims.to_coord(TileId(t)).unwrap();
            (TileId(t), TerrainId(if water(c.q, c.r) { WATER } else { GRASSLAND }))
        })
        .collect();
    game.debug_set_terrain_batch(rules, &terrain);
    let tiles: Vec<TileId> = terrain.iter().map(|(t, _)| *t).collect();
    game.debug_clear_features(rules, &tiles);
    game.debug_set_river_edges(rules, &[]);
    game.debug_reveal(US, &tiles);
    game
}

fn unit_tile(game: &GameState, unit: UnitHandle) -> TileId {
    game.view().auth.units().get(unit).unwrap().core.tile
}

#[test]
fn the_variant_and_its_single_checks_are_named() {
    assert_eq!(Variant::from_name("effects-checked"), Some(Variant::EffectsChecked));
    assert_eq!(Variant::EffectsChecked.effect_checks(), EffectChecks::ALL);
    // The worker demand is the game's AI's; the control goes without it.
    assert_eq!(Variant::Standard.effect_checks(), EffectChecks { demand: true, ..EffectChecks::NONE });
    assert_eq!(Variant::GuessedDemand.effect_checks(), EffectChecks::NONE);
    let single = [Variant::EffectsMoves, Variant::EffectsJobs].map(|v| v.effect_checks());
    assert_eq!(single.map(|c| (c.moves, c.demand, c.jobs)), [(true, true, false), (false, true, true)]);
}

/// A march into a foreign unit is applied and leaves the unit where it stood:
/// the next decision counts it against the task and the same march is not
/// ordered again; a walk that moves the unit counts nothing.
#[test]
fn a_walk_that_leaves_the_unit_on_its_tile_is_a_failed_attempt() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, |_, _| false);
    game.debug_found_city(&rules, US, tile(&game, 2, 10));
    game.debug_found_city(&rules, THEM, tile(&game, 27, 10));
    game.debug_meet(US, THEM);
    let warrior = unit_kind(&rules, "warrior");
    let start = tile(&game, 10, 10);
    let ours = game.debug_spawn_unit(&rules, US, start, warrior);
    game.debug_spawn_unit(&rules, THEM, tile(&game, 11, 10), warrior);
    let id = game.view().auth.units().get(ours).unwrap().core.id;
    let mut state = AiState::new(US, 1);
    state.set_task(id, UnitTask::Scout { target: tile(&game, 20, 10) }, 1);
    let walks = Walks::default();
    let mut dropped = 0;
    let march = Command::MarchTo { unit: ours, destination: tile(&game, 12, 10) };
    let order = |command: &Command| ordered(Decision::act(Actor::Unit(id), Reason::Explore, command.clone()));

    let first = {
        let (near, _) = game.ai_local_observation(&rules, US, ours, 5).unwrap();
        walks.judged(&near, &mut state, &mut dropped, order(&march))
    };
    assert!(first.command.is_some());
    assert!(game.execute(&rules, march.clone()).is_ok(), "the game applies the march");
    assert_eq!(unit_tile(&game, ours), start, "the march changed nothing");

    let again = {
        let (near, _) = game.ai_local_observation(&rules, US, ours, 5).unwrap();
        walks.judged(&near, &mut state, &mut dropped, order(&march))
    };
    assert_eq!(state.task_of(id).unwrap().failures, 1, "the walk without effect is a failure");
    assert!(again.command.is_none() && again.reason == Reason::Blocked, "the same walk is not ordered again");

    let step = Command::MoveUnit { unit: ours, destination: tile(&game, 10, 11) };
    let around = {
        let (near, _) = game.ai_local_observation(&rules, US, ours, 5).unwrap();
        walks.judged(&near, &mut state, &mut dropped, order(&step))
    };
    assert!(around.command.is_some(), "another walk is ordered");
    assert!(game.execute(&rules, step.clone()).is_ok());
    assert_ne!(unit_tile(&game, ours), start);
    {
        let (near, _) = game.ai_local_observation(&rules, US, ours, 5).unwrap();
        walks.judged(&near, &mut state, &mut dropped, |_| Decision::refuse(Actor::Unit(id), Reason::Waiting));
    }
    assert_eq!(state.task_of(id).unwrap().failures, 1, "a walk that moved the unit counts nothing");
    assert_eq!(dropped, 0);
}

/// A planner that decides `decision`.
fn ordered(decision: Decision) -> impl FnOnce(&mut AiState) -> Decision {
    move |_| decision
}

fn scenario() -> ScenarioConfig {
    ScenarioConfig { seed: 0xA1_5EED, players: 4, width: 40, height: 32, cities: 0, units: 0, water_ratio: 15 }
}

/// Walks a unit repeats right after the same walk in one turn, both applied:
/// the signature of a walk that changed nothing.
fn repeated_walks(variant: Variant, rounds: u32) -> usize {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario());
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(variant));
    driver.set_trace_capacity(2_000_000);
    while game.view().auth.turn().round < rounds && game.winner().is_none() {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let walks: Vec<_> = driver
        .trace()
        .entries()
        .filter(|e| matches!(e.actor, Actor::Unit(_)) && e.command.is_some())
        .map(|e| (e.round, e.player, e.actor, format!("{:?}", e.command), format!("{:?}", e.outcome)))
        .collect();
    walks
        .windows(2)
        .filter(|w| w[0] == w[1] && (w[0].3.contains("MarchTo") || w[0].3.contains("MoveUnit")) && w[0].4 == "Applied")
        .count()
}

#[test]
fn a_game_of_the_variant_repeats_few_walks() {
    let standard = repeated_walks(Variant::Standard, 200);
    let checked = repeated_walks(Variant::EffectsChecked, 200);
    assert!(standard > 0, "the standard repeats walks that changed nothing");
    assert!(checked * 4 <= standard, "repeated walks: {checked} with the check, {standard} without");
}

/// The judged walks are the turn's memory only: a game saved between two
/// players' turns continues identically.
#[test]
fn saved_between_players_the_variant_continues_identically() {
    let rules = compile_rules(core_rules()).unwrap();
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mind = || AiDriver::with_mind(settings.clone(), Vec::new(), ClassicMind::new(Variant::EffectsChecked));
    let mut game = generate_new_game(&rules, &scenario());
    let mut live = mind();
    while game.view().auth.turn().round < 20 {
        live.run(&mut game, &rules, 50);
    }
    loop {
        let report = live.run(&mut game, &rules, 1);
        if report.players_done.len() == 1 && game.view().auth.active_player() != US {
            break;
        }
    }
    let world = encode(&game, &rules);
    let memory = live.save();
    let mut loaded = decode(&world, &rules).unwrap();
    let mut restored = mind();
    restored.load(&memory, loaded.view().auth.map().tile_count(), &rules).unwrap();
    let play = |game: &mut GameState, driver: &mut classic_rules::ai::AiDriver| {
        let mut hashes = Vec::new();
        while hashes.len() < 40 {
            if driver.run(game, &rules, 13).round_done {
                hashes.push(authoritative_hash(game));
            }
        }
        hashes
    };
    assert_eq!(play(&mut loaded, &mut restored), play(&mut game, &mut live), "the loaded game diverged");
    assert_eq!(restored.states(), live.states());
}

/// Our city with its ring of land tiles, worked or not.
fn ring(game: &GameState, centre: TileId) -> Vec<TileId> {
    let dims = game.view().auth.map().dimensions;
    let mut tiles: Vec<TileId> = dims.neighbors(centre).into_iter().collect();
    tiles.sort();
    tiles
}

/// A worker is wanted for a tile only when a worker could build there now:
/// without the technologies of the improvements the finder offers no job.
#[test]
fn worker_demand_counts_the_jobs_the_finder_offers() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, |_, _| false);
    let centre = tile(&game, 5, 10);
    game.debug_found_city(&rules, US, centre);
    let land_needs_tech = rules
        .improvements
        .iter()
        .filter(|i| i.work_turns > 0 && !i.unbuildable && !i.placement.water)
        .all(|i| i.required_tech.is_some());
    assert!(land_needs_tech, "every land improvement needs a technology (the premise)");
    let jobs = |game: &GameState| {
        let obs = game.ai_observation(&rules, US);
        let brief = TurnBrief::from_observation(&obs, &rules);
        open_jobs(&obs.view, &rules, &brief, Strategy::Expand, ring(game, centre).into_iter())
    };
    assert_eq!(jobs(&game), 0, "no technology: no job, however many tiles are unimproved");
    for t in 0..rules.technologies.len() {
        game.debug_learn_tech(&rules, US, TechId(t as u16));
    }
    assert_eq!(jobs(&game), ring(&game, centre).len() as u32, "every unimproved grassland tile of the ring is a job");
}

/// A worker's decision with the variant's checks.
fn worker_decision(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle, state: &mut AiState, effects: EffectChecks) -> Decision {
    let obs = game.ai_observation(rules, US);
    let mut brief = observe(&obs, &mut AiState::new(US, 1), rules);
    brief.effects = effects;
    let (local, _) = game.ai_local_observation(rules, US, unit, 5).unwrap();
    let view = UnitView { local: &local, brief: &brief };
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let (mut scratch, mut counters) = (MapScratch::default(), AiCounters::default());
    let mut tools = PlanTools { rules, settings: &settings, scratch: &mut scratch, counters: &mut counters };
    decide_worker(&view, state, &mut tools)
}

/// A worker on an island it cannot leave: the straight distance gives it a
/// job it never reaches (`NoPath` every turn); by its path it has none, and a
/// job it holds is given up.
#[test]
fn a_job_the_workers_path_does_not_reach_is_no_job() {
    let rules = compile_rules(core_rules()).unwrap();
    let island = (15, 10);
    let mut game = world(&rules, |q, r| (q - island.0).abs() <= 1 && (r - island.1).abs() <= 1 && (q, r) != island);
    let centre = tile(&game, 5, 10);
    game.debug_found_city(&rules, US, centre);
    for t in 0..rules.technologies.len() {
        let def = &rules.technologies[t];
        if !def.key.contains("optics") && !def.key.contains("astronomy") {
            game.debug_learn_tech(&rules, US, TechId(t as u16));
        }
    }
    let worker = game.debug_spawn_unit(&rules, US, tile(&game, island.0, island.1), unit_kind(&rules, "worker"));
    let id = game.view().auth.units().get(worker).unwrap().core.id;

    let mut state = AiState::new(US, 1);
    let blind = worker_decision(&game, &rules, worker, &mut state, EffectChecks::NONE);
    assert_eq!(blind.reason, Reason::NoPath, "the straight distance chose a job across the water");
    assert!(matches!(state.task_of(id).map(|t| t.task), Some(UnitTask::Improve { .. })));

    let checked = worker_decision(&game, &rules, worker, &mut state, EffectChecks { jobs: true, ..EffectChecks::NONE });
    assert_eq!(checked.reason, Reason::NoJob, "by its path the worker has no job");
    assert!(state.task_of(id).is_none(), "the job it could not reach is given up");
}

/// A work boat may not enter deep water before Astronomy: a deposit there is
/// none of its jobs until then; one on the shallow water is.
#[test]
fn a_boat_sails_only_where_its_movement_rules_let_it() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, |q, _| q >= 10);
    game.debug_found_city(&rules, US, tile(&game, 9, 10));
    game.debug_learn_tech(&rules, US, tech(&rules, "sailing"));
    let sails = |game: &GameState, q: i32| {
        let obs = game.ai_observation(&rules, US);
        let brief = TurnBrief::from_observation(&obs, &rules);
        sails_to(&obs, &brief, &rules, tile(game, q, 10))
    };
    assert!(sails(&game, 10), "the shallow water next to the shore");
    assert!(!sails(&game, 13), "deep water before Astronomy");
    game.debug_learn_tech(&rules, US, tech(&rules, "astronomy"));
    assert!(sails(&game, 13), "deep water after Astronomy");
}
