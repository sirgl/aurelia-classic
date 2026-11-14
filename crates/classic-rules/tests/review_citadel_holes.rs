//! Review of rules/citadel: holes. Each test names what Unciv 4.22.10 does and fails when ours differs.

use classic_content::core::core_rules;
use classic_rules::ClassicGame;
use classic_rules::derived::great_people::GreatPersonAction;
use classic_rules::ids::{CityHandle, ImprovementId, PlayerId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::world::barbarians::BarbarianMode;
use classic_rules::world::city_states::Personality;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::rules::content::CityStateId;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

struct W {
    game: GameState,
    rules: CompiledRuleset,
}

impl W {
    fn new(players: u8) -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players, width: 30, height: 16, rng_seed: 5 });
        let grass = terrain(&rules, "core:terrain/grassland");
        let all: Vec<_> = (0..30 * 16).map(|i| (TileId(i), grass)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        game.debug_meet_all();
        W { game, rules }
    }
    fn tile(&self, q: i32, r: i32) -> TileId {
        self.game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
    }
    fn city(&mut self, owner: PlayerId, q: i32, r: i32) -> CityHandle {
        let at = self.tile(q, r);
        self.game.debug_found_city(&self.rules, owner, at)
    }
    fn spawn(&mut self, owner: PlayerId, key: &str, at: TileId) -> UnitHandle {
        let kind = UnitKindId(self.rules.units.iter().position(|u| u.key == key).unwrap() as u16);
        self.game.debug_spawn_unit(&self.rules, owner, at, kind)
    }
    fn citadel(&self) -> ImprovementId {
        ImprovementId(self.rules.improvements.iter().position(|i| i.key == "core:improvement/citadel").unwrap() as u16)
    }
    fn exec(&mut self, c: Command) -> Result<(), classic_rules::command::CommandError> {
        self.game.execute(&self.rules, c).map(|_| ())
    }
    fn owner(&self, t: TileId) -> Option<PlayerId> {
        self.game.view().auth.map().owner(t).flatten()
    }
}

fn terrain(rules: &CompiledRuleset, key: &str) -> TerrainId {
    TerrainId(rules.terrains.iter().find(|t| t.key == key).unwrap().map_code.unwrap())
}

/// Unciv: takeOwnership teleports a unit that may no longer stand on the tile (closed borders).
/// Ours: a unit of a player at peace stays on a tile that has become the citadel owner's.
#[test]
fn hole_unit_of_a_peaceful_player_left_inside_taken_land() {
    let mut w = W::new(2);
    w.city(P0, 5, 7);
    w.city(P1, 9, 7); // owns (8,7)
    let site = w.tile(7, 7);
    assert_eq!(w.owner(site), None);
    let victim_tile = w.tile(8, 7);
    assert_eq!(w.owner(victim_tile), Some(P1));
    let walker = w.spawn(P1, "core:unit/warrior", victim_tile);
    let general = w.spawn(P0, "core:unit/great-general", site);
    w.exec(Command::GreatPersonAction { unit: general, action: GreatPersonAction::Construct(w.citadel()) }).unwrap();
    assert_eq!(w.owner(victim_tile), Some(P0), "taken");
    let at = w.game.view().auth.units().get(walker).map(|u| u.core.tile);
    assert_ne!(at, Some(victim_tile), "a peaceful unit must not stay inside P0's closed borders");
}

/// Same for a ship on a taken water tile (Unciv: the filter is "all", water is taken). The tile
/// (8,7) is the other player's before the citadel, the ship stands on it and must be moved.
#[test]
fn hole_ship_of_a_peaceful_player_left_inside_taken_water() {
    let mut w = W::new(2);
    w.city(P0, 5, 7);
    w.city(P1, 9, 7);
    let sea = terrain(&w.rules, "core:terrain/coast");
    let wet = w.tile(8, 7);
    // Water for the ship to go to: the taken tile and three more on P1's side.
    let lake = [wet, w.tile(9, 6), w.tile(10, 7), w.tile(10, 6)];
    w.game.debug_set_terrain_batch(&w.rules, &lake.map(|t| (t, sea)));
    assert_eq!(w.owner(wet), Some(P1), "the water is P1's before the citadel");
    let site = w.tile(7, 7);
    let ship = w.spawn(P1, "core:unit/trireme", wet);
    let general = w.spawn(P0, "core:unit/great-general", site);
    w.exec(Command::GreatPersonAction { unit: general, action: GreatPersonAction::Construct(w.citadel()) }).unwrap();
    assert_eq!(w.owner(wet), Some(P0), "the water is taken");
    let at = w.game.view().auth.units().get(ship).map(|u| u.core.tile);
    assert!(at.is_some(), "the ship lives");
    assert_ne!(at, Some(wet), "ship inside P0's closed waters");
}

/// Unciv takeOwnership: `if (tile.isBarbarianEncampment()) tile.removeImprovement()`.
#[test]
fn hole_barbarian_camp_in_the_radius_survives() {
    let mut w = W::new(3);
    w.game.debug_set_player_kind(P2, PlayerKind::Module(ClassicKind::Barbarian));
    w.game.debug_set_barbarian_mode(BarbarianMode::Normal);
    w.city(P0, 5, 7);
    w.city(P1, 25, 12);
    let camp = w.tile(8, 7);
    w.game.debug_place_camp(&w.rules, camp);
    let site = w.tile(7, 7);
    let general = w.spawn(P0, "core:unit/great-general", site);
    w.exec(Command::BuildCitadel { unit: general }).unwrap();
    assert_eq!(w.owner(camp), Some(P0));
    let still = w.game.view().auth.module().barbarians().camps().iter().any(|c| c.tile == camp);
    assert!(!still, "the camp on a taken tile is removed (Unciv)");
}

