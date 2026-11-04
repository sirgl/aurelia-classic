//! Combat: strikes, counter-attack, siege, fortify.

use super::support::*;

#[test]
fn attack_damages_then_kills() {
    let (mut game, rules) = arena();
    game.execute(
        &rules,
        Command::DeclareWar {
            player: PlayerId(0),
            enemy: PlayerId(1),
        },
    )
    .unwrap();
    let (mut a, d) = adjacent_pair(&mut game, &rules);
    let dims = game.view().auth.map().dimensions;
    let attacker_tile = game.view().auth.units().get(a).unwrap().core.tile;
    // A reserve far away keeps P0 alive if the counter-attack kills `a`.
    let reserve = dims.to_tile(HexCoord { q: 14, r: 14 }).unwrap();
    game.debug_spawn_unit(&rules, PlayerId(0), reserve, UnitKindId(0));
    let hp0: u16 = game.view().auth.units().get(d).unwrap().core.hp;
    game.execute(
        &rules,
        Command::AttackUnit {
            attacker: a,
            defender: d,
        },
    )
    .unwrap();
    let hp1: u16 = game.view().auth.units().get(d).unwrap().core.hp;
    assert!(hp1 < hp0, "attack must wound: {} -> {}", hp0, hp1);
    // Finishing off: one strike per turn, with fresh units replacing the fallen.
    for _ in 0..12 {
        if game.view().auth.units().get(d).is_none() {
            break;
        }
        play_round(&mut game, &rules);
        if game.view().auth.units().get(a).is_none() {
            a = game.debug_spawn_unit(&rules, PlayerId(0), attacker_tile, UnitKindId(0));
        }
        game.execute(
            &rules,
            Command::AttackUnit {
                attacker: a,
                defender: d,
            },
        )
        .unwrap();
    }
    assert!(
        game.view().auth.units().get(d).is_none(),
        "defender must die"
    );
    game.validate(&rules).unwrap();
}

#[test]
fn attack_is_deterministic_on_same_seed() {
    // The same seed + the same commands = the same hp (the combat stream is deterministic).
    fn run_once() -> (u64, u16) {
        let (mut game, rules) = arena();
        game.execute(
            &rules,
            Command::DeclareWar {
                player: PlayerId(0),
                enemy: PlayerId(1),
            },
        )
        .unwrap();
        let (a, d) = adjacent_pair(&mut game, &rules);
        for _ in 0..3 {
            play_round(&mut game, &rules);
            game.execute(
                &rules,
                Command::AttackUnit {
                    attacker: a,
                    defender: d,
                },
            )
            .unwrap();
        }
        (
            authoritative_hash(&game),
            game.view().auth.units().get(d).unwrap().core.hp,
        )
    }
    assert_eq!(run_once(), run_once());
}

#[test]
fn attack_rejected_cases() {
    let (mut game, rules) = arena();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 4, r: 4 }).unwrap();
    let t1 = dims.to_tile(HexCoord { q: 5, r: 4 }).unwrap();
    let far = dims.to_tile(HexCoord { q: 12, r: 12 }).unwrap();
    let a = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
    let friend = game.debug_spawn_unit(&rules, PlayerId(0), t1, UnitKindId(0));
    let foe_far = game.debug_spawn_unit(&rules, PlayerId(1), far, UnitKindId(0));
    let before = authoritative_hash(&game);
    // Friendly fire.
    assert!(game
        .execute(
            &rules,
            Command::AttackUnit {
                attacker: a,
                defender: friend
            }
        )
        .is_err());
    // Out of reach.
    assert!(game
        .execute(
            &rules,
            Command::AttackUnit {
                attacker: a,
                defender: foe_far
            }
        )
        .is_err());
    // An unknown unit.
    assert!(game
        .execute(
            &rules,
            Command::AttackUnit {
                attacker: crate::ids::UnitHandle {
                    index: 777,
                    generation: 0
                },
                defender: friend,
            }
        )
        .is_err());
    assert_eq!(before, authoritative_hash(&game));
}

