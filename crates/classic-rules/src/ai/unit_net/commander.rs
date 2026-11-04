//! The commander (design section 6 and sections 5a–5d; the map of the mini
//! nets, «Operations», «TurnPlan», «Orders»): up to `MAX_OPS` operations —
//! the war operations of the strategy (`ai::operations` forms them, declares
//! the war and recruits; the commander runs their phases), the defence of the
//! threatened cities, the barbarian camps the camps advisor sends units to —
//! each with its phase by the Lanchester head with hysteresis; the orders
//! the cities read; and the turn's plan of the units in contact: the target
//! values with the key target, the greedy fire allocation by marginal value,
//! the cover of the weak, the enemy's reply checked twice, the queue — and
//! its fast update after each action (residual health, kills, misses).

use super::fields::{fighter, hostile, Strike, Theatre};
use super::math::{mul, reach_chance, sigmoid, unit, ONE};
use super::memory::{OpMemory, UnitMemory};
use super::weights::{op as ow, plan as pw};
use super::{Milli, Prices};
use crate::ai::battlefield::{KindStats, Posture};
use crate::ai::planner::TurnBrief;
use crate::ai::state::{AiState, OperationPhase, UnitTask};
use crate::ids::{EntityId, PlayerId, TileId, UnitHandle};
use crate::knowledge::observation::OwnUnit;
use crate::rules::CompiledRuleset;

/// What an operation is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpKind {
    /// A war operation of the strategy against an enemy city.
    Attack,
    /// The defence of a threatened city of ours.
    Defence,
    /// A barbarian camp.
    Camp,
}

/// An operation's phase (design section 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    Gather,
    Approach,
    Assault,
    Hold,
    Retreat,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Gather => "gather",
            Phase::Approach => "approach",
            Phase::Assault => "assault",
            Phase::Hold => "hold",
            Phase::Retreat => "retreat",
        }
    }
}

/// The operation head's options (design section 6).
pub const OPTIONS: [&str; 4] = ["attack", "wait", "hold", "retreat"];

/// One operation of the turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op {
    pub kind: OpKind,
    pub target: TileId,
    /// Where it gathers (a muster city, the front member's tile, the city defended).
    pub rally: TileId,
    pub phase: Phase,
    pub since: u32,
    /// The option held (index into `OPTIONS`).
    pub choice: usize,
    pub members: Vec<EntityId>,
    /// Readiness r, the Lanchester ratio ρ, P(take), the options' values.
    pub readiness: Milli,
    pub rho: Milli,
    pub p_take: Milli,
    pub options: [Milli; 4],
}

impl Op {
    /// The tile its members head for this phase.
    pub fn goal(&self) -> TileId {
        match self.phase {
            Phase::Gather | Phase::Hold => self.rally,
            _ => self.target,
        }
    }
}

/// An order for the cities (the map of the mini nets, «Orders»): a military
/// unit wanted near `city` by `round`, worth `value` (hundredths of score
/// points, the allocator's scale).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Order {
    pub city: TileId,
    pub round: u32,
    pub value: i64,
}

/// Why an operation ended (the bench's share of captures and retreats).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpEnd {
    Captured,
    Retreated,
    Gone,
}

