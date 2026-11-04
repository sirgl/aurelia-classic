//! The AI's carriers (`ai::carrier`, classic/docs/ai-strikes.md "Carriers"): a war operation
//! marches on a coastal city out of every city's reach of the bomber we have; the port builds
//! a carrier, the bomber is based on it, the carrier sails toward the target. The scene: our
//! port on the west shore, sea for twenty columns, the enemy's city on an island in the east.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::state::{Composition, Operation, OperationPhase, SeenCity};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{PlayerId, ProductionItem, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
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

fn scene(rules: &CompiledRuleset) -> (GameState, Operation) {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 44, height: 20, rng_seed: 31 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    let water: Vec<(TileId, TerrainId)> = (0..20).flat_map(|r| (11..30).map(move |q| (q, r))).map(|(q, r)| (tile(&game, q, r), TerrainId(WATER))).collect();
    game.debug_set_terrain_batch(rules, &water);
    let port = game.debug_found_city(rules, AI, tile(&game, 10, 10));
    game.debug_set_population(rules, port, 12);
    let city = game.debug_found_city(rules, ENEMY, tile(&game, 30, 10));
    game.debug_set_population(rules, city, 6);
    game.debug_meet(AI, ENEMY);
    let seen: Vec<TileId> = (0..44).flat_map(|q| (0..20).map(move |r| (q, r))).map(|(q, r)| tile(&game, q, r)).collect();
    game.debug_reveal(AI, &seen);
    for key in ["core:tech/electronics", "core:tech/radar", "core:tech/flight", "core:tech/optics", "core:tech/compass"] {
        game.debug_learn(rules, AI, tech(rules, key));
    }
    game.debug_earn(AI, 5000);
    // A garrison, so that the port builds for the war and not for its own defence.
    spawn(&mut game, rules, AI, 10, 10, "core:unit/infantry");
    game.execute(rules, Command::DeclareWar { player: AI, enemy: ENEMY }).unwrap();
    let round = game.view().auth.turn().round;
    let operation = Operation {
        target: tile(&game, 30, 10),
        enemy: ENEMY,
        muster: tile(&game, 27, 10),
        need: Composition { melee: 2, ranged: 0, siege: 0 },
        phase: OperationPhase::Marching,
        since_round: round,
        phase_round: round,
    };
    (game, operation)
}

fn driver_with(rules: &CompiledRuleset, game: &GameState, variant: Variant, operation: Operation) -> AiDriver<classic_rules::classic::Classic, ClassicMind> {
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, vec![ENEMY], ClassicMind::new(variant));
    driver.set_trace_capacity(100_000);
    let round = game.view().auth.turn().round;
    let memory = driver.state_mut(AI, round);
    memory.see_city(SeenCity { tile: operation.target, owner: ENEMY, population: 6, last_seen_round: round });
    memory.start_operation(operation);
    driver
}

fn carrier_ordered(rules: &CompiledRuleset, variant: Variant) -> bool {
    let (mut game, operation) = scene(rules);
    spawn(&mut game, rules, AI, 10, 10, "core:unit/bomber");
    let mut driver = driver_with(rules, &game, variant, operation);
    driver.run(&mut game, rules, u32::MAX);
    let ordered = driver.trace().entries().filter_map(|e| e.command.clone()).any(|c| match c {
        Command::EnqueueProduction { item: ProductionItem::Unit(k), .. } => rules.units[k.0 as usize].combat.carries.is_some(),
        _ => false,
    });
    ordered
}

#[test]
fn a_coastal_target_out_of_the_bombers_reach_makes_the_port_build_a_carrier() {
    let rules = compile_rules(core_rules()).unwrap();
    assert!(carrier_ordered(&rules, Variant::Standard));
    assert!(!carrier_ordered(&rules, Variant::Previous), "the AI before the heavy arms builds none");
}

#[test]
fn the_bomber_boards_the_carrier_and_the_carrier_sails_for_the_target() {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, operation) = scene(&rules);
    let bomber = spawn(&mut game, &rules, AI, 10, 10, "core:unit/bomber");
    let carrier = spawn(&mut game, &rules, AI, 11, 10, "core:unit/carrier");
    let mut driver = driver_with(&rules, &game, Variant::Standard, operation);
    let start = game.view().auth.units().get(carrier).unwrap().core.tile;
    let target = tile(&game, 30, 10);
    let dims = game.view().auth.map().dimensions;
    for _ in 0..4 {
        let round = game.view().auth.turn().round;
        while game.view().auth.turn().round == round {
            driver.run(&mut game, &rules, u32::MAX);
            if game.view().auth.active_player() == ENEMY {
                game.execute(&rules, Command::EndTurn { player: ENEMY }).unwrap();
            }
        }
    }
    let boarded = driver.trace().entries().any(|e| e.reason == classic_rules::ai::trace::Reason::BaseOnCarrier && matches!(e.command, Some(Command::RebaseToCarrier { unit, .. }) if unit == bomber));
    assert!(boarded, "the bomber never boarded");
    let now = game.view().auth.units().get(carrier).unwrap().core.tile;
    assert!(dims.distance(now, target).unwrap() + 4 <= dims.distance(start, target).unwrap(), "the carrier did not sail: {start:?} -> {now:?}");
    let bomber_at = game.view().auth.units().get(bomber).unwrap().core.tile;
    assert_eq!(bomber_at, now, "the bomber sails with it");
}
