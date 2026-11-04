//! The abilities of units the scripted AI never used, priced where they pay
//! (`Variant::UnitAbilities`, classic/docs/ai-unit-abilities.md).
//!
//! ```text
//! great person that may act      every city-state it may reach (met, at peace,
//! on a city-state's land         its capital known): the trade mission's gold
//! (trade mission, buying it)     and, for a unit that buys city-states, the
//!                                city it would buy (its worth less the unrest of
//!                                a puppet), each when the unit gets there
//!   best errand > its            on the city-state's land: the action; else the
//!   improvement at home          walk to the nearest of its tiles the rules let
//!                                it march to
//!   else                         as the standard (`great_people::unit_order`)
//! a cargo to sell                sold when the rules accept the sale (gold and
//!                                experience for nothing but the cargo)
//! ```
//!
//! What a unit may do is the rules' data: the trade mission belongs to a great
//! person with gold of a mission (`UnitAbilities::base_gold`), the purchase to a
//! unit with `UnitAbility::BuyCityState`, the sale is whatever the rules accept
//! (`Command::SellExoticGoods`). The actions that come first in the standard
//! chooser (a golden age, a discovery, a hurry) stay first.
//!
//! Not priced: the influence of a mission and the bonuses of an alliance it may
//! bring or a purchase may end, a unit's own bonus to the gold of a mission
//! (a promotion; the observation does not read it), and a policy's.

use crate::ai::great_people::{great_person_action, unit_order};
use crate::ai::planner::{Decision, UnitView};
use crate::ai::system::AdvisorInput;
use crate::ai::trace::{Actor, Reason};
use crate::ai::value::{Centi, Prices};
use crate::classic::content::UnitAbility;
use crate::command::Command;
use crate::derived::great_people::{great_person_of, GreatPersonAction};
use crate::ids::{PlayerId, TileId};
use crate::knowledge::observation::{AIObservation, CityStateInfo, OwnUnit};
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;

/// How far from a city-state's capital its land is looked for.
const LAND_RADIUS: i32 = 3;
/// The tiles of a city-state's land tried as the end of a walk, nearest first.
const LAND_TRIES: usize = 4;
/// Citizens of a city-state's capital the player has not seen.
const UNSEEN_POPULATION: u32 = 1;

/// Orders for every own great person: an errand to a city-state when it is
/// worth more than what the standard chooser does with the unit, else that.
/// Without prices, the standard's orders.
pub fn great_person_commands(input: &AdvisorInput<'_, '_>) -> Vec<Command> {
    let obs = input.observation;
    let rules = input.rules;
    let Some(prices) = input.brief.prices.as_ref() else {
        return crate::ai::great_people::great_person_commands(obs, rules);
    };
    let mut taken: Vec<TileId> = Vec::new();
    let mut bought: Vec<PlayerId> = Vec::new();
    let mut out = Vec::new();
    for unit in &obs.own_units {
        if let Some(errand) = errand(input, prices, unit, &taken, &bought) {
            taken.push(errand.tile);
            if errand.action == GreatPersonAction::BuyCityState {
                bought.push(errand.city_state);
            }
            out.push(errand.command);
            continue;
        }
        out.extend(unit_order(obs, rules, unit, &mut taken));
    }
    out
}

/// A great person's order toward a city-state.
#[derive(Debug, Clone)]
pub struct Errand {
    pub city_state: PlayerId,
    pub action: GreatPersonAction,
    /// Where the action is done (the unit's tile when it is there).
    pub tile: TileId,
    pub value: Centi,
    /// The action, or the walk to `tile`.
    pub command: Command,
}

/// The best errand of `unit` to a city-state when it beats the unit's
/// improvement; None: a unit that does neither, an action of the standard
/// chooser that comes first, or nothing worth more than the improvement.
pub fn errand(input: &AdvisorInput<'_, '_>, prices: &Prices, unit: &OwnUnit, taken: &[TileId], bought: &[PlayerId]) -> Option<Errand> {
    let obs = input.observation;
    let rules = input.rules;
    let gp = great_person_of(rules, unit.kind)?;
    let def = rules.units.get(unit.kind.0 as usize)?;
    let economy = def.module.economy;
    let buys = def.module.abilities.contains(&UnitAbility::BuyCityState);
    if economy.base_gold == 0 && !buys {
        return None;
    }
    if great_person_action(obs, rules, unit).is_some_and(|a| !matches!(a, GreatPersonAction::TradeMission | GreatPersonAction::Construct(_))) {
        return None;
    }
    let home = gp.improvements.iter().map(|i| improvement_value(prices, rules, *i)).max().unwrap_or(0);
    let mission = economy.base_gold + economy.gold_per_era * u32::from(obs.view.own_era());
    let moves = u32::from(def.movement.max(1));
    let dims = obs.view.dimensions();
    let mut best: Option<Errand> = None;
    for city_state in obs.city_states.iter().filter(|c| !c.at_war) {
        let here = owner(obs, unit.tile) == Some(city_state.player);
        let Some(tile) = (if here { Some(unit.tile) } else { land_to_walk(obs, city_state, unit, taken) }) else {
            continue;
        };
        let delay = dims.distance(unit.tile, tile).unwrap_or(0).max(0) as u32;
        let delay = delay.div_ceil(moves);
        let mut options = Vec::new();
        if economy.base_gold > 0 {
            options.push((GreatPersonAction::TradeMission, mission_value(prices, mission)));
        }
        if buys && !bought.contains(&city_state.player) {
            options.push((GreatPersonAction::BuyCityState, purchase_value(input, prices, city_state, delay)));
        }
        for (action, value) in options {
            if value <= home || best.as_ref().is_some_and(|b| b.value >= value) {
                continue;
            }
            let command = if here {
                Command::GreatPersonAction { unit: unit.handle, action }
            } else {
                Command::MarchTo { unit: unit.handle, destination: tile }
            };
            if !here || obs.view.accepts(&command) {
                best = Some(Errand { city_state: city_state.player, action, tile, value, command });
            }
        }
    }
    best
}

