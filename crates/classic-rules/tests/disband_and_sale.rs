//! Disbanding a unit and selling a building on the classic ruleset, and the
//! purchase price against the production invested (Unciv 4.22.10):
//!
//! - disband (MapUnit.disband, BaseUnit.getDisbandGold, UiOnlyUnitActions.getDisbandAction): an
//!   own unit with moves left goes; on its owner's land the treasury gets ⌊(30·P)^0.75⌋ / 20;
//!   a carrier's aircraft stay in an own city, sink elsewhere;
//! - sale (City.sellBuilding, Building.isSellable, ConstructionInfoTable.showSellButton): cost / 10,
//!   one sale per city and turn, never a wonder or the palace, not in a puppet, not a building
//!   holding a great work (Civ V CvCityBuildings::IsBuildingSellable); the city sells again on its
//!   owner's next turn, and the sale survives save and load;
//! - purchase (IConstruction.getBaseGoldCost): the price is the full item's, whatever the city
//!   has invested.

use classic_content::core::core_rules;
use classic_rules::command::CommandError;
use classic_rules::derived::building_sale::{sale_gold, sale_status, SaleBlock, BUILDING_SALE_DIVISOR};
use classic_rules::derived::disband::{disband_gold, disband_status, DisbandBlock};
use classic_rules::derived::maintenance::building_maintenance;
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::BuildingKind;
use classic_rules::transaction::journal::UnitChange;
use classic_rules::world::city::CityStatus;
use classic_rules::world::state::GameConfig;
use classic_rules::classic::ClassicChange;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const WATER: TerrainId = TerrainId(4);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

/// The first building of `kind` in the ruleset.
fn first_of(rules: &CompiledRuleset, kind: BuildingKind) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.kind == kind).unwrap() as u16)
}

/// Regular buildings that cost production and hold no great work.
fn regular_buildings(rules: &CompiledRuleset) -> Vec<BuildingKindId> {
    (0..rules.buildings.len())
        .filter(|i| {
            let b = &rules.buildings[*i];
            b.kind == BuildingKind::Regular && b.cost > 0 && b.module.great_work_slots.is_none()
        })
        .map(|i| BuildingKindId(i as u16))
        .collect()
}

/// P0's capital at (8, 8), P1's at (26, 16) on a 32 × 20 map.
fn world(rules: &CompiledRuleset) -> (GameState, CityHandle, CityHandle) {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 32, height: 20, rng_seed: 3 });
    let a = tile(&game, 8, 8);
    let b = tile(&game, 26, 16);
    let ca = game.debug_found_city(rules, P0, a);
    let cb = game.debug_found_city(rules, P1, b);
    (game, ca, cb)
}

/// Every player ends its turn once (back to P0).
fn round(game: &mut GameState, rules: &CompiledRuleset) {
    for _ in 0..2 {
        let player = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player }).unwrap();
    }
}

fn treasury(game: &GameState, player: PlayerId) -> u32 {
    game.view().auth.players().treasury_of(player)
}

fn alive(game: &GameState, unit: UnitHandle) -> bool {
    game.view().auth.units().get(unit).is_some()
}

#[track_caller]
fn refused(game: &mut GameState, rules: &CompiledRuleset, command: Command, reason: &str) {
    match game.execute(rules, command) {
        Err(CommandError::RuleViolation(text)) => assert_eq!(text, reason),
        other => panic!("expected the refusal «{reason}», got {other:?}"),
    }
}

/// Unciv's disband gold of a unit of production cost `cost` and HurryCostModifier `modifier`, in
/// floating point as Unciv computes it: `((30·cost)^0.75 · modifier.toPercent()).toInt() / 20`.
fn unciv_disband_gold(cost: u32, modifier: i32) -> u32 {
    ((30.0 * f64::from(cost)).powf(0.75) * f64::from(1.0f32 + modifier as f32 / 100.0f32)) as u32 / 20
}

// ---- disband ----

/// On its owner's land a unit brings ⌊(30·P)^0.75 · (1 + m %)⌋ / 20 gold (a 40-production unit 10): every
/// kind with a cost, at Standard speed for a human owner, as Unciv's floating point gives it;
/// the unit leaves the world with the journal's `Destroyed`.
#[test]
fn a_unit_disbanded_on_own_land_brings_a_twentieth_of_its_base_gold_cost() {
    let rules = rules();
    let (mut game, capital, _) = world(&rules);
    let at = game.view().auth.cities().get(capital).unwrap().tile;
    let warrior = kind(&rules, "core:unit/warrior");
    assert_eq!(rules.units[warrior.0 as usize].cost, 40);
    assert_eq!(disband_gold(game.view().auth, &rules, P0, warrior), 10);
    for (i, def) in rules.units.iter().enumerate() {
        if def.cost == 0 || def.founder {
            continue;
        }
        let got = disband_gold(game.view().auth, &rules, P0, UnitKindId(i as u16));
        assert_eq!(got, unciv_disband_gold(def.cost, def.hurry_cost_modifier), "{}", def.key);
    }
    let unit = game.debug_spawn_unit(&rules, P0, at, warrior);
    let id = game.view().auth.units().get(unit).unwrap().core.id;
    let before = treasury(&game, P0);
    let result = game.execute(&rules, Command::DisbandUnit { unit }).unwrap();
    let destroyed = result.changes().unit_changes.iter().any(|c| matches!(c, UnitChange::Destroyed { id: d, .. } if *d == id));
    assert!(destroyed, "the journal tells the unit is gone");
    assert!(!alive(&game, unit));
    assert_eq!(treasury(&game, P0), before + 10);
    game.validate(&rules).unwrap();
}