#[test]
fn siege_damages_then_captures_city() {
    // A city has 200 HP; a strike wounds it, and the strike that
    // takes the last HP captures it: owner, territory, half the
    // population, at most half the damage, the conqueror's decision.
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let ct = dims.to_tile(HexCoord { q: 10, r: 10 }).unwrap();
    let at = dims.to_tile(HexCoord { q: 9, r: 10 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(1), ct);
    let atk = game.debug_spawn_unit(&rules, PlayerId(0), at, UnitKindId(1));
    game.execute(
        &rules,
        Command::DeclareWar {
            player: PlayerId(0),
            enemy: PlayerId(1),
        },
    )
    .unwrap();
    let max = crate::combat::city::max_hit_points(
        game.view().auth,
        &rules,
        game.view().auth.cities().get(city).unwrap(),
    );
    assert_eq!(max, crate::world::city::CITY_BASE_HIT_POINTS);
    play_round(&mut game, &rules);
    game.execute(&rules, Command::AttackCity { attacker: atk, city })
        .unwrap();
    let damage = game.view().auth.cities().get(city).unwrap().damage;
    assert!(damage > 0, "siege must wound");
    game.validate(&rules).unwrap();
    // One more strike from a fresh unit when the city is at 1 HP.
    play_round(&mut game, &rules);
    game.debug_set_city_damage(city, max - 1);
    let fresh = if game.view().auth.units().get(atk).is_some() {
        atk
    } else {
        game.debug_spawn_unit(&rules, PlayerId(0), at, UnitKindId(1))
    };
    game.execute(&rules, Command::AttackCity { attacker: fresh, city })
        .unwrap();
    let view = game.view();
    let core = view.auth.cities().get(city).unwrap();
    assert_eq!(core.owner, PlayerId(0), "city must fall");
    assert_eq!(core.status, crate::world::city::CityStatus::AwaitingDecision);
    assert!(core.damage <= max / 2, "captured damage at most half: {}", core.damage);
    assert_eq!(core.population, 1);
    assert_eq!(view.auth.units().get(fresh).unwrap().core.tile, ct, "the attacker moves in");
    // The territory follows the winner (the ring is ours).
    for t in crate::math::tiles_in_hex_radius(
        &dims,
        ct,
        crate::derived::yields::CITY_TERRITORY_RADIUS,
    ) {
        assert_eq!(
            view.auth.map().owner(t).unwrap(),
            Some(PlayerId(0)),
            "conquered ring tile {:?}",
            t
        );
    }
    game.validate(&rules).unwrap();
}

#[test]
fn siege_validation() {
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let ct = dims.to_tile(HexCoord { q: 10, r: 10 }).unwrap();
    let at = dims.to_tile(HexCoord { q: 9, r: 10 }).unwrap();
    let far = dims.to_tile(HexCoord { q: 2, r: 2 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(1), ct);
    let atk = game.debug_spawn_unit(&rules, PlayerId(0), at, UnitKindId(1));
    let siege = Command::AttackCity {
        attacker: atk,
        city,
    };
    // Peace: refusal; own city: refusal; far away: refusal.
    assert!(game.execute(&rules, siege.clone()).is_err());
    let own = game.debug_found_city(&rules, PlayerId(0), far);
    assert!(game
        .execute(
            &rules,
            Command::AttackCity {
                attacker: atk,
                city: own,
            },
        )
        .is_err());
    let far_unit = game.debug_spawn_unit(&rules, PlayerId(0), far, UnitKindId(1));
    assert!(game
        .execute(
            &rules,
            Command::AttackCity {
                attacker: far_unit,
                city,
            },
        )
        .is_err());
    game.validate(&rules).unwrap();
}

#[test]
fn fortify_halves_incoming_damage() {
    // One seed, one roll: a fortified unit (15 def) takes strictly
    // less than an unfortified one (10 def). Two identical worlds, one flag.
    let setup_pair = || {
        let (mut game, rules) = arena_content();
        let dims = game.view().auth.map().dimensions;
        let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
        let t1 = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
        let atk = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(1));
        let def = game.debug_spawn_unit(&rules, PlayerId(1), t1, UnitKindId(1));
        game.execute(
            &rules,
            Command::DeclareWar {
                player: PlayerId(0),
                enemy: PlayerId(1),
            },
        )
        .unwrap();
        (game, rules, atk, def)
    };
    let (mut plain, rules, atk, def) = setup_pair();
    let (mut fort, _, atk2, def2) = setup_pair();
    // The defender fortifies on its own turn; both worlds pass one round.
    play_until_active(&mut fort, &rules, PlayerId(1));
    fort.execute(&rules, Command::Fortify { unit: def2 })
        .unwrap();
    assert!(fort
        .execute(&rules, Command::Fortify { unit: def2 })
        .is_err());
    play_until_active(&mut fort, &rules, PlayerId(0));
    play_round(&mut plain, &rules);
    plain
        .execute(
            &rules,
            Command::AttackUnit {
                attacker: atk,
                defender: def,
            },
        )
        .unwrap();
    fort.execute(
        &rules,
        Command::AttackUnit {
            attacker: atk2,
            defender: def2,
        },
    )
    .unwrap();
    let hp_plain = plain.view().auth.units().get(def).map(|u| u.core.hp);
    let hp_fort = fort.view().auth.units().get(def2).map(|u| u.core.hp);
    match (hp_plain, hp_fort) {
        (Some(a), Some(b)) => assert!(b > a, "fortified takes less: {} vs {}", a, b),
        _ => panic!("both defenders must survive one strike"),
    }
    // Movement cancels fortification: fortified, on the next turn moved.
    play_round(&mut fort, &rules);
    fort.execute(&rules, Command::Fortify { unit: atk2 })
        .unwrap();
    let t2 = fort
        .view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q: 7, r: 8 })
        .unwrap();
    fort.execute(
        &rules,
        Command::MoveUnit {
            unit: atk2,
            destination: t2,
        },
    )
    .unwrap();
    assert!(!fort.view().auth.units().get(atk2).unwrap().core.stance.is_fortified());
    plain.validate(&rules).unwrap();
    fort.validate(&rules).unwrap();
}

