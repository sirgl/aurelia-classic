//! Tile conditions on the classic ruleset (`Condition::TilePlot`,
//! `TileResourceClass`, `TileFeatureUnimproved`,
//! `CityAdjacentUnimprovedFeature`) and the Tile-scope percent stage:
//! river, sea, lake and sea resource yields of buildings, One with Nature,
//! Spain's natural wonders, Japan's atolls, Russia's strategic resources,
//! the Celts' forests. The oracle is the yield of the worked tile.

use classic_rules::ClassicGame;
use classic_rules::classic::effects::ClassicYield;
use classic_content::core::core_rules;
use classic_rules::derived::citizens::{worked_tile_yields, EmpireContext};
use classic_rules::effects::city::{city_modifiers, city_tile_effects};
use classic_rules::ids::{
    BeliefId, BuildingKindId, CityHandle, FeatureId, ImprovementId, PlayerId, ResourceId, TechId,
    TerrainId, TileId,
};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::{NaturalWonderId, Yields};
use classic_rules::rules::effects::YieldKind;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const GRASS: TerrainId = TerrainId(1);
const WATER: TerrainId = TerrainId(4);
const P0: PlayerId = PlayerId(0);

struct World {
    game: GameState,
    rules: CompiledRuleset,
    city: CityHandle,
}

impl World {
    /// A 24×24 grassland map, one player, a city at (8, 8).
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 2, width: 24, height: 24, rng_seed: 5 });
        let all: Vec<_> = (0..24 * 24).map(|i| (TileId(i), GRASS)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let dims = game.view().auth.map().dimensions;
        let at = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
        let city = game.debug_found_city(&rules, P0, at);
        World { game, rules, city }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        self.game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
    }

    fn nation(&mut self, key: &str) {
        let nation = self.rules.nations.iter().position(|c| c.key == key).unwrap();
        self.game.debug_set_player_nation(P0, nation as u8);
    }

    fn building(&mut self, key: &str) {
        let b = BuildingKindId(self.rules.buildings.iter().position(|b| b.key == key).unwrap() as u16);
        self.game.debug_add_building(&self.rules, self.city, b);
    }

    fn feature(&mut self, t: TileId, key: &str) {
        let f = FeatureId(self.rules.features.iter().position(|f| f.key == key).unwrap() as u16);
        self.game.debug_set_feature_batch(&self.rules, &[(t, f)]);
    }

    fn resource(&mut self, t: TileId, key: &str) {
        let r = ResourceId(self.rules.resources.iter().position(|r| r.key == key).unwrap() as u16);
        self.game.debug_set_resource_batch(&self.rules, &[(t, r)]);
    }

    fn improvement(&mut self, t: TileId, key: &str) {
        let i = ImprovementId(self.rules.improvements.iter().position(|i| i.key == key).unwrap() as u16);
        self.game.debug_set_improvement_batch(&self.rules, &[(t, i)]);
    }

    fn learn(&mut self, key: &str) {
        let t = TechId(self.rules.technologies.iter().position(|t| t.key == key).unwrap() as u16);
        self.game.debug_learn(&self.rules, P0, t);
    }

    fn wonder(&mut self, t: TileId) {
        self.game.debug_set_natural_wonder_batch(&self.rules, &[(t, NaturalWonderId(0))]);
    }

    /// The yields of `t` as the city works it.
    fn worked(&self, t: TileId) -> Yields {
        let auth = self.game.view().auth;
        let core = auth.cities().get(self.city).unwrap();
        let effects = city_tile_effects(auth, &self.rules, core);
        worked_tile_yields(auth, &self.rules, core, t, &effects, &EmpireContext::of(auth, P0))
    }

    fn city_faith(&self) -> i32 {
        let auth = self.game.view().auth;
        city_modifiers(auth, &self.rules, auth.cities().get(self.city).unwrap()).yields[YieldKind::Module(ClassicYield::Faith).index()].flat
    }
}

