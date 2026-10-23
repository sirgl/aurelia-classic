//! Production, purchase and maintenance effects on the classic ruleset
//! (classic/docs/classic/rules.md): production percents toward
//! the item at the head of the queue (Forge, Aristocracy, Monument to the
//! Gods, Rome, Marble), gold purchase percents (Mobilization, Skyscrapers,
//! Mercantilism, Big Ben), spaceship parts for gold and for a great
//! engineer, worker speed, road maintenance and free garrisons.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::derived::great_people::GreatPersonAction;
use classic_rules::derived::income::city_income;
use classic_rules::derived::maintenance::{route_maintenance, unit_maintenance};
use classic_rules::effects::city::{city_modifiers, head_hurry_price, item_production_percent};
use classic_rules::ids::{BeliefId, BuildingKindId, CityHandle, PlayerId, PolicyId, ProductionItem, TileId, UnitKindId};
use classic_rules::world::city::{gold_purchase_price, PurchasePercents};
use classic_rules::world::map::Route;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn world(rules: &CompiledRuleset) -> GameState {
    GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 })
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

fn policy(rules: &CompiledRuleset, key: &str) -> PolicyId {
    PolicyId(rules.module.policies.iter().position(|p| p.key == key).unwrap() as u16)
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn civ(rules: &CompiledRuleset, key: &str) -> u8 {
    rules.civs.iter().position(|c| c.key == key).unwrap() as u8
}

fn city(game: &mut GameState, rules: &CompiledRuleset, q: i32, r: i32) -> CityHandle {
    let t = tile(game, q, r);
    game.debug_found_city(rules, P0, t)
}

fn enqueue(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle, item: ProductionItem) {
    game.execute(rules, Command::ClearProduction { player: P0, city }).unwrap();
    game.execute(rules, Command::EnqueueProduction { player: P0, city, item }).unwrap();
}

/// The percent `city` would have toward `item` at the head of its queue.
fn percent_toward(game: &GameState, rules: &CompiledRuleset, city: CityHandle, item: ProductionItem) -> i32 {
    let auth = game.view().auth;
    item_production_percent(auth, rules, auth.cities().get(city).unwrap(), item)
}

#[test]
fn a_forge_speeds_land_units_while_they_head_the_queue() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, 5, 5);
    game.debug_set_population(&rules, c, 8);
    game.debug_add_building(&rules, c, building(&rules, "core:building/forge"));
    let warrior = ProductionItem::Unit(unit(&rules, "core:unit/warrior"));
    let monument = ProductionItem::Building(building(&rules, "core:building/monument"));
    let head_percent = |game: &GameState| {
        let auth = game.view().auth;
        city_modifiers(auth, &rules, auth.cities().get(c).unwrap()).item_production_percent
    };
    let production = |game: &GameState| {
        let auth = game.view().auth;
        city_income(auth, &rules, auth.cities().get(c).unwrap())
    };
    enqueue(&mut game, &rules, c, warrior);
    assert_eq!(head_percent(&game), 15);
    // The cached yields follow the queue (QueueChanged), the oracle agrees.
    game.validate(&rules).unwrap();
    let toward_warrior = production(&game);
    enqueue(&mut game, &rules, c, monument);
    assert_eq!(head_percent(&game), 0);
    game.validate(&rules).unwrap();
    let toward_monument = production(&game);
    assert!(toward_warrior.production > toward_monument.production, "+15 % toward the warrior");
    // The estimate toward another item uses that item's percent.
    assert_eq!(toward_monument.production_rate.toward(15), toward_warrior.production);
    assert_eq!(percent_toward(&game, &rules, c, warrior), 15);
}

