//! `trade-fit`'s trade routes (`ai::trade`, classic/docs/ai-trade.md): a
//! route priced by what it delivers (the food of an internal route by the
//! growth it buys in its destination), its risk near a hostile unit, the
//! route slots a technology opens worth their route in research. The tests
//! name classic content only to set the scene.

use classic_content::core::core_rules;
use classic_rules::ai::trade::{self, Hazards};
use classic_rules::ai::value::{PriceModel, Prices, SciencePricing};
use classic_rules::ai::variant::Variant;
use classic_rules::derived::trade::TradeOffer;
use classic_rules::ids::{CityHandle, PlayerId, TechId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::world::state::GameConfig;
use classic_rules::world::trade::{RouteKind, RouteYields};
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// Two cities of the first nation (a small one and a large one), one of the second.
fn game() -> (GameState, CompiledRuleset, CityHandle, CityHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 30, rng_seed: 5 });
    let small = game.debug_found_city(&rules, P0, tile(&game, 8, 8));
    let large = game.debug_found_city(&rules, P0, tile(&game, 16, 8));
    game.debug_set_population(&rules, large, 12);
    game.debug_found_city(&rules, P1, tile(&game, 30, 20));
    (game, rules, small, large)
}

fn prices(game: &GameState, rules: &CompiledRuleset) -> Prices {
    let obs = game.ai_observation(rules, P0);
    Prices::of_effects(&obs, rules, PriceModel::Uses, SciencePricing::Reach, true)
}

fn offer(destination: CityHandle, kind: RouteKind, origin: RouteYields, at: RouteYields) -> TradeOffer {
    TradeOffer { destination, kind, steps: 6, origin, destination_yields: at, side_value: 0 }
}

/// `trade-fit` alone carries the switch; it is `standard` in everything else.
#[test]
fn trade_fit_is_a_variant_of_its_own() {
    assert_eq!(Variant::from_name("trade-fit"), Some(Variant::TradeFit));
    assert!(Variant::TradeFit.trade_fit() && !Variant::Standard.trade_fit());
    assert_eq!(Variant::TradeFit.tall(), Variant::Standard.tall());
    assert_eq!(Variant::TradeFit.war_tuning(), Variant::Standard.war_tuning());
}

/// A food brought to a city further from its next citizens buys fewer of
/// them: the small city's food is worth at least the large one's, both within
/// the bounds around the allocator's own food price.
#[test]
fn the_food_of_a_route_is_worth_the_growth_it_buys_where_it_arrives() {
    let (game, rules, small, large) = game();
    let prices = prices(&game, &rules);
    let obs = game.ai_observation(&rules, P0);
    let city = |h: CityHandle| obs.own_cities.iter().find(|c| c.handle == h).unwrap();
    let (in_small, in_large) = (trade::food_price_in(&obs, &prices, city(small)), trade::food_price_in(&obs, &prices, city(large)));
    assert!(in_small >= in_large, "small {in_small}, large {in_large}");
    for price in [in_small, in_large] {
        assert!(price >= prices.food * trade::FOOD_PRICE_MIN_PERCENT / 100 && price <= prices.food * trade::FOOD_PRICE_MAX_PERCENT / 100);
    }
    // The route's value is its food at that price; a production route its hammers.
    let food = RouteYields { food: 300, ..RouteYields::default() };
    let hammers = RouteYields { production: 300, ..RouteYields::default() };
    assert_eq!(trade::fit_value(&obs, &prices, &offer(small, RouteKind::Food, RouteYields::default(), food)), 3 * in_small);
    assert_eq!(trade::fit_value(&obs, &prices, &offer(large, RouteKind::Production, RouteYields::default(), hammers)), 3 * prices.production);
}

/// An international route is worth the origin's gold and science and what it
/// adds elsewhere, at the gold price; the destination's yields are another
/// nation's.
#[test]
fn an_international_route_is_worth_what_the_origin_gets() {
    let (game, rules, small, _) = game();
    let prices = prices(&game, &rules);
    let obs = game.ai_observation(&rules, P0);
    let origin = RouteYields { gold: 400, science: 100, ..RouteYields::default() };
    let mut o = offer(small, RouteKind::International, origin, RouteYields { food: 900, gold: 900, ..RouteYields::default() });
    o.side_value = 200;
    assert_eq!(trade::fit_value(&obs, &prices, &o), 4 * prices.gold + prices.science + 2 * prices.gold);
}

/// A hostile military unit near the way between two cities is a hazard; one
/// far from it, or a civilian, is not.
#[test]
fn a_hostile_unit_near_the_way_is_a_hazard() {
    let (mut game, rules, _, _) = game();
    let (a, b) = (tile(&game, 8, 8), tile(&game, 16, 8));
    let soldier = UnitKindId(rules.units.iter().position(|u| u.strength > 0 && u.domain == classic_rules::rules::content::Domain::Land).unwrap() as u16);
    let far = tile(&game, 12, 25);
    game.debug_spawn_unit(&rules, P1, far, soldier);
    game.debug_meet(P0, P1);
    game.execute(&rules, classic_rules::command::Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    let obs = game.ai_observation(&rules, P0);
    assert!(!Hazards::of(&obs, &rules).near(&obs, a, Some(b)), "a unit far from the way");
    drop(obs);
    let near = tile(&game, 10, 8);
    game.debug_spawn_unit(&rules, P1, near, soldier);
    let obs = game.ai_observation(&rules, P0);
    assert!(obs.visible_foreign_units.iter().any(|u| u.tile == near), "the city sees the unit");
    assert!(Hazards::of(&obs, &rules).near(&obs, a, Some(b)), "a seen unit by the way is a hazard");
}

/// Every technology that opens a route slot (the rules' data) is worth the
/// slot's route in research once the empire has two cities; none before.
#[test]
fn a_route_slot_is_worth_its_route_in_research() {
    let (game, rules, _, _) = game();
    let prices = prices(&game, &rules);
    let w = prices.weights();
    let brief = classic_rules::ai::planner::TurnBrief::from_observation(&game.ai_observation(&rules, P0), &rules);
    let slots: Vec<TechId> = (0..rules.technologies.len())
        .map(|i| TechId(i as u16))
        .filter(|t| {
            rules.technologies[t.0 as usize].effects.passive_player.iter().any(|e| {
                matches!(e.modifier, classic_rules::rules::effects::Modifier::AddStat { stat: classic_rules::rules::stats::Stat::Module(classic_rules::rules::stats::ClassicStat::Trade(classic_rules::rules::stats::TradeStat::Routes)), .. })
            })
        })
        .collect();
    assert!(!slots.is_empty(), "the rules open route slots by technology");
    for tech in &slots {
        assert!(trade::slot_research_value(&rules, *tech, &brief.cities, &w) > 0);
        assert_eq!(trade::slot_research_value(&rules, *tech, &brief.cities[..1], &w), 0, "one city has no route to run");
    }
    let other = (0..rules.technologies.len()).map(|i| TechId(i as u16)).find(|t| !slots.contains(t)).unwrap();
    assert_eq!(trade::slot_research_value(&rules, other, &brief.cities, &w), 0);
}