thread_local! {
    /// Operations ended in this thread, by cause (read by the bench; never by a decision).
    pub static ENDS: std::cell::RefCell<Vec<(PlayerId, OpKind, OpEnd)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The ends noted since the last call, taken.
pub fn take_ends() -> Vec<(PlayerId, OpKind, OpEnd)> {
    ENDS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

fn note_end(player: PlayerId, kind: OpKind, end: OpEnd) {
    ENDS.with(|e| e.borrow_mut().push((player, kind, end)));
}

/// A unit's power: strength × health / 100 (the force channels' unit).
pub fn power(rules: &CompiledRuleset, kind: crate::ids::UnitKindId, hp: u16) -> i64 {
    rules.units.get(kind.0 as usize).map_or(0, |d| i64::from(d.strength.max(d.ranged_strength)) * i64::from(hp) / 100)
}

/// A unit's worth in value points (hundreds of hammers).
pub fn unit_value(rules: &CompiledRuleset, kind: crate::ids::UnitKindId) -> Milli {
    rules.units.get(kind.0 as usize).map_or(ONE / 2, |d| i64::from(d.cost) * ONE / pw::HAMMERS_PER_POINT)
}

/// A city's worth in value points.
pub fn city_value(population: u32) -> Milli {
    (pw::CITY_BASE_HAMMERS + pw::CITY_HAMMERS_PER_POP * i64::from(population)) * ONE / pw::HAMMERS_PER_POINT
}

/// The operations of the turn from the strategy's operations, the defence
/// watch and the camps advisor's tasks; their phases by the operation head.
/// Writes back the memory of the phases.
pub fn operations(brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, memory: &mut UnitMemory, prices: &Prices) -> Vec<Op> {
    let mut ops: Vec<Op> = Vec::new();
    let dims = brief.dims;
    let held = |kind: OpKind, target: TileId| memory.ops.iter().find(|m| m.kind == kind && m.target == target).copied();
    let military: Vec<&crate::ai::planner::UnitBrief> = brief.units.iter().filter(|u| fighter(rules, u.kind)).collect();
    let mut taken: Vec<EntityId> = Vec::new();
    // Attack: the strategy's war operations.
    for operation in state.operations().to_vec() {
        let members: Vec<EntityId> = military.iter().filter(|u| state.task_of(u.id).map(|t| t.task) == Some(UnitTask::Operation { target: operation.target })).map(|u| u.id).collect();
        taken.extend(&members);
        let memo = held(OpKind::Attack, operation.target);
        let mut op = Op {
            kind: OpKind::Attack,
            target: operation.target,
            rally: operation.muster,
            phase: memo.map_or(Phase::Gather, |m| m.phase),
            since: memo.map_or(brief.round, |m| m.since),
            choice: memo.map_or(0, |m| usize::from(m.choice)),
            members,
            readiness: 0,
            rho: 0,
            p_take: 0,
            options: [0; 4],
        };
        let forming = operation.phase == OperationPhase::Forming;
        let seen = state.seen_cities().iter().find(|c| c.tile == operation.target).copied().unwrap_or(crate::ai::state::SeenCity { tile: operation.target, owner: operation.enemy, population: 1, last_seen_round: brief.round });
        head(brief, rules, &mut op, prices, forming, &seen);
        if op.phase == Phase::Retreat && brief.round.saturating_sub(op.since) >= brief.speed.pace_rounds(ow::RETREAT_ROUNDS) {
            // The retreat is over: the operation ends; the strategy may form another.
            state.end_operation(operation.target);
            note_end(brief.player, OpKind::Attack, OpEnd::Retreated);
            continue;
        }
        ops.push(op);
    }
    // The attacks that ended since the last turn: taken, or gone.
    for m in memory.ops.iter().filter(|m| m.kind == OpKind::Attack) {
        if !state.operations().iter().any(|o| o.target == m.target) && m.phase != Phase::Retreat {
            let captured = brief.cities.iter().any(|c| c.tile == m.target);
            note_end(brief.player, OpKind::Attack, if captured { OpEnd::Captured } else { OpEnd::Gone });
        }
    }
    // Defence: the cities in danger.
    let dangers: Vec<(TileId, u32, u32)> = match &brief.defence {
        Some(watch) => watch.dangers().iter().map(|d| (d.city, d.threat, d.guard)).collect(),
        None => Vec::new(),
    };
    let mut dangers = dangers;
    dangers.sort_by_key(|(c, threat, guard)| (std::cmp::Reverse(i64::from(*threat) - i64::from(*guard)), *c));
    for (city, threat, guard) in dangers {
        if ops.len() >= ow::MAX_OPS || u64::from(threat) * ONE as u64 <= u64::from(guard) * ow::DEFEND_AT as u64 {
            continue;
        }
        let members: Vec<EntityId> = military
            .iter()
            .filter(|u| !taken.contains(&u.id) && dims.distance(u.tile, city).is_some_and(|d| d <= ow::DEFEND_RADIUS))
            .map(|u| u.id)
            .collect();
        taken.extend(&members);
        let memo = held(OpKind::Defence, city);
        ops.push(Op {
            kind: OpKind::Defence,
            target: city,
            rally: city,
            phase: Phase::Hold,
            since: memo.map_or(brief.round, |m| m.since),
            choice: 2,
            members,
            readiness: i64::from(guard) * ONE / i64::from(threat.max(1)),
            rho: 0,
            p_take: 0,
            options: [0; 4],
        });
    }
    // Camps: the units the camps advisor sends (`UnitTask::Raid` on a tile that is no city).
    let mut camps: Vec<TileId> = military
        .iter()
        .filter_map(|u| match state.task_of(u.id).map(|t| t.task) {
            Some(UnitTask::Raid { target }) if !state.seen_cities().iter().any(|c| c.tile == target) => Some(target),
            _ => None,
        })
        .collect();
    camps.sort();
    camps.dedup();
    for camp in camps {
        if ops.len() >= ow::MAX_OPS {
            break;
        }
        let members: Vec<EntityId> = military.iter().filter(|u| !taken.contains(&u.id) && state.task_of(u.id).map(|t| t.task) == Some(UnitTask::Raid { target: camp })).map(|u| u.id).collect();
        taken.extend(&members);
        let near = members.iter().filter_map(|id| brief.units.iter().find(|u| u.id == *id)).filter_map(|u| dims.distance(u.tile, camp)).min().unwrap_or(i32::MAX);
        let memo = held(OpKind::Camp, camp);
        ops.push(Op {
            kind: OpKind::Camp,
            target: camp,
            rally: camp,
            phase: if near <= 2 { Phase::Assault } else { Phase::Approach },
            since: memo.map_or(brief.round, |m| m.since),
            choice: 0,
            members,
            readiness: ONE,
            rho: 0,
            p_take: 0,
            options: [0; 4],
        });
    }
    ops.truncate(ow::MAX_OPS);
    memory.ops = ops.iter().map(|o| OpMemory { kind: o.kind, target: o.target, phase: o.phase, since: o.since, choice: o.choice as u8 }).collect();
    ops
}

/// The operation head (design section 6) of an attack: the force it needs,
/// readiness, the Lanchester ratio, P(take), the four options with
/// hysteresis, and the phase.
fn head(brief: &TurnBrief, rules: &CompiledRuleset, op: &mut Op, prices: &Prices, forming: bool, seen_city: &crate::ai::state::SeenCity) {
    let dims = brief.dims;
    let members: Vec<&crate::ai::planner::UnitBrief> = op.members.iter().filter_map(|id| brief.units.iter().find(|u| u.id == *id)).collect();
    let turns = |u: &crate::ai::planner::UnitBrief| -> i64 {
        let moves = rules.units.get(u.kind.0 as usize).map_or(2, |d| i64::from(d.movement.max(1)));
        i64::from(dims.distance(u.tile, op.target).unwrap_or(99)) / moves
    };
    // N: the city (its health with the defence bonus, as power) and the defenders seen near it.
    let seen = brief.sightings.iter().filter(|s| s.military && dims.distance(s.tile, op.target).is_some_and(|d| d <= crate::ai::operations::DEFENCE_RADIUS));
    let defenders: i64 = seen.clone().map(|s| i64::from(s.power)).sum::<i64>() / 100;
    // The city as the player estimates it (its population seen, a garrison like our best
    // melee unit, the rules' base health): strength × health / 100, the unit's power.
    let estimate = crate::ai::operations::estimate_target(brief, rules, seen_city);
    let city_power = estimate.strength / 100 * i64::from(estimate.hp) / 100;
    let need = (city_power * (100 + ow::CITY_DEFENCE_PERCENT) / 100 + defenders).max(1);
    let ready: i64 = members.iter().filter(|u| turns(u) <= ow::READY_TURNS).map(|u| power(rules, u.kind, u.hp)).sum();
    let all: i64 = members.iter().map(|u| power(rules, u.kind, u.hp)).sum();
    op.readiness = ready * ONE / need;
    // Fire a turn: ours ~ the ready power, theirs ~ the city's strike and the defenders.
    let our_fire = ready.max(1);
    let their_fire = (city_power / 2 + defenders).max(1);
    let r = op.readiness;
    op.rho = our_fire * ONE / their_fire * r / ONE * r / ONE;
    op.p_take = sigmoid((op.rho - ow::TAKE_AT) * ONE / ow::TAKE_WIDTH);
    let siege_turns = (need * 2 / our_fire.max(1)).clamp(1, ow::MAX_SIEGE_TURNS);
    let losses = their_fire * siege_turns * ONE / 100;
    let city = mul(prices.city, city_value(seen_city.population));
    let gain = mul(city, op.p_take);
    // Losses: their fire a turn (in health) over the siege's turns, at a unit's worth per 100 health.
    let loss_cost = mul(prices.unit, losses);
    let attack = gain - loss_cost;
    // Waiting: the members a turn away join (readiness with all), less a third of a city.
    let r_next = all * ONE / need;
    let p_next = sigmoid((our_fire.max(all) * ONE / their_fire * r_next / ONE * r_next / ONE - ow::TAKE_AT) * ONE / ow::TAKE_WIDTH);
    let wait = mul(city, p_next) - loss_cost - mul(city, ow::WAIT_COST) / 1;
    let hold_v = -mul(prices.unit, their_fire * ONE / 100);
    let retreat = -mul(prices.unit, their_fire * ONE / 200);
    op.options = [attack, wait, hold_v, retreat];
    // The option held unless another leads it by 15 % of the larger magnitude.
    let best = (0..4).max_by_key(|&i| (op.options[i], std::cmp::Reverse(i))).unwrap_or(0);
    let held_value = op.options[op.choice.min(3)];
    let margin = op.options[best].abs().max(held_value.abs()) * (ow::HYSTERESIS - ONE) / ONE;
    let choice = if op.options[best] - held_value > margin { best } else { op.choice.min(3) };
    let old = op.phase;
    let near = members.iter().map(|u| turns(u)).min().unwrap_or(99);
    op.phase = if forming {
        Phase::Gather
    } else {
        match (old, choice) {
            (Phase::Retreat, _) => Phase::Retreat,
            // Assault turns to retreat when the losses pass 1.5 × the gain.
            (Phase::Assault, _) if loss_cost * ONE > gain.max(1) * ow::RETREAT_RATIO || choice == 3 => Phase::Retreat,
            (Phase::Gather, _) if op.readiness >= ow::READY || brief.round.saturating_sub(op.since) >= brief.speed.pace_rounds(ow::GATHER_DEADLINE) => {
                if near <= ow::ASSAULT_TURNS { Phase::Assault } else { Phase::Approach }
            }
            (Phase::Gather, _) => Phase::Gather,
            (_, 1) if old == Phase::Approach => Phase::Gather,
            (_, _) if near <= ow::ASSAULT_TURNS => Phase::Assault,
            _ => Phase::Approach,
        }
    };
    if op.phase != old {
        op.since = brief.round;
    }
    op.choice = choice;
    // While gathering on the march, the front member is the rally.
    if !forming && op.phase == Phase::Gather {
        if let Some(front) = members.iter().min_by_key(|u| (dims.distance(u.tile, op.target).unwrap_or(99), u.id)) {
            op.rally = front.tile;
        }
    }
}

/// The orders of the turn: a defender for each defended city whose guard is
/// short of the threat.
pub fn orders(brief: &TurnBrief, ops: &[Op]) -> Vec<Order> {
    let Some(watch) = &brief.defence else { return Vec::new() };
    let mut out = Vec::new();
    for op in ops.iter().filter(|o| o.kind == OpKind::Defence) {
        let Some(danger) = watch.danger(op.target) else { continue };
        if i64::from(danger.guard) * ONE >= i64::from(danger.threat) * ow::ORDER_SHORT {
            continue;
        }
        let value = danger.loss(danger.guard) * ow::ORDER_VALUE / ONE;
        if value > 0 {
            out.push(Order { city: op.target, round: brief.round + 1, value });
        }
    }
    out
}

// ---- the turn's plan (5a–5d) ----

/// A unit's part in the turn's plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// Strike `target` from `from` (a move there first when `from` is not
    /// where it stands).
    Strike { target: TileId, from: TileId },
    /// Strike the city on `target` from `from` and take it.
    Take { target: TileId, from: TileId },
    /// Stand on `tile` between the foes and the weak `ward`.
    Cover { ward: EntityId, tile: TileId },
    /// Leave the fight (wounded, doomed where it stands).
    Withdraw,
}

impl Task {
    pub fn name(&self) -> &'static str {
        match self {
            Task::Strike { .. } => "strike",
            Task::Take { .. } => "take",
            Task::Cover { .. } => "cover",
            Task::Withdraw => "withdraw",
        }
    }
}

