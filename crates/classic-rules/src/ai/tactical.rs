//! The tactical layer of the variant `tactical` (classic/docs/ai-tactical.md,
//! docs/ai-war-design.md 4.4): one plan per turn and theatre instead of a
//! strike picker that starts again at every unit.
//!
//! At the start of the turn (`Tactics::observe`, from the whole-turn
//! observation) the layer keeps a snapshot of the theatre: the enemy units
//! and cities in sight, the front members near them. At the first member's
//! decision the plan is made (`build_plan`, within the decision budget):
//! the battlefield model (`ai::battlefield`), the roles (`ai::roles`), the
//! options of every member with the rules' own previews, and the beam search
//! (`ai::plan_search`). The plan is a list of steps in execution order —
//! withdrawals, ranged strikes, melee strikes with the capture last, then the
//! moves of everybody else — kept in the turn's brief. Every later unit
//! decision executes the next step (which may belong to another member, as
//! the front's steps do) after the rules' own check of the command; a step
//! whose target is gone is repaired from the member's alternatives or
//! dropped, never re-planned from a partial view.
//!
//! The plan lives for the turn only and nothing is added to the saved
//! memory: the next turn's plan is made from the next turn's observation by
//! the same deterministic search, and the continuity between turns is the
//! positions themselves (units already on the good ground and in the line
//! stay), the war objective and the tasks of the memory (`Heal` for a unit
//! that withdrew).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::ai::battlefield::{worth, Battlefield, KindStats, Posture, Target};
use crate::ai::front::{is_member, siege_rounds_within, Besieger, SIEGE_ROUNDS};
use crate::ai::plan_search::{self, Act, Member, Opt, Problem, Weights, MAX_GOES, MAX_STRIKES, STRIKE_TILES};
use crate::ai::planner::{role_of, AiRole, Decision, PlanTools, TurnBrief, UnitView};
use crate::ai::roles::{self, Role};
use crate::ai::state::{AiState, UnitTask};
use crate::ai::trace::{Actor, Reason};
use crate::world::unit::{MOVE_DENOMINATOR, UNIT_MAX_HP};
use crate::combat::city::CITY_HEAL_PER_TURN;
use crate::command::Command;
use crate::ids::{CityHandle, EntityId, TileId, UnitHandle};
use crate::knowledge::observation::{AIObservation, ObservedCity, ObservedUnit, OwnUnit};
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;

/// Members within this many tiles of an enemy join the plan.
pub const ENGAGE_RADIUS: i32 = 7;
/// Members within this many tiles of the city count for the siege group: the
/// ones that can join it in a round or two (the front counts those within
/// `GROUP_RADIUS`).
pub const READY_RADIUS: i32 = 6;
/// A member this close to the city (hit points aside) commits the group; the
/// siege test then allows this many rounds.
pub const COMMITTED_DISTANCE: i32 = 2;
pub const COMMITTED_ROUNDS: u32 = 8;
/// Members a plan holds.
pub const MAX_MEMBERS: usize = 16;
/// A ranged unit leaves the line only below this health.
pub const RANGED_WITHDRAW_BELOW: u16 = 30;
/// A melee unit this many hit points above its threshold, out of the enemy's
/// reach, keeps resting.
pub const REST_BAND: u16 = 20;
/// Points of a withdrawal tile's rating per hit point a turn of rest heals
/// more there than on land nobody owns (own land and cities heal faster).
/// Own design: a rest of 10 more points a turn is worth about as much as two points of the enemy's expected blow.
pub const REST_HEAL_WEIGHT: i64 = 2;
/// Tiles around members and enemies the model covers.
pub const THEATRE_RADIUS: i32 = 5;
/// Evaluations of the search per candidate of the decision budget.
pub const EVALUATIONS_PER_CANDIDATE: u32 = 24;
/// Previews of the options per candidate of the decision budget.
pub const PREVIEWS_PER_CANDIDATE: u32 = 5;
/// Previews the siege test may use besides.
pub const SIEGE_PREVIEWS: u32 = 48;

/// The snapshot of the theatre at the start of the turn.
#[derive(Debug, Clone)]
pub struct TheatreInput {
    pub round: u32,
    /// Every own military land unit: the members and the bodies around them.
    pub own: Vec<OwnUnit>,
    /// The persistent ids of the members, sorted.
    pub members: Vec<EntityId>,
    pub foes: Vec<ObservedUnit>,
    pub cities: Vec<ObservedCity>,
    /// Cities of players at war with us seen before and out of sight now:
    /// they strike whoever comes within their range.
    pub remembered: Vec<crate::ai::state::SeenCity>,
    pub own_cities: Vec<TileId>,
}

/// The tactical layer's part of the turn's brief.
#[derive(Debug, Default)]
pub struct Tactics {
    input: Option<TheatreInput>,
    plan: Mutex<Option<TheatrePlan>>,
}

impl Clone for Tactics {
    fn clone(&self) -> Self {
        let plan = self.plan.lock().map(|p| p.clone()).unwrap_or(None);
        Self { input: self.input.clone(), plan: Mutex::new(plan) }
    }
}

