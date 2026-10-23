//! Sight on the classic rules (`derived::visibility`): a unit sees its
//! kind's sight (+ promotions, +1 on hills), a city three rings, owned
//! tiles their neighbours; an embarked unit sees the embarked sight (1, a
//! Polynesian 2), the Great Lighthouse widens the sight of ships already at
//! sea, and submarines are seen only up close or by units that see them;
//! forest and jungle hide the tiles behind them from
//! a unit on flat land, not from one on hills. Every step is checked
//! against the full rebuild (`validate`).

use classic_content::core::core_rules;
use classic_rules::combat::exchange::{AttackKind, PlanError};
use classic_rules::combat::plan_attack;
use classic_rules::game::ClassicGame;
use classic_rules::ids::{BuildingKindId, FeatureId, PlayerId, PromotionId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::rules::roles::{Ground, TerrainPalette};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const GRASSLAND: TerrainId = TerrainId(0);
/// The classic palette is the legacy one (`rules_use_the_legacy_palette` in classic-content).
const HILLS: TerrainId = TerrainId(TerrainPalette::LEGACY.code(Ground::Hills));

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn forest(rules: &CompiledRuleset) -> FeatureId {
    FeatureId(rules.features.iter().position(|f| f.key == "core:feature/forest").unwrap() as u16)
}

fn promotion(rules: &CompiledRuleset, key: &str) -> PromotionId {
    PromotionId(rules.promotions.iter().position(|p| p.key == key).unwrap() as u16)
}

/// Open grassland, nobody owns anything.
fn flat_world(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 });
    let tiles: Vec<_> = (0..game.view().auth.map().tile_count() as u32).map(|t| (TileId(t), GRASSLAND)).collect();
    game.debug_set_terrain_batch(rules, &tiles);
    game
}

fn visible(game: &GameState, rules: &CompiledRuleset, t: TileId) -> bool {
    game.player_view(rules, P0).is_visible(t)
}

/// Tiles at exactly `ring` steps from `at` that P0 sees, and their number.
fn ring_seen(game: &GameState, rules: &CompiledRuleset, at: TileId, ring: i32) -> (usize, usize) {
    let dims = game.view().auth.map().dimensions;
    let ring: Vec<_> = classic_rules::math::hex_disc(dims, at, ring)
        .filter(|t| dims.distance(at, *t) == Some(ring))
        .collect();
    (ring.iter().filter(|t| visible(game, rules, **t)).count(), ring.len())
}

#[test]
fn a_unit_on_flat_land_sees_two_rings() {
    let rules = rules();
    let mut game = flat_world(&rules);
    let at = tile(&game, 12, 10);
    game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/warrior"));
    game.validate(&rules).unwrap();
    let (seen, total) = ring_seen(&game, &rules, at, 2);
    assert_eq!(seen, total, "ring 2");
    assert_eq!(ring_seen(&game, &rules, at, 3).0, 0, "nothing at ring 3");
}

#[test]
fn a_city_sees_three_rings_and_its_land_shows_the_neighbours() {
    let rules = rules();
    let mut game = flat_world(&rules);
    let at = tile(&game, 12, 10);
    let settler = game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/settler"));
    game.execute(&rules, Command::FoundCity { settler }).unwrap();
    game.validate(&rules).unwrap();
    for ring in 1..=3 {
        let (seen, total) = ring_seen(&game, &rules, at, ring);
        assert_eq!(seen, total, "ring {ring}");
    }
    let map = game.view().auth.map();
    let dims = map.dimensions;
    let owned: Vec<TileId> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| map.owner(*t).flatten() == Some(P0))
        .collect();
    assert!(!owned.is_empty());
    for t in owned {
        for n in classic_rules::math::hex_disc(dims, t, 1) {
            assert!(visible(&game, &rules, n), "the neighbour {n:?} of owned {t:?}");
        }
    }
}

#[test]
fn a_forest_hides_what_lies_behind_it_from_flat_land() {
    let rules = rules();
    let mut game = flat_world(&rules);
    let at = tile(&game, 12, 10);
    let wood = tile(&game, 13, 10);
    let behind = tile(&game, 14, 10);
    game.debug_set_feature_batch(&rules, &[(wood, forest(&rules))]);
    game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/warrior"));
    game.validate(&rules).unwrap();
    assert!(visible(&game, &rules, wood), "the forest itself is seen");
    assert!(!visible(&game, &rules, behind), "the tile behind the forest is hidden");
}