/// Off its owner's land (neutral or foreign) a disbanded unit brings nothing.
#[test]
fn a_unit_disbanded_off_own_land_brings_no_gold() {
    let rules = rules();
    let (mut game, _, rival) = world(&rules);
    let warrior = kind(&rules, "core:unit/warrior");
    let neutral = tile(&game, 16, 2);
    assert_eq!(game.view().auth.map().owner(neutral).flatten(), None);
    let foreign = game.view().auth.cities().get(rival).unwrap().tile;
    for at in [neutral, foreign] {
        let unit = game.debug_spawn_unit(&rules, P0, at, warrior);
        assert_eq!(disband_status(game.view().auth, &rules, unit), Ok(0));
        let before = treasury(&game, P0);
        game.execute(&rules, Command::DisbandUnit { unit }).unwrap();
        assert!(!alive(&game, unit));
        assert_eq!(treasury(&game, P0), before);
    }
}

/// A unit with no moves left stays, with the reason; another nation's unit cannot be disbanded
/// on the player's turn; a unit that is gone is refused.
#[test]
fn disbanding_needs_moves_left_and_the_owners_turn() {
    let rules = rules();
    let (mut game, capital, _) = world(&rules);
    let at = game.view().auth.cities().get(capital).unwrap().tile;
    let warrior = kind(&rules, "core:unit/warrior");
    let unit = game.debug_spawn_unit(&rules, P0, at, warrior);
    let moves = game.view().auth.units().get(unit).unwrap().core.moves_left;
    assert!(moves > 0);
    let mut here = at;
    for step in [tile(&game, 9, 8), tile(&game, 10, 8), tile(&game, 11, 8)] {
        if game.view().auth.units().get(unit).unwrap().core.moves_left == 0 {
            break;
        }
        game.execute(&rules, Command::MoveUnit { unit, destination: step }).unwrap();
        here = step;
    }
    assert_ne!(here, at);
    assert_eq!(game.view().auth.units().get(unit).unwrap().core.moves_left, 0);
    refused(&mut game, &rules, Command::DisbandUnit { unit }, DisbandBlock::NoMovesLeft.reason());
    assert!(alive(&game, unit));
    // P1's unit on P0's turn.
    let theirs = game.debug_spawn_unit(&rules, P1, tile(&game, 20, 10), warrior);
    refused(&mut game, &rules, Command::DisbandUnit { unit: theirs }, classic_rules::command::NOT_YOUR_TURN);
    assert!(alive(&game, theirs));
    // Next turn the unit has moves again; once gone, it is unknown.
    round(&mut game, &rules);
    game.execute(&rules, Command::DisbandUnit { unit }).unwrap();
    assert!(game.execute(&rules, Command::DisbandUnit { unit }).is_err());
    assert_eq!(DisbandBlock::ALL.len(), 2);
}

/// A carrier disbanded in an own coastal city leaves its aircraft based in the city; at sea
/// they go down with it.
#[test]
fn a_disbanded_carrier_leaves_its_aircraft_in_a_city_and_sinks_them_at_sea() {
    let rules = rules();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 32, height: 12, rng_seed: 4 });
    let dims = game.view().auth.map().dimensions;
    let batch: Vec<(TileId, TerrainId)> = (0..dims.height as i32)
        .flat_map(|r| (8..32).map(move |q| (q, r)))
        .map(|(q, r)| (dims.to_tile(HexCoord { q, r }).unwrap(), WATER))
        .collect();
    game.debug_set_terrain_batch(&rules, &batch);
    let port = game.debug_found_city(&rules, P0, tile(&game, 7, 5));
    let port_tile = game.view().auth.cities().get(port).unwrap().tile;
    let carrier_kind = kind(&rules, "core:unit/carrier");
    let fighter = kind(&rules, "core:unit/fighter");
    for (at, stays) in [(port_tile, true), (tile(&game, 12, 5), false)] {
        let carrier = game.debug_spawn_unit(&rules, P0, at, carrier_kind);
        let fighters: Vec<UnitHandle> = (0..2).map(|_| game.debug_spawn_unit(&rules, P0, port_tile, fighter)).collect();
        for f in &fighters {
            game.execute(&rules, Command::RebaseToCarrier { unit: *f, carrier }).unwrap();
        }
        round(&mut game, &rules);
        game.execute(&rules, Command::DisbandUnit { unit: carrier }).unwrap();
        assert!(!alive(&game, carrier));
        for f in &fighters {
            assert_eq!(alive(&game, *f), stays, "carrier disbanded on {at:?}");
            if stays {
                let auth = game.view().auth;
                assert_eq!(auth.units().module_of(*f).and_then(|m| m.carried_by), None, "based in the city now");
                assert_eq!(auth.units().get(*f).unwrap().core.tile, port_tile);
            }
        }
        game.validate(&rules).unwrap();
        let left: Vec<UnitHandle> = fighters.into_iter().filter(|f| alive(&game, *f)).collect();
        for f in left {
            game.debug_remove_unit(&rules, f);
        }
    }
}

