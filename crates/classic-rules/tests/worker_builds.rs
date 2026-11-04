//! Worker builds of the classic ruleset: where improvements may stand,
//! feature removal with chop production, roads behind The Wheel, work boats
//! (classic/docs/classic/improvements.json, features.json; improvement rules and
//! feature removal output; rules.md).

use classic_content::core::core_rules;
use classic_rules::ids::{
    CityHandle, FeatureId, ImprovementId, PlayerId, ResourceId, TechId, TerrainId, TileId,
    UnitHandle, UnitKindId,
};
use classic_rules::math::HexCoord;
use classic_rules::world::builds::{improvement_site, removal_site};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const PLAINS: TerrainId = TerrainId(0);
const GRASS: TerrainId = TerrainId(1);
const HILLS: TerrainId = TerrainId(2);
const DESERT: TerrainId = TerrainId(3);
const WATER: TerrainId = TerrainId(4);
const MOUNTAIN: TerrainId = TerrainId(6);
const P0: PlayerId = PlayerId(0);

struct World {
    game: GameState,
    rules: CompiledRuleset,
    city: CityHandle,
}

impl World {
    /// Plains everywhere, a city of player 0 at (8,8) owning ring 1.
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(
            &rules,
            GameConfig {
                players: 2,
                width: 24,
                height: 24,
                rng_seed: 5,
            },
        );
        let all: Vec<_> = (0..24 * 24).map(|i| (TileId(i), PLAINS)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let dims = game.view().auth.map().dimensions;
        let centre = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
        let city = game.debug_found_city(&rules, P0, centre);
        World { game, rules, city }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        let dims = self.game.view().auth.map().dimensions;
        dims.to_tile(HexCoord { q, r }).unwrap()
    }

    fn improvement(&self, key: &str) -> ImprovementId {
        ImprovementId(
            self.rules
                .improvements
                .iter()
                .position(|i| i.key == key)
                .unwrap() as u16,
        )
    }

