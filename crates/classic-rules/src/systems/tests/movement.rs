//! Movement: march steps, borders, roads and rivers.

use super::support::*;

/// Multi-turn move order: MarchTo walks as far as the moves go and keeps
/// the goal; at the start of each of the owner's turns the unit walks on,
/// with no further command, until it arrives.
#[test]
fn march_order_continues_each_turn_until_arrival() {
    let (mut game, rules) = arena();
    let dims = game.view().auth.map().dimensions;
    let t = |x: i32| dims.to_tile(HexCoord { q: x, r: 4 }).unwrap();
    let u = game.debug_spawn_unit(&rules, PlayerId(0), t(4), UnitKindId(0));
    game.execute(&rules, Command::MarchTo { unit: u, destination: t(10) })
        .unwrap();
    let state = |game: &GameState| {
        let core = game.view().auth.units().get(u).unwrap().core;
        (core.tile, core.goal)
    };
    assert_eq!(state(&game), (t(6), Some(t(10))), "two flat steps on the first turn");
    play_round(&mut game, &rules);
    assert_eq!(state(&game), (t(8), Some(t(10))), "the order walks on at turn start");
    play_round(&mut game, &rules);
    assert_eq!(state(&game), (t(10), None), "arrived: the order is done");
    game.validate(&rules).unwrap();
}

/// The order lives in the saved state: a loaded game walks on the same way.
#[test]
fn march_order_survives_save_and_load() {
    let (mut game, rules) = arena();
    let dims = game.view().auth.map().dimensions;
    let t = |x: i32| dims.to_tile(HexCoord { q: x, r: 4 }).unwrap();
    let u = game.debug_spawn_unit(&rules, PlayerId(0), t(2), UnitKindId(0));
    game.execute(&rules, Command::MarchTo { unit: u, destination: t(12) })
        .unwrap();
    let save = crate::persistence::encode::encode(&game, &rules);
    let mut copy = crate::persistence::decode::decode(&save, &rules).unwrap();
    for _ in 0..3 {
        play_round(&mut game, &rules);
        play_round(&mut copy, &rules);
        assert_eq!(authoritative_hash(&game), authoritative_hash(&copy));
    }
    let goal = |g: &GameState| g.view().auth.units().iter_cores().next().unwrap().goal;
    assert_eq!(goal(&copy), Some(t(12)));
}

/// A unit standing on the way at the start of the turn cancels the order.
#[test]
fn march_order_stops_when_blocked() {
    let (mut game, rules) = arena();
    let dims = game.view().auth.map().dimensions;
    let t = |x: i32| dims.to_tile(HexCoord { q: x, r: 4 }).unwrap();
    let u = game.debug_spawn_unit(&rules, PlayerId(0), t(2), UnitKindId(0));
    game.execute(&rules, Command::MarchTo { unit: u, destination: t(12) })
        .unwrap();
    // A foreign unit (at peace) stands on the next tile of the way.
    game.debug_spawn_unit(&rules, PlayerId(1), t(5), UnitKindId(0));
    play_round(&mut game, &rules);
    let core = *game.view().auth.units().get(u).unwrap().core;
    assert_eq!(core.goal, None, "blocked: the order is cancelled");
    assert_eq!(core.tile, t(4));
}

/// An enemy that comes into the unit's sight cancels the order at the
/// start of the turn: the player decides what to do next.
#[test]
fn march_order_stops_when_an_enemy_is_in_sight() {
    let (mut game, rules) = arena();
    let dims = game.view().auth.map().dimensions;
    let t = |x: i32| dims.to_tile(HexCoord { q: x, r: 4 }).unwrap();
    let u = game.debug_spawn_unit(&rules, PlayerId(0), t(2), UnitKindId(0));
    let far = dims.to_tile(HexCoord { q: 2, r: 14 }).unwrap();
    game.debug_spawn_unit(&rules, PlayerId(1), far, UnitKindId(0));
    game.execute(&rules, Command::DeclareWar { player: PlayerId(0), enemy: PlayerId(1) })
        .unwrap();
    game.execute(&rules, Command::MarchTo { unit: u, destination: t(12) })
        .unwrap();
    // An enemy warrior appears two tiles ahead (in sight, not on the way).
    let ahead = dims.to_tile(HexCoord { q: 6, r: 3 }).unwrap();
    game.debug_spawn_unit(&rules, PlayerId(1), ahead, UnitKindId(0));
    play_round(&mut game, &rules);
    let core = *game.view().auth.units().get(u).unwrap().core;
    assert_eq!(core.goal, None, "an enemy in sight cancels the order");
    assert_eq!(core.tile, t(4), "the unit stays where it saw the enemy");
}

