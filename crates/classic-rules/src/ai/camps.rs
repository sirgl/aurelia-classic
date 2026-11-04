//! Barbarian camps (`camps`, classic/docs/ai-army.md): free military units clear the camps
//! near the cities when it pays. The game's AI never goes for a camp (its scouts walk around
//! them, `scouting`); a camp lives on until a city-state or a unit that happens to pass
//! clears it, and spawns a raider every few rounds meanwhile.
//!
//! - **Which camps.** The camps the player knows within `CAMP_REACH` of its cities (Unciv's
//!   reach, `unciv::unit_automation::try_head_towards_encampment`), nearest first, one at a time.
//! - **Worth.** The rules' camp gold (`systems::barbarians::CAMP_GOLD`, scaled by the game
//!   speed's gold percent) at the allocator's price of gold, and the losses the camp would
//!   still cause: `LOSS_PER_CAMP_ROUND` (measured) a round over the camp's remaining life,
//!   each loss at the price of the cheapest worker's hammers.
//! - **Risk.** The camp's defender (the unit seen on it; unseen, the strongest barbarian kind
//!   in sight, else our own) struck by the group's kinds with the rules' damage formula: each
//!   member strikes as long as the worst answers leave it alive (twice at most), the group is
//!   the fewest free units whose expected blows kill the defender (at most `GROUP_MOST`), its
//!   risk the hammers of a member that one worst answer could kill.
//! - **The group.** Units without a task or beyond their city's garrison, not recon, healthy,
//!   no city of theirs in danger. With none free and a camp worth more than a new unit
//!   (its hammers and upkeep) and the risk, the most productive city trains one (`demand`).
//! - **Not a distraction.** Nothing while the player is at war with a nation or runs or
//!   prepares a war operation; the guards of a city in danger are not free.
//!
//! The task is a raid whose target is the camp's tile (`UnitTask::Raid`): a raid's target is
//! a city we have seen, a camp never is; the raids (`ai::raid`) are off in the variants that
//! clear camps, so the two never meet. Everything reads the brief, the memory and the
//! player's view: nothing hidden.

use crate::ai::planner::{role_of, AiRole, Decision, PlanTools, TurnBrief, UnitBrief, UnitView};
use crate::ai::planner_units::{hold, move_along, HEAL_BELOW_HP};
use crate::ai::state::{AiState, UnitTask};
use crate::ai::trace::{Actor, Reason};
use crate::ai::value::Centi;
use crate::combat::damage::{melee_damage, MeleeSide, Roll};
use crate::ids::{TileId, UnitKindId};
use crate::knowledge::observation::AIObservation;
use crate::rules::content::UnitRole;
use crate::rules::CompiledRuleset;

/// Camps within this many tiles of an own city are the empire's to clear (Unciv's
/// `tryHeadTowardsEncampment`: 6 tiles).
pub const CAMP_REACH: i32 = 6;
/// Units a clearing group takes at most (a raid's, `raid::RAID_SIZE`).
pub const GROUP_MOST: usize = 3;
/// Free units this far from a camp, beyond its reach of the cities, may go.
pub const WALK_MOST: i32 = CAMP_REACH + 4;
/// Losses to barbarians (civilians captured, military units killed) per round of a camp
/// within `CAMP_REACH` of a city, per mille: `standard` on the standard and duel suites at
/// Quick, 80 games, 14.9 losses a seat for 462 such camp-rounds (classic/docs/ai-army.md).
pub const LOSS_PER_CAMP_ROUND: i64 = 32;
/// The rounds a camp near the cities still lives when the player leaves it, in Standard
/// rounds: the measured median life of a camp, 63 rounds at Quick's pace (2/3).
pub const CAMP_LIFE: u32 = 95;
/// A member goes on only this healthy (a full unit strikes the defender twice).
pub const READY_HP: u16 = 80;

