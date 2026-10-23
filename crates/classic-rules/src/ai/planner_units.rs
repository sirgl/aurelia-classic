//! Unit decisions (R-07 AI stages D, G): one command per decision from a
//! fresh local observation, driven by the unit's task.
//!
//! Military units first take what the moment offers (`tactics`), then
//! follow their task: scout the frontier, garrison a city, escort a
//! settler, assault the war objective, heal. Tasks persist across turns and
//! are reassigned only when done, invalid or failing; targets are reserved
//! so two units never chase the same frontier tile or site, and a scout
//! keeps its target until it is reached or stops being frontier (no
//! oscillation between two tiles).

use crate::ai::planner::{role_of, AiRole, Decision, PlanTools, UnitView};
use crate::ai::planner_map::{explore, explored_path, find_path, step_points, Blockers, KnownMap, PathOutcome, Visit};
use crate::ai::scouting::{self, scout_target};
use crate::ai::state::{AiState, Reservation, Strategy, UnitTask};
use crate::ai::tactics;
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{EntityId, TileId};
use crate::navigation::Embark;
use crate::rules::content::UnitRole;
use crate::rules::CompiledRuleset;
use crate::world::stacking::stack_class;

/// After this round scouting stops (the known world is what matters).
pub const SCOUTING_ENDS_ROUND: u32 = 150;
/// From this round every city wants a garrison even in peace.
pub const GARRISON_FROM_ROUND: u32 = 10;
/// From this round every city wants a garrison, even with nobody met.
pub const GARRISON_ALONE_ROUND: u32 = 40;
/// Health below which a unit goes to heal.
pub const HEAL_BELOW_HP: u16 = 50;

/// One decision of a unit, by its role.
pub fn decide_unit(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
) -> Decision {
    let unit = view.local.unit;
    match role_of(tools.rules, unit.kind) {
        AiRole::Settler => {
            let _span = prof::span("civ5/unit settler");
            crate::ai::planner_settle::decide_settler(view, state, tools)
        }
        AiRole::Worker => {
            let _span = prof::span("civ5/unit worker");
            crate::ai::planner_workers::decide_worker(view, state, tools)
        }
        AiRole::Military => {
            let _span = prof::span("civ5/unit military");
            decide_military(view, state, tools)
        }
        AiRole::Naval => {
            let _span = prof::span("civ5/unit naval");
            crate::ai::fleet::decide_naval(view, state, tools)
        }
        AiRole::Civilian => Decision::refuse(Actor::Unit(unit.id), Reason::Idle),
    }
}

/// Foreign units the unit sees: a plan walks around them.
pub fn blockers(view: &UnitView<'_, '_>) -> Blockers {
    Blockers::new(view.local.foreign_units.iter().map(|u| u.tile).collect())
}

/// An enemy (at war) military unit the player sees within `radius` of
/// `tile`.
pub fn threatened(
    view: &UnitView<'_, '_>,
    rules: &CompiledRuleset,
    tile: TileId,
    radius: i32,
) -> bool {
    let dims = view.local.view.dimensions();
    view.local.foreign_units.iter().any(|u| {
        let at_war = view.local.relation(u.owner).is_some_and(|r| r.at_war);
        let near = dims.distance(u.tile, tile).is_some_and(|d| d <= radius);
        at_war && near && role_of(rules, u.kind) == AiRole::Military
    })
}

/// The own city nearest to the unit.
pub fn nearest_safe_tile(view: &UnitView<'_, '_>, _tools: &PlanTools<'_>) -> Option<TileId> {
    let dims = view.local.view.dimensions();
    view.brief
        .nearest_city(dims, view.local.unit.tile)
        .map(|c| c.tile)
}

/// May the unit end its move on `tile` (one unit per stack class, no
/// foreign unit, as far as it sees)?
pub(crate) fn can_stop(view: &UnitView<'_, '_>, rules: &CompiledRuleset, tile: TileId) -> bool {
    let class = stack_class(rules, view.local.unit.kind);
    // On water every land unit is embarked: one of any kind per tile.
    let kinds = view.local.view.terrain_kinds();
    let water = view.local.view.tile(tile).snapshot().is_some_and(|s| kinds.is_water(s.terrain.0));
    let own_blocks = view
        .local
        .own_units
        .iter()
        .any(|u| u.tile == tile && (water || stack_class(rules, u.kind) == class));
    let foreign = view.local.foreign_units.iter().any(|u| u.tile == tile);
    !own_blocks && !foreign
}