// ---- sale ----

/// A regular building sells for its cost / 10: the building and its maintenance leave the city,
/// the gold arrives, the journal tells the sale.
#[test]
fn a_building_sells_for_a_tenth_of_its_cost() {
    let rules = rules();
    let (mut game, capital, _) = world(&rules);
    let regular = regular_buildings(&rules);
    assert!(!regular.is_empty());
    for b in &regular {
        assert_eq!(sale_gold(&rules, *b), rules.buildings[b.0 as usize].cost / BUILDING_SALE_DIVISOR);
    }
    let sold = *regular.iter().find(|b| rules.buildings[b.0 as usize].maintenance > 0).unwrap();
    game.debug_add_building(&rules, capital, sold);
    let upkeep = building_maintenance(game.view().auth, &rules, P0);
    let before = treasury(&game, P0);
    let gold = rules.buildings[sold.0 as usize].cost / 10;
    let result = game.execute(&rules, Command::SellBuilding { player: P0, city: capital, building: sold }).unwrap();
    let told = result
        .changes()
        .module_changes
        .iter()
        .any(|c| *c == ClassicChange::BuildingSold { player: P0, city: capital, building: sold, gold });
    assert!(told, "the journal tells the sale");
    assert!(!game.view().auth.cities().get(capital).unwrap().buildings.contains(&sold));
    assert_eq!(treasury(&game, P0), before + gold);
    let maintenance = rules.buildings[sold.0 as usize].maintenance as i32;
    assert_eq!(building_maintenance(game.view().auth, &rules, P0), upkeep - maintenance);
    game.validate(&rules).unwrap();
}

/// One sale per city and turn: a second one waits for the owner's next turn, another city may
/// sell on the same turn.
#[test]
fn a_city_sells_one_building_a_turn() {
    let rules = rules();
    let (mut game, capital, _) = world(&rules);
    let second = game.debug_found_city(&rules, P0, tile(&game, 14, 8));
    let regular = regular_buildings(&rules);
    let (a, b) = (regular[0], regular[1]);
    for city in [capital, second] {
        game.debug_add_building(&rules, city, a);
        game.debug_add_building(&rules, city, b);
    }
    game.execute(&rules, Command::SellBuilding { player: P0, city: capital, building: a }).unwrap();
    refused(&mut game, &rules, Command::SellBuilding { player: P0, city: capital, building: b }, SaleBlock::SoldThisTurn.reason());
    game.execute(&rules, Command::SellBuilding { player: P0, city: second, building: a }).unwrap();
    round(&mut game, &rules);
    assert_eq!(sale_status(game.view().auth, &rules, P0, capital, b), Ok(sale_gold(&rules, b)));
    game.execute(&rules, Command::SellBuilding { player: P0, city: capital, building: b }).unwrap();
}

/// Wonders and the palace are never sold, nor a building holding a great work, nor anything in
/// a puppet or another nation's city, nor a building the city lacks.
#[test]
fn wonders_the_palace_great_works_and_puppets_are_not_for_sale() {
    let rules = rules();
    let (mut game, capital, rival) = world(&rules);
    let palace = first_of(&rules, BuildingKind::Palace);
    assert!(game.view().auth.cities().get(capital).unwrap().buildings.contains(&palace), "the capital has the palace");
    let sell = |building| Command::SellBuilding { player: P0, city: capital, building };
    refused(&mut game, &rules, sell(palace), SaleBlock::Palace.reason());
    for wonder in [first_of(&rules, BuildingKind::WorldWonder), first_of(&rules, BuildingKind::NationalWonder)] {
        game.debug_add_building(&rules, capital, wonder);
        refused(&mut game, &rules, sell(wonder), SaleBlock::Wonder.reason());
    }
    let regular = regular_buildings(&rules)[0];
    refused(&mut game, &rules, sell(regular), SaleBlock::NotInCity.reason());
    // A great writer's work in the amphitheater keeps it.
    let amphitheater = building(&rules, "core:building/amphitheater");
    game.debug_add_building(&rules, capital, amphitheater);
    let at = game.view().auth.cities().get(capital).unwrap().tile;
    let writer = game.debug_spawn_unit(&rules, P0, at, kind(&rules, "core:unit/great-writer"));
    game.execute(&rules, Command::CreateGreatWork { unit: writer }).unwrap();
    refused(&mut game, &rules, sell(amphitheater), SaleBlock::HoldsGreatWork.reason());
    // Another nation's city, and P1 on P0's turn.
    let rival_building = regular;
    game.debug_add_building(&rules, rival, rival_building);
    refused(
        &mut game,
        &rules,
        Command::SellBuilding { player: P0, city: rival, building: rival_building },
        SaleBlock::NotYourCity.reason(),
    );
    refused(
        &mut game,
        &rules,
        Command::SellBuilding { player: P1, city: rival, building: rival_building },
        classic_rules::command::NOT_YOUR_TURN,
    );
    // A puppet sells nothing.
    game.debug_add_building(&rules, capital, regular);
    game.debug_set_city_status(capital, CityStatus::Puppet);
    refused(&mut game, &rules, sell(regular), SaleBlock::CityTakesNoOrders.reason());
    game.debug_set_city_status(capital, CityStatus::Own);
    game.execute(&rules, sell(regular)).unwrap();
    assert_eq!(SaleBlock::ALL.len(), 8);
}

