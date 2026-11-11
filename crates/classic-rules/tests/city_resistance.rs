//! A captured city in resistance gives nothing (the owner's rule, 11 November): no yield of the
//! ruleset, no growth and no starvation, nothing built (its stock does not move), nothing
//! queued or bought there; when resistance ends everything returns.

use classic_content::core::core_rules;
use classic_rules::command::{CommandError, Refusal};
use classic_rules::derived::income::city_income;
use classic_rules::derived::yields::city_breakdown;
use classic_rules::ids::{CityHandle, PlayerId, ProductionItem, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::world::city::CityCore;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const GRASS: TerrainId = TerrainId(1);
const DESERT: TerrainId = TerrainId(3);

struct World {
    game: GameState,
    rules: CompiledRuleset,
    city: CityHandle,
}

impl World {
    /// A city of size 3 on `ground`, `resistance` turns in resistance.
    fn new(ground: TerrainId, resistance: u16) -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 1, width: 20, height: 20, rng_seed: 3 });
        let all: Vec<_> = (0..400).map(|i| (TileId(i), ground)).collect();
        game.debug_set_terrain_batch(&rules, &all);
        let dims = game.view().auth.map().dimensions;
        let city = game.debug_found_city(&rules, P0, dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap());
        game.debug_grow_city(&rules, city, 2);
        game.world_mut().cities.get_mut(city).unwrap().resistance_turns = resistance;
        World { game, rules, city }
    }
    fn core(&self) -> CityCore {
        self.game.view().auth.cities().get(self.city).unwrap().clone()
    }
    fn exec(&mut self, command: Command) -> Result<(), CommandError> {
        self.game.execute(&self.rules, command).map(|_| ())
    }
    fn end_turn(&mut self) {
        self.exec(Command::EndTurn { player: P0 }).unwrap();
    }
    fn warrior(&self) -> UnitKindId {
        UnitKindId(self.rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16)
    }
}

#[test]
fn a_city_in_resistance_gives_no_yield_of_the_ruleset_and_eats_nothing() {
    let w = World::new(GRASS, 3);
    let breakdown = city_breakdown(w.game.view().auth, &w.rules, &w.core());
    // Every channel the ruleset defines (`Yields` holds them all), not a list written here.
    assert_eq!(breakdown.rates, classic_rules::rules::content::Yields::default());
    assert_eq!(city_income(w.game.view().auth, &w.rules, &w.core()), Default::default());
    let free = World::new(GRASS, 0);
    let rates = city_breakdown(free.game.view().auth, &free.rules, &free.core()).rates;
    assert!(rates.food > 0 && rates.production > 0, "the same city gives something: {rates:?}");
}

#[test]
fn a_city_in_resistance_neither_grows_nor_starves_and_builds_nothing() {
    for (ground, label) in [(GRASS, "a growing city"), (DESERT, "a hungry city")] {
        let mut w = World::new(ground, 3);
        let kind = w.warrior();
        {
            let core = w.game.world_mut().cities.get_mut(w.city).unwrap();
            core.queue = vec![ProductionItem::Unit(kind)];
            core.production = 500;
            core.food_times100 = 250;
        }
        let before = w.core();
        w.end_turn();
        let after = w.core();
        assert_eq!(after.population, before.population, "{label}");
        assert_eq!(after.food_times100, before.food_times100, "{label}: the food store");
        assert_eq!(after.production, 500, "{label}: the stock does not move");
        assert_eq!(after.queue, before.queue, "{label}: nothing is built");
        assert_eq!(after.resistance_turns, 2, "{label}: one turn less, as before");
    }
}

#[test]
fn nothing_can_be_queued_or_bought_in_a_city_in_resistance() {
    let mut w = World::new(GRASS, 2);
    let kind = w.warrior();
    w.game.debug_earn(P0, 5_000);
    let gold = w.game.view().auth.players().treasury_of(P0);
    let refused = Err(CommandError::Refused(Refusal::CityInResistance { turns: 2 }));
    let city = w.city;
    assert_eq!(w.exec(Command::EnqueueProduction { player: P0, city, item: ProductionItem::Unit(kind) }), refused);
    w.game.world_mut().cities.get_mut(city).unwrap().queue = vec![ProductionItem::Unit(kind)];
    assert_eq!(w.exec(Command::HurryProduction { player: P0, city }), refused);
    let tile = w.game.view().auth.map().dimensions.to_tile(HexCoord { q: 8, r: 6 }).unwrap();
    assert_eq!(w.exec(Command::PurchaseTile { player: P0, city, tile }), refused);
    assert_eq!(w.game.view().auth.players().treasury_of(P0), gold, "no gold is charged");
}

/// The number the player sees (`resistance_turns`) is the rounds in which the city still gives
/// nothing, the current one included: a city with 3 gives nothing at the end of three of its
/// owner's turns, the number falls 3, 2, 1, and the fourth end of turn runs the city.
#[test]
fn the_number_shown_is_the_rounds_the_city_still_gives_nothing() {
    let mut w = World::new(GRASS, 3);
    let kind = w.warrior();
    {
        let core = w.game.world_mut().cities.get_mut(w.city).unwrap();
        core.queue = vec![ProductionItem::Unit(kind)];
        core.production = 3;
    }
    for shown in [3u16, 2, 1] {
        assert_eq!(w.core().resistance_turns, shown);
        assert_eq!(city_income(w.game.view().auth, &w.rules, &w.core()), Default::default(), "{shown} left: nothing is given now");
        w.end_turn();
        assert_eq!(w.core().production, 3, "{shown} left: this turn's end gave nothing");
    }
    assert_eq!(w.core().resistance_turns, 0);
    assert!(city_income(w.game.view().auth, &w.rules, &w.core()).production > 0, "the number shows 0: the city gives");
    w.end_turn();
    assert_ne!(w.core().production, 3, "the end of the first turn without resistance ran the city");
}

#[test]
fn when_resistance_ends_everything_returns() {
    // The same city with one turn of resistance left and with none: the resisting one gives
    // nothing at the end of this turn (it is the last of its rounds), the free one runs; at the
    // end of the next turn the two have run alike, and the cached yields follow the end.
    let (mut a, mut b) = (World::new(GRASS, 1), World::new(GRASS, 0));
    let kind = a.warrior();
    for w in [&mut a, &mut b] {
        let core = w.game.world_mut().cities.get_mut(w.city).unwrap();
        core.queue = vec![ProductionItem::Unit(kind)];
        core.production = 3;
    }
    a.end_turn();
    b.end_turn();
    assert_eq!(a.core().production, 3, "the resisting city's last round gives nothing");
    assert_ne!(b.core().production, 3);
    assert_eq!(a.core().resistance_turns, 0);
    a.end_turn();
    b.end_turn();
    let live = city_breakdown(a.game.view().auth, &a.rules, &a.core()).rates;
    assert!(live.food > 0);
    assert_eq!(a.game.derived().yields.of(a.city), Some(live), "the cached yields follow the end of resistance");
    assert!(a.core().production > 3, "everything returned: the city builds again");
}