#[test]
fn march_rejects_garbage_without_mutation() {
    let (mut game, rules) = arena();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 4, r: 4 }).unwrap();
    let u = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
    let before = authoritative_hash(&game);
    // We stand still.
    assert!(game
        .execute(
            &rules,
            Command::MarchTo {
                unit: u,
                destination: t0
            }
        )
        .is_err());
    // Beyond the map.
    assert!(game
        .execute(
            &rules,
            Command::MarchTo {
                unit: u,
                destination: TileId(99999)
            }
        )
        .is_err());
    assert_eq!(before, authoritative_hash(&game));
    game.validate(&rules).unwrap();
}

#[test]
fn march_deterministic_on_same_seed() {
    fn run_once() -> (u64, TileId) {
        // arena() fixes the seed (rng_seed 99): two runs are identical.
        let (mut game, rules) = arena();
        let dims = game.view().auth.map().dimensions;
        let t = |x: i32| dims.to_tile(HexCoord { q: x, r: 4 }).unwrap();
        let u = game.debug_spawn_unit(&rules, PlayerId(0), t(2), UnitKindId(0));
        game.execute(
            &rules,
            Command::MarchTo {
                unit: u,
                destination: t(7),
            },
        )
        .unwrap();
        let pos = game.view().auth.units().get(u).unwrap().core.tile;
        (authoritative_hash(&game), pos)
    }
    assert_eq!(run_once(), run_once());
}

#[test]
fn roads_speed_movement_over_hills() {
    use crate::ids::{TerrainId, UnitKindId};
    // Hills cost 2; with a road 1. Construction 2 turns, passing is cheaper.
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let hill = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    game.debug_set_terrain_batch(&rules, &[(hill, TerrainId(2))]);
    let worker = game.debug_spawn_unit(&rules, PlayerId(0), hill, UnitKindId(2));
    game.execute(&rules, Command::BuildRoad { unit: worker })
        .unwrap();
    // Occupied: a second construction and an improvement on top are refused.
    assert!(game
        .execute(&rules, Command::BuildRoad { unit: worker })
        .is_err());
    assert!(game
        .execute(
            &rules,
            Command::BuildImprovement {
                unit: worker,
                improvement: crate::ids::ImprovementId(0),
            },
        )
        .is_err());
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    assert!(!game.view().auth.map().road(hill));
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    assert!(game.view().auth.map().road(hill));
    // A road counts only between two road tiles (a city is one).
    // Without a route on t0 the hills cost 2 points: all of a warrior's.
    let scout = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(1));
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: scout,
            destination: hill,
        },
    )
    .unwrap();
    let moves = game.view().auth.units().get(scout).unwrap().core.moves_left;
    assert_eq!(moves, 0, "hills without a connected road cost 2 points");
    // With a city on t0 the road connects: half a point (30 of 120).
    game.debug_found_city(&rules, PlayerId(0), t0);
    let rider = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(1));
    play_round(&mut game, &rules);
    let back = dims.to_tile(HexCoord { q: 10, r: 8 }).unwrap();
    game.execute(&rules, Command::MoveUnit { unit: scout, destination: back })
        .unwrap();
    game.execute(&rules, Command::MoveUnit { unit: rider, destination: hill })
        .unwrap();
    let moves = game.view().auth.units().get(rider).unwrap().core.moves_left;
    assert_eq!(moves, 120 - 30, "road step: half a movement point");
    // A repeated road: refusal.
    let worker2 = game.debug_spawn_unit(&rules, PlayerId(0), hill, UnitKindId(2));
    assert!(game
        .execute(&rules, Command::BuildRoad { unit: worker2 })
        .is_err());
    // The save keeps the road layer.
    let save = crate::persistence::encode::encode(&game, &rules);
    let loaded = crate::persistence::decode::decode(&save, &rules).unwrap();
    assert!(loaded.view().auth.map().road(hill));
    assert_eq!(
        crate::replay::authoritative_hash(&game),
        crate::replay::authoritative_hash(&loaded)
    );
    game.validate(&rules).unwrap();
}

