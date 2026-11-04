//! The unit net (`unit-net`, `hybrid`; classic/docs/ai-unit-net.md; the
//! Russian design work/unit-net-design.html and the map of the mini nets
//! work/mini-nets-map.html): two hand-weighted mini nets in place of
//! `standard`'s tactical and operations layer.
//!
//! - The **commander** (`commander`) at the start of a turn: up to eight
//!   operations — the strategy's war operations (`ai::operations` forms
//!   them, declares the war and recruits; the commander runs their phases
//!   by a Lanchester head with hysteresis), the defence of the threatened
//!   cities, the camps — and the orders the cities read. At a theatre's
//!   first decision the turn's plan (TurnPlan): the target values with the
//!   key target, the greedy fire allocation by marginal value, the cover of
//!   the weak, the enemy's reply checked twice, the queue; after each
//!   action the fast update. The kernel asks it "who is next"
//!   (`AiMind::next_unit`).
//! - The **tactician** (`tactician`): each unit's candidates scored by the
//!   motives of its role and phase over the theatre's 24 channels and 14
//!   hand-convolved maps (`fields`), following its assignment.
//!
//! Units outside every theatre (no operation, no foe within reach, a
//! civilian out of danger) decide as in `standard`. Every weight is in
//! `weights`, with its reason. Integers only; deterministic.

pub mod commander;
pub mod fields;
pub mod math;
pub mod memory;
pub mod tactician;
pub mod trace;
pub mod weights;

use std::sync::Mutex;

use self::commander::{Op, OpKind, Order, Phase, TurnPlan};
use self::fields::{fighter, hostile, Theatre};
use self::math::ONE;
use crate::ai::planner::{role_of, AiRole, Decision, PlanTools, TurnBrief, UnitView};
use crate::ai::state::{AiState, UnitTask};
use crate::ids::{EntityId, TileId, UnitHandle};
use crate::knowledge::observation::{AIObservation, LocalObservation};
use crate::rules::CompiledRuleset;

/// Thousandths: 1000 is 1.0.
pub type Milli = i64;

/// The prices and risk of the turn (design section 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prices {
    /// Index into `weights::prices::POSTURES`.
    pub posture: usize,
    pub kill: Milli,
    pub city: Milli,
    pub damage: Milli,
    pub unit: Milli,
    pub risk: Milli,
}

impl Prices {
    pub fn of_posture(posture: usize) -> Self {
        let p = weights::prices::POSTURES[posture.min(3)];
        Self { posture: posture.min(3), kill: p.kill, city: p.city, damage: p.damage, unit: p.unit, risk: p.risk }
    }
}

/// The posture of the turn: the strategy net's when it plays (`hybrid`),
/// else read from `standard`'s war state — at war with a nation or a
/// city-state: war; a war operation forming: prepare; a city in danger:
/// defence; else peace.
pub fn posture(brief: &TurnBrief, state: &AiState) -> usize {
    if let Some(plan) = &brief.strategy {
        return plan.intent.posture_pick;
    }
    use crate::world::player::{ClassicKind, PlayerKind};
    let war = brief.relations.iter().any(|r| r.at_war && r.kind != PlayerKind::Module(ClassicKind::Barbarian));
    if war {
        3
    } else if !state.operations().is_empty() {
        2
    } else if brief.defence.as_ref().is_some_and(|w| !w.dangers().is_empty()) {
        1
    } else {
        0
    }
}

/// The unit net's part of the turn's brief.
#[derive(Debug)]
pub struct UnitNetTurn {
    pub prices: Prices,
    pub ops: Vec<Op>,
    pub orders: Vec<Order>,
    /// Some own fighter is near a hostile unit or city, or an operation runs:
    /// the kernel asks "who is next".
    pub contact: bool,
    /// The own fighters within `ENGAGE` of a hostile unit or city at the
    /// start of the turn.
    pub engaged: Vec<EntityId>,
    /// The turn's plan, made at a theatre's first decision, updated after
    /// each action.
    pub plan: Mutex<Option<TurnPlan>>,
    /// Units deferred this turn, with their count.
    pub deferred: Mutex<Vec<(EntityId, u32)>>,
}

