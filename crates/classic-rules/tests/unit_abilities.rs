//! Unit abilities of the classic ruleset that were inert before (catalog
//! `unit_ability.*`): the purchase rules (`PurchaseOnly`, `MoveAfterPurchase`,
//! a unit's own policy), founding abroad, the unique great people, and the
//! flags that no rule reads. Each test checks the oracle
//! (`GameState::validate`).

use classic_content::core::core_rules;
use classic_rules::derived::religion::great_person_kind;
use classic_rules::ids::{
    PlayerId, PolicyId, ProductionItem, TechId, TerrainId, TileId, UnitHandle, UnitKindId,
};
use classic_rules::math::{HexCoord, MapDimensions};
use classic_rules::rules::content::{CivilianPriority, SpecialUnit, UnitAbility};
use classic_rules::classic::effects::GreatPersonKind;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const WATER: TerrainId = TerrainId(4);
const DIMS: MapDimensions = MapDimensions { width: 24, height: 14 };

fn setup() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let game = GameState::new(&rules, GameConfig { players: 2, width: DIMS.width, height: DIMS.height, rng_seed: 5 });
    (game, rules)
}

fn tile(q: i32, r: i32) -> TileId {
    DIMS.to_tile(HexCoord { q, r }).unwrap()
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn policy(rules: &CompiledRuleset, key: &str) -> PolicyId {
    PolicyId(rules.module.policies.iter().position(|p| p.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn abilities<'r>(rules: &'r CompiledRuleset, key: &str) -> &'r [UnitAbility] {
    &rules.units[kind(rules, key).0 as usize].module.abilities
}

fn units_of(game: &GameState, player: PlayerId) -> Vec<UnitHandle> {
    game.view().auth.indexes().units_of(player).to_vec()
}

fn moves_left(game: &GameState, unit: UnitHandle) -> u16 {
    game.view().auth.units().get(unit).unwrap().core.moves_left
}

/// Buy the head of `city`'s queue: `item` is enqueued and hurried.
fn buy(game: &mut GameState, rules: &CompiledRuleset, city: classic_rules::ids::CityHandle, key: &str) -> UnitHandle {
    let before = units_of(game, P0);
    game.execute(rules, Command::EnqueueProduction { player: P0, city, item: ProductionItem::Unit(kind(rules, key)) })
        .unwrap();
    game.debug_earn(P0, 2000);
    game.execute(rules, Command::HurryProduction { player: P0, city }).unwrap();
    let after = units_of(game, P0);
    let bought: Vec<UnitHandle> = after.into_iter().filter(|u| !before.contains(u)).collect();
    assert_eq!(bought.len(), 1, "one unit is bought");
    bought[0]
}

/// A unit bought with gold cannot move on the turn it is bought,
/// unless its kind moves after its purchase (the Landsknecht, which needs
/// Mercenary Army and is only ever bought).
#[test]
fn a_bought_unit_waits_unless_it_moves_after_its_purchase() {
    let (mut game, rules) = setup();
    let city = game.debug_found_city(&rules, P0, tile(5, 5));
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/civil-service"));
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/commerce/mercenary-army"));
    let warrior = buy(&mut game, &rules, city, "core:unit/warrior");
    assert_eq!(moves_left(&game, warrior), 0, "a bought warrior waits for the next turn");
    let landsknecht = buy(&mut game, &rules, city, "core:unit/landsknecht");
    assert!(moves_left(&game, landsknecht) > 0, "a Landsknecht moves at once");
    game.validate(&rules).unwrap();
}

/// A Landsknecht is bought, never trained: production piles up in the city
/// and the unit does not appear. It needs the policy to be ordered at all.
#[test]
fn a_landsknecht_needs_its_policy_and_production_never_trains_it() {
    let (mut game, rules) = setup();
    let city = game.debug_found_city(&rules, P0, tile(5, 5));
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/civil-service"));
    let item = ProductionItem::Unit(kind(&rules, "core:unit/landsknecht"));
    let refused = game.execute(&rules, Command::EnqueueProduction { player: P0, city, item });
    assert!(format!("{refused:?}").contains("needs its policy"), "{refused:?}");
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/commerce/mercenary-army"));
    game.execute(&rules, Command::EnqueueProduction { player: P0, city, item }).unwrap();
    let cost = rules.units[kind(&rules, "core:unit/landsknecht").0 as usize].cost;
    let mut stock = 0;
    for _ in 0..60 {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
        stock = game.view().auth.cities().get(city).unwrap().production;
        if stock >= cost {
            break;
        }
    }
    assert!(stock >= cost, "the city saved {stock} of {cost}");
    assert!(units_of(&game, P0).is_empty(), "production trained nothing");
    // The purchase completes it.
    game.debug_earn(P0, 2000);
    game.execute(&rules, Command::HurryProduction { player: P0, city }).unwrap();
    assert_eq!(units_of(&game, P0).len(), 1);
    game.validate(&rules).unwrap();
}

/// The Conquistador founds cities only off the landmass of its owner's
/// capital; a Settler founds anywhere; a Knight founds nothing.
#[test]
fn a_conquistador_founds_cities_only_abroad() {
    let (mut game, rules) = setup();
    let batch: Vec<(TileId, TerrainId)> =
        (0..DIMS.height as i32).flat_map(|r| (10..13).map(move |q| (tile(q, r), WATER))).collect();
    game.debug_set_terrain_batch(&rules, &batch);
    game.debug_found_city(&rules, P0, tile(3, 5));
    let conquistador = kind(&rules, "core:unit/conquistador");
    let at_home = game.debug_spawn_unit(&rules, P0, tile(6, 8), conquistador);
    let refused = game.execute(&rules, Command::FoundCity { settler: at_home });
    assert!(format!("{refused:?}").contains("founds only abroad"), "{refused:?}");
    let abroad = game.debug_spawn_unit(&rules, P0, tile(16, 8), conquistador);
    game.execute(&rules, Command::FoundCity { settler: abroad }).unwrap();
    assert_eq!(game.view().auth.indexes().cities_of(P0).len(), 2, "the Conquistador founded a city");
    let knight = game.debug_spawn_unit(&rules, P0, tile(18, 3), kind(&rules, "core:unit/knight"));
    let refused = game.execute(&rules, Command::FoundCity { settler: knight });
    assert!(format!("{refused:?}").contains("not a settler"), "{refused:?}");
    // Four or more tiles from the capital: the minimal city distance is 3
    // (Unciv: ModConstants.kt minimalCityDistance), (6, 3) is now too close.
    let settler = game.debug_spawn_unit(&rules, P0, tile(8, 3), kind(&rules, "core:unit/settler"));
    game.execute(&rules, Command::FoundCity { settler }).unwrap();
    game.validate(&rules).unwrap();
}

/// A unique great person is the great person it
/// replaces: the Khan is a General, the Merchant of Venice a Merchant.
#[test]
fn unique_great_people_are_the_great_people_they_replace() {
    let (_, rules) = setup();
    let people: Vec<&str> = rules
        .units
        .iter()
        .filter(|u| u.module.abilities.contains(&UnitAbility::Special(SpecialUnit::People)))
        .map(|u| u.key.as_str())
        .collect();
    assert!(people.len() > 9, "the nine great people and their unique ones: {people:?}");
    for key in people {
        assert!(great_person_kind(&rules, kind(&rules, key)).is_some(), "{key} is a great person");
    }
    let khan = great_person_kind(&rules, kind(&rules, "core:unit/khan"));
    assert_eq!(khan, Some(GreatPersonKind::General));
}

/// The ability data the importer wrote: every air unit takes one slot,
/// the stealth bomber is marked, archaeologists are a low prize, great people a high one and
/// the settler a high one only early.
#[test]
fn the_imported_abilities_say_what_the_catalog_says() {
    let (_, rules) = setup();
    for key in ["core:unit/fighter", "core:unit/bomber", "core:unit/nuclear-missile"] {
        assert!(abilities(&rules, key).contains(&UnitAbility::AirSlots(1)), "{key}");
    }
    assert!(abilities(&rules, "core:unit/stealth-bomber").contains(&UnitAbility::Special(SpecialUnit::Stealth)));
    assert!(abilities(&rules, "core:unit/archaeologist")
        .contains(&UnitAbility::CivilianAttackPriority(CivilianPriority::Low)));
    assert!(abilities(&rules, "core:unit/great-general")
        .contains(&UnitAbility::CivilianAttackPriority(CivilianPriority::High)));
    assert!(abilities(&rules, "core:unit/settler")
        .contains(&UnitAbility::CivilianAttackPriority(CivilianPriority::HighEarlyGameOnly)));
    assert!(abilities(&rules, "core:unit/tank").contains(&UnitAbility::Mechanized));
    let _ = P1;
}

/// An Archaeologist is trained in a city with a University, or with the
/// unique building that replaces it (Siam's Wat); the order names the
/// building the city lacks.
#[test]
fn an_archaeologist_needs_a_university_or_its_replacement_in_the_city() {
    let (mut game, rules) = setup();
    let building = |key: &str| classic_rules::ids::BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16);
    let order = |game: &mut GameState, player: PlayerId, city| {
        let item = ProductionItem::Unit(kind(&rules, "core:unit/archaeologist"));
        game.execute(&rules, Command::EnqueueProduction { player, city, item }).map_err(|e| format!("{e:?}"))
    };
    let home = game.debug_found_city(&rules, P0, tile(5, 5));
    let siam = game.debug_found_city(&rules, P1, tile(15, 5));
    let nation = rules.nations.iter().position(|c| c.key == "core:nation/siam").unwrap();
    game.debug_set_player_nation(P1, nation as u8);
    for player in [P0, P1] {
        game.debug_learn(&rules, player, tech(&rules, "core:tech/archaeology"));
    }

    let refused = order(&mut game, P0, home).unwrap_err();
    assert!(refused.contains("needs University in the city"), "{refused}");
    game.debug_add_building(&rules, home, building("core:building/university"));
    order(&mut game, P0, home).unwrap();

    game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
    assert!(order(&mut game, P1, siam).is_err(), "no university yet");
    game.debug_add_building(&rules, siam, building("core:building/wat"));
    order(&mut game, P1, siam).unwrap();
}

/// The start-of-turn systems for the lending of movement and the loss of
/// religious strength skip a ruleset without such an effect (the guard reads
/// the units and promotions of the rules).
#[test]
fn the_guard_of_rare_unit_effects_reads_the_rules() {
    use classic_rules::classic::effects::{ClassicUnitAbility, ClassicUnitStat};
    use classic_rules::effects::unit::any_unit_effect;
    use classic_rules::rules::effects::{Modifier, UnitAbility, UnitStat};
    let lends = |e: &classic_rules::rules::effects::CompiledEffect| {
        e.modifier == Modifier::GrantAbility(UnitAbility::Module(ClassicUnitAbility::LendsMovementToGeneral))
    };
    let loses = |e: &classic_rules::rules::effects::CompiledEffect| {
        matches!(
            e.modifier,
            Modifier::ModifyUnitStat { stat: UnitStat::Module(ClassicUnitStat::RivalLandStrengthLossPercent), .. }
        )
    };
    let classic = compile_rules(core_rules()).unwrap();
    assert!(any_unit_effect(&classic, lends) && any_unit_effect(&classic, loses));
    let empty = compile_rules(classic_rules::rules::SourceRules::default()).unwrap();
    assert!(!any_unit_effect(&empty, lends) && !any_unit_effect(&empty, loses));
}
