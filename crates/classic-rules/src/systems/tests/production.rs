//! Production: queue order, locks, validation, no default order.

use super::support::*;

#[test]
fn production_queue_builds_in_order_with_overflow() {
    use crate::ids::ProductionItem;
    // Queue [settler, warrior]: built head first, overflow
    // carries on; there are no default warriors on top of the queue.
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    // Settlers need size 2.
    game.debug_grow_city(&rules, city, 1);
    game.execute(
        &rules,
        Command::EnqueueProduction {
            player: PlayerId(0),
            city,
            item: ProductionItem::Unit(UnitKindId(0)),
        },
    )
    .unwrap();
    game.execute(
        &rules,
        Command::EnqueueProduction {
            player: PlayerId(0),
            city,
            item: crate::ids::ProductionItem::Unit(UnitKindId(1)),
        },
    )
    .unwrap();
    for _ in 0..30 {
        game.execute(
            &rules,
            Command::EndTurn {
                player: PlayerId(0),
            },
        )
        .unwrap();
        if game
            .view()
            .auth
            .cities()
            .get(city)
            .unwrap()
            .queue
            .is_empty()
        {
            break;
        }
    }
    let view = game.view();
    assert!(
        view.auth.cities().get(city).unwrap().queue.is_empty(),
        "queue must drain"
    );
    // Exactly two spawns in queue order (the settler is absorbed by founding).
    let mut kinds: Vec<u16> = view.auth.units().iter_cores().map(|u| u.kind.0).collect();
    kinds.sort();
    assert_eq!(kinds, vec![0, 1], "settler then warrior, no extras");
    game.validate(&rules).unwrap();
}

#[test]
fn production_queue_builds_locked_and_free_buildings() {
    use crate::ids::{BuildingKindId, ProductionItem};
    let rules = crate::fixtures::chained_content();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 7,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    let granary = ProductionItem::Building(BuildingKindId(0));
    // Without pottery: an honest refusal, the queue is empty.
    assert!(game
        .execute(
            &rules,
            Command::EnqueueProduction {
                player: PlayerId(0),
                city,
                item: granary,
            },
        )
        .is_err());
    assert!(game
        .view()
        .auth
        .cities()
        .get(city)
        .unwrap()
        .queue
        .is_empty());
    // Learned pottery with beakers: the order is taken, the building stands.
    // A chain: agriculture first (a prerequisite in the chained content).
    let agri = agriculture_id(&rules);
    let pottery = pottery_id(&rules);
    game.execute(
        &rules,
        Command::ChooseTechnology {
            player: PlayerId(0),
            technology: agri,
        },
    )
    .unwrap();
    research_until(&mut game, &rules, PlayerId(0), agri);
    game.execute(
        &rules,
        Command::ChooseTechnology {
            player: PlayerId(0),
            technology: pottery,
        },
    )
    .unwrap();
    research_until(&mut game, &rules, PlayerId(0), pottery);
    game.execute(
        &rules,
        Command::EnqueueProduction {
            player: PlayerId(0),
            city,
            item: granary,
        },
    )
    .unwrap();
    for _ in 0..40 {
        game.execute(
            &rules,
            Command::EndTurn {
                player: PlayerId(0),
            },
        )
        .unwrap();
        if !game
            .view()
            .auth
            .cities()
            .get(city)
            .unwrap()
            .buildings
            .is_empty()
        {
            break;
        }
    }
    assert_eq!(
        game.view().auth.cities().get(city).unwrap().buildings,
        vec![BuildingKindId(0)]
    );
    game.validate(&rules).unwrap();
}