impl Tactics {
    /// The theatre of the turn, from the whole-turn observation (none: no
    /// front member near an enemy, nothing for the layer to do).
    pub fn observe(sight: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> Self {
        let at_war = |owner| sight.relation(owner).is_some_and(|r| r.at_war);
        let foes: Vec<ObservedUnit> = sight.visible_foreign_units.iter().filter(|u| at_war(u.owner)).copied().collect();
        let cities: Vec<ObservedCity> = sight.visible_foreign_cities.iter().filter(|c| at_war(c.owner)).copied().collect();
        let dims = brief.dims;
        let own: Vec<OwnUnit> = sight.own_units.iter().filter(|u| role_of(rules, u.kind) == AiRole::Military).copied().collect();
        // Cities of players at war with us that are out of sight now: they strike
        // whoever comes within their range, and a siege goes on around them.
        let remembered: Vec<crate::ai::state::SeenCity> = state
            .seen_cities()
            .iter()
            .filter(|c| at_war(c.owner) && !cities.iter().any(|v| v.tile == c.tile))
            .filter(|c| own.iter().any(|u| dims.distance(u.tile, c.tile).is_some_and(|d| d <= ENGAGE_RADIUS + 3)))
            .copied()
            .collect();
        if foes.is_empty() && cities.is_empty() && remembered.is_empty() {
            return Self::default();
        }
        let near_foe = |tile: TileId| {
            foes.iter()
                .map(|u| u.tile)
                .chain(cities.iter().map(|c| c.tile))
                .chain(remembered.iter().map(|c| c.tile))
                .filter_map(|t| dims.distance(t, tile))
                .min()
                .unwrap_or(i32::MAX)
        };
        let mut members: Vec<EntityId> = own
            .iter()
            .filter(|u| {
                let task = state.task_of(u.id).map(|t| t.task);
                (is_member(state, task) || matches!(task, Some(UnitTask::Heal { .. }))) && near_foe(u.tile) <= ENGAGE_RADIUS
            })
            .map(|u| u.id)
            .collect();
        if members.is_empty() || !own.iter().any(|u| members.contains(&u.id) && !matches!(state.task_of(u.id).map(|t| t.task), Some(UnitTask::Heal { .. }))) {
            return Self::default();
        }
        members.sort();
        Self {
            input: Some(TheatreInput {
                round: sight.round,
                own,
                members,
                foes,
                cities,
                remembered,
                own_cities: sight.own_cities.iter().map(|c| c.tile).collect(),
            }),
            plan: Mutex::new(None),
        }
    }

    pub fn has_theatre(&self) -> bool {
        self.input.is_some()
    }

    /// Does the plan command this unit (a member of the theatre)? The
    /// start-of-turn advisors leave its shots and set-up to the plan.
    pub fn commands(&self, unit: UnitHandle) -> bool {
        let Some(input) = self.input.as_ref() else { return false };
        let Some(own) = input.own.iter().find(|u| u.handle == unit) else { return false };
        match self.plan.lock() {
            Ok(plan) => match plan.as_ref() {
                Some(plan) => plan.standing_at(own.id).is_some(),
                None => input.members.binary_search(&own.id).is_ok(),
            },
            Err(_) => false,
        }
    }

    /// The plan, once made (reports and tests).
    pub fn plan(&self) -> Option<TheatrePlan> {
        self.plan.lock().ok().and_then(|p| p.clone())
    }
}

/// An alternative blow of a member when its target is gone: another target
/// from the tile it strikes from.
#[derive(Debug, Clone, Copy)]
pub struct Alt {
    pub tile: TileId,
    pub unit: Option<UnitHandle>,
    pub city: Option<CityHandle>,
}

#[derive(Debug, Clone)]
pub enum StepKind {
    Strike { tile: TileId, unit: Option<UnitHandle>, city: Option<CityHandle>, from: TileId, ranged: bool, alts: Vec<Alt> },
    Go { to: TileId },
    Withdraw { to: TileId, home: Option<TileId> },
    Hold,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub unit: EntityId,
    pub handle: UnitHandle,
    pub kind: StepKind,
    pub done: bool,
    /// Moves sent for this step: a unit that is still not there after one
    /// (a zone of control stopped it) gives the step up.
    pub moves: u8,
}

/// The plan of one turn of one theatre.
#[derive(Debug, Clone)]
pub struct TheatrePlan {
    pub steps: Vec<Step>,
    /// Members of the plan, with where they stand as the plan has moved them.
    pub standing: Vec<(EntityId, TileId)>,
    pub roles: Vec<(EntityId, Role)>,
    pub evaluations: u32,
    pub expected: ExpectedOutcome,
    /// Steps given up because the target was gone or the rules refused.
    pub repairs: u32,
}

/// What the search expected of its plan (for reports and tests).
#[derive(Debug, Clone, Default)]
pub struct ExpectedOutcome {
    pub score: i64,
    pub kills: u32,
    pub captures: u32,
    pub dealt: i64,
    pub taken: i64,
    pub reply: i64,
    pub lethal: u32,
    pub screened: u32,
    pub artillery: u32,
}

/// Diagnostics of the plans made in this process: how many, the wall time
/// they took in nanoseconds in all and the longest, and the evaluations of
/// the search. Never read by a decision.
pub static PLANS: AtomicU64 = AtomicU64::new(0);
pub static PLAN_NANOS: AtomicU64 = AtomicU64::new(0);
pub static PLAN_MAX_NANOS: AtomicU64 = AtomicU64::new(0);
pub static PLAN_EVALUATIONS: AtomicU64 = AtomicU64::new(0);
pub static PLAN_MEMBERS: AtomicU64 = AtomicU64::new(0);

/// Diagnostics (temporary): plans with a focus city, ready, with a strike at a
/// city, with a planned capture, with a withdrawal, committed.
pub static DIAG: [AtomicU64; 12] = [const { AtomicU64::new(0) }; 12];

/// (plans, total ns, longest ns, evaluations, members) since the process began.
pub fn plan_stats() -> (u64, u64, u64, u64, u64) {
    (PLANS.load(Ordering::Relaxed), PLAN_NANOS.load(Ordering::Relaxed), PLAN_MAX_NANOS.load(Ordering::Relaxed), PLAN_EVALUATIONS.load(Ordering::Relaxed), PLAN_MEMBERS.load(Ordering::Relaxed))
}

/// The tactical decision for the asked unit: the next step of the turn's
/// plan (the plan is made at the first call). None: the unit is not the
/// layer's, the front or the planners decide.
pub fn decide(view: &UnitView<'_, '_>, state: &mut AiState, tools: &mut PlanTools<'_>) -> Option<Decision> {
    let tactics = view.brief.tactics.as_ref()?;
    let input = tactics.input.as_ref()?;
    let unit = view.local.unit;
    if role_of(tools.rules, unit.kind) != AiRole::Military {
        return None;
    }
    let mut guard = tactics.plan.lock().ok()?;
    if guard.is_none() {
        let _span = prof::span("classic/tactical_plan");
        let started = prof::now_ns();
        let plan = build_plan(view, state, tools, input);
        let nanos = prof::now_ns().saturating_sub(started);
        PLANS.fetch_add(1, Ordering::Relaxed);
        PLAN_NANOS.fetch_add(nanos, Ordering::Relaxed);
        PLAN_MAX_NANOS.fetch_max(nanos, Ordering::Relaxed);
        PLAN_EVALUATIONS.fetch_add(u64::from(plan.evaluations), Ordering::Relaxed);
        PLAN_MEMBERS.fetch_add(plan.standing.len() as u64, Ordering::Relaxed);
        *guard = Some(plan);
    }
    let plan = guard.as_mut()?;
    if let Some(decision) = plan.next(view, state, tools.rules) {
        return Some(decision);
    }
    // Nothing is left to do for the asked unit: a member of the plan is done.
    plan.standing_at(unit.id).is_some().then(|| Decision::refuse(Actor::Unit(unit.id), Reason::Rally))
}

impl TheatrePlan {
    fn standing_at(&self, unit: EntityId) -> Option<TileId> {
        self.standing.iter().find(|(u, _)| *u == unit).map(|(_, t)| *t)
    }

