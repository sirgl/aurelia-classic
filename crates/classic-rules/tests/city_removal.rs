//! A destroyed city leaves no record behind (`remove_city`): nothing in the world state names it
//! after (`world::references::references_to_city`), and a save made after it loads and plays on
//! alike. The class: state that holds a `CityHandle` and outlives the city; a save could not be
//! loaded when a theft record named a razed city, or a religion a nuked holy city.
//!
//! The paths that destroy a city, each asking `combat::city::can_be_destroyed` (Unciv
//! `City.canBeDestroyed`): the decision to raze a captured city (`validate_decide_city`), a razing
//! city that burns down (`burn_city`), a nuke (`nuke_city`). A holy city survives each.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::command::CityDecision;
use classic_rules::ids::{BeliefId, CityHandle, PlayerId, TechId, TileId, UnitKindId};
use classic_rules::rules::content::ReligionId;
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::save_codec::{from_bytes, to_bytes};
use classic_rules::scenario::{generate_new_game_with, NewGameOptions, ScenarioConfig};
use classic_rules::world::references::references_to_city;
use classic_rules::world::relations::DealItem;
use classic_rules::world::state::GameConfig;
use classic_rules::world::victory::{VictoryKind, VictoryOptions};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap() as u16)
}

fn end_round(game: &mut GameState, rules: &CompiledRuleset) {
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    while game.view().auth.active_player() != player {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

/// The save of `game` as bytes, read back: it loads, hashes alike, and plays `rounds` more rounds
/// alike (every seat ends its turns).
fn round_trip(game: &GameState, rules: &CompiledRuleset, rounds: usize) {
    let bytes = to_bytes(&classic_rules::persistence::encode::encode(game, rules));
    let save = from_bytes(&bytes).expect("the bytes read");
    let mut loaded = classic_rules::persistence::decode::decode(&save, rules).expect("the save loads");
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(game), "the loaded game is the game");
    let mut game = game.clone();
    for _ in 0..rounds {
        end_round(&mut game, rules);
        end_round(&mut loaded, rules);
    }
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game), "the loaded game plays on alike");
}

/// P0 has a spy in the second city of P1 until a theft there is found out (the reviewer's
/// scene), and P1 offers that city to P0.
fn theft_in_second_city(seed: u64) -> Option<(GameState, CompiledRuleset, CityHandle)> {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: seed });
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    let second = game.debug_found_city(&rules, P1, tile(&game, 24, 8));
    game.debug_meet_all();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/acoustics"));
    game.debug_learn(&rules, P1, tech(&rules, "core:tech/agriculture"));
    end_round(&mut game, &rules);
    until_active(&mut game, &rules, P0);
    game.execute(&rules, Command::MoveSpy { player: P0, spy: 0, city: Some(second), diplomat: false }).unwrap();
    for _ in 0..60 {
        end_round(&mut game, &rules);
        if game.view().auth.module().espionage().thefts_against(P1).next().is_some() {
            return Some((game, rules, second));
        }
    }
    None
}

/// The city of a found-out theft, offered in a deal and named by a spy, is taken and razed: no
/// record names it, and the save loads (it failed with "espionage: theft" before).
#[test]
fn a_razed_city_of_a_theft_and_an_offer_leaves_no_record_and_the_save_loads() {
    let (mut game, rules, city) = (0..80).find_map(theft_in_second_city).expect("a seed with a found-out theft");
    until_active(&mut game, &rules, P1);
    game.execute(&rules, Command::ProposeDeal { from: P1, to: P0, items: vec![(P1, DealItem::City(city))] }).unwrap();
    until_active(&mut game, &rules, P0);
    game.debug_capture_city(&rules, city, P0);
    game.execute(&rules, Command::DecideCity { player: P0, city, decision: CityDecision::Raze }).unwrap();
    assert!(!references_to_city(game.view().auth, city).is_empty(), "the search finds the city while it burns");
    for _ in 0..40 {
        end_round(&mut game, &rules);
        if game.view().auth.cities().get(city).is_none() {
            break;
        }
    }
    assert!(game.view().auth.cities().get(city).is_none(), "the city is razed");
    assert_eq!(references_to_city(game.view().auth, city), Vec::<String>::new());
    assert!(game.view().auth.module().relations().offers().is_empty(), "the offer of the city is withdrawn");
    round_trip(&game, &rules, 5);
}