#[test]
fn a_unit_on_hills_sees_further_and_over_a_forest() {
    let rules = rules();
    let mut game = flat_world(&rules);
    let at = tile(&game, 12, 10);
    let wood = tile(&game, 13, 10);
    let behind = tile(&game, 14, 10);
    game.debug_set_terrain_batch(&rules, &[(at, HILLS)]);
    game.debug_set_feature_batch(&rules, &[(wood, forest(&rules))]);
    game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/warrior"));
    game.validate(&rules).unwrap();
    assert!(visible(&game, &rules, behind), "seen over the forest");
    let (seen, total) = ring_seen(&game, &rules, at, 3);
    assert!(seen > 0 && seen <= total, "ring 3 from hills: {seen}/{total}");
    assert_eq!(ring_seen(&game, &rules, at, 4).0, 0, "nothing at ring 4");
}

/// Hills hide what lies behind them from flat land and not from hills; a
/// mountain hides it from both.
#[test]
fn hills_hide_from_flat_land_and_mountains_from_everywhere() {
    let rules = rules();
    let mountain = TerrainId(TerrainPalette::LEGACY.code(Ground::Mountains));
    let behind_of = |game: &mut GameState, eye: Option<TerrainId>, between: TerrainId| {
        let (at, blocker, behind) = (tile(game, 12, 10), tile(game, 13, 10), tile(game, 14, 10));
        game.debug_set_terrain_batch(&rules, &[(at, eye.unwrap_or(GRASSLAND)), (blocker, between), (behind, GRASSLAND)]);
        game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/warrior"));
        game.validate(&rules).unwrap();
        assert!(visible(game, &rules, blocker), "the blocker itself is seen");
        visible(game, &rules, behind)
    };
    let seen = |eye, between| behind_of(&mut flat_world(&rules), eye, between);
    assert!(!seen(None, HILLS), "hills hide from flat land");
    assert!(seen(Some(HILLS), HILLS), "hills do not hide from hills");
    assert!(!seen(None, mountain), "a mountain hides from flat land");
    assert!(!seen(Some(HILLS), mountain), "a mountain hides from hills");
}

#[test]
fn moving_off_hills_and_planting_a_forest_keep_the_rebuild() {
    let rules = rules();
    let mut game = flat_world(&rules);
    let at = tile(&game, 12, 10);
    let next = tile(&game, 13, 10);
    game.debug_set_terrain_batch(&rules, &[(at, HILLS)]);
    let warrior = game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/warrior"));
    game.execute(&rules, Command::MoveUnit { unit: warrior, destination: next }).unwrap();
    game.validate(&rules).unwrap();
    assert_eq!(ring_seen(&game, &rules, next, 3).0, 0, "flat land again: two rings");
    game.debug_set_feature_batch(&rules, &[(tile(&game, 14, 10), forest(&rules))]);
    game.validate(&rules).unwrap();
    assert!(!visible(&game, &rules, tile(&game, 15, 10)), "hidden behind the new forest");
}

#[test]
fn scouting_promotion_adds_a_ring() {
    let rules = rules();
    let mut game = flat_world(&rules);
    let at = tile(&game, 12, 10);
    let scout = game.debug_spawn_unit(&rules, P0, at, unit(&rules, "core:unit/scout"));
    assert_eq!(ring_seen(&game, &rules, at, 3).0, 0, "a scout sees two rings like any unit");
    game.debug_set_experience(scout, 10);
    game.execute(&rules, Command::Promote { unit: scout, promotion: promotion(&rules, "core:promotion/scouting-1") })
        .unwrap();
    game.validate(&rules).unwrap();
    let (seen, total) = ring_seen(&game, &rules, at, 3);
    assert_eq!(seen, total, "Scouting I: ring 3");
}

const P1: PlayerId = PlayerId(1);
const WATER: TerrainId = TerrainId(4);

fn sight(game: &GameState, unit: UnitHandle) -> i32 {
    game.derived().visibility.sight_of(unit)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

/// Water on the columns `from..to`.
fn sea(game: &mut GameState, rules: &CompiledRuleset, from: i32, to: i32) {
    let height = game.view().auth.map().dimensions.height as i32;
    let batch: Vec<(TileId, TerrainId)> = (0..height)
        .flat_map(|r| (from..to).map(|q| (q, r)).collect::<Vec<_>>())
        .map(|(q, r)| (tile(game, q, r), WATER))
        .collect();
    game.debug_set_terrain_batch(rules, &batch);
}

/// A scout sees 2 tiles, 3 after Scouting I; a catapult 1 (Limited
/// Visibility); the knowledge follows the wider disc.
#[test]
fn units_see_their_own_radius_and_promotions_widen_it() {
    let rules = rules();
    let mut game = flat_world(&rules);
    let scout = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), unit(&rules, "core:unit/scout"));
    let catapult = game.debug_spawn_unit(&rules, P0, tile(&game, 15, 8), unit(&rules, "core:unit/catapult"));
    assert_eq!(sight(&game, scout), 2);
    assert_eq!(sight(&game, catapult), 1, "siege units see one tile less");
    assert!(!visible(&game, &rules, tile(&game, 8, 5)));
    game.debug_set_experience(scout, 10);
    game.execute(&rules, Command::Promote { unit: scout, promotion: promotion(&rules, "core:promotion/scouting-1") })
        .unwrap();
    assert_eq!(sight(&game, scout), 3, "Scouting I: +1 sight");
    assert!(visible(&game, &rules, tile(&game, 8, 5)));
    assert!(game.view().auth.knowledge()[0].is_explored(tile(&game, 8, 5)), "the wider disc is explored");
    game.validate(&rules).unwrap();
}

