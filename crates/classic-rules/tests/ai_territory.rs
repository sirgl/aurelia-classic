//! The AI's effect conditions over a territory (`ai::territory`) on the
//! real rules: for every kind of tile condition the buildings' yield
//! effects use, a building picked by scanning the rules (not by name)
//! counts exactly the tiles of a scene built to match it, and adds per tile
//! what the engine's own tile effects add; a city condition (coast, river)
//! of any source follows the centre. The site features (`ai::site`) fit
//! the rules' tables and read the scene.

use classic_content::core::core_rules;
use classic_rules::ai::site::{site_features, SiteInputs, FEATURE_SLOTS, SITE_FEATURES, SITE_NAMES, TERRAIN_SLOTS};
use classic_rules::ai::territory::{building_tile_bonus, effect_meets, effect_set_bonus, territory, tile_meets, TerritoryContext};
use classic_rules::derived::tile_yields::nature_yields;
use classic_rules::effects::city::city_tile_effects;
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, TerrainId, TileId};
use classic_rules::math::{hex_disc, HexCoord};
use classic_rules::rules::content::Yields;
use classic_rules::rules::effects::{CompiledEffect, Condition, EffectSet, Modifier, PlotFilter, YieldKind};
use classic_rules::{compile_rules, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const RADIUS: i32 = 3;

struct Scene {
    game: GameState,
    rules: CompiledRuleset,
    centre: TileId,
}

/// The map code of the first land terrain a city may stand on that is not
/// `but`.
fn land_code(rules: &CompiledRuleset, but: Option<u16>) -> u16 {
    rules
        .terrains
        .iter()
        .filter(|t| !t.water && t.can_found_city && !t.impassable)
        .filter_map(|t| t.map_code)
        .find(|code| Some(*code) != but && !rules.terrain_kinds.is_hills(*code))
        .expect("a flat land terrain")
}

/// The map code of a water terrain.
fn water_code(rules: &CompiledRuleset) -> u16 {
    rules.terrains.iter().filter(|t| t.water).find_map(|t| t.map_code).expect("a water terrain")
}

impl Scene {
    /// A 24×24 map of one flat land terrain (not `but`), two players, the
    /// centre at (8, 8), no city.
    fn new(but: Option<u16>) -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 2, width: 24, height: 24, rng_seed: 5 });
        let land = TerrainId(land_code(&rules, but));
        let all: Vec<_> = (0..24 * 24).map(|i| (TileId(i), land)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let centre = game.view().auth.map().dimensions.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
        Scene { game, rules, centre }
    }

    fn at(&self, q: i32, r: i32) -> TileId {
        self.game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
    }

    fn found(&mut self) -> CityHandle {
        self.game.debug_found_city(&self.rules, P0, self.centre)
    }

    /// The known tech that reveals `resource`, learned.
    fn reveal(&mut self, resource: classic_rules::ids::ResourceId) {
        if let Some(tech) = self.rules.resources[resource.0 as usize].required_tech {
            self.game.debug_learn(&self.rules, P0, tech);
        }
    }

    /// A sea of 12 tiles east of the centre: rows r = 7 and 8 from q = 9.
    fn sea(&mut self) -> Vec<TileId> {
        let water = TerrainId(water_code(&self.rules));
        let tiles: Vec<TileId> = (0..6).flat_map(|q| [self.at(9 + q, 8), self.at(9 + q, 7)]).collect();
        let batch: Vec<_> = tiles.iter().map(|t| (*t, water)).collect();
        self.game.debug_set_terrain_batch(&self.rules, &batch);
        tiles
    }

    /// Builds the scene that `condition` matches on, and returns the tiles it
    /// should match (within the radius or not). None: a kind this test does
    /// not build.
    fn arrange(&mut self, condition: Condition) -> Option<Vec<TileId>> {
        let rules = self.rules.clone();
        let picked = [self.at(9, 8), self.at(8, 10), self.at(5, 9)];
        match condition {
            Condition::TileResource(resource) => {
                self.reveal(resource);
                let batch: Vec<_> = picked.iter().map(|t| (*t, resource)).collect();
                self.game.debug_set_resource_batch(&rules, &batch);
                Some(picked.to_vec())
            }
            Condition::TileFeature(feature) => {
                let batch: Vec<_> = picked.iter().map(|t| (*t, feature)).collect();
                self.game.debug_set_feature_batch(&rules, &batch);
                Some(picked.to_vec())
            }
            Condition::TileTerrain(kind) => {
                let code = TerrainId(rules.terrains[kind.0 as usize].map_code?);
                let batch: Vec<_> = picked.iter().map(|t| (*t, code)).collect();
                self.game.debug_set_terrain_batch(&rules, &batch);
                Some(picked.to_vec())
            }
            Condition::TilePlot(PlotFilter::River) => {
                let (a, b) = (self.at(9, 8), self.at(10, 8));
                self.game.debug_set_river_edges(&rules, &[(a, b)]);
                Some(vec![a, b])
            }
            Condition::TilePlot(PlotFilter::Sea) => Some(self.sea()),
            Condition::TilePlot(PlotFilter::Lake) => {
                let lake = self.at(6, 8);
                self.game.debug_set_terrain_batch(&rules, &[(lake, TerrainId(water_code(&rules)))]);
                Some(vec![lake])
            }
            Condition::TilePlot(PlotFilter::SeaResource) => {
                let sea = self.sea();
                let resource = (0..rules.resources.len())
                    .map(|i| classic_rules::ids::ResourceId(i as u16))
                    .find(|r| rules.resources[r.0 as usize].class.is_some())?;
                self.reveal(resource);
                let wet = [sea[0], sea[3]];
                let batch: Vec<_> = wet.iter().map(|t| (*t, resource)).collect();
                self.game.debug_set_resource_batch(&rules, &batch);
                Some(wet.to_vec())
            }
            _ => None,
        }
    }
}

