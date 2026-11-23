//! The early growth's levers of `net2` (`early-growth` only, `TurnBrief::early_growth`; work/nets/early-growth): the
//! cities whose flat garrison waits (`weights::GARRISON`), the gold the tiles leave for a growth purchase
//! (`weights::BUY_CHEST`) and the site head's weight of the shared land (`weights::site::SHARED`). The executors read
//! `plan::Growth`; nothing here orders a unit or spends a gold.

use crate::ai::planner::{role_of, AiRole, TurnBrief};
use crate::ai::strategy_net::Milli;
use crate::ids::ProductionItem;
use crate::knowledge::observation::{AIObservation, OwnCity};
use crate::rules::effects::Modifier;
use crate::rules::CompiledRuleset;
use crate::world::player::PlayerKind;

use super::concepts::{fire, Frame};
use super::levers::{city_row, Pass};
use super::plan::{Growth, Inputs};
use super::weights::{self as w, c, gate, gb, gc};

/// The levers of the turn.
pub fn plan(obs: &AIObservation<'_>, brief: &TurnBrief, state: &crate::ai::state::AiState, rules: &CompiledRuleset, pass: &Pass, inputs: &Inputs) -> Growth {
    let at_war = if obs.relations.iter().any(|r| r.at_war && r.kind == PlayerKind::Major) { 1000 } else { 0 };
    let camps = crate::ai::camps::known_camps(obs);
    let mean = mean_production(obs);
    let mut garrisons = Vec::new();
    for city in &inputs.cities {
        let Some(own) = obs.own_cities.iter().find(|o| o.id == city.city) else { continue };
        let guard = city_row(&pass.reads, city);
        let row = garrison_row(brief, own, &guard, &camps, mean, at_war);
        if fire(&w::GARRISON, &Frame { c: &row, s: &pass.seat, ..Frame::default() }) >= gate::GARRISON.0 {
            garrisons.push(city.city);
        }
    }
    let waiting = waiting_price(obs, rules);
    let row = buy_row(waiting.is_some(), at_war);
    let level = fire(&w::BUY_CHEST, &Frame { c: &row, s: &pass.seat, ..Frame::default() });
    let chest = waiting.map_or(0, |price| (i64::from(price) * level / 1000) as u32);
    let jobs_ahead = jobs_ahead(obs, brief, state, rules);
    Growth { plays: true, garrisons, chest, shared: w::site::SHARED.0, jobs_ahead }
}

/// The jobs the workers have ahead (`Growth::jobs_ahead`) while the empire has fewer land workers (alive or queued) than
/// cities: per own city, its owned unimproved land tiles in the work radius with a job the known technologies allow
/// (`planner_workers::open_jobs`), at most its citizens and `gate::JOBS_AHEAD` more — the tiles it works now and the
/// next ones it will.
fn jobs_ahead(obs: &AIObservation<'_>, brief: &TurnBrief, state: &crate::ai::state::AiState, rules: &CompiledRuleset) -> u32 {
    // A worker a city: past it the allocator's own count of the worked tiles decides (the workers per city at r120
    // went from 0.95 to 1.41 with the jobs ahead for every worker, idle 23 %).
    let builder = |kind: crate::ids::UnitKindId| rules.units.get(kind.0 as usize).is_some_and(|d| d.builder && d.domain == crate::rules::content::Domain::Land);
    let workers = obs.own_units.iter().filter(|u| builder(u.kind)).count() + obs.own_cities.iter().flat_map(|c| c.queue.iter()).filter(|i| matches!(i, ProductionItem::Unit(k) if builder(*k))).count();
    if workers >= obs.own_cities.len() {
        return 0;
    }
    let dims = obs.view.dimensions();
    let mut total = 0;
    for city in &obs.own_cities {
        let cap = city.population + gate::JOBS_AHEAD.0;
        let mut open = 0;
        // Unimproved owned land tiles only, and no more than the cap: each costs the worker planner's job search.
        for tile in crate::math::hex_disc(dims, city.tile, crate::world::city::CITY_WORK_RADIUS) {
            if open >= cap {
                break;
            }
            let bare = obs.view.tile(tile).snapshot().is_some_and(|s| s.owner == Some(obs.player) && s.improvement.is_none() && !s.city);
            if bare {
                open += crate::ai::planner_workers::open_jobs(&obs.view, rules, brief, state.strategy.current, std::iter::once(tile));
            }
        }
        total += open;
    }
    total
}

