//! Memory update, strategy review and diplomacy (R-07 AI stages B, H).
//!
//! Diplomacy separates contact, relation and war objective: a war is
//! declared only at a strategy review, against a player whose city is
//! known and near, when the army is clearly stronger; it lasts at least
//! `MIN_WAR_ROUNDS`; peace comes when the objective is gone, the war is
//! lost or too long; a new war waits `PEACE_COOLDOWN_ROUNDS`. Losing sight
//! of the enemy never ends a war by itself (no war/peace oscillation).

use crate::ai::planner::{unit_power, AiRole, Decision, TurnBrief};
use crate::ai::state::{
    AiState, ObjectiveKind, SeenCity, Strategy, WarObjective, STRATEGY_REVIEW_ROUNDS,
};
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{EntityId, PlayerId, TileId};
use crate::knowledge::observation::{AIObservation, Relation};
use crate::math::MapDimensions;
use crate::rules::CompiledRuleset;
use crate::world::city::CITY_DEFENSE_BASE;

/// A war lasts at least this many rounds.
pub const MIN_WAR_ROUNDS: u32 = 15;
/// After a peace, no new war with the same player for this many rounds.
pub const PEACE_COOLDOWN_ROUNDS: u32 = 30;
/// A war without success ends after this many rounds.
pub const WAR_WEARINESS_ROUNDS: u32 = 60;
/// No war before this round (the empire first grows).
pub const EARLIEST_WAR_ROUND: u32 = 40;
// EARLIEST_WAR_ROUND is a round of a Standard game (another speed: `SpeedValues::game_round`),
// PREPARE_ROUNDS and PEACE_COOLDOWN_ROUNDS Standard rounds (× the speed's pace).
/// Foreign military power within this distance of our cities is a threat.
pub const THREAT_RADIUS: i32 = 5;
/// A threat not seen again for this many rounds is forgotten.
pub const THREAT_MEMORY_ROUNDS: u32 = 10;
/// A war objective is a known city at most this far from our cities.
pub const OBJECTIVE_RANGE: i32 = 14;
/// Rounds of peace before the AI opens its borders unasked.
pub const BORDERS_AFTER_PEACE: u32 = 20;
/// Unhappiness a new city brings (3 for the city, 1 for its citizen):
/// the empire expands while it stays happy after one more city (expansion
/// follows happiness, instead of a fixed number of cities).
pub const NEW_CITY_UNHAPPINESS: i32 = 4;
/// Cities an empire founds whatever its happiness.
pub const FIRST_CITIES: usize = 2;
/// Rounds without expansion after a settler found no site.
pub const NO_SITE_PAUSE_ROUNDS: u32 = 20;

/// Update the memory from the full observation at the start of a turn.
pub fn remember(obs: &AIObservation<'_>, rules: &CompiledRuleset, state: &mut AiState) {
    let alive: Vec<EntityId> = obs.own_units.iter().map(|u| u.id).collect();
    state.retain_units(&alive);
    remember_cities(obs, state);
    remember_relations(obs, rules, state);
    let cities = obs.own_cities.len() as u32;
    state.strategy.peak_cities = state.strategy.peak_cities.max(cities);
}

fn remember_cities(obs: &AIObservation<'_>, state: &mut AiState) {
    for city in &obs.visible_foreign_cities {
        state.see_city(SeenCity {
            tile: city.tile,
            owner: city.owner,
            population: city.population,
            last_seen_round: obs.round,
        });
    }
    let visible_now = |tile: TileId| obs.visible_foreign_cities.iter().any(|c| c.tile == tile);
    let stale: Vec<TileId> = state
        .seen_cities()
        .iter()
        .filter(|c| obs.view.is_visible(c.tile) && !visible_now(c.tile))
        .map(|c| c.tile)
        .collect();
    for tile in stale {
        state.forget_city(tile);
    }
}

