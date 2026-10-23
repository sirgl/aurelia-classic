//! A classic new game with every M4 track C option on — four AI civilizations,
//! four city-states, normal barbarians — played by the AI driver for 200
//! rounds: no panic, the world stays valid, barbarian camps appear, the
//! AI's rejected commands stay rare (under 5 %).

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::trace::Outcome;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::compile_rules;
use classic_rules::scenario::{generate_new_game_with, NewGameOptions, ScenarioConfig};
use classic_rules::world::barbarians::BarbarianMode;

const ROUNDS: u32 = 200;

#[test]
fn ai_civilizations_city_states_and_barbarians_play_200_rounds() {
    let rules = compile_rules(core_rules()).unwrap();
    let cfg = ScenarioConfig {
        seed: 21,
        players: 4,
        width: 56,
        height: 36,
        cities: 0,
        units: 0,
        water_ratio: 30,
    };
    let options = NewGameOptions {
        barbarians: BarbarianMode::Normal,
        city_states: 4,
        ..Default::default()
    };
    let mut game = generate_new_game_with(&rules, &cfg, None, options);
    let mut driver = AiDriver::new(
        AiSettings::for_rules(&rules, rules.default_difficulty()),
        Vec::new(),
    );
    driver.set_trace_capacity(1_000_000);
    let mut camps_seen = 0;
    // A civilization that knows a city-state and holds the gold of a gift.
    let mut could_gift = false;
    let mut met_minor = false;
    let mut rich_before = [false; 4];
    while game.view().auth.turn().round < ROUNDS && game.winner().is_none() {
        let report = driver.run(&mut game, &rules, u32::MAX);
        assert!(!report.waiting, "nobody is human: the driver never waits");
        camps_seen = camps_seen.max(game.view().auth.module().barbarians().camps().len());
        for p in 0..4u8 {
            let obs = game.ai_observation(&rules, classic_rules::ids::PlayerId(p));
            let gift = classic_rules::ai::city_states::GIFT_RESERVE + classic_rules::ai::city_states::SMALL_GIFT;
            met_minor |= !obs.city_states.is_empty();
            // The gold seen here includes the income of the end of turn, after
            // the advisor decided: only gold held over a whole turn (two
            // observations in a row) was there when it decided.
            let rich = !obs.city_states.is_empty() && obs.economy.treasury >= gift;
            could_gift |= rich && rich_before[p as usize];
            rich_before[p as usize] = rich;
        }
    }
    game.validate(&rules).unwrap();
    // Gifts given at any time (the influence of an early gift decays by
    // round 200).
    let gifts = driver
        .trace()
        .entries()
        .filter(|e| matches!(e.command, Some(classic_rules::Command::GiftGold { .. })))
        .filter(|e| matches!(e.outcome, Outcome::Applied))
        .count();
    let applied = driver.counters().commands_applied;
    let rejected = driver
        .trace()
        .entries()
        .filter(|e| matches!(e.outcome, Outcome::Rejected(_)))
        .count() as u64;
    let auth = game.view().auth;
    let cleared = auth.module().barbarians().cleared().len();
    eprintln!(
        "minors: round {}, applied {applied}, rejected {rejected}, camps max {camps_seen}, cleared now {cleared}",
        auth.turn().round
    );
    let states = auth.module().city_states();
    let best = (0..4u8)
        .flat_map(|m| (4..8u8).map(move |c| (c, m)))
        .map(|(c, m)| states.influence(classic_rules::ids::PlayerId(c), classic_rules::ids::PlayerId(m)))
        .max()
        .unwrap_or(0);
    eprintln!(
        "city-states: quests now {}, best influence {best}, allies {:?}",
        states.quests().len(),
        (4..8u8)
            .map(|c| states.ally(classic_rules::ids::PlayerId(c)))
            .collect::<Vec<_>>()
    );
    assert!(camps_seen > 0, "barbarian camps appeared");
    assert!(met_minor, "the nations met city-states");
    // With classic maintenance (M4-A) an empire may never hold the gold of a
    // gift in 200 rounds; one that did courted a city-state.
    assert!(best > 0 || gifts > 0 || !could_gift, "the AI courted a city-state");
    assert!(
        rejected * 20 <= applied,
        "rejections stay rare: {rejected} of {applied}"
    );
}