/// The owner of a known tile.
fn owner(obs: &AIObservation<'_>, tile: TileId) -> Option<PlayerId> {
    obs.view.tile(tile).snapshot().and_then(|s| s.owner)
}

/// The nearest tile of the city-state's land (not its city, free of own units,
/// not taken) the unit may march to; None when its capital is unknown.
fn land_to_walk(obs: &AIObservation<'_>, city_state: &CityStateInfo, unit: &OwnUnit, taken: &[TileId]) -> Option<TileId> {
    let capital = city_state.capital?;
    let dims = obs.view.dimensions();
    let mut tiles: Vec<(i32, TileId)> = crate::math::hex_disc(dims, capital, LAND_RADIUS)
        .filter(|t| {
            let snapshot = obs.view.tile(*t).snapshot().copied();
            snapshot.is_some_and(|s| s.owner == Some(city_state.player) && !s.city)
                && !taken.contains(t)
                && !obs.own_units.iter().any(|u| u.tile == *t && u.handle != unit.handle)
        })
        .map(|t| (dims.distance(unit.tile, t).unwrap_or(i32::MAX), t))
        .collect();
    tiles.sort_by_key(|(d, t)| (*d, t.0));
    tiles
        .into_iter()
        .take(LAND_TRIES)
        .map(|(_, t)| t)
        .find(|t| obs.view.accepts(&Command::MarchTo { unit: unit.handle, destination: *t }))
}

/// One-off gold: a gold a turn to the end is `prices.gold`.
pub fn mission_value(prices: &Prices, gold: u32) -> Centi {
    i64::from(gold) * prices.gold / i64::from(prices.rounds_left.max(1))
}

/// A city-state bought `delay` rounds from now: a city of its size from then on
/// (`operations::city_worth`) less the unrest of a puppet of that size at the
/// empire's own rates, priced by the moods it crosses.
pub fn purchase_value(input: &AdvisorInput<'_, '_>, prices: &Prices, city_state: &CityStateInfo, delay: u32) -> Centi {
    let population = city_state.capital.map_or(UNSEEN_POPULATION, |capital| population_of(input, capital));
    let citizens = i64::from(population.max(1));
    let unrest = prices.unrest_by_steps(prices.unhappiness.city + citizens * prices.unhappiness.citizen);
    crate::ai::operations::city_worth(prices, population, delay) - unrest
}

/// The citizens of the city on `tile`: seen now, else as last seen (the AI's
/// memory, then the player's), else `UNSEEN_POPULATION`.
fn population_of(input: &AdvisorInput<'_, '_>, tile: TileId) -> u32 {
    let obs = input.observation;
    obs.visible_foreign_cities
        .iter()
        .find(|c| c.tile == tile)
        .map(|c| c.population)
        .or_else(|| input.state.seen_cities().iter().find(|c| c.tile == tile).map(|c| c.population))
        .or_else(|| obs.view.remembered_cities().find(|(t, _)| *t == tile).and_then(|(_, s)| s.seen.as_ref().map(|c| c.population)))
        .unwrap_or(UNSEEN_POPULATION)
}

/// An improvement's own yields a turn, worked to the end.
pub fn improvement_value(prices: &Prices, rules: &CompiledRuleset, improvement: crate::ids::ImprovementId) -> Centi {
    let Some(def) = rules.improvements.get(improvement.0 as usize) else {
        return 0;
    };
    let y = def.yields;
    i64::from(y.food) * prices.food
        + i64::from(y.production) * prices.production
        + i64::from(y.gold) * prices.gold
        + i64::from(y.science) * prices.science
        + i64::from(y.culture) * prices.culture
        + i64::from(y.module) * prices.faith
}

/// A unit's sale of its cargo when the rules accept it.
pub fn sale(view: &UnitView<'_, '_>) -> Option<Decision> {
    let unit = &view.local.unit;
    let command = Command::SellExoticGoods { unit: unit.handle };
    view.local.view.accepts(&command).then(|| Decision::act(Actor::Unit(unit.id), Reason::SystemAdvisor, command))
}
