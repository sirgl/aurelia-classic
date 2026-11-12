//! The AI and a captured city in resistance (the owner's rule, 11 November: the city gives
//! nothing and builds nothing meanwhile): it prices the capture by the yields the city gives
//! once the resistance ends, and it sends no order and no purchase to a city in resistance.

use classic_content::core::core_rules;
use classic_rules::ai::conquest::priced_case;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::trace::Outcome;
use classic_rules::ai::value::{PriceModel, Prices, SciencePricing};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{PlayerId, TerrainId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::world::PUPPET_RESISTS;
use classic_rules::{compile_rules, ClassicGame, GameConfig, GameState};
use kernel::ai::driver::AiDriver;

const P0: PlayerId = PlayerId(0);

/// P0's one city of three on open land, `resistance` turns in resistance.
fn captured(resistance: u16) -> (GameState, classic_rules::CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 24, height: 24, rng_seed: 5 });
    let land = rules
        .terrains
        .iter()
        .filter(|t| !t.water && t.can_found_city && !t.impassable)
        .filter_map(|t| t.map_code)
        .find(|code| !rules.terrain_kinds.is_hills(*code))
        .expect("a flat land terrain");
    let all: Vec<_> = (0..24 * 24).map(|i| (TileId(i), TerrainId(land))).collect();
    game.debug_set_terrain_batch(&rules, &all);
    let centre = game.view().auth.map().dimensions.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, P0, centre);
    game.debug_set_population(&rules, city, 3);
    game.world_mut().cities.get_mut(city).unwrap().resistance_turns = resistance;
    (game, rules)
}

/// The observation of a city in resistance: its income now is nothing, its income once the
/// resistance ends is what the city gives without it.
#[test]
fn the_observation_tells_what_a_city_gives_after_its_resistance() {
    let (game, rules) = captured(4);
    let (free, _) = captured(0);
    let city = game.ai_observation(&rules, P0).own_cities.remove(0);
    let free = free.ai_observation(&rules, P0).own_cities.remove(0);
    assert_eq!(city.resistance, 4);
    assert_eq!(city.income, Default::default());
    assert!(free.income.production > 0 && free.income.food > 0);
    assert_eq!(city.income_open, free.income, "the same city, once the resistance has ended");
    assert_eq!(free.income_open, free.income);
}

/// A capture is priced by the rules: the decision's worth counts the yields from the end of the
/// resistance on, not the zero a city in resistance gives now, and an annexed city (a puppet
/// too where the rules let a puppet resist) is worth less than the same city free of it.
#[test]
fn a_captured_city_is_priced_by_the_yields_it_gives_after_its_resistance() {
    let (game, rules) = captured(3);
    let (free, _) = captured(0);
    let (obs, free_obs) = (game.ai_observation(&rules, P0), free.ai_observation(&rules, P0));
    // The same prices for both (the empire's own rates differ with the city's income).
    let prices = Prices::of_effects(&free_obs, &rules, PriceModel::Uses, SciencePricing::Reach, true);
    let case = priced_case(&obs, &rules, &prices, 0, &obs.own_cities[0]);
    let open = priced_case(&free_obs, &rules, &prices, 0, &free_obs.own_cities[0]);
    assert!(open.puppet > 0 || open.annex.is_some(), "a free city has a price: {open:?}");
    if PUPPET_RESISTS {
        assert!(case.puppet < open.puppet, "a puppet in resistance yields later: {case:?} against {open:?}");
    } else {
        assert_eq!(case.puppet, open.puppet, "a puppet has no resistance: it yields at once");
    }
    // The price reads the income of after the resistance, not the zero of now: with that income
    // taken away the same city is worth less.
    let nothing = classic_rules::knowledge::observation::OwnCity { income_open: Default::default(), ..obs.own_cities[0].clone() };
    let bare = priced_case(&obs, &rules, &prices, 0, &nothing);
    assert!(bare.puppet < case.puppet, "{bare:?} against {case:?}");
    assert!(case.annex.is_some() == bare.annex.is_some());
}

/// Every city of an AI-only game put in resistance for five rounds: the AI sends the cities no
/// order and no purchase (nothing is rejected by the rules, their queues and stocks stay), and
/// plays on.
#[test]
fn the_ai_sends_no_order_and_no_purchase_to_a_city_in_resistance() {
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig { seed: 0x1_7003, players: 4, width: 40, height: 28, cities: 0, units: 0, water_ratio: 20 };
    let mut game = generate_new_game(&rules, &config);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(Variant::Standard));
    driver.set_trace_capacity(1_000_000);
    while game.view().auth.turn().round < 40 {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let handles: Vec<_> = game.view().auth.cities().iter().map(|(h, _)| h).collect();
    assert!(handles.len() >= 4, "{} cities", handles.len());
    for h in &handles {
        game.world_mut().cities.get_mut(*h).unwrap().resistance_turns = 5;
    }
    let before: Vec<_> = handles.iter().map(|h| game.view().auth.cities().get(*h).unwrap().clone()).collect();
    let seen = driver.trace().entries().count();
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round < round + 4 {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let rejected = driver.trace().entries().skip(seen).filter(|e| matches!(e.outcome, Outcome::Rejected(_))).count();
    assert_eq!(rejected, 0, "no order reached the rules to be refused");
    for (h, was) in handles.iter().zip(&before) {
        let now = game.view().auth.cities().get(*h).unwrap();
        assert_eq!((&now.queue, now.production), (&was.queue, was.production), "city {h:?} was left alone");
    }
}
