//! Gold of the classic ruleset (classic/docs/classic/rules.md):
//! maintenance, city connections, deficits, purchases, building rules,
//! production processes and tile prices.

use classic_content::core::core_rules;
use classic_rules::derived::maintenance::{connection_gold_times100, upkeep};
use classic_rules::ids::{
    BuildingKindId, CityHandle, PlayerId, ProductionItem, TechId, TerrainId, TileId, UnitKindId,
};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::world::city::hurry_price;
use classic_rules::world::city_economy::Process;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const GRASS: TerrainId = TerrainId(1);
const WATER: TerrainId = TerrainId(4);
const MOUNTAIN: TerrainId = TerrainId(6);

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
                width: 30,
                height: 20,
                rng_seed: 4,
            },
        );
        let all: Vec<_> = (0..600).map(|i| (TileId(i), GRASS)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        World { game, rules }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        self.game
            .view()
            .auth
            .map()
            .dimensions
            .to_tile(HexCoord { q, r })
            .unwrap()
    }

    fn city(&mut self, q: i32, r: i32, pop: u32) -> CityHandle {
        let t = self.tile(q, r);
        let city = self.game.debug_found_city(&self.rules, P0, t);
        self.game
            .debug_grow_city(&self.rules, city, pop.saturating_sub(1));
        city
    }

    fn building(&self, key: &str) -> BuildingKindId {
        BuildingKindId(
            self.rules
                .buildings
                .iter()
                .position(|b| b.key == key)
                .unwrap() as u16,
        )
    }

    fn unit(&self, key: &str) -> UnitKindId {
        UnitKindId(self.rules.units.iter().position(|u| u.key == key).unwrap() as u16)
    }

    fn learn(&mut self, key: &str) {
        let t = TechId(
            self.rules
                .technologies
                .iter()
                .position(|t| t.key == key)
                .unwrap() as u16,
        );
        self.game.debug_learn(&self.rules, P0, t);
    }

    fn check(&self, command: Command) -> Result<(), String> {
        let mut scratch = classic_rules::navigation::PathScratch::default();
        classic_rules::command::validate(self.game.view(), &self.rules, command, &mut scratch)
            .map(|_| ())
            .map_err(|e| format!("{e:?}"))
    }

    fn enqueue(&self, city: CityHandle, key: &str) -> Result<(), String> {
        let item = ProductionItem::Building(self.building(key));
        self.check(Command::EnqueueProduction {
            player: P0,
            city,
            item,
        })
    }

    fn end_turn(&mut self) {
        self.game
            .execute(&self.rules, Command::EndTurn { player: P0 })
            .unwrap();
        // The second player has nothing: the round comes back to player 0.
    }
}

#[test]
fn units_and_buildings_cost_classic_maintenance() {
    let mut w = World::new();
    let city = w.city(5, 5, 1);
    let warrior = w.unit("core:unit/warrior");
    let at = w.tile(6, 6);
    for _ in 0..10 {
        w.game.debug_spawn_unit(&w.rules, P0, at, warrior);
    }
    // 10 units at turn 0, 3 free (Unciv: CivInfoStatsForNextTurn.kt): 7 × 0.5 = 3.5 → 3.
    assert_eq!(upkeep(w.game.view().auth, &w.rules, P0).units, 3);
    // The same empire played by the AI: × the AI unit upkeep percent, 100 % as in Unciv
    // (Difficulties.json; it was 85 % before the formula pass) → 3.
    w.game.configure_players(w.rules.default_difficulty(), &[P0]);
    assert_eq!(upkeep(w.game.view().auth, &w.rules, P0).units, 3);
    w.game.configure_players(w.rules.default_difficulty(), &[]);
    for key in ["core:building/monument", "core:building/library"] {
        let b = w.building(key);
        w.game.debug_add_building(&w.rules, city, b);
    }
    assert_eq!(upkeep(w.game.view().auth, &w.rules, P0).buildings, 2);
}