#[test]
fn hydro_plant_adds_production_on_river_tiles_only() {
    let mut w = World::new();
    let (river, dry) = (w.tile(9, 8), w.tile(7, 9));
    w.game.debug_set_river_edges(&w.rules, &[(river, w.tile(10, 8))]);
    let before = (w.worked(river).production, w.worked(dry).production);
    w.building("core:building/hydro_plant");
    assert_eq!(w.worked(river).production, before.0 + 1, "river tile");
    assert_eq!(w.worked(dry).production, before.1, "dry tile");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn lighthouse_and_seaport_reach_sea_and_sea_resource_tiles_not_lakes() {
    let mut w = World::new();
    // A sea of 12 tiles east of the city, a one-tile lake west of it.
    let sea: Vec<_> = (0..6).flat_map(|q| [(w.tile(9 + q, 8), WATER), (w.tile(9 + q, 7), WATER)]).collect();
    w.game.debug_set_terrain_batch(&w.rules, &sea);
    let lake = w.tile(6, 8);
    w.game.debug_set_terrain_batch(&w.rules, &[(lake, WATER)]);
    let (open, fish) = (w.tile(10, 8), w.tile(9, 8));
    w.resource(fish, "core:resource/fish");
    let (open0, fish0, lake0) = (w.worked(open), w.worked(fish), w.worked(lake));
    w.building("core:building/lighthouse");
    assert_eq!(w.worked(open).food, open0.food + 1, "sea tile +1 food");
    assert_eq!(w.worked(lake), lake0, "a lake is not sea");
    // Fish: +1 food (sea), +1 food (fish), +1 production (sea resource).
    assert_eq!(w.worked(fish).food, fish0.food + 2);
    assert_eq!(w.worked(fish).production, fish0.production + 1);
    w.building("core:building/seaport");
    assert_eq!(w.worked(fish).production, fish0.production + 2, "seaport +1 production");
    assert_eq!(w.worked(fish).gold, fish0.gold + 1, "seaport +1 gold");
    assert_eq!(w.worked(open).gold, open0.gold, "no resource, no seaport gold");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn floating_gardens_add_two_food_on_lakes() {
    let mut w = World::new();
    let lake = w.tile(7, 8);
    w.game.debug_set_terrain_batch(&w.rules, &[(lake, WATER)]);
    let before = w.worked(lake).food;
    w.building("core:building/floating_gardens");
    assert_eq!(w.worked(lake).food, before + 2);
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn one_with_nature_gives_four_faith_on_natural_wonders_of_following_cities() {
    let mut w = World::new();
    let site = w.tile(9, 9);
    w.wonder(site);
    let before = w.worked(site).module;
    let belief = BeliefId(
        w.rules.module.beliefs.iter().position(|b| b.key == "core:belief/pantheon/one-with-nature").unwrap() as u16,
    );
    w.game.debug_grant_faith(P0, 100);
    w.game.execute(&w.rules, Command::FoundPantheon { player: P0, belief }).unwrap();
    assert_eq!(w.worked(site).module, before + 4);
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn spain_doubles_the_yields_of_natural_wonder_tiles() {
    let mut w = World::new();
    let site = w.tile(9, 9);
    w.wonder(site);
    let plain = w.worked(site);
    assert!(!plain.is_zero(), "the wonder yields something");
    w.nation("core:nation/spain");
    w.game.debug_learn(&w.rules, P0, TechId(0));
    let doubled = w.worked(site);
    assert_eq!(doubled, plain.plus(plain), "+100% of every yield");
    let grass = w.tile(7, 9);
    assert_eq!(w.worked(grass).food, 2, "other tiles keep theirs");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn japan_gets_culture_from_unimproved_atolls() {
    let mut w = World::new();
    let atoll = w.tile(9, 8);
    w.game.debug_set_terrain_batch(&w.rules, &[(atoll, WATER)]);
    w.feature(atoll, "core:feature/atoll");
    let before = w.worked(atoll).culture;
    w.nation("core:nation/japan");
    w.game.debug_learn(&w.rules, P0, TechId(0));
    assert_eq!(w.worked(atoll).culture, before + 2, "unimproved atoll");
    w.improvement(atoll, "core:improvement/fishing-boats");
    // Fishing boats: +1 culture of Japan's other trait, no atoll bonus.
    assert_eq!(w.worked(atoll).culture, before + 1, "improved atoll");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn russia_gets_production_from_seen_strategic_resources() {
    let mut w = World::new();
    let iron = w.tile(9, 8);
    w.resource(iron, "core:resource/iron");
    let unseen = w.worked(iron).production;
    w.nation("core:nation/russia");
    w.game.debug_learn(&w.rules, P0, TechId(0));
    assert_eq!(w.worked(iron).production, unseen, "iron not seen yet");
    w.nation("core:nation/america");
    w.learn("core:tech/iron-working");
    let seen = w.worked(iron).production;
    w.nation("core:nation/russia");
    w.learn("core:tech/mining");
    assert_eq!(w.worked(iron).production, seen + 1, "+1 production once iron is seen");
    let wheat = w.tile(7, 9);
    w.resource(wheat, "core:resource/wheat");
    assert_eq!(w.worked(wheat).production, 0, "a bonus resource is not strategic");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn celts_get_faith_from_unimproved_forests_next_to_a_city() {
    let mut w = World::new();
    w.nation("core:nation/celts");
    w.game.debug_learn(&w.rules, P0, TechId(0));
    assert_eq!(w.city_faith(), 0);
    let forests = [w.tile(9, 8), w.tile(7, 8), w.tile(8, 9)];
    w.feature(forests[0], "core:feature/forest");
    assert_eq!(w.city_faith(), 1, "one unimproved forest");
    w.feature(forests[1], "core:feature/forest");
    assert_eq!(w.city_faith(), 1, "two");
    w.feature(forests[2], "core:feature/forest");
    assert_eq!(w.city_faith(), 2, "three or more");
    w.improvement(forests[2], "core:improvement/lumber-mill");
    assert_eq!(w.city_faith(), 1, "an improved forest does not count");
    let far = w.tile(11, 8);
    w.feature(far, "core:feature/forest");
    assert_eq!(w.city_faith(), 1, "only tiles next to the city centre");
    w.game.validate(&w.rules).unwrap();
}
