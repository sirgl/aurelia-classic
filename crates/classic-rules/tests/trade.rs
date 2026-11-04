//! Trade routes of the classic ruleset (classic/docs/classic/rules.md):
//! route count, range, path, international gold and science, internal food
//! and production, circuits and the return of the unit, war, plunder,
//! captured cities, save and load.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::derived::trade::{circuits, route_status, routes_possible, trade_offers, TradeBlock};
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::world::trade::{RouteKind, TradeDomain};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const GRASS: TerrainId = TerrainId(1);
const WATER: TerrainId = TerrainId(4);

struct World {
    game: GameState,
    rules: CompiledRuleset,
}

impl World {
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(
            &rules,
            GameConfig {
                players: 2,
                width: 40,
                height: 20,
                rng_seed: 5,
            },
        );
        let all: Vec<_> = (0..800).map(|i| (TileId(i), GRASS)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let every: Vec<TileId> = (0..800).map(TileId).collect();
        for p in [P0, P1] {
            game.debug_reveal(p, &every);
        }
        game.debug_meet(P0, P1);
        game.debug_meet(P1, P0);
        World { game, rules }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        let dims = self.game.view().auth.map().dimensions;
        dims.to_tile(HexCoord { q, r }).unwrap()
    }

    fn city(&mut self, owner: PlayerId, q: i32, r: i32) -> CityHandle {
        let t = self.tile(q, r);
        self.game.debug_found_city(&self.rules, owner, t)
    }

    fn learn(&mut self, p: PlayerId, key: &str) {
        let t = self.rules.technologies.iter().position(|t| t.key == key).unwrap();
        self.game.debug_learn(&self.rules, p, TechId(t as u16));
    }

    fn unit(&self, key: &str) -> UnitKindId {
        UnitKindId(self.rules.units.iter().position(|u| u.key == key).unwrap() as u16)
    }

