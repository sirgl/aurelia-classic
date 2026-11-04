//! Game speeds (classic/docs/classic/rules.md "Game speeds"; Unciv Speeds.json):
//! the content's table, the modifiers each speed applies, the speed in saves,
//! and AI games at the fastest and the slowest speed.

use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::trace::Outcome;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{BuildingKindId, PlayerId, ProductionItem, TechId};
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game_with, NewGameOptions, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::rules::speeds::SpeedValues;

const P0: PlayerId = PlayerId(0);

fn rules() -> CompiledRuleset {
    compile_rules(classic_content::classic::classic_rules()).unwrap()
}

fn config() -> ScenarioConfig {
    ScenarioConfig { seed: 0x5_EED5, players: 4, width: 40, height: 28, cities: 0, units: 0, water_ratio: 20 }
}

/// A new game at `speed` (a speed's short name; "" Standard) with one city of player 0.
fn game_at(rules: &CompiledRuleset, speed: &str) -> GameState {
    let options = NewGameOptions { city_states: 2, ..NewGameOptions::default() };
    let options = if speed.is_empty() { options } else { options.at_speed(rules, speed).expect("a speed of the content") };
    let mut game = generate_new_game_with(rules, &config(), None, options);
    let settler = game.view().auth.indexes().units_of(P0)[0];
    let tile = game.view().auth.units().get(settler).unwrap().core.tile;
    game.debug_found_city(rules, P0, tile);
    game
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).expect(key) as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).expect(key) as u16)
}

/// What a game at `speed` costs and lasts: (Monument, Writing, a farm's
/// worker-turns ×100, the last round, a golden age's rounds).
fn measures(rules: &CompiledRuleset, speed: &str) -> (u32, u32, u32, u32, u16) {
    let game = game_at(rules, speed);
    let auth = game.view().auth;
    let city = auth.indexes().cities_of(P0)[0];
    let core = auth.cities().get(city).unwrap();
    let mods = classic_rules::effects::city::city_modifiers(auth, rules, core);
    let monument = mods.production_cost(rules, ProductionItem::Building(building(rules, "core:building/monument"))).unwrap();
    let writing = classic_rules::technology_cost(auth, rules, P0, tech(rules, "core:tech/writing"));
    let farm = rules.improvements.iter().find(|i| i.key == "core:improvement/farm").unwrap().work_time;
    let work = kernel::systems::improvements::speed_work_time(farm, auth.settings().speed_values().improvement_percent);
    let rounds = auth.module().victory().options().max_rounds;
    let golden = classic_rules::derived::happiness::golden_age_length(auth, rules, P0, 10);
    (monument, writing, work, rounds, golden)
}

#[test]
fn the_content_has_unciv_s_four_speeds_and_standard_is_neutral() {
    let rules = rules();
    let names: Vec<&str> = rules.speeds.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Quick", "Standard", "Epic", "Marathon"]);
    let lengths: Vec<u32> = rules.speeds.iter().map(|s| s.values.max_rounds()).collect();
    assert_eq!(lengths, [330, 500, 750, 1500]);
    let standard = rules.speed("core:speed/standard").unwrap();
    assert_eq!(standard.values, SpeedValues::STANDARD, "the rule constants are written at the content's Standard");
    // A Standard game keeps no speed: its save and hash are the ones of a game of before speeds.
    let game = game_at(&rules, "standard");
    assert!(game.view().auth.settings().speed.is_empty());
    assert_eq!(authoritative_hash(&game), authoritative_hash(&game_at(&rules, "")));
}