/// The city that sold this turn still may not sell after a save and a load; the loaded game
/// has the same hash; on the next turn the city sells again.
#[test]
fn a_sale_this_turn_survives_save_and_load() {
    let rules = rules();
    let (mut game, capital, _) = world(&rules);
    let regular = regular_buildings(&rules);
    let (a, b) = (regular[0], regular[1]);
    game.debug_add_building(&rules, capital, a);
    game.debug_add_building(&rules, capital, b);
    let quiet = authoritative_hash(&game);
    let saved = encode(&game, &rules);
    let reloaded = decode(&saved, &rules).unwrap();
    assert_eq!(authoritative_hash(&reloaded), quiet);
    game.execute(&rules, Command::SellBuilding { player: P0, city: capital, building: a }).unwrap();
    let bytes = classic_rules::save_codec::to_bytes(&encode(&game, &rules));
    let mut loaded = decode(&classic_rules::save_codec::from_bytes(&bytes).unwrap(), &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    refused(&mut loaded, &rules, Command::SellBuilding { player: P0, city: capital, building: b }, SaleBlock::SoldThisTurn.reason());
    round(&mut loaded, &rules);
    loaded.execute(&rules, Command::SellBuilding { player: P0, city: capital, building: b }).unwrap();
}

// ---- purchase ----

/// Unciv prices a purchase on the item's whole production cost (IConstruction.getBaseGoldCost,
/// with `getProductionCost`, never the work done): the price does not fall with the production
/// the city has put in, and the stock it invested stays for the next order.
#[test]
fn the_purchase_price_does_not_count_the_production_invested() {
    let rules = rules();
    let (mut game, capital, _) = world(&rules);
    let warrior = kind(&rules, "core:unit/warrior");
    for _ in 0..2 {
        game.execute(&rules, Command::EnqueueProduction { player: P0, city: capital, item: ProductionItem::Unit(warrior) }).unwrap();
    }
    let price = |game: &GameState| {
        let auth = game.view().auth;
        classic_rules::effects::city::head_hurry_price(auth, &rules, auth.cities().get(capital).unwrap()).unwrap()
    };
    let fresh = price(&game);
    assert_eq!(game.view().auth.cities().get(capital).unwrap().production, 0);
    round(&mut game, &rules);
    let stock = game.view().auth.cities().get(capital).unwrap().production;
    assert!(stock > 0, "a turn of production is in the warrior");
    assert_eq!(price(&game), fresh, "the price is the whole warrior's");
    game.debug_earn(P0, fresh);
    game.execute(&rules, Command::HurryProduction { player: P0, city: capital }).unwrap();
    let after = game.view().auth.cities().get(capital).unwrap();
    assert_eq!(after.production, stock, "the invested stock goes to the next order");
    assert_eq!(after.queue.len(), 1);
}

// ---- journal ----

/// A journal with a disband and a sale (command tags 82 and 83) replays from the save to the
/// same world.
#[test]
fn a_journal_with_a_disband_and_a_sale_replays() {
    let rules = rules();
    let (mut game, capital, _) = world(&rules);
    let at = game.view().auth.cities().get(capital).unwrap().tile;
    let regular = regular_buildings(&rules)[0];
    game.debug_add_building(&rules, capital, regular);
    let unit = game.debug_spawn_unit(&rules, P0, at, kind(&rules, "core:unit/warrior"));
    let save = kernel::persistence::codec::to_bytes(&encode(&game, &rules));
    let mut journal = game.begin_journal(&rules);
    game.execute(&rules, Command::DisbandUnit { unit }).unwrap();
    game.execute(&rules, Command::SellBuilding { player: P0, city: capital, building: regular }).unwrap();
    round(&mut game, &rules);
    journal.extend(game.take_journal());
    let replayed = kernel::replay::journal::replay(&save, &journal, &rules).expect("the journal replays");
    assert_eq!(replayed.commands, 4, "disband, sale and two ends of turn");
    assert_eq!(authoritative_hash(&replayed.game), authoritative_hash(&game));
}

// ---- the queue and the hit points after a sale (review of rules/actions) ----

/// Pairs (dependent, requirement): both regular, the dependent costs production and needs the
/// requirement in its city (the class of cases, not one building).
fn dependent_pairs(rules: &CompiledRuleset) -> Vec<(BuildingKindId, BuildingKindId)> {
    regular_buildings(rules)
        .into_iter()
        .filter(|b| !rules.buildings[b.0 as usize].unbuildable)
        .filter_map(|b| {
            let r = *rules.buildings[b.0 as usize].requires_buildings.first()?;
            (rules.buildings[r.0 as usize].kind == BuildingKind::Regular).then_some((b, r))
        })
        .collect()
}

fn queue_of(game: &GameState, city: CityHandle) -> Vec<ProductionItem> {
    game.view().auth.cities().get(city).unwrap().queue.clone()
}

fn learn_for(game: &mut GameState, rules: &CompiledRuleset, buildings: &[BuildingKindId]) {
    for b in buildings {
        if let Some(t) = rules.buildings[b.0 as usize].required_tech {
            game.debug_learn(rules, P0, t);
        }
    }
}

/// Unciv drops a queued building that is no longer buildable (CityConstructions.validateConstructionQueue,
/// `isBuildable`): selling the requirement takes the dependent out of the queue at once, with a record.
#[test]
fn selling_a_requirement_takes_the_dependent_order_out_of_the_queue() {
    use classic_rules::transaction::journal::{CityChange, OrderRemoval};
    let rules = rules();
    let mut checked = 0;
    for (dep, req) in dependent_pairs(&rules) {
        let (mut game, capital, _) = world(&rules);
        learn_for(&mut game, &rules, &[dep, req]);
        game.debug_add_building(&rules, capital, req);
        let order = ProductionItem::Building(dep);
        if game.execute(&rules, Command::EnqueueProduction { player: P0, city: capital, item: order }).is_err() {
            continue;
        }
        let Ok(result) = game.execute(&rules, Command::SellBuilding { player: P0, city: capital, building: req }) else {
            continue;
        };
        let key = &rules.buildings[dep.0 as usize].key;
        assert!(!queue_of(&game, capital).contains(&order), "{key} still queued after its requirement was sold");
        let told = result.changes().city_changes.iter().any(|c| {
            matches!(c, CityChange::OrderRemoved { city, owner, item, reason }
                if *city == capital && *owner == P0 && *item == order && *reason == OrderRemoval::NeedsBuilding)
        });
        assert!(told, "{key}: the journal tells the removal");
        game.debug_earn(P0, 100_000);
        for _ in 0..4 {
            round(&mut game, &rules);
        }
        assert!(!game.view().auth.cities().get(capital).unwrap().buildings.contains(&dep), "{key} built without its requirement");
        game.validate(&rules).unwrap();
        checked += 1;
    }
    assert!(checked > 0, "the data has dependent buildings");
}

/// An order that lost its requirement without a sweep (a save from before) is not bought and not built:
/// the purchase is refused with the reason, nothing is spent, and the drain takes the order out.
#[test]
fn an_order_without_its_requirement_is_neither_bought_nor_built() {
    use classic_rules::command::Refusal;
    let rules = rules();
    let mut checked = 0;
    for (dep, req) in dependent_pairs(&rules) {
        let (mut game, capital, _) = world(&rules);
        learn_for(&mut game, &rules, &[dep, req]);
        game.debug_add_building(&rules, capital, req);
        let order = ProductionItem::Building(dep);
        if game.execute(&rules, Command::EnqueueProduction { player: P0, city: capital, item: order }).is_err() {
            continue;
        }
        game.debug_remove_building(capital, req);
        game.debug_earn(P0, 100_000);
        let gold = treasury(&game, P0);
        let key = &rules.buildings[dep.0 as usize].key;
        let error = game.execute(&rules, Command::HurryProduction { player: P0, city: capital }).unwrap_err();
        assert_eq!(error, CommandError::Refused(Refusal::NeedsBuilding), "{key}");
        assert_eq!(treasury(&game, P0), gold, "{key}: nothing spent");
        for _ in 0..4 {
            round(&mut game, &rules);
        }
        let city = game.view().auth.cities().get(capital).unwrap();
        assert!(!city.buildings.contains(&dep), "{key} built without its requirement");
        assert!(!city.queue.contains(&order), "{key} left in the queue");
        checked += 1;
    }
    assert!(checked > 0);
}

/// Unciv keeps a city's health when a building goes and heals only up to the maximum; the rules here
/// store damage, so the sale re-bases it: hit points = min(before, new maximum), never below 1.
#[test]
fn selling_a_building_of_defence_keeps_the_hit_points_up_to_the_new_maximum() {
    use classic_rules::combat::city::{hit_points, max_hit_points};
    let rules = rules();
    let mut defences = 0;
    for b in regular_buildings(&rules) {
        for hurt in [0u16, 30, 120, 10_000] {
            let (mut game, capital, _) = world(&rules);
            game.debug_add_building(&rules, capital, b);
            let max_with = max_hit_points(game.view().auth, &rules, game.view().auth.cities().get(capital).unwrap());
            game.debug_set_city_damage(capital, hurt.min(max_with - 1));
            let before = hit_points(game.view().auth, &rules, game.view().auth.cities().get(capital).unwrap());
            if game.execute(&rules, Command::SellBuilding { player: P0, city: capital, building: b }).is_err() {
                continue;
            }
            let core = game.view().auth.cities().get(capital).unwrap();
            let max = max_hit_points(game.view().auth, &rules, core);
            if hurt == 0 && max < max_with {
                defences += 1;
            }
            let key = &rules.buildings[b.0 as usize].key;
            assert_eq!(hit_points(game.view().auth, &rules, core), before.min(max).max(1), "{key} hurt {hurt}");
            assert!(core.damage < max, "{key}: the city is not left at 0");
            game.validate(&rules).unwrap();
        }
    }
    assert!(defences > 0, "the data has a sellable building that gives the city hit points");
}

/// A national wonder that wants a building in every city loses its order when another city sells
/// that building (the sale reaches the owner's other queues).
#[test]
fn selling_in_another_city_drops_the_national_wonder_that_wanted_the_building_everywhere() {
    use classic_rules::transaction::journal::{CityChange, OrderRemoval};
    let rules = rules();
    let mut checked = 0;
    for (i, wonder) in rules.buildings.iter().enumerate() {
        let (wonder_id, Some(req)) = (BuildingKindId(i as u16), wonder.requires_buildings_in_empire.first().copied()) else { continue };
        if wonder.kind != BuildingKind::NationalWonder || rules.buildings[req.0 as usize].kind != BuildingKind::Regular {
            continue;
        }
        let (mut game, capital, _) = world(&rules);
        let second = game.debug_found_city(&rules, P0, tile(&game, 8, 12));
        let mut needed = vec![wonder_id, req];
        needed.extend(wonder.requires_buildings.iter().copied());
        learn_for(&mut game, &rules, &needed);
        for city in [capital, second] {
            game.debug_add_building(&rules, city, req);
        }
        for r in &wonder.requires_buildings {
            game.debug_add_building(&rules, capital, *r);
        }
        let order = ProductionItem::Building(wonder_id);
        if game.execute(&rules, Command::EnqueueProduction { player: P0, city: capital, item: order }).is_err() {
            continue;
        }
        let Ok(result) = game.execute(&rules, Command::SellBuilding { player: P0, city: second, building: req }) else { continue };
        let key = &wonder.key;
        assert!(!queue_of(&game, capital).contains(&order), "{key} still queued");
        let told = result.changes().city_changes.iter().any(|c| {
            matches!(c, CityChange::OrderRemoved { city, item, reason, .. }
                if *city == capital && *item == order && *reason == OrderRemoval::NeedsBuilding)
        });
        assert!(told, "{key}: the journal tells the removal");
        checked += 1;
    }
    assert!(checked > 0, "the data has a national wonder that wants a building in every city");
}

// ---- the queue after a new city (rules/hurry-modifier) ----

/// National wonders that want a regular building in every city, with that building (the class of
/// cases from the data, not one wonder).
fn empire_wide_wonders(rules: &CompiledRuleset) -> Vec<(BuildingKindId, BuildingKindId)> {
    rules
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, w)| w.kind == BuildingKind::NationalWonder && !w.unbuildable)
        .filter_map(|(i, w)| {
            let req = *w.requires_buildings_in_empire.first()?;
            (rules.buildings[req.0 as usize].kind == BuildingKind::Regular).then_some((BuildingKindId(i as u16), req))
        })
        .collect()
}

