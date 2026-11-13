//! The gold price of every unit and building of the classic rules, at every game speed, equals
//! Unciv's (4.22.10):
//!
//! ```text
//! base   (30·P)^0.75 · (1 + hurryCostModifier %)     IConstruction.getBaseGoldCost
//! price  the base rounded down to 10                 BaseUnitCost / Building.getStatBuyCost
//! none   a world or national wonder                  Building.canBePurchasedWithStat (isAnyWonder)
//! none   a spaceship part ("Cannot be purchased")    IConstruction.canBePurchasedWithStatReasons
//! ```
//!
//! P is the item's production cost in the city at the game's speed. The modifiers are Unciv's
//! (`Civ V - Gods & Kings/Units.json` and `Buildings.json`, Vanilla where Gods & Kings lacks the
//! entry), by the catalog entry whose provenance names the Unciv one; every other item's modifier
//! is 0. The disband gold is Unciv's too: `BaseUnit.getDisbandGold = getBaseGoldCost(civ, null) / 20`.

use classic_rules::derived::disband::disband_gold;
use classic_rules::effects::city::{city_modifiers, head_hurry_price};
use classic_rules::ids::{BuildingKindId, PlayerId, ProductionItem, UnitKindId};
use classic_rules::rules::content::{BuildingKind, UnitRole};
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);

/// Unciv's `hurryCostModifier` of the items that have one (all others: 0).
const UNCIV: &[(&str, i32)] = &[
    ("core:unit/settler", 20),
    ("core:unit/frigate", 20),
    ("core:unit/caravel", 30),
    ("core:unit/catapult", 20),
    ("core:unit/horseman", 20),
    ("core:unit/swordsman", 20),
    ("core:unit/ship-of-the-line", 20),
    ("core:unit/ballista", 20),
    ("core:unit/companion-cavalry", 20),
    ("core:unit/mohawk-warrior", 20),
    ("core:unit/legion", 20),
    ("core:unit/berserker", 20),
    ("core:unit/turtle-ship", 30),
    ("core:unit/cataphract", 20),
    ("core:unit/african-forest-elephant", 20),
    ("core:building/courthouse", 50),
    ("core:building/seaport", 25),
    ("core:building/stable", 25),
    ("core:building/water_mill", 25),
    ("core:building/circus", 25),
    ("core:building/forge", 25),
    ("core:building/windmill", 25),
    ("core:building/mint", 25),
    ("core:building/observatory", 15),
    ("core:building/garden", 25),
    ("core:building/lighthouse", 25),
    ("core:building/harbor", 25),
    ("core:building/colosseum", 25),
    ("core:building/monument", 40),
    ("core:building/temple", 25),
    ("core:building/opera_house", 10),
    ("core:building/barracks", 25),
    ("core:building/armory", 25),
    ("core:building/arsenal", 25),
    ("core:building/walls", 25),
    ("core:building/castle", 25),
    ("core:building/military_base", 25),
    ("core:building/granary", 25),
    ("core:building/workshop", 25),
    ("core:building/market", 25),
    ("core:building/bank", 15),
    ("core:building/stock_exchange", 15),
    ("core:building/library", 25),
    ("core:building/university", 15),
    ("core:building/amphitheater", 25),
    ("core:building/constabulary", 10),
    ("core:building/police_station", 10),
    ("core:building/floating_gardens", 25),
    ("core:building/mughal_fort", 25),
    ("core:building/krepost", 25),
    ("core:building/longhouse", 25),
    ("core:building/bazaar", 25),
    ("core:building/satrap_s_court", 15),
    ("core:building/paper_maker", 25),
    ("core:building/wat", 15),
    ("core:building/mud_pyramid_mosque", 25),
    ("core:building/burial_tomb", 25),
    ("core:building/stele", 40),
    ("core:building/walls_of_babylon", 25),
    ("core:building/ceilidh_hall", 10),
    ("core:building/coffee_house", 25),
];

/// Own values: buildings Unciv's Gods & Kings rules do not have (Brave New World), with the
/// modifier of the building they stand for; −1: a guild, a national wonder there, not for gold.
const OWN: &[(&str, i32)] = &[
    ("core:building/writers_guild", -1),
    ("core:building/artists_guild", -1),
    ("core:building/musicians_guild", -1),
    ("core:building/zoo", 10),
    ("core:building/hotel", 25),
    ("core:building/caravansary", 25),
    ("core:building/ikanda", 25),
    ("core:building/ducal_stable", 25),
    ("core:building/royal_library", 25),
    ("core:building/candi", 25),
    ("core:building/hanse", 15),
];

fn modifier_of(key: &str) -> i32 {
    UNCIV.iter().chain(OWN).find(|(k, _)| *k == key).map_or(0, |(_, m)| *m)
}