    fn set_standing(&mut self, unit: EntityId, tile: TileId) {
        if let Some(entry) = self.standing.iter_mut().find(|(u, _)| *u == unit) {
            entry.1 = tile;
        }
    }

    /// The next command of the plan, repairing the steps the world has
    /// changed (a dead target, a refused order).
    fn next(&mut self, view: &UnitView<'_, '_>, state: &mut AiState, _rules: &CompiledRuleset) -> Option<Decision> {
        let pv = &view.local.view;
        for index in 0..self.steps.len() {
            if self.steps[index].done {
                continue;
            }
            let (unit, handle) = (self.steps[index].unit, self.steps[index].handle);
            // The asked unit and the units around it are known; the others as the plan has moved them.
            let at = [Some(view.local.unit)]
                .into_iter()
                .flatten()
                .chain(view.local.own_units.iter().copied())
                .find(|u| u.id == unit)
                .map(|u| u.tile)
                .or_else(|| self.standing_at(unit));
            let Some(at) = at else {
                self.steps[index].done = true;
                continue;
            };
            let actor = Actor::Unit(unit);
            match self.steps[index].kind.clone() {
                StepKind::Hold => {
                    // A unit fortifies when it is asked itself, after the others' steps.
                    let u = view.local.unit;
                    if u.id != unit {
                        continue;
                    }
                    self.steps[index].done = true;
                    if u.can_fortify && !u.fortified {
                        let command = Command::Fortify { unit: handle };
                        if pv.accepts(&command) {
                            return Some(Decision::act(actor, Reason::Fortify, command));
                        }
                    }
                }
                StepKind::Go { to } | StepKind::Withdraw { to, .. } => {
                    let withdraw = matches!(self.steps[index].kind, StepKind::Withdraw { .. });
                    if at == to {
                        self.steps[index].done = true;
                        if let StepKind::Withdraw { home: Some(home), .. } = self.steps[index].kind {
                            state.set_task(unit, UnitTask::Heal { at: home }, view.brief.round);
                        }
                        continue;
                    }
                    let command = Command::MoveUnit { unit: handle, destination: to };
                    self.steps[index].done = true;
                    if !pv.accepts(&command) {
                        self.repairs += 1;
                        continue;
                    }
                    self.set_standing(unit, to);
                    if let StepKind::Withdraw { home: Some(home), .. } = self.steps[index].kind {
                        state.set_task(unit, UnitTask::Heal { at: home }, view.brief.round);
                    }
                    let reason = if withdraw { Reason::Retreat } else { Reason::Advance };
                    return Some(Decision::act(actor, reason, command));
                }
                StepKind::Strike { tile, unit: target_unit, city, from, ranged, alts } => {
                    // The target may be gone, or the strike no longer possible: repair.
                    let mut choice = Some((tile, target_unit, city));
                    if pv.attack_preview_from(handle, from, tile).is_none() {
                        choice = alts.iter().find(|a| pv.attack_preview_from(handle, from, a.tile).is_some()).map(|a| (a.tile, a.unit, a.city));
                        self.repairs += 1;
                    }
                    let Some((tile, target_unit, city)) = choice else {
                        self.steps[index].done = true;
                        continue;
                    };
                    if at != from {
                        let command = Command::MoveUnit { unit: handle, destination: from };
                        if self.steps[index].moves > 0 || !pv.accepts(&command) {
                            self.steps[index].done = true;
                            self.repairs += 1;
                            continue;
                        }
                        self.set_standing(unit, from);
                        self.steps[index].moves += 1;
                        return Some(Decision::act(actor, Reason::Advance, command));
                    }
                    let reason = if city.is_some() { Reason::AttackCity } else { Reason::AttackUnit };
                    if pv.must_set_up_first(handle) {
                        let command = Command::SetUpRangedAttack { unit: handle };
                        if pv.accepts(&command) {
                            return Some(Decision::act(actor, reason, command));
                        }
                    }
                    let command = match (ranged, city, target_unit) {
                        (true, _, _) => Command::RangedAttack { unit: handle, target: tile },
                        (false, Some(city), _) => Command::AttackCity { attacker: handle, city },
                        (false, None, Some(defender)) => Command::AttackUnit { attacker: handle, defender },
                        (false, None, None) => {
                            self.steps[index].done = true;
                            continue;
                        }
                    };
                    self.steps[index].done = true;
                    if !pv.accepts(&command) {
                        self.repairs += 1;
                        continue;
                    }
                    return Some(Decision::act(actor, reason, command));
                }
            }
        }
        None
    }
}

// ---- Making the plan ----

fn distance(dims: crate::math::MapDimensions, a: TileId, b: TileId) -> i32 {
    dims.distance(a, b).unwrap_or(i32::MAX)
}

/// The plan of the turn from the snapshot and the player's view.
fn build_plan(view: &UnitView<'_, '_>, state: &mut AiState, tools: &mut PlanTools<'_>, input: &TheatreInput) -> TheatrePlan {
    let pv = &view.local.view;
    let rules = tools.rules;
    let dims = view.brief.dims;
    let budget = tools.settings.budget;

    // Members: the nearest to an enemy first, at most MAX_MEMBERS.
    let near_foe = |tile: TileId| input.foes.iter().map(|u| u.tile).chain(input.cities.iter().map(|c| c.tile)).chain(input.remembered.iter().map(|c| c.tile)).map(|t| distance(dims, t, tile)).min().unwrap_or(i32::MAX);
    let mut units: Vec<OwnUnit> = input.own.iter().filter(|u| input.members.binary_search(&u.id).is_ok()).copied().collect();
    units.sort_by_key(|u| (near_foe(u.tile), u.id));
    units.truncate(MAX_MEMBERS);
    units.sort_by_key(|u| u.id);
    let statics: Vec<TileId> = input.own.iter().filter(|u| !units.iter().any(|m| m.id == u.id)).map(|u| u.tile).collect();

    let mut centres: Vec<TileId> = units.iter().map(|u| u.tile).collect();
    centres.extend(input.foes.iter().map(|u| u.tile));
    centres.extend(input.cities.iter().map(|c| c.tile));
    centres.extend(input.remembered.iter().map(|c| c.tile));
    centres.sort();
    centres.dedup();
    let mut field = Battlefield::build(pv, rules, view.brief, input.round, &input.foes, &input.cities, &input.remembered, &input.own_cities, &centres, THEATRE_RADIUS, None);
    let role_of_unit = roles::assign(rules, &units);
    // Who leaves the line: a melee unit below its withdrawal health (50, and
    // 5 more a level), a ranged unit only when badly hurt (it shoots from
    // behind the line), anyone told to heal. A melee unit a little above
    // the threshold that is out of reach of the enemy keeps resting.
    let mut withdraws: Vec<bool> = units
        .iter()
        .map(|u| {
            let ranged = KindStats::of(rules, u.kind).range > 0;
            let below = if ranged { u.hp < RANGED_WITHDRAW_BELOW } else { u.hp < roles::withdraw_below(u.level) };
            below || matches!(state.task_of(u.id).map(|t| t.task), Some(UnitTask::Heal { .. }))
        })
        .collect();

    // A city's real strength: the rules' own preview of a blow on it shows the
    // strength it defends (and strikes) with, hills and garrison included.
    for index in 0..field.targets.len() {
        let target = field.targets[index];
        if !target.is_city() {
            continue;
        }
        let tile = crate::math::hex_disc(dims, target.tile, 1)
            .filter(|t| *t != target.tile && field.tile(*t).is_some_and(|i| !i.water))
            .min_by_key(|t| (units.iter().map(|u| distance(dims, u.tile, *t)).min().unwrap_or(i32::MAX), *t));
        let Some(tile) = tile else { continue };
        let strength = units.iter().filter(|u| !withdraws[units.iter().position(|x| x.id == u.id).unwrap_or(0)]).find_map(|u| {
            let from = if KindStats::of(rules, u.kind).range > 0 {
                crate::math::hex_disc(dims, target.tile, KindStats::of(rules, u.kind).range).find(|t| distance(dims, *t, target.tile) == KindStats::of(rules, u.kind).range && field.tile(*t).is_some_and(|i| !i.water))
            } else {
                Some(tile)
            }?;
            pv.attack_preview_from(u.handle, from, target.tile).map(|p| p.defender_strength)
        });
        if let Some(strength) = strength {
            field.targets[index].strength = strength;
            field.targets[index].stats.melee = strength;
            field.targets[index].stats.ranged = strength;
        }
    }
    // The fog prior: the strongest blow an enemy seen here deals to the first member.
    if let Some(first) = units.first() {
        let blow = field.targets.iter().filter(|t| t.stats.military && !t.is_city()).map(|t| field.blow(t, first.kind, first.hp, first.tile, false)).max().unwrap_or(0);
        field.prior_blow = blow;
    }

    // The focus: the city the members work on (the tile their tasks name, seen
    // or not), else the nearest city in sight.
    let task_target = |u: &OwnUnit| match state.task_of(u.id).map(|t| t.task) {
        Some(UnitTask::Assault { target }) | Some(UnitTask::Operation { target }) => Some(target),
        _ => None,
    };
    let seen_target = |tile: TileId| state.seen_cities().iter().any(|c| c.tile == tile && brief_at_war(view, c.owner));
    let mut tasked: Vec<(usize, std::cmp::Reverse<i32>, TileId)> = Vec::new();
    for u in &units {
        if let Some(t) = task_target(u) {
            if seen_target(t) || field.targets.iter().any(|x| x.tile == t) {
                match tasked.iter_mut().find(|(_, _, tile)| *tile == t) {
                    Some(entry) => entry.0 += 1,
                    None => tasked.push((1, std::cmp::Reverse(0), t)),
                }
            }
        }
    }
    tasked.sort_by_key(|(n, _, t)| (std::cmp::Reverse(*n), *t));
    let focus_tile: Option<TileId> = tasked.first().map(|(_, _, t)| *t).or_else(|| {
        field
            .targets
            .iter()
            .filter(|t| t.is_city())
            .min_by_key(|t| (units.iter().map(|u| distance(dims, u.tile, t.tile)).min().unwrap_or(i32::MAX), t.tile))
            .map(|t| t.tile)
    });
    let focus: Option<usize> = focus_tile.and_then(|t| field.targets.iter().position(|x| x.tile == t && x.is_city()));

    // The middle of the group: the member's tile nearest to all the others.
    let centre = units.iter().map(|u| u.tile).min_by_key(|t| (units.iter().map(|u| distance(dims, u.tile, *t)).sum::<i32>(), *t));
    let ring = crate::ai::front::STAGING_DISTANCE.max(crate::combat::city::CITY_ATTACK_RANGE + 1);
    let staging = focus_tile.zip(centre).and_then(|(city, middle)| {
        crate::math::hex_disc(dims, city, ring)
            .filter(|t| distance(dims, *t, city) == ring && field.tile(*t).is_some_and(|i| !i.water))
            .min_by_key(|t| (distance(dims, *t, middle), *t))
    });
    let mut previews_left = budget.candidates * PREVIEWS_PER_CANDIDATE;
    let mut builder = Options { field: &field, statics: &statics, bodies: Vec::new(), previews_left: &mut previews_left, incoming: BTreeMap::new(), focus: focus_tile, staging, goals: Vec::new() };
    builder.bodies = statics.iter().copied().chain(units.iter().map(|u| u.tile)).collect();
    let mut members: Vec<Member> = Vec::new();
    for (i, unit) in units.iter().enumerate() {
        let stats = KindStats::of(rules, unit.kind);
        let mut member = Member { unit: *unit, stats, role: role_of_unit[i], worth: worth(&stats, unit.level), withdraw: withdraws[i], options: Vec::new() };
        member.options = builder.strikes(&member);
        members.push(member);
    }
    // A group at the walls is committed: retreating costs more than going on, so
    // the test it must pass is looser (this also keeps the answer from flipping
    // every round with the city's visibility and the units' health).
    let committed = focus_tile.is_some_and(|c| members.iter().any(|m| !m.withdraw && distance(dims, m.unit.tile, c) <= COMMITTED_DISTANCE));
    let limit = if committed { COMMITTED_ROUNDS } else { SIEGE_ROUNDS };
    // A group at the walls goes on: the siege counts as ready.
    let committed_ready = committed;
    let ready = match (focus, focus_tile) {
        (Some(f), Some(tile)) if builder.field.targets[f].remembered => unseen_ready(view, state, tools.rules, &members, tile, limit),
        (Some(f), _) => {
            // The siege test has its own previews: strikes must not starve it.
            let mut own = SIEGE_PREVIEWS;
            siege_ready(builder.field, &members, f, &statics, &mut own, limit)
        }
        (None, Some(tile)) => unseen_ready(view, state, tools.rules, &members, tile, limit),
        (None, None) => false,
    };
    let ready = ready || committed_ready;
    let goals = assign_goals(&field_for_goals(builder.field), &members, focus_tile, staging, ready, &statics);
    builder.goals = goals.clone();
    for (i, member) in members.iter_mut().enumerate() {
        let moves = builder.moves(member, ready, goals[i]);
        member.options.extend(moves);
    }
    // Resting: a melee unit in the band above its threshold keeps resting where the enemy cannot reach it.
    for member in members.iter_mut() {
        let band = member.stats.range == 0 && !member.withdraw && member.unit.hp < roles::withdraw_below(member.unit.level) + REST_BAND;
        if band && !committed && member.unit.hp < UNIT_MAX_HP && builder.incoming(member, member.unit.tile) == 0 && member.unit.hp < roles::withdraw_below(member.unit.level) + REST_BAND && was_resting(member) {
            member.withdraw = true;
        }
    }
    let _ = &mut withdraws;
    let engage = builder.engage(&members);
    let used = budget.candidates * PREVIEWS_PER_CANDIDATE - *builder.previews_left + SIEGE_PREVIEWS;
    tools.counters.candidates += u64::from(used);

    // The danger net's threats (`TurnBrief::danger_net`): the members may move.
    let threats = view.brief.danger.as_ref().filter(|_| view.brief.danger_net).map(|sight| {
        let started = prof::now_ns();
        let movers: Vec<(TileId, i32)> = members
            .iter()
            .map(|m| {
                let step = pv.own_unit_profile(m.unit.handle).map_or(1, |p| p.routes.road_points.max(1));
                (m.unit.tile, i32::from(m.unit.moves_left) / step as i32)
            })
            .collect();
        let threats = crate::ai::danger::Threats::around(view.local, rules, view.brief, sight, &movers);
        crate::ai::danger::note_build(prof::now_ns().saturating_sub(started));
        threats
    });
    let problem = Problem {
        field: &field,
        members,
        statics: statics.clone(),
        own_cities: input.own_cities.clone(),
        focus: focus_tile,
        centre,
        staging,
        ready,
        goals,
        engage,
        weights: Weights::default(),
        danger: threats.as_ref(),
    };
    let solution = plan_search::search(&problem, budget.candidates * EVALUATIONS_PER_CANDIDATE);
    tools.counters.candidates += u64::from(solution.evaluations);
    {
        let d = |i: usize| DIAG[i].fetch_add(1, Ordering::Relaxed);
        d(0);
        if focus_tile.is_some() { d(1); }
        if problem.ready { d(2); }
        let city_strike = problem.members.iter().enumerate().any(|(i, m)| matches!(m.options[solution.choices[i]].act, Act::Strike(t, _) if field.targets[t].is_city()));
        if city_strike { d(3); }
        if solution.eval.captures > 0 { d(4); }
        if problem.members.iter().enumerate().any(|(i, m)| matches!(m.options[solution.choices[i]].act, Act::Withdraw(_))) { d(5); }
        if committed { d(6); }
        if problem.ready && !city_strike { d(7); }
        let option = problem.members.iter().any(|m| m.options.iter().any(|o| matches!(o.act, Act::Strike(t, _) if field.targets[t].is_city())));
        if option { d(8); }
        if option && !city_strike { d(9); }
        if option && problem.ready { d(10); }
        if option && problem.ready && !city_strike { d(11); }
    }
    steps_from(&problem, &solution, view.brief, state)
}

/// The options of the members: staying, going, withdrawing, striking.
struct Options<'f, 'v, 'a> {
    field: &'f Battlefield<'v, 'a>,
    statics: &'f [TileId],
    /// All own bodies as they stand now.
    bodies: Vec<TileId>,
    previews_left: &'f mut u32,
    /// The expected damage on a tile for a kind of unit under the standing posture.
    incoming: BTreeMap<(TileId, u16), i64>,
    focus: Option<TileId>,
    staging: Option<TileId>,
    goals: Vec<Option<TileId>>,
}

