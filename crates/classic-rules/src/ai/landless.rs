//! A player with units and no city (`Variant::Landless`; the owner's brief: "capture a city, or do damage
//! to the others"). A player that never had a city lives while it has a unit (`kernel
//! systems/elimination.rs`), so the state exists only before the first city is founded; a captured settler
//! becomes a worker, so the lost settler gives no city. Until then the empire's planners have nothing to
//! say to the units (`Strategy::Expand` with no settler, no task).
//!
//! Active ONLY while the player has no city and no unit that founds one (`active`); in every other state
//! the mind plays `Standard` move for move (the first turns of every game are "no city yet", but then the
//! settler is there). The decision reads the rules' data and the unit's own situation, in this order of worth:
//!
//! 1. Take a city. For every city the player knows (its memory and what it sees now), the estimate of the
//!    group on hand: the city's strength by the rules' formula from what is seen (population, the garrison
//!    on its tile, the technology part at the player's own share), its hit points as the bar shows them, its
//!    heal; the group's expected blows (`operations::estimated_strike`) against it in the front's siege model
//!    (`front::siege_rounds_within`). A city the group takes within `TAKE_ROUNDS` rounds (a margin on the
//!    strength unless the war is on already) is a target; the nearest by walk plus siege rounds is chosen. At
//!    peace the group walks up to it (a city-state's land is open to everyone; a nation's is not) and
//!    declares war in the turn when every member can strike, then strikes in the same turn.
//! 2. Otherwise do harm where it is worth most and cheapest: capture civilians (a worker is worth its price
//!    to the player and costs the owner the same; a settler counts twice: the city it would have founded),
//!    pillage improved tiles (`raid::improvement_value`). The risk of a trip is the fighters and the strike
//!    of cities where the unit would stand, times the price of the unit; the LAST unit of the player is
//!    worth its whole existence (its death is elimination: `LAST_UNIT_PRICE`), and a war on a nation is
//!    dearer than on a city-state (its army hunts us).
//! 3. Otherwise the planners' own business (scouting finds the cities of 1).
//!
//! Second iteration (12 Nov): the walk goes to the nearest tile next to the city the unit HAS A PATH to (a tile
//! behind the garrison or across water stopped the whole group for good); the group declares when three
//! quarters of it stand within `GATHER_RADIUS` of the city (units without a free tile next to it follow when
//! the war is on); a nation's closed border sends the units to the nearest gathering tile outside it, not to
//! a declaration eight tiles away; a war on a nation is priced at one unit (the fighters near the prey are
//! priced already) and the last unit at twice the first iteration's price.
//!
//! No content is named; fog of war is respected (only seen units, remembered cities and known tiles).

use crate::ai::front::{siege_rounds_within, Besieger};
use crate::ai::operations::{army_role, estimate_city, estimated_strike, CityEstimate};
use crate::ai::state::ArmyRole;
use crate::ai::trace::{Actor, Reason};
use crate::knowledge::public::PlayerQueries;
use crate::ai::planner::{role_of, AiRole, Decision, PlanTools, UnitView};
use crate::ai::planner_units::move_along;
use crate::ai::state::AiState;
use crate::combat::city::{CITY_HEAL_PER_TURN, CITY_STRENGTH_FROM_GARRISON_DIVISOR};
use kernel::world::city::CITY_BASE_HIT_POINTS;
use crate::ids::{PlayerId, TileId};
use crate::world::player::{ClassicKind, PlayerKind};
use crate::Command;

/// Rounds of siege within which a group takes a city to be a target. Own design: the war code plans
/// `operations::PLAN_SIEGE_ROUNDS` (3) for an army built for the city; a group on hand has no
/// reinforcement, so a few rounds more are allowed, up to the front's own limit for a siege under way.
pub const TAKE_ROUNDS: u32 = 5;

/// The margin on the city's strength when the war is not on yet, percent: walls and the buildings are not
/// seen, so the estimate is raised before the group commits to a war it cannot call off.
pub const STRENGTH_MARGIN_PERCENT: i64 = 125;

/// The price of the player's last unit in units of its own price: its death is elimination.
pub const LAST_UNIT_PRICE: i64 = 24;


