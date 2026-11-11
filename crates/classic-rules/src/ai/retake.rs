//! The retake of a lost city (`Variant::Retake`, `TurnBrief::retake`; the `retake` lane, classic/docs/ai-retake.md).
//!
//! Right after a capture a city is at its weakest: its health is what the siege left, its walls and
//! garrison are not yet the new owner's, and the units that took it stand around it hurt. The former owner
//! sees all of this when its units are near (the city's banner shows its health and its founder, the
//! units are in sight). Without this module the lost city is an ordinary target estimated at full health
//! and handled by the operations (a war already running starts none: `operations::odds_are_good`), the
//! units next to it keep their garrison tasks, and the war ends by the usual peace.
//!
//! A **case** is a city the player founded that an enemy at war holds, in sight, taken lately, which the
//! player's units within two turns of it bring to capture within `RETAKE_ROUNDS` rounds of strikes by the
//! siege model (`front::siege_rounds_within`) with the city's real health, against the units of its
//! holder that stand in the open next to it. The units of the case take part in the assault at once
//! (`planner_units::ensure_task`, no gathering: `tactics::siege_step`), and while a case runs the
//! player's strategy makes no peace with its holder (`planner_strategy::consider_peace`,
//! `operations::idle_wars`, the diplomacy advisor).
//!
//! The case reads only what the seat sees now (the city's banner, the units in sight); it keeps no
//! memory, so it needs nothing in the saved AI memory.

use crate::ai::front::{siege_rounds_within, Besieger};
use crate::ai::operations::{army_role, estimate_city, estimated_strike};
use crate::ai::planner::{unit_power, TurnBrief};
use crate::ai::planner_units::HEAL_BELOW_HP;
use crate::ai::state::{AiState, SeenCity, UnitTask};
use crate::ids::{EntityId, PlayerId, TileId};
use crate::knowledge::observation::AIObservation;
use crate::rules::CompiledRuleset;

/// Rounds of strikes the units on the spot may take to bring the city to capture: this turn's and the next.
/// A longer siege is the operations' (the city heals back, the captor's reinforcements arrive: the
/// operations of a war already running took a city in 3 % of the cases, `operations::odds_are_good`).
pub const RETAKE_ROUNDS: u32 = 2;

/// The captor's units in the open this close to the city fight for it (a city's guard standing
/// outside answers a strike the same turn).
const GUARD_RADIUS: i32 = 2;

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

/// The cases of the turn; empty unless `brief.retake`.
pub fn cases(observation: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) -> Vec<Case> {
    if !brief.retake {
        return Vec::new();
    }
    let dims = brief.dims;
    let window = crate::ai::operations::march_limit(brief);
    let mut out = Vec::new();
    for city in &observation.visible_foreign_cities {
        let lost = city.original_owner == brief.player && city.owner != brief.player;
        if !lost || !brief.at_war(city.owner) || brief.round.saturating_sub(city.acquired_round) > window {
            continue;
        }
        let seen = SeenCity { tile: city.tile, owner: city.owner, population: city.population, last_seen_round: brief.round };
        // The estimate of any enemy city, with the health the banner shows and the garrison in sight: the
        // strongest unit of its holder standing in it (an empty city has none; the estimate of any other
        // city assumes one like our best melee unit).
        let garrison = observation
            .visible_foreign_units
            .iter()
            .filter(|u| u.owner == city.owner && u.tile == city.tile && army_role(rules, u.kind).is_some())
            .max_by_key(|u| (unit_power(rules, u.kind, u.hp), u.handle.index))
            .map(|u| u.kind);
        let mut estimate = estimate_city(brief, rules, &seen, garrison);
        estimate.hp = city.hp;
        let mut group: Vec<(EntityId, Besieger)> = Vec::new();
        let mut own_power = 0u32;
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
            group.push((unit.id, strike));
        }
        if group.is_empty() {
            continue;
        }
        // The holder's units standing in the open next to the city.
        let guard_power: u32 = observation
            .visible_foreign_units
            .iter()
            .filter(|u| u.owner == city.owner && u.tile != city.tile && army_role(rules, u.kind).is_some())
            .filter(|u| dims.distance(u.tile, city.tile).is_some_and(|d| d <= GUARD_RADIUS))
            .map(|u| unit_power(rules, u.kind, u.hp))
            .sum();
        if guard_power > own_power {
            continue;
        }
        let mut besiegers: Vec<Besieger> = group.iter().map(|(_, b)| *b).collect();
        let Some(rounds) = siege_rounds_within(&mut besiegers, estimate.hp, estimate.heal, RETAKE_ROUNDS) else {
            continue;
        };
        out.push(Case { tile: city.tile, enemy: city.owner, hp: city.hp, rounds, group: group.into_iter().map(|(id, _)| id).collect() });
    }
    out
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

