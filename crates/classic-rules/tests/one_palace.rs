//! One palace per nation, whatever way a city changes owner (`world::capital`; Unciv 4.22.10
//! `CityConquestFunctions.moveToCiv`, `Civilization.moveCapitalToNextLargest`).
//!
//! The class: a rule that keeps a fact about a nation (its capital) at the cities, and a path that
//! moves a city without moving the fact. `transfer_city` kept the palace: a bought city-state, a
//! city of a deal and a liberated city arrived with its old palace, so a nation held several and
//! `capital_of` (the first by index) named a bought overseas city: Austria built all six parts of the
//! spaceship and could not install them (casebook W1, 29 of 1200 seats). Each path has its test, the
//! whole games check the invariant every round, and `ClassicWorld::validate` checks it everywhere.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::combat::city::capital_of;
use classic_rules::command::CityDecision;
use classic_rules::ids::{CityHandle, PlayerId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::{BuildingKind, CityStateId};
use classic_rules::world::city_states::Personality;
use classic_rules::save_codec::{from_bytes, to_bytes};
use classic_rules::scenario::{generate_new_game_with, NewGameOptions, ScenarioConfig};
use classic_rules::world::capital::{palace_building, palace_cities, palace_problem};
use classic_rules::world::city::CityStatus;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::world::relations::DealItem;
use classic_rules::world::state::GameConfig;
use classic_rules::world::victory::{VictoryKind, VictoryOptions};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const CS: PlayerId = PlayerId(2);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    while game.view().auth.active_player() != player {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

/// Two nations, each with a capital, in a small world.
fn two_nations() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 7 });
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    game.debug_meet_all();
    (game, rules)
}

fn palaces(game: &GameState, rules: &CompiledRuleset, owner: PlayerId) -> Vec<CityHandle> {
    palace_cities(game.view().auth, rules, owner)
}

/// Every nation with cities holds one palace, and the world validates.
fn assert_one_palace_each(game: &GameState, rules: &CompiledRuleset) {
    assert_eq!(palace_problem(game.view().auth, rules), None);
    game.validate(rules).unwrap();
}

/// A city that changes owner brings no palace, and the old owner's next city takes it.
#[test]
fn a_conquered_capital_hands_its_palace_to_the_largest_city_that_is_not_a_puppet() {
    let (mut game, rules) = two_nations();
    let capital = game.view().auth.indexes().cities_of(P1)[0];
    let small = game.debug_found_city(&rules, P1, tile(&game, 24, 8));
    let puppet = game.debug_found_city(&rules, P1, tile(&game, 20, 4));
    game.debug_set_population(&rules, small, 3);
    game.debug_set_population(&rules, puppet, 6);
    game.debug_set_city_status(puppet, CityStatus::Puppet);
    until_active(&mut game, &rules, P0);
    game.debug_capture_city(&rules, capital, P0);
    assert_eq!(palaces(&game, &rules, P1), vec![small], "the largest non-puppet city is the capital now");
    assert_eq!(capital_of(game.view().auth, &rules, P1), Some(small));
    assert_eq!(palaces(&game, &rules, P0), vec![game.view().auth.indexes().cities_of(P0)[0]], "the conqueror keeps its own");
    assert_eq!(game.view().auth.cities().get(puppet).unwrap().status, CityStatus::Puppet, "a puppet stays one");
    assert_one_palace_each(&game, &rules);
}

/// With only puppets left, the largest becomes the capital and is annexed (Unciv `annexCity`).
#[test]
fn with_only_puppets_left_the_largest_is_annexed_and_gets_the_palace() {
    let (mut game, rules) = two_nations();
    let capital = game.view().auth.indexes().cities_of(P1)[0];
    let small = game.debug_found_city(&rules, P1, tile(&game, 24, 8));
    let large = game.debug_found_city(&rules, P1, tile(&game, 20, 4));
    game.debug_set_population(&rules, small, 2);
    game.debug_set_population(&rules, large, 5);
    game.debug_set_city_status(small, CityStatus::Puppet);
    game.debug_set_city_status(large, CityStatus::Puppet);
    until_active(&mut game, &rules, P0);
    game.debug_capture_city(&rules, capital, P0);
    assert_eq!(palaces(&game, &rules, P1), vec![large]);
    assert_eq!(game.view().auth.cities().get(large).unwrap().status, CityStatus::Occupied);
    assert_eq!(game.view().auth.cities().get(small).unwrap().status, CityStatus::Puppet);
    assert_one_palace_each(&game, &rules);
}