#[test]
fn each_speed_scales_costs_build_times_length_and_golden_ages() {
    let rules = rules();
    let standard = measures(&rules, "standard");
    // The Standard numbers: 500 rounds, a golden age of 10 rounds.
    assert_eq!(standard.3, 500);
    assert_eq!(standard.4, 10);
    let quick = measures(&rules, "quick");
    let epic = measures(&rules, "epic");
    let marathon = measures(&rules, "marathon");
    let scaled = |x: u32, percent: u32| x * percent / 100;
    assert_eq!(quick.0, scaled(standard.0, 67), "production × 67 %");
    assert_eq!(epic.0, scaled(standard.0, 150), "production × 150 %");
    assert_eq!(marathon.0, scaled(standard.0, 300), "production × 300 %");
    for (m, percent) in [(&quick, 67), (&epic, 150), (&marathon, 300)] {
        let expected = scaled(standard.1, percent);
        assert!(m.1.abs_diff(expected) <= 1, "science × {percent} %: {} for {expected}", m.1);
    }
    // Whole worker-turns, rounded: a farm of 7 turns takes 5, 11 (10.5 rounded up) and 21.
    let turns = |work: u32, percent: u32| (work * percent / 100 + 50) / 100 * 100;
    assert_eq!(standard.2, 700, "a farm takes 7 worker-turns at Standard (Unciv: TileImprovements.json)");
    assert_eq!((quick.2, epic.2, marathon.2), (turns(700, 67), turns(700, 150), turns(700, 300)));
    assert_eq!((quick.2, epic.2, marathon.2), (500, 1100, 2100));
    assert_eq!((quick.3, epic.3, marathon.3), (330, 750, 1500), "the time victory at the speed's last round");
    assert_eq!((quick.4, epic.4, marathon.4), (8, 12, 20), "a golden age × 80, 125, 200 %");
}

#[test]
fn a_save_keeps_the_speed_and_an_old_save_plays_at_standard() {
    use classic_rules::persistence::{decode::decode, encode::encode};
    let rules = rules();
    let game = game_at(&rules, "quick");
    assert_eq!(game.view().auth.settings().speed, "core:speed/quick");
    let bytes = classic_rules::save_codec::to_bytes(&encode(&game, &rules));
    let loaded = decode(&classic_rules::save_codec::from_bytes(&bytes).unwrap(), &rules).unwrap();
    assert_eq!(loaded.view().auth.settings().speed, "core:speed/quick");
    assert_eq!(loaded.view().auth.settings().speed_values().max_rounds(), 330);
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_ne!(authoritative_hash(&game), authoritative_hash(&game_at(&rules, "")), "the speed is part of the truth");

    // A save of before speeds (codec 22 and older) plays at Standard.
    let main: &[u8] = include_bytes!("fixtures/main-b613023c.aurelia");
    let save = classic_rules::save_codec::from_bytes(main).unwrap();
    let (old, _) = classic_rules::persistence::decode::decode_with(&save, &rules, classic_rules::persistence::decode::RulesMatch::AnyRules).unwrap();
    assert!(old.view().auth.settings().speed.is_empty());
    assert_eq!(old.view().auth.settings().speed_values(), SpeedValues::STANDARD);
}

/// An AI game of 100 rounds at `speed`: the rules' rejections of the AI's
/// orders, and the round reached.
fn ai_game(speed: &str) -> (usize, u32) {
    let rules = rules();
    let options = NewGameOptions { city_states: 2, ..NewGameOptions::default() }.at_speed(&rules, speed).unwrap();
    let mut game = generate_new_game_with(&rules, &config(), None, options);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(Variant::Standard));
    driver.set_trace_capacity(1_000_000);
    while game.view().auth.turn().round < 100 && !game.is_over() {
        driver.run(&mut game, &rules, u32::MAX);
    }
    game.validate(&rules).unwrap();
    let refused: Vec<_> = driver.trace().entries().filter(|e| matches!(e.outcome, Outcome::Rejected(_))).collect();
    for e in refused.iter().take(5) {
        eprintln!("{speed}: refused {e} {:?}", e.command);
    }
    (refused.len(), game.view().auth.turn().round)
}

#[test]
fn quick_and_marathon_ai_games_run_100_rounds_without_refusals() {
    for speed in ["quick", "marathon"] {
        let (refused, round) = ai_game(speed);
        assert_eq!(round, 100, "{speed}");
        assert_eq!(refused, 0, "{speed}: the AI orders only what the rules accept");
    }
}

/// The speeds table is the only change of the content's fingerprint since
/// the edition before speeds (`Classic::EARLIER_EDITIONS`): without it the
/// rules hash as they did.
#[test]
fn without_speeds_the_rules_hash_as_before_speeds() {
    let mut rules = rules();
    rules.speeds.clear();
    assert_eq!(rules.hash(), BEFORE_SPEEDS_FINGERPRINT);
}

/// `CompiledRuleset::hash` of the classic content before the speeds table.
const BEFORE_SPEEDS_FINGERPRINT: u64 = 14820141147641077048;
