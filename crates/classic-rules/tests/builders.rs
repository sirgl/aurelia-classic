//! Combat units that build (catalog `worker_actions` of the Legion and the
//! Samurai): the Legion builds roads and forts, the Samurai fishing boats
//! without being used up; neither builds a farm or chops a forest; a worker
//! keeps its builds. Each test checks the oracle (`GameState::validate`).

use classic_content::core::core_rules;
use classic_rules::ids::{ImprovementId, PlayerId, ResourceId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const GRASS: TerrainId = TerrainId(1);
const WATER: TerrainId = TerrainId(4);

struct World {
    game: GameState,
    rules: CompiledRuleset,
}

impl World {
    /// A 30x20 grassland map with the sea at (9, 8); P0's city at (8, 8).
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 4 });
        let all: Vec<_> = (0..600).map(|i| (TileId(i), GRASS)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let mut w = World { game, rules };
        let sea = w.tile(9, 8);
        w.game.debug_set_terrain_batch(&w.rules, &[(sea, WATER)]);
        let city = w.tile(8, 8);
        w.game.debug_found_city(&w.rules, P0, city);
        w
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        self.game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
    }

    fn unit(&mut self, key: &str, at: TileId) -> UnitHandle {
        let kind = UnitKindId(self.rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16);
        self.game.debug_spawn_unit(&self.rules, P0, at, kind)
    }

    fn improvement(&self, key: &str) -> ImprovementId {
        ImprovementId(self.rules.improvements.iter().position(|i| i.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
    }

    fn learn(&mut self, key: &str) {
        let t = self.rules.technologies.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("{key}"));
        self.game.debug_learn(&self.rules, P0, TechId(t as u16));
    }

    fn working(&self, unit: UnitHandle) -> bool {
        self.game.view().auth.units().get(unit).unwrap().core.work.is_some()
    }

    fn alive(&self, unit: UnitHandle) -> bool {
        self.game.view().auth.units().get(unit).is_some()
    }
}

fn refused<T, E: std::fmt::Debug>(result: Result<T, E>, text: &str) {
    let message = format!("{:?}", result.map(|_| ()));
    assert!(message.contains(text), "{message}");
}

/// The Legion builds roads and forts, a plain swordsman builds nothing.
#[test]
fn the_legion_builds_roads_and_forts() {
    let mut w = World::new();
    w.learn("core:tech/the-wheel");
    w.learn("core:tech/engineering");
    let at = w.tile(12, 8);
    let legion = w.unit("core:unit/legion", at);
    let fort = w.improvement("core:improvement/fort");
    w.game.execute(&w.rules, Command::BuildRoad { unit: legion }).unwrap();
    assert!(w.working(legion), "the Legion lays a road");
    let other = w.tile(14, 8);
    let second = w.unit("core:unit/legion", other);
    w.game.execute(&w.rules, Command::BuildImprovement { unit: second, improvement: fort }).unwrap();
    assert!(w.working(second), "the Legion builds a fort");
    let swordsman = w.unit("core:unit/swordsman", w.tile(16, 8));
    refused(w.game.execute(&w.rules, Command::BuildRoad { unit: swordsman }), "not a worker");
    w.game.validate(&w.rules).unwrap();
}

/// The Legion's builds end where the catalog's list ends: no farm, no
/// chopping a forest. A worker builds both.
#[test]
fn the_legion_builds_nothing_else() {
    let mut w = World::new();
    w.learn("core:tech/agriculture");
    let farm = w.improvement("core:improvement/farm");
    let legion = w.unit("core:unit/legion", w.tile(12, 8));
    refused(
        w.game.execute(&w.rules, Command::BuildImprovement { unit: legion, improvement: farm }),
        "not built by this unit",
    );
    refused(
        w.game.execute(&w.rules, Command::ChopForest { unit: legion }),
        "not built by this unit",
    );
    let worker = w.unit("core:unit/worker", w.tile(7, 8));
    w.game.execute(&w.rules, Command::BuildImprovement { unit: worker, improvement: farm }).unwrap();
    assert!(w.working(worker));
    w.game.validate(&w.rules).unwrap();
}

/// The Samurai builds fishing boats on a sea tile it stands on, in one turn,
/// and stays; a work boat is used up.
#[test]
fn the_samurai_builds_fishing_boats_and_stays() {
    let mut w = World::new();
    w.learn("core:tech/sailing");
    let sea = w.tile(9, 8);
    let fish = ResourceId(w.rules.resources.iter().position(|r| r.key == "core:resource/fish").unwrap() as u16);
    w.game.debug_set_resource_batch(&w.rules, &[(sea, fish)]);
    let boats = w.improvement("core:improvement/fishing-boats");
    let samurai = w.unit("core:unit/samurai", sea);
    w.game.execute(&w.rules, Command::BuildImprovement { unit: samurai, improvement: boats }).unwrap();
    assert!(w.alive(samurai), "a Samurai is not used up");
    assert_eq!(w.game.view().auth.map().improvement(sea).flatten(), Some(boats));
    w.game.validate(&w.rules).unwrap();

    let mut w = World::new();
    w.learn("core:tech/sailing");
    let sea = w.tile(9, 8);
    w.game.debug_set_resource_batch(&w.rules, &[(sea, fish)]);
    let boat = w.unit("core:unit/work-boat", sea);
    w.game.execute(&w.rules, Command::BuildImprovement { unit: boat, improvement: boats }).unwrap();
    assert!(!w.alive(boat), "a work boat is used up");
    w.game.validate(&w.rules).unwrap();
}