/// How far a unit goes in one order: a step, or all it can this turn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Gait {
    Step,
    Stride,
}

/// One move towards `goal` over the known map: to the next tile of the
/// path where the unit may stop (passing an own unit costs one more step).
pub fn move_along(
    view: &UnitView<'_, '_>,
    tools: &mut PlanTools<'_>,
    goal: TileId,
    reason: Reason,
) -> Decision {
    walk_along(view, tools, goal, reason, Gait::Step)
}

/// The walk of a whole turn towards `goal` in one order: to the farthest
/// tile of the path the moves left reach and the unit may stop on.
pub fn stride_along(
    view: &UnitView<'_, '_>,
    tools: &mut PlanTools<'_>,
    goal: TileId,
    reason: Reason,
) -> Decision {
    walk_along(view, tools, goal, reason, Gait::Stride)
}

fn walk_along(
    view: &UnitView<'_, '_>,
    tools: &mut PlanTools<'_>,
    goal: TileId,
    reason: Reason,
    gait: Gait,
) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    if unit.moves_left == 0 {
        return Decision::refuse(actor, Reason::Waiting);
    }
    let first = path_step(view, tools, goal, &blockers(view), reason, gait);
    if first.command.is_some() || first.reason != Reason::Waiting {
        return first;
    }
    // Own units of the same kind stand on the way: go around them.
    let around = path_step(
        view,
        tools,
        goal,
        &blockers_with_own(view, tools.rules, goal),
        reason,
        gait,
    );
    if around.command.is_some() {
        return around;
    }
    Decision::refuse(actor, Reason::Blocked)
}

fn path_step(
    view: &UnitView<'_, '_>,
    tools: &mut PlanTools<'_>,
    goal: TileId,
    blockers: &Blockers,
    reason: Reason,
    gait: Gait,
) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    let limit = tools.settings.budget.search();
    let outcome = find_path(
        &KnownMap::of_unit(&view.local.view, unit.handle),
        unit.tile,
        goal,
        blockers,
        tools.scratch,
        limit,
        &mut tools.counters.search,
    );
    match outcome {
        PathOutcome::Found { tiles, .. } if gait == Gait::Stride => stride_on(view, tools.rules, &tiles, reason),
        PathOutcome::Found { tiles, .. } => step_on(view, tools.rules, &tiles, reason),
        PathOutcome::NoPath => Decision::refuse(actor, Reason::NoPath),
        PathOutcome::OverBudget => Decision::refuse(actor, Reason::PathOverBudget),
    }
}

/// Foreign units and own units of the mover's stack class (but not on the
/// goal): a path that never needs to stop next to them.
fn blockers_with_own(view: &UnitView<'_, '_>, rules: &CompiledRuleset, goal: TileId) -> Blockers {
    let class = stack_class(rules, view.local.unit.kind);
    let own = view
        .local
        .own_units
        .iter()
        .filter(|u| u.tile != goal && stack_class(rules, u.kind) == class)
        .map(|u| u.tile);
    let foreign = view.local.foreign_units.iter().map(|u| u.tile);
    Blockers::new(own.chain(foreign).collect())
}

/// Move to the next tile of `path`; past an own unit standing there to the
/// tile after it, when the moves left reach beyond the first step.
pub fn step_on(
    view: &UnitView<'_, '_>,
    rules: &CompiledRuleset,
    path: &[TileId],
    reason: Reason,
) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    let mut destination = None;
    if let Some(&first) = path.get(1) {
        if can_stop(view, rules, first) {
            destination = Some(first);
        } else if let Some(&second) = path.get(2) {
            let map = KnownMap::of_unit(&view.local.view, unit.handle);
            let first_step = step_points(&map, unit.tile, first).unwrap_or(u32::MAX);
            if u32::from(unit.moves_left) > first_step && can_stop(view, rules, second) {
                destination = Some(second);
            }
        }
    }
    match destination {
        Some(tile) => {
            let command = Command::MoveUnit {
                unit: unit.handle,
                destination: tile,
            };
            Decision::act(actor, reason, command)
        }
        None => Decision::refuse(actor, Reason::Waiting),
    }
}