#[test]
fn wonder_percents_follow_the_era_of_the_wonder() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, 5, 5);
    let pyramids = ProductionItem::Building(building(&rules, "core:building/pyramids"));
    let notre_dame = ProductionItem::Building(building(&rules, "core:building/notre_dame"));
    let library = ProductionItem::Building(building(&rules, "core:building/library"));
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/tradition/aristocracy"));
    assert_eq!(percent_toward(&game, &rules, c, pyramids), 15);
    assert_eq!(percent_toward(&game, &rules, c, notre_dame), 15);
    assert_eq!(percent_toward(&game, &rules, c, library), 0);
    // Monument to the Gods: ancient and classical wonders only.
    game.debug_grant_faith(P0, 20);
    let belief = rules.module.beliefs.iter().position(|b| b.key == "core:belief/pantheon/monument-to-the-gods").unwrap();
    game.execute(&rules, Command::FoundPantheon { player: P0, belief: BeliefId(belief as u16) }).unwrap();
    assert_eq!(percent_toward(&game, &rules, c, pyramids), 30);
    assert_eq!(percent_toward(&game, &rules, c, notre_dame), 15);
    game.validate(&rules).unwrap();
}

#[test]
fn marble_on_the_city_land_speeds_early_wonders() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, 5, 5);
    let stonehenge = ProductionItem::Building(building(&rules, "core:building/stonehenge"));
    let notre_dame = ProductionItem::Building(building(&rules, "core:building/notre_dame"));
    let marble = classic_rules::ids::ResourceId(rules.resources.iter().position(|r| r.key == "core:resource/marble").unwrap() as u16);
    let quarry = rules.improvements.iter().position(|i| i.key == "core:improvement/quarry").unwrap();
    let deposit = tile(&game, 6, 5);
    let ProductionItem::Building(kind) = stonehenge else { unreachable!() };
    game.debug_learn(&rules, P0, rules.buildings[kind.0 as usize].required_tech.unwrap());
    game.debug_set_population(&rules, c, 10);
    enqueue(&mut game, &rules, c, stonehenge);
    game.debug_set_resource_batch(&rules, &[(deposit, marble)]);
    assert_eq!(percent_toward(&game, &rules, c, stonehenge), 0, "not connected without a quarry");
    game.debug_set_improvement_batch(&rules, &[(deposit, classic_rules::ids::ImprovementId(quarry as u16))]);
    assert_eq!(percent_toward(&game, &rules, c, stonehenge), 15);
    assert_eq!(percent_toward(&game, &rules, c, notre_dame), 0, "a Medieval wonder");
    // The quarry refreshed the city's cached yields (the oracle compares).
    let auth = game.view().auth;
    assert_eq!(city_modifiers(auth, &rules, auth.cities().get(c).unwrap()).item_production_percent, 15);
    game.validate(&rules).unwrap();
}

#[test]
fn rome_builds_faster_what_its_capital_has() {
    let rules = rules();
    let mut game = world(&rules);
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/rome"));
    let capital = city(&mut game, &rules, 5, 5);
    let other = city(&mut game, &rules, 12, 5);
    let monument = building(&rules, "core:building/monument");
    let library = building(&rules, "core:building/library");
    let writing = rules.buildings[library.0 as usize].required_tech.unwrap();
    game.debug_learn(&rules, P0, writing);
    game.debug_add_building(&rules, capital, monument);
    enqueue(&mut game, &rules, other, ProductionItem::Building(library));
    assert_eq!(percent_toward(&game, &rules, other, ProductionItem::Building(monument)), 25);
    assert_eq!(percent_toward(&game, &rules, other, ProductionItem::Building(library)), 0);
    // The capital gets a library: the other city's library goes faster now,
    // and its cached yields know it (the oracle compares).
    game.debug_set_population(&rules, other, 10);
    let production = |game: &GameState| {
        let auth = game.view().auth;
        city_income(auth, &rules, auth.cities().get(other).unwrap()).production
    };
    let before = production(&game);
    game.debug_add_building(&rules, capital, library);
    assert_eq!(percent_toward(&game, &rules, other, ProductionItem::Building(library)), 25);
    assert!(production(&game) > before, "+25 % toward the library");
    game.validate(&rules).unwrap();
}

