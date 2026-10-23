//! The scouts of one empire share the unknown map (classic/docs/ai-scouting.md): a
//! target is chosen against the other scouts' targets and places, ruins
//! the scout knows are targets, a target nobody can stop on is given up.

use classic_content::core::core_rules;
use classic_rules::ai::planner::{observe, PlanTools, UnitView};
use classic_rules::ai::planner_map::MapScratch;
use classic_rules::ai::planner_units::decide_unit;
use classic_rules::ai::scouting::{scout_target, target_valid, wanted, wants_recon};
use classic_rules::ai::state::{AiState, UnitTask};
use classic_rules::ai::trace::{AiCounters, Reason};
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::command::Command;
use classic_rules::ids::{EntityId, PlayerId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::{tiles_in_hex_radius, HexCoord};
use classic_rules::rules::content::ImprovementKind;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const WIDTH: i32 = 40;
const HEIGHT: i32 = 30;

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

fn distance(game: &GameState, a: TileId, b: TileId) -> i32 {
    game.view().auth.map().dimensions.distance(a, b).unwrap()
}

fn scout(rules: &CompiledRuleset) -> UnitKindId {
    UnitKindId(
        rules
            .units
            .iter()
            .position(|u| u.key == "core:unit/scout")
            .unwrap() as u16,
    )
}

/// A flat land map with two players, nothing known to the first one.
fn open_world(rules: &CompiledRuleset) -> GameState {
    GameState::new(
        rules,
        GameConfig {
            players: 2,
            width: WIDTH as u32,
            height: HEIGHT as u32,
            rng_seed: 7,
        },
    )
}

fn reveal_disc(game: &mut GameState, q: i32, r: i32, radius: i32) {
    let tiles = tiles_in_hex_radius(&game.view().auth.map().dimensions, tile(game, q, r), radius);
    game.debug_reveal(P0, &tiles);
}

fn id_of(game: &GameState, unit: UnitHandle) -> EntityId {
    game.view().auth.units().get(unit).unwrap().core.id
}

/// The target `scout_target` gives the unit, with `state` holding the other
/// scouts' tasks.
fn target_of(
    game: &GameState,
    rules: &CompiledRuleset,
    state: &AiState,
    unit: UnitHandle,
) -> Option<TileId> {
    let observation = game.ai_observation(rules, P0);
    let brief = observe(&observation, &mut AiState::new(P0, 1), rules);
    let (local, _) = game.ai_local_observation(rules, P0, unit, 5).unwrap();
    let view = UnitView {
        local: &local,
        brief: &brief,
    };
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let (mut scratch, mut counters) = (MapScratch::default(), AiCounters::default());
    let mut tools = PlanTools {
        rules,
        settings: &settings,
        scratch: &mut scratch,
        counters: &mut counters,
    };
    scout_target(&view, state, &mut tools)
}

/// Every scout in turn takes its target; the next one sees the tasks.
fn allocate(game: &GameState, rules: &CompiledRuleset, scouts: &[UnitHandle]) -> Vec<TileId> {
    let mut state = AiState::new(P0, 1);
    let mut targets = Vec::new();
    for unit in scouts {
        let target = target_of(game, rules, &state, *unit).expect("a frontier to go to");
        state.set_task(id_of(game, *unit), UnitTask::Scout { target }, 1);
        targets.push(target);
    }
    targets
}

#[test]
fn two_scouts_at_one_place_do_not_take_neighbouring_targets() {
    let rules = rules();
    let mut game = open_world(&rules);
    let home = tile(&game, 20, 15);
    let scouts: Vec<UnitHandle> = (0..2)
        .map(|_| game.debug_spawn_unit(&rules, P0, home, scout(&rules)))
        .collect();
    reveal_disc(&mut game, 20, 15, 4);
    let targets = allocate(&game, &rules, &scouts);
    let apart = distance(&game, targets[0], targets[1]);
    assert!(apart >= 6, "targets {apart} tiles apart");
}

#[test]
fn three_scouts_split_the_frontier_between_them() {
    let rules = rules();
    let mut game = open_world(&rules);
    let home = tile(&game, 20, 15);
    let scouts: Vec<UnitHandle> = (0..3)
        .map(|_| game.debug_spawn_unit(&rules, P0, home, scout(&rules)))
        .collect();
    reveal_disc(&mut game, 20, 15, 4);
    let targets = allocate(&game, &rules, &scouts);
    for (i, a) in targets.iter().enumerate() {
        for b in &targets[i + 1..] {
            let apart = distance(&game, *a, *b);
            assert!(
                apart >= 5,
                "two of three targets {apart} tiles apart: {targets:?}"
            );
        }
    }
}

fn ruins_improvement(rules: &CompiledRuleset) -> classic_rules::ids::ImprovementId {
    let index = rules
        .improvements
        .iter()
        .position(|d| d.kind == ImprovementKind::AncientRuins)
        .unwrap();
    classic_rules::ids::ImprovementId(index as u16)
}

/// Everything within 12 tiles of (20, 15) is known except the far east, so
/// the nearest unknown lies about 10 tiles from a scout at the centre.
fn known_world(rules: &CompiledRuleset) -> (GameState, UnitHandle) {
    let mut game = open_world(rules);
    let unit = game.debug_spawn_unit(rules, P0, tile(&game, 20, 15), scout(rules));
    reveal_disc(&mut game, 20, 15, 12);
    (game, unit)
}

#[test]
fn ruins_the_scout_knows_are_a_target_before_a_distant_frontier() {
    let rules = rules();
    let (mut game, unit) = known_world(&rules);
    let ruins = tile(&game, 16, 15);
    game.debug_set_improvement_batch(&rules, &[(ruins, ruins_improvement(&rules))]);
    reveal_disc(&mut game, 16, 15, 1);
    let state = AiState::new(P0, 1);
    assert_eq!(target_of(&game, &rules, &state, unit), Some(ruins));
}

#[test]
fn a_ruin_another_scout_goes_to_is_not_a_target() {
    let rules = rules();
    let (mut game, unit) = known_world(&rules);
    let other = game.debug_spawn_unit(&rules, P0, tile(&game, 20, 15), scout(&rules));
    let ruins = tile(&game, 16, 15);
    game.debug_set_improvement_batch(&rules, &[(ruins, ruins_improvement(&rules))]);
    reveal_disc(&mut game, 16, 15, 1);
    let mut state = AiState::new(P0, 1);
    state.set_task(id_of(&game, other), UnitTask::Scout { target: ruins }, 1);
    assert_ne!(target_of(&game, &rules, &state, unit), Some(ruins));
}

#[test]
fn a_frontier_tile_with_a_foreign_unit_on_it_is_no_target() {
    let rules = rules();
    let mut game = open_world(&rules);
    let unit = game.debug_spawn_unit(&rules, P0, tile(&game, 20, 15), scout(&rules));
    reveal_disc(&mut game, 20, 15, 2);
    let state = AiState::new(P0, 1);
    let free = target_of(&game, &rules, &state, unit).unwrap();
    game.debug_spawn_unit(&rules, P1, free, scout(&rules));
    reveal_disc(&mut game, 20, 15, 2);
    let blocked = target_of(&game, &rules, &state, unit).unwrap();
    assert_ne!(blocked, free);
}

#[test]
fn the_empire_wants_scouts_by_the_unknown_share_of_the_map() {
    assert_eq!(wanted(10, 5), 1);
    assert_eq!(wanted(11, 5), 1);
    assert_eq!(wanted(12, 5), 2, "the second scout goes out with the first recon unit");
    assert_eq!(wanted(60, 20), 2, "a second scout while much is unknown");
    assert_eq!(wanted(60, 50), 1);
    assert_eq!(wanted(120, 20), 1, "the second scout is for the early game");
    assert_eq!(wanted(60, 95), 0, "the map is known");
}

#[test]
fn only_one_recon_unit_is_built_and_not_before_the_warrior_has_explored() {
    assert!(!wants_recon(5, 5, 0), "the warrior explores first");
    assert!(!wants_recon(20, 5, 0), "the first rounds belong to the workers and settlers");
    assert!(wants_recon(30, 5, 0));
    assert!(!wants_recon(20, 5, 1));
    assert!(!wants_recon(60, 20, 1), "the second scout is a soldier");
    assert!(!wants_recon(400, 20, 0), "scouting season is over");
}

#[test]
fn a_scout_heads_for_another_tile_than_the_one_a_foreign_unit_holds() {
    let rules = rules();
    let mut game = open_world(&rules);
    game.debug_found_city(&rules, P0, tile(&game, 20, 15));
    let unit = game.debug_spawn_unit(&rules, P0, tile(&game, 20, 15), scout(&rules));
    reveal_disc(&mut game, 20, 15, 2);
    let observation_state = AiState::new(P0, 1);
    let target = target_of(&game, &rules, &observation_state, unit).unwrap();
    let mut driver = AiDriver::new(
        AiSettings::for_rules(&rules, rules.default_difficulty()),
        Vec::new(),
    );
    game.debug_spawn_unit(&rules, P1, target, scout(&rules));
    reveal_disc(&mut game, 20, 15, 2);
    driver.run(&mut game, &rules, u32::MAX);
    let task = driver
        .states()
        .get(P0)
        .and_then(|s| s.task_of(id_of(&game, unit)))
        .map(|t| t.task);
    assert_ne!(
        task,
        Some(UnitTask::Scout { target }),
        "the scout still goes where a foreign unit stands"
    );
}

fn warrior(rules: &CompiledRuleset) -> UnitKindId {
    UnitKindId(
        rules
            .units
            .iter()
            .position(|u| u.key == "core:unit/warrior")
            .unwrap() as u16,
    )
}

/// A scout at (20, 15) sees a warrior of the other player two tiles east.
fn scout_with_a_hostile_warrior(rules: &CompiledRuleset) -> (GameState, UnitHandle) {
    let mut game = open_world(rules);
    let unit = game.debug_spawn_unit(rules, P0, tile(&game, 20, 15), scout(rules));
    game.debug_spawn_unit(rules, P1, tile(&game, 22, 15), warrior(rules));
    game.debug_force_war(rules, P0, P1);
    reveal_disc(&mut game, 20, 15, 3);
    (game, unit)
}

#[test]
fn a_target_toward_a_hostile_unit_is_given_up() {
    let rules = rules();
    let (game, unit) = scout_with_a_hostile_warrior(&rules);
    let observation = game.ai_observation(&rules, P0);
    let brief = observe(&observation, &mut AiState::new(P0, 1), &rules);
    let (local, _) = game.ai_local_observation(&rules, P0, unit, 5).unwrap();
    let view = UnitView {
        local: &local,
        brief: &brief,
    };
    assert!(!target_valid(&view, &rules, tile(&game, 23, 15)));
    assert!(target_valid(&view, &rules, tile(&game, 17, 15)));
}

#[test]
fn a_frontier_next_to_a_barbarian_camp_is_no_target() {
    let rules = rules();
    let camp = classic_rules::ids::ImprovementId(
        rules
            .improvements
            .iter()
            .position(|d| d.kind == classic_rules::classic::content::BARBARIAN_CAMP)
            .unwrap() as u16,
    );
    let mut game = open_world(&rules);
    let unit = game.debug_spawn_unit(&rules, P0, tile(&game, 20, 15), scout(&rules));
    reveal_disc(&mut game, 20, 15, 3);
    let state = AiState::new(P0, 1);
    let free = target_of(&game, &rules, &state, unit).unwrap();
    game.debug_set_improvement_batch(&rules, &[(free, camp)]);
    reveal_disc(&mut game, 20, 15, 3);
    let target = target_of(&game, &rules, &state, unit).unwrap();
    assert!(
        distance(&game, target, free) > 2,
        "the scout goes {} tiles from a camp",
        distance(&game, target, free)
    );
}

#[test]
fn a_ruin_that_another_scout_goes_to_is_the_target_of_no_second_one() {
    let rules = rules();
    let mut game = open_world(&rules);
    let unit = game.debug_spawn_unit(&rules, P0, tile(&game, 20, 15), scout(&rules));
    let other = game.debug_spawn_unit(&rules, P0, tile(&game, 20, 15), scout(&rules));
    let ruins = tile(&game, 16, 15);
    game.debug_set_improvement_batch(&rules, &[(ruins, ruins_improvement(&rules))]);
    reveal_disc(&mut game, 20, 15, WIDTH);
    let mut state = AiState::new(P0, 1);
    assert_eq!(target_of(&game, &rules, &state, unit), Some(ruins));
    state.set_task(id_of(&game, other), UnitTask::Scout { target: ruins }, 1);
    assert_eq!(target_of(&game, &rules, &state, unit), None);
}

#[test]
fn a_scout_keeps_away_from_the_place_where_another_scout_stands() {
    let rules = rules();
    let (mut game, unit) = known_world(&rules);
    let natural = target_of(&game, &rules, &AiState::new(P0, 1), unit).unwrap();
    let dims = game.view().auth.map().dimensions;
    let at = dims.to_coord(natural).unwrap();
    let standing = tile(&game, at.q, at.r + 3);
    let other = game.debug_spawn_unit(&rules, P0, standing, scout(&rules));
    let mut state = AiState::new(P0, 1);
    state.set_task(
        id_of(&game, other),
        UnitTask::Scout {
            target: tile(&game, 38, 28),
        },
        1,
    );
    let target = target_of(&game, &rules, &state, unit).unwrap();
    let apart = distance(&game, target, standing);
    assert!(
        apart >= 5,
        "the target lies {apart} tiles from the other scout"
    );
}

#[test]
fn a_scout_walks_its_whole_turn_in_one_order() {
    let rules = rules();
    let mut game = open_world(&rules);
    let home = tile(&game, 20, 15);
    game.debug_found_city(&rules, P0, home);
    let unit = game.debug_spawn_unit(&rules, P0, home, scout(&rules));
    reveal_disc(&mut game, 20, 15, 6);
    let mut driver = AiDriver::new(
        AiSettings::for_rules(&rules, rules.default_difficulty()),
        Vec::new(),
    );
    driver.run(&mut game, &rules, u32::MAX);
    let at = game.view().auth.units().get(unit).unwrap().core.tile;
    assert!(distance(&game, home, at) >= 2, "the scout stays at home");
    assert_eq!(driver.trace().total(Reason::Explore), 1);
}

/// What the scout at (20, 15) does on its way to (26, 15), with `refusals`
/// refused orders behind its task.
fn order_after(refusals: u8) -> Option<Command> {
    let rules = rules();
    let mut game = open_world(&rules);
    let unit = game.debug_spawn_unit(&rules, P0, tile(&game, 20, 15), scout(&rules));
    reveal_disc(&mut game, 20, 15, 6);
    let mut state = AiState::new(P0, 1);
    state.set_task(
        id_of(&game, unit),
        UnitTask::Scout {
            target: tile(&game, 26, 15),
        },
        1,
    );
    for _ in 0..refusals {
        state.note_failure(id_of(&game, unit));
    }
    let observation = game.ai_observation(&rules, P0);
    let brief = observe(&observation, &mut AiState::new(P0, 1), &rules);
    let (local, _) = game.ai_local_observation(&rules, P0, unit, 5).unwrap();
    let view = UnitView {
        local: &local,
        brief: &brief,
    };
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let (mut scratch, mut counters) = (MapScratch::default(), AiCounters::default());
    let mut tools = PlanTools {
        rules: &rules,
        settings: &settings,
        scratch: &mut scratch,
        counters: &mut counters,
    };
    decide_unit(&view, &mut state, &mut tools).command
}

#[test]
fn a_scout_whose_order_was_refused_walks_tile_by_tile() {
    let far = |command: Option<Command>| match command {
        Some(Command::MarchTo { .. }) => true,
        Some(Command::MoveUnit { .. }) => false,
        other => panic!("{other:?}"),
    };
    assert!(far(order_after(0)), "the first order covers the turn");
    assert!(
        !far(order_after(1)),
        "the tiles ahead are not in sight: one tile at a time"
    );
}
