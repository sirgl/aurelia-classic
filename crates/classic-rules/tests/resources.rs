//! Strategic resources of the classic ruleset (resources:
//! availability, training requirements, the combat penalty): deposits with
//! quantities, the player's ledger (total, used, available), units and buildings that need
//! resources, deficits and the reveal technology.

use classic_content::core::core_rules;
use classic_rules::derived::resources::{
    deposit_amount, production_resources, resource_ledger, strategic_combat_penalty,
    unit_resource_penalty, ResourceLedger,
};
use classic_rules::ids::{
    BuildingKindId, CityHandle, ImprovementId, PlayerId, ProductionItem, ResourceId, TechId,
    TerrainId, TileId, UnitKindId,
};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const GRASS: TerrainId = TerrainId(1);

struct World {
    game: GameState,
    rules: CompiledRuleset,
}

impl World {
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(
            &rules,
            GameConfig {
                players: 2,
                width: 30,
                height: 20,
                rng_seed: 9,
            },
        );
        let all: Vec<_> = (0..600).map(|i| (TileId(i), GRASS)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        World { game, rules }
    }

    fn tile(&self, q: i32, r: i32) -> TileId {
        let dims = self.game.view().auth.map().dimensions;
        dims.to_tile(HexCoord { q, r }).unwrap()
    }

    fn city(&mut self, q: i32, r: i32) -> CityHandle {
        let t = self.tile(q, r);
        self.game.debug_found_city(&self.rules, P0, t)
    }

    fn resource(&self, key: &str) -> ResourceId {
        ResourceId(self.rules.resources.iter().position(|r| r.key == key).unwrap() as u16)
    }

    fn unit(&self, key: &str) -> UnitKindId {
        UnitKindId(self.rules.units.iter().position(|u| u.key == key).unwrap() as u16)
    }

