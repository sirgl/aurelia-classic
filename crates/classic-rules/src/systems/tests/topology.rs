//! One hex topology for walking, combat and sight (regressions: A* walked
//! in four square directions, combat and sight used square neighbourhoods).

use super::support::*;
use crate::navigation::{find_path, PathRequest, PathScratch};

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

#[test]
fn path_walks_the_six_hex_directions() {
    let (game, rules) = arena();
    let dims = game.view().auth.map().dimensions;
    let (from, to) = (tile(&game, 2, 8), tile(&game, 6, 4));
    let path = find_path(
        game.view(),
        &rules,
        PathRequest {
            from,
            to,
            player: PlayerId(0),
            budget: crate::navigation::MoveBudget::flat(600, 600),
            profile: crate::navigation::MoveProfile::LAND,
        },
        &mut PathScratch::default(),
    )
    .expect("open field path");
    // Hex distance 4 along the (1,-1) direction: four steps, no zig-zag.
    assert_eq!(path.tiles.len(), 5, "{:?}", path.tiles);
    for pair in path.tiles.windows(2) {
        assert!(dims.adjacent(pair[0], pair[1]), "{:?}", path.tiles);
    }
}

#[test]
fn melee_reaches_only_hex_neighbours() {
    let (mut game, rules) = arena();
    game.execute(
        &rules,
        Command::DeclareWar {
            player: PlayerId(0),
            enemy: PlayerId(1),
        },
    )
    .unwrap();
    let attacker = game.debug_spawn_unit(&rules, PlayerId(0), tile(&game, 5, 5), UnitKindId(0));
    // (6,6) is a square-grid diagonal: two hexes away.
    let far = game.debug_spawn_unit(&rules, PlayerId(1), tile(&game, 6, 6), UnitKindId(0));
    let err = game.execute(
        &rules,
        Command::AttackUnit {
            attacker,
            defender: far,
        },
    );
    assert!(err.is_err(), "a unit two hexes away is out of melee range");
    // (6,4) is the hex neighbour in direction (1,-1).
    let near = game.debug_spawn_unit(&rules, PlayerId(1), tile(&game, 6, 4), UnitKindId(0));
    game.execute(
        &rules,
        Command::AttackUnit {
            attacker,
            defender: near,
        },
    )
    .expect("hex neighbour is in melee range");
}

#[test]
fn sight_radius_is_a_hex_radius() {
    let (mut game, rules) = arena();
    let at = tile(&game, 6, 6);
    game.debug_spawn_unit(&rules, PlayerId(0), at, UnitKindId(0));
    let dims = game.view().auth.map().dimensions;
    let tiles = dims.tile_count();
    let counts = &game.derived().visibility.counts;
    for t in 0..tiles as u32 {
        let distance = dims.distance(at, TileId(t)).unwrap();
        let seen = counts[t as usize] > 0;
        assert_eq!(
            seen,
            distance <= crate::derived::visibility::SIGHT_RADIUS,
            "tile {t} at distance {distance}"
        );
    }
}