/// `IConstruction.getBaseGoldCost`: a Double power times the Float percent (`Int.toPercent`).
fn unciv_base(p: u32, modifier: i32) -> f64 {
    let percent = 1.0f32 + modifier as f32 / 100.0f32;
    (30.0f64 * f64::from(p)).powf(0.75) * f64::from(percent)
}

/// `getStatBuyCost`: the base as a Float (`getBaseBuyCost`), rounded down to 10.
fn unciv_price(p: u32, modifier: i32) -> u32 {
    let cost = f64::from(unciv_base(p, modifier) as f32);
    (cost / 10.0) as u32 * 10
}

fn rules() -> CompiledRuleset {
    compile_rules(classic_content::classic::classic_rules()).unwrap()
}

fn items(rules: &CompiledRuleset) -> Vec<(ProductionItem, String)> {
    let units = rules
        .units
        .iter()
        .enumerate()
        .filter(|(_, u)| !u.unbuildable)
        .map(|(i, u)| (ProductionItem::Unit(UnitKindId(i as u16)), u.key.clone()));
    let buildings = rules
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.unbuildable && b.kind != BuildingKind::Palace)
        .map(|(i, b)| (ProductionItem::Building(BuildingKindId(i as u16)), b.key.clone()));
    units.chain(buildings).collect()
}

/// Unciv's price of `item` (None: not for gold).
fn unciv_expected(rules: &CompiledRuleset, item: ProductionItem, key: &str, p: u32) -> Option<u32> {
    let for_gold = match item {
        ProductionItem::Unit(u) => rules.units[u.0 as usize].role != Some(UnitRole::SpaceshipPart),
        ProductionItem::Building(b) => {
            let def = &rules.buildings[b.0 as usize];
            !def.wonder && !matches!(def.kind, BuildingKind::WorldWonder | BuildingKind::NationalWonder)
        }
        ProductionItem::Project(_) => false,
    };
    let modifier = modifier_of(key);
    (for_gold && modifier != -1).then(|| unciv_price(p, modifier))
}

#[test]
fn every_unit_and_building_costs_the_gold_unciv_asks_at_every_speed() {
    let rules = rules();
    let all = items(&rules);
    assert!(all.len() > 200, "the classic rules: {} items", all.len());
    let mut wrong = Vec::new();
    let mut checked = 0;
    for speed in rules.speeds.iter().map(|s| s.key.clone()) {
        let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 });
        game.set_speed(&rules, &speed);
        let at = game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q: 5, r: 5 }).unwrap();
        let city = game.debug_found_city(&rules, P0, at);
        let auth = game.view().auth;
        for (item, key) in &all {
            let mut core = auth.cities().get(city).unwrap().clone();
            core.queue = vec![*item];
            let p = city_modifiers(auth, &rules, &core).production_cost(&rules, *item).unwrap();
            let ours = head_hurry_price(auth, &rules, &core);
            let unciv = unciv_expected(&rules, *item, key, p);
            checked += 1;
            if ours != unciv {
                wrong.push(format!("{speed} | {key} | P {p} | ours {ours:?} | Unciv {unciv:?}"));
            }
        }
    }
    assert!(checked >= 4 * 200);
    assert!(wrong.is_empty(), "{} of {checked} prices differ from Unciv:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn every_unit_disbands_for_the_gold_unciv_gives() {
    let rules = rules();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 });
    let at = game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q: 5, r: 5 }).unwrap();
    let city = game.debug_found_city(&rules, P0, at);
    let auth = game.view().auth;
    let core = auth.cities().get(city).unwrap();
    let mut wrong = Vec::new();
    for (i, def) in rules.units.iter().enumerate().filter(|(_, u)| !u.unbuildable) {
        let kind = UnitKindId(i as u16);
        let p = city_modifiers(auth, &rules, core).production_cost(&rules, ProductionItem::Unit(kind)).unwrap();
        let unciv = unciv_base(p, modifier_of(&def.key)) as u32 / 20;
        let ours = disband_gold(auth, &rules, P0, kind);
        if ours != unciv {
            wrong.push(format!("{} | P {p} | ours {ours} | Unciv {unciv}", def.key));
        }
    }
    assert!(wrong.is_empty(), "{} disband golds differ from Unciv:\n{}", wrong.len(), wrong.join("\n"));
}

/// The settler: 106 production at the standard speed, ⌊(30·106)^0.75 · 1.2 / 10⌋ · 10 = 500 gold
/// (423.5 · 1.2 = 508.2), not the 420 of a unit of its production without the modifier.
#[test]
fn a_settler_costs_a_fifth_more_than_a_unit_of_its_production() {
    let rules = rules();
    let settler = rules.units.iter().position(|u| u.key == "core:unit/settler").unwrap();
    assert_eq!(rules.units[settler].hurry_cost_modifier, 20);
    assert_eq!(classic_rules::world::city::hurry_price(106, 20), Some(500));
    assert_eq!(classic_rules::world::city::hurry_price(106, 0), Some(420));
}