fn remember_relations(obs: &AIObservation<'_>, rules: &CompiledRuleset, state: &mut AiState) {
    let dims = obs.view.dimensions();
    for relation in &obs.relations {
        let threat = threat_of(obs, rules, dims, relation.other);
        let objective_alive = objective_still_known(state, relation.other);
        let memory = state.relation_mut(relation.other);
        if memory.at_war != relation.at_war {
            memory.at_war = relation.at_war;
            memory.last_change_round = Some(obs.round);
        }
        if !relation.at_war || !objective_alive {
            memory.objective = None;
        }
        if threat > 0 {
            memory.threat = threat;
            memory.threat_round = obs.round;
        } else if obs.round.saturating_sub(memory.threat_round) > THREAT_MEMORY_ROUNDS {
            memory.threat = 0;
        }
    }
}

/// A city objective is still remembered as the enemy's (a territory
/// objective is refreshed by `choose_objective` instead).
fn objective_still_known(state: &AiState, other: PlayerId) -> bool {
    let Some(objective) = state.relation(other).and_then(|m| m.objective) else {
        return true;
    };
    if objective.kind == ObjectiveKind::Territory {
        return true;
    }
    state
        .seen_cities()
        .iter()
        .any(|c| c.tile == objective.tile && c.owner == other)
}

/// Power of `other`'s military units the player sees near its cities (or
/// near its units while it has no city).
fn threat_of(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    dims: MapDimensions,
    other: PlayerId,
) -> u32 {
    let anchors: Vec<TileId> = if obs.own_cities.is_empty() {
        obs.own_units.iter().map(|u| u.tile).collect()
    } else {
        obs.own_cities.iter().map(|c| c.tile).collect()
    };
    let near = |tile: TileId| {
        anchors
            .iter()
            .any(|a| dims.distance(*a, tile).is_some_and(|d| d <= THREAT_RADIUS))
    };
    obs.visible_foreign_units
        .iter()
        .filter(|u| {
            u.owner == other && crate::ai::planner::role_of(rules, u.kind) == AiRole::Military
        })
        .filter(|u| near(u.tile))
        .map(|u| unit_power(rules, u.kind, u.hp))
        .sum()
}

/// Recent threat of every player at war with us.
pub fn war_threat(brief: &TurnBrief, state: &AiState) -> u32 {
    let mut total = 0;
    for relation in brief.relations.iter().filter(|r| r.at_war) {
        if let Some(memory) = state.relation(relation.other) {
            total += memory.threat;
        }
    }
    total
}

/// Periodic strategy review, or at once after a major event. A change is a
/// decision without a command (it shows in the trace).
pub fn review_strategy(brief: &TurnBrief, state: &mut AiState) -> Option<Decision> {
    if brief.round < state.strategy.next_review_round && !major_event(brief, state) {
        return None;
    }
    forget_old_losses(brief, state);
    let desired = desired_strategy(brief, state);
    state.strategy.next_review_round = brief.round + STRATEGY_REVIEW_ROUNDS;
    if desired == state.strategy.current {
        return None;
    }
    state.strategy.current = desired;
    state.strategy.since_round = brief.round;
    Some(Decision::refuse(Actor::Player, Reason::StrategyChanged))
}

/// With `SiegeFixes::recover` a lost city counts only while the loss is
/// recent: once the empire has recovered for as long as an operation may
/// march to take a city back (`operations::march_limit`), the cities it
/// holds now are its peak again and Recover ends (the war case decides
/// whether to take the city back, `operations::strategy_wages_war`).
fn forget_old_losses(brief: &TurnBrief, state: &mut AiState) {
    let recovering = state.strategy.current == Strategy::Recover;
    let window = crate::ai::operations::march_limit(brief);
    if brief.war.siege.recover && recovering && brief.round.saturating_sub(state.strategy.since_round) >= window {
        state.strategy.peak_cities = brief.cities.len() as u32;
    }
}

/// The review ran this turn.
fn reviewed_now(brief: &TurnBrief, state: &AiState) -> bool {
    state.strategy.next_review_round == brief.round + STRATEGY_REVIEW_ROUNDS
}