/// Every source's effect sets, with their keys: buildings, technologies,
/// nations, policies, beliefs.
fn sources(rules: &CompiledRuleset) -> Vec<(String, &EffectSet)> {
    let mut sets: Vec<(String, &EffectSet)> = Vec::new();
    sets.extend(rules.buildings.iter().map(|d| (d.key.clone(), &d.effects)));
    sets.extend(rules.technologies.iter().map(|d| (d.key.clone(), &d.effects)));
    sets.extend(rules.nations.iter().map(|d| (d.key.clone(), &d.effects)));
    sets.extend(rules.module.policies.iter().map(|d| (d.key.clone(), &d.effects)));
    sets.extend(rules.module.beliefs.iter().map(|d| (d.key.clone(), &d.effects)));
    sets
}

/// The kind of a tile condition, its value left out (River and Sea plots
/// are different kinds).
fn kind_of(condition: Condition) -> String {
    match condition {
        Condition::TilePlot(plot) => format!("TilePlot({plot:?})"),
        other => format!("{other:?}").split('(').next().unwrap_or_default().to_owned(),
    }
}

/// The first building (by index) for each kind of tile condition its flat
/// Tile-scope yields use.
fn tile_condition_buildings(rules: &CompiledRuleset) -> Vec<(BuildingKindId, Condition)> {
    let mut seen: Vec<String> = Vec::new();
    let mut picked = Vec::new();
    for (index, def) in rules.buildings.iter().enumerate() {
        for effect in &def.effects.passive_tile {
            if !matches!(effect.modifier, Modifier::AddYield { .. }) || effect.condition == Condition::Always {
                continue;
            }
            let kind = kind_of(effect.condition);
            if !seen.contains(&kind) {
                seen.push(kind);
                picked.push((BuildingKindId(index as u16), effect.condition));
            }
        }
    }
    picked
}

/// What the building's flat Tile-scope yields add on `tile` by
/// `effect_meets`.
fn ours(scene: &Scene, effects: &[CompiledEffect], tile: TileId, ctx: &TerritoryContext<'_>) -> Yields {
    let view = scene.game.player_view(&scene.rules, P0).revealed();
    let mut yields = Yields::ZERO;
    for effect in effects {
        if let Modifier::AddYield { yield_kind, amount } = effect.modifier {
            if effect_meets(&view, tile, effect, ctx) == Some(true) {
                yields.add(yield_kind, amount);
            }
        }
    }
    yields
}

/// The engine's flat tile bonus of the city on `tile`, by yield index.
fn engine(scene: &Scene, city: CityHandle, tile: TileId) -> Vec<i32> {
    let auth = scene.game.view().auth;
    let core = auth.cities().get(city).unwrap();
    city_tile_effects(auth, &scene.rules, core).bonus(auth, &scene.rules, core, tile).flat.to_vec()
}

