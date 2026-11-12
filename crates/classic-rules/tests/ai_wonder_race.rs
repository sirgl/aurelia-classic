//! The wonder race (`ai::wonder_race`, variants `next-w` and `next-race`): a world wonder
//! another nation has built is dropped from the queue with the city's stock kept, and its lost
//! head is not hurry-bought. The kernel also removes a lost order by itself at the drain
//! (`OrderRemoved`), so a stale order costs nothing whichever AI plays.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::ai::wonder_race::{hurry_head, lost};
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, TechId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// P0 (the AI) knows every technology and has two cities, one with the world wonder first in
/// the queue of the first (a wonder may be queued in one city only); P1 (human) has one city far away that completes the same wonder.
/// Returns the game, the rules, P0's cities, P1's city and the wonder.
fn lost_race() -> (GameState, CompiledRuleset, CityHandle, CityHandle, CityHandle, BuildingKindId) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 });
    let a = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let b = game.debug_found_city(&rules, P0, tile(&game, 12, 5));
    let rival = game.debug_found_city(&rules, P1, tile(&game, 25, 15));
    for t in 0..rules.technologies.len() {
        game.debug_learn(&rules, P0, TechId(t as u16));
    }
    let obs = game.ai_observation(&rules, P0);
    let wonder = (0..rules.buildings.len())
        .map(|i| BuildingKindId(i as u16))
        .find(|k| {
            rules.buildings[k.0 as usize].wonder
                && obs.view.accepts(&Command::EnqueueProduction { player: P0, city: a, item: ProductionItem::Building(*k) })
        })
        .expect("the rules have a world wonder both cities may order");
    drop(obs);
    game.execute(&rules, Command::EnqueueProduction { player: P0, city: a, item: ProductionItem::Building(wonder) }).unwrap();
    // The wonder stands in the rival's city without the sweep a completion makes (the harness grant: the
    // state of a save made before the kernel removed lost orders), so the order is still queued here.
    game.debug_add_building(&rules, rival, wonder);
    (game, rules, a, b, rival, wonder)
}

fn queue(game: &GameState, city: CityHandle) -> Vec<ProductionItem> {
    game.view().auth.cities().get(city).unwrap().queue.iter().copied().collect()
}

/// A wonder is lost when another city holds it; a plain building and a wonder nobody built are not.
#[test]
fn a_wonder_another_nation_built_is_lost_to_the_seat() {
    let (game, rules, a, _, _, wonder) = lost_race();
    let obs = game.ai_observation(&rules, P0);
    let city = obs.own_cities.iter().find(|c| c.handle == a).unwrap();
    assert!(lost(&obs, &rules, city, ProductionItem::Building(wonder)));
    let others = (0..rules.buildings.len()).map(|i| BuildingKindId(i as u16)).filter(|k| *k != wonder);
    for kind in others {
        let built = obs.built_wonders.contains(&kind);
        assert_eq!(lost(&obs, &rules, city, ProductionItem::Building(kind)), built && rules.buildings[kind.0 as usize].wonder, "{kind:?}");
    }
}

/// The hurry offers skip a lost head when the switch is on and keep every other head; with the
/// switch off nothing changes.
#[test]
fn a_lost_head_is_not_hurried_with_gold() {
    let (game, rules, a, _, _, wonder) = lost_race();
    let obs = game.ai_observation(&rules, P0);
    let city = obs.own_cities.iter().find(|c| c.handle == a).unwrap();
    let head = Some(ProductionItem::Building(wonder));
    assert_eq!(city.queue.first().copied(), head);
    assert_eq!(hurry_head(0, &obs, &rules, city), head, "switch off: the plain AI's offer");
    assert_eq!(hurry_head(1, &obs, &rules, city), None);
    assert_eq!(hurry_head(2, &obs, &rules, city), None);
    let mut plain = city.clone();
    plain.queue.clear();
    assert_eq!(hurry_head(1, &obs, &rules, &plain), None, "an empty queue has no head");
}

/// The kernel keeps the stock when a queue is cleared, so dropping a lost order early costs
/// nothing (the claim the dropping rests on).
#[test]
fn clearing_a_queue_keeps_the_stock() {
    let (mut game, rules, a, _, _, _) = lost_race();
    for _ in 0..4 {
        for p in [P0, P1] {
            game.execute(&rules, Command::EndTurn { player: p }).unwrap();
        }
    }
    let stock = |game: &GameState| game.ai_observation(&rules, P0).own_cities.iter().find(|c| c.handle == a).unwrap().production;
    let before = stock(&game);
    assert!(before > 0, "the city has gathered a stock");
    game.execute(&rules, Command::ClearProduction { player: P0, city: a }).unwrap();
    assert_eq!(stock(&game), before);
}