/// Unciv: the 15 influence is lost per tile; the citadel owner meets the city-state.
#[test]
fn city_state_loses_15_per_tile() {
    let mut w = W::new(3);
    w.game.debug_set_player_kind(P2, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    w.game.debug_set_personality(P2, Personality::Neutral);
    w.city(P0, 5, 7);
    w.city(P1, 25, 12);
    w.city(P2, 9, 7);
    w.game.debug_set_influence(&w.rules, P2, P0, 40);
    let site = w.tile(7, 7);
    let dims = w.game.view().auth.map().dimensions;
    let theirs: Vec<_> = classic_rules::math::hex_disc(dims, site, 1)
        .filter(|&t| w.owner(t) == Some(P2) && w.game.view().auth.indexes().city_on_tile(t).is_none())
        .collect();
    assert!(!theirs.is_empty());
    let general = w.spawn(P0, "core:unit/great-general", site);
    w.exec(Command::BuildCitadel { unit: general }).unwrap();
    let after = w.game.view().auth.module().city_states().influence(P2, P0);
    assert_eq!(after, 40 - 15 * theirs.len() as i32, "{} tiles", theirs.len());
}

/// Both paths must obey the same site rule. `BuildCitadel` skips `site()`: a second citadel on the
/// same tile is refused by Construct ("already built") but accepted by BuildCitadel.
#[test]
fn hole_build_citadel_on_a_tile_that_has_one() {
    let mut w = W::new(2);
    w.city(P0, 5, 7);
    w.city(P1, 25, 12);
    let site = w.tile(7, 7);
    let g1 = w.spawn(P0, "core:unit/great-general", site);
    w.exec(Command::BuildCitadel { unit: g1 }).unwrap();
    let g2 = w.spawn(P0, "core:unit/great-general", site);
    let construct = w.exec(Command::GreatPersonAction { unit: g2, action: GreatPersonAction::Construct(w.citadel()) });
    assert!(construct.is_err(), "Construct refuses a second citadel");
    assert!(w.exec(Command::BuildCitadel { unit: g2 }).is_err(), "BuildCitadel must refuse it as well");
}

/// A natural wonder tile or a feature that forbids improvements: Construct refuses, BuildCitadel does not.
#[test]
fn hole_build_citadel_on_ice() {
    let mut w = W::new(2);
    w.city(P0, 5, 7);
    w.city(P1, 25, 12);
    let site = w.tile(7, 7);
    let ice = w.rules.features.iter().position(|f| f.key == "core:feature/ice").unwrap();
    w.game.debug_set_feature_batch(&w.rules, &[(site, classic_rules::ids::FeatureId(ice as u16))]);
    let g = w.spawn(P0, "core:unit/great-general", site);
    let construct = w.exec(Command::GreatPersonAction { unit: g, action: GreatPersonAction::Construct(w.citadel()) });
    let order = w.exec(Command::BuildCitadel { unit: g });
    assert_eq!(construct.is_ok(), order.is_ok(), "construct {construct:?}, order {order:?}");
}

/// Tiles go to a city that may be farther than CULTURE_RADIUS_MAX (5): capture and raze release
/// only that disc (`transfer_city_tiles`, `release_city_tiles`).
#[test]
fn hole_taken_tile_beyond_the_city_disc() {
    let mut w = W::new(2);
    let a = w.city(P0, 3, 7);
    w.city(P1, 28, 12);
    for q in 5..=8 {
        let site = w.tile(q, 7);
        let g = w.spawn(P0, "core:unit/great-general", site);
        w.exec(Command::BuildCitadel { unit: g }).unwrap();
    }
    let dims = w.game.view().auth.map().dimensions;
    let centre = w.game.view().auth.cities().get(a).unwrap().tile;
    let far = (0..30 * 16)
        .map(TileId)
        .filter(|&t| w.game.view().auth.map().owning_city(t) == Some(a))
        .filter(|&t| dims.distance(centre, t).unwrap() > 5)
        .count();
    assert_eq!(far, 0, "tiles of the city lie within CULTURE_RADIUS_MAX (the capture and razing code assume it)");
}

/// The consequence of the far tile: the city is captured and `validate` fails (tile owned by P0,
/// its city now P1's).
#[test]
fn hole_far_tile_survives_the_capture_of_its_city() {
    let mut w = W::new(2);
    let a = w.city(P0, 3, 7);
    w.city(P0, 3, 13);
    w.city(P1, 28, 3);
    for q in 5..=8 {
        let site = w.tile(q, 7);
        let g = w.spawn(P0, "core:unit/great-general", site);
        w.exec(Command::BuildCitadel { unit: g }).unwrap();
    }
    w.game.debug_force_war(&w.rules, P0, P1);
    w.game.debug_set_city_damage(a, 5000);
    let hitter = w.spawn(P1, "core:unit/swordsman", w.tile(2, 7));
    w.exec(Command::EndTurn { player: P0 }).unwrap();
    w.exec(Command::AttackCity { attacker: hitter, city: a }).unwrap();
    let owner = w.game.view().auth.cities().get(a).map(|c| c.owner);
    assert_eq!(owner, Some(P1), "captured");
    w.game.validate(&w.rules).unwrap();
}