#[test]
fn roads_cost_gold_and_connect_cities_to_the_capital() {
    let mut w = World::new();
    let capital = w.city(4, 8, 8);
    let palace = w.building("core:building/palace");
    w.game.debug_add_building(&w.rules, capital, palace);
    w.city(10, 8, 5);
    let road: Vec<TileId> = (5..10).map(|q| w.tile(q, 8)).collect();
    // Unconnected: no connection gold.
    assert_eq!(
        connection_gold_times100(w.game.view().auth, &w.rules, P0),
        0
    );
    w.game.debug_set_road_batch(&w.rules, &road);
    // −100 + 8 × 15 + 5 × 110 = 570 hundredths (rules.md example).
    assert_eq!(
        connection_gold_times100(w.game.view().auth, &w.rules, P0),
        570
    );
    let up = upkeep(w.game.view().auth, &w.rules, P0);
    assert_eq!(up.connections, 5);
    // Road tiles owned by the cities cost 1 each (city tiles are free);
    // tiles outside the borders cost their builder nothing here.
    let owned = road
        .iter()
        .filter(|t| w.game.view().auth.map().owner(**t).flatten() == Some(P0))
        .count() as i32;
    assert_eq!(up.routes, owned);
}

#[test]
fn harbors_connect_cities_over_the_sea() {
    let mut w = World::new();
    let sea: Vec<_> = (0..600)
        .map(TileId)
        .filter(|t| {
            let r = w
                .game
                .view()
                .auth
                .map()
                .dimensions
                .to_coord(*t)
                .map(|c| c.r)
                .unwrap_or(0);
            r >= 10
        })
        .map(|t| (t, WATER))
        .collect();
    w.game.debug_set_terrain_batch(&w.rules, &sea);
    let capital = w.city(4, 9, 4);
    let other = w.city(14, 9, 3);
    w.game
        .debug_add_building(&w.rules, capital, w.building("core:building/palace"));
    assert_eq!(
        connection_gold_times100(w.game.view().auth, &w.rules, P0),
        0
    );
    let harbor = w.building("core:building/harbor");
    w.game.debug_add_building(&w.rules, capital, harbor);
    w.game.debug_add_building(&w.rules, other, harbor);
    // −100 + 4 × 15 + 3 × 110 = 290.
    assert_eq!(
        connection_gold_times100(w.game.view().auth, &w.rules, P0),
        290
    );
}