#[test]
fn every_kind_of_building_tile_condition_counts_the_matching_tiles_as_the_engine_does() {
    let rules = compile_rules(core_rules()).unwrap();
    let picked = tile_condition_buildings(&rules);
    let kinds: Vec<String> = picked.iter().map(|(_, c)| kind_of(*c)).collect();
    assert!(kinds.len() >= 5, "the rules' buildings use several kinds of tile conditions: {kinds:?}");
    let mut checked = 0;
    for (building, condition) in picked {
        let but = match condition {
            Condition::TileTerrain(kind) => rules.terrains[kind.0 as usize].map_code,
            _ => None,
        };
        let mut scene = Scene::new(but);
        let Some(expected) = scene.arrange(condition) else { continue };
        let city = scene.found();
        let before: Vec<Vec<i32>> = hex_disc(scene.game.view().auth.map().dimensions, scene.centre, RADIUS).map(|t| engine(&scene, city, t)).collect();
        scene.game.debug_add_building(&scene.rules, city, building);

        let view = scene.game.player_view(&scene.rules, P0).revealed();
        let dims = view.dimensions();
        let area = territory(&view, scene.centre, RADIUS, P0);
        assert_eq!(area.len(), 37, "{condition:?}: the whole disc is ours or nobody's");
        let core = scene.game.view().auth.cities().get(city).unwrap().clone();
        let techs = scene.game.view().auth.players().techs_of(P0).to_vec();
        let ctx = TerritoryContext {
            rules: &scene.rules,
            player: P0,
            techs: &techs,
            centre: scene.centre,
            population: core.population,
            buildings: &core.buildings,
        };
        let inside: Vec<TileId> = expected.iter().copied().filter(|t| dims.distance(*t, scene.centre).is_some_and(|d| d <= RADIUS)).collect();
        assert!(!inside.is_empty(), "{condition:?}: the scene puts matching tiles in the radius");

        // The count of the picked condition: exactly the scene's tiles.
        let bonus = building_tile_bonus(&view, &scene.rules, building, &area, &ctx);
        let mut sum = Yields::ZERO;
        for count in bonus.tile_effects.iter().filter(|c| c.effect.condition == condition) {
            assert_eq!(count.met as usize, inside.len(), "{} {condition:?}", scene.rules.buildings[building.0 as usize].key);
            assert_eq!(count.undecided, 0);
            if let Modifier::AddYield { yield_kind, amount } = count.effect.modifier {
                sum.add(yield_kind, amount * inside.len() as i32);
            }
        }
        for tile in &area {
            let meets = tile_meets(&view, *tile, condition, &ctx);
            assert_eq!(meets, Some(inside.contains(tile)), "{condition:?} on {tile:?}");
        }
        assert_ne!(sum, Yields::ZERO, "{condition:?}: the picked effects add a yield");

        // Per tile, what the building's tile effects add is the engine's.
        let effects = scene.rules.buildings[building.0 as usize].effects.passive_tile.clone();
        for (tile, before) in hex_disc(dims, scene.centre, RADIUS).zip(before) {
            let after = engine(&scene, city, tile);
            let ours = ours(&scene, &effects, tile, &ctx);
            for kind in YieldKind::all() {
                let i = kind.index();
                assert_eq!(after[i] - before[i], ours.of(kind), "{condition:?} {kind:?} on {tile:?}");
            }
        }
        checked += 1;
    }
    assert!(checked >= 5, "kinds checked: {checked} of {kinds:?}");
}

/// The first effect of any source whose condition is `condition`.
fn effect_with(rules: &CompiledRuleset, condition: Condition) -> (String, CompiledEffect) {
    sources(rules)
        .into_iter()
        .find_map(|(key, set)| {
            let all = set.passive_city.iter().chain(&set.passive_tile).chain(&set.passive_player);
            all.copied().find(|e| e.condition == condition).map(|e| (key, e))
        })
        .unwrap_or_else(|| panic!("no source with {condition:?}"))
}

#[test]
fn city_conditions_follow_the_centre_coast_and_river() {
    let rules = compile_rules(core_rules()).unwrap();
    for condition in [Condition::CityCoastal, Condition::CityOnRiver] {
        let (key, effect) = effect_with(&rules, condition);
        for (met, scene) in [(true, true), (false, false)] {
            let mut s = Scene::new(None);
            if scene {
                match condition {
                    Condition::CityCoastal => {
                        let sea = s.sea();
                        assert!(sea.contains(&s.at(9, 8)), "the sea touches the centre");
                    }
                    _ => {
                        let (a, b) = (s.centre, s.at(9, 8));
                        s.game.debug_set_river_edges(&s.rules, &[(a, b)]);
                    }
                }
            } else if condition == Condition::CityCoastal {
                // Water next to the centre that is only a lake is not a coast.
                let lake = s.at(7, 8);
                s.game.debug_set_terrain_batch(&s.rules, &[(lake, TerrainId(water_code(&s.rules)))]);
            }
            let view = s.game.player_view(&s.rules, P0).revealed();
            let ctx = TerritoryContext { rules: &s.rules, player: P0, techs: &[], centre: s.centre, population: 1, buildings: &[] };
            let area = territory(&view, s.centre, RADIUS, P0);
            // A city condition reads the centre whatever the tile.
            for tile in &area {
                assert_eq!(tile_meets(&view, *tile, condition, &ctx), Some(met), "{key} {condition:?}");
            }
            assert_eq!(effect_meets(&view, s.centre, &effect, &ctx), Some(met), "{key}");
            if let Modifier::AddYield { yield_kind, amount } = effect.modifier {
                let mut set = EffectSet::default();
                set.passive_city.push(effect);
                let bonus = effect_set_bonus(&view, &set, &area, &ctx);
                assert_eq!(bonus.city_effects[0].met, u32::from(met), "{key}");
                let mut expected = Yields::ZERO;
                if met {
                    expected.add(yield_kind, amount);
                }
                assert_eq!(bonus.city, expected, "{key}");
            }
        }
    }
}