/// Move to the farthest tile of `path` the unit's moves left reach and it
/// may stop on; the next tile as `step_on` when no tile is reached whole.
fn stride_on(
    view: &UnitView<'_, '_>,
    rules: &CompiledRuleset,
    path: &[TileId],
    reason: Reason,
) -> Decision {
    let unit = view.local.unit;
    let map = KnownMap::of_unit(&view.local.view, unit.handle);
    let mut left = u32::from(unit.moves_left);
    let mut farthest = None;
    for leg in path.windows(2) {
        match step_points(&map, leg[0], leg[1]) {
            Some(points) if points <= left => left -= points,
            _ => break,
        }
        if can_stop(view, rules, leg[1]) {
            farthest = Some(leg[1]);
        }
    }
    match farthest {
        Some(tile) => Decision::act(
            Actor::Unit(unit.id),
            reason,
            Command::MarchTo { unit: unit.handle, destination: tile },
        ),
        None => step_on(view, rules, path, reason),
    }
}

/// Fortify, or nothing when already fortified (the unit's turn is over).
pub(crate) fn hold(view: &UnitView<'_, '_>, reason: Reason) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    if unit.fortified || !unit.can_fortify {
        return Decision::refuse(actor, reason);
    }
    Decision::act(actor, reason, Command::Fortify { unit: unit.handle })
}

fn decide_military(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
) -> Decision {
    if let Some(decision) = tactics::immediate(view, state, tools) {
        return decision;
    }
    follow_task(view, state, tools)
}

/// The military unit's step of its task (assigned, or checked and
/// replaced first): scout, garrison, assault, escort, heal or hold.
pub fn follow_task(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
) -> Decision {
    let task = ensure_task(view, state, tools);
    match task {
        Some(UnitTask::Scout { target }) => scout_step(view, state, tools, target),
        Some(UnitTask::Defend { city }) if view.brief.endangered(city) => crate::ai::defence::guard_step(view, tools, city),
        Some(UnitTask::Defend { city }) => defend_step(view, tools, city),
        Some(UnitTask::Assault { target }) => tactics::assault_step(view, state, tools, target),
        Some(UnitTask::Escort { ward }) => escort_step(view, tools, ward),
        Some(UnitTask::Heal { at }) => heal_step(view, state, tools, at),
        Some(UnitTask::Operation { target }) => crate::ai::operations::member_step(view, state, tools, target),
        _ => hold(view, Reason::Idle),
    }
}

/// The unit's task, checked and replaced when done or invalid.
pub(crate) fn ensure_task(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
) -> Option<UnitTask> {
    let unit = view.local.unit;
    let current = state.task_of(unit.id).map(|t| t.task);
    if let Some(task) = current {
        if task_valid(view, state, tools.rules, task) {
            return Some(task);
        }
        state.clear_task(unit.id);
        tools.counters.tasks_dropped += 1;
    }
    let task = assign_military_task(view, state, tools)?;
    state.set_task(unit.id, task, view.brief.round);
    tools.counters.tasks_assigned += 1;
    Some(task)
}

fn task_valid(view: &UnitView<'_, '_>, state: &AiState, rules: &CompiledRuleset, task: UnitTask) -> bool {
    let unit = view.local.unit;
    match task {
        UnitTask::Scout { target } => scouting::target_valid(view, rules, target),
        UnitTask::Defend { city } => {
            let ours = view.brief.cities.iter().any(|c| c.tile == city);
            ours && !surplus_defender(view, state, city)
                && !scout_wanted_instead(view, state)
                && settler_waiting_here(view, state).is_none()
        }
        UnitTask::Assault { target } => state
            .relations()
            .iter()
            .any(|m| m.at_war && m.objective.is_some_and(|o| o.tile == target)),
        UnitTask::Escort { ward } => escorted_settler_alive(view, state, ward),
        UnitTask::Heal { .. } => unit.hp < crate::world::unit::UNIT_MAX_HP,
        UnitTask::Operation { target } => state.operation(target).is_some(),
        UnitTask::Settle { .. } | UnitTask::Improve { .. } => false,
    }
}

/// A garrison holder beyond what its city wants (the higher ids go first),
/// while an assault objective or an empty city needs units: it is
/// released for a new task. A guard of a city in danger stays.
fn surplus_defender(view: &UnitView<'_, '_>, state: &AiState, city: TileId) -> bool {
    if view.brief.endangered(city) {
        return false;
    }
    let needed_elsewhere = assault_objective(state).is_some()
        || ungarrisoned_city(view, state).is_some()
        || scouting_reopened(view, state)
        || unescorted_settler(view, state).is_some();
    if !needed_elsewhere {
        return false;
    }
    let unit = view.local.unit.id;
    let senior = state
        .tasks()
        .iter()
        .filter(|t| t.task == UnitTask::Defend { city } && t.unit < unit)
        .count();
    senior >= garrison_target(view, state).max(1)
}

