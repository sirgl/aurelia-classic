//! The tactician (design sections 3–5): one decision of one unit — its
//! candidates (every legal move, blow, shot, fortification, pillage, the
//! hold and the defer), each candidate's features from the theatre's maps
//! and the rules' previews, the nine concepts of the unit, the thirteen
//! motives, the score by the role × phase row of the output layer, and the
//! commander's assignment for the unit on top. The rule of the design holds:
//! no turn ends where the danger reaches the unit's health unless the move
//! takes a city or kills.

use super::commander::{unit_value, Op, Phase, Task, TurnPlan};
use super::fields::{ch, fighter, mp, Picture, Theatre};
use super::math::{mul, reach_chance, soft, ONE};
use super::weights::{concept as kw, motive as mw, row, TABLE};
use super::{Milli, Prices};
use crate::ai::battlefield::KindStats;
use crate::ai::planner::{role_of, AiRole, Decision, UnitView};
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{EntityId, TileId};
use crate::knowledge::observation::OwnUnit;
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::UnitRole;
use crate::rules::CompiledRuleset;

/// A unit's role for the output layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Melee,
    Ranged,
    Mounted,
    Siege,
    Civilian,
}

pub fn role(rules: &CompiledRuleset, kind: crate::ids::UnitKindId) -> Role {
    if !fighter(rules, kind) {
        return Role::Civilian;
    }
    let def = &rules.units[kind.0 as usize];
    match def.role {
        Some(UnitRole::Siege) => Role::Siege,
        Some(UnitRole::Mounted) if def.range == 0 => Role::Mounted,
        _ if def.ranged_strength > 0 && def.range > 0 => Role::Ranged,
        _ => Role::Melee,
    }
}

/// The candidate kinds (design section 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Move,
    Strike,
    Fortify,
    Defer,
    Pillage,
    Hold,
}

/// One candidate with its features.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub kind: Kind,
    pub command: Option<Command>,
    /// Where the unit ends the turn (as planned) and the target struck.
    pub dest: TileId,
    pub target: Option<TileId>,
    pub p_take: Milli,
    pub p_kill: Milli,
    pub dealt: i64,
    pub taken: i64,
    /// D_u at the end tile for this unit (hit points).
    pub danger: i64,
    /// The best blow this turn from the end tile (a move's follow-up; a
    /// shooter's position).
    pub fire: i64,
    pub motives: [Milli; 14],
    /// The kill chance this blow adds to the fire the plan has already put on its target, times the target's value (N3).
    pub finish: Milli,
    pub assigned: Milli,
    pub score: Milli,
}

/// The nine concepts of a unit (design section 4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Concepts {
    pub wounded: Milli,
    pub under_fire: Milli,
    pub can_kill: Milli,
    pub can_take: Milli,
    pub capturer: Milli,
    pub defer: Milli,
    pub on_front: Milli,
    pub cut: Milli,
    pub in_choke: Milli,
}

/// What the tactician decided and why (traces and tests).
#[derive(Debug, Clone)]
pub struct Verdict {
    pub row: usize,
    pub concepts: Concepts,
    pub chosen: Candidate,
    pub candidates: usize,
}

/// The row of the output layer for a unit.
pub fn row_of(role: Role, op: Option<&Op>, concepts: &Concepts, task: Option<Task>, near_city: bool) -> usize {
    if role == Role::Civilian {
        return row::CIVILIAN;
    }
    let striking = matches!(task, Some(Task::Strike { .. } | Task::Take { .. }));
    if concepts.wounded >= ONE / 2 && !striking {
        return row::WOUNDED;
    }
    let ranged = matches!(role, Role::Ranged | Role::Siege);
    let Some(op) = op else {
        // A unit outside the operations: the assault row of its role at a hostile
        // city it can strike soon, else the defence row.
        return match (near_city, role) {
            (false, _) => row::DEFENCE,
            (true, Role::Mounted) => row::MOUNTED_ASSAULT,
            (true, _) if ranged => row::RANGED_ASSAULT,
            (true, _) => row::MELEE_ASSAULT,
        };
    };
    match op.phase {
        Phase::Retreat => row::RETREAT,
        Phase::Hold => row::DEFENCE,
        Phase::Gather if ranged => row::RANGED_GATHER,
        Phase::Gather if role == Role::Mounted => row::MOUNTED_GATHER,
        Phase::Gather => row::MELEE_GATHER,
        Phase::Approach if ranged => row::RANGED_APPROACH,
        Phase::Approach if role == Role::Mounted => row::MOUNTED_APPROACH,
        Phase::Approach => row::MELEE_APPROACH,
        Phase::Assault if ranged => row::RANGED_ASSAULT,
        Phase::Assault if role == Role::Mounted => row::MOUNTED_ASSAULT,
        Phase::Assault => row::MELEE_ASSAULT,
    }
}

