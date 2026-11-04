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
        .filter(|(i, _)| rules.production_gate(ProductionItem::Unit(UnitKindId(*i as u16)), obs.economy.nation).is_none())
        .collect();
    kinds.sort_by_key(|(i, u)| (u.domain != crate::rules::content::Domain::Land, *i));
    kinds.first().map(|(i, _)| UnitKindId(*i as u16))
}

// ---- `trade-fit` (classic/docs/ai-trade.md) ----
//
// `Variant::TradeFit`, `TurnBrief::trade_fit`: every city that chooses its
// order may take a trade unit while route slots are left (not only the most
// productive one); a route is valued by what it delivers at the allocator's
// prices, the food of an internal route at the growth it buys in its
// destination; a route near a hostile military unit or a barbarian camp is
// priced by its measured chance of loss (that share of its value and of the
// unit); a trade
// unit with no route from its city moves to the own city with the best one;
// a trade unit goes ahead of a building where it pays more; a technology that
// opens a route slot is worth the route in research.

use crate::ai::value::{Centi, Prices};
use crate::ids::{CityHandle, TileId};
use crate::world::trade::RouteKind;

/// Tiles from the straight way between a route's two cities within which a
/// hostile military unit or a barbarian camp makes the route a risk
/// (`HAZARD_LOSS_PERCENT`): the rules plunder a route whose unit meets an
/// enemy combat unit on its path (`derived::trade::enemy_on`), and a unit of
/// two moves a turn reaches the way within the route's first turns.
pub const HOSTILE_REACH: i32 = 3;
/// Bounds of a destination's food price, percent of the empire's average
/// one (`food_price_in`).
pub const FOOD_PRICE_MIN_PERCENT: i64 = 50;
pub const FOOD_PRICE_MAX_PERCENT: i64 = 200;
/// Chance (percent) that a route with a hazard near its way is lost before
/// its end, measured on `standard`'s routes (which ignore hazards; Quick,
/// solo, 80 games, `ai-trade`): 17.5 % of the routes with a hazard at their
/// start were lost on the standard suite, 18 % on the duel suite, against
/// 1.5 % and 4 % without one (classic/docs/ai-trade.md).
pub const HAZARD_LOSS_PERCENT: i64 = 18;

/// The tiles a route must keep away from: hostile military units seen now
/// (players at war, barbarians) and the barbarian camps the player knows.
#[derive(Debug, Clone, Default)]
pub struct Hazards {
    tiles: Vec<TileId>,
}