/// A recon unit holding a city while the empire wants a scout (new land
/// came within reach, e.g. over the sea once its units embark, or its
/// leader looks for a capital) goes back to scouting.
fn scout_wanted_instead(view: &UnitView<'_, '_>, state: &AiState) -> bool {
    let recon = is_recon(view.local.view.rules(), view.local.unit.kind);
    recon && scouting_reopened(view, state)
}

/// The empire has fewer scouts than it wants after the early game: its
/// units embark (the sea opened new land), or its leader looks for a
/// capital (`ai::capital_search`).
fn scouting_reopened(view: &UnitView<'_, '_>, state: &AiState) -> bool {
    let embarks = KnownMap::of_unit(&view.local.view, view.local.unit.handle).profile.embark != Embark::No;
    (embarks || view.brief.search.is_some()) && scouts(state) < scouts_wanted(view, state)
}

fn escorted_settler_alive(view: &UnitView<'_, '_>, state: &AiState, ward: EntityId) -> bool {
    let alive = view.brief.units.iter().any(|u| u.id == ward);
    let settling = matches!(
        state.task_of(ward).map(|t| t.task),
        Some(UnitTask::Settle { .. })
    );
    alive && settling
}

/// Garrison holders a city wants.
fn garrison_target(view: &UnitView<'_, '_>, state: &AiState) -> usize {
    garrison_wanted(
        view.brief.round,
        &view.brief.relations,
        state.strategy.current,
    )
}

/// Garrison units each city wants: two while defending, one in war or
/// once another civilization is known (from `GARRISON_FROM_ROUND`), one
/// for everybody from `GARRISON_ALONE_ROUND`; none before (the first units
/// explore).
pub fn garrison_wanted(
    round: u32,
    relations: &[crate::knowledge::observation::Relation],
    strategy: Strategy,
) -> usize {
    if strategy == Strategy::Defend {
        return 2;
    }
    let at_war = relations.iter().any(|r| r.at_war);
    let met = !relations.is_empty();
    if at_war || (met && round >= GARRISON_FROM_ROUND) || round >= GARRISON_ALONE_ROUND {
        return 1;
    }
    0
}

fn assign_military_task(
    view: &UnitView<'_, '_>,
    state: &AiState,
    tools: &mut PlanTools<'_>,
) -> Option<UnitTask> {
    let unit = view.local.unit;
    let dims = view.local.view.dimensions();
    if unit.hp < HEAL_BELOW_HP {
        if heals_where_it_stands(view, tools.rules) {
            return Some(UnitTask::Heal { at: unit.tile });
        }
        let city = view.brief.nearest_city(dims, unit.tile)?;
        return Some(UnitTask::Heal { at: city.tile });
    }
    // A settler waiting for an escort where this unit stands goes first
    // (the city then trains a new garrison).
    if let Some(ward) = settler_waiting_here(view, state) {
        return Some(UnitTask::Escort { ward });
    }
    let first_scout = scouts(state) == 0 && scouts_wanted(view, state) > 0;
    if is_recon(tools.rules, unit.kind) || first_scout {
        if let Some(target) = scout_target(view, state, tools) {
            return Some(UnitTask::Scout { target });
        }
    }
    if let Some(city) = ungarrisoned_city(view, state) {
        return Some(UnitTask::Defend { city });
    }
    if let Some(target) = assault_objective(state) {
        return Some(UnitTask::Assault { target });
    }
    if let Some(ward) = unescorted_settler(view, state) {
        return Some(UnitTask::Escort { ward });
    }
    if scouts(state) < scouts_wanted(view, state) {
        if let Some(target) = scout_target(view, state, tools) {
            return Some(UnitTask::Scout { target });
        }
    }
    let city = view.brief.nearest_city(dims, unit.tile)?;
    Some(UnitTask::Defend { city: city.tile })
}

/// A hurt scout of a leader looking for a capital heals where it stands
/// when no enemy is near: the walk home and back cost the search tens of
/// rounds (`ai::capital_search`).
fn heals_where_it_stands(view: &UnitView<'_, '_>, rules: &CompiledRuleset) -> bool {
    let unit = view.local.unit;
    view.brief.search.is_some() && is_recon(rules, unit.kind) && !threatened(view, rules, unit.tile, SETTLER_DANGER_RADIUS)
}

