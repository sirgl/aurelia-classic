//! Research (R-07 AI stage F): the next technology by its value under the
//! current strategy (what it unlocks, its effects) plus half the value of
//! the best technology it opens next (one step of the prerequisite DAG),
//! per point of cost; ties by lower cost, then id. Only a technology whose
//! prerequisites are all known can be queued (the core's rule).

use crate::ai::planner::{Decision, TurnBrief};
use crate::ai::planner_value::{tech_value, unit_strength, Weights};
use crate::ai::state::Strategy;
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::TechId;
use crate::rules::CompiledRuleset;

/// Research choice of a turn with an empty research queue.
pub fn plan_research(
    brief: &TurnBrief,
    strategy: Strategy,
    rules: &CompiledRuleset,
) -> Option<Decision> {
    research_by(brief, rules, &Weights::of(strategy), |_| 0)
}

/// Research of the allocator (`allocator`, `standard`, classic/docs/ai-allocator.md): the same
/// choice with the allocator's prices as weights, each technology worth its
/// points of the score, and the world wonders it unlocks that nobody built
/// worth half their points (others race for them).
pub fn plan_research_valued(brief: &TurnBrief, rules: &CompiledRuleset, prices: &crate::ai::value::Prices) -> Option<Decision> {
    let points = |tech: TechId| {
        let wonders = rules.technologies[tech.0 as usize]
            .unlocks_buildings
            .iter()
            .filter(|b| rules.buildings[b.0 as usize].wonder && !brief.built_wonders.contains(b))
            .count() as i64;
        // Weights count ten times their value (`planner_value`).
        ((prices.score.tech + wonders * prices.score.wonder / 2) / 10) as i32
    };
    research_by(brief, rules, &prices.weights(), points)
}

/// The best technology by value per beaker (ties to the cheaper, then the
/// lower id), or of the near-equal ones the one the leader loves
/// (`ai::taste`): `w` values what it unlocks, `points` adds its own worth.
fn research_by(brief: &TurnBrief, rules: &CompiledRuleset, w: &Weights, points: impl Fn(TechId) -> i32) -> Option<Decision> {
    if !brief.research_queue.is_empty() {
        return None;
    }
    let cities = brief.cities.len().max(1) as i32;
    let best_strength = best_known_strength(brief, rules);
    let mut options: Vec<(i64, u32, TechId)> = Vec::new();
    for index in 0..rules.technologies.len() {
        let tech = TechId(index as u16);
        if !available(brief, rules, tech) {
            continue;
        }
        let own = tech_value(rules, tech, w, cities, best_strength) + sea_value(brief, rules, tech) + points(tech);
        let next = best_follow_up(brief, rules, tech, w, cities, best_strength);
        let cost = rules.technologies[index].cost.max(1);
        options.push((i64::from(own + next / 2) * 1000 / i64::from(cost), cost, tech));
    }
    options.sort_by_key(|&(_, cost, tech)| (cost, tech.0));
    let best = crate::ai::taste::pick(&brief.personality, &options, |o| o.0, |o| crate::ai::taste::Sides::of_tech(rules, o.2, &brief.built_wonders));
    let Some(&(_, _, technology)) = best else {
        return Some(Decision::refuse(Actor::Player, Reason::NoTechAvailable));
    };
    let command = Command::ChooseTechnology {
        player: brief.player,
        technology,
    };
    Some(Decision::act(Actor::Player, Reason::Research, command))
}

/// The map beyond the sea is mostly unknown below this share: the
/// technologies that let units embark and cross the ocean are worth more
/// (the other civilizations, city-states and land to settle are there).
pub const SEAFARING_KNOWN_PERCENT: u32 = 50;
const SEAFARING_VALUE: i32 = 2000;
const SEAFARING_FROM_ROUND: u32 = 40;

fn sea_value(brief: &TurnBrief, rules: &CompiledRuleset, tech: TechId) -> i32 {
    let Some(def) = rules.technologies.get(tech.0 as usize) else {
        return 0;
    };
    let seafaring = def.allows_embarking || def.embarked_all_water;
    // Early on everybody knows little: scouts need SEAFARING_FROM_ROUND to
    // find out whether more land lies within walking reach.
    let boxed_in = brief.round >= SEAFARING_FROM_ROUND && brief.known_percent < SEAFARING_KNOWN_PERCENT;
    if seafaring && boxed_in {
        SEAFARING_VALUE
    } else {
        0
    }
}

/// Not known and every prerequisite known.
fn available(brief: &TurnBrief, rules: &CompiledRuleset, tech: TechId) -> bool {
    let Some(def) = rules.technologies.get(tech.0 as usize) else {
        return false;
    };
    match &brief.researchable {
        // The rules' own answer (`ai::legal`).
        Some(list) => list.binary_search_by_key(&tech.0, |t| t.0).is_ok(),
        None => !brief.knows(tech) && def.requires.iter().all(|r| brief.knows(*r)),
    }
}

/// Value of the best technology that `tech` makes available.
fn best_follow_up(
    brief: &TurnBrief,
    rules: &CompiledRuleset,
    tech: TechId,
    w: &Weights,
    cities: i32,
    strength: i32,
) -> i32 {
    let mut best = 0;
    for (index, def) in rules.technologies.iter().enumerate() {
        if !def.requires.contains(&tech) {
            continue;
        }
        let opens = def.requires.iter().all(|r| *r == tech || brief.knows(*r));
        if !opens {
            continue;
        }
        let next = TechId(index as u16);
        best = best.max(tech_value(rules, next, w, cities, strength) + sea_value(brief, rules, next));
    }
    best
}

/// Strongest land unit the player's technologies unlock.
fn best_known_strength(brief: &TurnBrief, rules: &CompiledRuleset) -> i32 {
    let mut best = 0;
    for (index, def) in rules.units.iter().enumerate() {
        let known = def.required_tech.is_none_or(|t| brief.knows(t));
        if known && !def.unbuildable {
            best = best.max(unit_strength(rules, crate::ids::UnitKindId(index as u16)));
        }
    }
    best
}