/// `weights::gc` of one city.
pub fn garrison_row(brief: &TurnBrief, city: &OwnCity, guard: &[Milli; c::N], camps: &[crate::ids::TileId], mean: i64, at_war: Milli) -> [Milli; gc::N] {
    let low = if i64::from(city.income.production) * 1000 < mean { 1000 } else { 0 };
    let camp = if camps.iter().any(|t| brief.dims.distance(*t, city.tile).is_some_and(|d| d <= crate::ai::camps::CAMP_REACH)) { 1000 } else { 0 };
    [low, camp, guard[c::STACK_MENACE], guard[c::DANGER], guard[c::DISTRUST_NEAR], at_war]
}

/// `weights::gb` of the seat.
pub fn buy_row(waiting: bool, at_war: Milli) -> [Milli; gb::N] {
    [if waiting { 1000 } else { 0 }, at_war]
}

/// The mean production of the empire's cities, thousandths of a hammer.
fn mean_production(obs: &AIObservation<'_>) -> i64 {
    let n = obs.own_cities.len().max(1) as i64;
    obs.own_cities.iter().map(|c| i64::from(c.income.production.max(0))).sum::<i64>() * 1000 / n
}

/// The hurry price of the cheapest growth item heading a queue with `gate::CHEST_TURNS` turns or more left: a worker,
/// a settler, or a building whose effects yield science (a library's class). None: no such head.
fn waiting_price(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Option<u32> {
    let mut best: Option<u32> = None;
    for city in &obs.own_cities {
        let Some(&item) = city.queue.first() else { continue };
        if !growth_item(rules, item) {
            continue;
        }
        if crate::ai::planner_cities::turns(obs, rules, city, item) < obs.speed.pace_rounds(gate::CHEST_TURNS.0).max(1) {
            continue;
        }
        if let Some(price) = obs.view.own_hurry_price(city.handle) {
            best = Some(best.map_or(price, |b| b.min(price)));
        }
    }
    best
}

/// A worker, a settler, or a building whose effects yield science.
pub fn growth_item(rules: &CompiledRuleset, item: ProductionItem) -> bool {
    use crate::rules::effects::YieldKind;
    match item {
        ProductionItem::Unit(kind) => matches!(role_of(rules, kind), AiRole::Worker | AiRole::Settler) && !crate::ai::sea_work::is_sea_builder(rules, kind),
        ProductionItem::Building(b) => rules.buildings.get(usize::from(b.0)).is_some_and(|d| {
            !d.wonder
                && crate::ai::strategy_net::cell::effects_of(&d.effects).any(|e| {
                    matches!(e.modifier, Modifier::AddYield { yield_kind: YieldKind::Science, .. } | Modifier::AddYieldPerPopulation { yield_kind: YieldKind::Science, .. } | Modifier::AddYieldPercent { yield_kind: YieldKind::Science, .. })
                })
        }),
        ProductionItem::Project(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::concepts::{fire, Frame};
    use super::super::weights::{self as w, gate, gb, s};

    fn garrison(low: i32, camp: i32, menace: i32, danger: i32, distrust: i32, war: i32) -> i32 {
        let row = [low, camp, menace, danger, distrust, war].map(i64::from);
        let seat = [0; s::N];
        fire(&w::GARRISON, &Frame { c: &row, s: &seat, ..Frame::default() }) as i32
    }

    #[test]
    fn a_quiet_low_city_waits_for_its_garrison() {
        let gate = gate::GARRISON.0 as i32;
        assert!(garrison(0, 0, 0, 0, 0, 0) >= gate, "a city at the mean production keeps today's garrison");
        assert!(garrison(1000, 0, 0, 0, 0, 0) < gate, "below the mean and quiet: infrastructure first");
        assert!(garrison(1000, 1000, 0, 0, 0, 0) >= gate, "a camp in reach");
        assert!(garrison(1000, 0, 0, 1000, 0, 0) >= gate, "the watch's threat");
        assert!(garrison(1000, 0, 0, 0, 0, 1000) >= gate, "at war");
        assert!(garrison(1000, 0, 0, 0, 700, 0) < gate && garrison(1000, 0, 0, 0, 800, 0) >= gate, "a distrusted neighbour above 715");
    }

    #[test]
    fn the_chest_waits_for_a_growth_item_at_peace() {
        let seat = [0; s::N];
        let chest = |waiting: i64, war: i64| {
            let mut row = [0; gb::N];
            row[gb::WAITING] = waiting;
            row[gb::AT_WAR] = war;
            fire(&w::BUY_CHEST, &Frame { c: &row, s: &seat, ..Frame::default() })
        };
        assert_eq!((chest(0, 0), chest(1000, 0), chest(1000, 1000)), (0, 1000, 0));
    }
}
