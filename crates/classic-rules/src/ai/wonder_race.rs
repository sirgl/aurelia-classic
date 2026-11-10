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

/// The era of the technology that opens the wonder (the content era's order): a nation of an
/// earlier era cannot build it.
fn wonder_era(rules: &CompiledRuleset, kind: BuildingKindId) -> u8 {
    rules
        .buildings
        .get(kind.0 as usize)
        .and_then(|d| d.required_tech)
        .and_then(|t| rules.technologies.get(t.0 as usize))
        .and_then(|t| t.content_era)
        .and_then(|e| rules.eras.get(e.0 as usize))
        .map_or(0, |e| e.index)
}

/// The chance, percent, that an order of the wonder that takes `standard_turns` (Standard speed)
/// is finished before any other nation does it. The inputs are what the seat knows: the eras of
/// the nations it has met (`met_eras`, `PlayerQueries::met_eras`) against the wonder's era, and
/// the turns the order takes; the table (`wonder_chance`) is the bench's rate of won orders by
/// those two. A nation not met adds nothing the seat can know: the table's rows carry the average
/// of the unmet.
pub fn chance(rules: &CompiledRuleset, met_eras: &[(crate::ids::PlayerId, u8)], kind: BuildingKindId, standard_turns: i64) -> i64 {
    use crate::ai::wonder_chance::{CHANCE, TURNS};
    let era = wonder_era(rules, kind);
    let able = met_eras.iter().filter(|(_, e)| *e >= era).count().min(CHANCE[0].len() - 1);
    let row = TURNS.iter().position(|&t| standard_turns <= i64::from(t)).unwrap_or(TURNS.len() - 1);
    i64::from(CHANCE[row][able])
}