    fn unit(&self, key: &str) -> UnitKindId {
        UnitKindId(self.rules.units.iter().position(|u| u.key == key).unwrap() as u16)
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

    fn feature(&mut self, t: TileId, key: &str) -> FeatureId {
        let f = FeatureId(
            self.rules
                .features
                .iter()
                .position(|f| f.key == key)
                .unwrap() as u16,
        );
        self.game.debug_set_feature_batch(&self.rules, &[(t, f)]);
        f
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

    fn site(
        &self,
        t: TileId,
        key: &str,
    ) -> Result<classic_rules::world::builds::BuildPlan, &'static str> {
        improvement_site(
            self.game.view().auth,
            &self.rules,
            P0,
            t,
            self.improvement(key),
        )
    }

    fn worker(&mut self, t: TileId) -> UnitHandle {
        let kind = self.unit("core:unit/worker");
        self.game.debug_spawn_unit(&self.rules, P0, t, kind)
    }

    fn end_turns(&mut self, n: usize) {
        for _ in 0..n {
            // Player 1 has nothing: it is out, player 0 plays every round.
            self.game
                .execute(&self.rules, Command::EndTurn { player: P0 })
                .unwrap();
        }
    }
}

#[test]
fn mines_need_hills_farms_need_flatland_or_fresh_water() {
    let mut w = World::new();
    w.learn("core:tech/mining");
    w.learn("core:tech/agriculture");
    let t = w.tile(9, 8);
    assert_eq!(w.site(t, "core:improvement/mine"), Err("wrong terrain"));
    assert!(w.site(t, "core:improvement/farm").is_ok());
    w.game.debug_set_terrain_batch(&w.rules, &[(t, HILLS)]);
    assert!(w.site(t, "core:improvement/mine").is_ok());
    assert_eq!(
        w.site(t, "core:improvement/farm"),
        Err("hills need fresh water")
    );
    let other = w.tile(9, 7);
    w.game.debug_set_river_edges(&w.rules, &[(t, other)]);
    assert!(
        w.site(t, "core:improvement/farm").is_ok(),
        "fresh water makes a hill farm valid"
    );
}

#[test]
fn builds_need_their_technology_own_land_and_no_mountain() {
    let mut w = World::new();
    let t = w.tile(9, 8);
    assert_eq!(
        w.site(t, "core:improvement/farm"),
        Err("improvement locked")
    );
    w.learn("core:tech/agriculture");
    assert!(w.site(t, "core:improvement/farm").is_ok());
    let far = w.tile(15, 15);
    assert_eq!(w.site(far, "core:improvement/farm"), Err("not your land"));
    w.game.debug_set_terrain_batch(&w.rules, &[(t, MOUNTAIN)]);
    assert_eq!(w.site(t, "core:improvement/farm"), Err("wrong terrain"));
    let centre = w.tile(8, 8);
    assert_eq!(w.site(centre, "core:improvement/farm"), Err("city tile"));
}

#[test]
fn a_revealed_resource_allows_only_its_improvement() {
    let mut w = World::new();
    w.learn("core:tech/agriculture");
    w.learn("core:tech/animal-husbandry");
    let t = w.tile(9, 8);
    w.resource(t, "core:resource/horses");
    // Horses are hidden until Animal Husbandry — learnt here: revealed.
    assert_eq!(
        w.site(t, "core:improvement/farm"),
        Err("another resource is here")
    );
    assert!(
        w.site(t, "core:improvement/pasture").is_ok(),
        "pasture connects horses"
    );
    let plain = w.tile(7, 8);
    assert_eq!(
        w.site(plain, "core:improvement/pasture"),
        Err("wrong terrain"),
        "a pasture needs its resource"
    );
}

#[test]
fn a_farm_over_forest_removes_it_and_takes_longer() {
    let mut w = World::new();
    w.learn("core:tech/agriculture");
    let t = w.tile(9, 8);
    let forest = w.feature(t, "core:feature/forest");
    assert_eq!(
        w.site(t, "core:improvement/farm"),
        Err("removing the feature is locked")
    );
    w.learn("core:tech/mining");
    let plan = w.site(t, "core:improvement/farm").unwrap();
    assert_eq!(plan.removes, Some(forest));
    assert_eq!(plan.work_turns, 7 + 4, "farm 700 + forest removal 400");
    // A lumber mill keeps the forest; it needs one.
    w.learn("core:tech/construction");
    let mill = w.site(t, "core:improvement/lumber-mill").unwrap();
    assert_eq!(mill.removes, None);
    let open = w.tile(7, 8);
    assert_eq!(
        w.site(open, "core:improvement/lumber-mill"),
        Err("needs a feature")
    );
}

#[test]
fn removing_a_forest_grants_chop_production_to_its_city() {
    let mut w = World::new();
    let t = w.tile(9, 8);
    w.feature(t, "core:feature/forest");
    let worker = w.worker(t);
    assert!(
        w.game
            .execute(&w.rules, Command::ChopForest { unit: worker })
            .is_err(),
        "Mining needed"
    );
    w.learn("core:tech/mining");
    assert!(removal_site(w.game.view().auth, &w.rules, P0, t).is_ok());
    let before = w.game.view().auth.cities().get(w.city).unwrap().production;
    w.game
        .execute(&w.rules, Command::ChopForest { unit: worker })
        .unwrap();
    // Remove forest takes 4 worker turns.
    w.end_turns(3);
    assert!(
        w.game.view().auth.map().feature(t).unwrap().is_some(),
        "not done after 3 turns"
    );
    w.end_turns(1);
    assert_eq!(w.game.view().auth.map().feature(t).unwrap(), None);
    let after = w.game.view().auth.cities().get(w.city).unwrap().production;
    assert!(after >= before + 20, "the chop production reached the city");
    assert_eq!(
        classic_rules::world::builds::chop_production(w.game.view().auth, P0, t, 20),
        Some((w.city, 20)),
        "full 20 within two tiles"
    );
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn chop_production_falls_off_with_distance_and_needs_own_land() {
    let w = World::new();
    let auth = w.game.view().auth;
    let near = w.tile(9, 8);
    let unowned = w.tile(11, 8);
    assert_eq!(
        classic_rules::world::builds::chop_production(auth, P0, near, 20),
        Some((w.city, 20))
    );
    assert_eq!(
        classic_rules::world::builds::chop_production(auth, P0, unowned, 20),
        None,
        "no production off own land"
    );
}

#[test]
fn roads_need_the_wheel() {
    let mut w = World::new();
    let t = w.tile(9, 8);
    let worker = w.worker(t);
    assert!(w
        .game
        .execute(&w.rules, Command::BuildRoad { unit: worker })
        .is_err());
    w.learn("core:tech/the-wheel");
    w.game
        .execute(&w.rules, Command::BuildRoad { unit: worker })
        .unwrap();
}

#[test]
fn unique_improvements_belong_to_their_nation() {
    let mut w = World::new();
    w.learn("core:tech/chivalry");
    let t = w.tile(9, 8);
    let france = w
        .rules
        .nations
        .iter()
        .position(|c| c.key == "core:nation/france")
        .unwrap();
    assert_ne!(w.game.view().auth.players().nation_of(P0) as usize, france);
    assert_eq!(
        w.site(t, "core:improvement/chateau"),
        Err("unique to another nation")
    );
}

#[test]
fn work_boats_build_fishing_boats_at_once_and_are_used_up() {
    let mut w = World::new();
    w.learn("core:tech/sailing");
    let sea = w.tile(9, 8);
    w.game.debug_set_terrain_batch(&w.rules, &[(sea, WATER)]);
    w.resource(sea, "core:resource/fish");
    let boat_kind = w.unit("core:unit/work-boat");
    let boat = w.game.debug_spawn_unit(&w.rules, P0, sea, boat_kind);
    let boats = w.improvement("core:improvement/fishing-boats");
    let land_worker = w.worker(w.tile(7, 8));
    assert!(
        w.game
            .execute(
                &w.rules,
                Command::BuildImprovement {
                    unit: land_worker,
                    improvement: boats
                }
            )
            .is_err(),
        "a land worker cannot build on water"
    );
    w.game
        .execute(
            &w.rules,
            Command::BuildImprovement {
                unit: boat,
                improvement: boats,
            },
        )
        .unwrap();
    assert_eq!(
        w.game.view().auth.map().improvement(sea).unwrap(),
        Some(boats)
    );
    assert!(
        w.game.view().auth.units().get(boat).is_none(),
        "the work boat is used up"
    );
    let _ = DESERT;
    let _ = GRASS;
    w.game.validate(&w.rules).unwrap();
}

/// Regression: a worker could build the archaeological dig on an antiquity
/// site of its own land (the dig is a regular improvement with a work
/// time), skipping the archaeologist and the site's choice. The dig build
/// belongs to archaeologists only: an improvement that prompts its
/// builder when complete (`prompt_when_complete`) is the archaeology
/// system's, never a worker's.
#[test]
fn a_worker_does_not_dig_an_antiquity_site() {
    let mut w = World::new();
    w.learn("core:tech/archaeology");
    let t = w.tile(9, 8);
    let site = ResourceId(w.rules.resources.iter().position(|r| r.key == "core:resource/antiquity_site").unwrap() as u16);
    w.game.debug_set_resource_batch(&w.rules, &[(t, site)]);
    let unit = w.worker(t);
    let dig = w.improvement("core:improvement/archaeological-dig");
    let order = Command::BuildImprovement { unit, improvement: dig };
    assert!(w.game.execute(&w.rules, order).is_err(), "only archaeologists dig");
}