impl UnitNetTurn {
    pub fn plan(&self) -> Option<TurnPlan> {
        self.plan.lock().ok().and_then(|p| p.clone())
    }

    /// The operation a unit serves in.
    pub fn op_of(&self, unit: EntityId) -> Option<&Op> {
        self.ops.iter().find(|o| o.members.contains(&unit))
    }
}

/// The commander's start of a turn: the operations and their phases, the
/// orders, whether a theatre is in contact. The memory is the commander's
/// part of `AiState::mind_memory` (`memory::split`).
pub fn begin(sight: &AIObservation<'_>, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset) -> UnitNetTurn {
    let (strategy, own) = memory::split(&state.mind_memory);
    let mut memo = memory::UnitMemory::decode(&own);
    let prices = Prices::of_posture(posture(brief, state));
    let ops = commander::operations(brief, state, rules, &mut memo, &prices);
    state.mind_memory = memory::join(&strategy, &memo.encode());
    let orders = commander::orders(brief, &ops);
    let dims = brief.dims;
    // In contact: a hostile fighter within ENGAGE, a hostile city within CITY_REACH (its
    // fire, a unit about to assault), or, for an operation's member, a hostile city within
    // ENGAGE. A unit beside no enemy that is in no operation stays with the strategy: it is
    // free to join the next operation (a fortified unit is held, and no operation forms).
    let hostile_units: Vec<TileId> = sight
        .visible_foreign_units
        .iter()
        .filter(|u| hostile(&sight.relations, u.owner) && rules.units.get(u.kind.0 as usize).is_some_and(|d| d.strength > 0 || d.ranged_strength > 0))
        .map(|u| u.tile)
        .collect();
    let hostile_cities: Vec<TileId> = sight.visible_foreign_cities.iter().filter(|c| hostile(&sight.relations, c.owner)).map(|c| c.tile).collect();
    let within = |tiles: &[TileId], at: TileId, radius: i32| tiles.iter().any(|t| dims.distance(*t, at).is_some_and(|d| d <= radius));
    let in_operation = |id: EntityId| ops.iter().any(|o| o.members.contains(&id));
    let engaged: Vec<EntityId> = sight
        .own_units
        .iter()
        .filter(|u| fighter(rules, u.kind))
        .filter(|u| within(&hostile_units, u.tile, weights::fields::ENGAGE) || within(&hostile_cities, u.tile, weights::fields::CITY_REACH) || (in_operation(u.id) && within(&hostile_cities, u.tile, weights::fields::ENGAGE)))
        .map(|u| u.id)
        .collect();
    let contact = !engaged.is_empty() || ops.iter().any(|o| !o.members.is_empty());
    trace::turn(brief, &prices, &ops, &orders);
    UnitNetTurn { prices, ops, orders, contact, engaged, plan: Mutex::new(None), deferred: Mutex::new(Vec::new()) }
}

/// The units of the opening the tactician owns (their shots and moves are
/// its own): the land fighters of the operations and those in contact.
pub fn owns_command(turn: &UnitNetTurn, sight: &AIObservation<'_>, rules: &CompiledRuleset, command: &crate::command::Command) -> bool {
    let Some(handle) = crate::knowledge::actions::acting_unit(command) else { return false };
    let Some(u) = sight.own_units.iter().find(|u| u.handle == handle) else { return false };
    if !fighter(rules, u.kind) {
        return false;
    }
    use crate::command::Command as C;
    let military_order = matches!(command, C::MoveUnit { .. } | C::MarchTo { .. } | C::AttackUnit { .. } | C::AttackCity { .. } | C::RangedAttack { .. } | C::SetUpRangedAttack { .. } | C::Fortify { .. });
    military_order && (turn.op_of(u.id).is_some() || turn.engaged.contains(&u.id))
}