/// The picture of a theatre from a unit's fresh observation, the brief and
/// the memory: our units and the foes it sees, the hostile cities (the
/// ones out of sight remembered), the operations' goals.
pub fn picture(view: &UnitView<'_, '_>, state: &crate::ai::state::AiState, ops: &[Op]) -> Picture {
    let local = view.local;
    let relations = &local.relations;
    let hostile = |o| super::fields::hostile(relations, o);
    let mut own: Vec<OwnUnit> = local.own_units.clone();
    own.push(local.unit);
    own.sort_by_key(|u| u.id);
    let cities: Vec<_> = local.foreign_cities.iter().filter(|c| hostile(c.owner)).copied().collect();
    let dims = local.view.dimensions();
    let remembered = state
        .seen_cities()
        .iter()
        .filter(|c| hostile(c.owner) && !cities.iter().any(|v| v.tile == c.tile) && dims.distance(c.tile, local.unit.tile).is_some_and(|d| d <= local.radius + 3))
        .copied()
        .collect();
    Picture {
        own,
        foes: local.foreign_units.iter().filter(|u| hostile(u.owner)).copied().collect(),
        cities,
        remembered,
        own_cities: view.brief.cities.iter().map(|c| c.tile).collect(),
        targets: ops.iter().map(|o| o.goal()).collect(),
    }
}

/// The concepts of `unit` in the theatre.
pub fn concepts(theatre: &Theatre<'_, '_>, rules: &CompiledRuleset, unit: &OwnUnit, role: Role, others_shoot: bool) -> Concepts {
    let hp = i64::from(unit.hp.max(1));
    let here = unit.tile;
    let danger = i64::from(theatre.unit_danger(rules, unit, here));
    let mine: Vec<_> = theatre.strikes.iter().filter(|s| s.unit == unit.id).collect();
    let can_kill = mine.iter().filter(|s| !theatre.field.targets[s.target].is_city()).map(|s| reach_chance(i64::from(s.dealt.0), i64::from(s.dealt.1), i64::from(s.dealt.2), i64::from(theatre.field.targets[s.target].hp))).max().unwrap_or(0);
    let can_take = mine.iter().filter(|s| s.captures).map(|s| reach_chance(i64::from(s.dealt.0), i64::from(s.dealt.1), i64::from(s.dealt.2), i64::from(theatre.field.targets[s.target].hp))).max().unwrap_or(0);
    let melee = role == Role::Melee || role == Role::Mounted;
    let capturer = if melee {
        theatre
            .field
            .targets
            .iter()
            .filter(|t| t.is_city() && !t.remembered && theatre.field.distance(t.tile, here) <= 3)
            .map(|t| {
                let siege = theatre.map(t.tile, mp::SIEGE);
                let falling = soft(theatre.chan(t.tile, ch::FOCUS), kw::CAPTURER_AT, kw::CAPTURER_WIDTH);
                mul(siege.max(falling / 2), falling)
            })
            .max()
            .unwrap_or(0)
    } else {
        0
    };
    let defer = if others_shoot { mul(capturer, ONE - can_take) } else { 0 };
    Concepts {
        wounded: soft(-(hp * ONE / 100), -kw::WOUNDED_AT, kw::WOUNDED_WIDTH),
        under_fire: soft(danger * ONE / hp, kw::FIRE_AT, kw::FIRE_WIDTH),
        can_kill,
        can_take,
        capturer,
        defer,
        on_front: theatre.map(here, mp::FRONT),
        cut: theatre.map(here, mp::OUR_CUT),
        in_choke: theatre.map(here, mp::CHOKE),
    }
}