/// Unciv keeps only buildable orders at the city's turn (CityConstructions.validateConstructionQueue,
/// `isBuildable`; `ConditionalBuildingBuiltAll` counts every non-puppet city): a city founded after a
/// national wonder was queued, without the building the wonder wants in every city, takes the order
/// out at the next turn of the queue's city, with the record the sale gives (`NeedsBuilding`). The
/// purchase is refused in between, and nothing is built.
#[test]
fn founding_a_city_drops_the_national_wonder_that_wanted_the_building_everywhere() {
    use classic_rules::command::Refusal;
    use classic_rules::transaction::journal::{CityChange, OrderRemoval};
    let rules = rules();
    let mut checked = 0;
    for (wonder_id, req) in empire_wide_wonders(&rules) {
        let wonder = &rules.buildings[wonder_id.0 as usize];
        let (mut game, capital, _) = world(&rules);
        let mut needed = vec![wonder_id, req];
        needed.extend(wonder.requires_buildings.iter().copied());
        learn_for(&mut game, &rules, &needed);
        for b in std::iter::once(&req).chain(&wonder.requires_buildings) {
            game.debug_add_building(&rules, capital, *b);
        }
        let order = ProductionItem::Building(wonder_id);
        if game.execute(&rules, Command::EnqueueProduction { player: P0, city: capital, item: order }).is_err() {
            continue;
        }
        let key = &wonder.key;
        game.debug_found_city(&rules, P0, tile(&game, 8, 13));
        game.debug_earn(P0, 100_000);
        let gold = treasury(&game, P0);
        let error = game.execute(&rules, Command::HurryProduction { player: P0, city: capital }).unwrap_err();
        assert_eq!(error, CommandError::Refused(Refusal::NeedsBuilding), "{key}");
        assert_eq!(treasury(&game, P0), gold, "{key}: nothing spent");
        let mut told = false;
        for _ in 0..4 {
            for _ in 0..2 {
                let player = game.view().auth.active_player();
                let result = game.execute(&rules, Command::EndTurn { player }).unwrap();
                told |= result.changes().city_changes.iter().any(|c| {
                    matches!(c, CityChange::OrderRemoved { city, owner, item, reason }
                        if *city == capital && *owner == P0 && *item == order && *reason == OrderRemoval::NeedsBuilding)
                });
            }
        }
        assert!(told, "{key}: the journal tells the removal");
        let core = game.view().auth.cities().get(capital).unwrap();
        assert!(!core.queue.contains(&order), "{key} still queued after a city without the building was founded");
        assert!(!core.buildings.contains(&wonder_id), "{key} built");
        game.validate(&rules).unwrap();
        checked += 1;
    }
    assert!(checked > 0, "the data has a national wonder that wants a building in every city");
}

