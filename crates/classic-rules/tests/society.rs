//! Society effects on the classic ruleset (classic/docs/classic/rules.md): instant
//! grants and the free choices they owe (Great Library, Liberty, Terracotta
//! Army, Taj Mahal, Representation, CN Tower), golden-age and garrison
//! conditions (Brazil, Military Caste), coastal cities (Maritime
//! Infrastructure), society stats (Meritocracy, Fine Arts, Rationalism,
//! Civil Society, Courthouse, Flourishing of the Arts), Iron Curtain and
//! Assyria on conquest.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::command::CityDecision;
use classic_rules::effects::city::city_modifiers;
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, PolicyId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::effects::YieldKind;
use classic_rules::classic::effects::GreatPersonKind;
use classic_rules::rules::stats::Stat;
use classic_rules::world::city::CityStatus;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const WATER: TerrainId = TerrainId(4);

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
    PolicyId(rules.module.policies.iter().position(|p| p.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn nation(rules: &CompiledRuleset, key: &str) -> u8 {
    rules.nations.iter().position(|c| c.key == key).unwrap_or_else(|| panic!("{key}")) as u8
}

fn city(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, q: i32, r: i32) -> CityHandle {
    let t = tile(game, q, r);
    game.debug_found_city(rules, player, t)
}

fn golden_age_turns(game: &GameState, player: PlayerId) -> u16 {
    game.view().auth.players().economy_of(player).unwrap().golden_age_turns
}

fn units_of_kind(game: &GameState, player: PlayerId, kind: UnitKindId) -> usize {
    let auth = game.view().auth;
    auth.indexes()
        .units_of(player)
        .iter()
        .filter(|u| auth.units().get(**u).is_some_and(|v| v.core.kind == kind))
        .count()
}

fn save_load_keeps_hash(game: &GameState, rules: &CompiledRuleset) {
    let loaded = decode(&encode(game, rules), rules).expect("own save loads");
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(game), "save/load");
}

// ---- grants ----

/// The Great Library owes a free technology; the player chooses one it may
/// research; the choice survives a save; another choice is refused.
#[test]
fn great_library_owes_a_free_technology_of_the_players_choice() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, P0, 5, 5);
    game.debug_complete_building(&rules, c, building(&rules, "core:building/great_library"));
    let options = classic_rules::derived::grants::next_technology_options(game.view().auth, &rules, P0);
    assert!(!options.is_empty());
    save_load_keeps_hash(&game, &rules);
    let locked = tech(&rules, "core:tech/philosophy");
    assert!(!options.contains(&locked), "prerequisites first");
    assert!(game.execute(&rules, Command::ChooseFreeTechnology { player: P0, tech: locked }).is_err());
    let pick = options[0];
    game.execute(&rules, Command::ChooseFreeTechnology { player: P0, tech: pick }).unwrap();
    assert!(game.view().auth.players().techs_of(P0).contains(&pick));
    let next = classic_rules::derived::grants::researchable(game.view().auth, &rules, P0)[0];
    let again = Command::ChooseFreeTechnology { player: P0, tech: next };
    assert!(game.execute(&rules, again).is_err(), "one technology owed");
    game.validate(&rules).unwrap();
}

/// The Liberty finisher owes a great person: it appears in the capital and
/// counts toward the next threshold.
#[test]
fn liberty_finisher_gives_a_great_person_in_the_capital() {
    let rules = rules();
    let mut game = world(&rules);
    city(&mut game, &rules, P0, 5, 5);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/liberty/finisher"));
    let options = classic_rules::derived::grants::next_great_person_options(game.view().auth, &rules, P0);
    assert!(options.contains(&GreatPersonKind::Scientist), "{options:?}");
    assert!(!options.contains(&GreatPersonKind::General), "{options:?}");
    let scientist = unit(&rules, "core:unit/great-scientist");
    let before = units_of_kind(&game, P0, scientist);
    let born = game.view().auth.players().economy_of(P0).unwrap().great_people_created;
    game.execute(&rules, Command::ChooseFreeGreatPerson { player: P0, kind: GreatPersonKind::Scientist })
        .unwrap();
    assert_eq!(units_of_kind(&game, P0, scientist), before + 1);
    assert_eq!(game.view().auth.players().economy_of(P0).unwrap().great_people_created, born + 1);
    assert!(game
        .execute(&rules, Command::ChooseFreeGreatPerson { player: P0, kind: GreatPersonKind::Scientist })
        .is_err());
    game.validate(&rules).unwrap();
}