impl Hazards {
    pub fn of(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Self {
        let at_war = |p| obs.relations.iter().any(|r| r.other == p && r.at_war);
        let mut tiles: Vec<TileId> = obs
            .visible_foreign_units
            .iter()
            .filter(|u| !rules.is_civilian(u.kind) && at_war(u.owner))
            .map(|u| u.tile)
            .collect();
        if let Some(camp) = crate::systems::barbarians::camp_improvement(rules) {
            for i in 0..obs.view.tile_count() as u32 {
                if obs.view.tile(TileId(i)).snapshot().is_some_and(|s| s.improvement == Some(camp)) {
                    tiles.push(TileId(i));
                }
            }
        }
        Self { tiles }
    }

    /// A hazard lies within `HOSTILE_REACH` of the way from `a` to `b`
    /// (the hex ellipse: d(h, a) + d(h, b) ≤ d(a, b) + 2 × reach); only `a`
    /// when `b` is not known.
    pub fn near(&self, obs: &AIObservation<'_>, a: TileId, b: Option<TileId>) -> bool {
        let dims = obs.view.dimensions();
        let d = |x: TileId, y: TileId| dims.distance(x, y).unwrap_or(i32::MAX / 4);
        self.tiles.iter().any(|h| match b {
            Some(b) => d(*h, a) + d(*h, b) <= d(a, b) + 2 * HOSTILE_REACH,
            None => d(*h, a) <= HOSTILE_REACH,
        })
    }
}

/// The tile of a city the player knows: its own, or another's seen now.
fn city_tile(obs: &AIObservation<'_>, city: CityHandle) -> Option<TileId> {
    obs.own_cities
        .iter()
        .find(|c| c.handle == city)
        .map(|c| c.tile)
        .or_else(|| obs.visible_foreign_cities.iter().find(|c| c.handle == city).map(|c| c.tile))
}

/// The food of the step a city of `population` citizens with `surplus` whole
/// food a turn would grow last in `rounds` (the citizen a food more buys a
/// share of), as `value::growth_cost` counts it for the empire's average city.
fn last_citizen_food(obs: &AIObservation<'_>, population: u32, surplus: i32, rounds: u32) -> u32 {
    let threshold = |pop: u32| crate::systems::growth::speed_growth_threshold(&obs.speed, pop).max(1);
    let mut budget = surplus.max(0) as u32 * rounds;
    let mut step = population + 1;
    for _ in 0..64 {
        let price = threshold(step);
        if price > budget {
            return price;
        }
        budget -= price;
        step += 1;
    }
    threshold(step)
}

/// The price of a food a turn brought to `city` (centi-points): the
/// allocator's food price (the empire's average city) scaled by the growth
/// the food buys in this city: a city further from its next citizens buys
/// fewer of them. Bounded by `FOOD_PRICE_MIN_PERCENT` … `MAX`.
pub fn food_price_in(obs: &AIObservation<'_>, prices: &Prices, city: &OwnCity) -> Centi {
    let cities = obs.own_cities.len().max(1) as u32;
    let population: u32 = obs.own_cities.iter().map(|c| c.population).sum();
    let surplus: i32 = obs.own_cities.iter().map(|c| c.income.food_surplus.max(0)).sum();
    let average = last_citizen_food(obs, population / cities, surplus / cities as i32, prices.rounds_left);
    let here = last_citizen_food(obs, city.population, city.income.food_surplus, prices.rounds_left);
    let percent = (i64::from(average) * 100 / i64::from(here.max(1))).clamp(FOOD_PRICE_MIN_PERCENT, FOOD_PRICE_MAX_PERCENT);
    prices.food * percent / 100
}

/// What a route delivers at the allocator's prices (centi-points, a turn
/// kept to the last round): an internal route its food at the destination's
/// food price (`food_price_in`) or its production; an international one the
/// origin's gold and science and what it adds elsewhere (`side_value`, at the
/// gold price). The rules give a route no religious pressure.
pub fn fit_value(obs: &AIObservation<'_>, prices: &Prices, offer: &TradeOffer) -> Centi {
    let value = match offer.kind {
        RouteKind::Food => {
            let price = obs.own_cities.iter().find(|c| c.handle == offer.destination).map_or(prices.food, |c| food_price_in(obs, prices, c));
            i64::from(offer.destination_yields.food) * price
        }
        RouteKind::Production => i64::from(offer.destination_yields.production) * prices.production,
        RouteKind::International => {
            i64::from(offer.origin.gold) * prices.gold + i64::from(offer.origin.science) * prices.science + i64::from(offer.side_value) * prices.gold
        }
    };
    value / 100
}

/// The turns a route of `steps` steps runs before its unit comes back
/// (`derived::trade::circuits` circuits there and back at the domain's speed).
fn route_life(steps: u32, domain: crate::rules::stats::TradeDomain) -> i64 {
    let circuits = u32::from(crate::derived::trade::circuits(steps, domain));
    i64::from((circuits * 2 * steps.max(1)).div_ceil(crate::derived::trade::speed(domain)).max(1))
}

/// An offer's value net of its risk (centi-points, a turn kept to the last
/// round): `fit_value`, and where a hazard lies near the way
/// (`Hazards::near`) the route is lost with `HAZARD_LOSS_PERCENT` chance:
/// that share of its value, and the same share of the unit's cost (`cost`
/// hammers at the production price) spread over the route's life.
fn net_value(obs: &AIObservation<'_>, prices: &Prices, hazards: &Hazards, origin: TileId, offer: &TradeOffer, kind: UnitKindId, cost: u32, rules: &CompiledRuleset) -> Centi {
    let value = fit_value(obs, prices, offer);
    if !hazards.near(obs, origin, city_tile(obs, offer.destination)) {
        return value;
    }
    let domain = match rules.units[kind.0 as usize].domain {
        crate::rules::content::Domain::Sea => crate::rules::stats::TradeDomain::Sea,
        _ => crate::rules::stats::TradeDomain::Land,
    };
    let loss = i64::from(cost) * prices.production / route_life(offer.steps, domain);
    (value * (100 - HAZARD_LOSS_PERCENT) - loss * HAZARD_LOSS_PERCENT) / 100
}

/// The offer of a `kind` unit from `origin` (a city's tile) worth most net of
/// its risk (`net_value`; ties to the lower city, then kind); None when none
/// is worth something.
#[allow(clippy::too_many_arguments)]
fn best_offer<'o>(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, hazards: &Hazards, origin: &OwnCity, kind: UnitKindId, offers: &'o [TradeOffer]) -> Option<(&'o TradeOffer, Centi)> {
    let cost = obs.view.own_production_cost(origin.handle, ProductionItem::Unit(kind)).unwrap_or(rules.units[kind.0 as usize].cost);
    offers
        .iter()
        .map(|o| (o, net_value(obs, prices, hazards, origin.tile, o, kind, cost, rules)))
        .filter(|(_, v)| *v > 0)
        .max_by_key(|(o, v)| (*v, std::cmp::Reverse((o.destination.index, o.kind.code()))))
}

