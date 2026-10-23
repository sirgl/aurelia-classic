//! Movement rules (classic/docs/classic/rules.md, Standard speed):
//! terrain and feature costs, hills, the partial-move rule, one unit per
//! tile and class, and where a trained unit appears.

use super::support::*;
use crate::ids::FeatureId;
use crate::navigation::cost::{step_cost, StepCost};

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

#[test]
fn terrain_feature_and_hills_costs() {
    let (mut game, rules) = arena_content();
    let (flat, hills, forest, forest_hills) = (
        tile(&game, 5, 5),
        tile(&game, 6, 5),
        tile(&game, 6, 4),
        tile(&game, 5, 4),
    );
    let hill = crate::fixtures::HILLS;
    game.debug_set_terrain_batch(&rules, &[(hills, hill), (forest_hills, hill)]);
    game.debug_set_feature_batch(
        &rules,
        &[(forest, FeatureId(0)), (forest_hills, FeatureId(0))],
    );
    let from = tile(&game, 4, 5);
    let cost = |to| step_cost(game.view().auth, &rules, PlayerId(0), from, to);
    assert_eq!(cost(flat), Some(StepCost::Points(60)), "flat: 1 point");
    let from_flat = |to| step_cost(game.view().auth, &rules, PlayerId(0), flat, to);
    assert_eq!(
        from_flat(hills),
        Some(StepCost::Points(120)),
        "hills: 1 + 1"
    );
    assert_eq!(from_flat(forest), Some(StepCost::Points(120)), "forest: 2");
    assert_eq!(
        from_flat(forest_hills),
        Some(StepCost::Points(180)),
        "forest on hills: 2 + 1"
    );
    // A city on hills cancels the hills extra point.
    game.debug_found_city(&rules, PlayerId(0), hills);
    let from_flat = |to| step_cost(game.view().auth, &rules, PlayerId(0), flat, to);
    assert_eq!(from_flat(hills), Some(StepCost::Points(60)));
    // Water and mountains stay closed to land units (no embarkation yet).
    let water = tile(&game, 3, 3);
    game.debug_set_terrain_batch(
        &rules,
        &[(water, crate::fixtures::WATER)],
    );
    let near_water = tile(&game, 3, 4);
    assert_eq!(
        step_cost(game.view().auth, &rules, PlayerId(0), near_water, water),
        None
    );
}

/// Rules.md §7.3: a unit with any points left may enter a dearer tile; it
/// then has no points left.
#[test]
fn partial_move_enters_a_dearer_tile() {
    let (mut game, rules) = arena_content();
    let hills = tile(&game, 6, 5);
    game.debug_set_terrain_batch(
        &rules,
        &[(hills, crate::fixtures::HILLS)],
    );
    let warrior = game.debug_spawn_unit(&rules, PlayerId(0), tile(&game, 4, 5), UnitKindId(1));
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: warrior,
            destination: tile(&game, 5, 5),
        },
    )
    .unwrap();
    assert_eq!(
        game.view()
            .auth
            .units()
            .get(warrior)
            .unwrap()
            .core
            .moves_left,
        60
    );
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: warrior,
            destination: hills,
        },
    )
    .expect("60 points left are enough to step onto hills (120)");
    assert_eq!(
        game.view()
            .auth
            .units()
            .get(warrior)
            .unwrap()
            .core
            .moves_left,
        0
    );
}

/// Rules.md §8: one military and one civilian unit of a player per tile, and
/// no tile shared with another player's unit.
#[test]
fn one_unit_per_tile_and_class() {
    let (mut game, rules) = arena_content();
    let target = tile(&game, 6, 5);
    let first = game.debug_spawn_unit(&rules, PlayerId(0), tile(&game, 5, 5), UnitKindId(1));
    let second = game.debug_spawn_unit(&rules, PlayerId(0), tile(&game, 7, 5), UnitKindId(1));
    let worker = game.debug_spawn_unit(&rules, PlayerId(0), tile(&game, 6, 4), UnitKindId(2));
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: first,
            destination: target,
        },
    )
    .unwrap();
    let before = authoritative_hash(&game);
    let blocked = game.execute(
        &rules,
        Command::MoveUnit {
            unit: second,
            destination: target,
        },
    );
    assert!(blocked.is_err(), "two military units on one tile");
    assert_eq!(before, authoritative_hash(&game));
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: worker,
            destination: target,
        },
    )
    .expect("a civilian joins a military unit");
    // Another player's unit: its tile is no place to stop.
    let foreign = game.debug_spawn_unit(&rules, PlayerId(1), tile(&game, 8, 5), UnitKindId(1));
    let _ = foreign;
    let next_to_foreign = game.execute(
        &rules,
        Command::MoveUnit {
            unit: second,
            destination: tile(&game, 8, 5),
        },
    );
    assert!(
        next_to_foreign.is_err(),
        "no tile shared with another player"
    );
    game.validate(&rules).unwrap();
}

/// A unit trained in a city with a military unit already inside appears on
/// a free neighbour (the nearest valid plot).
#[test]
fn trained_unit_steps_out_of_a_full_city() {
    use crate::ids::ProductionItem;
    let (mut game, rules) = arena_content();
    let center = tile(&game, 8, 8);
    let city = game.debug_found_city(&rules, PlayerId(0), center);
    game.debug_spawn_unit(&rules, PlayerId(0), center, UnitKindId(1));
    let item = ProductionItem::Unit(UnitKindId(1));
    game.execute(
        &rules,
        Command::EnqueueProduction {
            player: PlayerId(0),
            city,
            item,
        },
    )
    .unwrap();
    for _ in 0..30 {
        if game.view().auth.units().len() > 1 {
            break;
        }
        play_round(&mut game, &rules);
    }
    let auth = game.view().auth;
    let tiles: Vec<TileId> = auth.units().iter_cores().map(|u| u.tile).collect();
    assert_eq!(tiles.len(), 2, "the warrior was trained");
    assert_eq!(tiles[0], center);
    assert!(
        auth.map().dimensions.adjacent(tiles[1], center),
        "{tiles:?}"
    );
    game.validate(&rules).unwrap();
}
