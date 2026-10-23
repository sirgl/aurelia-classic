//! Civilization traits and policies that change the owner's units (catalog
//! `civ.*` unit traits): gold maintenance, experience needed, movement in a
//! golden age, combat against city-states and larger empires, and the
//! promotions a trait gives to a combat role. Each test checks the oracle
//! (`GameState::validate`).

use classic_content::core::core_rules;
use classic_rules::combat::exchange::AttackKind;
use classic_rules::combat::experience::experience_needed_by;
use classic_rules::combat::plan_attack;
use classic_rules::derived::maintenance::unit_maintenance;
use classic_rules::effects::unit::unit_modifiers;
use classic_rules::ids::{PlayerId, PolicyId, PromotionId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::rules::content::CityStateId;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};

const MOVE_DENOMINATOR: u16 = classic_rules::world::unit::MOVE_DENOMINATOR;
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const WATER: TerrainId = TerrainId(4);

fn setup() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let game = GameState::new(&rules, GameConfig { players: 2, width: 24, height: 16, rng_seed: 5 });
    (game, rules)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn civ(rules: &CompiledRuleset, key: &str) -> u8 {
    rules.civs.iter().position(|c| c.key == key).unwrap_or_else(|| panic!("{key}")) as u8
}

fn promotion(rules: &CompiledRuleset, key: &str) -> PromotionId {
    PromotionId(rules.promotions.iter().position(|p| p.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn core_of(game: &GameState, unit: UnitHandle) -> classic_rules::world::unit::UnitCore {
    *game.view().auth.units().get(unit).unwrap().core
}

/// The gold a player of `civ_key` pays for `count` units of `kind_key`
/// (one city, the same turn, the same handicap).
fn upkeep_of(civ_key: &str, kind_key: &str, count: usize) -> i32 {
    let (mut game, rules) = setup();
    let sea: Vec<(TileId, TerrainId)> = (0..16).map(|r| (tile(&game, 12, r), WATER)).collect();
    game.debug_set_terrain_batch(&rules, &sea);
    game.debug_set_player_civ(P0, civ(&rules, civ_key));
    game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let at = if kind_key.ends_with("trireme") { tile(&game, 12, 5) } else { tile(&game, 6, 6) };
    for _ in 0..count {
        game.debug_spawn_unit(&rules, P0, at, unit(&rules, kind_key));
    }
    game.validate(&rules).unwrap();
    unit_maintenance(game.view().auth, &rules, P0)
}

/// Germany pays a quarter less for land units, the Ottomans two thirds less
/// for ships, the Zulu half for melee; each only for its own kind of unit.
#[test]
fn maintenance_traits_discount_their_own_units_only() {
    let plain = upkeep_of("core:nation/america", "core:unit/warrior", 30);
    assert!(plain > 0);
    assert!(upkeep_of("core:nation/germany", "core:unit/warrior", 30) < plain, "Germany: land units");
    assert!(upkeep_of("core:nation/zulu", "core:unit/warrior", 30) < upkeep_of("core:nation/germany", "core:unit/warrior", 30), "Zulu: melee, half");
    assert_eq!(upkeep_of("core:nation/ottomans", "core:unit/warrior", 30), plain, "Ottomans: ships only");
    let ships = upkeep_of("core:nation/america", "core:unit/trireme", 30);
    assert!(upkeep_of("core:nation/ottomans", "core:unit/trireme", 30) < ships, "Ottomans: ships");
    assert_eq!(upkeep_of("core:nation/germany", "core:unit/trireme", 30), ships, "Germany: land units only");
}

/// Nationalism: a third less for every unit.
#[test]
fn nationalism_cuts_the_maintenance_of_every_unit() {
    let (mut game, rules) = setup();
    game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    for _ in 0..30 {
        game.debug_spawn_unit(&rules, P0, tile(&game, 6, 6), unit(&rules, "core:unit/warrior"));
    }
    let before = unit_maintenance(game.view().auth, &rules, P0);
    let nationalism = PolicyId(
        rules.module.policies.iter().position(|p| p.key == "core:policy/autocracy/nationalism").unwrap() as u16,
    );
    game.debug_adopt_policy(&rules, P0, nationalism);
    let after = unit_maintenance(game.view().auth, &rules, P0);
    assert!(after < before, "{after} < {before}");
    game.validate(&rules).unwrap();
}

/// The Zulu need a quarter less experience for a promotion.
#[test]
fn the_zulu_need_less_experience_for_a_promotion() {
    let (mut game, rules) = setup();
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), unit(&rules, "core:unit/warrior"));
    let plain = experience_needed_by(game.view().auth, &rules, &core_of(&game, warrior));
    assert_eq!(plain, 10);
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/zulu"));
    game.debug_sources_changed(&rules, P0);
    let zulu = experience_needed_by(game.view().auth, &rules, &core_of(&game, warrior));
    assert_eq!(zulu, 7, "10 - 25 %, rounded down");
    game.validate(&rules).unwrap();
}

/// A golden age gives the Persians' units a move and 10 % strength (the
/// condition is the owner's, not the tile's).
#[test]
fn the_persians_move_further_in_a_golden_age() {
    let (mut game, rules) = setup();
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/persia"));
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), unit(&rules, "core:unit/warrior"));
    let kind = unit(&rules, "core:unit/warrior");
    let moves = |game: &GameState| unit_modifiers(game.view().auth, &rules, &core_of(game, warrior)).movement(&rules, kind);
    let before = moves(&game);
    game.debug_start_golden_age(&rules, P0, 10);
    assert_eq!(moves(&game), before + MOVE_DENOMINATOR, "one whole move");
    game.validate(&rules).unwrap();
}