#[test]
fn purchase_percents_make_gold_purchases_cheaper() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, 5, 5);
    let other = city(&mut game, &rules, 12, 5);
    let warrior = unit(&rules, "core:unit/warrior");
    let monument = building(&rules, "core:building/monument");
    let price = |game: &GameState| {
        let auth = game.view().auth;
        head_hurry_price(auth, &rules, auth.cities().get(c).unwrap()).unwrap()
    };
    let unit_cost = rules.units[warrior.0 as usize].cost;
    let building_def = &rules.buildings[monument.0 as usize];
    let expect = |cost: u32, modifier: i32, hurry: i32, kind: i32| {
        gold_purchase_price(cost, modifier, PurchasePercents { hurry, kind }).unwrap()
    };
    enqueue(&mut game, &rules, c, ProductionItem::Unit(warrior));
    assert_eq!(price(&game), expect(unit_cost, 0, 0, 0));
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/autocracy/mobilization"));
    assert_eq!(price(&game), expect(unit_cost, 0, 0, -25));
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/commerce/mercantilism"));
    assert_eq!(price(&game), expect(unit_cost, 0, -25, -25));
    enqueue(&mut game, &rules, c, ProductionItem::Building(monument));
    let cost = building_def.cost;
    let modifier = building_def.hurry_cost_modifier;
    assert_eq!(price(&game), expect(cost, modifier, -25, 0), "Mobilization is for units");
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/order/skyscrapers"));
    assert_eq!(price(&game), expect(cost, modifier, -25, -25));
    // Big Ben in another city: every city of the owner buys cheaper.
    game.debug_add_building(&rules, other, building(&rules, "core:building/big_ben"));
    assert_eq!(price(&game), expect(cost, modifier, -40, -25));
    assert!(price(&game) < expect(cost, modifier, 0, 0));
}

#[test]
fn spaceship_parts_need_their_policies_for_gold_and_engineers() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, 5, 5);
    let booster = ProductionItem::Unit(unit(&rules, "core:unit/ss-booster"));
    // The parts need Apollo: test the rules on a copy of the city with a
    // booster at the head of its queue.
    let with_head = |game: &GameState, item: ProductionItem| {
        let mut core = game.view().auth.cities().get(c).unwrap().clone();
        core.queue = vec![item];
        core
    };
    let auth_price = |game: &GameState| {
        let core = with_head(game, booster);
        head_hurry_price(game.view().auth, &rules, &core)
    };
    let engineer_may = |game: &GameState, item: ProductionItem| {
        let core = with_head(game, item);
        classic_rules::derived::great_people::engineer_may_hurry(game.view().auth, &rules, &core)
    };
    assert_eq!(auth_price(&game), None, "not for sale without Space Procurements");
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/freedom/space-procurements"));
    assert!(auth_price(&game).is_some());
    let warrior = ProductionItem::Unit(unit(&rules, "core:unit/warrior"));
    let monument = ProductionItem::Building(building(&rules, "core:building/monument"));
    assert!(engineer_may(&game, monument));
    assert!(!engineer_may(&game, warrior), "a great engineer hurries no units");
    assert!(!engineer_may(&game, booster));
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/order/spaceflight-pioneers"));
    assert!(engineer_may(&game, booster));
    // The command refuses a unit at the head.
    enqueue(&mut game, &rules, c, warrior);
    let at = game.view().auth.cities().get(c).unwrap().tile;
    let engineer = game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/great-engineer"));
    let hurry = Command::GreatPersonAction { unit: engineer, action: GreatPersonAction::HurryProduction };
    assert!(game.execute(&rules, hurry.clone()).is_err());
    enqueue(&mut game, &rules, c, monument);
    game.execute(&rules, hurry).unwrap();
    game.validate(&rules).unwrap();
}

