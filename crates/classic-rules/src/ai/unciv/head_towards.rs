// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/unit/HeadTowardsEnemyCityAutomation.kt and
// NextTurnAutomation.getForeignCityNearCapital.

//! Unciv's march on the enemy (`HeadTowardsEnemyCityAutomation`): every unit
//! without a nearer task heads for one city, the enemy city nearest our
//! capital and the enemy's cities within 10 tiles of it; near the city it
//! waits on the landing grounds (3 to 5 tiles out) until the units around can
//! take it within five turns; a unit that outranges the city stays out of its
//! strike. The cities are the ones the player knows (seen or remembered), not
//! every city of the enemy as in Unciv.

use crate::ai::operations::{estimate_city, estimated_strike};
use crate::ai::planner::{Decision, PlanTools};
use crate::ai::state::AiState;
use crate::ai::trace::Reason;
use crate::ids::TileId;
use crate::knowledge::observation::OwnUnit;

use super::sight::{actor, Kind, KnownCity, Unit, CITY_BOMBARD_RANGE};
use super::Step;

const MAX_DISTANCE_FROM_CITY_FOR_LANDING_AREA: i32 = 5;
const MIN_DISTANCE_FROM_CITY_FOR_LANDING_AREA: i32 = 3;
/// What a city heals a round in Unciv's estimate (`cityHealingPerTurn`).
const CITY_HEALING_PER_TURN: i32 = 20;