fn major_event(brief: &TurnBrief, state: &AiState) -> bool {
    let war_changed = state
        .relations()
        .iter()
        .any(|m| m.last_change_round == Some(brief.round));
    let city_lost = (brief.cities.len() as u32) < state.strategy.peak_cities;
    let threatened = threatened(brief, state);
    let defending = state.strategy.current == Strategy::Defend;
    war_changed || city_lost || threatened != defending
}

/// The enemies' power near our cities calls for the Defend strategy: above
/// three quarters of our army's. With `SiegeFixes::garrison` a hysteresis:
/// an empire enters it when that power is above its whole army's (the
/// defence's chance of a city's fall passes one half, `defence::chance`) and
/// leaves it below three quarters — at equal armies both sides no longer
/// defend, each city's guard answers its own danger (`ai::defence`).
fn threatened(brief: &TurnBrief, state: &AiState) -> bool {
    let threat = war_threat(brief, state);
    let power = brief.military_power;
    if !brief.war.siege.garrison || state.strategy.current == Strategy::Defend {
        return threat > 0 && threat * 4 > power * 3;
    }
    threat > power
}

fn desired_strategy(brief: &TurnBrief, state: &AiState) -> Strategy {
    if brief.cities.is_empty() {
        return Strategy::Expand;
    }
    if threatened(brief, state) {
        return Strategy::Defend;
    }
    let military = brief.count_role(AiRole::Military);
    if (brief.cities.len() as u32) < state.strategy.peak_cities
        || (military == 0 && brief.speed.standard_round(brief.round) > 30)
    {
        return Strategy::Recover;
    }
    let objective = state
        .relations()
        .iter()
        .any(|m| m.at_war && m.objective.is_some());
    if objective || preparing_war(brief, state) {
        return Strategy::Attack;
    }
    if expansion_open(brief, state) {
        return Strategy::Expand;
    }
    Strategy::Stabilize
}

/// Expansion makes sense: happiness allows one more city and no recent
/// site search failed.
pub fn expansion_open(brief: &TurnBrief, state: &AiState) -> bool {
    !expansion_paused(brief, state) && happiness_allows_city(brief)
}

/// A settler found no site in the last `NO_SITE_PAUSE_ROUNDS`.
pub fn expansion_paused(brief: &TurnBrief, state: &AiState) -> bool {
    match state.strategy.no_site_round {
        Some(round) => brief.round.saturating_sub(round) < NO_SITE_PAUSE_ROUNDS,
        None => false,
    }
}

/// One more city keeps the empire happy (or the empire is still young).
pub fn happiness_allows_city(brief: &TurnBrief) -> bool {
    brief.cities.len() < FIRST_CITIES || brief.happiness >= NEW_CITY_UNHAPPINESS
}

/// Diplomacy of one turn: peace and borders per met player, objectives of
/// running wars, at most one new war (only at a review).
pub fn plan_diplomacy(
    brief: &TurnBrief,
    state: &mut AiState,
    dims: MapDimensions,
) -> Vec<Decision> {
    let mut out = plan_relations(brief, state, dims);
    if reviewed_now(brief, state) {
        out.extend(consider_war(brief, state, dims));
    }
    out
}

/// Peace and borders per met player and the objectives of running wars:
/// the diplomacy without new wars.
pub fn plan_relations(brief: &TurnBrief, state: &mut AiState, dims: MapDimensions) -> Vec<Decision> {
    let mut out = Vec::new();
    for relation in &brief.relations {
        let decision = if relation.at_war {
            choose_objective(brief, state, dims, relation.other);
            consider_peace(brief, state, relation)
        } else {
            consider_borders(brief, state, relation)
        };
        out.extend(decision);
    }
    out
}

pub(crate) fn rounds_since(brief: &TurnBrief, round: Option<u32>) -> u32 {
    match round {
        Some(round) => brief.round.saturating_sub(round),
        None => u32::MAX,
    }
}