    fn building(&self, key: &str) -> BuildingKindId {
        BuildingKindId(self.rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
    }

    fn improvement(&self, key: &str) -> ImprovementId {
        ImprovementId(self.rules.improvements.iter().position(|i| i.key == key).unwrap() as u16)
    }

    fn learn(&mut self, key: &str) {
        let t = self.rules.technologies.iter().position(|t| t.key == key).unwrap();
        self.game.debug_learn(&self.rules, P0, TechId(t as u16));
    }

    /// A resource on `tile`, with a mine (or `improvement`) on it.
    fn deposit(&mut self, tile: TileId, resource: &str, improvement: &str) {
        let r = self.resource(resource);
        self.game.debug_set_resource_batch(&self.rules, &[(tile, r)]);
        let i = self.improvement(improvement);
        self.game.debug_set_improvement_batch(&self.rules, &[(tile, i)]);
    }

    fn ledger(&self) -> ResourceLedger {
        resource_ledger(self.game.view().auth, &self.rules, P0)
    }

    fn check(&self, command: Command) -> Result<(), String> {
        let mut scratch = classic_rules::navigation::PathScratch::default();
        classic_rules::command::validate(self.game.view(), &self.rules, command, &mut scratch)
            .map(|_| ())
            .map_err(|e| format!("{e:?}"))
    }
}

#[test]
fn a_deposit_counts_once_revealed_and_connected() {
    let mut w = World::new();
    w.city(5, 5);
    let iron = w.resource("core:resource/iron");
    let at = w.tile(6, 5);
    w.deposit(at, "core:resource/iron", "core:improvement/mine");
    let amount = i32::from(deposit_amount(&w.rules, iron, at));
    assert!([2, 6].contains(&amount), "one of iron's quantities: {amount}");
    assert_eq!(w.ledger().total(iron), 0, "unknown before Bronze Working");
    w.learn("core:tech/iron-working");
    assert_eq!(w.ledger().total(iron), amount);
    assert_eq!(w.ledger().available(iron), amount);
    // The same deposit without its mine is not connected.
    let other = w.tile(5, 6);
    let r = w.resource("core:resource/iron");
    w.game.debug_set_resource_batch(&w.rules, &[(other, r)]);
    assert_eq!(w.ledger().total(iron), amount, "an unimproved deposit gives nothing");
}

#[test]
fn a_swordsman_needs_iron_and_holds_it() {
    let mut w = World::new();
    let city = w.city(5, 5);
    w.learn("core:tech/iron-working");
    w.learn("core:tech/iron-working");
    let sword = ProductionItem::Unit(w.unit("core:unit/swordsman"));
    let order = Command::EnqueueProduction {
        player: P0,
        city,
        item: sword,
    };
    let err = w.check(order.clone()).unwrap_err();
    assert!(err.contains("needs Iron"), "{err}");
    let at = w.tile(6, 5);
    w.deposit(at, "core:resource/iron", "core:improvement/mine");
    let iron = w.resource("core:resource/iron");
    let amount = w.ledger().total(iron);
    w.check(order).unwrap();
    // Every swordsman alive uses one iron; at the deposit's size the next
    // one is refused.
    let kind = w.unit("core:unit/swordsman");
    for _ in 0..amount {
        w.game.debug_spawn_unit(&w.rules, P0, at, kind);
    }
    assert_eq!(w.ledger().used(iron), amount);
    assert_eq!(w.ledger().available(iron), 0);
    assert_eq!(production_resources(w.game.view().auth, &w.rules, P0, sword), Err(iron));
}

#[test]
fn a_city_drops_an_order_it_lacks_resources_for() {
    let mut w = World::new();
    let city = w.city(5, 5);
    w.learn("core:tech/iron-working");
    w.learn("core:tech/iron-working");
    let at = w.tile(6, 5);
    w.deposit(at, "core:resource/iron", "core:improvement/mine");
    let kind = w.unit("core:unit/swordsman");
    let sword = ProductionItem::Unit(kind);
    w.game
        .execute(&w.rules, Command::EnqueueProduction { player: P0, city, item: sword })
        .unwrap();
    // Other swordsmen take all the iron before this one is done.
    let iron = w.resource("core:resource/iron");
    for _ in 0..w.ledger().total(iron) {
        w.game.debug_spawn_unit(&w.rules, P0, at, kind);
    }
    let before = w.game.view().auth.indexes().units_of(P0).len();
    w.game.execute(&w.rules, Command::EndTurn { player: P0 }).unwrap();
    let core = w.game.view().auth.cities().get(city).unwrap();
    assert!(!core.queue.contains(&sword), "the order is dropped");
    let after = w.game.view().auth.indexes().units_of(P0).len();
    assert!(after <= before + 1, "no swordsman beyond the iron (a warrior may come)");
    assert!(w.ledger().available(iron) >= 0, "no deficit from production");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn a_deficit_weakens_the_units_that_need_the_resource() {
    let mut w = World::new();
    w.city(5, 5);
    w.learn("core:tech/iron-working");
    let at = w.tile(6, 5);
    w.deposit(at, "core:resource/iron", "core:improvement/mine");
    let iron = w.resource("core:resource/iron");
    let total = w.ledger().total(iron);
    let kind = w.unit("core:unit/swordsman");
    // Debug spawns bypass production: more swordsmen than iron.
    let extra = 3;
    for _ in 0..total + extra {
        w.game.debug_spawn_unit(&w.rules, P0, at, kind);
    }
    let ledger = w.ledger();
    assert_eq!(ledger.available(iron), -extra);
    assert_eq!(ledger.deficits().collect::<Vec<_>>(), vec![iron]);
    // Unciv BattleConstants.MISSING_RESOURCES_MALUS: a flat -25 whatever the shortfall.
    let expected = -25;
    assert_eq!(strategic_combat_penalty(&ledger, &w.rules, kind), expected);
    let auth = w.game.view().auth;
    assert_eq!(unit_resource_penalty(auth, &w.rules, P0, kind), expected);
    let warrior = w.unit("core:unit/warrior");
    assert_eq!(unit_resource_penalty(auth, &w.rules, P0, warrior), 0, "a warrior needs nothing");
}

#[test]
fn the_combat_penalty_follows_the_rules_example() {
    // 6 iron for 10 swordsmen: Unciv's flat -25 (BattleConstants.MISSING_RESOURCES_MALUS),
    // not a share of the shortfall.
    let mut w = World::new();
    w.city(5, 5);
    w.learn("core:tech/iron-working");
    let iron = w.resource("core:resource/iron");
    let ring = [(6, 5), (5, 6), (4, 6), (4, 5), (5, 4), (6, 4)];
    let at = ring
        .iter()
        .map(|(q, r)| w.tile(*q, *r))
        .find(|t| deposit_amount(&w.rules, iron, *t) == 6)
        .expect("a 6-iron tile next to the city");
    w.deposit(at, "core:resource/iron", "core:improvement/mine");
    assert_eq!(w.ledger().total(iron), 6);
    let kind = w.unit("core:unit/swordsman");
    for _ in 0..10 {
        w.game.debug_spawn_unit(&w.rules, P0, at, kind);
    }
    assert_eq!(strategic_combat_penalty(&w.ledger(), &w.rules, kind), -25);
}

#[test]
fn a_factory_consumes_coal() {
    let mut w = World::new();
    let city = w.city(5, 5);
    let factory = w.building("core:building/factory");
    let coal = w.resource("core:resource/coal");
    w.learn("core:tech/industrialization");
    let item = ProductionItem::Building(factory);
    assert_eq!(production_resources(w.game.view().auth, &w.rules, P0, item), Err(coal));
    let at = w.tile(6, 5);
    w.deposit(at, "core:resource/coal", "core:improvement/mine");
    let total = w.ledger().total(coal);
    assert!(total > 0);
    w.game.debug_add_building(&w.rules, city, factory);
    assert_eq!(w.ledger().used(coal), 1);
    assert_eq!(w.ledger().available(coal), total - 1);
}

#[test]
fn buying_needs_the_resources_too() {
    let mut w = World::new();
    let city = w.city(5, 5);
    w.learn("core:tech/iron-working");
    w.learn("core:tech/iron-working");
    let at = w.tile(6, 5);
    w.deposit(at, "core:resource/iron", "core:improvement/mine");
    let kind = w.unit("core:unit/swordsman");
    let sword = ProductionItem::Unit(kind);
    w.game
        .execute(&w.rules, Command::EnqueueProduction { player: P0, city, item: sword })
        .unwrap();
    w.game.debug_earn(P0, 5_000);
    w.check(Command::HurryProduction { player: P0, city }).unwrap();
    let iron = w.resource("core:resource/iron");
    for _ in 0..w.ledger().total(iron) {
        w.game.debug_spawn_unit(&w.rules, P0, at, kind);
    }
    let err = w.check(Command::HurryProduction { player: P0, city }).unwrap_err();
    assert!(err.contains("needs Iron"), "{err}");
}

/// Oracle: the ledger equals a count over every tile of the map (owned by
/// the player's cities, revealed, connected) and every unit and building.
#[test]
fn the_ledger_matches_a_count_over_the_whole_map() {
    let rules = compile_rules(core_rules()).unwrap();
    let config = classic_rules::scenario::ScenarioConfig {
        seed: 0x7E50,
        players: 3,
        width: 40,
        height: 26,
        cities: 0,
        units: 0,
        water_ratio: 25,
    };
    let mut game = classic_rules::scenario::generate_new_game(&rules, &config);
    let all: Vec<TechId> = (0..rules.technologies.len()).map(|i| TechId(i as u16)).collect();
    for p in 0..3 {
        for t in &all {
            game.debug_learn(&rules, PlayerId(p), *t);
        }
    }
    // A city next to every fifth resource tile (owner by turns).
    let sites: Vec<TileId> = {
        let map = game.view().auth.map();
        (0..map.tile_count() as u32)
            .map(TileId)
            .filter(|t| map.resource(*t).flatten().is_some())
            .step_by(5)
            .collect()
    };
    for (i, site) in sites.into_iter().enumerate() {
        let auth = game.view().auth;
        if auth.indexes().city_on_tile(site).is_some() || auth.map().owner(site).flatten().is_some() {
            continue;
        }
        game.debug_found_city(&rules, PlayerId((i % 3) as u8), site);
    }
    // Every other resource tile gets its improvement.
    let improved: Vec<(TileId, ImprovementId)> = {
        let map = game.view().auth.map();
        (0..map.tile_count() as u32)
            .map(TileId)
            .filter_map(|t| {
                let r = map.resource(t).flatten()?;
                let first = *rules.resources[r.0 as usize].improvements.first()?;
                (t.0 % 2 == 0).then_some((t, first))
            })
            .collect()
    };
    game.debug_set_improvement_batch(&rules, &improved);
    let auth = game.view().auth;
    let mut deposits = 0;
    for p in 0..3u8 {
        let player = PlayerId(p);
        let ledger = resource_ledger(auth, &rules, player);
        let mut total = vec![0i32; rules.resources.len()];
        for t in 0..auth.map().tile_count() as u32 {
            let tile = TileId(t);
            let Some(Some(r)) = auth.map().resource(tile) else {
                continue;
            };
            let Some(city) = auth.map().owning_city(tile) else {
                continue;
            };
            let core = auth.cities().get(city).unwrap();
            if core.owner != player {
                continue;
            }
            let def = &rules.resources[r.0 as usize];
            let improved = auth
                .map()
                .improvement(tile)
                .flatten()
                .is_some_and(|i| def.improvements.contains(&i));
            if improved || core.tile == tile {
                total[r.0 as usize] += i32::from(auth.map().resource_amount(tile));
                deposits += 1;
            }
        }
        for (i, expected) in total.iter().enumerate() {
            assert_eq!(ledger.total(ResourceId(i as u16)), *expected, "player {p} resource {i}");
        }
    }
    assert!(deposits > 0, "the generated cities stand on or own resources");
}

#[test]
fn deposits_survive_save_and_load() {
    let rules = compile_rules(core_rules()).unwrap();
    let config = classic_rules::scenario::ScenarioConfig {
        seed: 0x5A7E,
        players: 2,
        width: 40,
        height: 26,
        cities: 2,
        units: 0,
        water_ratio: 25,
    };
    let game = classic_rules::scenario::generate_new_game(&rules, &config);
    let auth = game.view().auth;
    let big = (0..auth.map().tile_count() as u32)
        .filter(|t| auth.map().resource_amount(TileId(*t)) > 1)
        .count();
    assert!(big > 0, "strategic deposits hold several copies");
    let save = classic_rules::persistence::encode::encode(&game, &rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &rules).unwrap();
    for t in 0..auth.map().tile_count() as u32 {
        let tile = TileId(t);
        assert_eq!(
            loaded.view().auth.map().resource_amount(tile),
            auth.map().resource_amount(tile)
        );
    }
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
}
