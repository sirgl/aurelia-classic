//! Worked tiles: automatic choice and manual toggle.

use super::support::*;

#[test]
fn worked_tiles_pick_best_by_focus_weights() {
    use crate::ids::TerrainId;
    // Centre plains, three rings of desert, one grass (2F) at (9,8):
    // pop 1 -> {centre, grass}.
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let center = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let grass = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    let ring = crate::math::tiles_in_hex_radius(
        &dims,
        center,
        crate::world::city::CITY_WORK_RADIUS,
    );
    let desert: Vec<_> = ring
        .iter()
        .filter(|t| **t != center && **t != grass)
        .map(|t| (*t, TerrainId(3)))
        .collect();
    game.debug_set_terrain_batch(&rules, &desert);
    game.debug_set_terrain_batch(&rules, &[(grass, TerrainId(1))]);
    let city = game.debug_found_city(&rules, PlayerId(0), center);
    let mut got = worked_set_tiles(&game, &rules, city);
    got.sort_by_key(|t| t.0);
    let mut expect = vec![center, grass];
    expect.sort_by_key(|t| t.0);
    assert_eq!(got, expect);
}

#[test]
fn toggle_worked_tile_excludes_and_restores() {
    use crate::ids::TerrainId;
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let center = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let grass = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    let ring = crate::math::tiles_in_hex_radius(
        &dims,
        center,
        crate::world::city::CITY_WORK_RADIUS,
    );
    let desert: Vec<_> = ring
        .iter()
        .filter(|t| **t != center && **t != grass)
        .map(|t| (*t, TerrainId(3)))
        .collect();
    game.debug_set_terrain_batch(&rules, &desert);
    game.debug_set_terrain_batch(&rules, &[(grass, TerrainId(1))]);
    let city = game.debug_found_city(&rules, PlayerId(0), center);
    let y0 = game.derived().yields.of(city).unwrap().food;
    // Took the grass off: the desert with the smaller id ((8,7)=120) works, food is less
    // by exactly 2 (grass 2F, desert 0).
    game.execute(
        &rules,
        Command::ToggleWorkedTile {
            player: PlayerId(0),
            city,
            tile: grass,
        },
    )
    .unwrap();
    let desert_best = dims.to_tile(HexCoord { q: 8, r: 7 }).unwrap();
    let mut got = worked_set_tiles(&game, &rules, city);
    got.sort_by_key(|t| t.0);
    let mut expect = vec![center, desert_best];
    expect.sort_by_key(|t| t.0);
    assert_eq!(got, expect);
    assert_eq!(
        game.derived().yields.of(city).unwrap().food,
        y0 - 2,
        "incremental follows toggle"
    );
    game.validate(&rules).unwrap();
    // Returned: grass is in the set again, the yield is restored.
    game.execute(
        &rules,
        Command::ToggleWorkedTile {
            player: PlayerId(0),
            city,
            tile: grass,
        },
    )
    .unwrap();
    assert_eq!(game.derived().yields.of(city).unwrap().food, y0);
    game.validate(&rules).unwrap();
    // The centre cannot be taken off; a foreign ring cannot; a foreign city cannot.
    assert!(game
        .execute(
            &rules,
            Command::ToggleWorkedTile {
                player: PlayerId(0),
                city,
                tile: center,
            },
        )
        .is_err());
    let far = dims.to_tile(HexCoord { q: 2, r: 2 }).unwrap();
    assert!(game
        .execute(
            &rules,
            Command::ToggleWorkedTile {
                player: PlayerId(0),
                city,
                tile: far,
            },
        )
        .is_err());
    assert!(game
        .execute(
            &rules,
            Command::ToggleWorkedTile {
                player: PlayerId(1),
                city,
                tile: grass,
            },
        )
        .is_err());
}

/// Regression (P2): a city worked the best tiles of its ring without
/// checking who owns them — a neighbour's tile could be worked by both.
/// A city works only the tiles that belong to it.
#[test]
fn cities_work_only_their_own_tiles() {
    use crate::ids::TerrainId;
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let tile = |q, r| dims.to_tile(HexCoord { q, r }).unwrap();
    // Desert everywhere around, one grassland tile between two cities.
    let around: Vec<_> = crate::math::tiles_in_hex_radius(&dims, tile(9, 8), 3)
        .into_iter()
        .map(|t| (t, TerrainId(3)))
        .collect();
    game.debug_set_terrain_batch(&rules, &around);
    game.debug_set_terrain_batch(&rules, &[(tile(9, 8), TerrainId(1))]);
    let west = game.debug_found_city(&rules, PlayerId(0), tile(8, 8));
    let east = game.debug_found_city(&rules, PlayerId(1), tile(10, 8));
    let auth = game.view().auth;
    let mut worked_by = Vec::new();
    for city in [west, east] {
        let core = auth.cities().get(city).unwrap();
        for t in crate::derived::yields::city_work(auth, &rules, core).tiles() {
            assert_eq!(
                auth.map().owner(t).unwrap(),
                Some(core.owner),
                "{city:?} works {t:?} owned by someone else"
            );
            worked_by.push(t);
        }
    }
    let grass = worked_by.iter().filter(|t| **t == tile(9, 8)).count();
    assert!(grass <= 1, "one tile is worked by at most one city");
    // The east city cannot take the west city's tile by hand either.
    play_until_active(&mut game, &rules, PlayerId(1));
    let toggle = game.execute(
        &rules,
        Command::ToggleWorkedTile {
            player: PlayerId(1),
            city: east,
            tile: tile(9, 8),
        },
    );
    assert!(toggle.is_err());
}