fn consider_peace(brief: &TurnBrief, state: &AiState, relation: &Relation) -> Option<Decision> {
    let memory = state.relation(relation.other)?;
    let age = rounds_since(brief, memory.last_change_round);
    if age < MIN_WAR_ROUNDS {
        return None;
    }
    let pointless = memory.objective.is_none() && memory.threat == 0;
    let losing = age >= 2 * MIN_WAR_ROUNDS && brief.military_power * 2 < memory.threat;
    let weary = age >= WAR_WEARINESS_ROUNDS;
    if !(pointless || losing || weary) {
        return None;
    }
    let command = Command::MakePeace {
        player: brief.player,
        enemy: relation.other,
    };
    Some(Decision::act(Actor::Player, Reason::MakePeace, command))
}

fn consider_borders(brief: &TurnBrief, state: &AiState, relation: &Relation) -> Option<Decision> {
    if relation.we_grant_entry || !relation.can_open_borders || state.strategy.current == Strategy::Attack {
        return None;
    }
    let since_war = state
        .relation(relation.other)
        .and_then(|m| m.last_change_round);
    let peace_age = match since_war {
        Some(_) => rounds_since(brief, since_war),
        None => rounds_since(brief, relation.contact.met_round),
    };
    let asked = relation.they_grant_entry && peace_age >= BORDERS_AFTER_PEACE;
    if !asked && peace_age < 2 * BORDERS_AFTER_PEACE {
        return None;
    }
    let command = Command::OpenBorders {
        host: brief.player,
        guest: relation.other,
    };
    Some(Decision::act(Actor::Player, Reason::OpenBorders, command))
}

/// The objective of a running war: the nearest remembered city of `other`
/// within reach once one is known; before that, the edge of its known
/// territory (refreshed as the army reveals more of it).
fn choose_objective(brief: &TurnBrief, state: &mut AiState, dims: MapDimensions, other: PlayerId) {
    let current = state.relation(other).and_then(|m| m.objective);
    if current.is_some_and(|o| o.kind == ObjectiveKind::City) {
        return;
    }
    let next = war_target(brief, state, dims, other);
    if let Some(next) = next {
        if current.map(|o| (o.tile, o.kind)) != Some((next.tile, next.kind)) {
            state.relation_mut(other).objective = Some(next);
        }
    }
}

/// The objective a war with `other` would have now.
fn war_target(
    brief: &TurnBrief,
    state: &AiState,
    dims: MapDimensions,
    other: PlayerId,
) -> Option<WarObjective> {
    if let Some((city, _)) = nearest_enemy_city(brief, state, dims, other) {
        return Some(WarObjective {
            tile: city.tile,
            kind: ObjectiveKind::City,
            chosen_round: brief.round,
        });
    }
    let edge = brief
        .territory
        .iter()
        .find(|e| e.owner == other && e.distance <= OBJECTIVE_RANGE)?;
    Some(WarObjective {
        tile: edge.tile,
        kind: ObjectiveKind::Territory,
        chosen_round: brief.round,
    })
}

/// The remembered city of `other` nearest to one of our cities, within
/// `OBJECTIVE_RANGE`, and its distance.
fn nearest_enemy_city(
    brief: &TurnBrief,
    state: &AiState,
    dims: MapDimensions,
    other: PlayerId,
) -> Option<(SeenCity, i32)> {
    let mut best: Option<(SeenCity, i32)> = None;
    for city in state.seen_cities().iter().filter(|c| c.owner == other) {
        let Some(distance) = brief
            .cities
            .iter()
            .filter_map(|own| dims.distance(own.tile, city.tile))
            .min()
        else {
            continue;
        };
        if distance > OBJECTIVE_RANGE {
            continue;
        }
        if best.is_none_or(|(_, d)| distance < d) {
            best = Some((*city, distance));
        }
    }
    best
}

/// A player the AI could fight: the objective a war would have and what
/// winning there takes.
#[derive(Debug, Clone, Copy)]
struct WarCandidate {
    enemy: PlayerId,
    target: WarObjective,
    distance: i32,
    /// Army power needed: twice the threat seen and the city's defense
    /// (a city of `UNSEEN_CITY_POPULATION` when none is known yet).
    required_power: u32,
    /// The enemy is at most half our size (known cities, else known land).
    smaller: bool,
}