/// A war on a nation, in units of a unit's price: the nation's army hunts the player's units.
pub const MAJOR_WAR_PRICE: i64 = 1;

/// Danger of a tile for the price of the trip: per hostile fighter within `NEAR` tiles, percent of a unit's
/// price; a city that strikes the tile counts as `CITY_FIGHTERS` fighters.
const FIGHTER_PERCENT: i64 = 25;
const NEAR: i32 = 5;
const CITY_FIGHTERS: i64 = 3;

/// How far from its units the player looks for cities on the map it knows, and the population it assumes
/// for one it has not seen (the margin on the strength covers the rest).
const KNOWN_RADIUS: i32 = 12;
const ASSUMED_POPULATION: u32 = 2;

/// Tiles from the city within which a unit counts as gathered at its border.
const GATHER_RADIUS: i32 = 3;

/// Rounds of walking a free unit looks ahead for prey.
const PREY_ROUNDS: i32 = 5;

/// The behaviour is on: the mind plays `landless`, the player has no city and no unit that founds one.
pub fn active(view: &UnitView<'_, '_>, tools: &PlanTools<'_>) -> bool {
    view.brief.landless && view.brief.cities.is_empty() && !view.brief.units.iter().any(|u| tools.rules.units[u.kind.0 as usize].founder)
}

fn dist(view: &UnitView<'_, '_>, a: TileId, b: TileId) -> i32 {
    view.local.view.dimensions().distance(a, b).unwrap_or(i32::MAX / 2)
}

fn movement(tools: &PlanTools<'_>, kind: crate::ids::UnitKindId) -> i32 {
    i32::from(tools.rules.units[kind.0 as usize].movement.max(1))
}

fn at_war(view: &UnitView<'_, '_>, owner: PlayerId) -> bool {
    view.local.relation(owner).is_some_and(|r| r.at_war)
}

