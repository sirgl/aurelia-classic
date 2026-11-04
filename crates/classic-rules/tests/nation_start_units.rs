//! A nation's own start unit for a starting slot (`NationDefinition::start_units`), the Pathfinder's
//! sight, and the unit counterpart of the rule on buildings: a regular unit that a nation's unique
//! unit replaces is not built by that nation.

use classic_content::core::core_rules;
use classic_rules::command::{CommandError, Refusal};
use classic_rules::derived::religion::{faith_purchase_status, FaithPurchaseBlock};
use classic_rules::derived::visibility::UnitSight;
use classic_rules::effects::unit::unit_modifiers;
use classic_rules::ids::{CityHandle, PlayerId, ProductionItem, TileId, UnitHandle, UnitKindId};
use classic_rules::scenario::{generate_new_game_with, NewGameOptions, ScenarioConfig};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn nation(rules: &CompiledRuleset, key: &str) -> u8 {
    rules.nations.iter().position(|c| c.key == key).unwrap_or_else(|| panic!("{key}")) as u8
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn keys(rules: &CompiledRuleset, kinds: &[UnitKindId]) -> Vec<String> {
    kinds.iter().map(|k| rules.units[k.0 as usize].key.clone()).collect()
}

fn starts_of(rules: &CompiledRuleset, nation_key: &str) -> Vec<String> {
    keys(rules, &rules.starting_units(nation(rules, nation_key)))
}

#[test]
fn the_shoshone_start_with_a_settler_and_a_pathfinder() {
    let rules = rules();
    assert_eq!(starts_of(&rules, "core:nation/shoshone"), ["core:unit/settler", "core:unit/pathfinder"]);
}

#[test]
fn other_nations_start_as_before() {
    let rules = rules();
    assert_eq!(starts_of(&rules, "core:nation/aztecs"), ["core:unit/settler", "core:unit/jaguar"]);
    assert_eq!(starts_of(&rules, "core:nation/polynesia"), ["core:unit/settler", "core:unit/maori-warrior"]);
    assert_eq!(starts_of(&rules, "core:nation/america"), ["core:unit/settler", "core:unit/warrior"]);
    // Only the Shoshone name a start unit.
    let named: Vec<&str> = rules.nations.iter().filter(|n| !n.start_units.is_empty()).map(|n| n.key.as_str()).collect();
    assert_eq!(named, ["core:nation/shoshone"]);
}

/// The units a human of `nation_key` finds on the map when the game starts.
fn new_game_units(rules: &CompiledRuleset, nation_key: &str) -> (GameState, Vec<UnitHandle>) {
    let cfg = ScenarioConfig { seed: 0x5EED_0B5E, players: 4, width: 48, height: 40, cities: 0, units: 0, water_ratio: 20 };
    let game = generate_new_game_with(rules, &cfg, Some(nation(rules, nation_key)), NewGameOptions::default());
    assert_eq!(game.view().auth.players().nation_of(P0), nation(rules, nation_key));
    let units = game.view().auth.indexes().units_of(P0).to_vec();
    (game, units)
}

#[test]
fn a_new_game_of_the_shoshone_has_a_settler_and_a_pathfinder_and_no_warrior() {
    let rules = rules();
    let (game, units) = new_game_units(&rules, "core:nation/shoshone");
    let kinds: Vec<UnitKindId> = units.iter().map(|h| game.view().auth.units().get(*h).unwrap().core.kind).collect();
    assert_eq!(keys(&rules, &kinds), ["core:unit/settler", "core:unit/pathfinder"]);
    game.validate(&rules).unwrap();
}

#[test]
fn a_new_game_of_the_aztecs_has_a_settler_and_a_jaguar() {
    let rules = rules();
    let (game, units) = new_game_units(&rules, "core:nation/aztecs");
    let kinds: Vec<UnitKindId> = units.iter().map(|h| game.view().auth.units().get(*h).unwrap().core.kind).collect();
    assert_eq!(keys(&rules, &kinds), ["core:unit/settler", "core:unit/jaguar"]);
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

/// The sight radius of a unit of `kind` standing on `at` (its promotions, free ones included).
fn sight(game: &mut GameState, rules: &CompiledRuleset, at: TileId, kind: UnitKindId) -> i32 {
    let handle = game.debug_spawn_unit(rules, P0, at, kind);
    let core = *game.view().auth.units().get(handle).unwrap().core;
    let mods = unit_modifiers(game.view().auth, rules, &core);
    UnitSight::of(rules, &mods, kind).at(game.view().auth.map(), at)
}

/// The Pathfinder sees 3 where a Scout sees 2: Extra Sight (1) is one of its free promotions; its
/// `sight` field stays 2.
#[test]
fn the_pathfinder_sees_three_through_its_promotions() {
    let rules = rules();
    let pathfinder = unit(&rules, "core:unit/pathfinder");
    let scout = unit(&rules, "core:unit/scout");
    assert_eq!(rules.units[pathfinder.0 as usize].sight, 2, "the field is the Scout's");
    assert_eq!(rules.units[scout.0 as usize].sight, 2);
    let extra = rules.promotions.iter().position(|p| p.key == "core:promotion/extra-sight-i").unwrap();
    assert!(rules.units[pathfinder.0 as usize].free_promotions.iter().any(|p| p.0 as usize == extra));
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 24, height: 16, rng_seed: 5 });
    // A tile that is neither hills nor water: the plain radius of a Scout.
    let tiles: Vec<TileId> = (0..16).flat_map(|r| (0..24).map(move |q| (q, r))).map(|(q, r)| tile(&game, q, r)).collect();
    let at = tiles.into_iter().find(|t| sight(&mut game, &rules, *t, scout) == 2).expect("a plain tile");
    assert_eq!(sight(&mut game, &rules, at, pathfinder), 3);
    game.validate(&rules).unwrap();
}