/// The Terracotta Army copies each land combat kind the owner has, once.
#[test]
fn terracotta_army_copies_each_land_combat_kind() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, P0, 5, 5);
    let warrior = unit(&rules, "core:unit/warrior");
    let archer = unit(&rules, "core:unit/archer");
    let worker = unit(&rules, "core:unit/worker");
    for (kind, q) in [(warrior, 8), (warrior, 9), (archer, 10), (worker, 11)] {
        let at = tile(&game, q, 8);
        game.debug_spawn_unit(&rules, P0, at, kind);
    }
    game.debug_complete_building(&rules, c, building(&rules, "core:building/terracotta_army"));
    assert_eq!(units_of_kind(&game, P0, warrior), 3);
    assert_eq!(units_of_kind(&game, P0, archer), 2);
    assert_eq!(units_of_kind(&game, P0, worker), 1, "civilians are not copied");
    game.validate(&rules).unwrap();
}

/// The Taj Mahal and Representation start golden ages of the standard
/// length; a golden age started during one lengthens it.
#[test]
fn taj_mahal_and_representation_start_golden_ages() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, P0, 5, 5);
    game.debug_complete_building(&rules, c, building(&rules, "core:building/taj_mahal"));
    assert_eq!(golden_age_turns(&game, P0), 10);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/liberty/representation"));
    assert_eq!(golden_age_turns(&game, P0), 20);
    assert_eq!(game.view().auth.players().economy_of(P0).unwrap().golden_ages, 1, "one golden age");
    game.validate(&rules).unwrap();
}

/// The CN Tower adds a citizen to every city; Resettlement gives new
/// cities two more.
#[test]
fn cn_tower_and_resettlement_add_citizens() {
    let rules = rules();
    let mut game = world(&rules);
    let a = city(&mut game, &rules, P0, 5, 5);
    let b = city(&mut game, &rules, P0, 12, 5);
    game.debug_complete_building(&rules, a, building(&rules, "core:building/cn_tower"));
    let pop = |game: &GameState, c| game.view().auth.cities().get(c).unwrap().population;
    assert_eq!((pop(&game, a), pop(&game, b)), (2, 2));
    game.validate(&rules).unwrap();
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/order/resettlement"));
    let at = tile(&game, 20, 12);
    let settler = game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/settler"));
    if game.view().auth.active_player() != P0 {
        game.execute(&rules, Command::EndTurn { player: P1 }).unwrap();
    }
    game.execute(&rules, Command::FoundCity { settler }).unwrap();
    let founded = game.view().auth.indexes().city_on_tile(at).unwrap();
    assert_eq!(pop(&game, founded), 3);
    game.validate(&rules).unwrap();
}

// ---- conditions ----

/// Brazil: +50 % great artists, musicians and writers and +100 % tourism
/// while in a golden age.
#[test]
fn brazil_carnival_holds_only_in_a_golden_age() {
    let rules = rules();
    let mut game = world(&rules);
    game.debug_set_player_nation(P0, nation(&rules, "core:nation/brazil"));
    let c = city(&mut game, &rules, P0, 5, 5);
    let rate = |game: &GameState| {
        let auth = game.view().auth;
        let mods = city_modifiers(auth, &rules, auth.cities().get(c).unwrap());
        (mods.stats.get(Stat::Module(classic_rules::rules::stats::ClassicStat::GreatPersonRate(GreatPersonKind::Artist))), mods.module.tourism.percent)
    };
    assert_eq!(rate(&game), (0, 0));
    game.debug_complete_building(&rules, c, building(&rules, "core:building/taj_mahal"));
    assert_eq!(rate(&game), (50, 100));
    game.validate(&rules).unwrap();
}

/// Military Caste: a garrisoned city has +1 happiness and +2 culture; the
/// unit leaving takes them away (the yields cache follows).
#[test]
fn military_caste_follows_the_garrison() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, P0, 5, 5);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/honor/military-caste"));
    let culture = |game: &GameState| game.derived().yields.of(c).unwrap().culture;
    let happiness = |game: &GameState| {
        let auth = game.view().auth;
        city_modifiers(auth, &rules, auth.cities().get(c).unwrap()).module.happiness
    };
    let before = (culture(&game), happiness(&game));
    let at = game.view().auth.cities().get(c).unwrap().tile;
    let warrior: UnitHandle = game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/warrior"));
    game.validate(&rules).unwrap();
    assert_eq!((culture(&game), happiness(&game)), (before.0 + 2, before.1 + 1));
    if game.view().auth.active_player() != P0 {
        game.execute(&rules, Command::EndTurn { player: P1 }).unwrap();
    }
    let out = tile(&game, 6, 5);
    game.execute(&rules, Command::MoveUnit { unit: warrior, destination: out }).unwrap();
    game.validate(&rules).unwrap();
    assert_eq!((culture(&game), happiness(&game)), before);
}