/// A holy city cannot be razed (Unciv `City.canBeDestroyed`): the religion keeps its holy city.
#[test]
fn a_holy_city_cannot_be_razed() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 7 });
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    let holy = game.debug_found_city(&rules, P0, tile(&game, 9, 3));
    game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    game.debug_meet_all();
    until_active(&mut game, &rules, P0);
    found_religion(&mut game, &rules, P0, holy);
    until_active(&mut game, &rules, P1);
    game.debug_capture_city(&rules, holy, P1);
    let raze = game.execute(&rules, Command::DecideCity { player: P1, city: holy, decision: CityDecision::Raze });
    assert!(raze.is_err(), "{raze:?}");
    game.execute(&rules, Command::DecideCity { player: P1, city: holy, decision: CityDecision::Puppet }).unwrap();
    assert_eq!(game.view().auth.module().religion().religions()[0].holy_city, holy);
}

/// P0 founds a religion in its second city (`holy`), and P1, at war with it, has a nuclear missile
/// in its own city three tiles away.
fn holy_city_under_a_missile() -> (GameState, CompiledRuleset, CityHandle, classic_rules::ids::UnitHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 7 });
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    let holy = game.debug_found_city(&rules, P0, tile(&game, 9, 3));
    game.debug_found_city(&rules, P1, tile(&game, 12, 3));
    game.debug_meet_all();
    until_active(&mut game, &rules, P0);
    found_religion(&mut game, &rules, P0, holy);
    game.debug_set_population(&rules, holy, 1);
    until_active(&mut game, &rules, P1);
    game.execute(&rules, Command::DeclareWar { player: P1, enemy: P0 }).unwrap();
    let missile = game.debug_spawn_unit(&rules, P1, tile(&game, 12, 3), unit_kind(&rules, "core:unit/nuclear-missile"));
    let at = game.view().auth.cities().get(holy).unwrap().tile;
    game.debug_reveal(P1, &[at]);
    (game, rules, holy, missile)
}

/// The reviewer's scene: a nuke that destroys a city of one citizen leaves a holy city standing
/// (Unciv `Nuke.doNukeExplosionDamageToCity`: `canBeDestroyed(true)`), and the save loads.
#[test]
fn a_nuke_leaves_a_holy_city_standing_and_the_save_loads() {
    let (mut game, rules, holy, missile) = holy_city_under_a_missile();
    let at = game.view().auth.cities().get(holy).unwrap().tile;
    game.execute(&rules, Command::Nuke { unit: missile, target: at }).unwrap();
    assert!(game.view().auth.cities().get(holy).is_some(), "the holy city stands");
    assert_eq!(game.view().auth.module().religion().religions()[0].holy_city, holy);
    round_trip(&game, &rules, 3);
}

/// The same nuke destroys a city that is not holy: nothing names it after, and the save loads.
#[test]
fn a_nuke_destroys_a_small_city_and_leaves_no_record() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 7 });
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    let small = game.debug_found_city(&rules, P0, tile(&game, 9, 3));
    game.debug_found_city(&rules, P1, tile(&game, 12, 3));
    game.debug_meet_all();
    game.debug_set_population(&rules, small, 1);
    until_active(&mut game, &rules, P1);
    game.execute(&rules, Command::DeclareWar { player: P1, enemy: P0 }).unwrap();
    let missile = game.debug_spawn_unit(&rules, P1, tile(&game, 12, 3), unit_kind(&rules, "core:unit/nuclear-missile"));
    let at = game.view().auth.cities().get(small).unwrap().tile;
    game.debug_reveal(P1, &[at]);
    game.execute(&rules, Command::Nuke { unit: missile, target: at }).unwrap();
    assert!(game.view().auth.cities().get(small).is_none(), "the city is destroyed");
    assert_eq!(references_to_city(game.view().auth, small), Vec::<String>::new());
    round_trip(&game, &rules, 3);
}