/// Regression (P1): an attack left the attacker's moves untouched, so one
/// unit struck any number of times per turn and still walked away.
/// Rules (rules.md): attacking needs moves left and ends the moves.
#[test]
fn attack_spends_the_attackers_moves() {
    let (mut game, rules) = arena();
    game.execute(
        &rules,
        Command::DeclareWar {
            player: PlayerId(0),
            enemy: PlayerId(1),
        },
    )
    .unwrap();
    let (a, d) = adjacent_pair(&mut game, &rules);
    game.execute(
        &rules,
        Command::AttackUnit {
            attacker: a,
            defender: d,
        },
    )
    .unwrap();
    assert_eq!(game.view().auth.units().get(a).unwrap().core.moves_left, 0);
    let before = authoritative_hash(&game);
    let again = game.execute(
        &rules,
        Command::AttackUnit {
            attacker: a,
            defender: d,
        },
    );
    assert!(again.is_err(), "no second strike in the same turn");
    assert_eq!(before, authoritative_hash(&game));
    play_round(&mut game, &rules);
    game.execute(
        &rules,
        Command::AttackUnit {
            attacker: a,
            defender: d,
        },
    )
    .expect("moves are back next turn");
}

#[test]
fn civilians_do_not_attack() {
    let (mut game, rules) = arena_content();
    game.execute(
        &rules,
        Command::DeclareWar {
            player: PlayerId(0),
            enemy: PlayerId(1),
        },
    )
    .unwrap();
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 4, r: 4 }).unwrap();
    let t1 = dims.to_tile(HexCoord { q: 5, r: 4 }).unwrap();
    let settler = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
    let foe = game.debug_spawn_unit(&rules, PlayerId(1), t1, UnitKindId(1));
    let strike = game.execute(
        &rules,
        Command::AttackUnit {
            attacker: settler,
            defender: foe,
        },
    );
    assert!(strike.is_err(), "a settler is not a combat unit");
}