fn enqueue(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle, key: &str) -> Result<(), CommandError> {
    let item = ProductionItem::Unit(unit(rules, key));
    game.execute(rules, Command::EnqueueProduction { player: P0, city, item }).map(|_| ())
}

fn city_of(nation_key: &str) -> (GameState, CompiledRuleset, CityHandle) {
    let rules = rules();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 24, height: 16, rng_seed: 5 });
    game.debug_set_player_nation(P0, nation(&rules, nation_key));
    let at = tile(&game, 5, 5);
    let city = game.debug_found_city(&rules, P0, at);
    (game, rules, city)
}

/// A unit that a nation's unique unit replaces is not on its order list: the Aztecs build the Jaguar
/// and no Warrior; the nation without a Warrior replacement does.
#[test]
fn a_replaced_unit_cannot_be_queued_by_the_nation_that_replaces_it() {
    let (mut game, rules, city) = city_of("core:nation/aztecs");
    let refused = enqueue(&mut game, &rules, city, "core:unit/warrior");
    assert_eq!(refused, Err(CommandError::Refused(Refusal::ReplacedByUniqueUnit)));
    enqueue(&mut game, &rules, city, "core:unit/jaguar").unwrap();
    // The Aztecs' other regular units are not replaced.
    enqueue(&mut game, &rules, city, "core:unit/worker").unwrap();
    assert_eq!(game.view().auth.cities().get(city).unwrap().queue.len(), 2);
    game.validate(&rules).unwrap();
}

#[test]
fn the_shoshone_build_the_pathfinder_in_place_of_the_scout() {
    let (mut game, rules, city) = city_of("core:nation/shoshone");
    assert_eq!(enqueue(&mut game, &rules, city, "core:unit/scout"), Err(CommandError::Refused(Refusal::ReplacedByUniqueUnit)));
    enqueue(&mut game, &rules, city, "core:unit/pathfinder").unwrap();
    // Their Warrior is not replaced.
    enqueue(&mut game, &rules, city, "core:unit/warrior").unwrap();
}

#[test]
fn other_nations_still_build_the_replaced_unit() {
    for key in ["core:nation/america", "core:nation/egypt"] {
        let (mut game, rules, city) = city_of(key);
        enqueue(&mut game, &rules, city, "core:unit/warrior").unwrap();
        enqueue(&mut game, &rules, city, "core:unit/scout").unwrap();
        game.validate(&rules).unwrap();
    }
}

/// Another nation's unique unit keeps its own refusal.
#[test]
fn another_nations_unique_unit_is_still_refused_as_such() {
    let (mut game, rules, city) = city_of("core:nation/america");
    assert_eq!(enqueue(&mut game, &rules, city, "core:unit/jaguar"), Err(CommandError::Refused(Refusal::OtherNation)));
}

/// The faith price of a replaced unit is refused like the building's.
#[test]
fn a_replaced_unit_is_not_for_sale_for_faith_either() {
    let (mut game, rules, city) = city_of("core:nation/aztecs");
    game.debug_grant_faith(P0, 5000);
    let item = ProductionItem::Unit(unit(&rules, "core:unit/warrior"));
    assert_eq!(faith_purchase_status(game.view().auth, &rules, P0, city, item), Err(FaithPurchaseBlock::OtherNation));
    let (mut game, rules, city) = city_of("core:nation/america");
    game.debug_grant_faith(P0, 5000);
    let item = ProductionItem::Unit(unit(&rules, "core:unit/warrior"));
    assert_ne!(faith_purchase_status(game.view().auth, &rules, P0, city, item), Err(FaithPurchaseBlock::OtherNation));
}
