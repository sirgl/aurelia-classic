//! Purchase: hurry production and buy tiles with gold.

use super::support::*;

#[test]
fn hurry_completes_head_for_gold() {
    use crate::ids::ProductionItem;
    // Warrior 30: the price of the rest of the head is paid from the treasury,
    // the unit appears at once and the queue is empty.
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    game.execute(
        &rules,
        Command::EnqueueProduction {
            player: PlayerId(0),
            city,
            item: ProductionItem::Unit(UnitKindId(1)),
        },
    )
    .unwrap();
    let hurry = Command::HurryProduction {
        player: PlayerId(0),
        city,
    };
    assert!(game.execute(&rules, hurry.clone()).is_err());
    for _ in 0..7 {
        game.execute(
            &rules,
            Command::EndTurn {
                player: PlayerId(0),
            },
        )
        .unwrap();
    }
    let units_before = game.view().auth.units().len();
    // Price of a 30-production item: ⌊900^0.75⌋ = 164 → 160.
    let price = crate::world::city::hurry_price(30, 0).unwrap();
    assert_eq!(price, 160);
    game.debug_earn(PlayerId(0), price + 3);
    let treasury = game.view().auth.players().treasury_of(PlayerId(0));
    game.execute(&rules, hurry).unwrap();
    let view = game.view();
    assert!(view.auth.cities().get(city).unwrap().queue.is_empty());
    assert_eq!(view.auth.units().len(), units_before + 1);
    assert_eq!(view.auth.players().treasury_of(PlayerId(0)), treasury - price);
    let spawned = view
        .auth
        .units()
        .iter_cores()
        .any(|u| u.kind == UnitKindId(1) && u.tile == t0);
    assert!(spawned, "hurried warrior spawns on city tile");
    game.validate(&rules).unwrap();
}

#[test]
fn hurry_validation() {
    use crate::ids::ProductionItem;
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    // An empty queue: refusal even with gold in the future.
    assert!(game
        .execute(
            &rules,
            Command::HurryProduction {
                player: PlayerId(0),
                city,
            },
        )
        .is_err());
    // A foreign city: refusal.
    assert!(game
        .execute(
            &rules,
            Command::HurryProduction {
                player: PlayerId(1),
                city,
            },
        )
        .is_err());
    // There is an order, no gold: refusal, the queue is intact.
    game.execute(
        &rules,
        Command::EnqueueProduction {
            player: PlayerId(0),
            city,
            item: ProductionItem::Unit(UnitKindId(1)),
        },
    )
    .unwrap();
    assert!(game
        .execute(
            &rules,
            Command::HurryProduction {
                player: PlayerId(0),
                city,
            },
        )
        .is_err());
    assert_eq!(game.view().auth.cities().get(city).unwrap().queue.len(), 1);
}

#[test]
fn purchase_tile_spends_gold_and_claims() {
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    let target = ring_tile(&game, t0, 2);
    game.debug_earn(PlayerId(0), 200);
    let turn = game.view().auth.turn().round;
    let price = crate::world::city::purchase_price(2, turn);
    let before = game.view().auth.players().treasury_of(PlayerId(0));
    assert_eq!(before, 200);
    game.execute(
        &rules,
        Command::PurchaseTile {
            player: PlayerId(0),
            city,
            tile: target,
        },
    )
    .unwrap();
    {
        let view = game.view();
        assert_eq!(
            view.auth.players().treasury_of(PlayerId(0)),
            before - price,
            "gold spent at the validated price"
        );
        assert_eq!(
            view.auth.map().owner(target),
            Some(Some(PlayerId(0))),
            "tile claimed"
        );
        let core = view.auth.cities().get(city).unwrap();
        assert_eq!(core.acquired, 0, "a purchase does not raise the culture level");
        assert_eq!(core.radius, 2, "radius follows acquired ring");
    }
    game.validate(&rules).unwrap();
    // Save: the radius survives the roundtrip, the hash matches.
    let save = crate::persistence::encode::encode(&game, &rules);
    let loaded = crate::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(loaded.view().auth.cities().get(city).unwrap().radius, 2);
    assert_eq!(
        crate::replay::authoritative_hash(&game),
        crate::replay::authoritative_hash(&loaded)
    );
}

#[test]
fn purchase_tile_validation() {
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    let target = ring_tile(&game, t0, 2);
    let buy = Command::PurchaseTile {
        player: PlayerId(0),
        city,
        tile: target,
    };
    // No gold: refusal (treasury 0 < 50), the world is untouched.
    assert_eq!(
        game.view().auth.players().treasury_of(PlayerId(0)),
        0,
        "fresh world starts goldless"
    );
    assert!(game.execute(&rules, buy.clone()).is_err());
    // A foreign city: refusal.
    assert!(game
        .execute(
            &rules,
            Command::PurchaseTile {
                player: PlayerId(1),
                city,
                tile: target,
            },
        )
        .is_err());
    // Already an own tile (the founding): refusal.
    let ring1 = ring_tile(&game, t0, 1);
    game.debug_earn(PlayerId(0), 200);
    assert!(game
        .execute(
            &rules,
            Command::PurchaseTile {
                player: PlayerId(0),
                city,
                tile: ring1,
            },
        )
        .is_err());
    // Ring 3: neutral, but not adjacent to the border (we own ring 1)
    // refusal: purchase only along the current border.
    let ring3 = ring_tile(&game, t0, 3);
    assert!(game
        .execute(
            &rules,
            Command::PurchaseTile {
                player: PlayerId(0),
                city,
                tile: ring3,
            },
        )
        .is_err());
    // Gold is there, rules are met: ok (the same tile that was rejected
    // above without money).
    assert!(game.execute(&rules, buy).is_ok());
    game.validate(&rules).unwrap();
}

/// Regression (P2): purchase_city_for returned None as soon as the first
/// city of the player could not buy the tile (`?` inside the loop), so a
/// tile next to the second city never got a purchase offer.
#[test]
fn purchase_city_is_found_among_all_cities() {
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let tile = |q, r| dims.to_tile(HexCoord { q, r }).unwrap();
    let _first = game.debug_found_city(&rules, PlayerId(0), tile(2, 2));
    let second = game.debug_found_city(&rules, PlayerId(0), tile(11, 11));
    // Ring 2 of the second city, far beyond the first city's ring 3.
    let target = tile(13, 11);
    let auth = game.view().auth;
    let found = crate::world::territory::purchase_city_for(auth, &rules, PlayerId(0), target);
    assert_eq!(found.map(|(city, _)| city), Some(second));
}