fn is_recon(rules: &CompiledRuleset, kind: crate::ids::UnitKindId) -> bool {
    rules.units.get(kind.0 as usize).and_then(|d| d.role) == Some(UnitRole::Recon)
}

/// The nearest own city with fewer garrison holders than wanted.
fn ungarrisoned_city(view: &UnitView<'_, '_>, state: &AiState) -> Option<TileId> {
    let target = garrison_target(view, state);
    let dims = view.local.view.dimensions();
    let unit = view.local.unit;
    view.brief
        .cities
        .iter()
        .filter(|c| state.holders(Reservation::Garrison(c.tile), Some(unit.id)) < target)
        .min_by_key(|c| (dims.distance(c.tile, unit.tile).unwrap_or(i32::MAX), c.id))
        .map(|c| c.tile)
}

fn assault_objective(state: &AiState) -> Option<TileId> {
    if state.strategy.current != Strategy::Attack {
        return None;
    }
    state
        .relations()
        .iter()
        .filter(|m| m.at_war)
        .find_map(|m| m.objective.map(|o| o.tile))
}

/// A settler on its way to a site, near this unit, that nobody escorts:
/// while any war runs, or when hostile units (barbarians too) are near it.
/// Without an escort against barbarians settlers waited in the city for
/// the rest of the game.
fn unescorted_settler(view: &UnitView<'_, '_>, state: &AiState) -> Option<EntityId> {
    let at_war = view.brief.relations.iter().any(|r| r.at_war);
    let rules = view.local.view.rules();
    let dims = view.local.view.dimensions();
    let unit = view.local.unit;
    let escorted = |id: EntityId| {
        state
            .tasks()
            .iter()
            .any(|t| t.task == UnitTask::Escort { ward: id } && t.unit != unit.id)
    };
    view.brief
        .units
        .iter()
        .filter(|u| {
            u.role == AiRole::Settler && dims.distance(u.tile, unit.tile).is_some_and(|d| d <= 3)
        })
        .filter(|u| {
            let met_danger = state.task_of(u.id).is_some_and(|t| t.failures > 0);
            at_war || met_danger || threatened(view, rules, u.tile, SETTLER_DANGER_RADIUS)
        })
        .filter(|u| {
            matches!(
                state.task_of(u.id).map(|t| t.task),
                Some(UnitTask::Settle { .. })
            )
        })
        .find(|u| !escorted(u.id))
        .map(|u| u.id)
}

/// An unescorted settler on this unit's tile that waits for an escort.
fn settler_waiting_here(view: &UnitView<'_, '_>, state: &AiState) -> Option<EntityId> {
    let unit = view.local.unit;
    let ward = unescorted_settler(view, state)?;
    let here = view.brief.units.iter().any(|u| u.id == ward && u.tile == unit.tile);
    here.then_some(ward)
}

/// Hostile military units this close to a settler call for an escort.
const SETTLER_DANGER_RADIUS: i32 = 3;

fn scouts(state: &AiState) -> usize {
    state
        .tasks()
        .iter()
        .filter(|t| matches!(t.task, UnitTask::Scout { .. }))
        .count()
}

/// Scouts the empire wants (`scouting::wanted`: by the share of the map
/// still unknown), none once the garrisons need every unit. Scouting ends
/// with the early game, or goes on over the sea once the units embark
/// (other continents and islands: the civilizations to meet are there), or
/// while a leader looks for a capital (`ai::capital_search`).
fn scouts_wanted(view: &UnitView<'_, '_>, state: &AiState) -> usize {
    let brief = view.brief;
    let embarks = KnownMap::of_unit(&view.local.view, view.local.unit.handle).profile.embark != Embark::No;
    let season = brief.round <= SCOUTING_ENDS_ROUND || embarks || brief.search.is_some();
    if !season || state.strategy.current == Strategy::Defend {
        return 0;
    }
    let military = brief.count_role(AiRole::Military);
    let garrisons = brief.cities.len() * garrison_target(view, state);
    let free = military.saturating_sub(garrisons);
    let wanted = scouting::wanted(brief.round, brief.known_percent);
    // One scout always, as long as another unit guards home (or early).
    let one_scout = usize::from(military >= 2 || garrison_target(view, state) == 0);
    wanted.min(free.max(one_scout))
}