/// The unique building that replaces the one a national wonder wants in every city counts for it
/// (Unciv: `containsBuildingOrEquivalent`): the wonder is ordered and stays in the queue across turns.
#[test]
fn a_unique_building_counts_in_every_city_for_the_one_it_replaces() {
    let rules = rules();
    let mut checked = 0;
    for (wonder_id, req) in empire_wide_wonders(&rules) {
        let wonder = &rules.buildings[wonder_id.0 as usize];
        let uniques = (0..rules.buildings.len()).filter(|u| rules.buildings[*u].replaces == Some(req));
        for unique in uniques.map(|u| BuildingKindId(u as u16)) {
            let (mut game, capital, _) = world(&rules);
            let second = game.debug_found_city(&rules, P0, tile(&game, 8, 13));
            let mut needed = vec![wonder_id, req];
            needed.extend(wonder.requires_buildings.iter().copied());
            learn_for(&mut game, &rules, &needed);
            for city in [capital, second] {
                game.debug_add_building(&rules, city, unique);
            }
            for r in &wonder.requires_buildings {
                game.debug_add_building(&rules, capital, *r);
            }
            let order = ProductionItem::Building(wonder_id);
            let key = format!("{} with {}", wonder.key, rules.buildings[unique.0 as usize].key);
            match game.execute(&rules, Command::EnqueueProduction { player: P0, city: capital, item: order }) {
                Ok(_) => {}
                Err(CommandError::RuleViolation(text)) if text != "needs a building in every city" => continue,
                Err(e) => panic!("{key}: {e:?}"),
            }
            round(&mut game, &rules);
            assert!(queue_of(&game, capital).contains(&order), "{key}: the order stays");
            checked += 1;
        }
    }
    assert!(checked > 0, "the data has a unique building that replaces a requirement of a national wonder");
}