/// The Mongols' mounted units move one further; their foot soldiers do not.
#[test]
fn the_mongols_horsemen_move_one_further() {
    let (mut game, rules) = setup();
    let mounted = unit(&rules, "core:unit/horseman");
    let foot = unit(&rules, "core:unit/warrior");
    let horse = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), mounted);
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 6, 5), foot);
    let moves = |game: &GameState, u: UnitHandle, kind: UnitKindId| {
        unit_modifiers(game.view().auth, &rules, &core_of(game, u)).movement(&rules, kind)
    };
    let (horse_before, warrior_before) = (moves(&game, horse, mounted), moves(&game, warrior, foot));
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/mongolia"));
    game.debug_sources_changed(&rules, P0);
    assert_eq!(moves(&game, horse, mounted), horse_before + MOVE_DENOMINATOR);
    assert_eq!(moves(&game, warrior, foot), warrior_before);
    game.validate(&rules).unwrap();
}

/// Strength of P0's warrior attacking P1's warrior next to it.
fn attack_strength(game: &GameState, rules: &CompiledRuleset, at: (i32, i32), target: (i32, i32)) -> i64 {
    let warrior = unit(rules, "core:unit/warrior");
    let attacker = game.view().auth.indexes().units_on(tile(game, at.0, at.1)).first().copied();
    let attacker = attacker.unwrap_or_else(|| panic!("a warrior at {at:?} ({warrior:?})"));
    plan_attack(game.view(), rules, attacker, tile(game, target.0, target.1), AttackKind::Melee)
        .unwrap()
        .attacker_strength
}

/// The Mongols fight city-states with +30 %.
#[test]
fn the_mongols_strike_city_states_harder() {
    let (mut game, rules) = setup();
    let warrior = unit(&rules, "core:unit/warrior");
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/mongolia"));
    game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), warrior);
    game.debug_spawn_unit(&rules, P1, tile(&game, 6, 5), warrior);
    game.debug_meet(P0, P1);
    game.debug_force_war(&rules, P0, P1);
    let major = attack_strength(&game, &rules, (5, 5), (6, 5));
    game.debug_set_player_kind(P1, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    let minor = attack_strength(&game, &rules, (5, 5), (6, 5));
    assert_eq!(minor - major, 8 * 30, "+30 % of a warrior's 8");
    game.validate(&rules).unwrap();
}