/// A target's value and what the plan puts on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanTarget {
    pub tile: TileId,
    pub hp: u16,
    pub value: Milli,
    /// The key target: it alone (or with few others) can take a city of ours next turn.
    pub key: bool,
    pub city: bool,
    /// Assigned damage (min, expected, max) and the chance it dies (is taken).
    pub assigned: (i64, i64, i64),
    pub p_kill: Milli,
}

/// One assignment of the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assignment {
    pub unit: EntityId,
    pub task: Task,
    pub gain: Milli,
}

/// The expected outcome of the plan (the reply checked).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Expected {
    pub kills: u32,
    pub captures: u32,
    /// Our units the reply is expected to kill (thousandths of a unit, summed chances).
    pub losses: Milli,
    pub dealt: i64,
}

/// The turn's plan (TurnPlan): targets, assignments, the queue, the
/// expected outcome; the options it chose from, for the fast update.
#[derive(Debug, Clone, Default)]
pub struct TurnPlan {
    pub round: u32,
    pub targets: Vec<PlanTarget>,
    pub assignments: Vec<Assignment>,
    pub queue: Vec<EntityId>,
    pub expected: Expected,
    pub options: Vec<Strike>,
    /// (unit, from) variants the reply check penalised, with the penalty.
    pub penalties: Vec<(EntityId, TileId, Milli)>,
    /// Units that acted on their task (the fast update saw it).
    pub spent: Vec<EntityId>,
    /// Fast updates made.
    pub updates: u32,
    /// The reply check's rounds made and the units it moved off a variant.
    pub reply_moves: u32,
    /// N4: where our fighters stand (as last seen by the plan): a shot is not planned from a
    /// tile another of them holds.
    pub at: Vec<(EntityId, TileId)>,
}

