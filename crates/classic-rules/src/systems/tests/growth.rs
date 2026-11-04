//! Growth: food surplus adds citizens.

use super::support::*;

#[test]
fn city_grows_from_food_surplus() {
    use crate::ids::TerrainId;
    // Grassland 2F: the centre and the best tile (pop 1) give 4 food, the
    // citizen eats 2 → +2 a turn. growth_threshold(1) = 15: the store holds
    // 14 after 7 turns and 16 after the 8th → size 2 with 16 − 15 = 1 left.
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let ring = crate::math::tiles_in_hex_radius(
        &dims,
        t0,
        crate::world::city::CITY_WORK_RADIUS,
    );
    let grass: Vec<_> = ring.iter().map(|t| (*t, TerrainId(1))).collect();
    game.debug_set_terrain_batch(&rules, &grass);
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    let view = game.view();
    let core = view.auth.cities().get(city).unwrap();
    assert_eq!((core.population, core.food_times100), (1, 200));
    for _ in 0..7 {
        game.execute(
            &rules,
            Command::EndTurn {
                player: PlayerId(0),
            },
        )
        .unwrap();
    }
    let view = game.view();
    let core = view.auth.cities().get(city).unwrap();
    assert_eq!((core.population, core.food_times100), (2, 100));
    // Oracle: growth triggers PopulationChanged -> yields converged.
    game.validate(&rules).unwrap();
}

/// «Avoid Growth»: the store fills to the threshold and stays there; the
/// city keeps its size until the order is lifted, then grows at once.
#[test]
fn a_city_that_avoids_growth_keeps_its_size() {
    use crate::ids::TerrainId;
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let ring = crate::math::tiles_in_hex_radius(&dims, t0, crate::world::city::CITY_WORK_RADIUS);
    let grass: Vec<_> = ring.iter().map(|t| (*t, TerrainId(1))).collect();
    game.debug_set_terrain_batch(&rules, &grass);
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    let player = PlayerId(0);
    game.execute(&rules, Command::SetAvoidGrowth { player, city, avoid: true }).unwrap();
    assert!(game.execute(&rules, Command::SetAvoidGrowth { player, city, avoid: true }).is_err(), "already set");
    for _ in 0..12 {
        game.execute(&rules, Command::EndTurn { player }).unwrap();
    }
    let core = game.view().auth.cities().get(city).unwrap();
    assert_eq!((core.population, core.food_times100), (1, 1500), "full store, no growth");
    game.execute(&rules, Command::SetAvoidGrowth { player, city, avoid: false }).unwrap();
    game.execute(&rules, Command::EndTurn { player }).unwrap();
    let core = game.view().auth.cities().get(city).unwrap();
    assert_eq!(core.population, 2, "the full store grows the city once it may");
    game.validate(&rules).unwrap();
}
