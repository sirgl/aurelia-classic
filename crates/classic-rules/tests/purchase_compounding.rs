//! Gold purchase percents multiply, as Unciv has them (`Building`/`BaseUnitCost.
//! getStatBuyCost`, `CityExpansionManager.getGoldCostOfTile`): every unique
//! multiplies the price by its own `1 + percent`; the kernel used to add the
//! percents of one kind (Mercantilism and Big Ben made -40 %, not -36.25 %).

use classic_content::core::core_rules;
use classic_rules::ClassicGame;
use classic_rules::effects::city::{city_modifiers, head_hurry_price};
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, PolicyId, ProductionItem, TileId, UnitKindId};
use classic_rules::{compile_rules, CompiledRuleset, GameConfig, GameState};

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

fn nation(rules: &CompiledRuleset, key: &str) -> u8 {
    rules.nations.iter().position(|c| c.key == key).unwrap() as u8
}

fn city(game: &mut GameState, rules: &CompiledRuleset, q: i32, r: i32) -> CityHandle {
    let t = tile(game, q, r);
    game.debug_found_city(rules, P0, t)
}

/// Unciv's gold price of an item (`getBaseGoldCost` + `getStatBuyCost`) in
/// floating point: (30 cost)^0.75 × (1 + modifier %), then every source of
/// the city multiplies it by (1 + percent), then down to a multiple of 10.
fn unciv_price(cost: u32, modifier: i32, sources: &[i32]) -> u32 {
    let mut price = (30.0 * f64::from(cost)).powf(0.75) * (1.0 + f64::from(modifier) / 100.0);
    for p in sources {
        price *= 1.0 + f64::from(*p) / 100.0;
    }
    (price as f32 / 10.0) as u32 * 10
}

/// Every priced item of the rule set, in cities holding Mercantilism, Big Ben,
/// Mobilization and Skyscrapers one after the other, costs what Unciv's
/// formula gives: the sources multiply, they do not add.
#[test]
fn every_gold_price_multiplies_its_sources_as_unciv() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, 5, 5);
    let other = city(&mut game, &rules, 12, 5);
    let items: Vec<(ProductionItem, bool)> = (0..rules.units.len())
        .map(|i| (ProductionItem::Unit(UnitKindId(i as u16)), true))
        .chain((0..rules.buildings.len()).map(|i| (ProductionItem::Building(BuildingKindId(i as u16)), false)))
        .collect();
    // (the sources held, for units, for buildings)
    let stages: [(&str, &[i32], &[i32]); 4] = [
        ("nothing", &[], &[]),
        ("Mercantilism", &[-25], &[-25]),
        ("Mercantilism, Mobilization, Skyscrapers", &[-25, -25], &[-25, -25]),
        ("and Big Ben", &[-25, -15, -25], &[-25, -15, -25]),
    ];
    let mut checked = 0;
    let mut stage_no = 0;
    for (label, units, buildings) in stages {
        match stage_no {
            1 => game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/commerce/mercantilism")),
            2 => {
                game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/autocracy/mobilization"));
                game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/order/skyscrapers"));
            }
            3 => game.debug_add_building(&rules, other, building(&rules, "core:building/big_ben")),
            _ => {}
        }
        stage_no += 1;
        for (item, is_unit) in &items {
            let mut core = game.view().auth.cities().get(c).unwrap().clone();
            core.queue = vec![*item];
            let auth = game.view().auth;
            let Some(price) = head_hurry_price(auth, &rules, &core) else { continue };
            let cost = city_modifiers(auth, &rules, &core).production_cost(&rules, *item).unwrap();
            let modifier = match item {
                ProductionItem::Unit(u) => rules.units[u.0 as usize].hurry_cost_modifier,
                ProductionItem::Building(b) => rules.buildings[b.0 as usize].hurry_cost_modifier,
                ProductionItem::Project(_) => continue,
            };
            let sources = if *is_unit { units } else { buildings };
            assert_eq!(price, unciv_price(cost, modifier, sources), "{item:?} (cost {cost}, modifier {modifier}) with {label}");
            checked += 1;
        }
    }
    assert!(checked > 200, "the sweep priced {checked} items");
}

/// The tile price takes the percents of America, Angkor Wat and the Krepost
/// one after the other (Unciv: `cost *= unique.params[0].toPercent()` for
/// every unique), not their sum.
#[test]
fn tile_purchase_percents_multiply() {
    let rules = rules();
    let mut game = world(&rules);
    game.debug_set_player_nation(P0, nation(&rules, "core:nation/america"));
    let c = city(&mut game, &rules, 5, 5);
    let price = |game: &GameState| {
        let auth = game.view().auth;
        city_modifiers(auth, &rules, auth.cities().get(c).unwrap()).adjusted_cost(classic_rules::rules::effects::CostTarget::TilePurchase, 200)
    };
    assert_eq!(price(&game), 100, "America: -50 %");
    game.debug_add_building(&rules, c, building(&rules, "core:building/angkor_wat"));
    assert_eq!(price(&game), 75, "-50 % then -25 %: 200 x 0.5 x 0.75, not 200 x 0.25 = 50");
    game.debug_add_building(&rules, c, building(&rules, "core:building/krepost"));
    assert_eq!(price(&game), 56, "and the Krepost's -25 %: 56.25 down");
}
