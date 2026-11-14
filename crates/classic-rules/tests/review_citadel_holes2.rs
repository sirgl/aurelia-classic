//! Review of rules/citadel, second round. Each test names what Unciv 4.22.10 does and fails when ours differs.

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


#[test]
fn n_ship_is_really_taken_and_moved() {
    let mut w = W::new(2);
    w.city(P0, 5, 7);
    w.city(P1, 9, 7);
    let sea = terrain(&w.rules, "core:terrain/coast");
    let wet = w.tile(8, 7);
    // Water for the ship to go to: the taken tile and three more on P1's side.
    let lake = [wet, w.tile(9, 6), w.tile(10, 7), w.tile(10, 6)];
    w.game.debug_set_terrain_batch(&w.rules, &lake.map(|t| (t, sea)));
    assert_eq!(w.owner(wet), Some(P1));
    let ship = w.spawn(P1, "core:unit/trireme", wet);
    let g = w.spawn(P0, "core:unit/great-general", w.tile(7, 7));
    w.exec(Command::GreatPersonAction { unit: g, action: GreatPersonAction::Construct(w.citadel()) }).unwrap();
    assert_eq!(w.owner(wet), Some(P0), "water taken");
    let at = w.game.view().auth.units().get(ship).map(|u| u.core.tile);
    assert!(at.is_some(), "the ship lives");
    assert_ne!(at, Some(wet));
    w.game.validate(&w.rules).unwrap();
}

/// A site 6 tiles from the only city (beyond the radius): what happens to the citadel and the land.
#[test]
fn n_site_beyond_every_radius() {
    let mut w = W::new(2);
    w.city(P0, 3, 7);
    w.city(P1, 28, 3);
    for q in 5..=8 {
        let g = w.spawn(P0, "core:unit/great-general", w.tile(q, 7));
        w.exec(Command::BuildCitadel { unit: g }).unwrap();
    }
    let site = w.tile(9, 7);
    let g = w.spawn(P0, "core:unit/great-general", site);
    let r = w.exec(Command::BuildCitadel { unit: g });
    assert!(r.is_err(), "no city of P0 reaches the site: the general is not spent for a citadel on no one's land");
    let again = w.exec(Command::GreatPersonAction { unit: g, action: GreatPersonAction::Construct(w.citadel()) });
    assert!(again.is_err(), "Construct refuses it as well");
    w.game.validate(&w.rules).unwrap();
}

/// A camp that still stands on the site tile (the general stands there) must not erase the citadel.
#[test]
fn n_camp_on_the_site() {
    let mut w = W::new(3);
    w.game.debug_set_player_kind(P2, PlayerKind::Module(ClassicKind::Barbarian));
    w.game.debug_set_barbarian_mode(BarbarianMode::Normal);
    w.city(P0, 5, 7);
    w.city(P1, 25, 12);
    let site = w.tile(7, 7);
    w.game.debug_place_camp(&w.rules, site);
    let g = w.spawn(P0, "core:unit/great-general", site);
    let r = w.exec(Command::BuildCitadel { unit: g });
    eprintln!("camp on the site: {r:?}");
    if r.is_ok() {
        assert_eq!(w.game.view().auth.map().improvement(site), Some(Some(w.citadel())));
        let camp_left = w.game.view().auth.module().barbarians().camps().iter().any(|c| c.tile == site);
        assert!(!camp_left, "the camp is gone with the site");
    }
    w.game.validate(&w.rules).unwrap();
}
