//! Work boats of the game's AI (`ai::sea_work`, classic/docs/ai-economy.md): which
//! variants build them, and that a game on a watery map has some built by the
//! AI that has them and none by its control `Variant::Landwork`.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::sea_work::is_sea_builder;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::UnitKindId;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::compile_rules;
use kernel::ai::driver::AiDriver;

#[test]
fn the_game_ai_builds_work_boats_and_its_control_does_not() {
    assert!(Variant::Standard.sea_work() && Variant::Planner.sea_work() && Variant::Closed.sea_work());
    assert!(!Variant::Landwork.sea_work() && !Variant::Allocator.sea_work());
    assert_eq!(Variant::from_name("landwork"), Some(Variant::Landwork));
}

/// The one builder of the sea in the ruleset is the unit that is a builder and
/// sails (the work boat), found by data.
#[test]
fn a_builder_of_the_sea_is_found_by_data() {
    let rules = compile_rules(core_rules()).unwrap();
    let boats: Vec<&str> = (0..rules.units.len()).filter(|i| is_sea_builder(&rules, UnitKindId(*i as u16))).map(|i| rules.units[i].name.as_str()).collect();
    assert_eq!(boats, ["Work Boat"]);
}

/// Work boats ordered by the AIs of a 4-player game on a watery map in `rounds`.
fn boats_ordered(variant: Variant, rounds: u32) -> usize {
    let rules = compile_rules(core_rules()).unwrap();
    let boat = (0..rules.units.len()).find(|i| is_sea_builder(&rules, UnitKindId(*i as u16))).unwrap();
    let config = ScenarioConfig { seed: 0x5ea_0001, players: 4, width: 44, height: 30, cities: 0, units: 0, water_ratio: 45 };
    let mut game = generate_new_game(&rules, &config);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(variant));
    driver.set_trace_capacity(2_000_000);
    while game.view().auth.turn().round < rounds && game.winner().is_none() {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let needle = format!("Unit(UnitKindId({boat}))");
    driver
        .trace()
        .entries()
        .filter(|e| e.command.as_ref().is_some_and(|c| format!("{c:?}").contains("EnqueueProduction") && format!("{c:?}").contains(&needle)))
        .count()
}

#[test]
fn a_game_on_a_watery_map_has_work_boats_only_with_the_work() {
    assert!(boats_ordered(Variant::Standard, 220) > 0);
    assert_eq!(boats_ordered(Variant::Landwork, 220), 0);
}