/// A lost order is gone from the queue whichever AI plays the seat (`pre-next`, the standard before
/// it, `standard`, `next-w`, `next-race`): the plain AI leaves it and the rules remove it at the
/// owner's drain, and the record names
/// the city, the owner, the wonder and `WonderBuilt`; nothing is spent (the stock is not lower and
/// no gold or hurry goes to the lost head); the AI sends no order or hurry for it.
#[cfg(feature = "experimental-ai")]
#[test]
fn the_rules_remove_a_lost_order_for_every_ai_and_nothing_is_spent() {
    use kernel::ai::Advance;
    use classic_rules::transaction::journal::{CityChange, OrderRemoval};
    // The fixture is the stale state of an old save, in which the wonder-race AIs see the lost head and
    // drop it themselves before the drain; the plain AI does not, so the rules remove it and say so.
    for (variant, ai_drops_it) in [(Variant::PreNext, false), (Variant::Standard, true), (Variant::NextW, true), (Variant::NextRace, true)] {
        let (mut game, rules, a, b, _, wonder) = lost_race();
        let stock_before = game.ai_observation(&rules, P0).own_cities.iter().find(|c| c.handle == a).unwrap().production;
        let item = ProductionItem::Building(wonder);
        let mut driver = AiDriver::with_mind(AiSettings::for_rules(&rules, rules.default_difficulty()), vec![P1], ClassicMind::new(variant));
        driver.configure_game(&mut game);
        let mut removals = Vec::new();
        let mut sent_for_it = 0;
        loop {
            match driver.advance(&game, &rules) {
                Advance::Worked => {}
                Advance::Command(command) | Advance::EndTurn(command) => {
                    if matches!(&command, Command::HurryProduction { city, .. } if city == &a)
                        || matches!(&command, Command::EnqueueProduction { item: queued, .. } if *queued == item)
                    {
                        sent_for_it += 1;
                    }
                    let result = game.execute(&rules, command);
                    if let Ok(done) = &result {
                        for change in &done.changes().city_changes {
                            if let CityChange::OrderRemoved { city, owner, item, reason } = *change {
                                removals.push((city, owner, item, reason));
                            }
                        }
                    }
                    driver.executed(result);
                }
                Advance::RoundDone | Advance::Waiting => break,
            }
        }
        let record = vec![(a, P0, item, OrderRemoval::WonderBuilt)];
        assert_eq!(removals, if ai_drops_it { vec![] } else { record }, "{variant:?}: the rules' record exactly when the AI left the order");
        assert!(!queue(&game, a).contains(&item), "{variant:?}: the order is gone");
        assert!(!queue(&game, b).contains(&item));
        let stock_after = game.ai_observation(&rules, P0).own_cities.iter().find(|c| c.handle == a).unwrap().production;
        assert!(stock_after >= stock_before, "{variant:?}: nothing was spent ({stock_before} -> {stock_after})");
        assert_eq!(sent_for_it, 0, "{variant:?}: the AI buys nothing and orders no lost wonder");
    }
}

/// The game's AI drops a lost wonder in every build (the thirteenth promotion: `standard` plays
/// `next-w`; the wonder rule is not behind `experimental-ai`).
#[test]
fn the_games_ai_drops_a_lost_order() {
    let (mut game, rules, a, _, _, wonder) = lost_race();
    let mut driver = AiDriver::with_mind(AiSettings::for_rules(&rules, rules.default_difficulty()), vec![P1], ClassicMind::new(Variant::Standard));
    driver.run(&mut game, &rules, u32::MAX);
    assert!(!queue(&game, a).contains(&ProductionItem::Building(wonder)));
}

/// The chance table of a world wonder (`ai::wonder_chance`, fitted to the bench) does not rise
/// with the turns the order takes, with the seat's place falling (1, 2, 3 or worse) or with the
/// largest sighted rival city growing; every cell is a percent; the bin edges ascend.
#[test]
fn the_wonder_chance_table_never_rises_against_the_seat() {
    use classic_rules::ai::wonder_chance::{CHANCE, POPULATION, TURNS};
    assert!(TURNS.windows(2).all(|w| w[0] < w[1]) && POPULATION.windows(2).all(|w| w[0] < w[1]));
    for (row, places) in CHANCE.iter().enumerate() {
        for (place, sizes) in places.iter().enumerate() {
            for (size, cell) in sizes.iter().enumerate() {
                assert!(*cell <= 100, "{cell}");
                if size > 0 {
                    assert!(*cell <= sizes[size - 1], "rival size: row {row} place {place} size {size}");
                }
                if place > 0 {
                    assert!(*cell <= places[place - 1][size], "place: row {row} place {place} size {size}");
                }
                if row > 0 {
                    assert!(*cell <= CHANCE[row - 1][place][size], "turns: row {row} place {place} size {size}");
                }
            }
        }
    }
}
