//! Resource effects on the classic ruleset (`Stat::ResourceCopies`,
//! `ResourceQuantityPercent`, `StrategicQuantityPercent`,
//! `ExtraLuxuryCopies`, the Feitoria's city-state luxuries, the hidden
//! sites of the Exploration finisher). The oracle is the resource ledger
//! (`derived::resources`) worked out by hand.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::derived::archaeology::sees_hidden;
use classic_rules::derived::resources::{resource_ledger, ResourceLedger};
use classic_rules::ids::{BuildingKindId, CityHandle, ImprovementId, PlayerId, PolicyId, ResourceId, TechId, TerrainId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::player::PlayerKind;
use classic_rules::{compile_rules, CompiledRuleset, GameConfig, GameState};
use classic_rules::world::player::ClassicKind;

const P0: PlayerId = PlayerId(0);
const CS: PlayerId = PlayerId(1);
const GRASS: TerrainId = TerrainId(1);

struct World {
    game: GameState,
    rules: CompiledRuleset,
    city: CityHandle,
}

impl World {
    /// A 30×20 grassland map, P0's city at (5, 5).
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 9 });
        let all: Vec<_> = (0..600).map(|i| (TileId(i), GRASS)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let at = game.view().auth.map().dimensions.to_tile(HexCoord { q: 5, r: 5 }).unwrap();
        let city = game.debug_found_city(&rules, P0, at);
        World { game, rules, city }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        self.game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
    }

    fn resource(&self, key: &str) -> ResourceId {
        ResourceId(self.rules.resources.iter().position(|r| r.key == key).unwrap() as u16)
    }

    fn improvement(&self, key: &str) -> ImprovementId {
        ImprovementId(self.rules.improvements.iter().position(|i| i.key == key).unwrap() as u16)
    }

    fn learn(&mut self, key: &str) {
        let t = self.rules.technologies.iter().position(|t| t.key == key).unwrap();
        self.game.debug_learn(&self.rules, P0, TechId(t as u16));
    }

    fn civ(&mut self, key: &str) {
        let civ = self.rules.civs.iter().position(|c| c.key == key).unwrap();
        self.game.debug_set_player_civ(P0, civ as u8);
    }

    fn building(&mut self, city: CityHandle, key: &str) {
        let b = BuildingKindId(self.rules.buildings.iter().position(|b| b.key == key).unwrap() as u16);
        self.game.debug_add_building(&self.rules, city, b);
    }

    fn policy(&mut self, key: &str) {
        let p = PolicyId(self.rules.module.policies.iter().position(|p| p.key == key).unwrap() as u16);
        self.game.debug_adopt_policy(&self.rules, P0, p);
    }

    /// A deposit of `amount` copies on `tile` with its improvement.
    fn deposit(&mut self, tile: TileId, resource: &str, amount: u8, improvement: &str) {
        let r = self.resource(resource);
        self.game.debug_set_deposit(&self.rules, tile, r, amount);
        let i = self.improvement(improvement);
        self.game.debug_set_improvement_batch(&self.rules, &[(tile, i)]);
    }

    fn ledger(&self, player: PlayerId) -> ResourceLedger {
        resource_ledger(self.game.view().auth, &self.rules, player)
    }
}

#[test]
fn a_recycling_center_provides_two_aluminum() {
    let mut w = World::new();
    let aluminum = w.resource("core:resource/aluminum");
    assert_eq!(w.ledger(P0).total(aluminum), 0);
    w.building(w.city, "core:building/recycling_center");
    assert_eq!(w.ledger(P0).total(aluminum), 2);
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn russia_doubles_iron_and_the_third_alternative_adds_half_again() {
    let mut w = World::new();
    let iron = w.resource("core:resource/iron");
    w.deposit(w.tile(6, 5), "core:resource/iron", 3, "core:improvement/mine");
    w.learn("core:tech/iron-working");
    assert_eq!(w.ledger(P0).total(iron), 3);
    w.civ("core:nation/russia");
    assert_eq!(w.ledger(P0).total(iron), 6, "Siberian Riches: +100 %");
    w.policy("core:policy/autocracy/third-alternative");
    assert_eq!(w.ledger(P0).total(iron), 9, "+50 % of strategic resources after the trait (6 x 1.5)");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn arabia_doubles_oil_only() {
    let mut w = World::new();
    let (oil, iron) = (w.resource("core:resource/oil"), w.resource("core:resource/iron"));
    w.deposit(w.tile(6, 5), "core:resource/oil", 4, "core:improvement/oil-well");
    w.deposit(w.tile(4, 5), "core:resource/iron", 2, "core:improvement/mine");
    w.learn("core:tech/biology");
    w.learn("core:tech/iron-working");
    w.civ("core:nation/arabia");
    assert_eq!(w.ledger(P0).total(oil), 8);
    assert_eq!(w.ledger(P0).total(iron), 2);
}

#[test]
fn a_bazaar_gives_one_more_copy_of_each_luxury_tile_of_its_city() {
    let mut w = World::new();
    let wine = w.resource("core:resource/wine");
    w.deposit(w.tile(6, 5), "core:resource/wine", 1, "core:improvement/plantation");
    w.deposit(w.tile(4, 5), "core:resource/wine", 1, "core:improvement/plantation");
    assert_eq!(w.ledger(P0).total(wine), 2);
    w.building(w.city, "core:building/bazaar");
    assert_eq!(w.ledger(P0).total(wine), 4, "each wine tile gives one more copy");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn a_feitoria_gives_portugal_an_untradeable_copy_of_the_city_states_luxuries() {
    let mut w = World::new();
    w.game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    let host = w.tile(15, 10);
    let host_city = w.game.debug_found_city(&w.rules, CS, host);
    let wine = w.resource("core:resource/wine");
    w.game.debug_set_resource_batch(&w.rules, &[(host, wine)]);
    assert_eq!(w.ledger(CS).total(wine), 1, "the city-state's own wine");
    let feitoria = w.improvement("core:improvement/feitoria");
    let site = w.tile(16, 10);
    assert_eq!(w.game.view().auth.map().owning_city(site), Some(host_city));
    w.game.debug_set_improvement_batch(&w.rules, &[(site, feitoria)]);
    assert_eq!(w.ledger(P0).total(wine), 0, "not Portugal: nothing");
    w.civ("core:nation/portugal");
    let ledger = w.ledger(P0);
    assert_eq!(ledger.total(wine), 1, "one copy of the city-state's wine");
    assert_eq!(ledger.tradeable(wine), 0, "that copy cannot be traded");
    assert_eq!(w.ledger(CS).total(wine), 1, "the city-state keeps its wine");
}

#[test]
fn the_exploration_finisher_reveals_hidden_antiquity_sites() {
    let mut w = World::new();
    assert!(!sees_hidden(w.game.view().auth, &w.rules, P0));
    w.policy("core:policy/exploration/finisher");
    assert!(sees_hidden(w.game.view().auth, &w.rules, P0));
}
