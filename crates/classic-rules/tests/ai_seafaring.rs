//! The AI crosses water (classic/docs/QA.md): its paths used the plain land step
//! for every unit, so no AI unit ever embarked or sailed. On archipelago
//! maps nobody met anybody in 300 rounds, on continents nobody met the
//! other continent: no World Congress, no diplomatic victory, no wars or
//! cities across the sea.

use classic_content::core::core_rules;
use classic_rules::ai::planner_map::{step_points, KnownMap};
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::ids::{PlayerId, TechId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::navigation::MoveProfile;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
/// Map code of coast (the generator's water).
const COAST: TerrainId = TerrainId(4);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap() as u16)
}

/// Two shores with a strait of `width` water columns from q = 12.
fn strait(width: i32) -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 30,
            height: 20,
            rng_seed: 7,
        },
    );
    let mut water = Vec::new();
    for r in 0..20 {
        for q in 12..12 + width {
            water.push((tile(&game, q, r), COAST));
        }
    }
    game.debug_set_terrain_batch(&rules, &water);
    (game, rules)
}

/// Every tile of the map known to P0 (the step rule reads known tiles).
fn reveal_all(game: &mut GameState) {
    let tiles: Vec<TileId> = (0..game.view().auth.map().tile_count() as u32).map(TileId).collect();
    game.debug_reveal(P0, &tiles);
}

#[test]
fn units_embark_onto_shallow_water_with_optics_and_cross_the_ocean_with_astronomy() {
    let (mut game, rules) = strait(3);
    reveal_all(&mut game);
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 11, 5), unit(&rules, "core:unit/warrior"));
    let (shore, shallow, deep) = (tile(&game, 11, 5), tile(&game, 12, 5), tile(&game, 13, 5));
    let points = |game: &GameState, from, to| {
        let view = game.player_view(&rules, P0);
        step_points(&KnownMap::of_unit(&view, warrior), from, to)
    };
    assert_eq!(points(&game, shore, shallow), None, "no embarking before Optics");
    let view = game.player_view(&rules, P0);
    assert_eq!(step_points(&KnownMap::land(&view), shore, shallow), None, "plain land never embarks");
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    assert!(points(&game, shore, shallow).is_some(), "Optics: shallow water");
    assert_eq!(points(&game, shallow, deep), None, "deep water needs Astronomy");
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/astronomy"));
    assert!(points(&game, shallow, deep).is_some(), "Astronomy: every water");
    let profile = game.player_view(&rules, P0).own_unit_profile(warrior).unwrap();
    assert_ne!(profile, MoveProfile::LAND);
}

/// A scout on its shore with Optics explores over the strait and meets
/// the nation on the other side.
#[test]
fn an_ai_scout_crosses_a_strait_and_meets_the_other_shore() {
    let (mut game, rules) = strait(2);
    game.debug_found_city(&rules, P0, tile(&game, 5, 10));
    game.debug_found_city(&rules, P1, tile(&game, 20, 10));
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    game.debug_spawn_unit(&rules, P0, tile(&game, 6, 10), unit(&rules, "core:unit/scout"));
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), vec![P1]);
    let met = |game: &GameState| game.view().auth.knowledge()[0].contact(P1).met_round.is_some();
    for _ in 0..40 {
        if met(&game) {
            break;
        }
        driver.run(&mut game, &rules, u32::MAX);
        if game.view().auth.active_player() == P1 {
            game.execute(&rules, Command::EndTurn { player: P1 }).unwrap();
        }
    }
    assert!(met(&game), "P0 met P1 across the water");
    game.validate(&rules).unwrap();
}

/// A nation that knows little of the map after its early scouting
/// (an island) researches Optics.
#[test]
fn an_island_ai_researches_optics_first() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 30,
            height: 20,
            rng_seed: 7,
        },
    );
    let island = |q: i32, r: i32| (3..=7).contains(&q) && (8..=12).contains(&r);
    let water: Vec<(TileId, TerrainId)> = (0..20)
        .flat_map(|r| (0..30).map(move |q| (q, r)))
        .filter(|(q, r)| !island(*q, *r) && *q < 24)
        .map(|(q, r)| (tile(&game, q, r), COAST))
        .collect();
    game.debug_set_terrain_batch(&rules, &water);
    game.debug_found_city(&rules, P0, tile(&game, 5, 10));
    game.debug_found_city(&rules, P1, tile(&game, 27, 10));
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/sailing"));
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), vec![P1]);
    let optics = tech(&rules, "core:tech/optics");
    let researching = |game: &GameState| {
        let players = game.view().auth.players();
        players.techs_of(P0).contains(&optics) || players.research_queue_of(P0).first() == Some(&optics)
    };
    // The choice is made when the research queue empties after round 40; which
    // technology is under way then depends on the policies adopted, so the
    // bound is generous (200 turns) instead of the moment one build happened to hit.
    for _ in 0..200 {
        if game.view().auth.turn().round > 40 && researching(&game) {
            break;
        }
        driver.run(&mut game, &rules, u32::MAX);
        if game.view().auth.active_player() == P1 {
            game.execute(&rules, Command::EndTurn { player: P1 }).unwrap();
        }
    }
    assert!(researching(&game), "Optics once the island is all P0 knows");
}