/// Siege `city` from `from` until it falls: the city is brought to its
/// last hit point, then one strike takes it.
fn capture_city(
    game: &mut GameState,
    rules: &CompiledRuleset,
    from: TileId,
    city: crate::ids::CityHandle,
) {
    let attacker = game.debug_spawn_unit(rules, PlayerId(0), from, UnitKindId(1));
    let max = crate::combat::city::max_hit_points(
        game.view().auth,
        rules,
        game.view().auth.cities().get(city).unwrap(),
    );
    game.debug_set_city_damage(city, max - 1);
    game.execute(rules, Command::AttackCity { attacker, city })
        .expect("the strike lands");
    assert_eq!(game.view().auth.cities().get(city).unwrap().owner, PlayerId(0), "the city must fall");
}

/// Regression (P2): capture claimed the whole hex disc of the city's radius
/// for the winner, taking tiles of the loser's other cities and of third
/// players. Only the captured city's own tiles change hands (a plot
/// belongs to one city).
#[test]
fn capture_transfers_only_the_city_tiles() {
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let tile = |q, r| dims.to_tile(HexCoord { q, r }).unwrap();
    let other = game.debug_found_city(&rules, PlayerId(1), tile(4, 8));
    let target = game.debug_found_city(&rules, PlayerId(1), tile(7, 8));
    // P0 keeps a city far away (never eliminated during the siege).
    game.debug_found_city(&rules, PlayerId(0), tile(13, 2));
    // The target buys a ring-2 tile: its radius becomes 2, and its disc
    // now covers (5,8), a tile of the other city.
    play_until_active(&mut game, &rules, PlayerId(1));
    game.debug_earn(PlayerId(1), 500);
    game.execute(
        &rules,
        Command::PurchaseTile {
            player: PlayerId(1),
            city: target,
            tile: tile(9, 8),
        },
    )
    .unwrap();
    play_until_active(&mut game, &rules, PlayerId(0));
    game.execute(
        &rules,
        Command::DeclareWar {
            player: PlayerId(0),
            enemy: PlayerId(1),
        },
    )
    .unwrap();
    capture_city(&mut game, &rules, tile(7, 9), target);
    let auth = game.view().auth;
    let owner = |q, r| auth.map().owner(tile(q, r)).unwrap();
    assert_eq!(owner(5, 8), Some(PlayerId(1)), "the other city keeps its tile");
    assert_eq!(owner(4, 8), Some(PlayerId(1)));
    assert_eq!(auth.cities().get(other).unwrap().owner, PlayerId(1));
    assert_eq!(owner(9, 8), Some(PlayerId(0)), "the bought tile follows the city");
    assert_eq!(owner(6, 8), Some(PlayerId(0)), "the ring follows the city");
    game.validate(&rules).unwrap();
}

/// The preview of a move and attack (`attack_preview_from`, the AI's
/// planning model, classic/docs/ai-tactics.md): from the attacker's own tile it is
/// the attack preview; from a tile next to a target out of reach it is
/// the preview the unit gets once it stands there.
#[test]
fn preview_from_a_tile_is_the_preview_of_the_strike_from_there() {
    use crate::knowledge::public::PlayerQueries;
    let (mut game, rules) = arena();
    game.execute(&rules, Command::DeclareWar { player: PlayerId(0), enemy: PlayerId(1) }).unwrap();
    let (a, d) = adjacent_pair(&mut game, &rules);
    let dims = game.view().auth.map().dimensions;
    let t = |q: i32, r: i32| dims.to_tile(HexCoord { q, r }).unwrap();
    let target = game.view().auth.units().get(d).unwrap().core.tile;
    let b = game.debug_spawn_unit(&rules, PlayerId(0), t(7, 3), UnitKindId(0));
    let view = game.player_view(&rules, PlayerId(0));
    assert_eq!(view.attack_preview_from(a, t(4, 4), target), view.attack_preview(a, target));
    assert!(view.attack_preview_from(a, t(4, 4), target).is_some());
    assert!(view.attack_preview(b, target).is_none(), "two tiles away");
    assert!(view.attack_preview_from(b, t(7, 3), target).is_none(), "out of reach from there");
    let planned = view.attack_preview_from(b, t(6, 3), target);
    assert!(planned.is_some());
    game.execute(&rules, Command::MoveUnit { unit: b, destination: t(6, 3) }).unwrap();
    let view = game.player_view(&rules, PlayerId(0));
    assert_eq!(view.attack_preview(b, target), planned);
}
