//! City founding: settlers only, ring claim, names.

use super::support::*;

#[test]
fn only_settlers_found_cities() {
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 4, r: 4 }).unwrap();
    let t1 = dims.to_tile(HexCoord { q: 6, r: 4 }).unwrap();
    let settler = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
    let warrior = game.debug_spawn_unit(&rules, PlayerId(0), t1, UnitKindId(1));
    // A warrior cannot found: an explicit error, the world is untouched.
    let before = authoritative_hash(&game);
    assert!(game
        .execute(&rules, Command::FoundCity { settler: warrior })
        .is_err());
    assert_eq!(before, authoritative_hash(&game));
    // A settler can.
    game.execute(&rules, Command::FoundCity { settler })
        .unwrap();
    assert_eq!(game.view().auth.cities().len(), 1);
    game.validate(&rules).unwrap();
}

#[test]
fn found_city_claims_ring() {
    use crate::math::tiles_in_hex_radius;
    use crate::derived::yields::CITY_TERRITORY_RADIUS;
    // The execute path: founding paints the territory ring (7 tiles at
    // radius 1, the city's work area, as in Civ).
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let settler = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
    game.execute(&rules, Command::FoundCity { settler })
        .unwrap();
    let ring = tiles_in_hex_radius(&dims, t0, CITY_TERRITORY_RADIUS);
    assert_eq!(ring.len(), 7, "radius 1 = center + 6 neighbors");
    for t in &ring {
        assert_eq!(
            game.view().auth.map().owner(*t).unwrap(),
            Some(PlayerId(0)),
            "ring tile {:?} claimed",
            t
        );
    }
    game.validate(&rules).unwrap();
    // The debug_found_city path (setup/generator): the same semantics.
    let (mut game2, rules2) = arena_content();
    let dims2 = game2.view().auth.map().dimensions;
    let c = dims2.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    game2.debug_found_city(&rules2, PlayerId(1), c);
    for t in tiles_in_hex_radius(&dims2, c, CITY_TERRITORY_RADIUS) {
        assert_eq!(
            game2.view().auth.map().owner(t).unwrap(),
            Some(PlayerId(1)),
            "debug path claims {:?}",
            t
        );
    }
    game2.validate(&rules2).unwrap();
}

#[test]
fn cities_are_named_in_order() {
    // The name at founding (world::city::next_city_name): the civilization's name list,
    // without a list "<civilization> Settlement" with a number; names are unique across the whole
    // game, not per owner. In the arena civilizations have no lists or names.
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 4, r: 4 }).unwrap();
    let t1 = dims.to_tile(HexCoord { q: 10, r: 10 }).unwrap();
    let c0 = game.debug_found_city(&rules, PlayerId(0), t0);
    let c1 = game.debug_found_city(&rules, PlayerId(0), t1);
    let t2 = dims.to_tile(HexCoord { q: 12, r: 4 }).unwrap();
    let c2 = game.debug_found_city(&rules, PlayerId(1), t2);
    let view = game.view();
    assert_eq!(view.auth.cities().get(c0).unwrap().name, "Settlement");
    assert_eq!(view.auth.cities().get(c1).unwrap().name, "Settlement 2");
    // Names are shared by all players: the second player does not repeat a taken one.
    assert_eq!(view.auth.cities().get(c2).unwrap().name, "Settlement 3");
    // The execute path names the same way.
    let (mut game2, rules2) = arena_content();
    let dims2 = game2.view().auth.map().dimensions;
    let s = dims2.to_tile(HexCoord { q: 6, r: 6 }).unwrap();
    let settler = game2.debug_spawn_unit(&rules2, PlayerId(0), s, UnitKindId(0));
    game2
        .execute(&rules2, Command::FoundCity { settler })
        .unwrap();
    let view2 = game2.view();
    let (_, core) = view2.auth.cities().iter().next().unwrap();
    assert_eq!(core.name, "Settlement");
}

/// Regression (P1): a second settler founded a city on a tile that already
/// had one — city_by_tile was overwritten and the first city dangled.
#[test]
fn second_city_on_the_same_tile_is_rejected() {
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let first = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
    let second = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
    game.execute(&rules, Command::FoundCity { settler: first })
        .unwrap();
    let before = authoritative_hash(&game);
    assert!(game
        .execute(&rules, Command::FoundCity { settler: second })
        .is_err());
    assert_eq!(before, authoritative_hash(&game));
    assert_eq!(game.view().auth.cities().len(), 1);
    game.validate(&rules).unwrap();
}

/// City founding: no city within the minimal city range (2) of another
/// city and none on foreign territory.
#[test]
fn cities_keep_their_distance_and_stay_off_foreign_land() {
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let tile = |q, r| dims.to_tile(HexCoord { q, r }).unwrap();
    game.debug_found_city(&rules, PlayerId(1), tile(8, 8));
    for (q, r) in [(8, 9), (10, 8)] {
        let settler = game.debug_spawn_unit(&rules, PlayerId(0), tile(q, r), UnitKindId(0));
        let result = game.execute(&rules, Command::FoundCity { settler });
        assert!(result.is_err(), "({q},{r}) is too close to a city");
    }
    let far = game.debug_spawn_unit(&rules, PlayerId(0), tile(11, 8), UnitKindId(0));
    game.execute(&rules, Command::FoundCity { settler: far })
        .expect("three tiles away is allowed");
    game.validate(&rules).unwrap();
}