/// Population assumed for an enemy city not seen yet.
pub const UNSEEN_CITY_POPULATION: u32 = 4;

/// Every met player at peace past the cooldown with a known city or known
/// territory within `OBJECTIVE_RANGE`, nearest first (then by player id).
fn war_candidates(brief: &TurnBrief, state: &AiState, dims: MapDimensions) -> Vec<WarCandidate> {
    let mut out = Vec::new();
    for relation in brief.relations.iter().filter(|r| !r.at_war) {
        let memory = state.relation(relation.other);
        if rounds_since(brief, memory.and_then(|m| m.last_change_round)) < brief.speed.pace_rounds(PEACE_COOLDOWN_ROUNDS) {
            continue;
        }
        let Some(target) = war_target(brief, state, dims, relation.other) else {
            continue;
        };
        out.push(candidate(brief, state, relation.other, target));
    }
    out.sort_by_key(|c| (c.distance, c.enemy));
    out
}

fn candidate(
    brief: &TurnBrief,
    state: &AiState,
    enemy: PlayerId,
    target: WarObjective,
) -> WarCandidate {
    let seen = state.seen_cities().iter().find(|c| c.tile == target.tile);
    let population = seen.map(|c| c.population).unwrap_or(UNSEEN_CITY_POPULATION);
    let threat = state.relation(enemy).map(|m| m.threat).unwrap_or(0);
    let defense = 2 * (CITY_DEFENSE_BASE.max(0) as u32 + population);
    let their_cities = state
        .seen_cities()
        .iter()
        .filter(|c| c.owner == enemy)
        .count();
    let edge = brief.territory.iter().find(|e| e.owner == enemy);
    let smaller = match edge {
        Some(edge) if their_cities == 0 => edge.known_tiles * 2 <= brief.own_tiles,
        _ => their_cities > 0 && their_cities * 2 <= brief.cities.len(),
    };
    let distance = brief
        .cities
        .iter()
        .filter_map(|c| brief.dims.distance(c.tile, target.tile))
        .min()
        .unwrap_or(i32::MAX);
    WarCandidate {
        enemy,
        target,
        distance,
        required_power: 2 * (threat + defense),
        smaller,
    }
}

/// No war yet, but a clearly smaller neighbour within reach and the army
/// too weak for it: build the army (strategy Attack before the war).
fn preparing_war(brief: &TurnBrief, state: &AiState) -> bool {
    let season = brief.round + brief.speed.pace_rounds(PREPARE_ROUNDS) >= brief.speed.game_round(EARLIEST_WAR_ROUND);
    let at_war = brief.relations.iter().any(|r| r.at_war);
    if !season || at_war || brief.cities.len() < 2 {
        return false;
    }
    war_candidates(brief, state, brief.dims)
        .iter()
        .any(|c| c.smaller && brief.military_power < c.required_power)
}

/// Rounds before `EARLIEST_WAR_ROUND` an AI starts to arm for a war.
pub const PREPARE_ROUNDS: u32 = 10;

/// A new war at a review: early-game peace, one war at a time, cooldown
/// after the last peace, a known near city (or territory) of the target
/// and a clearly stronger army.
fn consider_war(brief: &TurnBrief, state: &mut AiState, dims: MapDimensions) -> Option<Decision> {
    let ready = brief.round >= brief.speed.game_round(EARLIEST_WAR_ROUND)
        && brief.cities.len() >= 2
        && brief.count_role(AiRole::Military) >= 4
        && !brief.relations.iter().any(|r| r.at_war);
    if !ready {
        return None;
    }
    let candidates = war_candidates(brief, state, dims);
    let chosen = candidates
        .iter()
        .find(|c| brief.military_power >= c.required_power)?;
    let enemy = chosen.enemy;
    state.relation_mut(enemy).objective = Some(chosen.target);
    let command = Command::DeclareWar {
        player: brief.player,
        enemy,
    };
    Some(Decision::act(Actor::Player, Reason::DeclareWar, command))
}
