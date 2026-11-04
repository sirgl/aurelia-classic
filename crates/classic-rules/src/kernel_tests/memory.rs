//! Observation memory (R352): the picture of a city a player cannot see is
//! the city as that player last saw it, and changes only when the player
//! looks again. A foreign unit that walks out of sight is not remembered.

use crate::combat::city::max_hit_points;
use crate::command::CityDecision;
use crate::fixtures::{arena_content, play_round, play_until_active};
use crate::ids::{CityHandle, PlayerId, TileId, UnitHandle, UnitKindId};
use crate::knowledge::CitySighting;
use crate::math::HexCoord;
use crate::world::state::GameConfig;
use crate::{Command, CompiledRuleset, GameState};

const OBSERVER: PlayerId = PlayerId(0);
const OWNER: PlayerId = PlayerId(1);
const CONQUEROR: PlayerId = PlayerId(2);
const WARRIOR: UnitKindId = UnitKindId(1);

struct Scene {
    game: GameState,
    rules: CompiledRuleset,
    scout: UnitHandle,
    city: CityHandle,
    site: TileId,
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// P0 watches P1's city from two tiles away; P2 waits elsewhere.
fn scene() -> Scene {
    let rules = arena_content();
    let mut game = GameState::new(&rules, GameConfig { players: 3, width: 24, height: 24, rng_seed: 7 });
    game.debug_meet_all();
    game.debug_found_city(&rules, OWNER, tile(&game, 3, 18));
    let site = tile(&game, 14, 8);
    let city = game.debug_found_city(&rules, OWNER, site);
    let scout = game.debug_spawn_unit(&rules, OBSERVER, tile(&game, 12, 8), WARRIOR);
    game.debug_found_city(&rules, CONQUEROR, tile(&game, 20, 20));
    Scene { game, rules, scout, city, site }
}

impl Scene {
    fn move_scout(&mut self, q: i32, r: i32) {
        let destination = tile(&self.game, q, r);
        self.game.execute(&self.rules, Command::MoveUnit { unit: self.scout, destination }).unwrap();
    }

    /// The scout walks away; returns the city's population at that moment
    /// (a round passed first, and a small city may have starved).
    fn leave(&mut self) -> u32 {
        self.new_turn();
        let population = self.population();
        self.move_scout(10, 8);
        assert!(!self.game.player_view(&self.rules, OBSERVER).is_visible(self.site), "the scout left the city");
        population
    }

    /// A fresh turn of the observer (its scout has all its moves again).
    fn new_turn(&mut self) {
        play_until_active(&mut self.game, &self.rules, OBSERVER);
        play_round(&mut self.game, &self.rules);
    }

    fn population(&self) -> u32 {
        self.game.view().auth.cities().get(self.city).map_or(0, |city| city.population)
    }

    fn look_again(&mut self) {
        self.new_turn();
        self.move_scout(12, 8);
        assert!(self.game.player_view(&self.rules, OBSERVER).is_visible(self.site), "the scout is back");
    }

    /// The cities P0 remembers, as it would draw them.
    fn picture(&self) -> Vec<(TileId, CitySighting)> {
        self.picture_of(OBSERVER)
    }

    fn picture_of(&self, viewer: PlayerId) -> Vec<(TileId, CitySighting)> {
        let view = self.game.player_view(&self.rules, viewer);
        view.remembered_cities().map(|(tile, sighting)| (tile, sighting.clone())).collect()
    }

