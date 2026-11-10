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

/// What the seat knows of the race, taken once a turn (`Allocator::new`).
#[derive(Debug, Clone, Default)]
pub struct Known {
    /// The eras of the nations met (`PlayerQueries::met_eras`).
    met_eras: Vec<(crate::ids::PlayerId, u8)>,
    /// The seat's place among itself and the nations it met by score (1 = best): the victory
    /// screen's score list (`PlayerQueries::race`).
    rank: usize,
    /// The cities of other nations the seat has seen: owner, citizens when last seen, rounds since.
    seen: Vec<(crate::ids::PlayerId, u32, u32)>,
}

impl Known {
    pub fn of(obs: &AIObservation<'_>, brief: &crate::ai::planner::TurnBrief, state: &crate::ai::state::AiState) -> Self {
        use crate::knowledge::public::PlayerQueries;
        let race = obs.view.race();
        let own = race.own_score();
        Known {
            met_eras: brief.met_eras.clone(),
            rank: 1 + race.scores.iter().skip(1).filter(|(_, s)| *s > own).count(),
            seen: state.seen_cities().iter().map(|c| (c.owner, c.population, obs.round.saturating_sub(c.last_seen_round))).collect(),
        }
    }
}

/// The chance, percent, that an order of the wonder that takes `standard_turns` (Standard speed)
/// is finished before any other nation does it, from what the seat knows (`Known`) and the
/// table (`wonder_chance`, the bench's rate of won orders):
/// - the turns the order takes;
/// - the seat's place by score among the nations it met (the stronger seat wins more races);
/// - the largest city the seat has seen of a met nation whose era reaches the wonder's era (the
///   nations that can build it), its citizens widened by the rounds since the sighting
///   (`AGE_ROUNDS_PER_CITIZEN`): none seen is the first column. A nation not met adds nothing the
///   seat can know; the average of the unmet is in the table.
pub fn chance(rules: &CompiledRuleset, known: &Known, kind: BuildingKindId, standard_turns: i64) -> i64 {
    use crate::ai::wonder_chance::{CHANCE, POPULATION, TURNS};
    let era = wonder_era(rules, kind);
    let rival = known
        .seen
        .iter()
        .filter(|(owner, _, _)| known.met_eras.iter().any(|(p, e)| p == owner && *e >= era))
        .map(|(_, pop, age)| pop + age / AGE_ROUNDS_PER_CITIZEN)
        .max()
        .unwrap_or(0);
    let row = TURNS.iter().position(|&t| standard_turns <= i64::from(t)).unwrap_or(TURNS.len() - 1);
    let rank = known.rank.clamp(1, CHANCE[0].len()) - 1;
    let size = POPULATION.iter().position(|&p| rival <= p).unwrap_or(POPULATION.len() - 1);
    i64::from(CHANCE[row][rank][size])
}

/// A sighted city is taken to have grown by a citizen in this many rounds since it was seen (the
/// sighting's age widens the estimate of the rival's production; fitted with the table: the step
/// where the won rate of the orders by the sighted population is the same at every age).
pub const AGE_ROUNDS_PER_CITIZEN: u32 = crate::ai::wonder_chance::FITTED_AGE;