impl TurnPlan {
    pub fn task_of(&self, unit: EntityId) -> Option<Task> {
        self.assignments.iter().find(|a| a.unit == unit).map(|a| a.task)
    }

    pub fn target_at(&self, tile: TileId) -> Option<&PlanTarget> {
        self.targets.iter().find(|t| t.tile == tile)
    }
}

/// The opponent profile of a player (design 5d).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// The engine's barbarian rule (`ai::barbarians`, `ai::combat::attack_score`).
    Barbarian,
    /// An AI player: the mirrored potential (our greedy allocation by their values).
    Mirror,
    /// A human: the worst reasonable case, every foe on our most valuable unit it reaches.
    Worst,
    /// A city-state: holds where it stands.
    Hold,
}

pub fn profile(brief: &TurnBrief, owner: PlayerId) -> Profile {
    use crate::world::player::{ClassicKind, PlayerKind};
    match brief.relation(owner).map(|r| r.kind) {
        Some(PlayerKind::Module(ClassicKind::Barbarian)) => Profile::Barbarian,
        Some(PlayerKind::Module(ClassicKind::CityState(_))) => Profile::Hold,
        _ if brief.humans.contains(&owner) => Profile::Worst,
        _ => Profile::Mirror,
    }
}

/// The value of each target in sight (5a): its cost at the kill price, its
/// threat (what it is expected to destroy of ours next turn at the unit
/// price), the key share of a city of ours it may take next turn.
fn values(theatre: &Theatre<'_, '_>, brief: &TurnBrief, rules: &CompiledRuleset, prices: &Prices) -> Vec<PlanTarget> {
    let field = &theatre.field;
    let mut out = Vec::new();
    // Our cities that may fall next turn: their health against the foes' summed blows.
    let own_cities: Vec<(TileId, u16, u32)> = brief.cities.iter().map(|c| (c.tile, c.hp, c.population)).collect();
    let mut capturers: Vec<(TileId, Vec<(usize, Milli)>, Milli)> = Vec::new();
    for &(tile, hp, pop) in &own_cities {
        let mut sum = 0i64;
        let mut takers: Vec<usize> = Vec::new();
        for (i, foe) in field.targets.iter().enumerate() {
            if foe.is_city() || !foe.stats.military || !field.can_strike(i, tile, &Posture::default()) {
                continue;
            }
            // A city defends with its own strength: a blow on it scales with the foe's attack.
            let blow = i64::from(foe.stats.attack() / 100) * 2 * i64::from(foe.hp) / 100;
            sum += blow;
            if !foe.stats.is_ranged() {
                takers.push(i);
            }
        }
        if takers.is_empty() {
            continue;
        }
        let spread = sum * pw::SPREAD_PERCENT / 100;
        let fall = reach_chance(sum - spread, sum, sum + spread, i64::from(hp));
        if fall > 0 {
            let share = fall / takers.len() as i64;
            capturers.push((tile, takers.iter().map(|&i| (i, share)).collect(), mul(prices.city, city_value(pop))));
        }
    }
    for (i, t) in field.targets.iter().enumerate() {
        if t.remembered {
            continue;
        }
        let mut value = if t.is_city() {
            mul(prices.city, city_value(t.population))
        } else {
            let cost = t.kind.map_or(ONE / 2, |k| unit_value(rules, k));
            let mut v = mul(prices.kill, cost);
            // The threat: its expected blows on our units next turn, at the unit price.
            for u in theatre.own.iter() {
                if !field.can_strike(i, u.tile, &theatre.posture) {
                    continue;
                }
                let me = KindStats::of(rules, u.kind);
                let blow = i64::from(field.blow_by(t, &me, u.hp, u.tile, u.fortified));
                v += mul(prices.unit, unit_value(rules, u.kind) * blow / i64::from(u.hp.max(1)));
            }
            // An enemy siege unit near our walls: a tenth of the city.
            if t.kind.is_some_and(|k| rules.units.get(k.0 as usize).is_some_and(|d| d.role == Some(crate::rules::content::UnitRole::Siege))) {
                if let Some((_, _, worth)) = capturers.iter().find(|(c, _, _)| field.distance(*c, t.tile) <= 3) {
                    v += worth / 10;
                }
            }
            v
        };
        let mut key = false;
        for (_, takers, worth) in &capturers {
            if let Some((_, share)) = takers.iter().find(|(k, _)| *k == i) {
                value += mul(*worth, *share);
                key = true;
            }
        }
        out.push(PlanTarget { tile: t.tile, hp: t.hp, value, key, city: t.is_city(), assigned: (0, 0, 0), p_kill: 0 });
    }
    out
}

/// f(damage) of a target (5a): P(kill)·V + (1 − P)·½·V·d/hp. A city dies
/// only to a capture: without one its kill chance is 0 and damage counts.
fn worth(target: &PlanTarget, assigned: (i64, i64, i64), capture: bool) -> (Milli, Milli) {
    let hp = i64::from(target.hp.max(1));
    let p = if target.city && !capture { 0 } else { reach_chance(assigned.0, assigned.1, assigned.2, hp) };
    let d = assigned.1.min(hp);
    let wound = mul(target.value * d / hp, pw::WOUND_SHARE);
    (mul(p, target.value) + mul(ONE - p, wound), p)
}

/// The chance our unit dies on `s`'s variant: the blow's answer and the
/// danger where it ends against its health.
fn death_chance(s: &Strike, hp: u16) -> Milli {
    let total = i64::from(s.taken) + i64::from(s.danger);
    let spread = total * pw::SPREAD_PERCENT / 100;
    reach_chance(total - spread, total, total + spread, i64::from(hp))
}