/// `tryHeadTowardsEnemyCity`.
pub fn try_head_towards_enemy_city(u: &Unit<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Step {
    if u.sight.own_cities.is_empty() {
        return Step::Next;
    }
    // Only focus on attacking one enemy at a time, otherwise you lose on both fronts.
    let cities = enemy_cities_by_priority(u);
    let Some(city) = cities.into_iter().find(|c| u.approach(tools, c.tile).is_some()) else {
        return Step::Next;
    };
    head_towards_enemy_city(u, state, tools, city)
}

/// `getEnemyCitiesByPriority`: the enemy city nearest our capital, and the
/// cities of its owner within 10 tiles of it, nearest first.
pub fn enemy_cities_by_priority(u: &Unit<'_, '_>) -> Vec<KnownCity> {
    let Some(capital) = u.sight.capital.or(u.sight.own_cities.first().map(|c| c.tile)) else {
        return Vec::new();
    };
    let enemy = |c: &&KnownCity| u.at_war(c.owner) && u.local().relation(c.owner).is_some();
    let Some(closest) = u.sight.foreign_cities.iter().filter(enemy).min_by_key(|c| (u.distance(c.tile, capital), c.tile)).copied() else {
        return Vec::new();
    };
    let mut cities: Vec<(i32, KnownCity)> = u
        .sight
        .foreign_cities
        .iter()
        .filter(|c| c.owner == closest.owner)
        .map(|c| (u.distance(c.tile, closest.tile), *c))
        .filter(|(d, _)| *d <= 10)
        .collect();
    cities.sort_by_key(|(d, c)| (*d, c.tile));
    let mut cities: Vec<KnownCity> = cities.into_iter().map(|(_, c)| c).collect();
    if u.kind.is_ranged() {
        // Ranged units don't harm capturable cities, a waste of a turn.
        cities.retain(|c| c.hp != Some(1));
    }
    cities
}

/// `headTowardsEnemyCity`.
pub fn head_towards_enemy_city(u: &Unit<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>, city: KnownCity) -> Step {
    let range = u.kind.range();
    if range > CITY_BOMBARD_RANGE {
        // A long-ranged unit should never stand where the city strikes.
        return head_towards_enemy_city_long_range(u, tools, city.tile, range);
    }
    let Some(goal) = u.approach(tools, city.tile) else {
        return Step::Next;
    };
    let far = u.distance(u.tile(), city.tile) > MAX_DISTANCE_FROM_CITY_FOR_LANDING_AREA;
    if far && u.turns_to(tools, goal).is_some_and(|t| t > MIN_DISTANCE_FROM_CITY_FOR_LANDING_AREA as u32) {
        return head_towards(u, tools, goal, Reason::Advance);
    }
    let ours: Vec<OwnUnit> = u
        .sight
        .own_units
        .iter()
        .copied()
        .map(|o| if o.id == u.me.id { u.me } else { o })
        .filter(|o| Kind { rules: u.rules(), kind: o.kind }.is_military() && u.distance(o.tile, city.tile) <= 6)
        .collect();
    if cannot_take_city_soon(u, state, &ours, city) {
        return head_to_landing_grounds(u, tools, city.tile);
    }
    head_towards(u, tools, goal, Reason::Advance)
}

/// `cannotTakeCitySoon`: the units around cannot take the city within five turns.
fn cannot_take_city_soon(u: &Unit<'_, '_>, state: &AiState, ours: &[OwnUnit], city: KnownCity) -> bool {
    let expected: i32 = ours.iter().map(|o| i32::from(damage_to_city(u, state, o, city))).sum();
    let health = i32::from(city.hp.unwrap_or(crate::world::city::CITY_BASE_HIT_POINTS));
    expected < health && (expected <= CITY_HEALING_PER_TURN || health / (expected - CITY_HEALING_PER_TURN) > 5)
}

/// Unciv's `BattleDamage.calculateDamageToDefender(unit, city)`: the rules'
/// preview of the unit's blow from a tile next to the city when the city is
/// in sight, else the war planner's estimate of a city as last seen.
pub fn damage_to_city(u: &Unit<'_, '_>, state: &AiState, unit: &OwnUnit, city: KnownCity) -> u16 {
    let view = &u.local().view;
    if city.handle.is_some() {
        let dims = u.dims();
        let range = Kind { rules: u.rules(), kind: unit.kind }.range().max(1);
        let from = if u.distance(unit.tile, city.tile) <= range {
            Some(unit.tile)
        } else {
            dims.neighbors(city.tile).into_iter().filter(|t| u.is_land_tile(*t)).min_by_key(|t| (u.distance(*t, unit.tile), *t))
        };
        use crate::knowledge::public::PlayerQueries;
        if let Some(preview) = from.and_then(|from| if from == unit.tile { view.attack_preview(unit.handle, city.tile) } else { view.attack_preview_from(unit.handle, from, city.tile) }) {
            return preview.uncapped_to_defender;
        }
    }
    let seen = state.seen_cities().iter().find(|s| s.tile == city.tile).copied().unwrap_or(crate::ai::state::SeenCity {
        tile: city.tile,
        owner: city.owner,
        population: 1,
        last_seen_round: u.sight.round,
    });
    let estimate = estimate_city(u.brief(), u.rules(), &seen, None);
    estimated_strike(u.rules(), unit.kind, unit.hp, &estimate).map_or(0, |b| b.dealt)
}

/// `headToLandingGrounds`: a land tile 3 to 5 tiles from the city, nearest the unit.
fn head_to_landing_grounds(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>, city: TileId) -> Step {
    let dims = u.dims();
    let mut tiles: Vec<TileId> = crate::math::hex_disc(dims, city, MAX_DISTANCE_FROM_CITY_FOR_LANDING_AREA)
        .filter(|t| u.distance(*t, city) >= MIN_DISTANCE_FROM_CITY_FOR_LANDING_AREA)
        .filter(|t| u.is_land_tile(*t) && u.damage_from_terrain(*t) <= 0)
        .collect();
    tiles.sort_by_key(|t| (u.distance(*t, u.tile()), *t));
    let goal = tiles.into_iter().take(super::CANDIDATE_LIMIT).find(|t| (u.can_move_to(*t) || *t == u.tile()) && u.can_reach(tools, *t));
    match goal {
        Some(goal) if goal != u.tile() => head_towards(u, tools, goal, Reason::Rally),
        // No need to worry, keep going as the movement says.
        _ => Step::Done(Reason::Rally),
    }
}

/// `headTowardsEnemyCityLongRange`.
fn head_towards_enemy_city_long_range(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>, city: TileId, range: i32) -> Step {
    // Already in fire range but without sight: hold position.
    if u.distance(u.tile(), city) <= range {
        return Step::Done(Reason::Waiting);
    }
    let mut candidates: Vec<(TileId, u16)> = u
        .reachable()
        .iter()
        .copied()
        .filter(|(t, _)| u.distance(*t, city) >= range && u.distance(*t, city) > CITY_BOMBARD_RANGE && u.damage_from_terrain(*t) <= 0 && u.can_move_to(*t))
        .collect();
    // Closest to the city first, then the least movement spent.
    candidates.sort_by_key(|(t, left)| (u.distance(*t, city), std::cmp::Reverse(*left), *t));
    match candidates.first() {
        Some((tile, _)) => head_towards(u, tools, *tile, Reason::Advance),
        None => {
            let _ = tools;
            Step::Next
        }
    }
}

/// `movement.headTowards(tile)`: the walk of the turn along the path.
pub fn head_towards(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>, goal: TileId, reason: Reason) -> Step {
    if goal == u.tile() || !u.has_movement() {
        return Step::Done(reason);
    }
    let decision = crate::ai::planner_units::stride_along(u.view, tools, goal, reason);
    match decision.command {
        Some(_) => Step::Act(decision),
        None => Step::Done(reason),
    }
}

/// `movement.moveToTile(tile)` for a tile reached this turn.
pub fn move_to_tile(u: &Unit<'_, '_>, tile: TileId, reason: Reason) -> Step {
    if tile == u.tile() {
        return Step::Done(reason);
    }
    let command = crate::command::Command::MoveUnit { unit: u.me.handle, destination: tile };
    Step::Act(Decision::act(actor(&u.me), reason, command))
}