/// Embarked units see the embarked sight (1); Polynesia's units 2 (+1); the
/// radius comes back on land.
#[test]
fn embarked_units_see_one_tile_and_polynesians_two() {
    let rules = rules();
    let mut game = flat_world(&rules);
    sea(&mut game, &rules, 6, 14);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), unit(&rules, "core:unit/warrior"));
    game.execute(&rules, Command::MoveUnit { unit: warrior, destination: tile(&game, 6, 5) }).unwrap();
    assert_eq!(sight(&game, warrior), 1, "embarked");
    assert!(!visible(&game, &rules, tile(&game, 8, 5)));
    game.validate(&rules).unwrap();
    let polynesia = rules.civs.iter().position(|c| c.key == "core:nation/polynesia").unwrap() as u8;
    let mut game = flat_world(&rules);
    sea(&mut game, &rules, 6, 14);
    game.debug_set_player_civ(P0, polynesia);
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), unit(&rules, "core:unit/warrior"));
    game.execute(&rules, Command::MoveUnit { unit: warrior, destination: tile(&game, 6, 5) }).unwrap();
    assert_eq!(sight(&game, warrior), 2, "Wayfinding: +1 sight embarked");
    game.validate(&rules).unwrap();
}

/// An owner-wide source widens the sight of the units already on the map:
/// the Great Lighthouse gives naval units +1 sight when it is built.
#[test]
fn the_great_lighthouse_widens_the_sight_of_ships_at_sea() {
    let rules = rules();
    let mut game = flat_world(&rules);
    sea(&mut game, &rules, 6, 20);
    let city = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let ship = game.debug_spawn_unit(&rules, P0, tile(&game, 12, 8), unit(&rules, "core:unit/trireme"));
    assert_eq!(sight(&game, ship), 2);
    let wonder = BuildingKindId(rules.buildings.iter().position(|b| b.key == "core:building/great_lighthouse").unwrap() as u16);
    game.debug_add_building(&rules, city, wonder);
    assert_eq!(sight(&game, ship), 3, "the wonder's promotion reaches the ship");
    assert!(visible(&game, &rules, tile(&game, 15, 8)));
    game.validate(&rules).unwrap();
}

/// A submarine is seen only next to the viewer's units or cities, or by a
/// destroyer within its sight; an unseen submarine is no target.
#[test]
fn submarines_are_seen_only_up_close_or_by_destroyers() {
    let rules = rules();
    let mut game = flat_world(&rules);
    sea(&mut game, &rules, 4, 24);
    let sub = game.debug_spawn_unit(&rules, P1, tile(&game, 12, 8), unit(&rules, "core:unit/submarine"));
    let frigate = game.debug_spawn_unit(&rules, P0, tile(&game, 10, 8), unit(&rules, "core:unit/frigate"));
    game.debug_meet(P0, P1);
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    let seen = |game: &GameState| {
        let core = *game.view().auth.units().get(sub).unwrap().core;
        game.player_view(&rules, P0).sees_unit(&core)
    };
    assert!(visible(&game, &rules, tile(&game, 12, 8)), "the tile is in sight");
    assert!(!seen(&game), "two tiles away the submarine is invisible");
    let observed = game.ai_observation(&rules, P0);
    assert!(observed.visible_foreign_units.iter().all(|u| u.handle != sub), "the AI does not see it");
    assert_eq!(
        plan_attack(game.view(), &rules, frigate, tile(&game, 12, 8), AttackKind::Ranged).err(),
        Some(PlanError::NoTarget),
        "an unseen submarine is no target"
    );
    // Next to it, any unit sees it.
    game.execute(&rules, Command::MoveUnit { unit: frigate, destination: tile(&game, 11, 8) }).unwrap();
    assert!(seen(&game));
    // A destroyer sees it within its sight (2) without being next to it.
    let mut game = flat_world(&rules);
    sea(&mut game, &rules, 4, 24);
    let sub = game.debug_spawn_unit(&rules, P1, tile(&game, 12, 8), unit(&rules, "core:unit/submarine"));
    game.debug_spawn_unit(&rules, P0, tile(&game, 10, 8), unit(&rules, "core:unit/destroyer"));
    let core = *game.view().auth.units().get(sub).unwrap().core;
    assert!(game.player_view(&rules, P0).sees_unit(&core), "a destroyer sees submarines");
    game.validate(&rules).unwrap();
}