/// The greedy fire allocation (5a) over `options` for the units in
/// `units` (with their health), by marginal value less the price of the
/// risk; `penalties` from the reply check.
fn allocate(targets: &mut [PlanTarget], options: &[Strike], units: &[(EntityId, u16, Milli)], prices: &Prices, penalties: &[(EntityId, TileId, Milli)], at: &[(EntityId, TileId)]) -> Vec<Assignment> {
    for t in targets.iter_mut() {
        t.assigned = (0, 0, 0);
        t.p_kill = 0;
    }
    // N4, the places at the wall: the tiles a capture is planned from are kept for the
    // capturers (a shooter that moves does not take them), and a shot is not planned from a
    // tile another of our fighters holds.
    let reserved: Vec<(TileId, TileId)> = options.iter().filter(|s| s.captures && s.moves).map(|s| (s.target_tile, s.from)).collect();
    let held = |s: &Strike| at.iter().any(|(id, tile)| *id != s.unit && *tile == s.from);
    // N7: a city one of the units can take counts its kill chance for the shots before its
    // capturer is assigned (the capturer is kept for the end, the fire serves its capture).
    let capturable: Vec<TileId> = options.iter().filter(|s| s.captures && units.iter().any(|u| u.0 == s.unit)).map(|s| s.target_tile).collect();
    let keeps_room = |s: &Strike| {
        if s.captures || !s.moves {
            return true;
        }
        let elsewhere = options.iter().any(|o| o.unit == s.unit && o.target_tile == s.target_tile && !reserved.contains(&(o.target_tile, o.from)));
        !(elsewhere && reserved.contains(&(s.target_tile, s.from)))
    };
    let mut out: Vec<Assignment> = Vec::new();
    let mut used_tiles: Vec<TileId> = Vec::new();
    let mut captured: Vec<TileId> = Vec::new();
    // The city the force can take this turn is served first (all the fire it can use), then the
    // others with what is left: fire spread over several cities takes none of them.
    let focus = focus_city(targets, options, units);
    let passes: &[Option<TileId>] = if focus.is_some() { &[focus, None] } else { &[None] };
    for &restrict in passes {
        loop {
            let mut best: Option<(Milli, usize)> = None;
            for (k, s) in options.iter().enumerate() {
                if restrict.is_some_and(|r| s.target_tile != r) || out.iter().any(|a| a.unit == s.unit) {
                    continue;
                }
                let Some(&(_, hp, value)) = units.iter().find(|u| u.0 == s.unit) else { continue };
                if s.moves && used_tiles.contains(&s.from) {
                    continue;
                }
                if (!s.captures && held(s)) || !keeps_room(s) {
                    continue;
                }
                let Some(t) = targets.iter().find(|t| t.tile == s.target_tile) else { continue };
                // Enough fire: the next shooter is kept; a city still needs its capturer.
                if t.p_kill >= pw::NEAR_CERTAIN && !(s.captures && t.city && !captured.contains(&t.tile)) {
                    continue;
                }
                // N7, the capturer ahead: a capturer strikes a city only when the city can fall
                // this turn — to the fire assigned, its blow and the blows of the capturers still
                // free (the joint chance of the takers); else it is kept for the capture.
                if s.captures && t.city {
                    let free: (i64, i64, i64) = free_capturers(options, &out, s);
                    let joint = (t.assigned.0 + i64::from(s.dealt.0) + free.0, t.assigned.1 + i64::from(s.dealt.1) + free.1, t.assigned.2 + i64::from(s.dealt.2) + free.2);
                    if worth(t, joint, true).1 == 0 {
                        continue;
                    }
                }
                // A city with a taker already chosen counts its kill chance: the shooters keep adding fire.
                let taken = captured.contains(&t.tile) || capturable.contains(&t.tile);
                let before = worth(t, t.assigned, taken).0;
                let after_sum = (t.assigned.0 + i64::from(s.dealt.0), t.assigned.1 + i64::from(s.dealt.1), t.assigned.2 + i64::from(s.dealt.2));
                let after = worth(t, after_sum, s.captures || taken).0;
                let risk = mul(mul(prices.unit, value), death_chance(s, hp));
                let penalty: Milli = penalties.iter().filter(|p| p.0 == s.unit && p.1 == s.from).map(|p| p.2).sum();
                let gain = after - before - risk - penalty;
                if gain > 0 && best.is_none_or(|(g, b)| gain > g || (gain == g && k < b)) {
                    best = Some((gain, k));
                }
            }
            let Some((gain, k)) = best else { break };
            let s = options[k];
            let t = targets.iter_mut().find(|t| t.tile == s.target_tile).expect("a target of the options");
            t.assigned = (t.assigned.0 + i64::from(s.dealt.0), t.assigned.1 + i64::from(s.dealt.1), t.assigned.2 + i64::from(s.dealt.2));
            let (_, p) = worth(t, t.assigned, s.captures || captured.contains(&t.tile) || capturable.contains(&t.tile));
            t.p_kill = p;
            if s.captures && !captured.contains(&t.tile) {
                captured.push(t.tile);
            }
            if s.moves {
                used_tiles.push(s.from);
            }
            let task = if s.captures { Task::Take { target: s.target_tile, from: s.from } } else { Task::Strike { target: s.target_tile, from: s.from } };
            out.push(Assignment { unit: s.unit, task, gain });
        }
    }
    out
}

/// N7: the best blows on `s`'s city of the other capture-capable units with no assignment
/// yet (min, expected, max summed).
fn free_capturers(options: &[Strike], out: &[Assignment], s: &Strike) -> (i64, i64, i64) {
    let mut best: Vec<(EntityId, (u16, u16, u16))> = Vec::new();
    for o in options.iter().filter(|o| o.captures && o.target_tile == s.target_tile && o.unit != s.unit && !out.iter().any(|a| a.unit == o.unit)) {
        match best.iter_mut().find(|b| b.0 == o.unit) {
            Some(b) if o.dealt.1 > b.1 .1 => b.1 = o.dealt,
            Some(_) => {}
            None => best.push((o.unit, o.dealt)),
        }
    }
    best.iter().fold((0, 0, 0), |a, (_, d)| (a.0 + i64::from(d.0), a.1 + i64::from(d.1), a.2 + i64::from(d.2)))
}