/// The candidates of the unit with their features.
fn candidates(view: &UnitView<'_, '_>, theatre: &Theatre<'_, '_>, rules: &CompiledRuleset, may_defer: bool) -> Vec<Candidate> {
    let local = view.local;
    let unit = local.unit;
    let hp = i64::from(unit.hp);
    let blank = |kind, command, dest| Candidate { kind, command, dest, target: None, p_take: 0, p_kill: 0, dealt: 0, taken: 0, danger: 0, fire: 0, motives: [0; 14], finish: 0, assigned: 0, score: 0 };
    let best_from = |from: TileId| theatre.strikes.iter().filter(|s| s.unit == unit.id && s.from == from).map(|s| i64::from(s.dealt.1)).max().unwrap_or(0);
    let follow = |from: TileId| -> (Milli, Milli) {
        let mut take = 0;
        let mut kill = 0;
        for s in theatre.strikes.iter().filter(|s| s.unit == unit.id && s.from == from) {
            let t = &theatre.field.targets[s.target];
            let p = reach_chance(i64::from(s.dealt.0), i64::from(s.dealt.1), i64::from(s.dealt.2), i64::from(t.hp));
            if s.captures {
                take = take.max(p);
            } else if !t.is_city() {
                kill = kill.max(p);
            }
        }
        (take, kill)
    };
    let mut out = Vec::new();
    for command in crate::knowledge::actions::unit_actions(local) {
        let mut c = match command {
            Command::MoveUnit { destination, .. } => {
                let mut c = blank(Kind::Move, Some(command), destination);
                c.danger = i64::from(theatre.unit_danger(rules, &unit, destination));
                c.fire = best_from(destination);
                // The follow-up blow from there this turn (format 10): its take and kill.
                let (take, kill) = follow(destination);
                c.p_take = take;
                c.p_kill = kill;
                c
            }
            Command::AttackUnit { .. } | Command::AttackCity { .. } | Command::RangedAttack { .. } => {
                let Some((_, tile)) = crate::ai::net_e2e::encode::blow(local, &command) else { continue };
                let Some(p) = local.view.attack_preview(unit.handle, tile) else { continue };
                let target = theatre.target_on(tile).map(|(_, t)| *t);
                let city = target.is_some_and(|t| t.is_city()) || local.foreign_cities.iter().any(|c| c.tile == tile);
                let ranged = matches!(command, Command::RangedAttack { .. });
                let chance = reach_chance(i64::from(p.to_defender.0), i64::from(p.to_defender.1), i64::from(p.to_defender.2), i64::from(p.defender_hp));
                let mut c = blank(Kind::Strike, Some(command), unit.tile);
                c.target = Some(tile);
                c.dealt = i64::from(p.to_defender.1);
                c.taken = i64::from(p.to_attacker.1);
                if city {
                    c.p_take = if !ranged && local.view.captures_cities(unit.handle) { chance } else { 0 };
                } else {
                    c.p_kill = chance;
                }
                // A melee blow that kills a unit moves in; else it stays.
                if !ranged && !city && chance >= ONE / 2 {
                    c.dest = tile;
                }
                let after = OwnUnit { hp: unit.hp.saturating_sub(p.to_attacker.1), ..unit };
                c.danger = if c.p_take >= ONE / 2 { 0 } else { i64::from(theatre.unit_danger(rules, &after, c.dest)) };
                c
            }
            Command::SetUpRangedAttack { .. } => {
                let mut c = blank(Kind::Strike, Some(command), unit.tile);
                c.fire = best_from(unit.tile);
                c.dealt = c.fire;
                c.danger = i64::from(theatre.unit_danger(rules, &unit, unit.tile));
                c
            }
            Command::Fortify { .. } => {
                let mut c = blank(Kind::Fortify, Some(command), unit.tile);
                let fortified = OwnUnit { fortified: true, ..unit };
                c.danger = i64::from(theatre.unit_danger(rules, &fortified, unit.tile));
                c.fire = best_from(unit.tile);
                c
            }
            Command::Pillage { .. } => {
                let mut c = blank(Kind::Pillage, Some(command), unit.tile);
                c.danger = i64::from(theatre.unit_danger(rules, &unit, unit.tile));
                c
            }
            _ => continue,
        };
        c.danger = c.danger.min(4 * hp.max(1));
        out.push(c);
    }
    let mut hold = blank(Kind::Hold, None, unit.tile);
    hold.danger = i64::from(theatre.unit_danger(rules, &unit, unit.tile));
    hold.fire = best_from(unit.tile);
    out.push(hold);
    if may_defer {
        let mut defer = blank(Kind::Defer, None, unit.tile);
        defer.danger = out.last().map_or(0, |h| h.danger);
        out.push(defer);
    }
    out
}

