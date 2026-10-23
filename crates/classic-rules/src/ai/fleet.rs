//! The AI's navy (classic/docs/ai-operations.md "Fleet"): warships support an
//! operation against a coastal city. Land units cross the water by
//! themselves (they embark); the fleet is the rest of a naval invasion — the
//! ships that shell the city from the sea while the army lands, as the
//! rules let them (ranged ships strike cities from the water).
//!
//! No memory of its own: the wish for ships follows the operations
//! (`wanted`), the cities build the shortfall (`planner_cities`), a ship
//! sails to a tile in its range of the target of a marching operation and
//! otherwise waits in port; a wounded ship heals there.

use crate::ai::combat::ranged_range;
use crate::ai::planner::{Decision, PlanTools, UnitView};
use crate::ai::planner_units::{approach_at, hold, move_along, HEAL_BELOW_HP};
use crate::ai::state::{AiState, OperationPhase};
use crate::ai::trace::{Actor, Reason};
use crate::ids::{TileId, UnitKindId};
use crate::knowledge::observation::AIObservation;
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;

/// Warships an operation against a coastal city wants.
pub const FLEET_SIZE: usize = 2;

/// A tile of the sea or lake as the player knows it.
pub(crate) fn water(view: &crate::knowledge::view::PlayerView<'_>, tile: TileId) -> bool {
    let kinds = view.terrain_kinds();
    view.tile(tile).snapshot().is_some_and(|s| kinds.is_water(s.terrain.0))
}

/// The tile touches the sea, so a ship may stand next to it.
pub fn coastal(view: &crate::knowledge::view::PlayerView<'_>, tile: TileId) -> bool {
    view.dimensions().neighbors(tile).into_iter().any(|n| water(view, n))
}

/// Warships the running operations want: some of their targets are coastal.
pub fn wanted(obs: &AIObservation<'_>, state: &AiState) -> usize {
    if state.operations().iter().any(|o| coastal(&obs.view, o.target)) {
        FLEET_SIZE
    } else {
        0
    }
}

fn is_warship(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    rules.units.get(kind.0 as usize).is_some_and(|d| d.domain == Domain::Sea && d.ranged_strength > 0 && d.range > 0)
}

/// The strongest warship the player may build (`available`), the cheaper
/// on a tie.
pub fn best_warship(rules: &CompiledRuleset, available: impl Fn(UnitKindId) -> bool) -> Option<UnitKindId> {
    let mut best: Option<(u16, u32, UnitKindId)> = None;
    for index in 0..rules.units.len() {
        let kind = UnitKindId(index as u16);
        if !is_warship(rules, kind) || !available(kind) {
            continue;
        }
        let def = &rules.units[index];
        if best.is_none_or(|(s, c, _)| def.ranged_strength > s || (def.ranged_strength == s && def.cost < c)) {
            best = Some((def.ranged_strength, def.cost, kind));
        }
    }
    best.map(|(_, _, kind)| kind)
}

/// Warships alive or queued.
pub fn on_hand(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> usize {
    let alive = obs.own_units.iter().filter(|u| is_warship(rules, u.kind)).count();
    let queued = obs
        .own_cities
        .iter()
        .flat_map(|c| &c.queue)
        .filter(|item| matches!(item, crate::ids::ProductionItem::Unit(k) if is_warship(rules, *k)))
        .count();
    alive + queued
}

/// Warships still to build.
pub fn shortfall(obs: &AIObservation<'_>, state: &AiState, rules: &CompiledRuleset) -> usize {
    wanted(obs, state).saturating_sub(on_hand(obs, rules))
}

/// The decision of a ship: heal in port when wounded, sail into the range
/// of the city a marching operation attacks, else wait in port. Aircraft
/// are the air arm's (`ai::air`) when the variant has one.
pub fn decide_naval(view: &UnitView<'_, '_>, state: &mut AiState, tools: &mut PlanTools<'_>) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    let rules = tools.rules;
    if crate::ai::air::wing(rules, unit.kind).is_some() {
        return if view.brief.air { crate::ai::air::decide_air(view, state, tools) } else { Decision::refuse(actor, Reason::Idle) };
    }
    if crate::ai::carrier::carries(rules, unit.kind).is_some() {
        return if view.brief.carriers { crate::ai::carrier::decide_carrier(view, state, tools) } else { Decision::refuse(actor, Reason::Idle) };
    }
    if crate::ai::strikes::weapon(rules, unit.kind).is_some() {
        return if view.brief.strikes { crate::ai::strikes::decide_weapon(view, state, tools) } else { Decision::refuse(actor, Reason::Idle) };
    }
    if !is_warship(rules, unit.kind) {
        return Decision::refuse(actor, Reason::Idle);
    }
    let dims = view.local.view.dimensions();
    let distance = |a: TileId, b: TileId| dims.distance(a, b).unwrap_or(i32::MAX);
    let port = view
        .brief
        .cities
        .iter()
        .filter(|c| coastal(&view.local.view, c.tile))
        .min_by_key(|c| (distance(c.tile, unit.tile), c.id))
        .map(|c| c.tile);
    let target = state
        .operations()
        .iter()
        .filter(|o| o.phase == OperationPhase::Marching && coastal(&view.local.view, o.target))
        .map(|o| o.target)
        .min_by_key(|t| (distance(unit.tile, *t), t.0));
    let to_port = |tools: &mut PlanTools<'_>| match port {
        Some(tile) if tile != unit.tile => move_along(view, tools, tile, Reason::Rally),
        _ => hold(view, Reason::Rally),
    };
    if unit.hp < HEAL_BELOW_HP {
        return to_port(tools);
    }
    let Some(target) = target else { return to_port(tools) };
    let range = ranged_range(rules, unit.kind).unwrap_or(1);
    if distance(unit.tile, target) <= range {
        return hold(view, Reason::Rally);
    }
    approach_at(view, tools, target, range, true, Reason::Advance)
}