#[test]
fn the_territory_leaves_out_other_players_tiles_and_unknown_tiles() {
    let mut s = Scene::new(None);
    let theirs = s.at(12, 8);
    s.game.debug_found_city(&s.rules, P1, theirs);
    let auth = s.game.view().auth;
    let dims = auth.map().dimensions;
    let foreign = hex_disc(dims, s.centre, RADIUS).filter(|t| auth.map().owner(*t).flatten() == Some(P1)).count();
    assert!(foreign > 0, "the other city's borders reach the disc");
    let view = s.game.player_view(&s.rules, P0).revealed();
    let area = territory(&view, s.centre, RADIUS, P0);
    assert_eq!(area.len(), 37 - foreign);
    assert!(area.iter().all(|t| auth.map().owner(*t).flatten() != Some(P1)));
    // Without the open map the player knows nothing yet.
    let blind = s.game.player_view(&s.rules, P0);
    assert!(territory(&blind, s.centre, RADIUS, P0).is_empty());
}

#[test]
fn the_site_features_fit_the_tables_and_read_the_scene() {
    let mut s = Scene::new(None);
    let mut names = SITE_NAMES.to_vec();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), SITE_FEATURES, "the names are unique");
    assert!(s.rules.terrains.len() <= TERRAIN_SLOTS, "every terrain kind has its slot");
    assert!(s.rules.features.len() <= FEATURE_SLOTS, "every feature has its slot");

    let (a, b) = (s.centre, s.at(9, 8));
    s.game.debug_set_river_edges(&s.rules, &[(a, b)]);
    let view = s.game.player_view(&s.rules, P0).revealed();
    let own = [s.at(14, 8)];
    let inputs = SiteInputs { nation: 0, techs: &[], own_cities: &own, foreign_cities: &[], round: 42, excess_happiness: 3 };
    let features = site_features(&view, &s.rules, P0, s.centre, &inputs);
    let get = |name: &str| features[SITE_NAMES.iter().position(|n| *n == name).unwrap()];
    assert_eq!(get("territory_tiles"), 37);
    assert_eq!(get("unknown_tiles"), 0);
    assert_eq!(get("centre_river"), 1);
    assert_eq!(get("centre_fresh_water"), 1);
    assert_eq!(get("centre_coastal"), 0);
    assert_eq!(get("river_tiles"), 2);
    assert_eq!(get("own_city_distance"), 6);
    assert_eq!(get("own_cities"), 1);
    assert_eq!(get("round"), 42);
    assert_eq!(get("excess_happiness"), 3);
    assert_eq!(get("nature_r0_food"), nature_yields(&view, &s.rules, s.centre).food);
    let ring1: i32 = hex_disc(view.dimensions(), s.centre, 1).filter(|t| *t != s.centre).map(|t| nature_yields(&view, &s.rules, t).food).sum();
    assert_eq!(get("nature_r1_food"), ring1);
    // One terrain over the whole disc, and the best improvements add to it.
    let terrains: i32 = (0..TERRAIN_SLOTS).map(|i| get(&format!("terrain_{i:02}"))).sum();
    assert_eq!(terrains, 37);
    let total = |prefix: &str| -> i32 {
        (0..4).flat_map(|r| ["food", "production", "gold", "science", "culture", "faith"].map(move |c| (r, c))).map(|(r, c)| get(&format!("{prefix}_r{r}_{c}"))).sum()
    };
    assert!(total("potential") > total("nature"), "improvements add yields");
    assert_eq!(get("heuristic_legal"), 1, "the site is legal");
    assert_eq!(site_features(&view, &s.rules, P0, s.centre, &inputs), features, "deterministic");
}
