//! Diplomacy in a simultaneous window (multiplayer, platform/docs/multiplayer.md "Conflicts"): every
//! diplomatic command is tied to the seat that gives it, answers included, and commands of two
//! seats that touch the same relation resolve by arrival (the first is applied, the second meets
//! the changed world and is refused or applied by the rules' own checks).

use classic_content::core::core_rules;
use classic_rules::ids::{PlayerId, TechId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::world::relations::DealItem;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(
        rules
            .technologies
            .iter()
            .position(|t| t.key == key)
            .unwrap() as u16,
    )
}

/// Three nations that met (their scouts stand together), each with a city far apart.
fn three_nations() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 3,
            width: 24,
            height: 16,
            rng_seed: 3,
        },
    );
    let scout = unit(&rules, "core:unit/scout");
    for (i, p) in [P0, P1, P2].into_iter().enumerate() {
        game.debug_spawn_unit(&rules, p, tile(&game, 10 + i as i32, 8), scout);
        game.debug_found_city(&rules, p, tile(&game, 2 + 9 * i as i32, 2));
        for key in [
            "core:tech/writing",
            "core:tech/civil-service",
            "core:tech/chivalry",
            "core:tech/education",
        ] {
            game.debug_learn(&rules, p, tech(&rules, key));
        }
    }
    (game, rules)
}

fn opened() -> (GameState, CompiledRuleset) {
    let (mut game, rules) = three_nations();
    game.open_window(0b111);
    (game, rules)
}

fn embassy_swap(from: PlayerId, to: PlayerId) -> Command {
    Command::ProposeDeal {
        from,
        to,
        items: vec![(from, DealItem::Embassy), (to, DealItem::Embassy)],
    }
}

#[test]
fn a_seat_cannot_answer_an_offer_made_to_another_seat() {
    let (mut game, rules) = opened();
    let id = game.view().auth.module().relations().next_offer_id();
    game.execute_as(P0, &rules, embassy_swap(P0, P1))
        .expect("seat 0 offers");
    let answer = |player| Command::RespondDeal {
        player,
        offer: id,
        accept: true,
    };
    // Seat 2 (a bystander) and seat 0 (the proposer) both try to accept for seat 1.
    for thief in [P2, P0] {
        let before = authoritative_hash(&game);
        let result = game.execute_as(thief, &rules, answer(P1));
        assert!(
            result.is_err(),
            "seat {} accepted an offer made to seat 1",
            thief.0
        );
        assert_eq!(
            authoritative_hash(&game),
            before,
            "a refused command leaves the world as it was"
        );
    }
    game.execute_as(P1, &rules, answer(P1))
        .expect("the receiver answers for itself");
}

#[test]
fn a_seat_cannot_propose_or_declare_war_in_the_name_of_another() {
    let (mut game, rules) = opened();
    assert!(
        game.execute_as(P0, &rules, embassy_swap(P1, P2)).is_err(),
        "an offer from seat 1, given by seat 0"
    );
    assert!(
        game.execute_as(
            P0,
            &rules,
            Command::DeclareWar {
                player: P1,
                enemy: P2
            }
        )
        .is_err(),
        "a war of seat 1, declared by seat 0"
    );
    assert!(
        game.execute_as(
            P2,
            &rules,
            Command::MakePeace {
                player: P0,
                enemy: P1
            }
        )
        .is_err(),
        "a peace of seat 0, made by seat 2"
    );
}

/// Two wars on the same nation, or two declarations on each other, in both arrival orders; the
/// journal of each replays to the hash of the game that played it.
fn declare(first: (PlayerId, PlayerId), second: (PlayerId, PlayerId)) -> (u64, bool, bool) {
    let (mut game, rules) = three_nations();
    let save =
        kernel::persistence::codec::to_bytes(&kernel::persistence::encode::encode(&game, &rules));
    let mut journal = game.begin_journal(&rules);
    game.open_window(0b111);
    let r1 = game.execute_as(
        first.0,
        &rules,
        Command::DeclareWar {
            player: first.0,
            enemy: first.1,
        },
    );
    let r2 = game.execute_as(
        second.0,
        &rules,
        Command::DeclareWar {
            player: second.0,
            enemy: second.1,
        },
    );
    journal.extend(game.take_journal());
    let replayed =
        kernel::replay::journal::replay(&save, &journal, &rules).expect("the journal replays");
    assert_eq!(
        authoritative_hash(&replayed.game),
        authoritative_hash(&game),
        "replay of the arrival order"
    );
    (authoritative_hash(&game), r1.is_ok(), r2.is_ok())
}

#[test]
fn two_wars_on_the_same_nation_both_stand_in_either_order() {
    // Seats 0 and 1 both go to war with seat 2: independent relations, both accepted. The two
    // arrival orders are two worlds (measured: the hashes differ; why was not investigated), each
    // deterministic and each replaying to its own hash, which is what the server needs.
    let a = declare((P0, P2), (P1, P2));
    let b = declare((P1, P2), (P0, P2));
    eprintln!("hash a {:x}, hash b {:x}", a.0, b.0);
    assert!(a.1 && a.2 && b.1 && b.2, "{a:?} {b:?}");
}

#[test]
fn two_declarations_on_each_other_the_first_applies_and_the_second_is_refused() {
    let (_, first, second) = declare((P0, P1), (P1, P0));
    assert!(first && !second, "({first}, {second})");
    let (_, first, second) = declare((P1, P0), (P0, P1));
    assert!(first && !second, "({first}, {second})");
}
