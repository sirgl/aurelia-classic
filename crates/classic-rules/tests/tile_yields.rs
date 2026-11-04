//! Tile yields of the classic ruleset against the rules written out by
//! hand (classic/docs/classic/terrain.json, features.json, resources.json,
//! improvements.json): the oracle is this table, the code under test is
//! `derived::tile_yields` (and the citizen assignment that reads it).

use classic_content::core::core_rules;
use classic_rules::derived::tile_yields::{plot_yields, tile_yields_for};
use classic_rules::ids::{FeatureId, ImprovementId, PlayerId, ResourceId, TechId, TerrainId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::Yields;
use classic_rules::world::facts::TruthFacts;
use classic_rules::{compile_rules, CompiledRuleset, GameConfig, GameState};

const PLAINS: TerrainId = TerrainId(0);
const GRASS: TerrainId = TerrainId(1);
const HILLS: TerrainId = TerrainId(2);
const DESERT: TerrainId = TerrainId(3);
const WATER: TerrainId = TerrainId(4);
const SNOW: TerrainId = TerrainId(5);
const MOUNTAIN: TerrainId = TerrainId(6);
const P0: PlayerId = PlayerId(0);

fn y(food: i32, production: i32, gold: i32) -> Yields {
    Yields {
        food,
        production,
        gold,
        ..Yields::default()
    }
}

struct World {
    game: GameState,
    rules: CompiledRuleset,
}

impl World {
    /// A 24×24 map of desert (nothing) with one player.
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(
            &rules,
            GameConfig {
                players: 1,
                width: 24,
                height: 24,
                rng_seed: 5,
            },
        );
        let all: Vec<_> = (0..24 * 24).map(|i| (TileId(i), DESERT)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        World { game, rules }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        let dims = self.game.view().auth.map().dimensions;
        dims.to_tile(HexCoord { q, r }).unwrap()
    }

    fn terrain(&mut self, t: TileId, terrain: TerrainId) {
        self.game
            .debug_set_terrain_batch(&self.rules, &[(t, terrain)]);
    }

    fn feature(&mut self, t: TileId, key: &str) {
        let f = FeatureId(
            self.rules
                .features
                .iter()
                .position(|f| f.key == key)
                .unwrap() as u16,
        );
        self.game.debug_set_feature_batch(&self.rules, &[(t, f)]);
    }

    fn resource(&mut self, t: TileId, key: &str) {
        let r = ResourceId(
            self.rules
                .resources
                .iter()
                .position(|r| r.key == key)
                .unwrap() as u16,
        );
        self.game.debug_set_resource_batch(&self.rules, &[(t, r)]);
    }

    fn improvement(&mut self, t: TileId, key: &str) {
        let i = self.improvement_id(key);
        self.game
            .debug_set_improvement_batch(&self.rules, &[(t, i)]);
    }

    fn improvement_id(&self, key: &str) -> ImprovementId {
        ImprovementId(
            self.rules
                .improvements
                .iter()
                .position(|i| i.key == key)
                .unwrap() as u16,
        )
    }

    fn learn(&mut self, key: &str) {
        let t = TechId(
            self.rules
                .technologies
                .iter()
                .position(|t| t.key == key)
                .unwrap() as u16,
        );
        self.game.debug_learn(&self.rules, P0, t);
    }

    /// Plot yields as player 0 knows them, clamped at 0.
    fn plot(&self, t: TileId) -> Yields {
        let facts = TruthFacts::new(self.game.view().auth, &self.rules, P0);
        plot_yields(&facts, &self.rules, t, &facts.techs).at_least(0)
    }
}

#[test]
fn bare_terrain_yields_are_the_classic_table() {
    let mut w = World::new();
    let cases = [
        (PLAINS, y(1, 1, 0)),
        (GRASS, y(2, 0, 0)),
        (HILLS, y(0, 2, 0)),
        (DESERT, y(0, 0, 0)),
        (SNOW, y(0, 0, 0)),
        (MOUNTAIN, y(0, 0, 0)),
    ];
    for (i, (terrain, expect)) in cases.into_iter().enumerate() {
        let t = w.tile(2 + 3 * i as i32, 2);
        w.terrain(t, terrain);
        assert_eq!(w.plot(t), expect, "{terrain:?}");
    }
}

#[test]
fn water_gives_food_and_gold_lakes_one_more_food() {
    let mut w = World::new();
    // A one-tile lake and a 12-tile sea (two rows).
    let lake = w.tile(3, 3);
    w.terrain(lake, WATER);
    assert!(w.game.view().auth.map().lake(lake));
    assert_eq!(w.plot(lake), y(2, 0, 1), "lake: coast 1F 1G + lake 1F");
    let sea: Vec<_> = (0..6)
        .flat_map(|q| [w.tile(10 + q, 10), w.tile(10 + q, 11)])
        .collect();
    for t in &sea {
        w.terrain(*t, WATER);
    }
    assert!(!w.game.view().auth.map().lake(sea[0]));
    assert_eq!(w.plot(sea[0]), y(1, 0, 1), "coast: 1F 1G (Unciv Terrains.json: Coast and Ocean 1 food 1 gold)");
}

#[test]
fn features_replace_or_add_to_the_terrain() {
    let mut w = World::new();
    let forest = w.tile(2, 2);
    w.terrain(forest, GRASS);
    w.feature(forest, "core:feature/forest");
    assert_eq!(w.plot(forest), y(1, 1, 0), "forest replaces grassland");
    let marsh = w.tile(5, 2);
    w.terrain(marsh, GRASS);
    w.feature(marsh, "core:feature/marsh");
    assert_eq!(w.plot(marsh), y(1, 0, 0), "marsh: grassland 2F − 1F");
    let oasis = w.tile(8, 2);
    w.feature(oasis, "core:feature/oasis");
    assert_eq!(w.plot(oasis), y(3, 0, 1), "oasis on desert");
    let flood = w.tile(11, 2);
    w.feature(flood, "core:feature/flood-plains");
    assert_eq!(w.plot(flood), y(2, 0, 0), "flood plains on desert");
}

#[test]
fn rivers_give_no_gold_but_fresh_water() {
    let mut w = World::new();
    let a = w.tile(4, 4);
    let b = w.tile(5, 4);
    w.terrain(a, GRASS);
    w.terrain(b, GRASS);
    w.game.debug_set_river_edges(&w.rules, &[(a, b)]);
    assert_eq!(w.plot(a), y(2, 0, 0), "BNW: no river gold");
    let facts = TruthFacts::new(w.game.view().auth, &w.rules, P0);
    assert!(classic_rules::derived::tile_yields::fresh_water(
        &facts, &w.rules, a
    ));
    let dry = w.tile(9, 9);
    assert!(!classic_rules::derived::tile_yields::fresh_water(
        &facts, &w.rules, dry
    ));
}

#[test]
fn resources_count_once_revealed_and_improvements_add_their_part() {
    let mut w = World::new();
    // Wheat on plains: revealed from the start (+1F), a farm +1F.
    let wheat = w.tile(2, 2);
    w.terrain(wheat, PLAINS);
    w.resource(wheat, "core:resource/wheat");
    assert_eq!(w.plot(wheat), y(2, 1, 0));
    w.improvement(wheat, "core:improvement/farm");
    assert_eq!(w.plot(wheat), y(3, 1, 0));
    // Iron on hills: hidden until Iron Working.
    let iron = w.tile(6, 2);
    w.terrain(iron, HILLS);
    w.resource(iron, "core:resource/iron");
    assert_eq!(w.plot(iron), y(0, 2, 0), "iron hidden");
    w.learn("core:tech/iron-working");
    assert_eq!(w.plot(iron), y(0, 3, 0), "iron revealed: +1P");
    w.improvement(iron, "core:improvement/mine");
    assert_eq!(w.plot(iron), y(0, 4, 0), "mine on iron: +1P");
    // Silk (forest luxury): +2G, plantation +1 improvement gold.
    let silk = w.tile(10, 2);
    w.terrain(silk, GRASS);
    w.feature(silk, "core:feature/forest");
    w.resource(silk, "core:resource/silk");
    w.improvement(silk, "core:improvement/plantation");
    assert_eq!(
        w.plot(silk),
        y(1, 1, 3),
        "forest 1F1P + silk 2G + plantation 1G"
    );
}

#[test]
fn farm_technology_yields_follow_fresh_water() {
    let mut w = World::new();
    let wet = w.tile(3, 3);
    let wet_other = w.tile(4, 3);
    let dry = w.tile(10, 10);
    for t in [wet, wet_other, dry] {
        w.terrain(t, GRASS);
    }
    w.game.debug_set_river_edges(&w.rules, &[(wet, wet_other)]);
    w.improvement(wet, "core:improvement/farm");
    w.improvement(dry, "core:improvement/farm");
    assert_eq!(w.plot(wet), y(3, 0, 0));
    assert_eq!(w.plot(dry), y(3, 0, 0));
    w.learn("core:tech/civil-service");
    assert_eq!(
        w.plot(wet),
        y(4, 0, 0),
        "Civil Service: +1F with fresh water"
    );
    assert_eq!(w.plot(dry), y(3, 0, 0));
    w.learn("core:tech/fertilizer");
    assert_eq!(
        w.plot(dry),
        y(4, 0, 0),
        "Fertilizer: +1F without fresh water"
    );
    assert_eq!(w.plot(wet), y(4, 0, 0));
}

#[test]
fn city_centre_is_at_least_two_food_one_production() {
    let mut w = World::new();
    let centre = w.tile(8, 8);
    let city = w.game.debug_found_city(&w.rules, P0, centre);
    let auth = w.game.view().auth;
    assert_eq!(
        tile_yields_for(auth, &w.rules, centre, P0),
        y(2, 1, 0),
        "desert centre"
    );
    // A size-1 city on desert: centre 2F 1P; its citizen is better off
    // unemployed (+1P) than on a barren desert tile; science 1; the first
    // city gets the Palace (+3P, +3 science).
    let rates = w.game.derived().yields.of(city).unwrap();
    assert_eq!((rates.food, rates.production, rates.science), (2, 5, 4));
}

#[test]
fn technology_tile_effects_reach_improvements_of_the_city() {
    // Economics: trading posts +1 gold (a Tile effect of the technology,
    // applied to the tiles the owner's cities work).
    let mut w = World::new();
    let centre = w.tile(8, 8);
    let post = w.tile(9, 8);
    w.terrain(post, PLAINS);
    w.improvement(post, "core:improvement/trading-post");
    let city = w.game.debug_found_city(&w.rules, P0, centre);
    let gold = |w: &World| w.game.derived().yields.of(city).unwrap().gold;
    // The Palace of the first city adds 3 gold.
    assert_eq!(gold(&w), 1 + 3, "trading post 1G");
    w.learn("core:tech/economics");
    assert_eq!(gold(&w), 2 + 3, "Economics: +1G");
    w.game.validate(&w.rules).unwrap();
}