#[test]
fn the_pyramids_make_workers_faster() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, 5, 5);
    let rate = |game: &GameState| classic_rules::world::builds::work_rate(game.view().auth, &rules, P0);
    assert_eq!(rate(&game), 100);
    game.debug_add_building(&rules, c, building(&rules, "core:building/pyramids"));
    assert_eq!(rate(&game), 125);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/liberty/citizenship"));
    assert_eq!(rate(&game), 150);
    // A road (two worker-turns) takes one turn and a half: still two turns.
    let at = tile(&game, 6, 5);
    let worker = game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/worker"));
    let job = |game: &GameState| game.view().auth.units().get(worker).unwrap().core.work;
    game.debug_learn(&rules, P0, rules.roles.road_tech.expect("roads need a technology"));
    game.execute(&rules, Command::BuildRoad { unit: worker }).unwrap();
    assert_eq!(job(&game).unwrap().work_left, 200);
    // The worker jobs step of the owner's end turn.
    game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
    assert_eq!(job(&game).map(|w| w.work_left), Some(50), "200 − 150");
    game.validate(&rules).unwrap();
}

#[test]
fn road_maintenance_takes_the_player_percents_and_inca_hills() {
    let rules = rules();
    let mut game = world(&rules);
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/inca"));
    city(&mut game, &rules, 5, 5);
    let flat = [tile(&game, 6, 5), tile(&game, 4, 5), tile(&game, 5, 4)];
    let hills = tile(&game, 5, 6);
    let hills_terrain = classic_rules::ids::TerrainId(rules.terrain_kinds.palette().code(classic_rules::rules::roles::Ground::Hills));
    let plains = classic_rules::ids::TerrainId(1);
    let mut terrain: Vec<(TileId, classic_rules::ids::TerrainId)> = flat.iter().map(|t| (*t, plains)).collect();
    terrain.push((hills, hills_terrain));
    game.debug_set_terrain_batch(&rules, &terrain);
    let mut roads: Vec<(TileId, Route)> = flat.iter().map(|t| (*t, Route::Road)).collect();
    roads.push((hills, Route::Railroad));
    game.debug_set_route_batch(&rules, &roads);
    let cost = |game: &GameState| route_maintenance(game.view().auth, &rules, P0);
    let handicap = game.view().auth.settings().handicap(&rules, P0).route_cost_percent as i64;
    // Inca: the railroad on hills is free, the three roads cost half.
    assert_eq!(i64::from(cost(&game)), 3 * 50 / 100 * handicap / 100);
    game.debug_set_player_civ(P0, 0);
    let plain = i64::from(cost(&game));
    assert_eq!(plain, 5 * handicap / 100, "3 roads + a railroad");
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/commerce/wagon-trains"));
    assert_eq!(i64::from(cost(&game)), 5 * 50 / 100 * handicap / 100);
}

#[test]
fn oligarchy_garrisons_cost_no_maintenance() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, 5, 5);
    let warrior = unit(&rules, "core:unit/warrior");
    let at = game.view().auth.cities().get(c).unwrap().tile;
    game.debug_spawn_unit(&rules, P0, at, warrior);
    for q in 0..9 {
        let t = tile(&game, 10 + q, 10);
        game.debug_spawn_unit(&rules, P0, t, warrior);
    }
    let cost = |game: &GameState| unit_maintenance(game.view().auth, &rules, P0);
    let before = cost(&game);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/tradition/oligarchy"));
    let after = cost(&game);
    let handicap = game.view().auth.settings().handicap(&rules, P0);
    let paid = |units: u64| {
        let free = classic_rules::derived::maintenance::FREE_UNITS + handicap.gold_free_units.max(0) as u64;
        classic_rules::derived::maintenance::unit_cost(units.saturating_sub(free) * 50, 0) as i32
            * handicap.unit_cost_percent
            / 100
    };
    let units = game.view().auth.indexes().units_of(P0).len() as u64;
    assert_eq!(before, paid(units));
    assert_eq!(after, paid(units - 1), "the garrison is free");
}

#[test]
fn negative_purchase_percents_lower_the_price() {
    // ⌊1200^0.75⌋ = 203; −50 % → 101 → 100. (The old formula cast a
    // negative percent to u64: it wrapped in release and overflowed in
    // debug builds.)
    assert_eq!(gold_purchase_price(40, -50, PurchasePercents::default()), Some(100));
    assert_eq!(gold_purchase_price(40, 0, PurchasePercents { hurry: -100, kind: 0 }), Some(0));
    assert_eq!(gold_purchase_price(40, -1, PurchasePercents::default()), None, "not for sale");
}