#[test]
fn a_deficit_empties_the_treasury_and_costs_science() {
    let mut w = World::new();
    w.city(5, 5, 1);
    let warrior = w.unit("core:unit/warrior");
    let at = w.tile(6, 6);
    for _ in 0..30 {
        w.game.debug_spawn_unit(&w.rules, P0, at, warrior);
    }
    w.game.debug_earn(P0, 3);
    let income = classic_rules::derived::income::player_income(w.game.view().auth, &w.rules, P0);
    assert!(income.gold < -5, "{income:?}");
    let science_before = w.game.view().auth.players().science_of(P0);
    let units_before = w.game.view().auth.indexes().units_of(P0).len();
    w.end_turn();
    let auth = w.game.view().auth;
    assert_eq!(
        auth.players().treasury_of(P0),
        0,
        "the treasury never goes below 0"
    );
    // The science of the turn lost the shortfall (at least 0).
    let gained = auth.players().science_of(P0) - science_before;
    assert!(
        (gained as i32) < income.science,
        "science {gained} of {}",
        income.science
    );
    // At −5 or worse with more military units than cities, units go (1/2 a turn).
    for _ in 0..10 {
        w.end_turn();
    }
    assert!(w.game.view().auth.indexes().units_of(P0).len() < units_before);
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn purchase_prices_follow_the_classic_formula() {
    // rules.md table: warrior 40 → 200, worker 70 → 310, monument
    // (+40 %) → 280, library (+25 %) → 400; wonders are not for sale.
    assert_eq!(hurry_price(40, 0), Some(200));
    assert_eq!(hurry_price(70, 0), Some(310));
    assert_eq!(hurry_price(40, 40), Some(280));
    assert_eq!(hurry_price(75, 25), Some(400));
    assert_eq!(hurry_price(185, -1), None);
    let mut w = World::new();
    let city = w.city(5, 5, 1);
    w.learn("core:tech/masonry");
    // The Mausoleum: a masonry wonder without a policy branch (the
    // Pyramids need Liberty).
    let pyramids = ProductionItem::Building(w.building("core:building/mausoleum_of_halicarnassus"));
    w.game
        .execute(
            &w.rules,
            Command::EnqueueProduction {
                player: P0,
                city,
                item: pyramids,
            },
        )
        .unwrap();
    w.game.debug_earn(P0, 10_000);
    let err = w
        .check(Command::HurryProduction { player: P0, city })
        .unwrap_err();
    assert!(err.contains("cannot be purchased"), "{err}");
}

#[test]
fn buildings_need_their_place() {
    let mut w = World::new();
    let inland = w.city(5, 5, 3);
    for tech in [
        "core:tech/compass",
        "core:tech/the-wheel",
        "core:tech/writing",
        "core:tech/philosophy",
        "core:tech/astronomy",
    ] {
        w.learn(tech);
    }
    assert!(w
        .enqueue(inland, "core:building/harbor")
        .unwrap_err()
        .contains("coast"));
    assert!(w
        .enqueue(inland, "core:building/water_mill")
        .unwrap_err()
        .contains("river"));
    assert!(w
        .enqueue(inland, "core:building/observatory")
        .unwrap_err()
        .contains("mountain"));
    let peak = w.tile(6, 5);
    w.game
        .debug_set_terrain_batch(&w.rules, &[(peak, MOUNTAIN)]);
    assert!(w.enqueue(inland, "core:building/observatory").is_ok());
    let palace = w.enqueue(inland, "core:building/palace").unwrap_err();
    assert!(palace.contains("NotProducible"), "{palace}");
    // A national wonder needs its building in every city, and only once.
    let other = w.city(12, 5, 1);
    let library = w.building("core:building/library");
    w.game.debug_add_building(&w.rules, inland, library);
    assert!(w
        .enqueue(inland, "core:building/national_college")
        .unwrap_err()
        .contains("every city"));
    w.game.debug_add_building(&w.rules, other, library);
    assert!(w.enqueue(inland, "core:building/national_college").is_ok());
    let college = ProductionItem::Building(w.building("core:building/national_college"));
    w.game
        .execute(
            &w.rules,
            Command::EnqueueProduction {
                player: P0,
                city: inland,
                item: college,
            },
        )
        .unwrap();
    assert!(w
        .enqueue(other, "core:building/national_college")
        .unwrap_err()
        .contains("QueuedElsewhere"));
}

#[test]
fn processes_turn_production_into_gold() {
    let mut w = World::new();
    let city = w.city(5, 5, 3);
    let wealth = Command::SetProcess {
        player: P0,
        city,
        process: Some(Process::Wealth),
    };
    assert!(w.check(wealth.clone()).unwrap_err().contains("locked"));
    let tech = w
        .rules
        .roles
        .process_tech(Process::Wealth.role().unwrap())
        .expect("a technology unlocks Wealth");
    w.game.debug_learn(&w.rules, P0, tech);
    w.game.execute(&w.rules, wealth).unwrap();
    let auth = w.game.view().auth;
    let core = auth.cities().get(city).unwrap().clone();
    let income = classic_rules::derived::income::city_income(auth, &w.rules, &core);
    let rates = classic_rules::derived::yields::city_breakdown(auth, &w.rules, &core).rates;
    assert_eq!(income.gold, rates.gold + income.production * 25 / 100);
    let units_before = auth.indexes().units_of(P0).len();
    let stock_before = core.production;
    w.end_turn();
    let auth = w.game.view().auth;
    assert_eq!(
        auth.indexes().units_of(P0).len(),
        units_before,
        "no default warrior"
    );
    assert_eq!(
        auth.cities().get(city).unwrap().production,
        stock_before,
        "no production stored"
    );
    let save = classic_rules::persistence::encode::encode(&w.game, &w.rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &w.rules).unwrap();
    assert_eq!(authoritative_hash(&w.game), authoritative_hash(&loaded));
    assert_eq!(
        loaded
            .view()
            .auth
            .cities()
            .get(city)
            .unwrap()
            .economy
            .process,
        Some(Process::Wealth)
    );
}

#[test]
fn tile_prices_grow_with_distance_and_tiles_bought() {
    let mut w = World::new();
    let city = w.city(10, 10, 1);
    let offer = |w: &World, q, r| {
        classic_rules::world::territory::tile_purchase_offer(
            w.game.view().auth,
            &w.rules,
            city,
            w.tile(q, r),
        )
        .map(|o| o.price)
    };
    // Player 0 is America: tiles cost 50 % less (Manifest Destiny).
    // Unciv: CityExpansionManager.getGoldCostOfTile, 50 (d − 1) + 5 claimed:
    // ring 2 with nothing claimed is 50 → 25.
    assert_eq!(offer(&w, 12, 10), Some(25));
    // Ring 3 touches no border yet.
    assert_eq!(offer(&w, 13, 10), None);
    w.game.debug_earn(P0, 1000);
    let tile = w.tile(12, 10);
    w.game
        .execute(
            &w.rules,
            Command::PurchaseTile {
                player: P0,
                city,
                tile,
            },
        )
        .unwrap();
    // One tile claimed: ring 2 is 55 → 27; ring 3 is 105 → 52 (the
    // percent truncates; Unciv rounds 27.5 and 52.5 up).
    assert_eq!(offer(&w, 12, 9), Some(27));
    assert_eq!(offer(&w, 13, 10), Some(52));
}

#[test]
fn an_ai_pays_its_difficultys_production_costs() {
    // rules.md: an AI's costs × AITrainPercent / AIConstructPercent of
    // the game's level; a human pays the data cost.
    let mut w = World::new();
    let city = w.city(5, 5, 1);
    let warrior = ProductionItem::Unit(w.unit("core:unit/warrior"));
    let monument = ProductionItem::Building(w.building("core:building/monument"));
    let cost = |w: &World, item| {
        let auth = w.game.view().auth;
        let core = auth.cities().get(city).unwrap();
        classic_rules::effects::city::city_modifiers(auth, &w.rules, core).production_cost(&w.rules, item).unwrap()
    };
    assert_eq!((cost(&w, warrior), cost(&w, monument)), (40, 40));
    let deity = w.rules.handicap("core:handicap/deity").unwrap().values;
    w.game.configure_players("core:handicap/deity", &[P0]);
    let expected = |base: u32, percent: i32| (base as i64 * percent as i64 / 100) as u32;
    assert_eq!(cost(&w, warrior), expected(40, deity.ai_train_percent));
    assert_eq!(cost(&w, monument), expected(40, deity.ai_construct_percent));
    assert!(cost(&w, warrior) < 40, "the Deity AI trains cheaper");
}

#[test]
fn a_national_wonder_costs_more_with_every_city() {
    // rules.md: Buildings.Cost + NumCityCostMod × the owner's cities
    // (National College 125 + 30 per city), in the order, the queue and
    // the completion alike.
    let mut w = World::new();
    let first = w.city(5, 5, 1);
    let college = ProductionItem::Building(w.building("core:building/national_college"));
    let def = &w.rules.buildings[w.building("core:building/national_college").0 as usize];
    let (base, per_city) = (def.cost, def.cost_per_city_pct as u32);
    assert_eq!((base, per_city), (125, 30));
    let cost = |w: &World, city| {
        let auth = w.game.view().auth;
        let core = auth.cities().get(city).unwrap();
        classic_rules::effects::city::city_modifiers(auth, &w.rules, core).production_cost(&w.rules, college).unwrap()
    };
    assert_eq!(cost(&w, first), base + per_city);
    let second = w.city(12, 5, 1);
    let third = w.city(5, 12, 1);
    assert_eq!(cost(&w, first), base + 3 * per_city);
    assert_eq!(cost(&w, third), cost(&w, second), "one price for every city of the owner");
    let monument = ProductionItem::Building(w.building("core:building/monument"));
    let auth = w.game.view().auth;
    let core = auth.cities().get(first).unwrap();
    let plain = classic_rules::effects::city::city_modifiers(auth, &w.rules, core).production_cost(&w.rules, monument).unwrap();
    assert_eq!(plain, 40, "an ordinary building does not grow");
}

#[test]
fn a_unique_building_meets_the_requirement_of_the_building_it_replaces() {
    // A Stock Exchange needs a Bank; Germany's Hanse is its Bank.
    let mut w = World::new();
    let city = w.city(5, 5, 1);
    w.learn("core:tech/electricity");
    let refused = w.enqueue(city, "core:building/stock_exchange").unwrap_err();
    assert!(refused.contains("needs a building"), "{refused}");
    let germany = w.rules.nations.iter().position(|c| c.key == "core:nation/germany").unwrap();
    w.game.debug_set_player_nation(P0, germany as u8);
    let hanse = w.building("core:building/hanse");
    w.game.debug_add_building(&w.rules, city, hanse);
    assert_eq!(w.enqueue(city, "core:building/stock_exchange"), Ok(()));
}
