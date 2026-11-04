//! The allocator's true effects (`Prices::true_effects`,
//! classic/docs/ai-effects.md) on the real rules: an effect is priced by what
//! it changes in the city. The effects are picked by scanning the rules for
//! their kind, never by name: a tile-conditional yield is worth the matching
//! tiles of the territory, a stat gated by the city's state nothing outside
//! that state, a one-off grant what it gives, once.

use classic_content::core::core_rules;
use classic_rules::ai::value::{PriceModel, Prices, SciencePricing};
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, ResourceId, TerrainId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::rules::effects::{CompiledEffect, Condition, Grant, Modifier, Selector, Trigger};
use classic_rules::rules::stats::{ClassicStat, SocietyStat, Stat};
use classic_rules::world::city::CityStatus;
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);

struct Scene {
    game: GameState,
    rules: CompiledRuleset,
    city: CityHandle,
}

impl Scene {
    /// A 24×24 map of the first flat land terrain a city stands on, two
    /// players, one city of P0 at (8, 8) grown to `population`.
    fn new(population: u32) -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 2, width: 24, height: 24, rng_seed: 5 });
        let land = rules
            .terrains
            .iter()
            .filter(|t| !t.water && t.can_found_city && !t.impassable)
            .filter_map(|t| t.map_code)
            .find(|code| !rules.terrain_kinds.is_hills(*code))
            .expect("a flat land terrain");
        let all: Vec<_> = (0..24 * 24).map(|i| (TileId(i), TerrainId(land))).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let centre = game.view().auth.map().dimensions.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
        let city = game.debug_found_city(&rules, P0, centre);
        game.debug_set_population(&rules, city, population);
        Scene { game, rules, city }
    }

    fn at(&self, q: i32, r: i32) -> TileId {
        self.game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
    }

    /// `resource` on `tiles`, its reveal technology learned.
    fn place(&mut self, resource: ResourceId, tiles: &[TileId]) {
        if let Some(tech) = self.rules.resources[resource.0 as usize].required_tech {
            self.game.debug_learn(&self.rules, P0, tech);
        }
        let batch: Vec<_> = tiles.iter().map(|t| (*t, resource)).collect();
        self.game.debug_set_resource_batch(&self.rules, &batch);
    }

    /// The price of `effect` of a building of the city, with and without
    /// the true effects, at the prices of `prices_from` (one set of prices
    /// for the scenes compared).
    fn price(&self, effect: &CompiledEffect, prices: &Prices) -> (i64, i64) {
        let obs = self.game.ai_observation(&self.rules, P0);
        let city = obs.own_cities.iter().find(|c| c.handle == self.city).expect("the city");
        let mut truly = *prices;
        truly.true_effects = true;
        let mut planner = *prices;
        planner.true_effects = false;
        (truly.effect_in(&obs, &self.rules, city, effect, 1), planner.effect_in(&obs, &self.rules, city, effect, 1))
    }

    fn prices(&self) -> Prices {
        let obs = self.game.ai_observation(&self.rules, P0);
        Prices::of_effects(&obs, &self.rules, PriceModel::Uses, SciencePricing::Reach, true)
    }
}

/// The first building effect (by building index) that `pick` accepts.
fn first_effect(rules: &CompiledRuleset, pick: impl Fn(&CompiledEffect) -> bool) -> (BuildingKindId, CompiledEffect) {
    for (index, def) in rules.buildings.iter().enumerate() {
        let set = &def.effects;
        let all = set.passive_player.iter().chain(&set.passive_city).chain(&set.passive_unit).chain(&set.passive_tile).chain(set.triggered.iter().flatten());
        for effect in all {
            if pick(effect) {
                return (BuildingKindId(index as u16), *effect);
            }
        }
    }
    panic!("no building effect of the kind in the rules");
}