    fn building(&self, key: &str) -> BuildingKindId {
        BuildingKindId(self.rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
    }

    fn caravan_in(&mut self, city: CityHandle) -> UnitHandle {
        let tile = self.game.view().auth.cities().get(city).unwrap().tile;
        let caravan = self.unit("core:unit/caravan");
        self.game.debug_spawn_unit(&self.rules, P0, tile, caravan)
    }

    fn exec(&mut self, command: Command) -> Result<(), String> {
        self.game.execute(&self.rules, command).map(|_| ()).map_err(|e| format!("{e:?}"))
    }

    fn round(&mut self) {
        for p in [P0, P1] {
            self.game.execute(&self.rules, Command::EndTurn { player: p }).unwrap();
        }
    }

    fn gold_rate(&self, city: CityHandle) -> i32 {
        self.game.derived().yields.of(city).unwrap().gold
    }
}

#[test]
fn routes_come_with_technologies() {
    let mut w = World::new();
    let auth = w.game.view().auth;
    assert_eq!(routes_possible(auth, &w.rules, P0), 0, "no base route (rules 3.1)");
    let home = w.city(P0, 5, 5);
    let caravan = ProductionItem::Unit(w.unit("core:unit/caravan"));
    w.learn(P0, "core:tech/animal-husbandry");
    assert_eq!(routes_possible(w.game.view().auth, &w.rules, P0), 1);
    w.exec(Command::EnqueueProduction { player: P0, city: home, item: caravan }).unwrap();
    let err = w.exec(Command::EnqueueProduction { player: P0, city: home, item: caravan }).unwrap_err();
    assert!(err.contains("no trade route left"), "{err}");
    w.learn(P0, "core:tech/sailing");
    assert_eq!(routes_possible(w.game.view().auth, &w.rules, P0), 2);
}

#[test]
fn a_larger_empire_gets_one_more_route_per_four_cities() {
    let mut w = World::new();
    // No technology slot: size alone gives nothing.
    for i in 0..8 {
        w.city(P0, 2 + 4 * (i % 4), 2 + 4 * (i / 4));
    }
    assert_eq!(routes_possible(w.game.view().auth, &w.rules, P0), 0);
    w.learn(P0, "core:tech/animal-husbandry");
    assert_eq!(routes_possible(w.game.view().auth, &w.rules, P0), 1 + 8 / 4, "1 technology slot, 2 for 8 cities");
}

#[test]
fn circuits_follow_the_rules_table() {
    // Own function (derived/trade.rs): round(24 × speed / (2 L)) between 2 and 8; speed 3 land, 5 sea.
    assert_eq!(circuits(10, TradeDomain::Land), 4);
    assert_eq!(circuits(7, TradeDomain::Land), 5);
    assert_eq!(circuits(20, TradeDomain::Land), 2);
    assert_eq!(circuits(15, TradeDomain::Sea), 4);
    assert_eq!(circuits(1, TradeDomain::Sea), 8, "a one-step route is capped at 8 circuits");
    assert_eq!(circuits(40, TradeDomain::Land), 2, "a very long route still runs twice");
}

#[test]
fn an_international_route_pays_gold_and_science_at_both_ends() {
    let mut w = World::new();
    let home = w.city(P0, 5, 5);
    let away = w.city(P1, 13, 5);
    w.learn(P0, "core:tech/animal-husbandry");
    for key in ["core:tech/pottery", "core:tech/mining", "core:tech/archery"] {
        w.learn(P1, key);
    }
    let market = w.building("core:building/market");
    w.game.debug_add_building(&w.rules, away, market);
    let (gold_home, gold_away) = (w.gold_rate(home), w.gold_rate(away));
    let unit = w.caravan_in(home);
    w.exec(Command::EstablishTradeRoute { unit, destination: away, kind: RouteKind::International }).unwrap();
    let auth = w.game.view().auth;
    assert!(auth.units().get(unit).is_none(), "the caravan becomes the route");
    let route = &auth.module().trade().routes()[0];
    assert_eq!(route.length(), 8);
    assert_eq!(route.circuits_total, 5);
    // Own function (derived/trade.rs): (base 80 + 18 % of the geometric mean of both cities' gold + the Market's
    // target bonus 100) × (100 + 3 × 8 tiles) %.
    use classic_rules::derived::trade::{city_gold_term, distance_percent, science_for, INTERNATIONAL_BASE, RECIPIENT_BASE};
    let term = city_gold_term(gold_home as u32 * 100, gold_away as u32 * 100);
    let scale = distance_percent(8);
    assert_eq!(scale, 124);
    assert_eq!(INTERNATIONAL_BASE, 80);
    assert_eq!(route.origin_yields.gold, (INTERNATIONAL_BASE + term + 100) * scale / 100);
    // The recipient gets (25 + half the city term + the Market's recipient bonus 100) × the same factor.
    assert_eq!(route.destination_yields.gold, (RECIPIENT_BASE + term / 2 + 100) * scale / 100);
    // Science: 0.3 per technology the partner knows: P1 knows 3 techs P0 does not (0.90); P0 knows Animal
    // Husbandry and P1 does not (0.30).
    assert_eq!(route.origin_yields.science, science_for(3));
    assert_eq!(route.origin_yields.science, 90);
    assert_eq!(route.destination_yields.science, 30);
    // The city's rate is a whole number: the route's hundredths round either way.
    let expected = route.origin_yields.gold as i32;
    assert!((w.gold_rate(home) - (gold_home + expected / 100)).abs() <= 1, "the city's gold grows");
    assert!((w.gold_rate(away) - (gold_away + route.destination_yields.gold as i32 / 100)).abs() <= 1);
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn a_route_runs_its_circuits_and_the_caravan_comes_back() {
    let mut w = World::new();
    let home = w.city(P0, 5, 5);
    let away = w.city(P1, 13, 5);
    w.learn(P0, "core:tech/animal-husbandry");
    let unit = w.caravan_in(home);
    w.exec(Command::EstablishTradeRoute { unit, destination: away, kind: RouteKind::International }).unwrap();
    // 8 steps, 5 circuits: 80 steps at 3 a turn.
    let mut rounds = 0;
    while !w.game.view().auth.module().trade().routes().is_empty() && rounds < 40 {
        w.round();
        rounds += 1;
    }
    assert_eq!(rounds, 26, "80 steps, three a turn, three on the first day");
    let caravan = w.unit("core:unit/caravan");
    let back = w.game.view().auth.units().iter_handles().filter(|(_, u)| u.owner == P0 && u.kind == caravan).count();
    assert_eq!(back, 1, "a new caravan in the origin");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn internal_routes_need_their_building_and_feed_the_destination() {
    let mut w = World::new();
    let home = w.city(P0, 5, 5);
    let other = w.city(P0, 11, 5);
    w.learn(P0, "core:tech/animal-husbandry");
    let unit = w.caravan_in(home);
    let auth = w.game.view().auth;
    assert_eq!(route_status(auth, &w.rules, unit, other, RouteKind::Food).unwrap_err(), TradeBlock::NoGranary);
    assert_eq!(
        route_status(auth, &w.rules, unit, other, RouteKind::International).unwrap_err(),
        TradeBlock::OwnCityForInternational
    );
    let granary = w.building("core:building/granary");
    w.game.debug_add_building(&w.rules, home, granary);
    let food = w.game.derived().yields.of(other).unwrap().food;
    w.exec(Command::EstablishTradeRoute { unit, destination: other, kind: RouteKind::Food }).unwrap();
    // Own function (derived/trade.rs `internal_amount`): 200 + 15 per citizen of the destination in the first era.
    let route = &w.game.view().auth.module().trade().routes()[0];
    let citizens = w.game.view().auth.cities().get(other).unwrap().population;
    assert_eq!(route.destination_yields.food, 200 + 15 * citizens);
    assert_eq!(route.origin_yields, Default::default());
    // 2.15 food, shown as whole food: +2.
    assert_eq!(w.game.derived().yields.of(other).unwrap().food, food + 2);
}

#[test]
fn range_limits_routes_and_the_caravansary_extends_it() {
    let mut w = World::new();
    let home = w.city(P0, 2, 5);
    let far = w.city(P1, 15, 5);
    w.learn(P0, "core:tech/animal-husbandry");
    let unit = w.caravan_in(home);
    let auth = w.game.view().auth;
    assert_eq!(
        route_status(auth, &w.rules, unit, far, RouteKind::International).unwrap_err(),
        TradeBlock::OutOfRange,
        "13 tiles, range 10"
    );
    let caravansary = w.building("core:building/caravansary");
    w.game.debug_add_building(&w.rules, home, caravansary);
    let plan = route_status(w.game.view().auth, &w.rules, unit, far, RouteKind::International).unwrap();
    assert_eq!(plan.path.len(), 14, "range 15 with the Caravansary");
}

#[test]
fn roads_halve_the_cost_of_a_land_path() {
    let mut w = World::new();
    let home = w.city(P0, 2, 5);
    let far = w.city(P1, 16, 5);
    w.learn(P0, "core:tech/animal-husbandry");
    let unit = w.caravan_in(home);
    let road: Vec<TileId> = (2..=16).map(|q| w.tile(q, 5)).collect();
    w.game.debug_set_road_batch(&w.rules, &road);
    let plan = route_status(w.game.view().auth, &w.rules, unit, far, RouteKind::International).unwrap();
    assert_eq!(plan.path.len(), 15, "14 road steps cost 700 ≤ 1099");
}

#[test]
fn war_ends_the_routes_between_the_players() {
    let mut w = World::new();
    let home = w.city(P0, 5, 5);
    let away = w.city(P1, 13, 5);
    w.learn(P0, "core:tech/animal-husbandry");
    let unit = w.caravan_in(home);
    w.exec(Command::EstablishTradeRoute { unit, destination: away, kind: RouteKind::International }).unwrap();
    let gold = w.gold_rate(home);
    w.game.debug_force_war(&w.rules, P1, P0);
    assert!(w.game.view().auth.module().trade().routes().is_empty());
    assert!(w.gold_rate(home) < gold, "the route's gold is gone");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn an_enemy_unit_plunders_the_caravan_on_its_tile() {
    let mut w = World::new();
    let home = w.city(P0, 5, 5);
    let other = w.city(P0, 13, 5);
    w.city(P1, 9, 12);
    w.learn(P0, "core:tech/animal-husbandry");
    let granary = w.building("core:building/granary");
    w.game.debug_add_building(&w.rules, home, granary);
    let unit = w.caravan_in(home);
    w.exec(Command::EstablishTradeRoute { unit, destination: other, kind: RouteKind::Food }).unwrap();
    w.game.debug_force_war(&w.rules, P1, P0);
    // P1's turn: a raider stands on the caravan's tile.
    w.game.execute(&w.rules, Command::EndTurn { player: P0 }).unwrap();
    let at = w.game.view().auth.module().trade().routes()[0].unit_tile();
    let warrior = w.unit("core:unit/warrior");
    let raider = w.game.debug_spawn_unit(&w.rules, P1, at, warrior);
    let gold = w.game.view().auth.players().treasury_of(P1);
    let mut scratch = classic_rules::navigation::PathScratch::default();
    let order = Command::PlunderTradeRoute { unit: raider };
    classic_rules::command::validate(w.game.view(), &w.rules, order.clone(), &mut scratch).unwrap();
    let result = w.game.execute(&w.rules, order).unwrap();
    assert!(w.game.view().auth.module().trade().routes().is_empty());
    assert_eq!(w.game.view().auth.players().treasury_of(P1), gold + 60, "60 gold on land in the first era");
    assert!(
        result.changes().module_changes.contains(&classic_rules::classic::ClassicChange::Plundered {
            player: P1,
            tile: at,
            source: classic_rules::PlunderSource::TradeRoute,
            gold: 60,
        }),
        "the journal names the plunder and where it happened"
    );
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn a_caravan_that_meets_an_enemy_on_its_path_is_plundered() {
    let mut w = World::new();
    let home = w.city(P0, 5, 5);
    let other = w.city(P0, 13, 5);
    w.city(P1, 9, 12);
    w.learn(P0, "core:tech/animal-husbandry");
    let granary = w.building("core:building/granary");
    w.game.debug_add_building(&w.rules, home, granary);
    let unit = w.caravan_in(home);
    w.exec(Command::EstablishTradeRoute { unit, destination: other, kind: RouteKind::Food }).unwrap();
    w.game.debug_force_war(&w.rules, P1, P0);
    let route = &w.game.view().auth.module().trade().routes()[0];
    let ahead = route.path[route.position as usize + 1];
    let warrior = w.unit("core:unit/warrior");
    w.game.debug_spawn_unit(&w.rules, P1, ahead, warrior);
    let gold = w.game.view().auth.players().treasury_of(P1);
    w.game.execute(&w.rules, Command::EndTurn { player: P0 }).unwrap();
    assert!(w.game.view().auth.module().trade().routes().is_empty(), "plundered on the step");
    assert_eq!(w.game.view().auth.players().treasury_of(P1), gold + 60);
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn a_cargo_ship_pays_half_again_on_a_sea_route() {
    let mut w = World::new();
    // A sea between the cities: rows 6 and 7 are water.
    let sea: Vec<_> = (0..40).flat_map(|q| [(q, 6), (q, 7)]).map(|(q, r)| (w.tile(q, r), WATER)).collect();
    w.game.debug_set_terrain_batch(&w.rules, &sea);
    let home = w.city(P0, 5, 5);
    let away = w.city(P1, 15, 5);
    w.learn(P0, "core:tech/sailing");
    let tile = w.game.view().auth.cities().get(home).unwrap().tile;
    let ship = w.unit("core:unit/cargo-ship");
    let unit = w.game.debug_spawn_unit(&w.rules, P0, tile, ship);
    let offers = trade_offers(w.game.view().auth, &w.rules, unit);
    let offer = offers.iter().find(|o| o.destination == away).expect("a sea route");
    use classic_rules::derived::trade::{city_gold_term, distance_percent, domain_percent};
    let term = city_gold_term(w.gold_rate(home) as u32 * 100, w.gold_rate(away) as u32 * 100);
    let land_value = classic_rules::derived::trade::INTERNATIONAL_BASE + term;
    let scale = distance_percent(offer.steps) * domain_percent(TradeDomain::Sea) / 100;
    assert_eq!(offer.origin.gold, land_value * scale / 100, "a sea route pays half as much again");
    assert!(scale > distance_percent(offer.steps), "the sea factor 150 % is part of the scale");
}

#[test]
fn routes_survive_save_and_load() {
    let mut w = World::new();
    let home = w.city(P0, 5, 5);
    let away = w.city(P1, 13, 5);
    w.learn(P0, "core:tech/animal-husbandry");
    let unit = w.caravan_in(home);
    w.exec(Command::EstablishTradeRoute { unit, destination: away, kind: RouteKind::International }).unwrap();
    w.round();
    let save = classic_rules::persistence::encode::encode(&w.game, &w.rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &w.rules).unwrap();
    assert_eq!(loaded.view().auth.module().trade(), w.game.view().auth.module().trade());
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&w.game));
    let mut bad = save;
    bad.world.module.trade_routes[0].path.clear();
    assert!(classic_rules::persistence::decode::decode(&bad, &w.rules).is_err());
}