/// The city to serve first: of the cities some unit can take, the one with the highest
/// chance to fall to everything that can reach it (each unit's best blow), times its value;
/// None when no city has a taker.
fn focus_city(targets: &[PlanTarget], options: &[Strike], units: &[(EntityId, u16, Milli)]) -> Option<TileId> {
    let mut best: Option<(Milli, TileId)> = None;
    for t in targets.iter().filter(|t| t.city && t.hp > 0) {
        if !options.iter().any(|s| s.target_tile == t.tile && s.captures && units.iter().any(|u| u.0 == s.unit)) {
            continue;
        }
        let mut sum = (0i64, 0i64, 0i64);
        let mut seen: Vec<EntityId> = Vec::new();
        for s in options.iter().filter(|s| s.target_tile == t.tile && units.iter().any(|u| u.0 == s.unit)) {
            if seen.contains(&s.unit) {
                continue;
            }
            seen.push(s.unit);
            let top = options.iter().filter(|o| o.unit == s.unit && o.target_tile == t.tile).max_by_key(|o| o.dealt.1).unwrap_or(s);
            sum = (sum.0 + i64::from(top.dealt.0), sum.1 + i64::from(top.dealt.1), sum.2 + i64::from(top.dealt.2));
        }
        let worth = mul(reach_chance(sum.0, sum.1, sum.2, i64::from(t.hp.max(1))), t.value);
        if worth > 0 && best.is_none_or(|(w, tile)| worth > w || (worth == w && t.tile < tile)) {
            best = Some((worth, t.tile));
        }
    }
    best.map(|(_, tile)| tile)
}

/// The queue: shooters first (the key target's first, then by value), then
/// melee blows that do not take a city, the capturers last; then the units
/// with other tasks.
fn queue(assignments: &[Assignment], targets: &[PlanTarget], options: &[Strike]) -> Vec<EntityId> {
    let rank = |a: &Assignment| -> (u8, std::cmp::Reverse<bool>, std::cmp::Reverse<Milli>, EntityId) {
        let ranged = options.iter().any(|s| s.unit == a.unit && s.ranged);
        let (class, tile) = match a.task {
            Task::Strike { target, .. } if ranged => (0, Some(target)),
            Task::Strike { target, .. } => (1, Some(target)),
            Task::Take { target, .. } => (2, Some(target)),
            Task::Cover { .. } => (3, None),
            Task::Withdraw => (4, None),
        };
        let t = tile.and_then(|tile| targets.iter().find(|t| t.tile == tile));
        (class, std::cmp::Reverse(t.is_some_and(|t| t.key)), std::cmp::Reverse(t.map_or(0, |t| t.value)), a.unit)
    };
    let mut sorted: Vec<&Assignment> = assignments.iter().collect();
    sorted.sort_by_key(|a| rank(a));
    sorted.into_iter().map(|a| a.unit).collect()
}

/// Where each unit stands after the plan (its strike's tile, a melee
/// killer in its target's tile), for the reply check.
fn planned_posture(theatre: &Theatre<'_, '_>, assignments: &[Assignment], targets: &[PlanTarget]) -> (Posture, Vec<(EntityId, TileId)>) {
    let mut at: Vec<(EntityId, TileId)> = theatre.own.iter().map(|u| (u.id, u.tile)).collect();
    for a in assignments {
        let tile = match a.task {
            Task::Strike { target, from } | Task::Take { target, from } => {
                let kills = targets.iter().any(|t| t.tile == target && t.p_kill >= ONE / 2);
                let melee = !theatre.strikes.iter().any(|s| s.unit == a.unit && s.ranged);
                if kills && melee { target } else { from }
            }
            Task::Cover { tile, .. } => tile,
            Task::Withdraw => continue,
        };
        if let Some(e) = at.iter_mut().find(|e| e.0 == a.unit) {
            e.1 = tile;
        }
    }
    let rules = theatre.field.rules;
    let posture = Posture {
        bodies: at.iter().map(|e| e.1).collect(),
        sources: at.iter().filter(|e| theatre.own_unit(e.0).is_some_and(|(_, u)| fighter(rules, u.kind))).map(|e| e.1).collect(),
    };
    (posture, at)
}

/// The enemy's reply on our planned positions (5c, 5d): per our unit the
/// blows the foes still alive are expected to land, by each foe's owner's
/// profile; returns per unit (id, damage, chance to die).
fn reply(theatre: &Theatre<'_, '_>, brief: &TurnBrief, rules: &CompiledRuleset, targets: &[PlanTarget], posture: &Posture, at: &[(EntityId, TileId)], prices: &Prices) -> Vec<(EntityId, i64, Milli)> {
    let field = &theatre.field;
    let mut damage: Vec<(EntityId, i64)> = at.iter().map(|e| (e.0, 0)).collect();
    // Our values as the enemy sees them: a unit's cost; our capturer standing at a city is their key.
    let value_of = |id: EntityId| -> Milli {
        let Some((_, u)) = theatre.own_unit(id) else { return 0 };
        let base = unit_value(rules, u.kind);
        let capturer = at.iter().any(|e| e.0 == id && field.targets.iter().any(|t| t.is_city() && field.distance(t.tile, e.1) <= 1));
        if capturer { base * 3 } else { base }
    };
    let mut struck: Vec<TileId> = Vec::new();
    for (i, foe) in field.targets.iter().enumerate() {
        if foe.remembered || !foe.threatens() {
            continue;
        }
        if targets.iter().any(|t| t.tile == foe.tile && t.p_kill >= ONE / 2) {
            continue;
        }
        let profile = if foe.is_city() { Profile::Mirror } else { profile(brief, foe.owner) };
        let mut best: Option<(i64, EntityId, i64)> = None;
        for &(id, tile) in at {
            let Some((_, u)) = theatre.own_unit(id) else { continue };
            if profile == Profile::Hold && field.distance(foe.tile, tile) > foe.stats.range.max(1) {
                continue;
            }
            if !field.can_strike(i, tile, posture) {
                continue;
            }
            let me = KindStats::of(rules, u.kind);
            let blow = i64::from(field.blow_by(foe, &me, u.hp, tile, false));
            let answer = i64::from(field.answer_by(foe, &me, u.hp, tile, false));
            let score = match profile {
                // The engine's barbarian rule: dealt % − 0.6 · taken % + 100 for a kill; no risky blow.
                Profile::Barbarian => {
                    if answer >= i64::from(foe.hp) {
                        continue;
                    }
                    let dealt = blow * 100 / i64::from(u.hp.max(1));
                    let taken = answer * 100 / i64::from(foe.hp.max(1));
                    dealt - taken * 60 / 100 + if blow >= i64::from(u.hp) { 100 } else { 0 }
                }
                Profile::Worst => value_of(id),
                Profile::Mirror | Profile::Hold => {
                    let p = reach_chance(blow * 2 / 3, blow, blow * 4 / 3, i64::from(u.hp));
                    mul(p, value_of(id)) + mul(ONE - p, value_of(id) * blow / i64::from(u.hp.max(1)) / 2) - mul(prices.unit, answer * ONE / 100) / 10
                }
            };
            // A barbarian strikes one target once (its orders are planned together).
            if profile == Profile::Barbarian && struck.contains(&tile) {
                continue;
            }
            if best.is_none_or(|(s, b, _)| score > s || (score == s && id < b)) {
                best = Some((score, id, blow));
            }
        }
        if let Some((_, id, blow)) = best {
            if let Some(e) = damage.iter_mut().find(|e| e.0 == id) {
                e.1 += blow;
            }
            if let Some(e) = at.iter().find(|e| e.0 == id) {
                struck.push(e.1);
            }
        }
    }
    damage
        .into_iter()
        .map(|(id, d)| {
            let hp = theatre.own_unit(id).map_or(100, |(_, u)| i64::from(u.hp));
            let spread = d * pw::SPREAD_PERCENT / 100;
            (id, d, if d == 0 { 0 } else { reach_chance(d - spread, d, d + spread, hp) })
        })
        .collect()
}

