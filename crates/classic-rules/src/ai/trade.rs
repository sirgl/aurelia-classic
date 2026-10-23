//! Trade route decisions from the observation (`OwnEconomy::trade_offers`:
//! the routes the trade route screen offers each own trade unit).
//!
//! ```text
//! trade unit in a city   the best offer: origin gold + 2 × origin science,
//!                        or the food / production an internal route
//!                        brings, plus what it adds elsewhere (a new
//!                        trading partner, a route to a city-state:
//!                        `TradeOffer::side_value`); ties to the lower
//!                        city, then kind
//! routes left            the most productive city with a non-empty queue
//!                        queues a trade unit it can build (a caravan; a
//!                        cargo ship only where no caravan can be built)
//!                        once the empire has two cities and one of them
//!                        has TRADE_FROM_POPULATION citizens
//! ```

use crate::command::Command;
use crate::derived::trade::TradeOffer;
use crate::ids::{ProductionItem, UnitKindId};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;
use crate::world::city::MAX_QUEUE;

/// Population of the largest city before the first trade unit.
pub const TRADE_FROM_POPULATION: u32 = 8;

/// What an offer is worth to the AI (hundredths of yields).
pub fn offer_score(offer: &TradeOffer) -> u32 {
    offer.origin.gold
        + 2 * offer.origin.science
        + offer.destination_yields.food
        + offer.destination_yields.production
        + offer.side_value
}

/// EstablishTradeRoute for every own trade unit with an offer.
pub fn route_commands(obs: &AIObservation<'_>) -> Vec<Command> {
    obs.economy
        .trade_offers
        .iter()
        .filter_map(|(unit, offers)| {
            let best = offers
                .iter()
                .max_by_key(|o| (offer_score(o), std::cmp::Reverse((o.destination.index, o.kind.code()))))?;
            Some(Command::EstablishTradeRoute {
                unit: *unit,
                destination: best.destination,
                kind: best.kind,
            })
        })
        .collect()
}

/// A trade unit order while routes are left and none is idle or queued.
pub fn production_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    // Trade comes after the first growth: two cities, one of them at
    // TRADE_FROM_POPULATION (the early production goes to settlers,
    // shrines and defenders).
    let grown = obs.own_cities.iter().any(|c| c.population >= TRADE_FROM_POPULATION);
    let ready = obs.own_cities.len() >= 2 && grown;
    if obs.economy.trade_routes_left == 0 || !obs.economy.trade_offers.is_empty() || !ready {
        return Vec::new();
    }
    let Some(kind) = trade_unit(obs, rules) else {
        return Vec::new();
    };
    let city = obs
        .own_cities
        .iter()
        .filter(|c| !c.queue.is_empty() && c.queue.len() < MAX_QUEUE)
        // A cargo ship is built in a coastal city only.
        .filter(|c| obs.view.accepts(&enqueue(obs, c, kind)))
        .max_by_key(|c| (c.income.production, std::cmp::Reverse(c.id)));
    city.map(|c| enqueue(obs, c, kind)).into_iter().collect()
}

fn enqueue(obs: &AIObservation<'_>, city: &OwnCity, kind: UnitKindId) -> Command {
    Command::EnqueueProduction {
        player: obs.player,
        city: city.handle,
        item: ProductionItem::Unit(kind),
    }
}

/// The trade unit the player can build: land first.
pub(crate) fn trade_unit(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Option<UnitKindId> {
    let known = |t: Option<crate::ids::TechId>| t.is_none_or(|t| obs.economy.techs.contains(&t));
    let mut kinds: Vec<(usize, &crate::rules::compiled::UnitDefinition)> = rules
        .units
        .iter()
        .enumerate()
        .filter(|(_, u)| u.module.economy.trade && known(u.required_tech))
        .filter(|(i, _)| rules.production_gate(ProductionItem::Unit(UnitKindId(*i as u16)), obs.economy.civ).is_none())
        .collect();
    kinds.sort_by_key(|(i, u)| (u.domain != crate::rules::content::Domain::Land, *i));
    kinds.first().map(|(i, _)| UnitKindId(*i as u16))
}
