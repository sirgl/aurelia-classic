//! The wonder race (`Variant::WonderCancel`, `Variant::WonderRace`, `TurnBrief::wonder_race`).
//!
//! What the rules do with an order of a world wonder that another nation completed first: the
//! city's stock is spent on the head when it reaches the cost and the wonder is not built
//! (`kernel` `lost_wonder_race`: the order is consumed without a reward). The plain AI never looks
//! at a queue again once it holds an item (`planner_cities::QUEUE_TARGET`), so a lost wonder stays
//! at the head until the stock reaches its cost and burns it (rival/REPORT.md: 12 % of the seats'
//! production in the bench). A stock that is not yet spent stays in the city and goes to the next
//! order whole, so dropping a lost order at once costs nothing.
//!
//! 1. `lost`: the wonder is built elsewhere (the announcement every player gets,
//!    `AIObservation::built_wonders`) and not in this city: the order is dropped and the hurry
//!    price is not paid for it.
//! 2. `chance`: the price of a world wonder is the chance to finish it first, from what the seat
//!    knows (the met nations' eras, `TurnBrief::met_eras`, the nations not met) and a table
//!    fitted to the bench (`wonder_chance`).

use crate::ids::{BuildingKindId, ProductionItem};
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::CompiledRuleset;

/// A world wonder.
pub fn is_wonder(rules: &CompiledRuleset, kind: BuildingKindId) -> bool {
    rules.buildings.get(kind.0 as usize).is_some_and(|d| d.wonder)
}

/// The order is a world wonder another nation (or city) has completed: nothing in this city can
/// finish it any more.
pub fn lost(obs: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, item: ProductionItem) -> bool {
    match item {
        ProductionItem::Building(kind) => is_wonder(rules, kind) && obs.built_wonders.contains(&kind) && !city.buildings.contains(&kind),
        ProductionItem::Unit(_) | ProductionItem::Project(_) => false,
    }
}
