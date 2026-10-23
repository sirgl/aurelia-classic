//! Espionage on the classic ruleset (classic/docs/classic/rules.md): spies of the eras,
//! travel and surveillance, stealing a technology, rigging a city-state
//! election, a coup, diplomats sent home by war, save/load, the AI.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::espionage::placement_orders;
use classic_rules::ids::{CityHandle, PlayerId, TechId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::espionage::SpyState;
use classic_rules::world::player::PlayerKind;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};
use classic_rules::world::player::ClassicKind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const CS: PlayerId = PlayerId(2);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
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

/// Two civilizations and a city-state, met; P0 enters the Renaissance.
fn world() -> (GameState, CompiledRuleset, CityHandle, CityHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 3,
            width: 30,
            height: 20,
            rng_seed: 6,
        },
    );
    game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    let theirs = game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    let minor = game.debug_found_city(&rules, CS, tile(&game, 13, 9));
    game.debug_meet_all();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/acoustics"));
    (game, rules, theirs, minor)
}

fn end_round(game: &mut GameState, rules: &CompiledRuleset) {
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active })
            .unwrap();
    }
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..4 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, Command::EndTurn { player: active })
            .unwrap();
    }
    panic!("{player:?} never became active");
}

fn spy(game: &GameState, player: PlayerId) -> classic_rules::world::espionage::Spy {
    game.view().auth.module().espionage().spies_of(player)[0]
}

fn send(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle, diplomat: bool) {
    until_active(game, rules, P0);
    game.execute(
        rules,
        Command::MoveSpy {
            player: P0,
            spy: 0,
            city: Some(city),
            diplomat,
        },
    )
    .unwrap();
}

#[test]
fn the_renaissance_gives_every_civilization_a_spy_and_industrial_one_more() {
    let (mut game, rules, _, _) = world();
    end_round(&mut game, &rules);
    let count = |g: &GameState, p| g.view().auth.module().espionage().spies_of(p).len();
    assert_eq!((count(&game, P0), count(&game, P1)), (1, 1));
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/archaeology"));
    end_round(&mut game, &rules);
    assert_eq!((count(&game, P0), count(&game, P1)), (2, 1));
}

#[test]
fn a_spy_travels_watches_and_steals_a_technology() {
    let (mut game, rules, theirs, _) = world();
    // Agriculture needs nothing: P0 could research it now.
    let agriculture = tech(&rules, "core:tech/agriculture");
    game.debug_learn(&rules, P1, agriculture);
    end_round(&mut game, &rules);
    send(&mut game, &rules, theirs, false);
    end_round(&mut game, &rules);
    assert_eq!(spy(&game, P0).state, SpyState::Surveillance);
    // Unciv: Spy.kt, a thief can be killed even without a counter-spy
    // (roll from 230 for a Recruit): a revived spy is sent again.
    for _ in 0..160 {
        if game.view().auth.module().espionage().steal_rights(P0, P1) > 0 {
            break;
        }
        if spy(&game, P0).state == SpyState::Unassigned {
            send(&mut game, &rules, theirs, false);
        }
        end_round(&mut game, &rules);
    }
    assert_eq!(
        game.view().auth.module().espionage().steal_rights(P0, P1),
        1
    );
    assert_eq!(
        spy(&game, P0).rank,
        1,
        "a successful theft promotes the spy"
    );
    until_active(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::StealTechnology {
            player: P0,
            from: P1,
            tech: agriculture,
        },
    )
    .unwrap();
    assert!(game
        .view()
        .auth
        .players()
        .techs_of(P0)
        .contains(&agriculture));
}

#[test]
fn a_rigged_election_brings_influence() {
    let (mut game, rules, _, minor) = world();
    end_round(&mut game, &rules);
    send(&mut game, &rules, minor, false);
    // Unciv: CityStateFunctions.kt, the spy (0 / 2 + 30 votes) runs against
    // a neutral party (20 votes); a win gives +20, then -1 a turn.
    let mut influence = 0;
    for _ in 0..80 {
        end_round(&mut game, &rules);
        influence = game.view().auth.module().city_states().influence(CS, P0);
        if influence >= 10 {
            break;
        }
    }
    assert!((10..=20).contains(&influence), "{influence}");
}