fn scout_step(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
    target: TileId,
) -> Decision {
    let unit = view.local.unit;
    let refused_before = state.task_of(unit.id).is_some_and(|t| t.failures > 0);
    let decision = if refused_before {
        move_along(view, tools, target, Reason::Explore)
    } else {
        stride_along(view, tools, target, Reason::Explore)
    };
    if decision.command.is_some() || decision.reason == Reason::Waiting {
        return decision;
    }
    // The way is closed or units stand in it: give the target up for another
    // one next decision.
    state.clear_task(unit.id);
    tools.counters.tasks_dropped += 1;
    decision
}

fn defend_step(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, city: TileId) -> Decision {
    let unit = view.local.unit;
    let dims = view.local.view.dimensions();
    if unit.tile == city {
        return hold(view, Reason::Garrison);
    }
    let adjacent = dims.adjacent(unit.tile, city);
    if adjacent && !can_stop(view, tools.rules, city) {
        return hold(view, Reason::Garrison);
    }
    let decision = move_along(view, tools, city, Reason::Garrison);
    if decision.command.is_none() && !matches!(decision.reason, Reason::Waiting | Reason::Blocked) {
        return hold(view, Reason::Garrison);
    }
    decision
}

fn escort_step(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, ward: EntityId) -> Decision {
    let unit = view.local.unit;
    let near = view
        .local
        .own_units
        .iter()
        .find(|u| u.id == ward)
        .map(|u| u.tile);
    let far = view
        .brief
        .units
        .iter()
        .find(|u| u.id == ward)
        .map(|u| u.tile);
    let Some(settler) = near.or(far) else {
        return Decision::refuse(Actor::Unit(unit.id), Reason::TargetGone);
    };
    if settler == unit.tile {
        return Decision::refuse(Actor::Unit(unit.id), Reason::Escort);
    }
    move_along(view, tools, settler, Reason::Escort)
}

fn heal_step(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
    at: TileId,
) -> Decision {
    let unit = view.local.unit;
    if unit.tile == at
        || !can_stop(view, tools.rules, at) && view.local.view.dimensions().adjacent(unit.tile, at)
    {
        return hold(view, Reason::Heal);
    }
    let decision = move_along(view, tools, at, Reason::Heal);
    if decision.command.is_none() && !matches!(decision.reason, Reason::Waiting | Reason::Blocked) {
        state.clear_task(unit.id);
        return hold(view, Reason::Heal);
    }
    decision
}

/// Path from the unit to the nearest known tile next to `target` (a city
/// cannot be entered, it is attacked from a neighbour): a land unit stops on
/// land, since an embarked one cannot attack.
pub fn approach(
    view: &UnitView<'_, '_>,
    tools: &mut PlanTools<'_>,
    target: TileId,
    reason: Reason,
) -> Decision {
    approach_at(view, tools, target, 1, false, reason)
}

/// Path from the unit to the nearest known tile `range` tiles from
/// `target`, on the water for a ship and on land for the rest.
pub fn approach_at(
    view: &UnitView<'_, '_>,
    tools: &mut PlanTools<'_>,
    target: TileId,
    range: i32,
    on_water: bool,
    reason: Reason,
) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    if unit.moves_left == 0 {
        return Decision::refuse(actor, Reason::Waiting);
    }
    let dims = view.local.view.dimensions();
    let blockers = blockers(view);
    let limit = tools.settings.budget.search();
    let kinds = view.local.view.terrain_kinds();
    let mut goal = None;
    let mut visit = |tile: TileId, _points: u32| {
        let water = view.local.view.tile(tile).snapshot().is_some_and(|s| kinds.is_water(s.terrain.0));
        if dims.distance(tile, target) == Some(range) && water == on_water {
            goal = Some(tile);
            return Visit::Stop;
        }
        Visit::Continue
    };
    explore(
        &KnownMap::of_unit(&view.local.view, unit.handle),
        unit.tile,
        &blockers,
        tools.scratch,
        limit,
        &mut tools.counters.search,
        &mut visit,
    );
    let Some(goal) = goal else {
        return Decision::refuse(actor, Reason::NoPath);
    };
    match explored_path(tools.scratch, unit.tile, goal) {
        Some(path) if path.len() > 1 => step_on(view, tools.rules, &path, reason),
        _ => Decision::refuse(actor, Reason::Rally),
    }
}