// ---- the requirement "in every city" and the status of a city (review of rules/hurry-modifier) ----

/// P0's capital with the wonder's requirements and the national wonder `wonder` queued in it, or
/// None where the data does not let the order stand.
fn empire_wide_order(rules: &CompiledRuleset, wonder_id: BuildingKindId, req: BuildingKindId) -> Option<(GameState, CityHandle)> {
    let wonder = &rules.buildings[wonder_id.0 as usize];
    let (mut game, capital, _) = world(rules);
    let mut needed = vec![wonder_id, req];
    needed.extend(wonder.requires_buildings.iter().copied());
    learn_for(&mut game, rules, &needed);
    for b in std::iter::once(&req).chain(&wonder.requires_buildings) {
        game.debug_add_building(rules, capital, *b);
    }
    let order = ProductionItem::Building(wonder_id);
    game.execute(rules, Command::EnqueueProduction { player: P0, city: capital, item: order }).ok()?;
    Some((game, capital))
}

fn end_turn_of(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) -> Vec<classic_rules::transaction::journal::CityChange> {
    game.execute(rules, Command::EndTurn { player }).unwrap().changes().city_changes.clone()
}

fn dropped(changes: &[classic_rules::transaction::journal::CityChange], city: CityHandle, order: ProductionItem) -> bool {
    use classic_rules::transaction::journal::{CityChange, OrderRemoval};
    changes.iter().any(|c| {
        matches!(c, CityChange::OrderRemoved { city: at, item, reason, .. }
            if *at == city && *item == order && *reason == OrderRemoval::NeedsBuilding)
    })
}

/// Unciv moves a captured city to the conqueror only when the conqueror decides
/// (`CityConquestFunctions.moveToCiv`), and `ConditionalBuildingBuiltAll` skips puppets: a city
/// that awaits the decision, or is a puppet, does not take the national wonder out of the queue.
/// Annexing it makes it a city of the empire, and the order goes at the next turn.
#[test]
fn a_city_awaiting_the_decision_or_a_puppet_does_not_count_for_every_city_until_annexed() {
    let rules = rules();
    let mut checked = 0;
    for (wonder_id, req) in empire_wide_wonders(&rules) {
        let key = rules.buildings[wonder_id.0 as usize].key.clone();
        let order = ProductionItem::Building(wonder_id);
        for status in [CityStatus::AwaitingDecision, CityStatus::Puppet] {
            let Some((mut game, capital)) = empire_wide_order(&rules, wonder_id, req) else { continue };
            let second = game.debug_found_city(&rules, P0, tile(&game, 8, 13));
            game.debug_set_city_status(second, status);
            let changes = end_turn_of(&mut game, &rules, P0);
            assert!(!dropped(&changes, capital, order), "{key} with a city {status:?}: the order stays");
            round(&mut game, &rules);
            assert!(queue_of(&game, capital).contains(&order), "{key} with a city {status:?}: still queued");
        }
        let Some((mut game, capital)) = empire_wide_order(&rules, wonder_id, req) else { continue };
        let second = game.debug_found_city(&rules, P0, tile(&game, 8, 13));
        game.debug_set_city_status(second, CityStatus::AwaitingDecision);
        let annex = Command::DecideCity { player: P0, city: second, decision: classic_rules::command::CityDecision::Annex };
        game.execute(&rules, annex).unwrap();
        let changes = end_turn_of(&mut game, &rules, P0);
        assert!(dropped(&changes, capital, order), "{key}: an annexed city counts, the order goes");
        checked += 1;
    }
    assert!(checked > 0, "the data has a national wonder that wants a building in every city");
}

