//! Research: prerequisites, queue, technology bonus.

use super::support::*;

#[test]
fn research_requires_prereqs_in_order() {
    // Pottery without agriculture: an honest refusal; after agriculture: ok.
    // Writing without pottery: refusal even with agriculture.
    let rules = crate::fixtures::chained_content();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 21,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    game.debug_found_city(&rules, PlayerId(0), t0);
    let (agri, pottery, writing) = (
        agriculture_id(&rules),
        pottery_id(&rules),
        writing_id(&rules),
    );
    assert!(game
        .execute(
            &rules,
            Command::ChooseTechnology {
                player: PlayerId(0),
                technology: pottery,
            },
        )
        .is_err());
    game.execute(
        &rules,
        Command::ChooseTechnology {
            player: PlayerId(0),
            technology: agri,
        },
    )
    .unwrap();
    assert!(game
        .execute(
            &rules,
            Command::ChooseTechnology {
                player: PlayerId(0),
                technology: writing,
            },
        )
        .is_err());
    research_until(&mut game, &rules, PlayerId(0), agri);
    game.execute(
        &rules,
        Command::ChooseTechnology {
            player: PlayerId(0),
            technology: pottery,
        },
    )
    .unwrap();
    game.validate(&rules).unwrap();
}

#[test]
fn research_queue_completes_in_order_with_overflow() {
    // Queue [agri, pottery]: complete in order, the overflow of beakers
    // carries on. An empty queue is silence (not a panic, not a tech).
    let rules = crate::fixtures::chained_content();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 22,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    game.debug_found_city(&rules, PlayerId(0), t0);
    let (agri, pottery) = (agriculture_id(&rules), pottery_id(&rules));
    game.execute(
        &rules,
        Command::ChooseTechnology {
            player: PlayerId(0),
            technology: agri,
        },
    )
    .unwrap();
    // Pottery in the queue too early: no prerequisite, refusal (strict v1).
    assert!(game
        .execute(
            &rules,
            Command::ChooseTechnology {
                player: PlayerId(0),
                technology: pottery,
            },
        )
        .is_err());
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
    let techs = game.view().auth.players().techs_of(PlayerId(0));
    assert!(techs.contains(&agri) && techs.contains(&pottery));
    assert!(game
        .view()
        .auth
        .players()
        .research_queue_of(PlayerId(0))
        .is_empty());
    game.validate(&rules).unwrap();
}

#[test]
fn technology_boosts_yields() {
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 4, r: 4 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    let percent = |game: &GameState| {
        let core = game.view().auth.cities().get(city).unwrap().clone();
        let b = crate::derived::yields::city_breakdown(game.view().auth, &rules, &core);
        crate::derived::yields::channel_totals(&b.modifiers, crate::rules::effects::YieldKind::Production).percent
    };
    let y0 = percent(&game);
    // Agriculture = TechId(0): an order into the queue (no instant effect,
    // A2), then beakers until completion and exactly +10% by the tech's effect from data
    // (legacy `Output` lands on production).
    game.execute(
        &rules,
        Command::ChooseTechnology {
            player: PlayerId(0),
            technology: TechId(0),
        },
    )
    .unwrap();
    assert_eq!(percent(&game), y0, "queued tech must not boost yet");
    research_until(&mut game, &rules, PlayerId(0), TechId(0));
    let y1 = percent(&game);
    assert_eq!(
        y1,
        y0 + 10,
        "tech must boost yields: {} -> {}",
        y0,
        y1
    );
    // A repeat is an error (already learned), there is no foreign tech.
    assert!(game
        .execute(
            &rules,
            Command::ChooseTechnology {
                player: PlayerId(0),
                technology: TechId(0),
            },
        )
        .is_err());
    assert!(game
        .execute(
            &rules,
            Command::ChooseTechnology {
                player: PlayerId(0),
                technology: TechId(9),
            },
        )
        .is_err());
    game.validate(&rules).unwrap();
}