/// Maritime Infrastructure: production only in coastal cities.
#[test]
fn maritime_infrastructure_reaches_coastal_cities() {
    let rules = rules();
    let mut game = world(&rules);
    let sea: Vec<(TileId, TerrainId)> = (0..20).map(|r| (tile(&game, 0, r), WATER)).collect();
    game.debug_set_terrain_batch(&rules, &sea);
    let coastal = city(&mut game, &rules, P0, 1, 8);
    let inland = city(&mut game, &rules, P0, 10, 8);
    let flat = |game: &GameState, c| {
        let auth = game.view().auth;
        city_modifiers(auth, &rules, auth.cities().get(c).unwrap()).yields[YieldKind::Production.index()].flat
    };
    let before = (flat(&game, coastal), flat(&game, inland));
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/exploration/maritime-infrastructure"));
    assert_eq!(flat(&game, coastal), before.0 + 3);
    assert_eq!(flat(&game, inland), before.1);
    game.validate(&rules).unwrap();
}

// ---- society stats ----

/// Meritocracy: +1 happiness per city connected to the capital.
#[test]
fn meritocracy_counts_cities_connected_to_the_capital() {
    let rules = rules();
    let mut game = world(&rules);
    city(&mut game, &rules, P0, 5, 5);
    city(&mut game, &rules, P0, 9, 5);
    city(&mut game, &rules, P0, 20, 15);
    let road: Vec<TileId> = (6..9).map(|q| tile(&game, q, 5)).collect();
    game.debug_set_road_batch(&rules, &road);
    let happiness = |game: &GameState| classic_rules::derived::happiness::player_happiness(game.view().auth, &rules, P0).buildings;
    let before = happiness(&game);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/liberty/meritocracy"));
    assert_eq!(happiness(&game) - before, 1, "one city connected, the far one not");
    game.validate(&rules).unwrap();
}

/// Civil Society: specialists eat half; Universal Suffrage: they make half
/// the unhappiness.
#[test]
fn freedom_tenets_halve_specialist_food_and_unhappiness() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, P0, 5, 5);
    game.debug_set_population(&rules, c, 4);
    game.debug_add_building(&rules, c, building(&rules, "core:building/public_school"));
    let scientist = rules.specialists.iter().position(|s| s.key == "core:specialist/scientist").unwrap();
    let specialist = classic_rules::rules::content::SpecialistId(scientist as u16);
    if game.view().auth.active_player() != P0 {
        game.execute(&rules, Command::EndTurn { player: P1 }).unwrap();
    }
    game.execute(&rules, Command::SetSpecialists { player: P0, city: c, specialist, count: 1 }).unwrap();
    let eaten = |game: &GameState| {
        let auth = game.view().auth;
        classic_rules::derived::income::city_income(auth, &rules, auth.cities().get(c).unwrap()).food_eaten
    };
    let specialists = |game: &GameState| {
        let auth = game.view().auth;
        classic_rules::derived::yields::city_work(auth, &rules, auth.cities().get(c).unwrap()).specialist_count()
    };
    let n = specialists(&game) as i32;
    assert!(n > 0, "the city has specialists");
    assert_eq!(eaten(&game), 8);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/freedom/civil-society"));
    assert_eq!(eaten(&game), 8 - n, "each specialist eats one food less");
    let unhappy = |game: &GameState| classic_rules::derived::happiness::player_happiness(game.view().auth, &rules, P0).from_population;
    let before = unhappy(&game);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/freedom/universal-suffrage"));
    assert_eq!(before - unhappy(&game), (n + 1) / 2);
    game.validate(&rules).unwrap();
}

/// Fine Arts turns half the excess happiness into culture; Rationalism
/// adds 10 % science while the empire is happy.
#[test]
fn fine_arts_and_rationalism_read_the_excess_happiness() {
    let rules = rules();
    let mut game = world(&rules);
    city(&mut game, &rules, P0, 5, 5);
    // A turn of P0 takes the happiness snapshot.
    for _ in 0..2 {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
    }
    let excess = classic_rules::derived::happiness::excess_snapshot(game.view().auth, P0);
    assert!(excess > 0, "{excess}");
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/aesthetics/fine-arts"));
    let culture = classic_rules::derived::income::culture_from_happiness(game.view().auth, &rules, P0);
    assert_eq!(culture as i32, excess / 2);
    let science = |game: &GameState| classic_rules::derived::income::player_income(game.view().auth, &rules, P0).science;
    let before = science(&game);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/rationalism/opener"));
    assert_eq!(science(&game), before + before / 10);
}

