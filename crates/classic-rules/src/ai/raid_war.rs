//! Raid in a war (`Variant::RaidWar`; the tactics exam, /Users/roman.ivanov/civ/work/nets/exam/LOG.md): a
//! unit with nothing better to do pillages the improvements of a nation we are at war with. The war is
//! never the raid's: this module declares nothing and provokes nothing (it asks only the relations that
//! already say "at war"), and the formed raids of `ai::raid` stay off as in `Standard`.
//!
//! What the unit would otherwise be doing prices it. A unit with a task of the empire's (a garrison that
//! the city wants, the last guard, an escort, a member of an operation or of an assault, a wounded unit
//! healing, a scout) does not go; a spare garrison (beyond what its city wants, the city not endangered) and a
//! unit with no task are free. A free unit goes when the gain (the enemy's yields lost until a worker
//! restores the tile, the gold of the pillage, the hit points the pillage heals) is above its risk where it
//! would pillage (`field_craft::trip`: the fighters and the cities that strike the tile, the chance of its death
//! times its price). It never leaves a ranged or a civilian unit of ours uncovered
//! (`field_craft::has_dependant`, a ward counts when a hostile fighter is near it) and never leaves fortified
//! defensive ground while a hostile fighter is near it. No stance rule and no guard change: raiding only.

use crate::ai::planner::{role_of, AiRole, Decision, PlanTools, UnitView};
use crate::ai::planner_units::spare_garrison;
use crate::ai::state::{AiState, UnitTask};

/// A hostile fighter within this many of its turns of walking and striking is "near" a unit holding ground.
pub const NEAR_TURNS: i32 = 4;

/// Rounds of its walking a free unit looks ahead for prey. Own design: a war between the AIs lasts a dozen rounds at
/// the median (`raid::MAX_ARRIVAL_ROUNDS` is the formed raid's ten); half of it is the trip a single unit with nothing
/// else to do makes, and it is priced at every step by the fighters and cities in the way. Chosen on the exam's
/// raid scenes (even set, T5 / T5b / T5r win shares): 3 rounds 61 / 50 / 60 %, 4 rounds 64 / 53 / 59 %, 5 rounds
/// 65 / 54 / 59 %, 6 rounds 65 / 55 / 62 %; 5 is the middle of the plateau.
pub const SORTIE_ROUNDS: i32 = 5;

/// A ward (a civilian or a ranged unit of ours beside the unit) is left exposed when a hostile fighter is this close
/// to it: the distance at which the empire already sends an escort with a settler (`planner_units`).
pub const WARD_DANGER_RADIUS: i32 = 3;

/// A fortified unit on ground with a defence bonus while a hostile fighter is near it: it holds.
fn holds_ground(view: &UnitView<'_, '_>, tools: &PlanTools<'_>) -> bool {
    let unit = view.local.unit;
    let rules = tools.rules;
    if !unit.fortified || view.brief.cities.iter().any(|c| c.tile == unit.tile) {
        return false;
    }
    if crate::ai::siege_tactics::ground_percent(&view.local.view, rules, unit.tile, view.local.player) == 0 {
        return false;
    }
    let dims = view.local.view.dimensions();
    view.local.foreign_units.iter().any(|f| {
        view.local.relation(f.owner).is_some_and(|r| r.at_war)
            && role_of(rules, f.kind) == AiRole::Military
            && dims.distance(f.tile, unit.tile).is_some_and(|d| {
                d <= NEAR_TURNS * (i32::from(rules.units[f.kind.0 as usize].movement.max(1)) + crate::ai::combat::ranged_range(rules, f.kind).unwrap_or(1))
            })
    })
}

/// The unit's city keeps the garrison it wants without it, by the units that stand in the city (the tasks of the
/// empire say who is to garrison, the tiles say who does), and the city is not in danger.
fn city_covered(view: &UnitView<'_, '_>, state: &AiState, city: crate::ids::TileId) -> bool {
    if view.brief.endangered(city) {
        return false;
    }
    let me = view.local.unit.id;
    let target = crate::ai::planner_units::garrison_wanted_by(view.brief, state.strategy.current).max(1);
    let rules = view.local.view.rules();
    view.local.own_units.iter().filter(|u| u.id != me && u.tile == city && role_of(rules, u.kind) == AiRole::Military).count() >= target
}

/// Tiles around a city that are its own ground (the city's work radius): a unit farther from the city it is to
/// garrison is not guarding it now.
pub const CITY_RING: i32 = 3;

/// The unit is out of the city's ring and the city is not in danger: it is on its way, not on guard.
fn away(view: &UnitView<'_, '_>, city: crate::ids::TileId) -> bool {
    !view.brief.endangered(city) && view.local.view.dimensions().distance(view.local.unit.tile, city).is_some_and(|d| d > CITY_RING)
}

/// The pillage trip of a unit that is free to go, if one pays.
pub fn sortie(view: &UnitView<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Option<Decision> {
    if !view.brief.raid_war {
        return None;
    }
    let unit = view.local.unit;
    let free = match state.task_of(unit.id).map(|t| t.task) {
        None => true,
        // A garrison goes only when its city keeps the garrison it wants without it, or when it is already out on the
        // enemy's land (its task was given when its moves ran out), or on its way and not on guard.
        Some(UnitTask::Defend { city }) => crate::ai::field_craft::abroad(view) || spare_garrison(view, state, city) || city_covered(view, state, city) || away(view, city),
        // A wounded unit rests, unless the pillage itself heals it past the wounded (`HEAL_BELOW_HP`).
        Some(UnitTask::Heal { .. }) => unit.hp.saturating_add(crate::systems::combat::actions::PILLAGE_HEAL_HP) >= crate::ai::planner_units::HEAL_BELOW_HP,
        // Scout, escort, assault, operation, raid, settle, improve: the empire's own business.
        Some(_) => false,
    };
    if !free || crate::ai::field_craft::has_dependant(view, tools.rules, Some(WARD_DANGER_RADIUS)) || holds_ground(view, tools) {
        return None;
    }
    crate::ai::field_craft::trip(view, tools, SORTIE_ROUNDS)
}