/// The enemy's reply on our units where they stand now (5c, 5d; scenes and
/// traces): per unit the expected damage and the chance to die.
pub fn reply_now(theatre: &Theatre<'_, '_>, brief: &TurnBrief, rules: &CompiledRuleset, prices: &Prices) -> Vec<(EntityId, i64, Milli)> {
    let (posture, at) = planned_posture(theatre, &[], &[]);
    reply(theatre, brief, rules, &[], &posture, &at, prices)
}

/// The cover of the weak (5b, V8): for each weak unit (a shooter, a siege
/// unit, a wounded one) the foes' expected blows from the tiles nobody
/// covers; the free melee units take the tiles that keep most of them off.
fn covers(theatre: &Theatre<'_, '_>, rules: &CompiledRuleset, assignments: &[Assignment], posture: &Posture, at: &[(EntityId, TileId)]) -> Vec<Assignment> {
    let field = &theatre.field;
    let weak: Vec<(EntityId, TileId, KindStats, u16, Milli)> = at
        .iter()
        .filter_map(|&(id, tile)| {
            let (_, u) = theatre.own_unit(id)?;
            let stats = KindStats::of(rules, u.kind);
            (stats.is_ranged() || u.hp < 50 || !stats.military).then_some((id, tile, stats, u.hp, unit_value(rules, u.kind)))
        })
        .collect();
    let flank = |posture: &Posture| -> Milli {
        weak.iter()
            .map(|(_, tile, stats, hp, value)| {
                let blows: i64 = field.targets.iter().enumerate().filter(|(i, f)| !f.is_city() && field.can_strike(*i, *tile, posture)).map(|(_, f)| i64::from(field.blow_by(f, stats, *hp, *tile, false))).sum();
                value * blows.min(i64::from(*hp)) / i64::from((*hp).max(1))
            })
            .sum()
    };
    let mut posture = posture.clone();
    let mut base = flank(&posture);
    let mut out = Vec::new();
    if base == 0 {
        return out;
    }
    let free: Vec<(usize, &OwnUnit)> = theatre.own.iter().enumerate().filter(|(_, u)| fighter(rules, u.kind) && !KindStats::of(rules, u.kind).is_ranged() && u.hp >= 50 && u.moves_left > 0 && !assignments.iter().any(|a| a.unit == u.id)).collect();
    for (ui, u) in free {
        let mut best: Option<(Milli, TileId)> = None;
        for &(t, _) in &theatre.reach[ui] {
            if posture.bodies.contains(&t) && t != u.tile {
                continue;
            }
            if !weak.iter().any(|w| field.distance(w.1, t) <= 2) {
                continue;
            }
            let mut moved = posture.clone();
            if let Some(b) = moved.bodies.iter_mut().find(|b| **b == u.tile) {
                *b = t;
            }
            if let Some(s) = moved.sources.iter_mut().find(|s| **s == u.tile) {
                *s = t;
            }
            // Our own danger there counts against the cover.
            let me = KindStats::of(rules, u.kind);
            let own = i64::from(theatre.danger_on(&me, u.hp, t, false, &moved));
            let kept = base - flank(&moved) - unit_value(rules, u.kind) * own.min(i64::from(u.hp)) / i64::from(u.hp.max(1));
            if kept > 0 && best.is_none_or(|(k, bt)| kept > k || (kept == k && t < bt)) {
                best = Some((kept, t));
            }
        }
        if let Some((kept, t)) = best {
            let ward = weak.iter().min_by_key(|w| (field.distance(w.1, t), w.0)).map(|w| w.0).expect("a weak unit near");
            if let Some(b) = posture.bodies.iter_mut().find(|b| **b == u.tile) {
                *b = t;
            }
            if let Some(s) = posture.sources.iter_mut().find(|s| **s == u.tile) {
                *s = t;
            }
            base = flank(&posture);
            out.push(Assignment { unit: u.id, task: Task::Cover { ward, tile: t }, gain: kept });
            if base == 0 {
                break;
            }
        }
    }
    let _ = at;
    out
}