/// The camps the player knows within `CAMP_REACH` of its cities, nearest to a city first.
pub fn known_camps(obs: &AIObservation<'_>) -> Vec<TileId> {
    let Some(camp) = crate::systems::barbarians::camp_improvement(obs.view.rules()) else { return Vec::new() };
    let dims = obs.view.dimensions();
    let mut out: Vec<(i32, TileId)> = Vec::new();
    for city in &obs.own_cities {
        for t in crate::math::hex_disc(dims, city.tile, CAMP_REACH) {
            if obs.view.is_known(t) && obs.view.tile(t).snapshot().is_some_and(|s| s.improvement == Some(camp)) {
                let d = dims.distance(city.tile, t).unwrap_or(i32::MAX);
                match out.iter_mut().find(|(_, c)| *c == t) {
                    Some(entry) => entry.0 = entry.0.min(d),
                    None => out.push((d, t)),
                }
            }
        }
    }
    out.sort();
    out.into_iter().map(|(_, t)| t).collect()
}

/// The player may spend units on camps: no war with a nation, no war operation running or
/// prepared. (Barbarians near the cities do not stop it: they are what the camps send; the
/// guards of a city in danger are not free, `free_units`.)
pub fn calm(brief: &TurnBrief, state: &AiState) -> bool {
    let at_war = brief.relations.iter().any(|r| r.at_war);
    !at_war && state.operations().is_empty() && state.war_plan.is_none()
}

/// The camp's defender as the player sees it (kind, health), else the strongest barbarian
/// kind in sight, else `ours` (an even match); full health then.
fn defender(obs: &AIObservation<'_>, rules: &CompiledRuleset, camp: TileId, ours: UnitKindId) -> (UnitKindId, u16) {
    if let Some(u) = obs.visible_foreign_units.iter().find(|u| u.tile == camp && role_of(rules, u.kind) == AiRole::Military) {
        return (u.kind, u.hp);
    }
    let barbarian = |owner| obs.relation(owner).is_some_and(|r| matches!(r.kind, crate::world::player::PlayerKind::Module(crate::world::player::ClassicKind::Barbarian)));
    let strongest = obs
        .visible_foreign_units
        .iter()
        .filter(|u| barbarian(u.owner) && role_of(rules, u.kind) == AiRole::Military)
        .max_by_key(|u| (rules.units[u.kind.0 as usize].strength, u.kind.0))
        .map(|u| u.kind);
    (strongest.unwrap_or(ours), crate::world::unit::UNIT_MAX_HP)
}

/// (expected damage dealt, the most damage taken) of a melee blow of `kind` on `foe` (the
/// rules' formula, base strengths: the place and the barbarian bonus left out).
fn blow(rules: &CompiledRuleset, kind: UnitKindId, foe: UnitKindId) -> (u16, u16) {
    let s = |k: UnitKindId| (i64::from(rules.units[k.0 as usize].strength) * 100).max(100);
    let (a, d) = (s(kind), s(foe));
    let worst = Roll::Drawn(crate::combat::damage::MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE - 1);
    (melee_damage(a, d, 0, 0, Roll::Expected, MeleeSide::UnitVsUnit), melee_damage(d, a, 0, 0, worst, MeleeSide::UnitVsUnit))
}

/// The blows a unit with `hp` strikes before the worst answers could kill it (at most two),
/// and its risk: its hammers at `hammer` when even one blow may kill it.
fn strikes(rules: &CompiledRuleset, kind: UnitKindId, hp: u16, worst: u16, hammer: Centi) -> (u32, Centi) {
    match () {
        _ if 2 * worst < hp => (2, 0),
        _ if worst < hp => (1, 0),
        _ => (1, i64::from(rules.units[kind.0 as usize].cost) * hammer),
    }
}

