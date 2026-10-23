//! Civilization traits of the society systems on the classic ruleset: Babylon
//! and Venice (a free great person at a technology), Carthage (free
//! harbors), Poland (a free policy per era), Korea (science boosts), the
//! Maya (Long Count), the Huns (razing), the Shoshone (territory), the
//! Netherlands (traded luxuries), Indonesia (unique luxuries), Venice (no
//! settlers).

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::command::CityDecision;
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, ResourceId, TechId, TerrainId, TileId, UnitKindId};
use classic_rules::world::relations::DealItem;
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

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn resource(rules: &CompiledRuleset, key: &str) -> ResourceId {
    ResourceId(rules.resources.iter().position(|r| r.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn as_civ(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, key: &str) {
    let civ = rules.civs.iter().position(|c| c.key == key).unwrap_or_else(|| panic!("{key}"));
    game.debug_set_player_civ(player, civ as u8);
}

fn city(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, q: i32, r: i32) -> CityHandle {
    let t = tile(game, q, r);
    game.debug_found_city(rules, player, t)
}

fn units_of_kind(game: &GameState, player: PlayerId, kind: UnitKindId) -> usize {
    let auth = game.view().auth;
    auth.indexes()
        .units_of(player)
        .iter()
        .filter(|u| auth.units().get(**u).is_some_and(|v| v.core.kind == kind))
        .count()
}

/// End turns until `player` is active.
fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..4 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    panic!("{player:?} never became active");
}

/// A settler of `player` founds a city on `at` (OnCityFounded fires).
fn found(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, q: i32, r: i32) -> CityHandle {
    let at = tile(game, q, r);
    until_active(game, rules, player);
    let settler = game.debug_spawn_unit(rules, player, at, unit(rules, "core:unit/settler"));
    game.execute(rules, Command::FoundCity { settler }).unwrap();
    game.view().auth.indexes().city_on_tile(at).unwrap()
}

/// Babylon: a free Great Scientist in the capital on learning Writing,
/// not on another technology.
#[test]
fn babylon_gets_a_great_scientist_at_writing() {
    let rules = rules();
    let mut game = world(&rules);
    as_civ(&mut game, &rules, P0, "core:nation/babylon");
    city(&mut game, &rules, P0, 5, 5);
    let scientist = unit(&rules, "core:unit/great-scientist");
    game.debug_discover(&rules, P0, tech(&rules, "core:tech/pottery"));
    assert_eq!(units_of_kind(&game, P0, scientist), 0);
    game.debug_discover(&rules, P0, tech(&rules, "core:tech/writing"));
    assert_eq!(units_of_kind(&game, P0, scientist), 1);
    game.validate(&rules).unwrap();
}

/// Carthage: a free Harbor in founded coastal cities only.
#[test]
fn carthage_gets_harbors_in_coastal_cities() {
    let rules = rules();
    let mut game = world(&rules);
    as_civ(&mut game, &rules, P0, "core:nation/carthage");
    let sea: Vec<(TileId, TerrainId)> = (0..20).map(|r| (tile(&game, 0, r), WATER)).collect();
    game.debug_set_terrain_batch(&rules, &sea);
    let harbor = building(&rules, "core:building/harbor");
    let coastal = found(&mut game, &rules, P0, 1, 8);
    let inland = found(&mut game, &rules, P0, 12, 8);
    let has = |game: &GameState, c| game.view().auth.cities().get(c).unwrap().buildings.contains(&harbor);
    assert!(has(&game, coastal));
    assert!(!has(&game, inland));
    game.validate(&rules).unwrap();
}

/// Poland: a free policy on entering a new era.
#[test]
fn poland_gets_a_free_policy_per_era() {
    let rules = rules();
    let mut game = world(&rules);
    as_civ(&mut game, &rules, P0, "core:nation/poland");
    city(&mut game, &rules, P0, 5, 5);
    let free = |game: &GameState| game.view().auth.module().culture().of(P0).free_policies;
    game.debug_discover(&rules, P0, tech(&rules, "core:tech/pottery"));
    assert_eq!(free(&game), 0, "still the first era");
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/writing"));
    game.debug_discover(&rules, P0, tech(&rules, "core:tech/philosophy"));
    assert_eq!(free(&game), 1, "the Classical era");
    game.validate(&rules).unwrap();
}

/// Korea: a science building in the capital adds half a median
/// technology to the research stock.
#[test]
fn korea_science_buildings_in_the_capital_boost_research() {
    let rules = rules();
    let mut game = world(&rules);
    as_civ(&mut game, &rules, P0, "core:nation/korea");
    let capital = city(&mut game, &rules, P0, 5, 5);
    let other = city(&mut game, &rules, P0, 12, 5);
    let science = |game: &GameState| game.view().auth.players().science_of(P0);
    let library = building(&rules, "core:building/library");
    game.debug_complete_building(&rules, other, library);
    assert_eq!(science(&game), 0, "not in the capital");
    game.debug_complete_building(&rules, capital, library);
    assert!(science(&game) > 0);
    game.validate(&rules).unwrap();
}

/// The Maya: after Theology, a free great person at the end of each Long
/// Count cycle (≈ 748 BC: round 86), each kind once.
#[test]
fn maya_long_count_owes_great_people_of_new_kinds() {
    let rules = rules();
    let mut game = world(&rules);
    as_civ(&mut game, &rules, P0, "core:nation/maya");
    city(&mut game, &rules, P0, 5, 5);
    city(&mut game, &rules, P1, 25, 15);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/theology"));
    // 40-year rounds to 1000 BC (round 75), then 25 years: 725 BC at 86.
    while game.view().auth.turn().round < 87 {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
    }
    let options = classic_rules::derived::grants::next_great_person_options(game.view().auth, &rules, P0);
    assert!(!options.is_empty(), "a cycle ended");
    until_active(&mut game, &rules, P0);
    let kind = options[0];
    game.execute(&rules, Command::ChooseFreeGreatPerson { player: P0, kind }).unwrap();
    let owed = classic_rules::derived::grants::owed(game.view().auth, P0);
    assert_eq!(owed.long_count_given, 1 << kind.index());
    game.validate(&rules).unwrap();
}

/// Citizens a razing city loses over one turn of its conqueror (P0 of
/// `civ`): razing and anything else (starvation) together.
fn razed_in_a_turn(rules: &CompiledRuleset, civ: Option<&str>) -> u32 {
    let mut game = world(rules);
    if let Some(key) = civ {
        as_civ(&mut game, rules, P0, key);
    }
    let rules = rules;
    city(&mut game, &rules, P0, 2, 2);
    city(&mut game, &rules, P1, 25, 15);
    let target = city(&mut game, &rules, P1, 10, 8);
    game.debug_set_population(&rules, target, 12);
    let at = tile(&game, 9, 8);
    let attacker = game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/warrior"));
    game.debug_meet(P0, P1);
    until_active(&mut game, &rules, P0);
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    let max = classic_rules::combat::city::max_hit_points(game.view().auth, &rules, game.view().auth.cities().get(target).unwrap());
    game.debug_set_city_damage(target, max - 1);
    game.execute(&rules, Command::AttackCity { attacker, city: target }).unwrap();
    game.execute(&rules, Command::DecideCity { player: P0, city: target, decision: CityDecision::Raze }).unwrap();
    let pop = |game: &GameState| game.view().auth.cities().get(target).unwrap().population;
    let before = pop(&game);
    game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
    game.validate(&rules).unwrap();
    before - pop(&game)
}

/// The Huns raze two citizens a turn, the others one.
#[test]
fn huns_raze_twice_as_fast() {
    let rules = rules();
    assert_eq!(razed_in_a_turn(&rules, Some("core:nation/huns")), razed_in_a_turn(&rules, None) + 1);
}

/// The Shoshone: a founded city claims eight tiles more than its ring.
#[test]
fn shoshone_cities_start_with_more_territory() {
    let rules = rules();
    let count = |civ: Option<&str>| {
        let mut game = world(&rules);
        if let Some(key) = civ {
            as_civ(&mut game, &rules, P0, key);
        }
        let c = found(&mut game, &rules, P0, 12, 10);
        game.validate(&rules).unwrap();
        let map = game.view().auth.map();
        (0..map.tile_count()).filter(|t| map.owning_city(TileId(*t as u32)) == Some(c)).count()
    };
    assert_eq!(count(Some("core:nation/shoshone")), count(None) + 8);
}

/// The Netherlands keep half the happiness of a luxury traded away; any
/// other civilization loses it (and the partner gains it).
#[test]
fn netherlands_keep_half_a_traded_luxury() {
    let rules = rules();
    let traded = |civ: Option<&str>| {
        let mut game = world(&rules);
        if let Some(key) = civ {
            as_civ(&mut game, &rules, P0, key);
        }
        let c = city(&mut game, &rules, P0, 5, 5);
        city(&mut game, &rules, P1, 20, 10);
        let at = game.view().auth.cities().get(c).unwrap().tile;
        let silk = resource(&rules, "core:resource/silk");
        game.debug_set_deposit(&rules, at, silk, 1);
        game.debug_meet(P0, P1);
        let luxuries = |game: &GameState, p| classic_rules::derived::happiness::player_happiness(game.view().auth, &rules, p).luxuries;
        let before = (luxuries(&game, P0), luxuries(&game, P1));
        until_active(&mut game, &rules, P0);
        let id = game.view().auth.module().relations().next_offer_id();
        let items = vec![(P0, DealItem::Resource { resource: silk, amount: 1 })];
        game.execute(&rules, Command::ProposeDeal { from: P0, to: P1, items }).unwrap();
        game.execute(&rules, Command::RespondDeal { player: P1, offer: id, accept: true }).unwrap();
        game.validate(&rules).unwrap();
        (before, (luxuries(&game, P0), luxuries(&game, P1)))
    };
    let (before, after) = traded(None);
    assert_eq!((before.0 - after.0, after.1 - before.1), (4, 4), "the luxury moves with the deal");
    let (before, after) = traded(Some("core:nation/netherlands"));
    assert_eq!(before.0 - after.0, 2, "half is kept");
}

/// Indonesia: a city founded on another landmass than the capital gets
/// two copies of a unique luxury and cannot be razed; one on the home
/// landmass gets nothing.
#[test]
fn indonesia_gets_unique_luxuries_on_new_landmasses() {
    let rules = rules();
    let mut game = world(&rules);
    as_civ(&mut game, &rules, P0, "core:nation/indonesia");
    // A sea from column 14 to 16 splits the map in two landmasses.
    let mut sea = Vec::new();
    for q in 14..17 {
        for r in 0..20 {
            if let Some(t) = game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q, r }) {
                sea.push((t, WATER));
            }
        }
    }
    game.debug_set_terrain_batch(&rules, &sea);
    let home = found(&mut game, &rules, P0, 5, 5);
    let near = found(&mut game, &rules, P0, 9, 10);
    let far = found(&mut game, &rules, P0, 22, 10);
    let map = game.view().auth.map();
    let on_tile = |c: CityHandle| map.resource(game.view().auth.cities().get(c).unwrap().tile).flatten();
    let unique = |r: Option<ResourceId>| r.is_some_and(|r| rules.resources[r.0 as usize].civilization.is_some());
    assert!(!unique(on_tile(home)) && !unique(on_tile(near)));
    assert!(unique(on_tile(far)), "{:?}", on_tile(far));
    let far_tile = game.view().auth.cities().get(far).unwrap().tile;
    assert_eq!(map.resource_amount(far_tile), 2);
    assert!(classic_rules::derived::grants::holds_unique_luxury(
        game.view().auth,
        &rules,
        game.view().auth.cities().get(far).unwrap()
    ));
    game.validate(&rules).unwrap();
}

/// Venice trains no settlers.
#[test]
fn venice_trains_no_settlers() {
    let rules = rules();
    let mut game = world(&rules);
    as_civ(&mut game, &rules, P0, "core:nation/venice");
    let c = city(&mut game, &rules, P0, 5, 5);
    game.debug_set_population(&rules, c, 4);
    until_active(&mut game, &rules, P0);
    let settler = classic_rules::ids::ProductionItem::Unit(unit(&rules, "core:unit/settler"));
    let order = Command::EnqueueProduction { player: P0, city: c, item: settler };
    assert!(game.execute(&rules, order).is_err());
    let warrior = classic_rules::ids::ProductionItem::Unit(unit(&rules, "core:unit/warrior"));
    game.execute(&rules, Command::EnqueueProduction { player: P0, city: c, item: warrior }).unwrap();
}
