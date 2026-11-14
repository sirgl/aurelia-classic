//! The citadel of the great general as Unciv 4.22.10 has it
//! (TileImprovements.json, Citadel: "Can be built just outside your
//! borders", "Gain control over [all] tiles in a [1]-tile radius",
//! "Adjacent enemy units ending their turn take [30] damage";
//! ImprovementFunctions.kt, UniqueTriggerActivation.kt
//! `OneTimeTakeOverTilesInRadius`, UnitTurnManager.kt `doCitadelDamage`).

use classic_content::core::core_rules;
use classic_rules::derived::great_people::GreatPersonAction;
use classic_rules::ids::{CityHandle, ImprovementId, PlayerId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::world::unit::UNIT_MAX_HP;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

struct World {
    game: GameState,
    rules: CompiledRuleset,
}

impl World {
    /// Open grassland, 24 × 14, two players who have met; P1 has a city far
    /// east at (20, 7).
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 2, width: 24, height: 14, rng_seed: 5 });
        let grass = terrain(&rules, "core:terrain/grassland");
        let all: Vec<_> = (0..24 * 14).map(|i| (TileId(i), grass)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        game.debug_meet(P0, P1);
        let mut w = World { game, rules };
        let far = w.tile(20, 7);
        w.game.debug_found_city(&w.rules, P1, far);
        w
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

    fn exec(&mut self, command: Command) -> Result<(), classic_rules::command::CommandError> {
        self.game.execute(&self.rules, command).map(|_| ())
    }

    fn build(&mut self, general: UnitHandle) -> Result<(), classic_rules::command::CommandError> {
        let action = GreatPersonAction::Construct(self.citadel());
        self.exec(Command::GreatPersonAction { unit: general, action })
    }

    fn owner(&self, at: TileId) -> Option<PlayerId> {
        self.game.view().auth.map().owner(at).flatten()
    }

    fn owning_city(&self, at: TileId) -> Option<CityHandle> {
        self.game.view().auth.map().owning_city(at)
    }

    fn hp(&self, unit: UnitHandle) -> Option<u16> {
        self.game.view().auth.units().get(unit).map(|u| u.core.hp)
    }

    fn war(&mut self) {
        self.exec(Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    }

    /// P0 ends its turn, then P1 ends its own.
    fn round(&mut self) {
        self.exec(Command::EndTurn { player: P0 }).unwrap();
        self.exec(Command::EndTurn { player: P1 }).unwrap();
    }
}

fn terrain(rules: &CompiledRuleset, key: &str) -> TerrainId {
    TerrainId(rules.terrains.iter().find(|t| t.key == key).unwrap().map_code.unwrap())
}

/// A citadel on the nobody's tile just outside the border takes that tile
/// and every tile around it for the player; the enemy that ends its turn
/// next to it takes 30 damage. Before: the tile stayed nobody's (the culture
/// bomb asked for the tile's own city), so the citadel hurt nobody.
#[test]
fn a_citadel_just_outside_the_border_takes_the_land_and_hurts_the_enemy() {
    let mut w = World::new();
    let home = w.city(P0, 5, 7);
    let site = w.tile(7, 7);
    let beyond = w.tile(8, 7);
    assert_eq!((w.owner(site), w.owner(beyond)), (None, None), "outside the border");
    let general = w.spawn(P0, "core:unit/great-general", site);
    w.build(general).unwrap();
    assert_eq!(w.game.view().auth.map().improvement(site), Some(Some(w.citadel())));
    let dims = w.game.view().auth.map().dimensions;
    for near in classic_rules::math::hex_disc(dims, site, 1) {
        assert_eq!(w.owner(near), Some(P0), "tile {near:?} around the citadel");
        assert_eq!(w.owning_city(near), Some(home), "the only city gets them");
    }
    w.war();
    let enemy = w.spawn(P1, "core:unit/warrior", beyond);
    w.round();
    assert_eq!(w.hp(enemy), Some(UNIT_MAX_HP - 30), "30 damage at the end of the enemy's turn");
    w.game.validate(&w.rules).unwrap();
}

/// "Can be built just outside your borders": a tile of another player next
/// to own land takes a citadel too (Unciv: ImprovementFunctions.kt), and
/// the citadel takes the foreign tiles around it, never a city. Before: "not
/// your land".
#[test]
fn a_citadel_may_stand_on_foreign_land_next_to_own_and_takes_it() {
    let mut w = World::new();
    let home = w.city(P0, 5, 7);
    let theirs = w.city(P1, 8, 7);
    let site = w.tile(7, 7);
    assert_eq!(w.owner(site), Some(P1), "the foreign city's ring");
    assert_eq!(w.owner(w.tile(6, 7)), Some(P0), "next to own land");
    let general = w.spawn(P0, "core:unit/great-general", site);
    w.build(general).unwrap();
    let dims = w.game.view().auth.map().dimensions;
    let center = w.tile(8, 7);
    for near in classic_rules::math::hex_disc(dims, site, 1) {
        if near == center {
            assert_eq!(w.owner(near), Some(P1), "a city tile is never taken");
            continue;
        }
        assert_eq!(w.owner(near), Some(P0), "tile {near:?}");
        assert_eq!(w.owning_city(near), Some(home));
    }
    assert!(w.game.view().auth.cities().get(theirs).is_some());
    w.game.validate(&w.rules).unwrap();
}

/// The same rule through the general's own order (`BuildCitadel`).
#[test]
fn the_build_citadel_order_follows_the_same_rule() {
    let mut w = World::new();
    w.city(P0, 5, 7);
    w.city(P1, 8, 7);
    let site = w.tile(7, 7);
    let general = w.spawn(P0, "core:unit/great-general", site);
    w.exec(Command::BuildCitadel { unit: general }).unwrap();
    assert_eq!(w.owner(site), Some(P0));
    assert_eq!(w.owner(w.tile(8, 6)), Some(P0), "a foreign tile around it");
    assert_eq!(w.owner(w.tile(8, 7)), Some(P1), "the city stays");
    w.game.validate(&w.rules).unwrap();
}

/// Two tiles away from own land is not "just outside".
#[test]
fn a_citadel_needs_own_land_next_to_it() {
    let mut w = World::new();
    w.city(P0, 5, 7);
    let general = w.spawn(P0, "core:unit/great-general", w.tile(8, 7));
    assert!(w.build(general).is_err());
    assert!(w.exec(Command::BuildCitadel { unit: general }).is_err());
}

/// The taken tiles go to one city: the nearest own city holding a tile
/// among them or next to them (the west city holds (4, 7) next to (5, 7),
/// the east city (7, 7); the east one is nearer). The west city's own tile
/// (4, 7) is not among them.
#[test]
fn the_taken_tiles_go_to_the_nearest_bordering_city() {
    let mut w = World::new();
    let west = w.city(P0, 3, 7);
    assert_eq!(w.owning_city(w.tile(4, 7)), Some(west));
    let east = w.city(P0, 8, 7);
    let site = w.tile(6, 7);
    let dims = w.game.view().auth.map().dimensions;
    assert_eq!((dims.distance(w.tile(3, 7), site), dims.distance(w.tile(8, 7), site)), (Some(3), Some(2)));
    let general = w.spawn(P0, "core:unit/great-general", site);
    w.build(general).unwrap();
    for near in classic_rules::math::hex_disc(dims, site, 1) {
        assert_eq!(w.owning_city(near), Some(east), "tile {near:?}");
    }
}

/// The damage comes at the end of the victim's own turn, after its moves: a
/// unit that walks away is not hurt (Unciv: UnitTurnManager.endTurn). A pin
/// of the time, not a change.
#[test]
fn a_unit_that_walks_away_before_its_turn_ends_is_not_hurt() {
    let mut w = World::new();
    w.city(P0, 5, 7);
    let general = w.spawn(P0, "core:unit/great-general", w.tile(7, 7));
    w.build(general).unwrap();
    w.war();
    let stays = w.spawn(P1, "core:unit/warrior", w.tile(8, 6));
    let leaves = w.spawn(P1, "core:unit/warrior", w.tile(8, 7));
    w.exec(Command::EndTurn { player: P0 }).unwrap();
    let away = w.tile(10, 7);
    w.exec(Command::MoveUnit { unit: leaves, destination: away }).unwrap();
    w.exec(Command::EndTurn { player: P1 }).unwrap();
    assert_eq!(w.hp(stays), Some(UNIT_MAX_HP - 30));
    assert_eq!(w.hp(leaves), Some(UNIT_MAX_HP), "walked away");
}

/// Two citadels next to a unit do not add up: the strongest one hurts.
#[test]
fn citadels_do_not_add_up() {
    let mut w = World::new();
    w.city(P0, 5, 7);
    for at in [(7, 7), (7, 8)] {
        let site = w.tile(at.0, at.1);
        let general = w.spawn(P0, "core:unit/great-general", site);
        w.build(general).unwrap();
    }
    w.war();
    let dims = w.game.view().auth.map().dimensions;
    let (a, b) = (w.tile(7, 7), w.tile(7, 8));
    let both = classic_rules::math::hex_disc(dims, a, 1)
        .find(|&t| t != a && t != b && dims.distance(t, b) == Some(1))
        .unwrap();
    let enemy = w.spawn(P1, "core:unit/warrior", both);
    w.round();
    assert_eq!(w.hp(enemy), Some(UNIT_MAX_HP - 30));
}

/// A pillaged citadel hurts nobody (Unciv: `getUnpillagedImprovement`).
/// Before: it still hurt.
#[test]
fn a_pillaged_citadel_hurts_nobody() {
    let mut w = World::new();
    w.city(P0, 5, 7);
    let site = w.tile(7, 7);
    let general = w.spawn(P0, "core:unit/great-general", site);
    w.build(general).unwrap();
    w.war();
    w.exec(Command::EndTurn { player: P0 }).unwrap();
    let raider = w.spawn(P1, "core:unit/warrior", site);
    w.exec(Command::Pillage { unit: raider }).unwrap();
    assert!(w.game.view().auth.map().pillaged(site));
    let next = w.spawn(P1, "core:unit/warrior", w.tile(8, 7));
    w.exec(Command::EndTurn { player: P1 }).unwrap();
    assert_eq!(w.hp(next), Some(UNIT_MAX_HP), "pillaged: no damage");
}

/// "Land" in Unciv's data is any land tile, hills too (Hill is a feature
/// there): a citadel stands on hills.
#[test]
fn a_citadel_stands_on_hills() {
    let mut w = World::new();
    w.city(P0, 5, 7);
    let site = w.tile(7, 7);
    let hill = terrain(&w.rules, "core:terrain/hill");
    w.game.debug_set_terrain_batch(&w.rules, &[(site, hill)]);
    let general = w.spawn(P0, "core:unit/great-general", site);
    w.build(general).unwrap();
    assert_eq!(w.game.view().auth.map().improvement(site), Some(Some(w.citadel())));
}