/// Whose city or land may be the target of a war: nations and city-states met, not the barbarians.
fn victim(view: &UnitView<'_, '_>, owner: PlayerId) -> Option<Victim> {
    match view.local.relation(owner)?.kind {
        PlayerKind::Major => Some(Victim::Nation),
        PlayerKind::Module(ClassicKind::CityState(_)) => Some(Victim::CityState),
        PlayerKind::Module(ClassicKind::Barbarian) => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Victim {
    Nation,
    CityState,
}

/// A city as the player estimates it.
struct Target {
    tile: TileId,
    owner: PlayerId,
    estimate: CityEstimate,
}

fn city_estimate(view: &UnitView<'_, '_>, tools: &PlanTools<'_>, tile: TileId, population: u32, margin: i64) -> CityEstimate {
    let rules = tools.rules;
    let seen = crate::ai::state::SeenCity { tile, owner: view.local.player, population, last_seen_round: view.brief.round };
    let mut estimate = estimate_city(view.brief, rules, &seen, None);
    // The garrison on the city's tile, as seen now.
    let garrison: i64 = view
        .local
        .foreign_units
        .iter()
        .filter(|u| u.tile == tile && role_of(rules, u.kind) == AiRole::Military)
        .map(|u| i64::from(rules.units[u.kind.0 as usize].strength) * 100 * i64::from(u.hp) / 100 * 100 / CITY_STRENGTH_FROM_GARRISON_DIVISOR)
        .max()
        .unwrap_or(0);
    estimate.strength = (estimate.strength + garrison) * margin / 100;
    // The bar of the city when it is in sight.
    if let Some(city) = view.local.foreign_cities.iter().find(|c| c.tile == tile) {
        estimate.hp = city.hp.max(1);
    } else {
        estimate.hp = CITY_BASE_HIT_POINTS;
    }
    estimate.heal = CITY_HEAL_PER_TURN as u16;
    estimate
}

/// Rounds the military units of the player take to take the city, None when they do not within `limit`
/// or have no melee unit to enter it.
fn siege_estimate(view: &UnitView<'_, '_>, tools: &PlanTools<'_>, estimate: &CityEstimate, limit: u32, only: impl Fn(&crate::ai::planner::UnitBrief) -> bool) -> Option<u32> {
    let rules = tools.rules;
    let mut group: Vec<Besieger> = Vec::new();
    let mut melee = false;
    for u in view.brief.units.iter().filter(|u| u.role == AiRole::Military && only(u)) {
        if let Some(b) = estimated_strike(rules, u.kind, u.hp, estimate) {
            melee |= army_role(rules, u.kind) == Some(ArmyRole::Melee);
            group.push(b);
        }
    }
    if !melee {
        return None;
    }
    siege_rounds_within(&mut group, estimate.hp, estimate.heal, limit)
}

/// The cities the player knows (what it remembers and what it sees now) that its units take.
fn targets(view: &UnitView<'_, '_>, state: &AiState, tools: &PlanTools<'_>) -> Vec<(i64, Target)> {
    let mut cities: Vec<(TileId, PlayerId, u32)> = state.seen_cities().iter().map(|c| (c.tile, c.owner, c.population)).collect();
    for c in &view.local.foreign_cities {
        cities.retain(|(t, _, _)| *t != c.tile);
        cities.push((c.tile, c.owner, c.population));
    }
    // The cities on the map the player knows (explored, not in sight now): the population is not known.
    let dims = view.local.view.dimensions();
    for u in view.brief.units.iter().filter(|u| u.role == AiRole::Military) {
        for tile in crate::math::tiles_in_hex_radius(&dims, u.tile, KNOWN_RADIUS) {
            if cities.iter().any(|(t, _, _)| *t == tile) {
                continue;
            }
            if let Some(snapshot) = view.local.view.tile(tile).snapshot() {
                if let (true, Some(owner)) = (snapshot.city, snapshot.owner) {
                    cities.push((tile, owner, ASSUMED_POPULATION));
                }
            }
        }
    }
    let mut out = Vec::new();
    for (tile, owner, population) in cities {
        if victim(view, owner).is_none() {
            continue;
        }
        let war = at_war(view, owner);
        let margin = if war { 100 } else { STRENGTH_MARGIN_PERCENT };
        let estimate = city_estimate(view, tools, tile, population, margin);
        let rounds_opt = siege_estimate(view, tools, &estimate, if war { TAKE_ROUNDS + 3 } else { TAKE_ROUNDS }, |_| true);
        let Some(rounds) = rounds_opt else { continue };
        let walk = view.brief.units.iter().filter(|u| u.role == AiRole::Military).map(|u| (dist(view, u.tile, tile) + movement(tools, u.kind) - 1) / movement(tools, u.kind)).max().unwrap_or(0);
        out.push((i64::from(rounds) + i64::from(walk), Target { tile, owner, estimate }));
    }
    out.sort_by_key(|(cost, t)| (*cost, t.tile));
    out
}

/// The units that can strike the city this turn take it by themselves (the others follow in).
fn ready_group_takes(view: &UnitView<'_, '_>, tools: &PlanTools<'_>, target: &Target) -> bool {
    let ready = |u: &crate::ai::planner::UnitBrief| {
        let range = crate::ai::combat::ranged_range(tools.rules, u.kind);
        dist(view, u.tile, target.tile) <= movement(tools, u.kind) + range.unwrap_or(1) - i32::from(range.is_none())
    };
    siege_estimate(view, tools, &target.estimate, TAKE_ROUNDS, ready).is_some()
}

/// The tiles next to the city (or, for a shooter, within its range) the unit may walk to, the nearest first: the
/// ones it may stop on. A foreign city cannot be walked into, so the goal of the walk is never the city's own tile.
/// Whether the unit can get to one (a lake, a mountain, a river without a crossing) is the walk's answer, see
/// `walk_to_staging`.
fn staging(view: &UnitView<'_, '_>, tools: &PlanTools<'_>, city: TileId) -> Vec<TileId> {
    let unit = view.local.unit;
    let dims = view.local.view.dimensions();
    let range = crate::ai::combat::ranged_range(tools.rules, unit.kind).unwrap_or(1).max(1);
    let (lo, hi) = if range > 1 { (2, range) } else { (1, 1) };
    let mut out: Vec<TileId> = crate::math::tiles_in_hex_radius(&dims, city, hi)
        .into_iter()
        .filter(|t| dist(view, *t, city) >= lo && view.local.view.tile(*t).snapshot().is_some())
        .filter(|t| *t == unit.tile || crate::ai::planner_units::can_stop(view, tools.rules, *t))
        .collect();
    out.sort_by_key(|t| (dist(view, unit.tile, *t), *t));
    out
}

/// One step toward the nearest staging tile the unit has a path to; None when it has no path to any (or stands on
/// one). The decision may be a refusal (the way is blocked for now): the caller decides what that means.
fn walk_to_staging(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, city: TileId) -> Option<Decision> {
    let unit = view.local.unit;
    let candidates = staging(view, tools, city);
    let tried_none = candidates.is_empty();
    for goal in candidates {
        if goal == unit.tile {
            return None;
        }
        let step = move_along(view, tools, goal, Reason::Raid);
        if matches!(step.reason, Reason::NoPath | Reason::PathOverBudget) {
            continue;
        }
        return Some(step);
    }
    // Candidates, but no way to any of them (a nation's closed border, water): the walk is stopped.
    (!tried_none).then(|| Decision::refuse(Actor::Unit(unit.id), Reason::NoPath))
}

/// One step toward the tile nearest the city, within `GATHER_RADIUS` of it and nearer than the unit stands, that the
/// unit has a path to (the border of a nation closes the tiles next to the city at peace). None when the unit
/// stands as near as it can get.
fn walk_to_gather(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, city: TileId) -> Option<Decision> {
    let unit = view.local.unit;
    let here = dist(view, unit.tile, city);
    let radius = GATHER_RADIUS;
    let dims = view.local.view.dimensions();
    let mut tiles: Vec<TileId> = crate::math::tiles_in_hex_radius(&dims, city, radius)
        .into_iter()
        .filter(|t| dist(view, *t, city) >= 2 && dist(view, *t, city) < here && view.local.view.tile(*t).snapshot().is_some() && crate::ai::planner_units::can_stop(view, tools.rules, *t))
        .collect();
    tiles.sort_by_key(|t| (dist(view, *t, city), dist(view, unit.tile, *t), *t));
    for goal in tiles {
        let step = move_along(view, tools, goal, Reason::Raid);
        if matches!(step.reason, Reason::NoPath | Reason::PathOverBudget) {
            continue;
        }
        return Some(step);
    }
    None
}

/// The lowest id of the group declares, so that one order is given.
fn declares_for_group(view: &UnitView<'_, '_>) -> bool {
    let me = view.local.unit.id;
    !view.brief.units.iter().filter(|u| u.role == AiRole::Military).any(|u| u.id < me)
}

/// The unit's decision, or None when this module has nothing to say (the planners' business).
pub fn decide(view: &UnitView<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Option<Decision> {
    if !active(view, tools) {
        return None;
    }
    take_city(view, state, tools).or_else(|| harm(view, tools))
}

fn take_city(view: &UnitView<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Option<Decision> {
    let unit = view.local.unit;
    let (_, target) = targets(view, state, tools).into_iter().next()?;
    let actor = Actor::Unit(unit.id);
    if !at_war(view, target.owner) {
        // At peace: walk up to the city; declare when every member can strike this turn.
        // The group is gathered when three quarters of it stand within `GATHER_RADIUS` of the city (its units
        // that stand farther follow when the war is on): the group as a whole takes the city, that is what made it
        // the target, and the units that cannot find a tile next to the city (they are taken by the others) are no
        // reason to wait for ever.
        let gather = GATHER_RADIUS;
        let military = view.brief.units.iter().filter(|u| u.role == AiRole::Military).count();
        let gathered = view.brief.units.iter().filter(|u| u.role == AiRole::Military && dist(view, u.tile, target.tile) <= gather).count();
        if ready_group_takes(view, tools, &target) || gathered * 4 >= military * 3 {
            if !declares_for_group(view) {
                return Some(crate::ai::planner_units::hold(view, Reason::Idle));
            }
            let command = Command::DeclareWar { player: view.local.player, enemy: target.owner };
            if view.local.view.accepts(&command) {
                return Some(Decision::act(Actor::Player, Reason::DeclareWar, command));
            }
        }
        let range = crate::ai::combat::ranged_range(tools.rules, unit.kind);
        let reach = movement(tools, unit.kind) + range.unwrap_or(1) - i32::from(range.is_none());
        if dist(view, unit.tile, target.tile) <= reach {
            // Ready: wait for the others where it stands.
            return Some(crate::ai::planner_units::hold(view, Reason::Idle));
        }
        let Some(step) = walk_to_staging(view, tools, target.tile) else {
            return Some(crate::ai::planner_units::hold(view, Reason::Idle));
        };
        if step.command.is_some() {
            return Some(step);
        }
        // The walk is stopped: a nation's land is closed at peace. The unit goes as near as the border lets it
        // (the gathering tiles, outside the city's strike), and when most of the group stands there (the group
        // takes the city: it is the target), the war is declared and the walk goes on.
        if let Some(step) = walk_to_gather(view, tools, target.tile) {
            if step.command.is_some() {
                return Some(step);
            }
        }
        let gather = GATHER_RADIUS;
        let near: Vec<_> = view.brief.units.iter().filter(|u| u.role == AiRole::Military && dist(view, u.tile, target.tile) <= gather).collect();
        let military = view.brief.units.iter().filter(|u| u.role == AiRole::Military).count();
        if victim(view, target.owner) == Some(Victim::Nation) && near.len() * 4 >= military * 3 && near.iter().all(|u| u.id >= unit.id) {
            let command = Command::DeclareWar { player: view.local.player, enemy: target.owner };
            if view.local.view.accepts(&command) {
                return Some(Decision::act(Actor::Player, Reason::DeclareWar, command));
            }
        }
        return Some(crate::ai::planner_units::hold(view, Reason::Idle));
    }
    // At war: strike or close in.
    let city = view.local.foreign_cities.iter().find(|c| c.tile == target.tile);
    if let Some(city) = city {
        let ranged = crate::ai::combat::ranged_range(tools.rules, unit.kind).is_some();
        let command = if ranged { Command::RangedAttack { unit: unit.handle, target: target.tile } } else { Command::AttackCity { attacker: unit.handle, city: city.handle } };
        if unit.can_attack && view.local.view.accepts(&command) {
            return Some(Decision::act(actor, Reason::AttackCity, command));
        }
    }
    let step = walk_to_staging(view, tools, target.tile)?;
    step.command.is_some().then_some(step)
}

/// What the player does harm to: a civilian to capture or a tile to pillage, with its worth and the tile the
/// unit stands on to do it.
struct Prey {
    tile: TileId,
    owner: PlayerId,
    worth: i64,
    civilian: Option<kernel::ids::UnitHandle>,
}

fn harm(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>) -> Option<Decision> {
    let rules = tools.rules;
    let unit = view.local.unit;
    let prices = view.brief.prices.as_ref()?;
    if role_of(rules, unit.kind) != AiRole::Military {
        return None;
    }
    let price_of = |kind: crate::ids::UnitKindId| i64::from(rules.units[kind.0 as usize].cost) * prices.production / i64::from(prices.rounds_left.max(1));
    let mine = price_of(unit.kind).max(1);
    let military: usize = view.brief.units.iter().filter(|u| u.role == AiRole::Military).count();
    // The group dies with its last unit: the price of one falls with the number of units about it.
    let death = mine * (LAST_UNIT_PRICE / (military.max(1) * military.max(1)) as i64).max(1);
    let dims = view.local.view.dimensions();
    let reach = movement(tools, unit.kind) * PREY_ROUNDS;
    let mut prey: Vec<Prey> = Vec::new();
    // Civilians in sight.
    for f in &view.local.foreign_units {
        if victim(view, f.owner).is_none() || role_of(rules, f.kind) == AiRole::Military {
            continue;
        }
        let def = &rules.units[f.kind.0 as usize];
        if dist(view, unit.tile, f.tile) > reach || def.cost == 0 {
            continue;
        }
        let worth = price_of(f.kind) * if def.founder { 2 } else { 1 };
        prey.push(Prey { tile: f.tile, owner: f.owner, worth, civilian: Some(f.handle) });
    }
    // Improved tiles of a nation or a city-state, as known.
    for tile in crate::math::tiles_in_hex_radius(&dims, unit.tile, reach) {
        let Some(snapshot) = view.local.view.tile(tile).snapshot().copied() else { continue };
        let Some(owner) = snapshot.owner.filter(|o| victim(view, *o).is_some()) else { continue };
        if snapshot.city || view.local.view.known_pillaged(tile) {
            continue;
        }
        let Some(improvement) = snapshot.improvement else { continue };
        let Some(def) = rules.improvements.get(improvement.0 as usize) else { continue };
        if matches!(def.kind, crate::rules::content::ImprovementKind::AncientRuins) || def.kind == crate::classic::content::BARBARIAN_CAMP {
            continue;
        }
        let (a, b) = crate::ai::raid::improvement_value(prices, rules, improvement);
        prey.push(Prey { tile, owner, worth: a + b, civilian: None });
    }
    // The risk where the unit would stand: hostile fighters about the tile (the owner's and the ones at
    // war with us) and the cities that strike it.
    let near_fighters = |tile: TileId, owner: PlayerId| -> i64 {
        let fighters = view
            .local
            .foreign_units
            .iter()
            .filter(|f| role_of(rules, f.kind) == AiRole::Military && (f.owner == owner || at_war(view, f.owner)) && dist(view, f.tile, tile) <= NEAR)
            .count() as i64;
        let strikes = view.local.foreign_cities.iter().filter(|c| (c.owner == owner || at_war(view, c.owner)) && dist(view, c.tile, tile) <= crate::combat::city::CITY_ATTACK_RANGE).count() as i64;
        // Cities of the map the player knows strike the tile too, in sight or not.
        let known = crate::math::tiles_in_hex_radius(&dims, tile, crate::combat::city::CITY_ATTACK_RANGE)
            .into_iter()
            .filter(|t| view.local.view.tile(*t).snapshot().is_some_and(|s| s.city && s.owner.is_some_and(|o| o == owner || at_war(view, o))))
            .count() as i64;
        fighters + strikes.max(known) * CITY_FIGHTERS
    };
    let mut best: Option<(i64, i32, TileId, usize)> = None;
    for (i, p) in prey.iter().enumerate() {
        let war_price = if at_war(view, p.owner) { 0 } else if victim(view, p.owner) == Some(Victim::Nation) { mine * MAJOR_WAR_PRICE } else { 0 };
        let risk = death * (near_fighters(p.tile, p.owner) * FIGHTER_PERCENT).min(100) / 100 + mine * crate::ai::raid::LOSS_MIN_PERCENT / 100 + war_price;
        let net = p.worth - risk;
        if net <= 0 {
            continue;
        }
        let d = dist(view, unit.tile, p.tile);
        let key = (-net / i64::from(d.max(1)), d, p.tile, i);
        if best.is_none_or(|b| (b.0, b.1, b.2) > (key.0, key.1, key.2)) {
            best = Some(key);
        }
    }
    let (_, d, _, index) = best?;
    let p = &prey[index];
    let actor = Actor::Unit(unit.id);
    if !at_war(view, p.owner) {
        // Declare when the prey is within a turn's strike, or when the walk is stopped (closed borders).
        let near = d <= 2 * movement(tools, unit.kind);
        let step = if near { None } else { Some(move_along(view, tools, p.tile, Reason::Raid)) };
        let blocked = step.as_ref().is_some_and(|s| s.command.is_none());
        if near || (blocked && d <= 3 * movement(tools, unit.kind) + 2) {
            let command = Command::DeclareWar { player: view.local.player, enemy: p.owner };
            if view.local.view.accepts(&command) {
                return Some(Decision::act(Actor::Player, Reason::DeclareWar, command));
            }
            return None;
        }
        return step.filter(|s| s.command.is_some());
    }
    if let Some(defender) = p.civilian {
        if d == 1 {
            let command = Command::AttackUnit { attacker: unit.handle, defender };
            if unit.can_attack && view.local.view.accepts(&command) {
                return Some(Decision::act(actor, Reason::AttackUnit, command));
            }
        }
    } else if p.tile == unit.tile {
        let command = Command::Pillage { unit: unit.handle };
        if view.local.view.accepts(&command) {
            return Some(Decision::act(actor, Reason::Pillage, command));
        }
    }
    let step = move_along(view, tools, p.tile, Reason::Raid);
    step.command.is_some().then_some(step)
}