/// The flank of the weak around (V8): the foes' expected blows on our
/// shooters, siege and wounded units from where nobody covers, in their
/// value; with the unit `mover` on `tile` instead of where it stands.
fn flank(theatre: &Theatre<'_, '_>, rules: &CompiledRuleset, mover: EntityId, tile: TileId) -> Milli {
    let posture = theatre.moved_posture(mover, tile);
    let field = &theatre.field;
    let mut sum = 0;
    for u in theatre.own.iter().filter(|u| u.id != mover && fighter(rules, u.kind)) {
        let stats = KindStats::of(rules, u.kind);
        if !(stats.is_ranged() || u.hp < 50) || field.distance(u.tile, tile) > 6 {
            continue;
        }
        let blows: i64 = field.targets.iter().enumerate().filter(|(i, f)| !f.is_city() && field.can_strike(*i, u.tile, &posture)).map(|(_, f)| i64::from(field.blow_by(f, &stats, u.hp, u.tile, u.fortified))).sum();
        sum += unit_value(rules, u.kind) * blows.min(i64::from(u.hp)) / i64::from(u.hp.max(1));
    }
    sum
}

/// The motives of a candidate (design section 5) and its score by `row`.
#[allow(clippy::too_many_arguments)]
fn score(c: &mut Candidate, theatre: &Theatre<'_, '_>, rules: &CompiledRuleset, unit: &OwnUnit, role: Role, k: &Concepts, prices: &Prices, goal: Option<TileId>, cohesion: Option<&(Vec<Milli>, Vec<Milli>)>, flank_here: Milli, w: &[Milli; 14]) {
    let hp_after = (i64::from(unit.hp) - c.taken).max(1);
    let melee = matches!(role, Role::Melee | Role::Mounted);
    let shooter = matches!(role, Role::Ranged | Role::Siege);
    let target = c.target.unwrap_or(c.dest);
    let mut m = [0; 14];
    // m1 take, m2 kill, m3 damage.
    m[0] = mul(c.p_take, prices.city);
    // The cut-off bonus is a unit's: a city's garrison is not a foe cut off (N7: a capturer
    // does not spend its blow on a city it cannot take for the bonus of a «cut-off» garrison).
    let on_city = theatre.target_on(target).is_some_and(|(_, t)| t.is_city());
    m[1] = mul(c.p_kill, prices.kill) + if c.kind == Kind::Strike && !on_city { mul(mw::KILL_CUT, theatre.map(target, mp::CUT_OFF)) } else { 0 };
    if c.kind == Kind::Strike {
        m[2] = mul(c.dealt * ONE / 100 * prices.damage / ONE, ONE + theatre.map(target, mp::BALANCE));
    }
    // m4 survive, with the rule of the design.
    let ratio = c.danger * ONE / hp_after;
    let civilian = role == Role::Civilian;
    let ratio = if civilian { mul(ratio, super::weights::plan::CIVILIAN_DANGER) } else { ratio };
    let doomed = c.danger >= hp_after && c.p_take < ONE / 2 && c.p_kill < ONE / 2;
    m[3] = -mul(mul((ratio - mw::SURVIVE_AT).max(0), prices.unit), prices.risk) - if doomed { mw::DOOM } else { 0 };
    // m5 toward the operation's goal: turns closer.
    if let Some(goal) = goal {
        let moves = i64::from(rules.units.get(unit.kind.0 as usize).map_or(2, |d| d.movement.max(1)));
        let dims = theatre.field.dims;
        let d0 = i64::from(dims.distance(unit.tile, goal).unwrap_or(0));
        let d1 = i64::from(dims.distance(c.dest, goal).unwrap_or(0));
        m[4] = (d0 - d1) * mw::TOWARD / moves;
    }
    // m6 formation.
    if let (Some((coh, lonely)), Some(i)) = (cohesion, theatre.index(c.dest)) {
        m[5] = coh[i] - mul(mw::LONELY, lonely[i]);
    }
    // m7 the wall, m8 the firing position, m9 the choke, m10 healing.
    if melee {
        m[6] = theatre.map(c.dest, mp::GAP);
        m[8] = theatre.map(c.dest, mp::CHOKE);
    }
    if shooter && c.kind != Kind::Strike {
        let safe = ONE - soft(c.danger * ONE, mul(i64::from(unit.hp), mw::FIRE_SAFE_AT) * ONE, mw::FIRE_SAFE_WIDTH_HP * ONE);
        m[7] = mul(c.fire * ONE / 100, safe);
    }
    if c.kind != Kind::Strike {
        let heal = mul(theatre.map(c.dest, mp::SAFE2), theatre.chan(c.dest, ch::HEAL));
        m[9] = mul(heal, k.wounded);
    }
    // m11 defer, m12 pillage.
    if c.kind == Kind::Defer {
        m[10] = mul(mw::DEFER, k.defer);
    }
    if c.kind == Kind::Pillage {
        let safe = if c.danger < i64::from(unit.hp) { ONE } else { 0 };
        m[11] = mul(mul(theatre.chan(c.dest, ch::PILLAGE), mw::PILLAGE_BASE + k.wounded), safe);
    }
    // m13 cover: the blows on the weak this end tile keeps off.
    if melee && matches!(c.kind, Kind::Move | Kind::Hold | Kind::Fortify) {
        let kept = flank_here - flank(theatre, rules, unit.id, c.dest);
        m[12] = mul(mul(kept, mw::COVER), prices.unit);
    }
    m[13] = c.finish;
    c.motives = m;
    c.score = (0..14).map(|i| mul(w[i], m[i])).sum::<Milli>() + c.assigned;
}