/// Ethiopia fights an empire with more cities with +20 %.
#[test]
fn ethiopia_strikes_larger_empires_harder() {
    let (mut game, rules) = setup();
    let warrior = unit(&rules, "core:unit/warrior");
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/ethiopia"));
    game.debug_found_city(&rules, P0, tile(&game, 2, 2));
    game.debug_found_city(&rules, P1, tile(&game, 2, 12));
    game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), warrior);
    game.debug_spawn_unit(&rules, P1, tile(&game, 6, 5), warrior);
    game.debug_meet(P0, P1);
    game.debug_force_war(&rules, P0, P1);
    let equal = attack_strength(&game, &rules, (5, 5), (6, 5));
    game.debug_found_city(&rules, P1, tile(&game, 10, 12));
    let larger = attack_strength(&game, &rules, (5, 5), (6, 5));
    assert_eq!(larger - equal, 8 * 20, "+20 % of a warrior's 8");
    game.validate(&rules).unwrap();
}

/// A trait that gives a promotion to a combat role (America's units get
/// the Sentry promotion): melee units get it, a settler does not.
#[test]
fn a_trait_gives_a_promotion_to_its_combat_role() {
    let (mut game, rules) = setup();
    game.debug_set_player_civ(P0, civ(&rules, "core:nation/america"));
    let sentry = promotion(&rules, "core:promotion/sentry");
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), unit(&rules, "core:unit/warrior"));
    let settler = game.debug_spawn_unit(&rules, P0, tile(&game, 6, 5), unit(&rules, "core:unit/settler"));
    let granted = |u: UnitHandle| unit_modifiers(game.view().auth, &rules, &core_of(&game, u)).granted.contains(&sentry);
    assert!(granted(warrior), "a warrior is melee");
    assert!(!granted(settler), "a settler has no combat role");
}

// ---- policies, buildings and promotions that change units ----

use classic_rules::combat::exchange::AttackKind as Kind;
use classic_rules::ids::{BuildingKindId, ProductionItem};
use classic_rules::rules::effects::UnitStat;
use classic_rules::world::tourism::GreatWorkClass;

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

/// Volunteer Army: six units pay no maintenance (its Foreign Legion units
/// come free).
#[test]
fn volunteer_army_makes_five_units_free() {
    let (mut game, rules) = setup();
    game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    for _ in 0..30 {
        game.debug_spawn_unit(&rules, P0, tile(&game, 6, 6), unit(&rules, "core:unit/warrior"));
    }
    let before = unit_maintenance(game.view().auth, &rules, P0);
    let army = PolicyId(
        rules.module.policies.iter().position(|p| p.key == "core:policy/freedom/volunteer-army").unwrap() as u16,
    );
    game.debug_adopt_policy(&rules, P0, army);
    // Adoption brings four Foreign Legion units; the five free slots pay for
    // them, so the bill stays what it was.
    let units = game.view().auth.indexes().units_of(P0).len();
    assert_eq!(units, 34);
    assert_eq!(unit_maintenance(game.view().auth, &rules, P0), before);
    for _ in 0..4 {
        game.debug_spawn_unit(&rules, P0, tile(&game, 6, 6), unit(&rules, "core:unit/warrior"));
    }
    assert!(unit_maintenance(game.view().auth, &rules, P0) > before, "the seventh and the rest pay");
}

/// China's Great Generals give 15 % more: the aura of a general next to the
/// unit rises from 15 % to 30 % of the warrior's 8.
#[test]
fn chinas_great_general_aura_is_stronger() {
    let strike = |civ_key: &str| {
        let (mut game, rules) = setup();
        game.debug_set_player_civ(P0, civ(&rules, civ_key));
        let warrior = unit(&rules, "core:unit/warrior");
        let attacker = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), warrior);
        game.debug_spawn_unit(&rules, P0, tile(&game, 4, 5), unit(&rules, "core:unit/great-general"));
        game.debug_spawn_unit(&rules, P1, tile(&game, 6, 5), warrior);
        game.debug_meet(P0, P1);
        game.debug_force_war(&rules, P0, P1);
        plan_attack(game.view(), &rules, attacker, tile(&game, 6, 5), Kind::Melee).unwrap().attacker_strength
    };
    assert_eq!(strike("core:nation/china") - strike("core:nation/america"), 8 * 15);
}

