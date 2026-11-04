//! End of turn: move reset, healing, upkeep, elimination.

use super::support::*;

#[test]
fn moves_exhaust_and_reset_on_end_turn() {
    // Debug units: movement fallback = MOVEMENT_DEFAULT = 2 (120 points).
    let (mut game, rules) = arena();
    let dims = game.view().auth.map().dimensions;
    let t = |x: i32| dims.to_tile(HexCoord { q: x, r: 4 }).unwrap();
    let u = game.debug_spawn_unit(&rules, PlayerId(0), t(4), UnitKindId(0));
    let left = |game: &GameState| game.view().auth.units().get(u).unwrap().core.moves_left;
    assert_eq!(left(&game), 120);
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: u,
            destination: t(5),
        },
    )
    .unwrap();
    assert_eq!(left(&game), 60);
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: u,
            destination: t(6),
        },
    )
    .unwrap();
    assert_eq!(left(&game), 0);
    // The margin is exhausted: reject without mutation.
    let before = authoritative_hash(&game);
    assert!(game
        .execute(
            &rules,
            Command::MoveUnit {
                unit: u,
                destination: t(7)
            }
        )
        .is_err());
    assert_eq!(before, authoritative_hash(&game));
    // The owner's EndTurn resets the points.
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    assert_eq!(left(&game), 120);
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: u,
            destination: t(7),
        },
    )
    .unwrap();
    game.validate(&rules).unwrap();
}

#[test]
fn elimination_declares_winner() {
    // P1 with one warrior against P0: we kill it, P1 is dead, P0 is the winner.
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
    let a = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(1));
    let d = game.debug_spawn_unit(&rules, PlayerId(1), t1, UnitKindId(1));
    // A reserve far away keeps P0 alive while its attackers fall.
    let reserve = dims.to_tile(HexCoord { q: 14, r: 14 }).unwrap();
    game.debug_spawn_unit(&rules, PlayerId(0), reserve, UnitKindId(1));
    assert_eq!(game.winner(), None, "two alive — no winner yet");
    // Finishing off the defender (return fire may kill the attacker,
    // so we respawn it; P1's defence is not replenished).
    for _ in 0..30 {
        if game.view().auth.units().get(d).is_none() {
            break;
        }
        let atk = if game.view().auth.units().get(a).is_none() {
            game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(1))
        } else {
            a
        };
        game.execute(
            &rules,
            Command::AttackUnit {
                attacker: atk,
                defender: d,
            },
        )
        .unwrap();
        if game.view().auth.units().get(d).is_some() {
            play_round(&mut game, &rules); // one attack per turn
        }
    }
    assert!(
        game.view().auth.units().get(d).is_none(),
        "defender must die"
    );
    assert!(
        !game.view().auth.players().is_alive(PlayerId(1)),
        "P1 eliminated"
    );
    assert_eq!(game.winner(), Some(PlayerId(0)));
    game.validate(&rules).unwrap();
}

#[test]
fn treasury_earns_income_and_pays_upkeep() {
    // Gold comes from the city (a +3 gold building here). The legacy
    // content has no handicap table: its units and buildings cost no
    // maintenance (maintenance of the classic content: tests/gold.rs).
    let (mut game, rules) = arena_with_building("market", "AddYield(yield_kind: Gold, amount: 3)");
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    game.debug_add_building(&rules, city, crate::ids::BuildingKindId(0));
    assert_eq!(game.view().auth.players().treasury_of(PlayerId(0)), 0);
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    assert_eq!(game.view().auth.players().treasury_of(PlayerId(0)), 3);
    let t1 = dims.to_tile(HexCoord { q: 4, r: 4 }).unwrap();
    for _ in 0..3 {
        game.debug_spawn_unit(&rules, PlayerId(0), t1, UnitKindId(1));
    }
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    // 3 + 3, no maintenance.
    assert_eq!(game.view().auth.players().treasury_of(PlayerId(0)), 6);
    for _ in 0..20 {
        game.debug_spawn_unit(&rules, PlayerId(0), t1, UnitKindId(1));
    }
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    assert_eq!(game.view().auth.players().treasury_of(PlayerId(0)), 9);
    game.validate(&rules).unwrap();
}