#[test]
fn production_queue_validation() {
    use crate::ids::{CityHandle, ProductionItem};
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    // A foreign city: refusal.
    assert!(game
        .execute(
            &rules,
            Command::EnqueueProduction {
                player: PlayerId(1),
                city,
                item: crate::ids::ProductionItem::Unit(UnitKindId(1)),
            },
        )
        .is_err());
    // A stale city: UnknownCity, not a panic.
    assert!(game
        .execute(
            &rules,
            Command::EnqueueProduction {
                player: PlayerId(0),
                city: CityHandle {
                    index: 999,
                    generation: 0,
                },
                item: crate::ids::ProductionItem::Unit(UnitKindId(1)),
            },
        )
        .is_err());
    // An unknown kind: refusal.
    assert!(game
        .execute(
            &rules,
            Command::EnqueueProduction {
                player: PlayerId(0),
                city,
                item: ProductionItem::Unit(crate::ids::UnitKindId(999)),
            },
        )
        .is_err());
    // Overflow: 16 orders ok, the 17th refused, the world untouched.
    for _ in 0..crate::world::city::MAX_QUEUE {
        game.execute(
            &rules,
            Command::EnqueueProduction {
                player: PlayerId(0),
                city,
                item: crate::ids::ProductionItem::Unit(UnitKindId(1)),
            },
        )
        .unwrap();
    }
    assert!(game
        .execute(
            &rules,
            Command::EnqueueProduction {
                player: PlayerId(0),
                city,
                item: crate::ids::ProductionItem::Unit(UnitKindId(1)),
            },
        )
        .is_err());
    assert_eq!(
        game.view().auth.cities().get(city).unwrap().queue.len(),
        crate::world::city::MAX_QUEUE
    );
    // Clear resets.
    game.execute(
        &rules,
        Command::ClearProduction {
            player: PlayerId(0),
            city,
        },
    )
    .unwrap();
    assert!(game
        .view()
        .auth
        .cities()
        .get(city)
        .unwrap()
        .queue
        .is_empty());
}

/// There is no default order: a city with an empty queue builds nothing
/// and keeps its production for the next order (it used to train a free
/// warrior every few turns, which crowded long games with filler units).
#[test]
fn an_empty_queue_builds_nothing_and_keeps_the_production() {
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 4, r: 4 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    let units_before = game.view().auth.units().len();
    // Plains centre 1P + a plains tile 1P = 2 a turn: the stock pays a
    // warrior (30) after 15 turns.
    for _ in 0..15 {
        game.execute(&rules, Command::EndTurn { player: PlayerId(0) })
            .unwrap();
    }
    assert_eq!(game.view().auth.units().len(), units_before, "no unit without an order");
    let stock = game.view().auth.cities().get(city).unwrap().production;
    assert!(stock > 0, "the production waits in the stock");
    game.execute(
        &rules,
        Command::EnqueueProduction {
            player: PlayerId(0),
            city,
            item: crate::ids::ProductionItem::Unit(UnitKindId(1)),
        },
    )
    .unwrap();
    game.execute(&rules, Command::EndTurn { player: PlayerId(0) })
        .unwrap();
    assert!(game.view().auth.units().len() > units_before, "the stock pays the first order");
    game.validate(&rules).unwrap();
}

#[test]
fn buildings_reject_duplicates_queued_and_built() {
    // A building is unique per city (as in Nation): a repeat into the queue is refused,
    // a repeat after construction is refused, one instance in the world.
    use crate::ids::{BuildingKindId, ProductionItem};
    let rules = crate::fixtures::chained_content();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 8,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    let agri = agriculture_id(&rules);
    let pottery = pottery_id(&rules);
    game.execute(
        &rules,
        Command::ChooseTechnology {
            player: PlayerId(0),
            technology: agri,
        },
    )
    .unwrap();
    research_until(&mut game, &rules, PlayerId(0), agri);
    game.execute(
        &rules,
        Command::ChooseTechnology {
            player: PlayerId(0),
            technology: pottery,
        },
    )
    .unwrap();
    research_until(&mut game, &rules, PlayerId(0), pottery);
    let order = Command::EnqueueProduction {
        player: PlayerId(0),
        city,
        item: ProductionItem::Building(BuildingKindId(0)),
    };
    game.execute(&rules, order.clone()).unwrap();
    assert!(game.execute(&rules, order.clone()).is_err());
    assert_eq!(game.view().auth.cities().get(city).unwrap().queue.len(), 1);
    for _ in 0..40 {
        game.execute(
            &rules,
            Command::EndTurn {
                player: PlayerId(0),
            },
        )
        .unwrap();
        if !game
            .view()
            .auth
            .cities()
            .get(city)
            .unwrap()
            .buildings
            .is_empty()
        {
            break;
        }
    }
    assert!(game.execute(&rules, order).is_err());
    assert_eq!(
        game.view().auth.cities().get(city).unwrap().buildings.len(),
        1
    );
    game.validate(&rules).unwrap();
}