/// A burning city that becomes a holy city is not destroyed (Unciv `City.destroyCity` asks
/// `canBeDestroyed()`): it keeps its last citizen and burns on; the save loads.
#[test]
fn a_burning_city_that_becomes_holy_keeps_its_last_citizen() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 7 });
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    let second = game.debug_found_city(&rules, P1, tile(&game, 24, 8));
    game.debug_meet_all();
    game.debug_set_population(&rules, second, 3);
    until_active(&mut game, &rules, P0);
    game.debug_capture_city(&rules, second, P0);
    game.execute(&rules, Command::DecideCity { player: P0, city: second, decision: CityDecision::Raze }).unwrap();
    found_religion(&mut game, &rules, P0, second);
    for _ in 0..8 {
        end_round(&mut game, &rules);
    }
    let core = game.view().auth.cities().get(second).expect("the holy city stands");
    assert!(core.population >= 1, "{}", core.population);
    assert_eq!(game.view().auth.module().religion().religions()[0].holy_city, second);
    round_trip(&game, &rules, 3);
}

/// `player` founds Buddhism in its city `city` with a Great Prophet.
fn found_religion(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, city: CityHandle) {
    let at = game.view().auth.cities().get(city).unwrap().tile;
    let prophet = game.debug_spawn_unit(rules, player, at, unit_kind(rules, "core:unit/great-prophet"));
    game.execute(
        rules,
        Command::FoundReligion {
            unit: prophet,
            religion: ReligionId(0),
            founder: belief(rules, "core:belief/founder/tithe"),
            follower: belief(rules, "core:belief/follower/pagodas"),
            pantheon: Some(belief(rules, "core:belief/pantheon/ancestor-worship")),
            bonus: None,
        },
    )
    .unwrap();
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn belief(rules: &CompiledRuleset, key: &str) -> BeliefId {
    BeliefId(rules.module.beliefs.iter().position(|b| b.key == key).unwrap() as u16)
}

/// The guard of the class: a played world, several cities of different nations taken and razed;
/// at each removal and 20 rounds later nothing names a removed city, and the save loads and plays
/// on alike.
#[test]
fn nothing_names_a_razed_city_in_a_played_world() {
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig { seed: 0x5EED_C17E, players: 4, width: 40, height: 28, cities: 0, units: 0, water_ratio: 15 };
    let victories = VictoryOptions { disabled: VictoryKind::Domination.bit(), ..VictoryOptions::default() };
    let options = NewGameOptions { city_states: 2, victories, ..NewGameOptions::default() };
    let mut game = generate_new_game_with(&rules, &config, None, options);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(Variant::Standard));
    play(&mut game, &rules, &mut driver, 120);
    let mut razed = Vec::new();
    for victim in 0..4u8 {
        let victim = PlayerId(victim);
        let conqueror = PlayerId((victim.0 + 1) % 4);
        let auth = game.view().auth;
        let target = auth.indexes().cities_of(victim).iter().copied().find(|c| {
            let core = auth.cities().get(*c);
            core.is_some_and(|core| !core.original_capital) && auth.module().religion().holy_city_of(*c).is_none()
        });
        let Some(city) = target else { continue };
        until_active(&mut game, &rules, conqueror);
        game.debug_capture_city(&rules, city, conqueror);
        if game.execute(&rules, Command::DecideCity { player: conqueror, city, decision: CityDecision::Raze }).is_ok() {
            razed.push(city);
        }
    }
    assert!(razed.len() >= 2, "the world has cities to raze: {razed:?}");
    for _ in 0..60 {
        if razed.iter().all(|c| game.view().auth.cities().get(*c).is_none()) {
            break;
        }
        play(&mut game, &rules, &mut driver, 1);
        for city in razed.iter().filter(|c| game.view().auth.cities().get(**c).is_none()) {
            assert_eq!(references_to_city(game.view().auth, *city), Vec::<String>::new(), "{city:?} at its removal");
        }
    }
    let gone: Vec<CityHandle> = razed.iter().copied().filter(|c| game.view().auth.cities().get(*c).is_none()).collect();
    assert!(gone.len() >= 2, "the razed cities burnt down: {gone:?} of {razed:?}");
    play(&mut game, &rules, &mut driver, 20);
    for city in &gone {
        assert_eq!(references_to_city(game.view().auth, *city), Vec::<String>::new(), "{city:?} 20 rounds later");
    }
    round_trip(&game, &rules, 5);
    game.validate(&rules).unwrap();
}

fn play(game: &mut GameState, rules: &CompiledRuleset, driver: &mut AiDriver<classic_rules::classic::Classic, ClassicMind>, rounds: u32) {
    let mut done = 0;
    while done < rounds {
        if driver.run(game, rules, 100).round_done {
            done += 1;
        }
    }
}
