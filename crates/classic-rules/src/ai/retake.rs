//! The retake of a lost city (`Variant::Retake2`, `TurnBrief::retake`; the `retake` lane, classic/docs/ai-retake.md).
//!
//! Right after a capture a city is at its weakest: its health is what the siege left, its walls and
//! garrison are not yet the new owner's, and the units that took it stand around it hurt. The former owner
//! sees all of this when its units are near (the city's health bar, the units in sight). Without this
//! module the lost city is an ordinary target estimated at full health and handled by the operations (a war
//! already running starts none: `operations::odds_are_good`), the units next to it keep their garrison
//! tasks, and the war ends by the usual peace.
//!
//! A **case** is a city the seat held at its last turn (its own memory, `Memory`), that the player it
//! lost it to holds at war, in sight, within the window, which the seat's units within two turns of it
//! bring to capture within `RETAKE_ROUNDS` rounds of strikes by the siege model
//! (`front::siege_rounds_within`) with the city's real health, against what the captor has in sight
//! around it (`cases_remembered`). The units of the case take part in the assault at once
//! (`planner_units::ensure_task`, no gathering: `tactics::siege_step`), and while a case runs the
//! seat's strategy makes no peace with its holder (`planner_strategy::consider_peace`,
//! `operations::idle_wars`, the diplomacy advisor).
//!
//! The memory is in the strategy memory of `AiState::mind_memory` (a versioned section, see `Memory`).

use crate::ai::front::{siege_rounds_within, Besieger};
use crate::combat::damage::{city_strike_damage, Roll};
use crate::ai::strategy_net;
use crate::ai::operations::{army_role, estimate_city, estimated_strike};
use crate::ai::planner::{unit_power, TurnBrief};
use crate::ai::planner_units::HEAL_BELOW_HP;
use crate::ai::state::{AiState, SeenCity, UnitTask};
use crate::ids::{EntityId, PlayerId, TileId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;

/// Rounds of strikes the units on the spot may take to bring the city to capture: this turn's and the next.
/// A longer siege is the operations' (the city heals back, the captor's reinforcements arrive: the
/// operations of a war already running took a city in 3 % of the cases, `operations::odds_are_good`).
pub const RETAKE_ROUNDS: u32 = 2;

/// A unit takes part when it strikes within this many of its own turns of walking (its movement a turn,
/// then its range): the model counts `RETAKE_ROUNDS` rounds of strikes, and a unit that needs more turns
/// to arrive strikes after the city healed or was reinforced.
const TURNS: i32 = 2;

/// A lost city the units on the spot can retake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub tile: TileId,
    pub enemy: PlayerId,
    /// The city's health as seen.
    pub hp: u16,
    /// Rounds of strikes the siege model needs.
    pub rounds: u32,
    /// The units that strike.
    pub group: Vec<EntityId>,
}

/// The unit may leave its task for the retake: not a member of an operation, not a guard of a city in
/// danger, not the only guard of its city.
fn free_for_retake(brief: &TurnBrief, state: &AiState, unit: EntityId) -> bool {
    match state.task_of(unit).map(|t| t.task) {
        Some(UnitTask::Operation { .. }) | Some(UnitTask::Raid { .. }) => false,
        Some(UnitTask::Defend { city }) => {
            !brief.endangered(city) && state.tasks().iter().filter(|t| t.task == UnitTask::Defend { city }).count() > crate::ai::planner_units::garrison_wanted_by(brief, state.strategy.current).max(1)
        }
        _ => true,
    }
}

impl TurnBrief {
    /// The city the unit retakes this turn.
    pub fn retake_target_of(&self, unit: EntityId) -> Option<TileId> {
        self.retaking.iter().find(|c| c.group.contains(&unit)).map(|c| c.tile)
    }

    /// `tile` is a city being retaken this turn.
    pub fn is_retaken(&self, tile: TileId) -> bool {
        self.retaking.iter().any(|c| c.tile == tile)
    }

    /// A retake against `enemy` runs: no peace with it.
    pub fn retaking_from(&self, enemy: PlayerId) -> bool {
        self.retaking.iter().any(|c| c.enemy == enemy)
    }
}


// ---- retake2: the seat finds the lost city by its own memory, and weighs the guard by what it can see ----

/// The captor's units this close to the city, in sight, answer the assault (a unit four tiles away is
/// two turns from the walls for most of the kinds, and the assault lasts `RETAKE_ROUNDS` rounds).
/// The review of the first iteration: with 14 or more captor units within four tiles a retake cost
/// 7 units per extra city retaken, while the test counted the ones within two tiles only.
const GUARD_RADIUS_NEAR: i32 = 4;

