//! The investment planner (`Variant::Planner`, classic/docs/ai-investment.md) and its
//! control `Variant::Closed`, on the classic rules: which variants plan, and that
//! they order only what the rules accept. The valuation itself is tested where
//! it lives (`ai::grand_strategy`, `ai::value`).

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::trace::Outcome;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::world::victory::{VictoryKind, VictoryOptions};
use classic_rules::{compile_rules, ClassicGame};
use kernel::ai::driver::AiDriver;

/// The planner is a named variant that plans; the others do not.
#[test]
fn only_the_planner_plans() {
    assert_eq!(Variant::from_name("planner"), Some(Variant::Planner));
    assert_eq!(Variant::from_name("closed"), Some(Variant::Closed));
    assert!(Variant::Planner.plans());
    assert_eq!(Variant::ALL.iter().filter(|v| v.plans()).count(), 1);
}

/// The game's AI judges an open game over an open horizon, its control
/// `closed` over the nominal length (`value::horizon`).
#[test]
fn only_the_closed_control_keeps_the_nominal_horizon() {
    use classic_rules::ai::value::SciencePricing;
    assert_eq!(Variant::Standard.science_pricing(), SciencePricing::Reach);
    assert_eq!(Variant::Closed.science_pricing(), SciencePricing::ReachClosed);
    assert_eq!(Variant::Planner.science_pricing(), SciencePricing::Aim);
}

/// Refused orders of one AI-only game of `rounds` with the time victory off
/// (the planner's open horizon is in play) and the rounds the game reached.
fn refused(variant: Variant, rounds: u32) -> (usize, u32) {
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig { seed: 0x1_7003, players: 4, width: 40, height: 28, cities: 0, units: 0, water_ratio: 20 };
    let mut game = generate_new_game(&rules, &config);
    // A nominal length of 60 rounds: from round 61 the open horizon is the only one.
    game.debug_set_victory_options(VictoryOptions { max_rounds: 60, ..VictoryOptions::default() }.with(VictoryKind::Time, false));
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(variant));
    driver.set_trace_capacity(1_000_000);
    while game.view().auth.turn().round < rounds && game.winner().is_none() {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let refused = driver
        .trace()
        .entries()
        .filter(|e| matches!(&e.outcome, Outcome::Rejected(err) if format!("{err:?}").contains("needs ")))
        .count();
    (refused, game.view().auth.turn().round)
}

/// Like the game's AI, the planner and its control order only what the rules
/// accept, and play on to the round asked for.
#[test]
fn the_planner_orders_only_what_the_rules_accept() {
    for variant in [Variant::Planner, Variant::Closed] {
        let (refused, round) = refused(variant, 120);
        assert_eq!((refused, round), (0, 120), "{variant:?}");
    }
}