impl Options<'_, '_, '_> {
    fn posture(&self) -> Posture {
        let mut sources = self.bodies.clone();
        sources.extend(self.field.tiles.iter().filter(|(_, i)| i.own_city).map(|(t, _)| *t));
        Posture { bodies: self.bodies.clone(), sources }
    }

    /// Expected damage the enemy deals a unit of `member`'s kind on `tile`,
    /// all of it at once, with the bodies as they stand.
    fn incoming(&mut self, member: &Member, tile: TileId) -> i64 {
        let key = (tile, member.unit.kind.0);
        if let Some(v) = self.incoming.get(&key) {
            return *v;
        }
        let posture = self.posture();
        let mut total = 0i64;
        for (f, t) in self.field.targets.iter().enumerate() {
            if t.threatens() && self.field.can_strike(f, tile, &posture) {
                total += i64::from(self.field.blow(t, member.unit.kind, member.unit.hp, tile, false));
            }
        }
        total += i64::from(self.field.fog_blow(tile));
        self.incoming.insert(key, total);
        total
    }

    fn free(&self, tile: TileId, unit: &OwnUnit) -> bool {
        let Some(info) = self.field.tile(tile) else { return false };
        // Tiles other members stand on are free: they may move on (the search
        // refuses two bodies on one tile, the steps vacate first).
        let _ = unit;
        !info.water && !self.field.targets.iter().any(|t| t.tile == tile) && !self.statics.contains(&tile)
    }

    /// Staying and the strikes of a member.
    fn strikes(&mut self, member: &Member) -> Vec<Opt> {
        let unit = member.unit;
        let field = self.field;
        let dims = field.dims;
        let mut options = vec![Opt { act: Act::Stay, exchange: None, base: 0 }];
        let reachable = field.view.own_reachable(unit.handle);
        let range = member.stats.range;
        // Strikes.
        if unit.can_attack && unit.moves_left > 0 && (!member.withdraw || range > 0) {
            let mut strikes: Vec<Opt> = Vec::new();
            for (ti, target) in field.targets.iter().enumerate() {
                if (target.worth == 0 && !target.is_city()) || target.remembered {
                    continue;
                }
                let reach = range.max(1) + reachable.iter().map(|(t, _)| distance(dims, unit.tile, *t)).max().unwrap_or(0);
                if distance(dims, unit.tile, target.tile) > reach {
                    continue;
                }
                let fits = |t: TileId| {
                    let d = distance(dims, t, target.tile);
                    if range > 0 { (1..=range).contains(&d) } else { d == 1 }
                };
                // A unit that must set up before it shoots pays a move point for it: it
                // shoots from where it stands, or from a tile it reaches with more than
                // that point left (moving undoes the set-up).
                let setup = field.view.must_set_up_first(unit.handle);
                let set_up_now = unit.stance == crate::world::unit::Stance::SetUp;
                let point = MOVE_DENOMINATOR;
                let ready_here = if setup { unit.moves_left > point } else { unit.moves_left > 0 };
                let mut tiles: Vec<(i64, TileId)> = reachable
                    .iter()
                    .filter(|(t, left)| {
                        if *t == unit.tile {
                            return ready_here || set_up_now;
                        }
                        let needed = if setup || set_up_now { point } else { 0 };
                        fits(*t) && *left > needed && !member.withdraw && self.free(*t, &unit)
                    })
                    .filter(|(t, _)| fits(*t))
                    .map(|(t, _)| (0, *t))
                    .collect();
                for entry in tiles.iter_mut() {
                    let incoming = self.incoming(member, entry.1);
                    let percent = field.defence_percent(entry.1, field.player, false);
                    entry.0 = incoming * 4 - percent + if entry.1 == unit.tile { -3 } else { 0 };
                }
                tiles.sort();
                for (_, from) in tiles.into_iter().take(STRIKE_TILES) {
                    if *self.previews_left == 0 {
                        break;
                    }
                    *self.previews_left -= 1;
                    let Some(exchange) = plan_search::preview(field, &unit, from, target, range > 0) else { continue };
                    let base = plan_search::strike_value(target, &exchange, member.worth);
                    strikes.push(Opt { act: Act::Strike(ti, from), exchange: Some(exchange), base });
                }
            }
            strikes.sort_by_key(|o| (std::cmp::Reverse(o.base), match o.act {
                Act::Strike(t, f) => (t, f),
                _ => (0, TileId(0)),
            }));
            strikes.truncate(MAX_STRIKES);
            options.extend(strikes);
        }
        options
    }

    /// What a strike next turn would gain each member against each enemy
    /// unit: the rules' preview from the tile next to the target (at range
    /// for a ranged unit) nearest the member, as `strike_value`.
    fn engage(&mut self, members: &[Member]) -> Vec<Vec<(u16, u16)>> {
        let field = self.field;
        let dims = field.dims;
        members
            .iter()
            .map(|member| {
                field
                    .targets
                    .iter()
                    .map(|target| {
                        if target.is_city() || member.withdraw {
                            return (0, 0);
                        }
                        let range = member.stats.range.max(1);
                        let tile = crate::math::hex_disc(dims, target.tile, range)
                            .filter(|t| distance(dims, *t, target.tile) == range && field.tile(*t).is_some_and(|i| !i.water))
                            .filter(|t| !self.bodies.contains(t) || *t == member.unit.tile)
                            .min_by_key(|t| (distance(dims, *t, member.unit.tile), *t));
                        let Some(tile) = tile else { return (0, 0) };
                        if *self.previews_left == 0 {
                            return (0, 0);
                        }
                        *self.previews_left -= 1;
                        match plan_search::preview(field, &member.unit, tile, target, member.stats.range > 0) {
                            Some(x) => (x.dealt, x.taken),
                            None => (0, 0),
                        }
                    })
                    .collect()
            })
            .collect()
    }

    /// The positions a member may go to or withdraw to; `ready`: the siege
    /// group would take the focus city, so the tiles nearest it count.
    fn moves(&mut self, member: &Member, ready: bool, goal: Option<TileId>) -> Vec<Opt> {
        let unit = member.unit;
        let field = self.field;
        let dims = field.dims;
        let mut options = Vec::new();
        let reachable = field.view.own_reachable(unit.handle);
        // (safety, progress, tile): the best by both measures together and the
        // best by progress alone, so that a step into the enemy's reach is
        // always among the options when it brings the unit closer.
        let mut goes: Vec<(i64, i64, TileId)> = Vec::new();
        for (tile, _) in reachable.iter().filter(|(t, _)| *t != unit.tile) {
            if !self.free(*tile, &unit) {
                continue;
            }
            let incoming = self.incoming(member, *tile);
            let percent = field.defence_percent(*tile, field.player, false);
            if member.withdraw {
                let home = field.tiles.iter().filter(|(_, i)| i.own_city).map(|(t, _)| distance(dims, *t, *tile)).min().unwrap_or(20);
                let city = field.tile(*tile).is_some_and(|i| i.own_city);
                let key = incoming * 8 - if city { 200 } else { 0 } + i64::from(home) * 2 - percent - (field.rest_heal(*tile) - 10) * REST_HEAL_WEIGHT;
                goes.push((key, key, *tile));
                continue;
            }
            let toward = match self.focus {
                _ if goal.is_some() => i64::from(distance(dims, goal.unwrap_or(*tile), *tile)),
                Some(f) if ready => i64::from((distance(dims, f, *tile) - member.stats.range.max(1)).max(0)),
                Some(f) => match self.staging {
                    Some(point) => i64::from((distance(dims, point, *tile) - 1).max(0).max(crate::ai::front::STAGING_DISTANCE - distance(dims, f, *tile))),
                    None => i64::from((distance(dims, f, *tile) - crate::ai::front::STAGING_DISTANCE).abs()),
                },
                None => field.targets.iter().map(|t| i64::from(distance(dims, t.tile, *tile))).min().unwrap_or(0),
            };
            goes.push((incoming + toward * 12 - percent, toward * 100 + incoming, *tile));
        }
        let mut chosen: Vec<TileId> = Vec::new();
        goes.sort_by_key(|g| (g.0, g.2));
        chosen.extend(goes.iter().take(MAX_GOES - 1).map(|g| g.2));
        goes.sort_by_key(|g| (g.1, g.2));
        for g in &goes {
            if chosen.len() >= MAX_GOES + 1 {
                break;
            }
            if !chosen.contains(&g.2) {
                chosen.push(g.2);
            }
        }
        for tile in chosen {
            let act = if member.withdraw { Act::Withdraw(tile) } else { Act::Go(tile) };
            options.push(Opt { act, exchange: None, base: 0 });
        }
        options
    }
}