/// What clearing one camp is worth (hundredths of score points): its gold and the losses it
/// would still cause. None without the allocator's prices.
pub fn worth(brief: &TurnBrief, rules: &CompiledRuleset) -> Option<Centi> {
    let prices = brief.prices.as_ref()?;
    let left = i64::from(prices.rounds_left.max(1));
    let gold = i64::from(kernel::rules::speeds::SpeedValues::scale(crate::systems::barbarians::CAMP_GOLD, brief.speed.gold_percent));
    let worker = rules
        .units
        .iter()
        .enumerate()
        .filter(|(i, u)| role_of(rules, UnitKindId(*i as u16)) == AiRole::Worker && u.cost > 0)
        .map(|(_, u)| i64::from(u.cost))
        .min()
        .unwrap_or(0);
    let life = i64::from(brief.speed.pace_rounds(CAMP_LIFE).min(prices.rounds_left));
    let losses = LOSS_PER_CAMP_ROUND * life * worker * prices.production / left / 1000;
    Some(gold * prices.gold / left + losses)
}

/// The free units: land military units, not recon, healthy, without a task or holding a
/// city beyond its garrison (the higher ids first), none on a city in danger.
fn free_units<'b>(brief: &'b TurnBrief, state: &AiState, rules: &CompiledRuleset) -> Vec<&'b UnitBrief> {
    let wanted = crate::ai::planner_units::garrison_wanted_by(brief, state.strategy.current);
    let mut out = Vec::new();
    for unit in &brief.units {
        if unit.role != AiRole::Military || unit.hp < READY_HP || rules.units.get(unit.kind.0 as usize).and_then(|d| d.role) == Some(UnitRole::Recon) {
            continue;
        }
        let free = match state.task_of(unit.id).map(|t| t.task) {
            None => true,
            Some(UnitTask::Defend { city }) => {
                let senior = state.tasks().iter().filter(|t| t.task == UnitTask::Defend { city } && t.unit < unit.id).count();
                senior >= wanted.max(1) && !brief.endangered(city)
            }
            _ => false,
        };
        if free {
            out.push(unit);
        }
    }
    out
}