/// A nation whose last city falls has none; its captor has one palace, not two.
#[test]
fn the_last_city_of_a_nation_takes_its_palace_with_it_and_the_captor_has_one() {
    let (mut game, rules) = two_nations();
    let only = game.view().auth.indexes().cities_of(P1)[0];
    until_active(&mut game, &rules, P0);
    game.debug_capture_city(&rules, only, P0);
    assert!(game.view().auth.indexes().cities_of(P1).is_empty());
    assert_eq!(palaces(&game, &rules, P0).len(), 1);
    let captured = game.view().auth.cities().get(only).unwrap();
    assert!(!classic_rules::world::capital::holds_palace(&rules, captured), "the palace is not the prize");
    assert_one_palace_each(&game, &rules);
}

/// Buying a city-state: its cities arrive without its palace; the buyer's capital stays.
#[test]
fn a_bought_city_state_brings_no_palace() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 3, width: 30, height: 20, rng_seed: 9 });
    let key = "core:city-state/byblos";
    let id = rules.module.city_states.iter().position(|c| c.key == key).unwrap();
    game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(CityStateId(id as u16))));
    game.debug_set_personality(CS, Personality::Neutral);
    let home = game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    let first = game.debug_found_city(&rules, CS, tile(&game, 13, 9));
    let second = game.debug_found_city(&rules, CS, tile(&game, 13, 14));
    game.debug_meet_all();
    let austria = rules.nations.iter().position(|n| n.key == "core:nation/austria").unwrap();
    game.debug_set_player_nation(P0, austria as u8);
    assert_eq!(palaces(&game, &rules, CS), vec![first], "the city-state has a palace");
    game.debug_set_influence(&rules, CS, P0, 80);
    game.debug_earn(P0, 5000);
    until_active(&mut game, &rules, CS);
    for _ in 0..6 {
        until_active(&mut game, &rules, CS);
        game.execute(&rules, Command::EndTurn { player: CS }).unwrap();
    }
    until_active(&mut game, &rules, P0);
    game.execute(&rules, Command::BuyoutCityState { player: P0, city_state: CS }).unwrap();
    assert_eq!(palaces(&game, &rules, P0), vec![home], "one palace, the buyer's own");
    assert_eq!(capital_of(game.view().auth, &rules, P0), Some(home));
    for bought in [first, second] {
        assert!(!classic_rules::world::capital::holds_palace(&rules, game.view().auth.cities().get(bought).unwrap()));
    }
    assert_one_palace_each(&game, &rules);
}

/// A city of a deal arrives without the palace it held (the capital is not offered by the rules; the
/// harness gives it away to put the transfer itself under test).
#[test]
fn a_city_given_in_a_deal_brings_no_palace_and_the_giver_gets_a_new_one() {
    let (mut game, rules) = two_nations();
    let capital = game.view().auth.indexes().cities_of(P1)[0];
    let second = game.debug_found_city(&rules, P1, tile(&game, 24, 8));
    game.debug_deal_item(&rules, P1, P0, DealItem::City(capital)).unwrap();
    assert_eq!(palaces(&game, &rules, P1), vec![second]);
    assert_eq!(game.view().auth.cities().get(capital).unwrap().owner, P0);
    assert_eq!(palaces(&game, &rules, P0), vec![game.view().auth.indexes().cities_of(P0)[0]]);
    assert_one_palace_each(&game, &rules);
}

/// A liberated city returns to its founder without a palace when the founder has one; a founder
/// with no city gets one in it (Unciv `liberateCity`, "Resurrection!").
#[test]
fn a_liberated_city_comes_back_without_a_palace() {
    let (mut game, rules) = two_nations();
    let second = game.debug_found_city(&rules, P1, tile(&game, 24, 8));
    until_active(&mut game, &rules, P0);
    game.debug_capture_city(&rules, second, P0);
    game.execute(&rules, Command::DecideCity { player: P0, city: second, decision: CityDecision::Liberate }).unwrap();
    assert_eq!(game.view().auth.cities().get(second).unwrap().owner, P1);
    assert_eq!(palaces(&game, &rules, P1), vec![game.view().auth.indexes().cities_of(P1)[0]]);
    assert_one_palace_each(&game, &rules);
}