fn field_for_goals<'a, 'f, 'v>(field: &'f Battlefield<'v, 'a>) -> &'f Battlefield<'v, 'a> {
    field
}

/// One tile of work for each member: at its range from the focus city (the
/// siege is ready), else around the staging tile. The members, the strikers
/// of range first, take the nearest tile nobody else has, so that the group
/// does not crowd the same places.
fn assign_goals(field: &Battlefield<'_, '_>, members: &[Member], focus: Option<TileId>, staging: Option<TileId>, ready: bool, statics: &[TileId]) -> Vec<Option<TileId>> {
    let mut goals: Vec<Option<TileId>> = vec![None; members.len()];
    let Some(city) = focus else { return goals };
    let free = |t: TileId| field.tile(t).is_some_and(|i| !i.water) && !statics.contains(&t) && !field.targets.iter().any(|x| x.tile == t);
    let mut taken: Vec<TileId> = Vec::new();
    let mut order: Vec<usize> = (0..members.len()).filter(|&i| !members[i].withdraw).collect();
    order.sort_by_key(|&i| (members[i].stats.range == 0, members[i].unit.id));
    for i in order {
        let member = &members[i];
        let candidates: Vec<TileId> = if ready {
            let range = member.stats.range.max(1);
            crate::math::hex_disc(field.dims, city, range).filter(|t| distance(field.dims, *t, city) == range && free(*t)).collect()
        } else {
            let anchor = staging.unwrap_or(city);
            let mut ring: Vec<TileId> = crate::math::hex_disc(field.dims, anchor, 2).filter(|t| distance(field.dims, *t, city) >= crate::ai::front::STAGING_DISTANCE && free(*t)).collect();
            ring.sort_by_key(|t| (distance(field.dims, *t, anchor), *t));
            ring
        };
        let pick = candidates
            .into_iter()
            .filter(|t| !taken.contains(t))
            .min_by_key(|t| (distance(field.dims, *t, member.unit.tile), *t));
        if let Some(tile) = pick {
            taken.push(tile);
            goals[i] = Some(tile);
        }
    }
    goals
}