/// The group is not outmatched when its power (after the city's strikes while the assault lasts) is at
/// least this percent of the power of the captor's units in sight within `GUARD_RADIUS_NEAR`. Fitted on
/// the tuning positions (offsets 200-206, 355 positions, 710 plays; classic/docs/ai-retake.md "Fit"):
/// the smallest margin (the most retakes) with own units lost at most 1.0 above `war-pays` per position
/// and failures that lose more than 5 units at most 2 points above it, a third under the gates of the
/// lane (1.5 and 3 points). Margins 100 / 150 / 175 / 200 gave +1.48 / +1.20 / +0.98 / +0.86 units.
const GUARD_MARGIN_PERCENT: u64 = 175;
fn margin_percent() -> u64 {
    GUARD_MARGIN_PERCENT
}

/// Most cities the memory keeps as lost (the oldest go first): a seat that loses more than this in a
/// window has no group left to retake any of them.
const LOST_KEPT: usize = 8;

/// A city of the seat's that another player took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lost {
    pub tile: TileId,
    /// The round the seat found it gone (its next turn after the capture).
    pub round: u32,
    /// The player that holds it: read from the city when the seat sees it; None until then.
    pub captor: Option<PlayerId>,
}

/// What the seat remembers for the retake (`retake2`): its cities at its last turn, and the ones it lost
/// since. It always knows when a city of its own is gone; who took it, and the city's health and
/// garrison, it reads while the city is in sight, as a human player does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory {
    pub held: Vec<TileId>,
    pub lost: Vec<Lost>,
}

/// The version of the section's body (its first byte).
const MEMORY_VERSION: u8 = 1;

impl Memory {
    /// Bytes: version, lost count (1), each lost city (tile 4, round 4, captor 1: 255 none), held count (2), tiles (4 each).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![MEMORY_VERSION, self.lost.len().min(LOST_KEPT) as u8];
        for l in self.lost.iter().take(LOST_KEPT) {
            out.extend_from_slice(&l.tile.0.to_le_bytes());
            out.extend_from_slice(&l.round.to_le_bytes());
            out.push(l.captor.map_or(u8::MAX, |p| p.0));
        }
        out.extend_from_slice(&(self.held.len().min(usize::from(u16::MAX)) as u16).to_le_bytes());
        for t in self.held.iter().take(usize::from(u16::MAX)) {
            out.extend_from_slice(&t.0.to_le_bytes());
        }
        out
    }

    /// The memory of a section's body; None for a version or a length it does not know (the seat then starts afresh).
    pub fn decode(bytes: &[u8]) -> Option<Memory> {
        let (&version, mut rest) = bytes.split_first()?;
        if version != MEMORY_VERSION {
            return None;
        }
        let mut take = |n: usize| -> Option<&[u8]> {
            let (head, tail) = rest.split_at_checked(n)?;
            rest = tail;
            Some(head)
        };
        let n = usize::from(take(1)?[0]);
        let mut lost = Vec::with_capacity(n.min(LOST_KEPT));
        for _ in 0..n {
            let b = take(9)?;
            let captor = (b[8] != u8::MAX).then_some(PlayerId(b[8]));
            lost.push(Lost { tile: TileId(u32::from_le_bytes(b[0..4].try_into().ok()?)), round: u32::from_le_bytes(b[4..8].try_into().ok()?), captor });
        }
        let h = usize::from(u16::from_le_bytes(take(2)?.try_into().ok()?));
        let mut held = Vec::with_capacity(h.min(256));
        for _ in 0..h {
            held.push(TileId(u32::from_le_bytes(take(4)?.try_into().ok()?)));
        }
        Some(Memory { held, lost })
    }

    /// The seat's turn: `own` its cities now, `seen` the cities of others in sight (tile, holder), `window` the
    /// rounds a lost city stays a case. A city held at the last turn and not now was taken; it stays while
    /// the window lasts, the seat does not hold it again, and no third player has taken it from its captor
    /// (a city that changed hands again is not the one the seat lost to this enemy, and is no case).
    pub fn observe(&mut self, now: u32, window: u32, own: &[TileId], seen: &[(TileId, PlayerId)]) {
        let holder = |tile: TileId| seen.iter().find(|(t, _)| *t == tile).map(|(_, p)| *p);
        for tile in &self.held {
            if !own.contains(tile) && !self.lost.iter().any(|l| l.tile == *tile) {
                self.lost.push(Lost { tile: *tile, round: now, captor: holder(*tile) });
            }
        }
        self.lost.retain_mut(|l| {
            if own.contains(&l.tile) || now.saturating_sub(l.round) > window {
                return false;
            }
            match (holder(l.tile), l.captor) {
                (Some(now_holder), None) => {
                    l.captor = Some(now_holder);
                    true
                }
                (Some(now_holder), Some(captor)) => now_holder == captor,
                (None, _) => true,
            }
        });
        if self.lost.len() > LOST_KEPT {
            self.lost.drain(..self.lost.len() - LOST_KEPT);
        }
        self.held = own.to_vec();
        self.held.sort();
    }
}

