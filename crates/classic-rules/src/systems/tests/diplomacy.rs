//! Diplomacy: war gates attacks, peace, open borders.

use super::support::*;

#[test]
fn war_gates_attacks() {
    let (mut game, rules) = arena();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 4, r: 4 }).unwrap();
    let t1 = dims.to_tile(HexCoord { q: 5, r: 4 }).unwrap();
    let a = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
    let d = game.debug_spawn_unit(&rules, PlayerId(1), t1, UnitKindId(0));
    // Peacetime: attack forbidden.
    assert!(game
        .execute(
            &rules,
            Command::AttackUnit {
                attacker: a,
                defender: d
            }
        )
        .is_err());
    // Declaration: ok once, a repeat and war with oneself are not.
    game.execute(
        &rules,
        Command::DeclareWar {
            player: PlayerId(0),
            enemy: PlayerId(1),
        },
    )
    .unwrap();
    assert!(game
        .execute(
            &rules,
            Command::DeclareWar {
                player: PlayerId(0),
                enemy: PlayerId(1)
            }
        )
        .is_err());
    assert!(game
        .execute(
            &rules,
            Command::DeclareWar {
                player: PlayerId(0),
                enemy: PlayerId(0)
            }
        )
        .is_err());
    // Now combat goes on (with return fire the attacker may also get hurt).
    game.execute(
        &rules,
        Command::AttackUnit {
            attacker: a,
            defender: d,
        },
    )
    .unwrap();
    game.validate(&rules).unwrap();
}

#[test]
fn peace_ends_war_and_attack_gate() {
    // DoW -> the strike is allowed; peace -> the strike is forbidden by the war gate;
    // peace without war and peace with oneself are honest errors.
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let t1 = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    let atk = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(1));
    let def = game.debug_spawn_unit(&rules, PlayerId(1), t1, UnitKindId(1));
    let strike = Command::AttackUnit {
        attacker: atk,
        defender: def,
    };
    assert!(game.execute(&rules, strike.clone()).is_err());
    game.execute(
        &rules,
        Command::DeclareWar {
            player: PlayerId(0),
            enemy: PlayerId(1),
        },
    )
    .unwrap();
    // Without moves_left the strike may fail on turn economics, so reset it.
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(1),
        },
    )
    .unwrap();
    assert!(game.execute(&rules, strike.clone()).is_ok());
    game.execute(
        &rules,
        Command::MakePeace {
            player: PlayerId(0),
            enemy: PlayerId(1),
        },
    )
    .unwrap();
    assert!(!game
        .view()
        .auth
        .diplomacy()
        .at_war(PlayerId(0), PlayerId(1)));
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(1),
        },
    )
    .unwrap();
    assert!(game.execute(&rules, strike).is_err());
    // Peace without war is an error; with oneself is an error.
    assert!(game
        .execute(
            &rules,
            Command::MakePeace {
                player: PlayerId(0),
                enemy: PlayerId(1),
            },
        )
        .is_err());
    assert!(game
        .execute(
            &rules,
            Command::MakePeace {
                player: PlayerId(0),
                enemy: PlayerId(0),
            },
        )
        .is_err());
    game.validate(&rules).unwrap();
}

#[test]
fn closed_borders_gate_movement() {
    // P1's city paints the ring; a P0 unit will not stand on it in peacetime;
    // a grant or war opens the way; one's own land is always open.
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let ct = dims.to_tile(HexCoord { q: 10, r: 10 }).unwrap();
    let from = dims.to_tile(HexCoord { q: 8, r: 10 }).unwrap();
    game.debug_found_city(&rules, PlayerId(1), ct);
    let scout = game.debug_spawn_unit(&rules, PlayerId(0), from, UnitKindId(1));
    // P1's neighbouring ring (9,10) is closed in peacetime.
    let ring = dims.to_tile(HexCoord { q: 9, r: 10 }).unwrap();
    assert!(game
        .execute(
            &rules,
            Command::MoveUnit {
                unit: scout,
                destination: ring,
            },
        )
        .is_err());
    // One's own land is always open (a control move nearby).
    let mine = dims.to_tile(HexCoord { q: 7, r: 10 }).unwrap();
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: scout,
            destination: mine,
        },
    )
    .unwrap();
    // A P1->P0 grant opens the way (P1 decides on its own turn).
    play_until_active(&mut game, &rules, PlayerId(1));
    game.execute(
        &rules,
        Command::OpenBorders {
            host: PlayerId(1),
            guest: PlayerId(0),
        },
    )
    .unwrap();
    play_until_active(&mut game, &rules, PlayerId(0));
    assert!(game
        .execute(
            &rules,
            Command::MoveUnit {
                unit: scout,
                destination: ring,
            },
        )
        .is_ok());
    game.validate(&rules).unwrap();
}

#[test]
fn open_borders_validation() {
    let (game, rules) = arena_content();
    let mut game = game;
    // To oneself is an error; a duplicate is an error; an unknown one is an error.
    assert!(game
        .execute(
            &rules,
            Command::OpenBorders {
                host: PlayerId(0),
                guest: PlayerId(0),
            },
        )
        .is_err());
    game.execute(
        &rules,
        Command::OpenBorders {
            host: PlayerId(0),
            guest: PlayerId(1),
        },
    )
    .unwrap();
    assert!(game
        .execute(
            &rules,
            Command::OpenBorders {
                host: PlayerId(0),
                guest: PlayerId(1),
            },
        )
        .is_err());
    assert!(game
        .execute(
            &rules,
            Command::OpenBorders {
                host: PlayerId(9),
                guest: PlayerId(1),
            },
        )
        .is_err());
    game.validate(&rules).unwrap();
}