/// The job tile of a civilian (its settle site or work tile) for m5.
fn job(state: &AiState, unit: EntityId) -> Option<TileId> {
    match state.task_of(unit)?.task {
        UnitTask::Settle { site } => Some(site),
        UnitTask::Improve { tile, .. } => Some(tile),
        // A fighter's work from the planners (the attack it serves, the raid): its goal outside
        // an operation of the commander's.
        UnitTask::Assault { target } | UnitTask::Operation { target } | UnitTask::Raid { target } => Some(target),
        _ => None,
    }
}

/// Is the unit the tactician's this turn: a land fighter of an operation or
/// with a hostile unit or city within reach; a civilian with a hostile
/// fighter within 3 tiles.
fn in_theatre(view: &UnitView<'_, '_>, turn: &UnitNetTurn, rules: &CompiledRuleset) -> bool {
    let local = view.local;
    let unit = local.unit;
    let dims = local.view.dimensions();
    let hostile_near = |radius: i32, fighters_only: bool| {
        local.foreign_units.iter().any(|f| hostile(&local.relations, f.owner) && (!fighters_only || rules.units.get(f.kind.0 as usize).is_some_and(|d| d.strength > 0 || d.ranged_strength > 0)) && dims.distance(f.tile, unit.tile).is_some_and(|d| d <= radius))
            || (!fighters_only && local.foreign_cities.iter().any(|c| hostile(&local.relations, c.owner) && dims.distance(c.tile, unit.tile).is_some_and(|d| d <= radius)))
    };
    if fighter(rules, unit.kind) {
        let fighting = turn.op_of(unit.id).is_some_and(|o| matches!(o.phase, Phase::Assault | Phase::Hold));
        return fighting || turn.engaged.contains(&unit.id);
    }
    // A civilian: only when a hostile fighter is close.
    matches!(role_of(rules, unit.kind), AiRole::Settler | AiRole::Worker | AiRole::Civilian) && hostile_near(3, true)
}

/// The march of an operation's member out of contact (the commander's
/// phase; the paths are the planners'): gathering on the march, to the
/// rally; approaching, a member more than `LEAD` tiles ahead of the group's
/// median waits for the others (no trickle); retreating, home. None: the
/// planners' own step (the muster of a forming operation, the march).
fn march(view: &UnitView<'_, '_>, turn: &UnitNetTurn, state: &AiState, tools: &mut PlanTools<'_>) -> Option<Decision> {
    let unit = view.local.unit;
    let op = turn.op_of(unit.id)?;
    let dims = view.brief.dims;
    let actor = crate::ai::trace::Actor::Unit(unit.id);
    let forming = state.operation(op.target).is_some_and(|o| o.phase == crate::ai::state::OperationPhase::Forming);
    match op.phase {
        Phase::Gather if !forming && op.kind == OpKind::Attack => {
            if dims.distance(unit.tile, op.rally).is_some_and(|d| d <= 1) {
                return Some(Decision::refuse(actor, crate::ai::trace::Reason::Rally));
            }
            Some(crate::ai::planner_units::move_along(view, tools, op.rally, crate::ai::trace::Reason::Rally))
        }
        Phase::Approach => {
            let mut distances: Vec<i32> = op.members.iter().filter_map(|id| view.brief.units.iter().find(|u| u.id == *id)).filter_map(|u| dims.distance(u.tile, op.target)).collect();
            distances.sort();
            let median = *distances.get(distances.len() / 2)?;
            let mine = dims.distance(unit.tile, op.target)?;
            (mine + weights::op::LEAD < median).then(|| Decision::refuse(actor, crate::ai::trace::Reason::Rally))
        }
        Phase::Retreat => {
            let home = view.brief.nearest_city(dims, unit.tile)?.tile;
            if home == unit.tile {
                return None;
            }
            Some(crate::ai::planner_units::move_along(view, tools, home, crate::ai::trace::Reason::Retreat))
        }
        _ => None,
    }
}

