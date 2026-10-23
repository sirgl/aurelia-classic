//! The AI's carriers (classic/docs/ai-strikes.md "Carriers"): a ship that
//! carries aircraft or missiles takes the air arm to a coast no city of
//! ours reaches. From data only: a *carrier* is a sea unit whose
//! `combat.carries` names a kind of cargo, its *cargo* the air units whose
//! `combat.carried_as` is that kind.
//!
//! The plan, in three parts:
//! - **Wanted** (`wanted`): a war operation marches on a coastal city that
//!   no city of ours reaches with the longest range of the cargo we have
//!   (strikers and weapons on hand), and no carrier is alive or queued.
//! - **Sailing** (`decide_carrier`): a carrier with cargo aboard sails to
//!   the water `standoff` tiles from the target (inside the cargo's range,
//!   outside the city's strike); one without waits in port for its cargo;
//!   a wounded one mends in port.
//! - **Boarding** (`board`): an aircraft or weapon that has nothing to
//!   strike from where it is goes to the carrier in reach (the rules'
//!   `RebaseToCarrier`, range and room checked by them) when a carrier
//!   sails for a target that lies out of the city's reach.

use crate::ai::fleet::coastal;
use crate::ai::planner::{Decision, PlanTools, TurnBrief, UnitView};
use crate::ai::planner_units::{approach_at, hold, move_along, HEAL_BELOW_HP};
use crate::ai::state::{AiState, OperationPhase};
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{TileId, UnitHandle, UnitKindId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::{CargoKind, Domain};
use crate::rules::CompiledRuleset;

/// The carrier stays this far from its target: the city's strike
/// (`operations::STAGING_DISTANCE` is the army's).
pub const STANDOFF: i32 = crate::combat::city::CITY_ATTACK_RANGE + 1;

/// What a unit kind carries (None: it is no carrier).
pub fn carries(rules: &CompiledRuleset, kind: UnitKindId) -> Option<CargoKind> {
    let def = rules.units.get(kind.0 as usize)?;
    (def.domain == Domain::Sea).then_some(def.combat.carries).flatten()
}

/// The kind of cargo an air unit is (None: it is carried by nothing).
pub fn cargo_class(rules: &CompiledRuleset, kind: UnitKindId) -> Option<CargoKind> {
    let def = rules.units.get(kind.0 as usize)?;
    (def.domain == Domain::Air).then_some(def.combat.carried_as).flatten()
}

/// The longest range of the player's air units (aircraft and weapons
/// alike) that a carrier could carry.
fn cargo_range(brief: &TurnBrief, rules: &CompiledRuleset) -> i32 {
    brief
        .units
        .iter()
        .filter(|u| cargo_class(rules, u.kind).is_some())
        .filter(|u| crate::ai::air::wing(rules, u.kind).is_some() || crate::ai::strikes::weapon(rules, u.kind).is_some())
        .map(|u| i32::from(rules.units[u.kind.0 as usize].range))
        .max()
        .unwrap_or(0)
}

/// Carriers alive or queued.
pub fn on_hand(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> usize {
    let alive = obs.own_units.iter().filter(|u| carries(rules, u.kind).is_some()).count();
    let queued = obs
        .own_cities
        .iter()
        .flat_map(|c| &c.queue)
        .filter(|item| matches!(item, crate::ids::ProductionItem::Unit(k) if carries(rules, *k).is_some()))
        .count();
    alive + queued
}

/// The coastal target of a running operation that no city of ours reaches
/// with the cargo we have: the place a carrier is for.
pub fn out_of_reach_target(brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, view: &crate::knowledge::view::PlayerView<'_>) -> Option<TileId> {
    let range = cargo_range(brief, rules);
    if range == 0 {
        return None;
    }
    state
        .operations()
        .iter()
        .filter(|o| coastal(view, o.target))
        .filter(|o| !brief.cities.iter().any(|c| brief.dims.distance(c.tile, o.target).is_some_and(|d| d <= range)))
        .map(|o| (brief.dims.distance(o.muster, o.target).unwrap_or(i32::MAX), o.target.0, o.target))
        .min()
        .map(|(_, _, target)| target)
}

/// Carriers the war calls for: one, while an operation marches on a
/// coastal city out of our cities' reach and we have cargo for it.
pub fn wanted(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> usize {
    if !(brief.air || brief.strikes) || on_hand(obs, rules) > 0 {
        return 0;
    }
    usize::from(out_of_reach_target(brief, state, rules, &obs.view).is_some())
}

/// The cheapest carrier `available` lets the player build.
pub fn best_carrier(rules: &CompiledRuleset, available: impl Fn(UnitKindId) -> bool) -> Option<UnitKindId> {
    (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|k| carries(rules, *k).is_some() && available(*k))
        .min_by_key(|k| (rules.units[k.0 as usize].cost, k.0))
}

/// The decision of a carrier.
pub fn decide_carrier(view: &UnitView<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Decision {
    let unit = view.local.unit;
    let rules = tools.rules;
    let dims = view.local.view.dimensions();
    let distance = |a: TileId, b: TileId| dims.distance(a, b).unwrap_or(i32::MAX);
    let port = view
        .brief
        .cities
        .iter()
        .filter(|c| coastal(&view.local.view, c.tile))
        .min_by_key(|c| (distance(c.tile, unit.tile), c.id))
        .map(|c| c.tile);
    let to_port = |tools: &mut PlanTools<'_>| match port {
        Some(tile) if tile != unit.tile => move_along(view, tools, tile, Reason::Rally),
        _ => hold(view, Reason::Rally),
    };
    if unit.hp < HEAL_BELOW_HP {
        return to_port(tools);
    }
    let (aboard, _) = view.local.view.cargo_aboard(unit.handle);
    let target = state
        .operations()
        .iter()
        .filter(|o| o.phase == OperationPhase::Marching && coastal(&view.local.view, o.target))
        .map(|o| (distance(unit.tile, o.target), o.target.0, o.target))
        .min()
        .map(|(_, _, t)| t);
    let (Some(target), true) = (target, aboard > 0) else {
        return to_port(tools);
    };
    // Inside the range of the shortest-ranged cargo aboard would leave a
    // striker idle; the longest range of the cargo we have, less one,
    // is the aim, never closer than the city's strike reaches.
    let standoff = (cargo_range(view.brief, rules) - 1).max(STANDOFF);
    if distance(unit.tile, target) <= standoff {
        return hold(view, Reason::Rally);
    }
    approach_at(view, tools, target, standoff, true, Reason::Advance)
}

/// An aircraft or weapon with nothing to strike from its base goes to the
/// carrier in the rules' range that sails for a target out of the cities'
/// reach: the carrier with room nearest to the target. None: nothing to
/// board.
pub fn board(view: &UnitView<'_, '_>, state: &AiState, rules: &CompiledRuleset) -> Option<Decision> {
    let unit = view.local.unit;
    let class = cargo_class(rules, unit.kind)?;
    let target = out_of_reach_target(view.brief, state, rules, &view.local.view)?;
    let dims = view.local.view.dimensions();
    let distance = |a: TileId, b: TileId| dims.distance(a, b).unwrap_or(i32::MAX);
    let range = i32::from(rules.units.get(unit.kind.0 as usize)?.range);
    // Already within reach of the target from where it stands.
    if distance(unit.tile, target) <= range {
        return None;
    }
    let mut best: Option<(i32, UnitHandle, crate::ids::EntityId)> = None;
    for carrier in view.brief.units.iter().filter(|u| carries(rules, u.kind) == Some(class)) {
        if distance(carrier.tile, target) >= distance(unit.tile, target) {
            continue;
        }
        let key = (distance(carrier.tile, target), carrier.handle, carrier.id);
        if best.is_none_or(|b| (key.0, key.2) < (b.0, b.2)) {
            best = Some(key);
        }
    }
    let (_, carrier, _) = best?;
    let command = Command::RebaseToCarrier { unit: unit.handle, carrier };
    view.local.view.accepts(&command).then(|| Decision::act(Actor::Unit(unit.id), Reason::BaseOnCarrier, command))
}