/// A save of an older build: a nation with two palaces and a nation with none load with one each.
/// The nation keeps the palace of its original capital; the one without gets it there.
#[test]
fn a_save_with_several_palaces_or_none_loads_with_one() {
    let (mut game, rules) = two_nations();
    let palace = palace_building(&rules).unwrap();
    let original = game.view().auth.indexes().cities_of(P0)[0];
    let second = game.debug_found_city(&rules, P0, tile(&game, 9, 3));
    game.debug_add_building(&rules, second, palace);
    let p1_capital = game.view().auth.indexes().cities_of(P1)[0];
    game.debug_found_city(&rules, P1, tile(&game, 24, 8));
    game.debug_remove_building(p1_capital, palace);
    assert!(palace_problem(game.view().auth, &rules).is_some(), "the scene is broken");
    let bytes = to_bytes(&classic_rules::persistence::encode::encode(&game, &rules));
    let save = from_bytes(&bytes).expect("the bytes read");
    let loaded = classic_rules::persistence::decode::decode(&save, &rules).expect("the save loads");
    assert_eq!(palaces(&loaded, &rules, P0), vec![original]);
    assert_eq!(palaces(&loaded, &rules, P1), vec![p1_capital]);
    assert_one_palace_each(&loaded, &rules);
}

/// A world that keeps the invariant loads as it is: nothing is repaired in it.
#[test]
fn a_valid_world_loads_unchanged() {
    let (mut game, rules) = two_nations();
    game.debug_found_city(&rules, P0, tile(&game, 9, 3));
    let bytes = to_bytes(&classic_rules::persistence::encode::encode(&game, &rules));
    let save = from_bytes(&bytes).expect("the bytes read");
    let loaded = classic_rules::persistence::decode::decode(&save, &rules).expect("the save loads");
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
}

/// The palace is a building of kind Palace and only of it: no other building is lost with it.
#[test]
fn the_palace_is_the_only_building_a_transfer_takes_off() {
    let (mut game, rules) = two_nations();
    let capital = game.view().auth.indexes().cities_of(P1)[0];
    game.debug_found_city(&rules, P1, tile(&game, 24, 8));
    let monument = rules.buildings.iter().position(|b| b.kind == BuildingKind::Regular).unwrap();
    game.debug_add_building(&rules, capital, classic_rules::ids::BuildingKindId(monument as u16));
    game.debug_deal_item(&rules, P1, P0, DealItem::City(capital)).unwrap();
    let core = game.view().auth.cities().get(capital).unwrap();
    assert!(core.buildings.contains(&classic_rules::ids::BuildingKindId(monument as u16)));
    assert!(!classic_rules::world::capital::holds_palace(&rules, core));
}

fn play(game: &mut GameState, rules: &CompiledRuleset, driver: &mut AiDriver<classic_rules::classic::Classic, ClassicMind>, rounds: u32) {
    let mut done = 0;
    while done < rounds {
        if driver.run(game, rules, 100).round_done {
            done += 1;
            assert_eq!(palace_problem(game.view().auth, rules), None, "round {}", game.view().auth.turn().round);
        }
    }
}

/// Whole games with city-states: the AIs found, fight, trade and buy, and in between the harness
/// captures a capital, gives a city in a deal and liberates one. After every round each nation
/// with cities holds one palace.
#[test]
fn whole_games_keep_one_palace_per_nation_every_round() {
    let rules = compile_rules(core_rules()).unwrap();
    for seed in [0x5EED_C17E_u64, 0xA057_21A5] {
        let config = ScenarioConfig { seed, players: 4, width: 40, height: 28, cities: 0, units: 0, water_ratio: 15 };
        let victories = VictoryOptions { disabled: VictoryKind::Domination.bit(), ..VictoryOptions::default() };
        let options = NewGameOptions { city_states: 3, victories, ..NewGameOptions::default() };
        let mut game = generate_new_game_with(&rules, &config, None, options);
        let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
        let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(Variant::Standard));
        play(&mut game, &rules, &mut driver, 50);
        let mut moved = 0;
        for step in 0..6u8 {
            let victim = PlayerId(step % 4);
            let taker = PlayerId((step + 1) % 4);
            let auth = game.view().auth;
            let Some(capital) = auth.indexes().cities_of(victim).first().copied() else { continue };
            until_active(&mut game, &rules, taker);
            if step % 2 == 0 {
                game.debug_capture_city(&rules, capital, taker);
            } else if game.debug_deal_item(&rules, victim, taker, DealItem::City(capital)).is_err() {
                continue;
            }
            moved += 1;
            assert_one_palace_each(&game, &rules);
            play(&mut game, &rules, &mut driver, 5);
        }
        assert!(moved >= 3, "the games moved capitals: {moved}");
        assert_one_palace_each(&game, &rules);
    }
}
