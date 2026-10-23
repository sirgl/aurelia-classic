//! Culture: border growth one level (one tile) at a time, the classic
//! thresholds, culture from buildings only (cities make no culture per
//! citizen).

use super::support::*;
use crate::ids::{BuildingKindId, TerrainId};

/// Arena units plus a Monument (+2 culture) and a Temple of culture (+40).
const CULTURE_RON: &str = r#"(
    units: [
        (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
        (id: "core:unit/warrior", movement: 2, strength: 10, cost: 30),
    ],
    buildings: [
        (id: "core:building/monument", cost: 40, effects: [(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Culture, amount: 2))]),
        (id: "core:building/opera", cost: 40, effects: [(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Culture, amount: 40))]),
    ],
    resources: [
        (id: "core:resource/wheat", food_bonus: 2, terrains: [0, 1], density: 8),
    ],
)"#;

const MONUMENT: BuildingKindId = BuildingKindId(0);
const OPERA: BuildingKindId = BuildingKindId(1);

fn culture_world() -> (GameState, crate::CompiledRuleset, TileId) {
    let rules = crate::fixtures::compile_ron(CULTURE_RON);
    let mut game = GameState::new(
        &rules,
        crate::GameConfig {
            players: 1,
            width: 16,
            height: 16,
            rng_seed: 7,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let ring = crate::math::tiles_in_hex_radius(&dims, t0, 3);
    let grass: Vec<_> = ring.iter().map(|t| (*t, TerrainId(1))).collect();
    game.debug_set_terrain_batch(&rules, &grass);
    (game, rules, t0)
}

fn owned(game: &GameState) -> usize {
    let view = game.view();
    (0..view.auth.map().tile_count())
        .filter(|i| view.auth.map().owner(TileId(*i as u32)).unwrap_or(None) == Some(PlayerId(0)))
        .count()
}

fn end_turn(game: &mut GameState, rules: &crate::CompiledRuleset) {
    game.execute(
        rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
}

#[test]
fn a_city_without_culture_buildings_never_grows_its_border() {
    let (mut game, rules, t0) = culture_world();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    for _ in 0..30 {
        end_turn(&mut game, &rules);
    }
    let core = game.view().auth.cities().get(city).unwrap();
    assert_eq!(core.culture, 0, "no culture per citizen in Civ V");
    assert_eq!(core.acquired, 0);
    assert_eq!(owned(&game), 7, "the founding ring only");
}

#[test]
fn culture_acquires_single_tiles() {
    // Monument: 2 culture a turn. Level 0 costs 10 (Unciv:
    // CityExpansionManager.getCultureToNextTile): the 5th turn pays it, one
    // tile (ring 2) joins the city and 10 - 10 = 0 culture is left.
    let (mut game, rules, t0) = culture_world();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    game.debug_add_building(&rules, city, MONUMENT);
    assert_eq!(owned(&game), 7, "founding claims ring 1");
    for turn in 1..=5 {
        end_turn(&mut game, &rules);
        let core = game.view().auth.cities().get(city).unwrap();
        if turn < 5 {
            assert_eq!(core.acquired, 0, "turn {turn}: below the threshold");
        }
    }
    let core = game.view().auth.cities().get(city).unwrap();
    assert_eq!(core.acquired, 1, "exactly one level");
    assert_eq!(core.culture, 0, "10 culture - 10 spent");
    assert_eq!(core.radius, 2, "acquired tile is ring 2");
    assert_eq!(owned(&game), 8, "7 founding + 1 cultural");
    game.validate(&rules).unwrap();
}

#[test]
fn at_most_one_level_per_turn() {
    // 40 culture a turn covers levels 0 (10) and 1 (20) at once, but the
    // border grows one level per turn (rules.md); the rest is kept.
    let (mut game, rules, t0) = culture_world();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    game.debug_add_building(&rules, city, OPERA);
    end_turn(&mut game, &rules);
    let core = game.view().auth.cities().get(city).unwrap();
    assert_eq!(core.acquired, 1);
    assert_eq!(core.culture, 40 - 10);
    end_turn(&mut game, &rules);
    let core = game.view().auth.cities().get(city).unwrap();
    assert_eq!(core.acquired, 2);
    assert_eq!(core.culture, 30 + 40 - 20);
    assert_eq!(owned(&game), 9);
    game.validate(&rules).unwrap();
}

#[test]
fn the_city_culture_also_fills_the_player_pool() {
    let (mut game, rules, t0) = culture_world();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    game.debug_add_building(&rules, city, MONUMENT);
    for _ in 0..3 {
        end_turn(&mut game, &rules);
    }
    let pool = game.view().auth.module().culture().of(PlayerId(0));
    assert_eq!(pool.culture, 6, "3 turns × 2 culture");
    assert_eq!(pool.lifetime, 6);
    assert_eq!(pool.recent, vec![2, 2, 2]);
    assert_eq!(pool.max_cities, 1);
}

#[test]
fn culture_prefers_resource_tile() {
    use crate::ids::ResourceId;
    // Priority: a tile with a resource is taken before the other free
    // tiles of ring 2.
    let (mut game, rules, t0) = culture_world();
    let dims = game.view().auth.map().dimensions;
    let ring2: Vec<_> = crate::math::tiles_in_hex_radius(&dims, t0, 2)
        .into_iter()
        .filter(|t| {
            dims.to_coord(*t)
                .zip(dims.to_coord(t0))
                .map(|(a, b)| a.distance(b) == 2)
                .unwrap_or(false)
        })
        .collect();
    assert!(ring2.len() >= 6, "ring 2 exists around center");
    let res_tile = ring2[0];
    game.debug_set_resource_batch(&rules, &[(res_tile, ResourceId(0))]);
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    game.debug_add_building(&rules, city, OPERA);
    end_turn(&mut game, &rules);
    assert_eq!(
        game.view().auth.map().owner(res_tile),
        Some(Some(PlayerId(0))),
        "resource tile must win the priority"
    );
    game.validate(&rules).unwrap();
}
