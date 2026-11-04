//! Worker jobs: farms, chopping, roads, resources.

use super::support::*;

#[test]
fn workers_build_farm_over_turns() {
    use crate::ids::{ImprovementId, UnitKindId};
    // A worker builds a farm in 3 turns: progress ticks, on the 3rd the improvement
    // is on the map + journal; the city's yield grew by exactly +2 food.
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let wt = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    let worker = game.debug_spawn_unit(&rules, PlayerId(0), wt, UnitKindId(2));
    let y0 = game.derived().yields.of(city).unwrap().food;
    game.execute(
        &rules,
        Command::BuildImprovement {
            unit: worker,
            improvement: ImprovementId(0),
        },
    )
    .unwrap();
    for _ in 0..2 {
        game.execute(
            &rules,
            Command::EndTurn {
                player: PlayerId(0),
            },
        )
        .unwrap();
        assert_eq!(
            game.view().auth.map().improvement(wt).unwrap(),
            None,
            "farm not ready yet"
        );
    }
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    assert_eq!(
        game.view().auth.map().improvement(wt).unwrap(),
        Some(ImprovementId(0))
    );
    assert!(game
        .view()
        .auth
        .units()
        .get(worker)
        .unwrap()
        .core
        .work
        .is_none());
    // The city moves its citizen from another plains tile (1F 1P) to the
    // farm (3F 1P): +2 food.
    assert_eq!(
        game.derived().yields.of(city).unwrap().food,
        y0 + 2,
        "plain farm feeds +2"
    );
    game.validate(&rules).unwrap();
}

#[test]
fn workers_chop_forest_over_turns() {
    use crate::ids::UnitKindId;
    // Forest: +1P while it stands; chopping (chop_turns=2) removes the feature with
    // a FeatureChopped journal entry and gives a one-off grant of 8 to the nearest city;
    // yields drop by exactly 1P. Grant + the income of 2 turns, exactly.
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let wt = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    let forest = crate::ids::FeatureId(0);
    game.debug_set_feature_batch(&rules, &[(wt, forest)]);
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    let worker = game.debug_spawn_unit(&rules, PlayerId(0), wt, UnitKindId(2));
    let y0 = game.derived().yields.of(city).unwrap().production;
    let p0 = game.view().auth.cities().get(city).unwrap().production;
    let income = y0 as u32;
    game.execute(&rules, Command::ChopForest { unit: worker })
        .unwrap();
    // Tick 1: the feature still stands (work is under way, the order is already taken).
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    assert_eq!(
        game.view().auth.map().feature(wt).unwrap(),
        Some(forest),
        "chop not ready yet"
    );
    // Tick 2: done, the feature is removed, the worker is free.
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    assert_eq!(
        game.view().auth.map().feature(wt).unwrap(),
        None,
        "forest chopped"
    );
    assert!(game
        .view()
        .auth
        .units()
        .get(worker)
        .unwrap()
        .core
        .work
        .is_none());
    // Grant: exactly +8 on top of the income of two turns (the forest in both ticks is
    // still standing during produce; completion comes after produce).
    let c = game.view().auth.cities().get(city).unwrap();
    assert_eq!(
        c.production,
        p0 + 2 * income + 8,
        "chop grant lands on nearest city"
    );
    // Yields: the forest bonus went away by exactly 1P.
    assert_eq!(
        game.derived().yields.of(city).unwrap().production,
        y0 - 1,
        "forest prod bonus gone"
    );
    game.validate(&rules).unwrap();
}

#[test]
fn chop_validation_rules() {
    use crate::ids::UnitKindId;
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let wt = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    let forest = crate::ids::FeatureId(0);
    let worker = game.debug_spawn_unit(&rules, PlayerId(0), wt, UnitKindId(2));
    let warrior = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(1));
    // No forest: refusal (and the world is untouched: the fuzz invariant checks the hash).
    assert!(game
        .execute(&rules, Command::ChopForest { unit: worker })
        .is_err());
    // Not a worker: refusal (there is a forest on both tiles).
    game.debug_set_feature_batch(&rules, &[(t0, forest), (wt, forest)]);
    assert!(game
        .execute(&rules, Command::ChopForest { unit: warrior })
        .is_err());
    // A worker on a forest tile: success; a repeat is already working.
    game.execute(&rules, Command::ChopForest { unit: worker })
        .unwrap();
    assert!(game
        .execute(&rules, Command::ChopForest { unit: worker })
        .is_err());
    // A march abandons the chop (like construction): a step onto a neighbouring open tile.
    let away = dims.to_tile(HexCoord { q: 10, r: 8 }).unwrap();
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: worker,
            destination: away,
        },
    )
    .unwrap();
    assert!(game
        .view()
        .auth
        .units()
        .get(worker)
        .unwrap()
        .core
        .work
        .is_none());
    game.validate(&rules).unwrap();
}