#[test]
fn a_coup_takes_the_alliance_or_kills_the_spy() {
    let (mut game, rules, _, minor) = world();
    game.debug_set_influence(&rules, CS, P1, 90);
    game.debug_set_influence(&rules, CS, P0, 80);
    end_round(&mut game, &rules);
    send(&mut game, &rules, minor, false);
    for _ in 0..4 {
        end_round(&mut game, &rules);
    }
    game.debug_set_influence(&rules, CS, P1, 90);
    game.debug_set_influence(&rules, CS, P0, 80);
    until_active(&mut game, &rules, P0);
    let chance = classic_rules::world::espionage::coup_chance(game.view().auth, P0, &spy(&game, P0));
    // Unciv: Spy.kt, 50 - (90 - 80) / 2 + 30 / 2 (Recruit, no defender).
    assert_eq!(chance, Some(60), "50 - 5 + 15");
    game.execute(&rules, Command::StageCoup { player: P0, spy: 0 })
        .unwrap();
    let auth = game.view().auth;
    let won = auth.module().city_states().ally(CS) == Some(P0);
    let lost = auth.module().city_states().influence(CS, P0) == 60
        && spy(&game, P0).state == SpyState::Dead;
    assert!(won || lost, "a coup either succeeds or costs the spy");
}

#[test]
fn war_sends_diplomats_home() {
    let (mut game, rules, theirs, _) = world();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/writing"));
    end_round(&mut game, &rules);
    send(&mut game, &rules, theirs, true);
    end_round(&mut game, &rules);
    assert_eq!(spy(&game, P0).state, SpyState::MakingIntroductions);
    until_active(&mut game, &rules, P0);
    game.execute(
        &rules,
        Command::DeclareWar {
            player: P0,
            enemy: P1,
        },
    )
    .unwrap();
    assert_eq!(spy(&game, P0).state, SpyState::Unassigned);
}

#[test]
fn espionage_survives_save_and_load_and_the_ai_places_spies() {
    let (mut game, rules, theirs, _) = world();
    end_round(&mut game, &rules);
    until_active(&mut game, &rules, P0);
    let orders = placement_orders(&game.ai_observation(&rules, P0), &rules);
    assert_eq!(orders.len(), 1, "{orders:?}");
    send(&mut game, &rules, theirs, false);
    end_round(&mut game, &rules);
    let save = classic_rules::persistence::encode::encode(&game, &rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_eq!(
        loaded.view().auth.module().espionage(),
        game.view().auth.module().espionage()
    );
}

#[test]
fn every_civilization_names_its_spies_from_its_own_list_and_numbers_the_second_round() {
    use classic_rules::world::espionage::spy_name;
    let rules = compile_rules(core_rules()).unwrap();
    assert_eq!(rules.civs.len(), 43);
    for (civ, def) in rules.civs.iter().enumerate() {
        let list = &def.module.spy_names;
        assert!(list.len() >= 3, "{} has {} spy names", def.name, list.len());
        let first_round: Vec<String> = (0..list.len()).map(|i| spy_name(&rules, civ as u8, i as u8)).collect();
        assert_eq!(&first_round, list, "{}: the list in turn", def.name);
        let again = spy_name(&rules, civ as u8, list.len() as u8);
        assert_eq!(again, format!("{} 2", list[0]), "{}: the second round is numbered", def.name);
        let third = spy_name(&rules, civ as u8, (list.len() * 2) as u8);
        assert_eq!(third, format!("{} 3", list[0]));
    }
    assert_eq!(spy_name(&rules, 200, 0), "Spy 1", "an unknown nation numbers its spies");
}

#[test]
fn a_spy_that_comes_back_takes_a_name_no_living_spy_holds() {
    let (mut game, rules, _, _) = world();
    end_round(&mut game, &rules);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/archaeology"));
    end_round(&mut game, &rules);
    let names = |g: &GameState| -> Vec<u8> { g.view().auth.module().espionage().spies_of(P0).iter().map(|s| s.name).collect() };
    assert_eq!(names(&game), [0, 1]);
    // The first spy dies; five rounds later a new one stands in its place.
    let mut save = classic_rules::persistence::encode::encode(&game, &rules);
    save.world.module.espionage.spies[P0.0 as usize][0].0 .2 = SpyState::Dead.tag();
    let mut game = classic_rules::persistence::decode::decode(&save, &rules).unwrap();
    for _ in 0..6 {
        end_round(&mut game, &rules);
    }
    let after = names(&game);
    assert_eq!(after.len(), 2);
    assert_eq!(after[1], 1);
    assert_ne!(after[0], after[1], "two living spies never share a name");
}