/// One decision of a unit by the tactician (None: not its unit this turn).
pub fn decide(view: &UnitView<'_, '_>, state: &mut AiState, tools: &mut PlanTools<'_>) -> Option<Decision> {
    let rules = tools.rules;
    let turn = view.brief.unit_net.as_ref()?;
    if !in_theatre(view, turn, rules) {
        return if fighter(rules, view.local.unit.kind) { march(view, turn, state, tools) } else { None };
    }
    let _span = prof::span("classic/unit_net_decide");
    let local = view.local;
    let picture = tactician::picture(view, state, &turn.ops);
    let theatre = Theatre::build(&local.view, rules, view.brief, &local.relations, &picture);
    // The plan: made at the first decision of the turn's theatre, else updated.
    let plan = {
        let mut guard = turn.plan.lock().ok()?;
        refresh(&mut guard, &theatre, view.brief, rules, &turn.prices, &local.relations, None);
        guard.clone()
    };
    let deferred = turn.deferred.lock().ok().and_then(|d| d.iter().find(|e| e.0 == local.unit.id).map(|e| e.1)).unwrap_or(0);
    let may_defer = deferred < kernel::ai::driver::REORDERS_PER_UNIT;
    let (decision, verdict) = tactician::decide(view, rules, &theatre, plan.as_ref(), &turn.ops, &turn.prices, may_defer, job(state, local.unit.id))?;
    if decision.reason == crate::ai::trace::Reason::Deferred {
        if let Ok(mut d) = turn.deferred.lock() {
            match d.iter_mut().find(|e| e.0 == local.unit.id) {
                Some(e) => e.1 += 1,
                None => d.push((local.unit.id, 1)),
            }
        }
    }
    trace::decision(view.brief, &local.unit, &verdict, plan.as_ref());
    Some(decision)
}

/// The plan made (no plan yet, or new targets in sight) or brought up to
/// date by the fast update; `remaining`: the units still to decide (the
/// others are spent).
fn refresh(slot: &mut Option<TurnPlan>, theatre: &Theatre<'_, '_>, brief: &TurnBrief, rules: &CompiledRuleset, prices: &Prices, relations: &[crate::knowledge::observation::Relation], remaining: Option<&[(EntityId, UnitHandle)]>) {
    let units = commander::plan_units(theatre, rules, relations);
    let fresh_targets = |plan: &TurnPlan| theatre.field.targets.iter().any(|t| !t.remembered && t.hp > 0 && !plan.targets.iter().any(|p| p.tile == t.tile) && !plan.options.iter().any(|s| s.target_tile == t.tile));
    let rebuild = match slot.as_ref() {
        None => true,
        Some(plan) => plan.round != brief.round || (fresh_targets(plan) && theatre.strikes.iter().any(|s| units.contains(&s.unit))),
    };
    if rebuild {
        let mut plan = commander::plan(theatre, brief, rules, prices, &units);
        if let Some(old) = slot.as_ref().filter(|p| p.round == brief.round) {
            plan.spent = old.spent.clone();
            plan.updates = old.updates + 1;
        }
        trace::plan(brief, &plan, "made");
        *slot = Some(plan);
        return;
    }
    let Some(plan) = slot.as_mut() else { return };
    let seen: Vec<(TileId, u16)> = theatre.field.targets.iter().filter(|t| !t.remembered).map(|t| (t.tile, t.hp)).collect();
    let gone: Vec<TileId> = plan.targets.iter().filter(|t| theatre.field.tile(t.tile).is_some_and(|i| i.visible) && !seen.iter().any(|s| s.0 == t.tile)).map(|t| t.tile).collect();
    let spent: Vec<EntityId> = theatre
        .own
        .iter()
        .filter(|u| units.contains(&u.id) && (!u.can_attack || remaining.is_some_and(|r| !r.iter().any(|e| e.0 == u.id))))
        .map(|u| u.id)
        .collect();
    let healths = commander::healths(theatre, rules, &units);
    plan.at = theatre.own.iter().filter(|u| fighter(rules, u.kind)).map(|u| (u.id, u.tile)).collect();
    commander::update(plan, &seen, &gone, &spent, &healths, prices);
}

