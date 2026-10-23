//! Landings and paratroopers (`ai::operations`, classic/docs/ai-strikes.md "Landings"): an
//! army that has to cross water gathers on the enemy's shore, not afloat; a paratrooper in a
//! city jumps to the tile where its operation gathers. The scenes: two shores of grassland
//! with a strait between, the AI on the west one.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::state::{Composition, Operation, OperationPhase, SeenCity};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{PlayerId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;

const AI: PlayerId = PlayerId(0);
const ENEMY: PlayerId = PlayerId(1);
const GRASSLAND: u16 = 1;
const WATER: u16 = 4;

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn spawn(game: &mut GameState, rules: &CompiledRuleset, owner: PlayerId, q: i32, r: i32, key: &str) -> UnitHandle {
    let at = tile(game, q, r);
    game.debug_spawn_unit(rules, owner, at, kind(rules, key))
}

/// Grassland, a strait of water in the columns 9 and 10; the AI's city (7, 10) on the west
/// shore, the enemy's (14, 10) on the east one, at war, the AI knows how to embark and has
/// explored the east shore.
fn scene(rules: &CompiledRuleset) -> GameState {
    wide_scene(rules, 2)
}

/// The same with a strait of `width` columns from column 9, the enemy's city `width + 5`
/// columns east of it (the shore starts at column 9 + width).
fn wide_scene(rules: &CompiledRuleset, width: i32) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 40, height: 20, rng_seed: 31 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    let water: Vec<(TileId, TerrainId)> = (0..20).flat_map(|r| (9..9 + width).map(move |q| (q, r))).map(|(q, r)| (tile(&game, q, r), TerrainId(WATER))).collect();
    game.debug_set_terrain_batch(rules, &water);
    game.debug_found_city(rules, AI, tile(&game, 7, 10));
    let city = game.debug_found_city(rules, ENEMY, tile(&game, 12 + width, 10));
    game.debug_set_population(rules, city, 5);
    game.debug_meet(AI, ENEMY);
    let seen: Vec<TileId> = (0..40).flat_map(|q| (0..20).map(move |r| (q, r))).map(|(q, r)| tile(&game, q, r)).collect();
    game.debug_reveal(AI, &seen);
    game.debug_learn(rules, AI, tech(rules, "core:tech/optics"));
    game.execute(rules, Command::DeclareWar { player: AI, enemy: ENEMY }).unwrap();
    game
}

fn is_water(game: &GameState, tile: TileId) -> bool {
    game.view().auth.map().is_water(tile)
}

fn operation(game: &GameState, muster: TileId, phase: OperationPhase) -> Operation {
    let round = game.view().auth.turn().round;
    Operation {
        target: tile(game, 14, 10),
        enemy: ENEMY,
        muster,
        need: Composition { melee: 2, ranged: 0, siege: 0 },
        phase,
        since_round: round,
        phase_round: round,
    }
}

/// The commands of the AI's first turn with its memory holding the operation.
fn first_turn(game: &mut GameState, rules: &CompiledRuleset, variant: Variant, operation: Operation) -> Vec<(kernel::ai::trace::Reason, Command)> {
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, vec![ENEMY], ClassicMind::new(variant));
    driver.set_trace_capacity(100_000);
    let round = game.view().auth.turn().round;
    let memory = driver.state_mut(AI, round);
    memory.see_city(SeenCity { tile: operation.target, owner: ENEMY, population: 5, last_seen_round: round });
    memory.start_operation(operation);
    driver.run(game, rules, u32::MAX);
    driver.trace().entries().filter(|e| e.outcome == kernel::ai::trace::Outcome::Applied).filter_map(|e| e.command.clone().map(|c| (e.reason, c))).collect()
}