/// Healing (classic/docs/classic/rules.md): a unit that did not spend
/// moves this turn heals at the end of its owner's turn by where it rests —
/// 25 in a city, 20 in friendly land, 10 in neutral or enemy land. A unit
/// that moved or attacked does not heal; fortifying adds nothing.
#[test]
fn units_heal_by_where_they_rest() {
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let tile = |q, r| dims.to_tile(HexCoord { q, r }).unwrap();
    game.debug_found_city(&rules, PlayerId(0), tile(8, 8));
    game.debug_found_city(&rules, PlayerId(1), tile(3, 12));
    game.execute(&rules, Command::DeclareWar { player: PlayerId(0), enemy: PlayerId(1) })
        .unwrap();
    let spots = [tile(8, 8), tile(9, 8), tile(13, 3), tile(4, 12), tile(12, 8)];
    let units: Vec<_> = spots
        .iter()
        .map(|t| game.debug_spawn_unit(&rules, PlayerId(0), *t, UnitKindId(1)))
        .collect();
    for unit in &units {
        game.debug_set_unit_hp(*unit, 50);
    }
    game.execute(&rules, Command::Fortify { unit: units[1] }).unwrap();
    game.execute(&rules, Command::MoveUnit { unit: units[4], destination: tile(12, 7) })
        .unwrap();
    game.execute(&rules, Command::EndTurn { player: PlayerId(0) })
        .unwrap();
    let hp: Vec<u16> = units
        .iter()
        .map(|u| game.view().auth.units().get(*u).unwrap().core.hp)
        .collect();
    assert_eq!(hp, vec![75, 70, 60, 60, 50], "city, friendly (fortified), neutral, enemy, moved");
}

#[test]
fn classic_schedule_keeps_the_known_dependencies() {
    use crate::classic::{ClassicCityStep, ClassicStep};
    use crate::turn::resolution::{KernelCityStep, KernelStep, Step};
    let schedule = crate::classic::turn::SCHEDULE;
    schedule.validate();
    let position = |step| schedule.end_turn.iter().position(|(_, s)| *s == step).unwrap();
    let kernel = |k| position(Step::Kernel(k));
    let module = |m| position(Step::Module(m));
    // The golden age counts down before the cities read it (golden age step).
    assert!(module(ClassicStep::GoldenAge) < kernel(KernelStep::Cities));
    // Happiness is the snapshot the golden age fills from.
    assert!(module(ClassicStep::Happiness) < module(ClassicStep::GoldenAge));
    // Research agreements accumulate the science of the turn.
    assert!(kernel(KernelStep::Science) < module(ClassicStep::Diplomacy));
    let city = [
        Step::Module(ClassicCityStep::GreatPeople),
        Step::Kernel(KernelCityStep::Growth),
        Step::Kernel(KernelCityStep::Production),
        Step::Kernel(KernelCityStep::Borders),
    ];
    assert_eq!(schedule.city, city);
}

#[test]
#[should_panic(expected = "kernel steps repeated or out of order")]
fn a_schedule_may_not_reorder_kernel_steps() {
    use crate::turn::resolution::{KernelStep, Step, TurnSchedule};
    use crate::turn::TurnPhase::Start;
    let schedule: TurnSchedule<(), ()> = TurnSchedule {
        end_turn: &[
            (Start, Step::Kernel(KernelStep::RestoreMoves)),
            (Start, Step::Kernel(KernelStep::HealUnits)),
        ],
        city: &[],
    };
    schedule.validate();
}

#[test]
#[should_panic(expected = "a phase goes back")]
fn a_schedule_may_not_go_back_a_phase() {
    use crate::turn::resolution::{Step, TurnSchedule};
    use crate::turn::TurnPhase::{Resolution, Start};
    let schedule: TurnSchedule<u8, ()> = TurnSchedule {
        end_turn: &[(Resolution, Step::Module(0)), (Start, Step::Module(1))],
        city: &[],
    };
    schedule.validate();
}