/// A kill with Golden Age Points (the Minuteman) fills the golden age meter
/// with the victim's strength; another unit's kill does not.
#[test]
fn golden_age_points_come_from_kills() {
    let meter = |kind: &str| {
        let (mut game, rules) = setup();
        let attacker = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), unit(&rules, kind));
        let victim = game.debug_spawn_unit(&rules, P1, tile(&game, 6, 5), unit(&rules, "core:unit/warrior"));
        game.debug_set_unit_hp(victim, 1);
        game.debug_meet(P0, P1);
        game.debug_force_war(&rules, P0, P1);
        game.execute(&rules, Command::AttackUnit { attacker, defender: victim }).unwrap();
        assert!(game.view().auth.units().get(victim).is_none(), "the warrior died");
        game.validate(&rules).unwrap();
        game.view().auth.players().economy_of(P0).unwrap().golden_age_meter
    };
    assert_eq!(meter("core:unit/warrior"), 0);
    assert_eq!(meter("core:unit/minuteman"), 8, "the strength of a warrior");
}

/// Mystic Blade: after its first fight the Kris Swordsman loses the blade
/// and takes one of its random promotions.
#[test]
fn the_mystic_blade_is_replaced_after_the_first_fight() {
    let (mut game, rules) = setup();
    let blade = PromotionId(rules.promotions.iter().position(|p| p.key == "core:promotion/mystic-blade").unwrap() as u16);
    let attacker = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), unit(&rules, "core:unit/kris-swordsman"));
    let victim = game.debug_spawn_unit(&rules, P1, tile(&game, 6, 5), unit(&rules, "core:unit/warrior"));
    game.debug_set_unit_hp(victim, 1);
    game.debug_meet(P0, P1);
    game.debug_force_war(&rules, P0, P1);
    assert!(core_of(&game, attacker).promotions.contains(blade));
    game.execute(&rules, Command::AttackUnit { attacker, defender: victim }).unwrap();
    let after = core_of(&game, attacker);
    assert!(!after.promotions.contains(blade), "the blade is gone");
    assert!(after.promotions.iter().next().is_some(), "a random promotion took its place");
    game.validate(&rules).unwrap();
}

/// Air Recon: a fighter sees six tiles (its own two and four more).
#[test]
fn air_recon_widens_the_sight_of_aircraft() {
    let (mut game, rules) = setup();
    let fighter = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), unit(&rules, "core:unit/fighter"));
    let mods = unit_modifiers(game.view().auth, &rules, &core_of(&game, fighter));
    assert_eq!(mods.stat(UnitStat::Sight), 4);
}

/// The Royal Library gives units trained in its city experience for each
/// Great Work there: a land unit trained with one work starts with 10.
#[test]
fn the_royal_library_trains_units_with_experience() {
    let trained = |work: bool| {
        let (mut game, rules) = setup();
        game.debug_set_player_civ(P0, civ(&rules, "core:nation/assyria"));
        let city = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
        game.debug_add_building(&rules, city, building(&rules, "core:building/royal_library"));
        if work {
            let at = game.view().auth.cities().get(city).unwrap().tile;
            let writer = game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/great-writer"));
            game.execute(&rules, Command::CreateGreatWork { unit: writer }).unwrap();
            let works = &game.view().auth.module().tourism().city(city).works;
            assert_eq!(works.len(), 1);
            let class = game.view().auth.module().tourism().work(works[0].work).unwrap().class;
            assert_eq!(class, GreatWorkClass::Literature);
        }
        let before = game.view().auth.indexes().units_of(P0).to_vec();
        game.execute(
            &rules,
            Command::EnqueueProduction { player: P0, city, item: ProductionItem::Unit(unit(&rules, "core:unit/warrior")) },
        )
        .unwrap();
        game.debug_earn(P0, 2000);
        game.execute(&rules, Command::HurryProduction { player: P0, city }).unwrap();
        let after = game.view().auth.indexes().units_of(P0).to_vec();
        let new: Vec<UnitHandle> = after.into_iter().filter(|u| !before.contains(u)).collect();
        assert_eq!(new.len(), 1);
        game.validate(&rules).unwrap();
        core_of(&game, new[0]).experience
    };
    assert_eq!(trained(false), 0);
    assert_eq!(trained(true), 10);
}