/// The turn's plan of a theatre (5a–5c): values, the allocation, the reply
/// checked twice with damped penalties, the cover, the queue.
pub fn plan(theatre: &Theatre<'_, '_>, brief: &TurnBrief, rules: &CompiledRuleset, prices: &Prices, units: &[EntityId]) -> TurnPlan {
    let mut targets = values(theatre, brief, rules, prices);
    let options: Vec<Strike> = theatre.strikes.iter().filter(|s| units.contains(&s.unit)).copied().collect();
    let healths: Vec<(EntityId, u16, Milli)> = theatre.own.iter().filter(|u| units.contains(&u.id)).map(|u| (u.id, u.hp, unit_value(rules, u.kind))).collect();
    let mut penalties: Vec<(EntityId, TileId, Milli)> = Vec::new();
    let at: Vec<(EntityId, TileId)> = theatre.own.iter().filter(|u| fighter(rules, u.kind)).map(|u| (u.id, u.tile)).collect();
    let mut assignments = allocate(&mut targets, &options, &healths, prices, &penalties, &at);
    let mut reply_moves = 0;
    for round in 0..pw::REPLY_ROUNDS {
        let (posture, at) = planned_posture(theatre, &assignments, &targets);
        let answer = reply(theatre, brief, rules, &targets, &posture, &at, prices);
        let mut changed = false;
        for a in &assignments {
            let (Task::Strike { from, .. } | Task::Take { from, .. }) = a.task else { continue };
            let Some(&(_, _, death)) = answer.iter().find(|e| e.0 == a.unit) else { continue };
            let value = healths.iter().find(|h| h.0 == a.unit).map_or(0, |h| h.2);
            let price = mul(mul(prices.unit, value), death);
            // A unit that dies for less than its price: a damped penalty on that variant.
            if a.gain < price {
                penalties.push((a.unit, from, mul(mul(price - a.gain + mul(value, pw::DEATH_PENALTY), ONE), pw::REPLY_DAMPING[round])));
                changed = true;
            }
        }
        if !changed {
            break;
        }
        reply_moves += 1;
        assignments = allocate(&mut targets, &options, &healths, prices, &penalties, &at);
    }
    let (posture, at) = planned_posture(theatre, &assignments, &targets);
    let answer = reply(theatre, brief, rules, &targets, &posture, &at, prices);
    let cover = covers(theatre, rules, &assignments, &posture, &at);
    assignments.extend(cover);
    // The wounded that the reply would kill where they stand, with no task: away.
    let tasked: Vec<EntityId> = assignments.iter().map(|a| a.unit).collect();
    for u in theatre.own.iter().filter(|u| units.contains(&u.id) && !tasked.contains(&u.id)) {
        let death = answer.iter().find(|e| e.0 == u.id).map_or(0, |e| e.2);
        if u.hp < 50 && death >= ONE / 2 {
            assignments.push(Assignment { unit: u.id, task: Task::Withdraw, gain: 0 });
        }
    }
    let expected = Expected {
        kills: targets.iter().filter(|t| !t.city && t.p_kill >= ONE / 2).count() as u32,
        captures: targets.iter().filter(|t| t.city && t.p_kill >= ONE / 2).count() as u32,
        losses: answer.iter().map(|e| e.2).sum(),
        dealt: targets.iter().map(|t| t.assigned.1.min(i64::from(t.hp))).sum(),
    };
    let queue = queue(&assignments, &targets, &options);
    TurnPlan { round: brief.round, targets, assignments, queue, expected, options, penalties, spent: Vec::new(), updates: 0, reply_moves, at }
}

/// The fast update after an action (the map of the mini nets, TurnPlan):
/// the targets' health as seen now (`seen`: tile → health; a target not
/// seen any more where we see is killed), the units that acted (`spent`),
/// and the allocation again for the units left — a miss moves the next
/// shooter onto the same target, a kill frees the others; the queue anew.
pub fn update(plan: &mut TurnPlan, seen: &[(TileId, u16)], gone: &[TileId], spent: &[EntityId], units: &[(EntityId, u16, Milli)], prices: &Prices) {
    for t in plan.targets.iter_mut() {
        if let Some(&(_, hp)) = seen.iter().find(|s| s.0 == t.tile) {
            t.hp = hp;
        } else if gone.contains(&t.tile) {
            t.hp = 0;
        }
    }
    for id in spent {
        if !plan.spent.contains(id) {
            plan.spent.push(*id);
        }
    }
    plan.targets.retain(|t| t.hp > 0);
    let left: Vec<(EntityId, u16, Milli)> = units.iter().filter(|u| !plan.spent.contains(&u.0)).copied().collect();
    let options: Vec<Strike> = plan.options.iter().filter(|s| plan.targets.iter().any(|t| t.tile == s.target_tile)).copied().collect();
    let mut targets = plan.targets.clone();
    let fresh = allocate(&mut targets, &options, &left, prices, &plan.penalties, &plan.at);
    // Kept: the spent units' assignments and the cover; the strikes anew.
    let mut assignments: Vec<Assignment> = plan.assignments.iter().filter(|a| plan.spent.contains(&a.unit) || matches!(a.task, Task::Cover { .. } | Task::Withdraw)).copied().collect();
    assignments.retain(|a| !fresh.iter().any(|f| f.unit == a.unit));
    assignments.extend(fresh);
    plan.targets = targets;
    plan.queue = queue(&assignments.iter().filter(|a| !plan.spent.contains(&a.unit)).copied().collect::<Vec<_>>(), &plan.targets, &plan.options);
    plan.assignments = assignments;
    plan.updates += 1;
}

/// The units of the plan: the operations' members and every fighter of
/// ours in contact (a foe within `ENGAGE`).
pub fn plan_units(theatre: &Theatre<'_, '_>, rules: &CompiledRuleset, relations: &[crate::knowledge::observation::Relation]) -> Vec<EntityId> {
    let field = &theatre.field;
    let _ = relations;
    theatre
        .own
        .iter()
        .filter(|u| fighter(rules, u.kind) && field.targets.iter().any(|t| !t.remembered && field.distance(t.tile, u.tile) <= super::weights::fields::ENGAGE && hostile(relations, t.owner)))
        .map(|u| u.id)
        .collect()
}

/// Our unit's health and value for the allocation.
pub fn healths(theatre: &Theatre<'_, '_>, rules: &CompiledRuleset, units: &[EntityId]) -> Vec<(EntityId, u16, Milli)> {
    theatre.own.iter().filter(|u| units.contains(&u.id)).map(|u| (u.id, u.hp, unit_value(rules, u.kind))).collect()
}

/// Kept for the handle lookups of the queue.
pub fn handle_of(theatre: &Theatre<'_, '_>, id: EntityId) -> Option<UnitHandle> {
    theatre.own_unit(id).map(|(_, u)| u.handle)
}

/// Unused weight guard: keeps `unit` imported for the clamp of shares.
pub fn clamp_share(x: Milli) -> Milli {
    unit(x)
}