/// The fast update from a fresh observation alone (no theatre built): the
/// targets' health as seen now, the plan's units that acted or are done.
fn light_update(plan: &mut TurnPlan, near: &LocalObservation<'_>, rules: &CompiledRuleset, prices: &Prices, remaining: &[(EntityId, UnitHandle)]) {
    let relations = &near.relations;
    // A city's health, not its garrison's: the units standing in a city are left out.
    let cities: Vec<(TileId, u16)> = near.foreign_cities.iter().filter(|c| hostile(relations, c.owner)).map(|c| (c.tile, c.hp)).collect();
    let seen: Vec<(TileId, u16)> = cities
        .iter()
        .copied()
        .chain(near.foreign_units.iter().filter(|u| hostile(relations, u.owner) && !near.foreign_cities.iter().any(|c| c.tile == u.tile)).map(|u| (u.tile, u.hp)))
        .collect();
    let gone: Vec<TileId> = plan.targets.iter().filter(|t| near.view.is_visible(t.tile) && !seen.iter().any(|s| s.0 == t.tile)).map(|t| t.tile).collect();
    let mut own: Vec<crate::knowledge::observation::OwnUnit> = near.own_units.clone();
    own.push(near.unit);
    let units: Vec<EntityId> = plan.options.iter().map(|s| s.unit).chain(plan.assignments.iter().map(|a| a.unit)).collect();
    let spent: Vec<EntityId> = units.iter().copied().filter(|id| !remaining.iter().any(|r| r.0 == *id) || own.iter().any(|u| u.id == *id && !u.can_attack)).collect();
    let healths: Vec<(EntityId, u16, Milli)> = own.iter().filter(|u| units.contains(&u.id)).map(|u| (u.id, u.hp, commander::unit_value(rules, u.kind))).collect();
    plan.at = own.iter().filter(|u| fighter(rules, u.kind)).map(|u| (u.id, u.tile)).collect();
    commander::update(plan, &seen, &gone, &spent, &healths, prices);
}

/// "Who is next" (`AiMind::next_unit`): when the queue's head is a unit of
/// the plan, the first unit of the plan's queue still to decide; else the
/// head (units outside the plan keep the driver's order).
pub fn next_unit(near: &LocalObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, remaining: &[(EntityId, UnitHandle)]) -> Option<EntityId> {
    let turn = brief.unit_net.as_ref()?;
    let head = remaining.first()?.0;
    let view = UnitView { local: near, brief };
    if !in_theatre(&view, turn, rules) || !fighter(rules, near.unit.kind) {
        return None;
    }
    let mut guard = turn.plan.lock().ok()?;
    if guard.as_ref().is_none_or(|p| p.round != brief.round) {
        let picture = tactician::picture(&view, state, &turn.ops);
        let theatre = Theatre::build(&near.view, rules, brief, &near.relations, &picture);
        refresh(&mut guard, &theatre, brief, rules, &turn.prices, &near.relations, Some(remaining));
    } else if let Some(plan) = guard.as_mut() {
        light_update(plan, near, rules, &turn.prices, remaining);
    }
    let plan = guard.as_ref()?;
    if !plan.queue.contains(&head) && !plan.assignments.iter().any(|a| a.unit == head) {
        // The head has no task: the plan's units go first only when the head is one of them.
        return None;
    }
    let next = plan.queue.iter().find(|id| remaining.iter().any(|r| r.0 == **id)).copied();
    trace::next(brief, head, next, plan);
    next.filter(|n| *n != head)
}

/// The orders' share of a city's production (the allocator's hook): the
/// value of a military unit in `city` this turn.
pub fn order_value(turn: &UnitNetTurn, city: TileId, dims: crate::math::MapDimensions) -> i64 {
    turn.orders.iter().filter(|o| dims.distance(o.city, city).is_some_and(|d| d <= weights::op::DEFEND_RADIUS)).map(|o| o.value).max().unwrap_or(0)
}

/// Kept for `ONE`'s import in the docs.
pub const UNIT: Milli = ONE;