/// Is the unit resting after a withdrawal? The memory does not say; a unit
/// out of reach of the enemy a little above its threshold and not at full
/// health is taken to be.
fn was_resting(member: &Member) -> bool {
    member.unit.hp < UNIT_MAX_HP
}

fn brief_at_war(view: &UnitView<'_, '_>, owner: crate::ids::PlayerId) -> bool {
    view.brief.at_war(owner)
}

/// The same test for a city out of sight, from what the player remembers of
/// it (`operations::estimate_target`) and the members' health, as the
/// front's `wait_out_of_sight` does.
fn unseen_ready(view: &UnitView<'_, '_>, state: &AiState, rules: &CompiledRuleset, members: &[Member], tile: TileId, limit: u32) -> bool {
    let Some(seen) = state.seen_cities().iter().find(|c| c.tile == tile) else { return false };
    let city = crate::ai::operations::estimate_target(view.brief, rules, seen);
    let dims = view.brief.dims;
    let mut group: Vec<Besieger> = members
        .iter()
        .filter(|m| distance(dims, m.unit.tile, tile) <= READY_RADIUS)
        .filter_map(|m| crate::ai::operations::estimated_strike(rules, m.unit.kind, m.unit.hp, &city))
        .filter(|b| b.ranged || b.hp >= crate::ai::front::HEAL_BELOW)
        .collect();
    siege_rounds_within(&mut group, city.hp, city.heal, limit).is_some()
}