/// The orders of the trade units (`trade-fit`): a unit in a city starts its
/// route worth most net of its risk (`best_offer`); a unit whose city has
/// none worth something moves to the own city where a unit of its kind would
/// have the best (the rules' check: a cargo ship to a coastal one); otherwise
/// it waits.
pub fn fit_route_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices) -> Vec<Command> {
    if obs.economy.trade_offers.is_empty() {
        return Vec::new();
    }
    let hazards = Hazards::of(obs, rules);
    let mut out = Vec::new();
    // The rules run one route per pair of cities: a pair taken this turn is not offered again.
    let mut taken: Vec<(TileId, CityHandle)> = Vec::new();
    for (unit, offers) in &obs.economy.trade_offers {
        let Some(own) = obs.own_units.iter().find(|u| u.handle == *unit) else { continue };
        let here = obs.own_cities.iter().find(|c| c.tile == own.tile);
        let open: Vec<TradeOffer> = offers.iter().filter(|o| !taken.contains(&(own.tile, o.destination))).cloned().collect();
        if let Some((best, _)) = here.and_then(|c| best_offer(obs, rules, prices, &hazards, c, own.kind, &open)) {
            taken.push((own.tile, best.destination));
            out.push(Command::EstablishTradeRoute { unit: *unit, destination: best.destination, kind: best.kind });
            continue;
        }
        let home = obs
            .own_cities
            .iter()
            .filter(|c| c.tile != own.tile)
            .filter(|c| obs.view.accepts(&Command::ChangeTradeHome { unit: *unit, city: c.handle }))
            .filter_map(|c| {
                let prospects = obs.view.trade_prospects(c.handle, own.kind);
                best_offer(obs, rules, prices, &hazards, c, own.kind, &prospects).map(|(_, v)| (v, c))
            })
            .max_by_key(|(v, c)| (*v, std::cmp::Reverse(c.id)));
        if let Some((_, city)) = home {
            out.push(Command::ChangeTradeHome { unit: *unit, city: city.handle });
        }
    }
    out
}

/// The trade unit `city` could build now and the value of its best route
/// from there net of its risk (`trade-fit`, `best_offer`): each trade unit
/// kind the rules let the city build, one destination each (the rules run
/// one route per pair of cities): the trade units standing in the city or
/// queued there take the best ones first. None when no route slot is left
/// (the observation's, less `ordered` trade units ordered this turn) or no
/// destination is left worth something.
pub fn fit_prospect(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices, hazards: &Hazards, city: &OwnCity, ordered: u32) -> Option<(UnitKindId, Centi)> {
    if obs.economy.trade_routes_left <= ordered || city.queue.len() >= MAX_QUEUE {
        return None;
    }
    // The trade units already here (standing in the city or in its queue)
    // take its best destinations first: a new one gets the next.
    let is_trade = |k: UnitKindId| rules.units.get(k.0 as usize).is_some_and(|u| u.module.economy.trade);
    let here = obs.own_units.iter().filter(|u| u.tile == city.tile && is_trade(u.kind)).count()
        + city.queue.iter().filter(|i| matches!(i, ProductionItem::Unit(k) if is_trade(*k))).count();
    let known = |t: Option<crate::ids::TechId>| t.is_none_or(|t| obs.economy.techs.contains(&t));
    // Per destination, the best kind and value.
    let mut destinations: Vec<(CityHandle, UnitKindId, Centi)> = Vec::new();
    for (index, def) in rules.units.iter().enumerate() {
        let kind = UnitKindId(index as u16);
        if !def.module.economy.trade || !known(def.required_tech) || rules.production_gate(ProductionItem::Unit(kind), obs.economy.nation).is_some() {
            continue;
        }
        if !obs.view.accepts(&enqueue(obs, city, kind)) {
            continue;
        }
        let offers = obs.view.trade_prospects(city.handle, kind);
        let cost = obs.view.own_production_cost(city.handle, ProductionItem::Unit(kind)).unwrap_or(def.cost);
        for offer in &offers {
            let value = net_value(obs, prices, hazards, city.tile, offer, kind, cost, rules);
            match destinations.iter_mut().find(|(d, _, _)| *d == offer.destination) {
                Some(entry) if value > entry.2 => *entry = (offer.destination, kind, value),
                Some(_) => {}
                None => destinations.push((offer.destination, kind, value)),
            }
        }
    }
    destinations.retain(|(_, _, v)| *v > 0);
    destinations.sort_by_key(|(d, k, v)| (std::cmp::Reverse(*v), d.index, k.0));
    destinations.get(here).map(|(_, kind, value)| (*kind, *value))
}