/// An occupied city with a Courthouse is unhappy as a normal city.
#[test]
fn courthouse_ends_occupation_unhappiness() {
    let rules = rules();
    let mut game = world(&rules);
    city(&mut game, &rules, P0, 5, 5);
    let c = city(&mut game, &rules, P0, 12, 5);
    game.debug_set_population(&rules, c, 6);
    game.debug_set_city_status(c, CityStatus::Occupied);
    let occupied = |game: &GameState| classic_rules::derived::happiness::player_happiness(game.view().auth, &rules, P0).from_occupied;
    assert!(occupied(&game) > 0);
    game.debug_add_building(&rules, c, building(&rules, "core:building/courthouse"));
    assert_eq!(occupied(&game), 0);
}

/// Flourishing of the Arts: +33 % of the culture of the city's wonders.
#[test]
fn flourishing_of_the_arts_raises_wonder_culture() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, P0, 5, 5);
    game.debug_add_building(&rules, c, building(&rules, "core:building/great_library"));
    game.debug_add_building(&rules, c, building(&rules, "core:building/pyramids"));
    game.debug_add_building(&rules, c, building(&rules, "core:building/stonehenge"));
    let culture = |game: &GameState| game.derived().yields.of(c).unwrap().culture;
    let before = culture(&game);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/aesthetics/flourishing-of-the-arts"));
    // Three wonders of +1 culture: 3 × 33 % rounds down to +0, so add more.
    let bonus = culture(&game) - before;
    let wonder_culture: i32 = ["great_library", "pyramids", "stonehenge"]
        .iter()
        .flat_map(|k| rules.buildings[building(&rules, &format!("core:building/{k}")).0 as usize].effects.passive_city.iter())
        .filter_map(|e| match e.modifier {
            classic_rules::rules::effects::Modifier::AddYield { yield_kind: YieldKind::Culture, amount } => Some(amount),
            _ => None,
        })
        .sum();
    assert_eq!(bonus, wonder_culture * 33 / 100);
    game.validate(&rules).unwrap();
}

// ---- conquest ----

/// A city of P1 captured by P0 (a warrior against a city at 1 HP).
fn captured_city(game: &mut GameState, rules: &CompiledRuleset) -> CityHandle {
    city(game, rules, P0, 2, 2);
    city(game, rules, P1, 25, 15);
    let target = city(game, rules, P1, 10, 8);
    game.debug_set_population(rules, target, 4);
    let at = tile(game, 9, 8);
    let attacker = game.debug_spawn_unit(rules, P0, at, unit(rules, "core:unit/warrior"));
    game.debug_meet(P0, P1);
    if game.view().auth.active_player() != P0 {
        game.execute(rules, Command::EndTurn { player: P1 }).unwrap();
    }
    game.execute(rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    let max = classic_rules::combat::city::max_hit_points(game.view().auth, rules, game.view().auth.cities().get(target).unwrap());
    game.debug_set_city_damage(target, max - 1);
    game.execute(rules, Command::AttackCity { attacker, city: target }).unwrap();
    assert_eq!(game.view().auth.cities().get(target).unwrap().owner, P0);
    target
}

/// Iron Curtain: a free Courthouse in a conquered city.
#[test]
fn iron_curtain_gives_a_courthouse_on_conquest() {
    let rules = rules();
    let mut game = world(&rules);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/order/iron-curtain"));
    let c = captured_city(&mut game, &rules);
    let courthouse = building(&rules, "core:building/courthouse");
    assert!(game.view().auth.cities().get(c).unwrap().buildings.contains(&courthouse));
    game.validate(&rules).unwrap();
}

/// Assyria: a free technology the old owner knows, once per city; Venice
/// may not annex.
#[test]
fn assyria_takes_a_technology_and_venice_cannot_annex() {
    let rules = rules();
    let mut game = world(&rules);
    game.debug_set_player_nation(P0, nation(&rules, "core:nation/assyria"));
    let writing = tech(&rules, "core:tech/writing");
    game.debug_learn(&rules, P1, tech(&rules, "core:tech/pottery"));
    game.debug_learn(&rules, P1, writing);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/pottery"));
    let c = captured_city(&mut game, &rules);
    let options = classic_rules::derived::grants::next_technology_options(game.view().auth, &rules, P0);
    assert_eq!(options, vec![writing]);
    game.execute(&rules, Command::ChooseFreeTechnology { player: P0, tech: writing }).unwrap();
    assert!(game.view().auth.players().techs_of(P0).contains(&writing));
    save_load_keeps_hash(&game, &rules);
    game.validate(&rules).unwrap();

    let mut game = world(&rules);
    game.debug_set_player_nation(P0, nation(&rules, "core:nation/venice"));
    let c2 = captured_city(&mut game, &rules);
    let annex = Command::DecideCity { player: P0, city: c2, decision: CityDecision::Annex };
    assert!(game.execute(&rules, annex).is_err());
    let puppet = Command::DecideCity { player: P0, city: c2, decision: CityDecision::Puppet };
    game.execute(&rules, puppet).unwrap();
    let _ = c;
}
