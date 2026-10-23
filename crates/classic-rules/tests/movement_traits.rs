//! Civilization traits that change how units walk (catalog `civ.*`
//! movement traits): the Inca climb hills for one point, the Iroquois treat
//! the forests and jungles of friendly land as roads, Carthage crosses
//! mountains once it has earned a Great General. Each rule is checked in the
//! core (`MoveUnit`) and as the AI prices it (`planner_map::step_points`,
//! the same `step_price` over the same facts), and the oracle
//! (`GameState::validate`) stays quiet.

use classic_content::core::core_rules;
use classic_rules::ai::planner_map::{step_points, KnownMap};
use classic_rules::ids::{FeatureId, PlayerId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::navigation::cost::{step_cost_for, StepCost};
use classic_rules::navigation::MoveProfile;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const PLAINS: TerrainId = TerrainId(1);
const HILLS: TerrainId = TerrainId(2);
const MOUNTAIN: TerrainId = TerrainId(6);
/// A whole move of a land unit (a warrior has two).
const MOVE: u16 = classic_rules::world::unit::MOVE_DENOMINATOR;
/// The road step of the core (`ROAD_MOVE_POINTS`, in move points).
const ROAD: u16 = 30;

fn setup() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 20, height: 12, rng_seed: 7 });
    let count = 20 * 12;
    let flat: Vec<(TileId, TerrainId)> = (0..count).map(|i| (TileId(i), PLAINS)).collect();
    game.debug_set_terrain_batch(&rules, &flat);
    (game, rules)
}