/// N3: the kill chance this blow adds to the fire the plan has put on its target (the
/// others' still to come, its own taken out), times the target's value, less what the fire
/// already makes certain: a shot that adds nothing is worth nothing, whatever the plan said.
fn finish(c: &Candidate, unit: &OwnUnit, task: Option<Task>, plan: Option<&TurnPlan>) -> Milli {
    let (Some(plan), Some(tile), Kind::Strike) = (plan, c.target, c.kind) else { return 0 };
    let Some(t) = plan.targets.iter().find(|t| t.tile == tile) else { return 0 };
    let own = match task {
        Some(Task::Strike { target, .. } | Task::Take { target, .. }) if target == tile => plan.options.iter().filter(|s| s.unit == unit.id && s.target_tile == tile).map(|s| s.dealt).max_by_key(|d| d.1).unwrap_or((0, 0, 0)),
        _ => (0, 0, 0),
    };
    let others = (t.assigned.0 - i64::from(own.0), t.assigned.1 - i64::from(own.1), t.assigned.2 - i64::from(own.2));
    let hp = i64::from(t.hp.max(1));
    let d = c.dealt;
    let before = reach_chance(others.0.max(0), others.1.max(0), others.2.max(0), hp);
    let after = reach_chance(others.0.max(0) + d * 8 / 10, others.1.max(0) + d, others.2.max(0) + d * 12 / 10, hp);
    mul((after - before).max(0), t.value)
}

/// The assignment's worth for a candidate (m14): the plan's task done by it,
/// or a step to the tile it is done from.
fn assigned(c: &Candidate, unit: &OwnUnit, task: Option<Task>) -> Milli {
    let Some(task) = task else { return 0 };
    match task {
        Task::Strike { target, from } | Task::Take { target, from } => {
            if c.kind == Kind::Strike && c.target == Some(target) && unit.tile == from {
                mw::ASSIGNED
            } else if c.kind == Kind::Strike && c.target.is_none() && unit.tile == from {
                // The set-up before the shot.
                mw::ASSIGNED
            } else if c.kind == Kind::Move && c.dest == from {
                mul(mw::ASSIGNED, mw::ASSIGNED_STEP)
            } else {
                0
            }
        }
        Task::Cover { tile, .. } => {
            let there = (c.kind == Kind::Move && c.dest == tile) || (matches!(c.kind, Kind::Hold | Kind::Fortify) && unit.tile == tile);
            if there { mw::ASSIGNED } else { 0 }
        }
        Task::Withdraw => 0,
    }
}