/// A paratrooper in the port jumps over the strait to the tile where the operation gathers;
/// the standard AI sends it by boat.
#[test]
fn a_paratrooper_jumps_to_the_tile_where_its_operation_gathers() {
    let rules = compile_rules(core_rules()).unwrap();
    let trooper = |variant: Variant| {
        let mut game = scene(&rules);
        let unit = spawn(&mut game, &rules, AI, 7, 10, "core:unit/paratrooper");
        let muster = tile(&game, 11, 10);
        let op = operation(&game, muster, OperationPhase::Forming);
        (first_turn(&mut game, &rules, variant, op), unit, game)
    };
    let (commands, unit, game) = trooper(Variant::Landings);
    let jump = commands.iter().find(|(_, c)| matches!(c, Command::Paradrop { unit: u, .. } if *u == unit));
    let Some((reason, Command::Paradrop { destination, .. })) = jump else { panic!("{commands:?}") };
    assert_eq!(*reason, kernel::ai::trace::Reason::Paradrop);
    let dims = game.view().auth.map().dimensions;
    assert!(!is_water(&game, *destination));
    assert!(dims.distance(*destination, tile(&game, 14, 10)).unwrap() >= classic_rules::ai::operations::STAGING_DISTANCE, "out of the city's strike");
    assert!(dims.distance(*destination, tile(&game, 11, 10)).unwrap() <= classic_rules::ai::operations::MUSTER_RADIUS);
    let (commands, unit, _) = trooper(Variant::Previous);
    assert!(!commands.iter().any(|(_, c)| matches!(c, Command::Paradrop { unit: u, .. } if *u == unit)), "{commands:?}");
}

/// No jump into an enemy unit's reach: an enemy army next to the muster point keeps the
/// paratrooper at home.
#[test]
fn a_paratrooper_does_not_jump_next_to_an_enemy() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let unit = spawn(&mut game, &rules, AI, 7, 10, "core:unit/paratrooper");
    // Eyes on the east shore, and an enemy on every tile near the muster point.
    spawn(&mut game, &rules, AI, 8, 10, "core:unit/scout");
    for q in 10..=13 {
        for r in 7..=13 {
            if (q, r) != (14, 10) {
                spawn(&mut game, &rules, ENEMY, q, r, "core:unit/warrior");
            }
        }
    }
    let muster = tile(&game, 11, 10);
    let op = operation(&game, muster, OperationPhase::Forming);
    let commands = first_turn(&mut game, &rules, Variant::Landings, op);
    assert!(!commands.iter().any(|(_, c)| matches!(c, Command::Paradrop { unit: u, .. } if *u == unit)), "{commands:?}");
}

/// A landing across water when no land route exists: a band with the embarking technology
/// crosses the strait and stands on the enemy's shore within a few rounds.
#[test]
fn an_army_lands_on_the_far_shore_when_no_land_route_exists() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    for (q, r) in [(7, 9), (7, 11), (6, 10), (6, 9), (6, 11)] {
        spawn(&mut game, &rules, AI, q, r, "core:unit/swordsman");
    }
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, vec![ENEMY], ClassicMind::new(Variant::Landings));
    let round = game.view().auth.turn().round;
    let muster = tile(&game, 12, 10);
    let op = operation(&game, muster, OperationPhase::Forming);
    let memory = driver.state_mut(AI, round);
    memory.see_city(SeenCity { tile: op.target, owner: ENEMY, population: 5, last_seen_round: round });
    memory.start_operation(op);
    let mut landed = false;
    for _ in 0..30 {
        let r = game.view().auth.turn().round;
        while game.view().auth.turn().round == r {
            driver.run(&mut game, &rules, u32::MAX);
            if game.view().auth.active_player() == ENEMY {
                game.execute(&rules, Command::EndTurn { player: ENEMY }).unwrap();
            }
        }
        let auth = game.view().auth;
        let dims = auth.map().dimensions;
        landed |= auth.indexes().units_of(AI).iter().filter_map(|h| auth.units().get(*h)).any(|u| !auth.map().is_water(u.core.tile) && dims.to_coord(u.core.tile).unwrap().q >= 11);
    }
    assert!(landed, "no unit of the AI stood on the far shore after 30 rounds");
}
