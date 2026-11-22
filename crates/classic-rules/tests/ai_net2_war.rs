//! The war executors of `net2` (part C): the war order the war machine reads, the sea leg of the staging.
//! Scenes of `ai_landings`.
//!
//! (landings: Landings and paratroopers (`ai::operations`, classic/docs/ai-strikes.md "Landings"): an
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


use classic_rules::ai::net2::plan::{Net2Plan, Opening, OrderPhase, WarKind, WarOrder, WarPlan};
use classic_rules::ai::operations::{set_net2_for_test, Routes};
use classic_rules::ai::retake::order_margin;

fn order(kind: WarKind) -> Net2Plan {
    let war = WarOrder { rival: ENEMY, kind, since: 0, budget: kind.budget(), phase: OrderPhase::Preparing, value: 800, aim: None, opening: Opening::Plain, raid: false };
    Net2Plan { war: WarPlan::Order(war), ..Net2Plan::default() }
}

/// Open point 2 of the design: the staging DOES cross water for a walker that embarks (the known map's profile
/// embarks once the player knows how), and with legs on it names the shore the army boards from and the one it
/// lands on. Without legs the stage is the same and the leg is empty.
#[test]
fn the_staging_crosses_a_strait_and_names_the_sea_leg() {
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules);
    let view = game.player_view(&rules, AI);
    let walker = kind(&rules, "core:unit/swordsman");
    let (from, target) = (tile(&game, 7, 10), tile(&game, 14, 10));
    let mut routes = Routes::new(view);
    let plain = routes.stage(walker, from, target, classic_rules::ai::operations::STAGING_DISTANCE, classic_rules::ai::operations::BORDER_REACH).expect("a way over the strait");
    assert!(plain.embark.is_none() && plain.land.is_none());
    routes.with_legs(true);
    let sea = routes.stage(walker, from, target, classic_rules::ai::operations::STAGING_DISTANCE, classic_rules::ai::operations::BORDER_REACH).unwrap();
    assert_eq!((sea.tile, sea.walk, sea.strike), (plain.tile, plain.walk, plain.strike), "the leg changes nothing of the route");
    let (embark, land) = (sea.embark.expect("boards"), sea.land.expect("lands"));
    assert!(!is_water(&game, embark) && !is_water(&game, land));
    let dims = game.view().auth.map().dimensions;
    assert!(dims.to_coord(embark).unwrap().q <= 8 && dims.to_coord(land).unwrap().q >= 11, "{embark:?} {land:?}");
}

/// The war machine reads the order: no plan plays as today; a plan with no order starts nothing; an `Overseas`
/// order on the enemy across the strait forms an operation on its city, mustering on the boarding shore.
#[test]
fn an_overseas_order_forms_an_operation_across_the_strait_and_no_order_forms_none() {
    let rules = compile_rules(core_rules()).unwrap();
    let formed = |plan: Option<Net2Plan>| {
        let mut game = scene(&rules);
        game.debug_found_city(&rules, AI, tile(&game, 5, 14));
        for (q, r) in [(7, 9), (7, 11), (6, 10), (6, 9), (6, 11), (5, 10)] {
            spawn(&mut game, &rules, AI, q, r, "core:unit/swordsman");
        }
        set_net2_for_test(plan);
        let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
        let mut driver = AiDriver::with_mind(settings, vec![ENEMY], ClassicMind::new(Variant::Landings));
        let round = game.view().auth.turn().round;
        driver.state_mut(AI, round).see_city(SeenCity { tile: tile(&game, 14, 10), owner: ENEMY, population: 5, last_seen_round: round });
        let mut ops = Vec::new();
        for _ in 0..45 {
            let r = game.view().auth.turn().round;
            while game.view().auth.turn().round == r {
                driver.run(&mut game, &rules, u32::MAX);
                if game.view().auth.active_player() == ENEMY {
                    game.execute(&rules, Command::EndTurn { player: ENEMY }).unwrap();
                }
            }
            let round = game.view().auth.turn().round;
            for o in driver.state_mut(AI, round).operations() {
                if !ops.contains(o) {
                    ops.push(*o);
                }
            }
        }
        set_net2_for_test(None);
        ops
    };
    let none = formed(Some(Net2Plan::default()));
    assert!(none.is_empty(), "a plan with no order starts no operation: {none:?}");
    let ops = formed(Some(order(WarKind::Overseas)));
    assert!(!ops.is_empty(), "the order forms an operation");
    assert!(ops.iter().all(|o| o.enemy == ENEMY && o.target == tile(&scene(&rules), 14, 10)), "{ops:?}");
}

#[test]
fn the_retake_margin_falls_with_the_order_value() {
    assert_eq!(order_margin(0), 175);
    assert_eq!(order_margin(550), 175);
    assert_eq!(order_margin(775), 150);
    assert_eq!(order_margin(1000), 125);
    assert_eq!(order_margin(5000), 125);
}

/// Design §2.4 step 4: `known_power` counts `net2`'s soldiers estimate of a rival instead of its army per city
/// when the doubt is at most `SOLDIERS_DOUBT_MAX`: a disarmed rival with four known cities reads as one soldier,
/// not as 2.6 a city. A doubtful estimate, or no plan, keeps the army per city.
#[test]
fn known_power_counts_the_soldiers_estimate_when_it_is_sure() {
    use classic_rules::ai::net2::plan::{RivalRead, Soldiers, SOLDIERS_DOUBT_MAX};
    use classic_rules::ai::operations::{era_strength, known_power, ARMY_PER_CITY_TENTHS};
    use classic_rules::ai::planner::TurnBrief;
    use classic_rules::ai::state::AiState;
    use classic_rules::ClassicGame;
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules);
    let obs = game.ai_observation(&rules, AI);
    let brief = TurnBrief::from_observation(&obs, &rules);
    let mut state = AiState::new(AI, 0);
    for q in [12, 14, 16, 18] {
        state.see_city(SeenCity { tile: tile(&game, q, 10), owner: ENEMY, population: 3, last_seen_round: 0 });
    }
    let strength = era_strength(&rules, 0) as u32;
    let per_city = (4 * ARMY_PER_CITY_TENTHS * u64::from(strength) / 10) as u32;
    let with = |soldiers: Option<Soldiers>| {
        let plan = soldiers.map(|soldiers| {
            let read = RivalRead { soldiers, ..empty_read(ENEMY) };
            Net2Plan { rivals: vec![read], ..Net2Plan::default() }
        });
        set_net2_for_test(plan);
        let power = known_power(&brief, &state, &rules, ENEMY, &[(ENEMY, 0)]);
        set_net2_for_test(None);
        power
    };
    assert_eq!(with(None), per_city, "no plan: the army per city");
    assert_eq!(with(Some(Soldiers::Unread)), per_city, "no estimate: the army per city");
    assert_eq!(with(Some(Soldiers::Estimate { count: 1, doubt: 0 })), strength, "an exact estimate: one soldier");
    assert_eq!(with(Some(Soldiers::Estimate { count: 1, doubt: SOLDIERS_DOUBT_MAX + 1 })), per_city, "a doubtful one: the army per city");
}

fn empty_read(rival: PlayerId) -> classic_rules::ai::net2::plan::RivalRead {
    use classic_rules::ai::net2::plan::{Help, RivalRead};
    RivalRead { rival, weak: 0, reach: 0, menace: 0, distrust: 0, prey: 0, runaway: 0, spoiler: 0, retake: 0, duty: 0, grabber: 0, border: 0, brake: 0, war_value: 0, war_kind: WarKind::Prey, trust: 1000, help: Help::Give, soldiers: Default::default() }
}