/// A city of the other player, empty of defenders and one hit point from falling, taken by a
/// warrior that stands next to it. `attacker` is the active player.
fn capture(game: &mut GameState, rules: &CompiledRuleset, attacker: PlayerId, city: CityHandle) {
    let at = game.view().auth.cities().get(city).unwrap().tile;
    let from = game.view().auth.map().dimensions.neighbors(at).as_slice()[0];
    let warrior = game.debug_spawn_unit(rules, attacker, from, kind(rules, "core:unit/warrior"));
    let max = classic_rules::combat::city::max_hit_points(game.view().auth, rules, game.view().auth.cities().get(city).unwrap());
    game.debug_set_city_damage(city, max - 1);
    game.execute(rules, Command::AttackCity { attacker: warrior, city }).unwrap();
}

fn declare_war(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, enemy: PlayerId) {
    game.debug_meet(player, enemy);
    game.execute(rules, Command::DeclareWar { player, enemy }).unwrap();
}

/// The real capture: the city P0 takes from P1 awaits the decision and, with the order in the
/// capital's queue, leaves it alone through the end of the turn that makes it a puppet.
#[test]
fn a_captured_city_does_not_take_the_national_wonder_out_of_the_queue() {
    let rules = rules();
    let mut checked = 0;
    for (wonder_id, req) in empire_wide_wonders(&rules) {
        let key = rules.buildings[wonder_id.0 as usize].key.clone();
        let order = ProductionItem::Building(wonder_id);
        let Some((mut game, capital)) = empire_wide_order(&rules, wonder_id, req) else { continue };
        let prize = game.debug_found_city(&rules, P1, tile(&game, 8, 13));
        declare_war(&mut game, &rules, P0, P1);
        capture(&mut game, &rules, P0, prize);
        let core = game.view().auth.cities().get(prize).unwrap();
        assert_eq!((core.owner, core.status), (P0, CityStatus::AwaitingDecision), "{key}");
        let changes = end_turn_of(&mut game, &rules, P0);
        assert!(!dropped(&changes, capital, order), "{key}: the captured city is not counted");
        assert_eq!(game.view().auth.cities().get(prize).unwrap().status, CityStatus::Puppet, "{key}");
        round(&mut game, &rules);
        assert!(queue_of(&game, capital).contains(&order), "{key}: the order stays beside a puppet");
        checked += 1;
    }
    assert!(checked > 0, "the data has a national wonder that wants a building in every city");
}

/// Losing the one city without the building lifts the empire-wide requirement: the order is refused
/// for it while the city is the empire's, and no longer once the other player has taken it.
#[test]
fn losing_the_city_without_the_building_lets_the_national_wonder_be_ordered() {
    let rules = rules();
    let mut checked = 0;
    for (wonder_id, req) in empire_wide_wonders(&rules) {
        let key = rules.buildings[wonder_id.0 as usize].key.clone();
        let wonder = &rules.buildings[wonder_id.0 as usize];
        let (mut game, capital, _) = world(&rules);
        let mut needed = vec![wonder_id, req];
        needed.extend(wonder.requires_buildings.iter().copied());
        learn_for(&mut game, &rules, &needed);
        for b in std::iter::once(&req).chain(&wonder.requires_buildings) {
            game.debug_add_building(&rules, capital, *b);
        }
        let weak = game.debug_found_city(&rules, P0, tile(&game, 8, 13));
        let order = ProductionItem::Building(wonder_id);
        let enqueue = Command::EnqueueProduction { player: P0, city: capital, item: order };
        match game.execute(&rules, enqueue.clone()) {
            Err(CommandError::RuleViolation(text)) if text == "needs a building in every city" => {}
            Err(_) => continue,
            Ok(_) => panic!("{key}: ordered while a city lacks the building"),
        }
        end_turn_of(&mut game, &rules, P0);
        declare_war(&mut game, &rules, P1, P0);
        capture(&mut game, &rules, P1, weak);
        assert_eq!(game.view().auth.cities().get(weak).unwrap().owner, P1, "{key}");
        end_turn_of(&mut game, &rules, P1);
        // Other rules (a holy city, a coast) may still refuse the order; the empire-wide one does not.
        match game.execute(&rules, enqueue) {
            Ok(_) => assert!(queue_of(&game, capital).contains(&order), "{key}"),
            Err(CommandError::RuleViolation(text)) => assert_ne!(text, "needs a building in every city", "{key}"),
            Err(e) => panic!("{key}: {e:?}"),
        }
        checked += 1;
    }
    assert!(checked > 0, "the data has a national wonder that wants a building in every city");
}
