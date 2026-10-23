//! The science economy of the AI (`ai::value::SciencePricing::Reach`,
//! classic/docs/ai-allocator.md, "Science"): a beaker is priced by what the
//! technologies researched in the rounds left open. The rule reads the
//! declared effects of what a technology unlocks, never a name; its test on
//! a synthetic ruleset is in `ai::value`, this is the classic rules.

use classic_content::core::core_rules;
use classic_rules::ai::value::{PriceModel, Prices, SciencePricing};
use classic_rules::ai::variant::Variant;
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::world::state::GameConfig;
use classic_rules::ClassicGame;
use classic_rules::{compile_rules, GameState};

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// With what the classic technologies open, a beaker is worth several times
/// the score of the technologies alone, and the price the AI's other uses
/// of production and gold set stays put.
#[test]
fn classic_beakers_buy_more_than_their_score() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 30, rng_seed: 5 });
    game.debug_found_city(&rules, PlayerId(0), tile(&game, 5, 5));
    game.debug_found_city(&rules, PlayerId(1), tile(&game, 30, 20));
    let obs = game.ai_observation(&rules, PlayerId(0));
    let direct = Prices::of_with(&obs, &rules, PriceModel::Uses, SciencePricing::Direct);
    let reach = Prices::of_with(&obs, &rules, PriceModel::Uses, SciencePricing::Reach);
    assert!(reach.science >= 3 * direct.science, "reach {} against direct {}", reach.science, direct.science);
    assert_eq!((reach.gold, reach.food, reach.happiness), (direct.gold, direct.food, direct.happiness));
}

/// Refused orders (`needs Coal`, `needs its policy`) of the AI's cities in a
/// game of `rounds`: the cities of `Direct` chose them again every round
/// while the empire's best cities stood idle; the game's AI orders only what
/// the rules accept.
///
/// The seed is one where `Direct` runs into the refusals (472 in 260 rounds): the
/// first seed of this test, 0x1_7011, stopped reaching them when the scouts moved
/// the early game (the control must be refused, or the test proves nothing).
fn refused_orders(variant: Variant, rounds: u32) -> usize {
    use classic_rules::ai::trace::Outcome;
    use classic_rules::ai::driver::ClassicMind;
    use classic_rules::ai::AiSettings;
    use kernel::ai::driver::AiDriver;
    use classic_rules::scenario::{generate_new_game, ScenarioConfig};
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig { seed: 0x1_7003, players: 4, width: 40, height: 28, cities: 0, units: 0, water_ratio: 20 };
    let mut game = generate_new_game(&rules, &config);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(variant));
    driver.set_trace_capacity(1_000_000);
    while game.view().auth.turn().round < rounds && game.winner().is_none() {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let refused: Vec<_> = driver
        .trace()
        .entries()
        .filter(|e| matches!(&e.outcome, Outcome::Rejected(err) if format!("{err:?}").contains("needs ")))
        .collect();
    for e in refused.iter().take(5) {
        eprintln!("{variant:?} refused: {e} {:?}", e.command);
        if let Some(classic_rules::Command::EnqueueProduction { item: classic_rules::ids::ProductionItem::Building(k), .. }) = &e.command {
            let d = &rules.buildings[k.0 as usize];
            eprintln!("  building {} kind {:?} needs in empire {:?}", d.key, d.kind, d.requires_buildings_in_empire.iter().map(|b| rules.buildings[b.0 as usize].key.to_string()).collect::<Vec<_>>());
        }
    }
    refused.len()
}

// The standard AI orders only what the rules accept. Marked `#[ignore]` on 2026-10-22 for a
// national wonder it was said to order against the per-city requirement. It already passed on
// origin/main before the legality work (run with `--ignored`, 3.4 s, standard 0 refused, the
// control refused): the planner asked `PlayerView::own_building_site`, the rules' own check of
// that requirement, since the order filter was repaired. The marker was stale; the test runs.
#[test]
fn the_cities_order_only_what_the_rules_accept() {
    let (standard, direct) = (refused_orders(Variant::Standard, 260), refused_orders(Variant::Direct, 260));
    assert_eq!(standard, 0, "the game's AI orders only what the rules accept");
    assert!(direct > 0, "the control: Direct is refused (otherwise the case does not reach the refusals)");
}