fn tile(q: i32, r: i32) -> TileId {
    classic_rules::math::MapDimensions { width: 20, height: 12 }.to_tile(HexCoord { q, r }).unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn civ(rules: &CompiledRuleset, key: &str) -> u8 {
    rules.civs.iter().position(|c| c.key == key).unwrap_or_else(|| panic!("{key}")) as u8
}

fn feature(rules: &CompiledRuleset, key: &str) -> FeatureId {
    FeatureId(rules.features.iter().position(|f| f.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn moves_left(game: &GameState, unit: UnitHandle) -> u16 {
    game.view().auth.units().get(unit).unwrap().core.moves_left
}

/// The moves a fresh warrior of `civ_key` has left after one step from
/// `from` to `to` on the prepared map (`prepare` sets the world up).
fn warrior_step(civ_key: &str, prepare: impl Fn(&mut GameState, &CompiledRuleset), from: TileId, to: TileId) -> u16 {
    let (mut game, rules) = setup();
    prepare(&mut game, &rules);
    game.debug_set_player_civ(P0, civ(&rules, civ_key));
    game.debug_sources_changed(&rules, P0);
    let warrior = game.debug_spawn_unit(&rules, P0, from, unit(&rules, "core:unit/warrior"));
    game.execute(&rules, Command::MoveUnit { unit: warrior, destination: to }).unwrap();
    assert_eq!(game.view().auth.units().get(warrior).unwrap().core.tile, to);
    game.validate(&rules).unwrap();
    moves_left(&game, warrior)
}

/// The Inca (Strength of Stone): a step onto hills takes one point, not all
/// the moves; flat ground is the same for everyone.
#[test]
fn the_inca_climb_hills_for_one_point() {
    let hills = |game: &mut GameState, rules: &CompiledRuleset| game.debug_set_terrain_batch(rules, &[(tile(6, 5), HILLS)]);
    assert_eq!(warrior_step("core:nation/america", hills, tile(5, 5), tile(6, 5)), 0, "hills take a warrior's two moves");
    assert_eq!(warrior_step("core:nation/inca", hills, tile(5, 5), tile(6, 5)), MOVE, "one point of two");
    let flat = |_: &mut GameState, _: &CompiledRuleset| {};
    assert_eq!(
        warrior_step("core:nation/inca", flat, tile(5, 5), tile(6, 5)),
        warrior_step("core:nation/america", flat, tile(5, 5), tile(6, 5)),
        "flat ground is the same",
    );
}

/// The Iroquois (Great Warpath): a step between two tiles of friendly land,
/// each a road or a forest or jungle, costs a road's price. In foreign or
/// unowned land the woods are woods.
#[test]
fn the_iroquois_walk_friendly_woods_as_roads() {
    let woods_at = |q: i32, key: &'static str| {
        move |game: &mut GameState, rules: &CompiledRuleset| {
            game.debug_found_city(rules, P0, tile(5, 5));
            game.debug_set_feature_batch(rules, &[(tile(q, 5), feature(rules, key))]);
        }
    };
    // From the capital onto a forest and a jungle of its own land.
    for key in ["core:feature/forest", "core:feature/jungle"] {
        assert_eq!(warrior_step("core:nation/iroquois", woods_at(6, key), tile(5, 5), tile(6, 5)), 2 * MOVE - ROAD, "{key}");
        assert!(warrior_step("core:nation/america", woods_at(6, key), tile(5, 5), tile(6, 5)) < MOVE, "{key}: others pay in full");
    }
    // Woods nobody owns: no road for the Iroquois.
    let far = |game: &mut GameState, rules: &CompiledRuleset| {
        game.debug_found_city(rules, P0, tile(2, 2));
        game.debug_set_feature_batch(rules, &[(tile(12, 5), feature(rules, "core:feature/forest"))]);
    };
    assert_eq!(
        warrior_step("core:nation/iroquois", far, tile(11, 5), tile(12, 5)),
        warrior_step("core:nation/america", far, tile(11, 5), tile(12, 5)),
        "unowned woods",
    );
    // Woods of a stranger's land.
    let foreign = |game: &mut GameState, rules: &CompiledRuleset| {
        game.debug_found_city(rules, P1, tile(12, 5));
        // At war the borders are open to the army.
        game.debug_meet(P0, P1);
        game.debug_force_war(rules, P0, P1);
        game.debug_set_feature_batch(rules, &[(tile(11, 5), feature(rules, "core:feature/forest"))]);
    };
    assert_eq!(
        warrior_step("core:nation/iroquois", foreign, tile(10, 5), tile(11, 5)),
        warrior_step("core:nation/america", foreign, tile(10, 5), tile(11, 5)),
        "foreign woods",
    );
}

/// Carthage crosses mountains only after a Great General is born; the
/// others never do. The first step onto a mountain takes all the moves.
#[test]
fn carthage_crosses_mountains_after_a_great_general() {
    let (mut game, rules) = setup();
    game.debug_set_terrain_batch(&rules, &[(tile(6, 5), MOUNTAIN)]);
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/carthage"));
    game.debug_sources_changed(&rules, P0);
    let warrior = game.debug_spawn_unit(&rules, P0, tile(5, 5), unit(&rules, "core:unit/warrior"));
    let climb = |game: &mut GameState| game.execute(&rules, Command::MoveUnit { unit: warrior, destination: tile(6, 5) });
    assert!(climb(&mut game).is_err(), "no general yet: the mountain is closed");
    game.debug_earn_great_general(P0, &rules);
    climb(&mut game).unwrap();
    assert_eq!(game.view().auth.units().get(warrior).unwrap().core.tile, tile(6, 5));
    game.validate(&rules).unwrap();
    // Another civilization with a general still cannot.
    let (mut game, rules) = setup();
    game.debug_set_terrain_batch(&rules, &[(tile(6, 5), MOUNTAIN)]);
    let warrior = game.debug_spawn_unit(&rules, P0, tile(5, 5), unit(&rules, "core:unit/warrior"));
    game.debug_earn_great_general(P0, &rules);
    assert!(game.execute(&rules, Command::MoveUnit { unit: warrior, destination: tile(6, 5) }).is_err());
}

fn hp(game: &GameState, unit: UnitHandle) -> u16 {
    game.view().auth.units().get(unit).unwrap().core.hp
}

/// Phoenician Heritage: a unit that may cross mountains and ends its owner's
/// turn on one loses 50 hit points, but never dies of it (it is left with
/// one). Without the ability (no general yet, or another civilization) the
/// mountain does not hurt.
#[test]
fn ending_the_turn_on_a_mountain_hurts_a_unit_that_may_cross_them() {
    let (mut game, rules) = setup();
    game.debug_set_terrain_batch(&rules, &[(tile(6, 5), MOUNTAIN)]);
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/carthage"));
    game.debug_sources_changed(&rules, P0);
    let warrior = unit(&rules, "core:unit/warrior");
    let healthy = game.debug_spawn_unit(&rules, P0, tile(5, 5), warrior);
    let weak = game.debug_spawn_unit(&rules, P0, tile(5, 6), warrior);
    game.debug_set_unit_hp(weak, 30);
    game.debug_earn_great_general(P0, &rules);
    let climb = |game: &mut GameState, unit: UnitHandle| {
        game.execute(&rules, Command::MoveUnit { unit, destination: tile(6, 5) }).unwrap()
    };
    climb(&mut game, healthy);
    game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
    assert_eq!(hp(&game, healthy), 50, "a full unit loses 50");
    assert_eq!(hp(&game, weak), 40, "the unit on the plain only rests and heals");

    let weak_on_mountain = game.debug_spawn_unit(&rules, P0, tile(6, 5), warrior);
    game.debug_set_unit_hp(weak_on_mountain, 30);
    while game.view().auth.active_player() != P0 {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
    }
    game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
    assert_eq!(hp(&game, weak_on_mountain), 1, "it is never killed by the mountain");
    game.validate(&rules).unwrap();

    // Another civilization with a general: the unit is just not there to be hurt.
    let (mut game, rules) = setup();
    game.debug_set_terrain_batch(&rules, &[(tile(6, 5), MOUNTAIN)]);
    game.debug_earn_great_general(P0, &rules);
    let stranger = game.debug_spawn_unit(&rules, P0, tile(6, 5), unit(&rules, "core:unit/warrior"));
    game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
    assert_eq!(hp(&game, stranger), 100);
}

/// The AI prices the steps of these civilizations as the core walks them:
/// on a random map of hills, mountains, woods, roads and two empires, for
/// the Inca, the Iroquois and Carthage with and without a general.
#[test]
fn the_ai_view_prices_trait_steps_as_the_core_walks() {
    let mut seed = 0xC17A_0B5Eu64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut cheaper = 0;
    for round in 0..9 {
        let (mut game, rules) = setup();
        let count = 20 * 12;
        let terrain: Vec<(TileId, TerrainId)> = (0..count)
            .map(|i| {
                let t = match rnd() % 6 {
                    0 => MOUNTAIN,
                    1 | 2 => HILLS,
                    _ => PLAINS,
                };
                (TileId(i), t)
            })
            .collect();
        game.debug_set_terrain_batch(&rules, &terrain);
        let mut woods = Vec::new();
        for (tile, t) in &terrain {
            if *t != MOUNTAIN && rnd() % 3 == 0 {
                let key = if rnd() % 2 == 0 { "core:feature/forest" } else { "core:feature/jungle" };
                woods.push((*tile, feature(&rules, key)));
            }
        }
        game.debug_set_feature_batch(&rules, &woods);
        let roads: Vec<TileId> = (0..count).filter(|_| rnd() % 6 == 0).map(TileId).collect();
        game.debug_set_road_batch(&rules, &roads);
        game.debug_found_city(&rules, P0, tile(5, 5));
        game.debug_found_city(&rules, P0, tile(14, 7));
        game.debug_found_city(&rules, P1, tile(10, 2));
        let key = ["core:nation/inca", "core:nation/iroquois", "core:nation/carthage"][round % 3];
        game.debug_set_player_civ(P0, civ(&rules, key));
        if round >= 3 {
            game.debug_earn_great_general(P0, &rules);
        }
        let all: Vec<TileId> = (0..count).map(TileId).collect();
        game.debug_reveal(P0, &all);
        let auth = game.view().auth;
        let view = game.player_view(&rules, P0);
        let profile = MoveProfile::of_kind(auth, &rules, P0, unit(&rules, "core:unit/warrior"));
        let plain = MoveProfile::of_kind(auth, &rules, P1, unit(&rules, "core:unit/warrior"));
        for from in 0..count {
            let from = TileId(from);
            for to in auth.map().dimensions.neighbors(from) {
                let core = step_cost_for(auth, &rules, P0, &profile, from, to).map(|cost| match cost {
                    StepCost::Points(points) => points,
                    StepCost::AllMoves => 120,
                });
                let ai = step_points(&KnownMap { view: &view, profile }, from, to);
                assert_eq!(ai, core, "round {round}, {key}: {from:?} -> {to:?}");
                let other = step_cost_for(auth, &rules, P1, &plain, from, to).map(|cost| match cost {
                    StepCost::Points(points) => points,
                    StepCost::AllMoves => 120,
                });
                if core.is_some() && (other.is_none() || core < other) {
                    cheaper += 1;
                }
            }
        }
    }
    assert!(cheaper > 100, "the trait changes the prices: {cheaper} cheaper steps");
}