#[test]
fn end_turn_returns_to_player_actions() {
    let (mut game, rules) = arena();
    let end = Command::EndTurn {
        player: PlayerId(0),
    };
    game.execute(&rules, end).unwrap();
    let phase = game.view().auth.turn().phase;
    assert_eq!(phase, crate::turn::TurnPhase::PlayerActions);
}

/// Three players with one unit each (all alive).
fn three_players() -> (GameState, CompiledRuleset, Vec<crate::ids::UnitHandle>) {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 3,
            width: 16,
            height: 16,
            rng_seed: 7,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let units = (0..3)
        .map(|p| {
            let tile = dims.to_tile(HexCoord { q: 2 + 5 * p, r: 8 }).unwrap();
            game.debug_spawn_unit(&rules, PlayerId(p as u8), tile, UnitKindId(0))
        })
        .collect();
    (game, rules, units)
}

/// Regression (P2): every player's EndTurn bumped the turn number, so one
/// round of N players counted N turns and the active player never changed.
#[test]
fn round_grows_once_after_every_player_ended_the_turn() {
    let (mut game, rules, _) = three_players();
    let turn = |game: &GameState| {
        let t = game.view().auth.turn();
        (t.round, game.view().auth.active_player())
    };
    assert_eq!(turn(&game), (0, PlayerId(0)));
    for (player, after) in [(0u8, (0, PlayerId(1))), (1, (0, PlayerId(2))), (2, (1, PlayerId(0)))] {
        game.execute(&rules, Command::EndTurn { player: PlayerId(player) })
            .unwrap();
        assert_eq!(turn(&game), after);
    }
}

#[test]
fn only_the_active_player_acts() {
    let (mut game, rules, units) = three_players();
    let before = authoritative_hash(&game);
    let early_end = game.execute(&rules, Command::EndTurn { player: PlayerId(1) });
    assert!(early_end.is_err(), "P1 cannot end P0's turn");
    let dims = game.view().auth.map().dimensions;
    let step = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let early_move = game.execute(
        &rules,
        Command::MoveUnit {
            unit: units[1],
            destination: step,
        },
    );
    assert!(early_move.is_err(), "P1 cannot move during P0's turn");
    assert_eq!(before, authoritative_hash(&game));
    game.execute(&rules, Command::EndTurn { player: PlayerId(0) })
        .unwrap();
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: units[1],
            destination: step,
        },
    )
    .expect("P1 moves on its own turn");
}

#[test]
fn eliminated_players_are_skipped() {
    let (mut game, rules, _) = three_players();
    // P1 has nothing left: its slot in the round is skipped.
    let p1_units = game.view().auth.indexes().units_of(PlayerId(1)).to_vec();
    let dims = game.view().auth.map().dimensions;
    game.debug_meet(PlayerId(0), PlayerId(1));
    game.execute(
        &rules,
        Command::DeclareWar {
            player: PlayerId(0),
            enemy: PlayerId(1),
        },
    )
    .unwrap();
    let next_to_p1 = dims.to_tile(HexCoord { q: 6, r: 8 }).unwrap();
    let mut killer = game.debug_spawn_unit(&rules, PlayerId(0), next_to_p1, UnitKindId(0));
    for _ in 0..40 {
        if game.view().auth.units().get(p1_units[0]).is_none() {
            break;
        }
        if game.view().auth.units().get(killer).is_none() {
            killer = game.debug_spawn_unit(&rules, PlayerId(0), next_to_p1, UnitKindId(0));
        }
        let _ = game.execute(
            &rules,
            Command::AttackUnit {
                attacker: killer,
                defender: p1_units[0],
            },
        );
        for p in 0..3u8 {
            let _ = game.execute(&rules, Command::EndTurn { player: PlayerId(p) });
        }
    }
    assert!(!game.view().auth.players().is_alive(PlayerId(1)));
    let round = game.view().auth.turn().round;
    assert_eq!(game.view().auth.active_player(), PlayerId(0));
    game.execute(&rules, Command::EndTurn { player: PlayerId(0) })
        .unwrap();
    assert_eq!(game.view().auth.active_player(), PlayerId(2));
    game.execute(&rules, Command::EndTurn { player: PlayerId(2) })
        .unwrap();
    assert_eq!(game.view().auth.turn().round, round + 1);
}
