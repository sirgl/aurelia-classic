//! Technology rules on the classic ruleset: Engineering's bridges and
//! Machinery's faster roads (`navigation::cost::RouteRules`), Satellites
//! reveal the map, Archaeology makes the antiquity sites appear, and
//! Honor's opener shows new barbarian camps (`PlayerRule::SeeBarbarianCamps`).

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::derived::archaeology::trigger_tech;
use classic_rules::ids::{PlayerId, PolicyId, TechId, TerrainId, TileId};
use classic_rules::knowledge::view::KnownTile;
use classic_rules::math::HexCoord;
use classic_rules::navigation::cost::{step_cost, StepCost};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::{compile_rules, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const GRASS: TerrainId = TerrainId(1);

struct World {
    game: GameState,
    rules: CompiledRuleset,
}

impl World {
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 6 });
        let all: Vec<_> = (0..600).map(|i| (TileId(i), GRASS)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        World { game, rules }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        self.game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
    }

    fn tech(&self, key: &str) -> TechId {
        TechId(self.rules.technologies.iter().position(|t| t.key == key).unwrap() as u16)
    }

    fn step(&self, from: TileId, to: TileId) -> Option<StepCost> {
        step_cost(self.game.view().auth, &self.rules, P0, from, to)
    }
}

#[test]
fn engineering_bridges_rivers_and_machinery_speeds_roads() {
    let mut w = World::new();
    let (a, b, c) = (w.tile(5, 5), w.tile(6, 5), w.tile(7, 5));
    w.game.debug_set_road_batch(&w.rules, &[a, b, c]);
    w.game.debug_set_river_edges(&w.rules, &[(b, c)]);
    assert_eq!(w.step(a, b), Some(StepCost::Points(30)), "a road step: half a move");
    assert_eq!(w.step(b, c), Some(StepCost::AllMoves), "no bridge: the river takes every move");
    let engineering = w.tech("core:tech/engineering");
    w.game.debug_learn(&w.rules, P0, engineering);
    assert_eq!(w.step(b, c), Some(StepCost::Points(30)), "Engineering: the road crosses the river");
    let machinery = w.tech("core:tech/machinery");
    w.game.debug_learn(&w.rules, P0, machinery);
    assert_eq!(w.step(a, b), Some(StepCost::Points(20)), "Machinery: a third of a move");
    let off_road = w.tile(5, 7);
    assert_eq!(w.step(off_road, w.tile(6, 7)), Some(StepCost::Points(60)));
}

#[test]
fn satellites_reveal_the_whole_map() {
    let mut w = World::new();
    let far = w.tile(25, 15);
    let view = |w: &World| w.game.player_view(&w.rules, P0).tile(far);
    assert_eq!(view(&w), KnownTile::Unknown);
    let satellites = w.tech("core:tech/satellites");
    w.game.debug_discover(&w.rules, P0, satellites);
    assert!(matches!(view(&w), KnownTile::Remembered { .. }), "remembered as it is now");
    let saved = decode(&encode(&w.game, &w.rules), &w.rules).unwrap();
    assert_eq!(authoritative_hash(&saved), authoritative_hash(&w.game));
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn archaeology_is_the_technology_that_makes_sites_appear() {
    let w = World::new();
    assert_eq!(trigger_tech(&w.rules), Some(w.tech("core:tech/archaeology")));
}

#[test]
fn honor_shows_a_new_barbarian_camp_on_explored_land() {
    let mut w = World::new();
    let site = w.tile(20, 10);
    w.game.debug_reveal(P0, &[site]);
    w.game.debug_place_camp(&w.rules, site);
    let known_camp = |w: &World| match w.game.player_view(&w.rules, P0).tile(site) {
        KnownTile::Remembered { snapshot, .. } | KnownTile::Visible(snapshot) => snapshot.improvement.is_some(),
        KnownTile::Unknown => false,
    };
    assert!(!known_camp(&w), "without Honor the memory is stale");
    let honor = PolicyId(w.rules.module.policies.iter().position(|p| p.key == "core:policy/honor/opener").unwrap() as u16);
    w.game.debug_adopt_policy(&w.rules, P0, honor);
    let other = w.tile(22, 12);
    w.game.debug_reveal(P0, &[other]);
    w.game.debug_place_camp(&w.rules, other);
    let seen = match w.game.player_view(&w.rules, P0).tile(other) {
        KnownTile::Remembered { snapshot, .. } | KnownTile::Visible(snapshot) => snapshot.improvement.is_some(),
        KnownTile::Unknown => false,
    };
    assert!(seen, "Honor: the new camp shows in the memory");
    let unexplored = w.tile(26, 2);
    w.game.debug_place_camp(&w.rules, unexplored);
    assert_eq!(w.game.player_view(&w.rules, P0).tile(unexplored), KnownTile::Unknown, "only on explored land");
}