/// The camps being cleared (the targets of camp tasks).
fn cleared_now(state: &AiState) -> Vec<TileId> {
    let mut out: Vec<TileId> = state
        .tasks()
        .iter()
        .filter_map(|t| match t.task {
            UnitTask::Raid { target } if !is_city(state, target) => Some(target),
            _ => None,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn is_city(state: &AiState, tile: TileId) -> bool {
    state.seen_cities().iter().any(|c| c.tile == tile)
}

/// A raid task on `target` is a camp's (`camps` on, no city seen there).
pub fn is_camp_task(brief: &TurnBrief, state: &AiState, target: TileId) -> bool {
    brief.camps && !is_city(state, target)
}

/// The group for `camp` among `free`: the nearest units, as few as kill its defender by the
/// expected blows, and their risk (hundredths of score points). None: too far, or even
/// `GROUP_MOST` would not kill it.
fn group<'b>(obs: &AIObservation<'_>, brief: &TurnBrief, rules: &CompiledRuleset, camp: TileId, free: &[&'b UnitBrief]) -> Option<(Vec<&'b UnitBrief>, Centi)> {
    let prices = brief.prices.as_ref()?;
    let dims = brief.dims;
    let mut near: Vec<(i32, &UnitBrief)> = free.iter().filter_map(|u| dims.distance(u.tile, camp).filter(|d| *d <= WALK_MOST).map(|d| (d, *u))).collect();
    near.sort_by_key(|(d, u)| (*d, u.id));
    let first = near.first()?.1.kind;
    let (foe, foe_hp) = defender(obs, rules, camp, first);
    let hammer = prices.production / i64::from(prices.rounds_left.max(1));
    let (mut dealt, mut risk, mut out) = (0u32, 0i64, Vec::new());
    for (_, unit) in near.into_iter().take(GROUP_MOST) {
        let (d, worst) = blow(rules, unit.kind, foe);
        // Each member strikes while the worst answers leave it alive (twice at most).
        let (n, lost) = strikes(rules, unit.kind, unit.hp, worst, hammer);
        dealt += u32::from(d) * n;
        risk += lost;
        out.push(unit);
        if dealt >= u32::from(foe_hp) {
            return Some((out, risk));
        }
    }
    None
}

/// The turn's camp tasks (at its start, with the defence): the camp being cleared keeps its
/// group; else the nearest camp whose worth beats its group's risk gets one.
pub fn assign(obs: &AIObservation<'_>, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset) {
    if !brief.camps || !calm(brief, state) || !cleared_now(state).is_empty() {
        return;
    }
    let Some(worth) = worth(brief, rules) else { return };
    let free = free_units(brief, state, rules);
    if free.is_empty() {
        return;
    }
    for camp in known_camps(obs) {
        let Some((members, risk)) = group(obs, brief, rules, camp, &free) else { continue };
        if worth <= risk {
            continue;
        }
        for unit in members {
            state.set_task(unit.id, UnitTask::Raid { target: camp }, brief.round);
        }
        return;
    }
}

/// Units the cities should train to clear a camp: one when the player is calm, no group
/// clears a camp and none could be formed of free units, no military unit is queued, and a
/// known camp is worth more than a new unit (its hammers at the allocator's price and its
/// upkeep to the end) and the risk.
pub fn demand(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> usize {
    if !calm(brief, state) || !cleared_now(state).is_empty() {
        return 0;
    }
    let queued = obs.own_cities.iter().flat_map(|c| &c.queue).any(|i| matches!(i, crate::ids::ProductionItem::Unit(k) if role_of(rules, *k) == AiRole::Military));
    if queued || !free_units(brief, state, rules).is_empty() {
        return 0;
    }
    let (Some(worth), Some(prices)) = (worth(brief, rules), brief.prices.as_ref()) else { return 0 };
    let Some(kind) = crate::ai::operations::Arsenal::of(brief, rules).melee else { return 0 };
    let camps = known_camps(obs);
    let Some(&camp) = camps.first() else { return 0 };
    let hammer = prices.production / i64::from(prices.rounds_left.max(1));
    let units = obs.own_units.len();
    let upkeep = i64::from(crate::derived::income::unit_upkeep_at(units + 1, brief.round, brief.speed.max_rounds()) - crate::derived::income::unit_upkeep_at(units, brief.round, brief.speed.max_rounds())) * prices.gold;
    let cost = i64::from(rules.units[kind.0 as usize].cost) * hammer + upkeep;
    let (foe, foe_hp) = defender(obs, rules, camp, kind);
    let (d, worst) = blow(rules, kind, foe);
    let (n, risk) = strikes(rules, kind, crate::world::unit::UNIT_MAX_HP, worst, hammer);
    if u32::from(d) * n < u32::from(foe_hp) {
        return 0;
    }
    usize::from(worth > cost + risk)
}

/// The camp task is still one: the camp is there, the player calm, the unit healthy.
pub fn valid(view: &UnitView<'_, '_>, state: &AiState, target: TileId) -> bool {
    let camp = crate::systems::barbarians::camp_improvement(view.local.view.rules());
    let there = camp.is_some() && view.local.view.tile(target).snapshot().is_some_and(|s| s.improvement == camp);
    there && calm(view.brief, state) && view.local.unit.hp >= HEAL_BELOW_HP
}

/// One step of a member: onto the camp when nobody holds it (that clears it), else next to
/// it to strike its defender (the strikes are `tactics::immediate`'s, which comes first),
/// waiting there for the others.
pub fn member_step(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, camp: TileId) -> Decision {
    let unit = view.local.unit;
    let dims = view.local.view.dimensions();
    let held = view.local.foreign_units.iter().any(|u| u.tile == camp);
    if unit.moves_left == 0 {
        return Decision::refuse(Actor::Unit(unit.id), Reason::Waiting);
    }
    if held && dims.adjacent(unit.tile, camp) {
        return hold(view, Reason::Rally);
    }
    if held {
        return crate::ai::planner_units::approach(view, tools, camp, Reason::AttackUnit);
    }
    move_along(view, tools, camp, Reason::AttackUnit)
}