/// A promotion that depends on the feature of the battle tile (the Jaguar's
/// +33 % in forest and jungle) works: a Unit-scope `TileFeature` condition
/// is checked on the tile of the fight (the target's, for an attacker), not
/// on the owner.
#[test]
fn a_jaguar_fights_harder_in_the_forest() {
    let (mut game, rules) = setup();
    let jaguar = unit(&rules, "core:unit/jaguar");
    game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), jaguar);
    game.debug_spawn_unit(&rules, P1, tile(&game, 6, 5), unit(&rules, "core:unit/warrior"));
    game.debug_meet(P0, P1);
    game.debug_force_war(&rules, P0, P1);
    let open = attack_strength(&game, &rules, (5, 5), (6, 5));
    let forest = classic_rules::ids::FeatureId(rules.features.iter().position(|f| f.key == "core:feature/forest").unwrap() as u16);
    game.debug_set_feature_batch(&rules, &[(tile(&game, 6, 5), forest)]);
    let wooded = attack_strength(&game, &rules, (5, 5), (6, 5));
    assert!(wooded > open, "{wooded} > {open}");
    assert_eq!(wooded - open, open * 33 / 100, "+33 % in the forest");
    game.validate(&rules).unwrap();
}

/// Japan (Bushido): a wounded unit hits as hard as a whole one; everyone
/// else loses a third of the damage the wounds take away. At full health the
/// two are equal.
#[test]
fn japan_fights_as_well_wounded_as_whole() {
    let blow = |civ_key: &str, hp: u16| {
        let (mut game, rules) = setup();
        game.debug_set_player_civ(P0, civ(&rules, civ_key));
        game.debug_sources_changed(&rules, P0);
        let warrior = unit(&rules, "core:unit/warrior");
        let attacker = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), warrior);
        game.debug_spawn_unit(&rules, P1, tile(&game, 6, 5), warrior);
        game.debug_meet(P0, P1);
        game.debug_force_war(&rules, P0, P1);
        game.debug_set_unit_hp(attacker, hp);
        let plan = plan_attack(game.view(), &rules, attacker, tile(&game, 6, 5), AttackKind::Melee).unwrap();
        plan.damage_to_defender(classic_rules::combat::damage::Roll::Expected)
    };
    assert_eq!(blow("core:nation/japan", 100), blow("core:nation/america", 100), "whole units are equal");
    assert!(blow("core:nation/japan", 30) > blow("core:nation/america", 30), "a wounded Japanese hits harder");
    assert_eq!(blow("core:nation/japan", 30), blow("core:nation/japan", 100), "as hard as a whole one");
}

/// Polynesia (Wayfinding): +10 % strength within two tiles of an own Moai
/// (a warrior's 8, so 80 hundredths); farther away, with another
/// civilization's Moai, or with no Moai there is none.
#[test]
fn polynesia_fights_harder_near_its_moai() {
    let strike = |civ_key: &str, moai_at: Option<(i32, i32)>, from: (i32, i32)| {
        let (mut game, rules) = setup();
        game.debug_set_player_civ(P0, civ(&rules, civ_key));
        game.debug_sources_changed(&rules, P0);
        game.debug_found_city(&rules, P0, tile(&game, 5, 5));
        let warrior = unit(&rules, "core:unit/warrior");
        game.debug_spawn_unit(&rules, P0, tile(&game, from.0, from.1), warrior);
        game.debug_spawn_unit(&rules, P1, tile(&game, from.0 + 1, from.1), warrior);
        if let Some(at) = moai_at {
            let moai = rules.improvements.iter().position(|i| i.key == "core:improvement/moai").unwrap();
            game.debug_set_improvement_batch(&rules, &[(tile(&game, at.0, at.1), classic_rules::ids::ImprovementId(moai as u16))]);
        }
        game.debug_meet(P0, P1);
        game.debug_force_war(&rules, P0, P1);
        game.validate(&rules).unwrap();
        attack_strength(&game, &rules, from, (from.0 + 1, from.1))
    };
    let polynesia = "core:nation/polynesia";
    let bare = strike(polynesia, None, (7, 5));
    assert_eq!(strike(polynesia, Some((6, 5)), (7, 5)) - bare, 8 * 10, "one tile from the Moai");
    assert_eq!(strike(polynesia, Some((5, 5 - 0)), (7, 5)) - bare, 8 * 10, "two tiles from the Moai");
    assert_eq!(strike(polynesia, Some((4, 5)), (7, 5)), bare, "three tiles from the Moai");
    assert_eq!(strike("core:nation/america", Some((6, 5)), (7, 5)), bare, "only Polynesia");
}