#[test]
fn rivers_ford_and_feed_farms() {
    use crate::ids::{ImprovementId, UnitKindId};
    // Crossing a river (an edge between two tiles) takes all moves;
    // walking along it does not. A farm on a river (fresh water) gets +1F.
    use crate::navigation::cost::{step_cost, StepCost};
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let farm = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    let river = dims.to_tile(HexCoord { q: 10, r: 8 }).unwrap();
    let across = dims.to_tile(HexCoord { q: 11, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    let below = dims.to_tile(HexCoord { q: 9, r: 9 }).unwrap();
    game.debug_set_river_edges(&rules, &[(river, across), (farm, below)]);
    let cost = |from, to| step_cost(game.view().auth, &rules, PlayerId(0), from, to);
    assert_eq!(cost(farm, river), Some(StepCost::Points(60)));
    assert_eq!(cost(river, across), Some(StepCost::AllMoves));
    let worker = game.debug_spawn_unit(&rules, PlayerId(0), farm, UnitKindId(2));
    game.execute(
        &rules,
        Command::BuildImprovement {
            unit: worker,
            improvement: ImprovementId(0),
        },
    )
    .unwrap();
    for _ in 0..3 {
        game.execute(
            &rules,
            Command::EndTurn {
                player: PlayerId(0),
            },
        )
        .unwrap();
    }
    // Centre plains at the city minimum 2F; the farm tile: plains 1F +
    // farm 2F + river (fresh water) 1F = 4F, the best tile for 1 citizen.
    let core = game.view().auth.cities().get(city).unwrap().clone();
    let food = crate::derived::yields::city_breakdown(game.view().auth, &rules, &core).rates.food;
    assert_eq!(food, 2 + 4, "centre 2F + riverside farm 4F");
    game.validate(&rules).unwrap();
}

/// Regression (P2): A* ignored borders and only the destination was
/// checked, so a long move walked straight through closed foreign land.
#[test]
fn paths_go_around_closed_borders() {
    use crate::navigation::{find_path, PathRequest, PathScratch};
    let (mut game, rules) = arena();
    let dims = game.view().auth.map().dimensions;
    let tile = |q, r| dims.to_tile(HexCoord { q, r }).unwrap();
    game.debug_found_city(&rules, PlayerId(1), tile(8, 8));
    let unit = game.debug_spawn_unit(&rules, PlayerId(0), tile(5, 8), UnitKindId(0));
    let path = find_path(
        game.view(),
        &rules,
        PathRequest {
            from: tile(5, 8),
            to: tile(11, 8),
            player: PlayerId(0),
            budget: crate::navigation::MoveBudget::flat(600, 600),
            profile: crate::navigation::MoveProfile::LAND,
        },
        &mut PathScratch::default(),
    )
    .expect("a way around the city");
    let foreign = |game: &GameState, t: TileId| {
        game.view().auth.map().owner(t).flatten() == Some(PlayerId(1))
    };
    assert!(!path.tiles.iter().any(|t| foreign(&game, *t)), "{:?}", path.tiles);
    // The march gets there over several turns without entering P1's land.
    for _ in 0..10 {
        let at = game.view().auth.units().get(unit).unwrap().core.tile;
        if at == tile(11, 8) {
            break;
        }
        let _ = game.execute(
            &rules,
            Command::MarchTo {
                unit,
                destination: tile(11, 8),
            },
        );
        let at = game.view().auth.units().get(unit).unwrap().core.tile;
        assert!(!foreign(&game, at), "the march entered closed borders at {at:?}");
        play_round(&mut game, &rules);
    }
    let at = game.view().auth.units().get(unit).unwrap().core.tile;
    assert_eq!(at, tile(11, 8), "the march arrives");
}

/// The move preview (`PlayerView::own_reachable`, the AI's planning of
/// where to stand): the unit's tile with its moves, two flat steps with
/// what is left after them, nothing farther, nothing for another player's
/// unit; an enemy unit's tile is not entered and its neighbours stop the
/// unit (zone of control).
#[test]
fn move_preview_reaches_what_a_move_order_reaches() {
    use crate::knowledge::public::PlayerQueries;
    let (mut game, rules) = arena();
    let dims = game.view().auth.map().dimensions;
    let t = |q: i32, r: i32| dims.to_tile(HexCoord { q, r }).unwrap();
    let u = game.debug_spawn_unit(&rules, PlayerId(0), t(4, 4), UnitKindId(0));
    let other = game.debug_spawn_unit(&rules, PlayerId(1), t(10, 10), UnitKindId(0));
    let view = game.player_view(&rules, PlayerId(0));
    let reach = view.own_reachable(u);
    let full = game.view().auth.units().get(u).unwrap().core.moves_max;
    let left = |tile: TileId| reach.iter().find(|(t, _)| *t == tile).map(|(_, m)| *m);
    assert_eq!(left(t(4, 4)), Some(full));
    assert_eq!(left(t(5, 4)), Some(full - 60), "one flat step");
    assert_eq!(left(t(6, 4)), Some(full - 120), "two flat steps");
    assert_eq!(left(t(7, 4)), None, "three steps are next turn");
    for (tile, _) in &reach {
        let accepted = *tile == t(4, 4) || view.accepts(&Command::MoveUnit { unit: u, destination: *tile });
        assert!(accepted, "a previewed tile is a valid move: {tile:?}");
    }
    assert!(view.own_reachable(other).is_empty(), "not our unit");

    game.execute(&rules, Command::DeclareWar { player: PlayerId(0), enemy: PlayerId(1) }).unwrap();
    game.debug_spawn_unit(&rules, PlayerId(1), t(5, 4), UnitKindId(0));
    let view = game.player_view(&rules, PlayerId(0));
    let reach = view.own_reachable(u);
    let left = |tile: TileId| reach.iter().find(|(t, _)| *t == tile).map(|(_, m)| *m);
    assert_eq!(left(t(5, 4)), None, "the enemy's tile");
    assert_eq!(left(t(5, 3)), Some(0), "next to the enemy from next to it: all moves");
}