/// The group is outmatched: its power after the strikes the city makes while the assault lasts
/// (`rounds` strikes of `city_hit` hit points among the group's `group_hp`) is less than `margin`
/// percent of the power of the units that stand there.
pub fn outmatched(own_power: u32, group_hp: u32, city_hit: u32, rounds: u32, guard_power: u32, margin: u64) -> bool {
    let group_hp = u64::from(group_hp.max(1));
    let left = group_hp.saturating_sub(u64::from(city_hit) * u64::from(rounds));
    let after = u64::from(own_power) * left / group_hp;
    after * 100 < u64::from(guard_power) * margin
}

/// `cases` for `retake2`: the lost cities come from the seat's own memory (`Memory`, kept in the
/// strategy memory of `AiState::mind_memory`), not from the banner; a major only.
pub fn cases_remembered(observation: &AIObservation<'_>, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset) -> Vec<Case> {
    if !brief.retake || observation.view.own_kind() != crate::world::player::PlayerKind::Major {
        return Vec::new();
    }
    let dims = brief.dims;
    let window = crate::ai::operations::march_limit(brief);
    let own: Vec<TileId> = brief.cities.iter().map(|c| c.tile).collect();
    let seen: Vec<(TileId, PlayerId)> = observation.visible_foreign_cities.iter().map(|c| (c.tile, c.owner)).collect();
    let mut kept = strategy_net::Memory::decode(&state.mind_memory);
    let mut memory = kept.retake.take().unwrap_or_default();
    memory.observe(brief.round, window, &own, &seen);
    kept.retake = Some(memory.clone());
    state.mind_memory = kept.encode();
    let mut out = Vec::new();
    for lost in &memory.lost {
        let Some(captor) = lost.captor else { continue };
        let Some(city) = observation.visible_foreign_cities.iter().find(|c| c.tile == lost.tile && c.owner == captor) else { continue };
        if !brief.at_war(captor) {
            continue;
        }
        let seen_city = SeenCity { tile: city.tile, owner: city.owner, population: city.population, last_seen_round: brief.round };
        let garrison = observation
            .visible_foreign_units
            .iter()
            .filter(|u| u.owner == captor && u.tile == city.tile && army_role(rules, u.kind).is_some())
            .max_by_key(|u| (unit_power(rules, u.kind, u.hp), u.handle.index))
            .map(|u| u.kind);
        let mut estimate = estimate_city(brief, rules, &seen_city, garrison);
        // The health bar shows what the siege left; a bar above a city's base shows walls.
        estimate.hp = city.hp;
        let mut group: Vec<(EntityId, Besieger)> = Vec::new();
        let (mut own_power, mut group_hp, mut strength_sum) = (0u32, 0u32, 0i64);
        for unit in &brief.units {
            if army_role(rules, unit.kind).is_none() || unit.hp < HEAL_BELOW_HP || !free_for_retake(brief, state, unit.id) {
                continue;
            }
            let Some(distance) = dims.distance(unit.tile, city.tile) else { continue };
            let def = &rules.units[unit.kind.0 as usize];
            let steps = i32::from(def.movement.max(1));
            let range = crate::ai::combat::ranged_range(rules, unit.kind).unwrap_or(1);
            if distance > TURNS * steps + range {
                continue;
            }
            let Some(strike) = estimated_strike(rules, unit.kind, unit.hp, &estimate) else { continue };
            own_power += unit_power(rules, unit.kind, unit.hp);
            group_hp += u32::from(unit.hp);
            strength_sum += i64::from(def.strength.max(1)) * 100;
            group.push((unit.id, strike));
        }
        if group.is_empty() {
            continue;
        }
        let mut besiegers: Vec<Besieger> = group.iter().map(|(_, b)| *b).collect();
        let Some(rounds) = siege_rounds_within(&mut besiegers, estimate.hp, estimate.heal, RETAKE_ROUNDS) else {
            continue;
        };
        // What stands there: the captor's units in sight within `GUARD_RADIUS_NEAR` of the city, outside it (the one
        // in it is the garrison of the estimate).
        let guard_power: u32 = observation
            .visible_foreign_units
            .iter()
            .filter(|u| u.owner == captor && u.tile != city.tile && army_role(rules, u.kind).is_some())
            .filter(|u| dims.distance(u.tile, city.tile).is_some_and(|d| d <= GUARD_RADIUS_NEAR))
            .map(|u| unit_power(rules, u.kind, u.hp))
            .sum();
        // The city strikes once a round at the group (a unit of the group's mean strength).
        let mean = strength_sum / group.len() as i64;
        let city_hit = u32::from(city_strike_damage(estimate.strength, mean, Roll::Expected));
        if outmatched(own_power, group_hp, city_hit, rounds, guard_power, margin_percent()) {
            continue;
        }
        crate::ai::war_probe::note(|p| p.retakes.push((brief.round, brief.player, city.tile.0, captor, group.len() as u32)));
        out.push(Case { tile: city.tile, enemy: captor, hp: city.hp, rounds, group: group.into_iter().map(|(id, _)| id).collect() });
    }
    out
}