/// A flat yield a building adds on the tiles of its city that hold a
/// resource: nothing in a city without such a tile (the planner priced one
/// tile in every city), the yield on each worked one and a share on the
/// others with one. The planner's price ignores the territory.
#[test]
fn a_tile_conditional_yield_is_worth_the_matching_tiles_of_the_territory() {
    let rules = compile_rules(core_rules()).unwrap();
    let (_, effect) = first_effect(&rules, |e| {
        e.trigger == Trigger::Passive
            && e.selector == Selector::SourceCityTiles
            && matches!(e.condition, Condition::TileResource(_))
            && e.second_condition == Condition::Always
            && matches!(e.modifier, Modifier::AddYield { amount, .. } if amount > 0)
    });
    let Condition::TileResource(resource) = effect.condition else { unreachable!() };
    let Modifier::AddYield { yield_kind, amount } = effect.modifier else { unreachable!() };

    let bare = Scene::new(4);
    let prices = bare.prices();
    let per_tile = prices.of_yield(yield_kind) * i64::from(amount);
    let (without, planner_without) = bare.price(&effect, &prices);
    assert_eq!(without, 0, "no tile of the territory holds the resource");
    assert!((planner_without - per_tile).abs() < 10 * i64::from(amount), "the planner's price: one tile ({planner_without}, {per_tile})");

    let mut rich = Scene::new(4);
    let tiles = [rich.at(9, 8), rich.at(8, 10), rich.at(6, 9)];
    rich.place(resource, &tiles);
    let (with, planner_with) = rich.price(&effect, &prices);
    assert_eq!(planner_with, planner_without, "the planner's price ignores the territory");
    assert!(with > 0, "three tiles of the territory hold it: {with}");
    assert!(with <= 3 * per_tile, "at most the yield on each of the three: {with} > {}", 3 * per_tile);

    // A worked tile counts in full: the city works the first one.
    let mut worked = Scene::new(1);
    worked.place(resource, &tiles[..1]);
    let obs = worked.game.ai_observation(&worked.rules, P0);
    let works = obs.own_cities[0].worked.contains(&tiles[0]);
    let (one, _) = worked.price(&effect, &prices);
    if works {
        assert_eq!(one, per_tile, "a worked matching tile counts in full");
    } else {
        assert!(one < per_tile, "an open matching tile counts at the spare citizens' share");
    }
}

/// A stat the rules apply only to an occupied city (an exemption from the
/// unhappiness of occupation): nothing in the player's own city, the
/// occupation's unhappiness in an occupied one — more in a bigger one.
#[test]
fn a_stat_gated_by_occupation_acts_only_in_an_occupied_city() {
    let rules = compile_rules(core_rules()).unwrap();
    let gate = Stat::Module(ClassicStat::Society(SocietyStat::NoOccupiedUnhappiness));
    let (_, effect) = first_effect(&rules, |e| e.trigger == Trigger::Passive && matches!(e.modifier, Modifier::AddStat { stat, .. } if stat == gate));

    let own = Scene::new(4);
    // A point of happiness worth something (a young empire's may be worth
    // nothing yet).
    let mut prices = own.prices();
    prices.happiness = 1_000;
    let (in_own, planner) = own.price(&effect, &prices);
    assert_eq!(in_own, 0, "an own city is never occupied: the exemption changes nothing");
    assert!(planner > 0, "the planner's price is the same in any city: {planner}");

    let mut occupied = Scene::new(4);
    occupied.game.debug_set_city_status(occupied.city, CityStatus::Occupied);
    let (in_occupied, planner_occupied) = occupied.price(&effect, &prices);
    assert_eq!(planner_occupied, planner);
    assert!(in_occupied > 0, "an occupied city: {in_occupied}");

    let mut bigger = Scene::new(8);
    bigger.game.debug_set_city_status(bigger.city, CityStatus::Occupied);
    let (in_bigger, _) = bigger.price(&effect, &prices);
    assert!(in_bigger > in_occupied, "occupied citizens cost more: {in_bigger} > {in_occupied}");

    let mut puppet = Scene::new(4);
    puppet.game.debug_set_city_status(puppet.city, CityStatus::Puppet);
    assert_eq!(puppet.price(&effect, &prices).0, 0, "a puppet is not occupied");
}

/// A building given free on completion is worth that building in the city,
/// once — nothing where the city has it; a free technology a technology's
/// worth at the beaker price, whatever the number of cities.
#[test]
fn a_one_off_grant_is_priced_once_by_what_it_gives() {
    let rules = compile_rules(core_rules()).unwrap();
    let (_, free) = first_effect(&rules, |e| e.trigger == Trigger::OnBuilt && matches!(e.modifier, Modifier::FreeBuilding(_)));
    let Modifier::FreeBuilding(given) = free.modifier else { unreachable!() };

    let scene = Scene::new(4);
    let prices = scene.prices();
    let obs = scene.game.ai_observation(&scene.rules, P0);
    let city = &obs.own_cities[0];
    let (grant, _) = scene.price(&free, &prices);
    let mut truly = prices;
    truly.true_effects = true;
    assert_eq!(grant, truly.building(&obs, &scene.rules, city, given, 1, 0), "the free building's own value in the city");

    let mut holding = Scene::new(4);
    holding.game.debug_add_building(&holding.rules, holding.city, given);
    assert_eq!(holding.price(&free, &prices).0, 0, "a city that has it gains nothing");

    let (_, tech) = first_effect(&rules, |e| e.trigger == Trigger::OnBuilt && matches!(e.modifier, Modifier::Grant(Grant::FreeTechnologies(_))));
    let Modifier::Grant(Grant::FreeTechnologies(n)) = tech.modifier else { unreachable!() };
    let worth = truly.technology(&obs, &scene.rules);
    assert!(worth > 0);
    assert_eq!(truly.effect_in(&obs, &scene.rules, city, &tech, 1), i64::from(n) * worth);
    assert_eq!(truly.effect_in(&obs, &scene.rules, city, &tech, 12), i64::from(n) * worth, "once for the empire, not per city");
}