/// Would the siege group take the focus city within the front's rounds?
fn siege_ready(field: &Battlefield<'_, '_>, members: &[Member], focus: usize, statics: &[TileId], previews_left: &mut u32, limit: u32) -> bool {
    let city = &field.targets[focus];
    let mut group: Vec<Besieger> = Vec::new();
    for member in members {
        if distance(field.dims, member.unit.tile, city.tile) > READY_RADIUS || (member.stats.range == 0 && member.withdraw) {
            continue;
        }
        let ranged = member.stats.range > 0;
        let exchange = member
            .options
            .iter()
            .find_map(|o| match (o.act, o.exchange) {
                (Act::Strike(t, _), Some(x)) if t == focus => Some((x.dealt, x.taken)),
                _ => None,
            })
            .or_else(|| {
                let reach = if ranged { member.stats.range } else { 1 };
                // A tile nobody stands on, at the unit's range from the city.
                let free = |t: &TileId| {
                    distance(field.dims, *t, city.tile) == reach
                        && field.tile(*t).is_some_and(|i| !i.water)
                        && !statics.contains(t)
                        && !field.targets.iter().any(|x| x.tile == *t)
                        && !members.iter().any(|m| m.unit.tile == *t && m.unit.id != member.unit.id)
                };
                let mut tiles: Vec<TileId> = crate::math::hex_disc(field.dims, city.tile, reach).filter(free).collect();
                tiles.sort_by_key(|t| (distance(field.dims, *t, member.unit.tile), *t));
                tiles.into_iter().take(3).find_map(|t| {
                    if *previews_left == 0 {
                        return None;
                    }
                    *previews_left -= 1;
                    field.view.attack_preview_from(member.unit.handle, t, city.tile).map(|p| (p.to_defender.1, p.to_attacker.1))
                })
            });
        if let Some((dealt, taken)) = exchange {
            group.push(Besieger { ranged, dealt, taken, hp: member.unit.hp });
        }
    }
    let rounds = siege_rounds_within(&mut group, city.hp, CITY_HEAL_PER_TURN as u16, limit);
    rounds.is_some()
}

