//! Reviewer's tests of rules/city-purchase (794d623 + 0fbd890). Each one is a hole; a test that
//! fails names it. Own copy only, never committed.

use classic_rules::effects::city::{city_modifiers, head_hurry_price};
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, PolicyId, ProductionItem, TileId, UnitKindId};
use classic_rules::rules::effects::CostTarget;
use classic_rules::world::city::CityStatus;
use classic_rules::{compile_rules, CompiledRuleset, GameConfig, GameState};
use classic_rules::command::Command;
use classic_rules::ClassicGame;

const P0: PlayerId = PlayerId(0);

fn rules() -> CompiledRuleset {
    compile_rules(classic_content::classic::classic_rules()).unwrap()
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

/// Unciv `getStatBuyCost`, faithfully: base is a Float (`getBaseBuyCost` returns Float?), the
/// discounts multiply in Double, then `(cost / 10f).toInt() * 10`.
fn unciv_price_exact(cost: u32, modifier: i32, sources: &[i32]) -> u32 {
    let base = ((30.0 * f64::from(cost)).powf(0.75) * (1.0 + f64::from(modifier) / 100.0)) as f32;
    let mut price = f64::from(base);
    for p in sources {
        price *= 1.0 + f64::from(*p) / 100.0;
    }
    (price / 10.0) as u32 * 10
}

/// HOLE 1 (non-blocking for P1, known): Unciv `getGoldCostOfTile` ends in `cost.roundToInt()`,
/// the kernel truncates. America alone: 65 x 0.5 = 32.5 -> Unciv 33.
#[test]
fn tile_price_rounds_like_unciv_roundtoint() {
    let rules = rules();
    let mut game = world(&rules);
    game.debug_set_player_nation(P0, nation(&rules, "core:nation/america"));
    let c = city(&mut game, &rules, 5, 5);
    let auth = game.view().auth;
    let mods = city_modifiers(auth, &rules, auth.cities().get(c).unwrap());
    assert_eq!(mods.adjusted_cost(CostTarget::TilePurchase, 65), 33, "65 x 0.5 = 32.5 rounds up");
}

/// HOLE 1b: the game speed's gold percent and the percents are one float product in Unciv, one
/// rounding at the end. At Quick (x 0.67) a ring-2 tile of America is 50 x 0.67 x 0.5 = 16.75 -> 17.
#[test]
fn tile_price_at_quick_is_one_rounding_at_the_end() {
    let rules = rules();
    let mut game = world(&rules);
    game.set_speed(&rules, "core:speed/quick");
    game.debug_set_player_nation(P0, nation(&rules, "core:nation/america"));
    let c = city(&mut game, &rules, 5, 5);
    let auth = game.view().auth;
    let offers = classic_rules::world::territory::tile_purchase_offers(auth, &rules, c);
    let ring2: Vec<u32> = offers.iter().filter(|(_, o)| o.ring == 2).map(|(_, o)| o.price).collect();
    assert!(!ring2.is_empty());
    let unclaimed_price = |claimed: u32| (((50.0 + 5.0 * f64::from(claimed)) * 0.67 * 0.5) + 0.5).floor() as u32;
    let claimed = classic_rules::world::territory::tiles_claimed(auth, c);
    assert_eq!(ring2[0], unclaimed_price(claimed), "claimed {claimed}, prices {ring2:?}");
}

/// Sweep: every unit and building, every subset of the four sources, every speed, against
/// Unciv's formula with the Float base. Counts the mismatches.
#[test]
fn sweep_all_subsets_and_speeds_against_faithful_unciv_float() {
    let rules = rules();
    let speeds = ["", "core:speed/quick", "core:speed/epic", "core:speed/marathon"];
    let mut bad: Vec<String> = Vec::new();
    assert!(rules.speed("core:speed/quick").is_some_and(|s| !s.values.is_standard()));
    let mut checked = 0;
    for speed in speeds {
        for mask in 0..16u32 {
            let mut game = world(&rules);
            if !speed.is_empty() {
                game.set_speed(&rules, speed);
            }
            let c = city(&mut game, &rules, 5, 5);
            let other = city(&mut game, &rules, 12, 5);
            let (mut u, mut b) = (Vec::new(), Vec::new());
            if mask & 1 != 0 {
                game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/commerce/mercantilism"));
                u.push(-25);
                b.push(-25);
            }
            if mask & 2 != 0 {
                game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/autocracy/mobilization"));
                u.push(-25);
            }
            if mask & 4 != 0 {
                game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/order/skyscrapers"));
                b.push(-25);
            }
            if mask & 8 != 0 {
                game.debug_add_building(&rules, other, building(&rules, "core:building/big_ben"));
                u.push(-15);
                b.push(-15);
            }
            let items: Vec<(ProductionItem, bool)> = (0..rules.units.len())
                .map(|i| (ProductionItem::Unit(UnitKindId(i as u16)), true))
                .chain((0..rules.buildings.len()).map(|i| (ProductionItem::Building(BuildingKindId(i as u16)), false)))
                .collect();
            for (item, is_unit) in items {
                let mut core = game.view().auth.cities().get(c).unwrap().clone();
                core.queue = vec![item];
                let auth = game.view().auth;
                let Some(price) = head_hurry_price(auth, &rules, &core) else { continue };
                let cost = city_modifiers(auth, &rules, &core).production_cost(&rules, item).unwrap();
                let modifier = match item {
                    ProductionItem::Unit(u) => rules.units[u.0 as usize].hurry_cost_modifier,
                    ProductionItem::Building(b) => rules.buildings[b.0 as usize].hurry_cost_modifier,
                    ProductionItem::Project(_) => continue,
                };
                let want = unciv_price_exact(cost, modifier, if is_unit { &u } else { &b });
                checked += 1;
                if price != want {
                    bad.push(format!("speed '{speed}' mask {mask} {item:?} cost {cost} mod {modifier}: kernel {price}, unciv {want}"));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} of {checked} differ, first: {:#?}", bad.len(), &bad[..bad.len().min(8)]);
}

/// HOLE 2 (pre-existing, not compounding): Unciv refuses to buy items and tiles in a puppet
/// (`CityConstructions.canBePurchased`: `isPuppet`; `CityExpansionManager.canBuyTile`: puppet or razing).
#[test]
fn a_puppet_buys_nothing() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, 5, 5);
    game.debug_set_city_status(c, CityStatus::Puppet);
    game.debug_earn(P0, 5000);
    let warrior = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
    game.world_mut().cities.get_mut(c).unwrap().queue = vec![ProductionItem::Unit(warrior)];
    let hurry = game.execute(&rules, Command::HurryProduction { player: P0, city: c });
    eprintln!("HURRY IN PUPPET: {:?}", hurry.as_ref().err());
    let t0 = tile(&game, 7, 5);
    let tl = game.execute(&rules, Command::PurchaseTile { player: P0, city: c, tile: t0 });
    eprintln!("TILE IN PUPPET: {:?}", tl.as_ref().err());
    assert!(hurry.is_err(), "hurry in a puppet");
    assert!(tl.is_err(), "tile in a puppet");
    let t = tile(&game, 7, 5);
    assert!(game.execute(&rules, Command::PurchaseTile { player: P0, city: c, tile: t }).is_err(), "tile in a puppet");
}

/// HOLE 3 (new): `PurchasePercents` drops sources past 8 silently, and `merge` drops the whole
/// other side when the sum passes 8. Unciv multiplies every unique.
#[test]
fn nine_sources_all_count() {
    use classic_rules::world::city::PurchasePercents;
    assert_eq!(PurchasePercents::of(&[-10; 9]).scale(10_000), 3_874, "0.9^9 x 10000");
    let mut five = PurchasePercents::of(&[-10; 5]);
    five.merge(&PurchasePercents::of(&[-10; 4]));
    assert_eq!(five.scale(10_000), 3_874, "merge of 5 and 4 sources");
}

/// HOLE 4 (new, small): a +2000 % surcharge was applied whole before, now clamped to +1000 %.
#[test]
fn a_big_surcharge_is_not_clamped() {
    use classic_rules::world::city::PurchasePercents;
    assert_eq!(PurchasePercents::of(&[2000]).scale(100), 2100);
}