/// The value of the route slots a technology opens, in the research
/// planner's weights (`planner_value::tech_value`: a yield a turn is ten
/// times its weight): each slot an internal food route into a city of the
/// empire's mean size (`derived::trade::internal_amount` of the first era,
/// a land route) once the empire has two cities to trade between.
pub fn slot_research_value(rules: &CompiledRuleset, tech: crate::ids::TechId, cities: &[crate::ai::planner::CityBrief], w: &crate::ai::planner_value::Weights) -> i32 {
    use crate::rules::effects::Modifier;
    use crate::rules::stats::{ClassicStat, Stat, TradeStat};
    if cities.len() < 2 {
        return 0;
    }
    let Some(def) = rules.technologies.get(tech.0 as usize) else { return 0 };
    let slots: i32 = def
        .effects
        .passive_player
        .iter()
        .filter_map(|e| match e.modifier {
            Modifier::AddStat { stat: Stat::Module(ClassicStat::Trade(TradeStat::Routes)), amount } => Some(amount.max(0)),
            _ => None,
        })
        .sum();
    if slots == 0 {
        return 0;
    }
    let mean = cities.iter().map(|c| c.population).sum::<u32>() / cities.len() as u32;
    let food = crate::derived::trade::internal_amount(0, mean) as i32;
    slots * food * w.food / 10
}

/// Trade units put ahead of a building (`trade-fit`): while route slots are
/// left, a city whose queue starts with a building (not a wonder) takes a
/// trade unit before it when the allocator would choose the unit over the
/// building now: its route's value per turn of production (`fit_prospect`,
/// `Prices::later` over the turns the unit takes with the city's stock)
/// beats the building's (`Prices::building` over its own turns). The
/// building keeps its place and the stock (the rules keep a city's stock
/// when its order changes); the best cities first, one unit per slot left.
pub fn fit_ahead_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset, prices: &Prices) -> Vec<Command> {
    let left = obs.economy.trade_routes_left;
    if left == 0 {
        return Vec::new();
    }
    let hazards = Hazards::of(obs, rules);
    let cities = obs.own_cities.len() as i32;
    let mut ahead: Vec<(Centi, &OwnCity, UnitKindId)> = Vec::new();
    for city in &obs.own_cities {
        let Some(ProductionItem::Building(head)) = city.queue.first().copied() else { continue };
        let def = &rules.buildings[head.0 as usize];
        if def.wonder || def.kind == crate::rules::content::BuildingKind::NationalWonder || city.queue.len() >= MAX_QUEUE {
            continue;
        }
        let Some((kind, value)) = fit_prospect(obs, rules, prices, &hazards, city, 0) else { continue };
        let unit_turns = crate::ai::planner_cities::turns(obs, rules, city, ProductionItem::Unit(kind));
        let head_turns = crate::ai::planner_cities::turns(obs, rules, city, ProductionItem::Building(head));
        if unit_turns == u32::MAX {
            continue;
        }
        let unit_rate = prices.later(value, unit_turns) / i64::from(unit_turns.max(1));
        let head_rate = if head_turns == u32::MAX { 0 } else { prices.building(obs, rules, city, head, cities, head_turns) / i64::from(head_turns.max(1)) };
        if unit_rate > head_rate {
            ahead.push((unit_rate, city, kind));
        }
    }
    ahead.sort_by_key(|(rate, city, _)| (std::cmp::Reverse(*rate), city.id));
    let mut out = Vec::new();
    for (_, city, kind) in ahead.into_iter().take(left as usize) {
        out.push(Command::ClearProduction { player: obs.player, city: city.handle });
        out.push(enqueue(obs, city, kind));
        out.extend(city.queue.iter().map(|item| Command::EnqueueProduction { player: obs.player, city: city.handle, item: *item }));
    }
    out
}