/// The steps of a solution in execution order: withdrawals, the strikes in
/// the search's order (ranged first, the capture last), then the moves of
/// the others; a destination another member still stands on is vacated
/// first.
fn steps_from(problem: &Problem<'_, '_, '_>, solution: &plan_search::Solution, brief: &TurnBrief, state: &AiState) -> TheatrePlan {
    let _ = state;
    let field = problem.field;
    let eval = &solution.eval;
    let target_of = |t: usize| -> &Target { &field.targets[t] };
    let mut steps: Vec<Step> = Vec::new();
    let home_of = |tile: TileId| brief.nearest_city(brief.dims, tile).map(|c| c.tile);
    // Withdrawals.
    for (i, member) in problem.members.iter().enumerate() {
        if let Act::Withdraw(to) = member.options[solution.choices[i]].act {
            let home = if member.unit.hp < roles::withdraw_below(member.unit.level) { home_of(to) } else { None };
            steps.push(Step { unit: member.unit.id, handle: member.unit.handle, kind: StepKind::Withdraw { to, home }, done: false, moves: 0 });
        }
    }
    // Strikes: the planned ones, then the back-ups that go in only if the target lives.
    let planned: Vec<usize> = eval.order.iter().copied().filter(|&i| !eval.skipped[i]).chain(eval.order.iter().copied().filter(|&i| eval.backup[i])).collect();
    for &i in &planned {
        let member = &problem.members[i];
        let Act::Strike(t, from) = member.options[solution.choices[i]].act else { continue };
        let target = target_of(t);
        let ranged = member.stats.range > 0;
        let alts: Vec<Alt> = member
            .options
            .iter()
            .filter_map(|o| match o.act {
                Act::Strike(other, f) if other != t && f == from => Some((other, o.base)),
                _ => None,
            })
            .filter(|(_, base)| *base > 0)
            .map(|(other, _)| {
                let o = target_of(other);
                Alt { tile: o.tile, unit: o.unit, city: o.city }
            })
            .collect();
        steps.push(Step {
            unit: member.unit.id,
            handle: member.unit.handle,
            kind: StepKind::Strike { tile: target.tile, unit: target.unit, city: target.city, from, ranged, alts },
            done: false,
            moves: 0,
        });
    }
    // The others.
    for (i, member) in problem.members.iter().enumerate() {
        let skipped_strike = eval.skipped[i];
        match member.options[solution.choices[i]].act {
            Act::Go(to) => steps.push(Step { unit: member.unit.id, handle: member.unit.handle, kind: StepKind::Go { to }, done: false, moves: 0 }),
            Act::Stay => steps.push(Step { unit: member.unit.id, handle: member.unit.handle, kind: StepKind::Hold, done: false, moves: 0 }),
            Act::Strike(..) if skipped_strike => steps.push(Step { unit: member.unit.id, handle: member.unit.handle, kind: StepKind::Hold, done: false, moves: 0 }),
            Act::Strike(..) => {}
            _ => {}
        }
    }
    vacate_first(&mut steps, &problem.members);
    TheatrePlan {
        steps,
        standing: problem.members.iter().map(|m| (m.unit.id, m.unit.tile)).collect(),
        roles: problem.members.iter().map(|m| (m.unit.id, m.role)).collect(),
        evaluations: solution.evaluations,
        expected: ExpectedOutcome {
            score: eval.score,
            kills: eval.kills,
            captures: eval.captures,
            dealt: eval.dealt,
            taken: eval.taken,
            reply: eval.reply,
            lethal: eval.lethal,
            screened: eval.screened,
            artillery: eval.artillery,
        },
        repairs: 0,
    }
}

/// Order the steps so that a member stands where nobody still has to leave: a
/// step whose destination another member stands on waits for that member's
/// step (a topological order that keeps the given order otherwise). A circle,
/// which the search avoids, leaves its steps as they are.
fn vacate_first(steps: &mut Vec<Step>, members: &[Member]) {
    let destination = |s: &Step| match &s.kind {
        StepKind::Go { to } | StepKind::Withdraw { to, .. } => Some(*to),
        StepKind::Strike { from, .. } => Some(*from),
        StepKind::Hold => None,
    };
    let n = steps.len();
    // step j waits for the steps of units standing on its destination.
    let waits_for: Vec<Vec<usize>> = (0..n)
        .map(|j| {
            let Some(dest) = destination(&steps[j]) else { return Vec::new() };
            let start = members.iter().find(|m| m.unit.id == steps[j].unit).map(|m| m.unit.tile);
            if start == Some(dest) {
                return Vec::new();
            }
            let occupants: Vec<_> = members.iter().filter(|m| m.unit.tile == dest && m.unit.id != steps[j].unit).map(|m| m.unit.id).collect();
            (0..n).filter(|&k| k != j && occupants.contains(&steps[k].unit) && !matches!(steps[k].kind, StepKind::Hold) && destination(&steps[k]) != Some(dest)).collect()
        })
        .collect();
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut placed = vec![false; n];
    for _ in 0..n {
        let next = (0..n).find(|&j| !placed[j] && waits_for[j].iter().all(|&k| placed[k]));
        let Some(j) = next.or_else(|| (0..n).find(|&j| !placed[j])) else { break };
        placed[j] = true;
        order.push(j);
    }
    let old = std::mem::take(steps);
    let mut slots: Vec<Option<Step>> = old.into_iter().map(Some).collect();
    for j in order {
        if let Some(step) = slots[j].take() {
            steps.push(step);
        }
    }
}