    fn capture(&mut self) {
        play_until_active(&mut self.game, &self.rules, CONQUEROR);
        self.game.execute(&self.rules, Command::DeclareWar { player: CONQUEROR, enemy: OWNER }).unwrap();
        let from = tile(&self.game, 15, 8);
        let attacker = self.game.debug_spawn_unit(&self.rules, CONQUEROR, from, WARRIOR);
        let core = self.game.view().auth.cities().get(self.city).unwrap();
        let max = max_hit_points(self.game.view().auth, &self.rules, core);
        self.game.debug_set_city_damage(self.city, max - 1);
        self.game.execute(&self.rules, Command::AttackCity { attacker, city: self.city }).unwrap();
        assert_eq!(self.game.view().auth.cities().get(self.city).unwrap().owner, CONQUEROR);
    }
}

fn seen_numbers(sighting: &CitySighting) -> (u32, u8) {
    let seen = sighting.seen.as_ref().expect("a city seen by a unit has its numbers");
    (seen.population, seen.era)
}

#[test]
fn a_city_in_sight_is_live_not_remembered() {
    let scene = scene();
    assert!(scene.picture().is_empty());
}

#[test]
fn a_city_out_of_sight_is_remembered_as_last_seen() {
    let mut scene = scene();
    scene.game.debug_grow_city(&scene.rules, scene.city, 2);
    let left_at = scene.leave();
    let picture = scene.picture();
    assert_eq!(picture.len(), 1);
    let (at, sighting) = &picture[0];
    assert_eq!(*at, scene.site);
    assert_eq!(sighting.owner, OWNER);
    assert_eq!(seen_numbers(sighting).0, left_at, "the population at the moment the scout left");
}

#[test]
fn growth_out_of_sight_does_not_change_the_picture_until_re_observed() {
    let mut scene = scene();
    let left_at = scene.leave();
    let before = scene.picture();
    assert_eq!(seen_numbers(&before[0].1).0, left_at);
    scene.game.debug_grow_city(&scene.rules, scene.city, 4);
    assert_eq!(scene.picture(), before, "growth out of sight is invisible");
    play_round(&mut scene.game, &scene.rules);
    assert_eq!(scene.picture(), before, "so is a whole round");
    scene.look_again();
    assert!(scene.picture().is_empty(), "in sight again, the city is live");
    let left_again = scene.leave();
    assert!(left_again > left_at, "the city grew meanwhile");
    assert_eq!(seen_numbers(&scene.picture()[0].1).0, left_again, "the next memory holds the new size");
}

#[test]
fn a_new_owner_out_of_sight_does_not_change_the_picture_until_re_observed() {
    let mut scene = scene();
    scene.leave();
    let before = scene.picture();
    assert_eq!(before.len(), 1, "P0 remembers the city");
    scene.capture();
    assert_eq!(scene.picture(), before);
    assert_eq!(scene.picture()[0].1.owner, OWNER, "still the old owner's city to P0");
    scene.look_again();
    scene.leave();
    assert_eq!(scene.picture()[0].1.owner, CONQUEROR, "re-observed, the owner is the new one");
}

#[test]
fn a_razed_city_stays_on_the_map_until_re_observed() {
    let mut scene = scene();
    scene.leave();
    let before = scene.picture();
    assert_eq!(before.len(), 1, "P0 remembers the city");
    scene.capture();
    let decision = Command::DecideCity { player: CONQUEROR, city: scene.city, decision: CityDecision::Raze };
    scene.game.execute(&scene.rules, decision).unwrap();
    for _ in 0..5 {
        play_round(&mut scene.game, &scene.rules);
    }
    assert!(scene.game.view().auth.cities().get(scene.city).is_none(), "the city is gone");
    assert_eq!(scene.picture(), before, "P0 still remembers it");
    scene.look_again();
    assert!(scene.picture().is_empty(), "the ruins are seen: the memory is cleared");
    scene.leave();
    assert!(scene.picture().is_empty());
}

#[test]
fn a_foreign_unit_leaving_sight_is_not_remembered() {
    let mut scene = scene();
    let walker = scene.game.debug_spawn_unit(&scene.rules, OWNER, tile(&scene.game, 12, 9), WARRIOR);
    let walker_tile = scene.game.view().auth.units().get(walker).unwrap().core.tile;
    assert!(scene.game.player_view(&scene.rules, OBSERVER).is_visible(walker_tile));
    scene.leave();
    let core = scene.game.view().auth.units().get(walker).unwrap().core.clone();
    let view = scene.game.player_view(&scene.rules, OBSERVER);
    assert!(!view.sees_unit(&core), "the walker is out of sight");
    assert_eq!(scene.picture().len(), 1, "the memory holds the city and nothing of the walker");
}