#[test]
fn build_validation_and_move_cancels() {
    use crate::ids::{ImprovementId, UnitKindId};
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let wt = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    game.debug_found_city(&rules, PlayerId(0), t0);
    let worker = game.debug_spawn_unit(&rules, PlayerId(0), wt, UnitKindId(2));
    let warrior = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(1));
    // A non-worker cannot build.
    assert!(game
        .execute(
            &rules,
            Command::BuildImprovement {
                unit: warrior,
                improvement: ImprovementId(0),
            },
        )
        .is_err());
    // Unknown improvement: refusal.
    assert!(game
        .execute(
            &rules,
            Command::BuildImprovement {
                unit: worker,
                improvement: ImprovementId(99),
            },
        )
        .is_err());
    game.execute(
        &rules,
        Command::BuildImprovement {
            unit: worker,
            improvement: ImprovementId(0),
        },
    )
    .unwrap();
    // A second construction on top: refusal (one at a time).
    assert!(game
        .execute(
            &rules,
            Command::BuildImprovement {
                unit: worker,
                improvement: ImprovementId(1),
            },
        )
        .is_err());
    // A move abandons construction (a march = a new order).
    let away = dims.to_tile(HexCoord { q: 7, r: 8 }).unwrap();
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: worker,
            destination: away,
        },
    )
    .unwrap();
    assert!(game
        .view()
        .auth
        .units()
        .get(worker)
        .unwrap()
        .core
        .work
        .is_none());
    // A stale handle: UnknownUnit via get (the types guarantee it).
    game.validate(&rules).unwrap();
}

#[test]
fn wheat_needs_farm_to_feed() {
    use crate::ids::{ImprovementId, ResourceId};
    // Wheat without a farm: zero; with a farm: +2F in the city's yield.
    // Sorted ids: gold_ore=0, wheat=1 (see fixtures::ARENA_RON).
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let wt = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    game.debug_set_resource_batch(&rules, &[(wt, ResourceId(1))]);
    let y_bare = game.derived().yields.of(city).unwrap().food;
    // The farm honestly by a worker in 3 turns (the B1 mechanic is already covered).
    let worker = game.debug_spawn_unit(&rules, PlayerId(0), wt, UnitKindId(2));
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
    // Wheat with its farm: 1F + 2F (farm) + 2F (wheat behind the farm).
    assert_eq!(
        game.derived().yields.of(city).unwrap().food,
        y_bare + 4,
        "farm (+2) and wheat (+2) stack"
    );
    game.validate(&rules).unwrap();
}

#[test]
fn gold_flows_only_with_mine_and_writing() {
    use crate::ids::{ImprovementId, ResourceId, TerrainId, UnitKindId};
    // Gold vein (id 0): without a mine and writing: zero gold;
    // with a mine but without writing: zero; with everything: +3/turn into the treasury.
    // Required: hills with gold in the ring + a worker + a mine + writing.
    let rules = crate::fixtures::chained_content();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 31,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let gt = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    game.debug_set_terrain_batch(&rules, &[(gt, TerrainId(2))]);
    game.debug_set_resource_batch(&rules, &[(gt, ResourceId(0))]);
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    let core_of = |game: &GameState| game.view().auth.cities().get(city).unwrap().clone();
    // Before the mine the vein is silent even with the mechanic unlocked.
    let gold_bare =
        crate::derived::yields::city_breakdown(game.view().auth, &rules, &core_of(&game)).rates.gold;
    assert_eq!(gold_bare, 0);
    let worker = game.debug_spawn_unit(&rules, PlayerId(0), gt, UnitKindId(2));
    game.execute(
        &rules,
        Command::BuildImprovement {
            unit: worker,
            improvement: ImprovementId(1),
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
    // The mine is there, writing is not: gold is still zero.
    let gold_locked =
        crate::derived::yields::city_breakdown(game.view().auth, &rules, &core_of(&game)).rates.gold;
    assert_eq!(gold_locked, 0);
    // We learn agri -> pottery -> writing in a chain and check the flow.
    for tech in [
        agriculture_id(&rules),
        pottery_id(&rules),
        writing_id(&rules),
    ] {
        game.execute(
            &rules,
            Command::ChooseTechnology {
                player: PlayerId(0),
                technology: tech,
            },
        )
        .unwrap();
        research_until(&mut game, &rules, PlayerId(0), tech);
    }
    let gold_open =
        crate::derived::yields::city_breakdown(game.view().auth, &rules, &core_of(&game)).rates.gold;
    assert_eq!(gold_open, 3);
    let t_before = game.view().auth.players().treasury_of(PlayerId(0));
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    assert!(
        game.view().auth.players().treasury_of(PlayerId(0)) > t_before,
        "gold must flow into treasury"
    );
    game.validate(&rules).unwrap();
}