/// The decision of one unit of a theatre: None when the unit is not the
/// tactician's this turn (no operation, no foe near, a civilian out of
/// danger): the planners decide.
#[allow(clippy::too_many_arguments)]
pub fn decide(view: &UnitView<'_, '_>, rules: &CompiledRuleset, theatre: &Theatre<'_, '_>, plan: Option<&TurnPlan>, ops: &[Op], prices: &Prices, may_defer: bool, job: Option<TileId>) -> Option<(Decision, Verdict)> {
    let unit = view.local.unit;
    let role = role(rules, unit.kind);
    let op = ops.iter().find(|o| o.members.contains(&unit.id));
    let task = plan.and_then(|p| p.task_of(unit.id));
    let others_shoot = plan.is_some_and(|p| p.queue.iter().any(|id| *id != unit.id && !p.spent.contains(id) && p.options.iter().any(|s| s.unit == *id && s.ranged)));
    let k = concepts(theatre, rules, &unit, role, others_shoot);
    let near_city = theatre.field.targets.iter().any(|t| t.is_city() && theatre.field.distance(t.tile, unit.tile) <= 3);
    let r = row_of(role, op, &k, task, near_city);
    let w = &TABLE[r].w;
    let goal = match (op, role) {
        (Some(op), _) => Some(op.goal()),
        (None, _) => job,
    };
    let cohesion = op.map(|op| {
        let tiles: Vec<TileId> = theatre.own.iter().filter(|u| u.id != unit.id && op.members.contains(&u.id)).map(|u| u.tile).collect();
        theatre.cohesion(&tiles)
    });
    let melee = matches!(role, Role::Melee | Role::Mounted);
    let flank_here = if melee { flank(theatre, rules, unit.id, unit.tile) } else { 0 };
    let mut cands = candidates(view, theatre, rules, may_defer);
    for c in &mut cands {
        c.assigned = assigned(c, &unit, task);
        c.finish = finish(c, &unit, task, plan);
        score(c, theatre, rules, &unit, role, &k, prices, goal, cohesion.as_ref(), flank_here, w);
    }
    // The rule: no turn ends where the danger reaches the unit's health
    // unless the move takes or kills; such candidates go when another is left.
    let hp = |c: &Candidate| (i64::from(unit.hp) - c.taken).max(1);
    let doomed = |c: &Candidate| c.danger >= hp(c) && c.p_take < ONE / 2 && c.p_kill < ONE / 2 && c.kind != Kind::Defer;
    if cands.iter().any(|c| !doomed(c)) {
        cands.retain(|c| !doomed(c));
    }
    let n = cands.len();
    // Ties: a blow, then staying (fortified), then a move; the earlier candidate.
    let rank = |k: Kind| match k {
        Kind::Strike => 5,
        Kind::Fortify => 4,
        Kind::Hold => 3,
        Kind::Pillage => 2,
        Kind::Move => 1,
        Kind::Defer => 0,
    };
    let best = cands.into_iter().enumerate().max_by_key(|(i, c)| (c.score, rank(c.kind), std::cmp::Reverse(*i)))?.1;
    let actor = Actor::Unit(unit.id);
    let decision = match (&best.kind, best.command.clone()) {
        (Kind::Defer, _) => Decision::refuse(actor, Reason::Deferred),
        (Kind::Hold, _) => Decision::refuse(actor, Reason::Rally),
        (Kind::Strike, Some(command)) => {
            let city = best.target.is_some_and(|t| view.local.foreign_cities.iter().any(|c| c.tile == t)) || matches!(command, Command::AttackCity { .. });
            Decision::act(actor, if city { Reason::AttackCity } else { Reason::AttackUnit }, command)
        }
        (Kind::Fortify, Some(command)) => Decision::act(actor, Reason::Fortify, command),
        (Kind::Pillage, Some(command)) => Decision::act(actor, Reason::Pillage, command),
        (Kind::Move, Some(command)) => {
            let away = best.motives[4] < 0 || best.danger < i64::from(theatre.unit_danger(rules, &unit, unit.tile)) && best.motives[3] > 0;
            Decision::act(actor, if away { Reason::Retreat } else { Reason::Advance }, command)
        }
        (_, None) => Decision::refuse(actor, Reason::Rally),
    };
    let _ = role_of(rules, unit.kind) == AiRole::Military;
    Some((decision, Verdict { row: r, concepts: k, chosen: best, candidates: n }))
}
