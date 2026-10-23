//! Tactics (R-07 AI stage G): what a military unit does with what it sees
//! next to it, and the assault on the war objective.
//!
//! Combat is judged by the AI's planning model (strength × health from the
//! rules data; a unit in a city or fortified counts more), never by
//! probing attacks or the future combat rolls. Targets come from the fresh
//! local observation of each decision: a target killed by the previous
//! command is simply not there any more (no stale handles). Hurt units
//! next to an enemy fall back to heal; a city is assaulted by a group
//! that gathers within reach first, unless the city is already weak.

use crate::knowledge::public::PlayerQueries;
use crate::ai::planner::{role_of, unit_power, AiRole, Decision, PlanTools, UnitView};
use crate::ai::planner_units::{approach, move_along, nearest_safe_tile, threatened};
use crate::ai::state::{AiState, ObjectiveKind, UnitTask};
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::TileId;
use crate::knowledge::observation::{ObservedCity, ObservedUnit};

/// A unit attacks only with at least this much health.
pub const ATTACK_MIN_HP: u16 = 40;
/// A unit this hurt next to an enemy falls back.
pub const RETREAT_BELOW_HP: u16 = 35;
/// A city at or below this health is attacked without waiting for a group.
pub const WEAK_CITY_HP: u16 = 50;
/// Units of the assault group that gather before the siege starts.
pub const SIEGE_GROUP: usize = 3;
/// Rounds an assault unit waits for its group before it attacks anyway.
pub const RALLY_ROUNDS: u32 = 8;

/// What the moment demands before the task: attack a weak adjacent enemy,
/// strike an adjacent enemy city, or fall back when badly hurt.
pub fn immediate(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
) -> Option<Decision> {
    let unit = view.local.unit;
    if unit.moves_left == 0 {
        return None;
    }
    if unit.hp < RETREAT_BELOW_HP && threatened(view, tools.rules, unit.tile, 2) {
        return Some(fall_back(view, state, tools));
    }
    if crate::ai::combat::ranged_range(tools.rules, unit.kind).is_some() {
        // Ranged units take no answer: they shoot whenever a target is in
        // range and never charge.
        return ranged_step(view, tools);
    }
    if unit.hp < ATTACK_MIN_HP || !unit.can_attack {
        return None;
    }
    if let Some(target) = best_adjacent_target(view, tools) {
        let command = Command::AttackUnit {
            attacker: unit.handle,
            defender: target.handle,
        };
        return Some(Decision::act(
            Actor::Unit(unit.id),
            Reason::AttackUnit,
            command,
        ));
    }
    let city = adjacent_enemy_city(view)?;
    if city.hp <= WEAK_CITY_HP || group_near(view, city.tile) + 1 >= SIEGE_GROUP {
        return Some(attack_city(view, city));
    }
    None
}

/// Enemy units and cities at war within `range` of the unit, in sight.
fn targets_in_range(view: &UnitView<'_, '_>, range: i32) -> Vec<TileId> {
    let unit = view.local.unit;
    let dims = view.local.view.dimensions();
    let near = |t: TileId| dims.distance(unit.tile, t).is_some_and(|d| d <= range);
    let units = view
        .local
        .foreign_units
        .iter()
        .filter(|u| at_war(view, u.owner))
        .map(|u| u.tile);
    let cities = view
        .local
        .foreign_cities
        .iter()
        .filter(|c| at_war(view, c.owner))
        .map(|c| c.tile);
    let mut tiles: Vec<TileId> = units.chain(cities).filter(|t| near(*t)).collect();
    tiles.sort();
    tiles.dedup();
    tiles
}

/// A ranged unit with a target in range: set up (siege) or shoot the
/// target its preview rates best. None: nothing in range worth a shot.
fn ranged_step(view: &UnitView<'_, '_>, tools: &PlanTools<'_>) -> Option<Decision> {
    let unit = view.local.unit;
    let range = crate::ai::combat::ranged_range(tools.rules, unit.kind)?;
    let targets = targets_in_range(view, range);
    if targets.is_empty() || !unit.can_attack {
        return None;
    }
    let actor = Actor::Unit(unit.id);
    if view.local.view.must_set_up_first(unit.handle) {
        let command = Command::SetUpRangedAttack { unit: unit.handle };
        return Some(Decision::act(actor, Reason::AttackUnit, command));
    }
    let (target, _) = crate::ai::combat::best_shot(&view.local.view, unit.handle, &targets)?;
    let city = view.local.foreign_cities.iter().any(|c| c.tile == target);
    let reason = if city { Reason::AttackCity } else { Reason::AttackUnit };
    let command = Command::RangedAttack {
        unit: unit.handle,
        target,
    };
    Some(Decision::act(actor, reason, command))
}

fn fall_back(view: &UnitView<'_, '_>, state: &mut AiState, tools: &mut PlanTools<'_>) -> Decision {
    let unit = view.local.unit;
    let Some(safe) = nearest_safe_tile(view, tools) else {
        return Decision::refuse(Actor::Unit(unit.id), Reason::Retreat);
    };
    state.set_task(unit.id, UnitTask::Heal { at: safe }, view.brief.round);
    tools.counters.tasks_assigned += 1;
    if safe == unit.tile {
        let command = Command::Fortify { unit: unit.handle };
        return if unit.fortified || !unit.can_fortify {
            Decision::refuse(Actor::Unit(unit.id), Reason::Retreat)
        } else {
            Decision::act(Actor::Unit(unit.id), Reason::Retreat, command)
        };
    }
    move_along(view, tools, safe, Reason::Retreat)
}

fn at_war(view: &UnitView<'_, '_>, owner: crate::ids::PlayerId) -> bool {
    view.local.relation(owner).is_some_and(|r| r.at_war)
}

/// Power of a defender: its health and strength, more in a city.
fn defender_power(view: &UnitView<'_, '_>, tools: &PlanTools<'_>, target: &ObservedUnit) -> u32 {
    let power = unit_power(tools.rules, target.kind, target.hp);
    let in_city = view
        .local
        .foreign_cities
        .iter()
        .any(|c| c.tile == target.tile);
    if in_city {
        power + power / 4
    } else {
        power
    }
}

/// The adjacent enemy unit this unit should attack: the best power ratio
/// (civilians are free captures), only when the odds are good.
fn best_adjacent_target(view: &UnitView<'_, '_>, tools: &PlanTools<'_>) -> Option<ObservedUnit> {
    let unit = view.local.unit;
    let dims = view.local.view.dimensions();
    let ours = unit_power(tools.rules, unit.kind, unit.hp).max(1);
    let era = view.local.view.own_era();
    let mut best: Option<((u8, u32), ObservedUnit)> = None;
    for target in &view.local.foreign_units {
        if !at_war(view, target.owner) || !dims.adjacent(unit.tile, target.tile) {
            continue;
        }
        let theirs = defender_power(view, tools, target);
        let civilian = role_of(tools.rules, target.kind) != AiRole::Military;
        let favourable = civilian || ours * 10 >= theirs * 11 || target.hp <= 30;
        // The rules must allow the attack (domains, embarked targets).
        if !favourable || view.local.view.attack_preview(unit.handle, target.tile).is_none() {
            continue;
        }
        let key = (target_priority(tools.rules, target.kind, era), ours * 100 / theirs.max(1));
        if best.is_none_or(|(k, b)| key > k || (key == k && target.tile < b.tile)) {
            best = Some((key, *target));
        }
    }
    best.map(|(_, target)| target)
}

/// Last era in which a `HighEarlyGameOnly` civilian (the worker) is a prize
/// (the classical era); own value, unverified.
const EARLY_GAME_LAST_ERA: u8 = 1;

/// Which adjacent target a unit takes first (`CivilianAttackPriority` of its abilities):
/// high-priority civilians (settlers, great people; workers early on) before
/// military targets, low-priority ones (missionaries, caravans) after them.
fn target_priority(rules: &crate::rules::CompiledRuleset, kind: crate::ids::UnitKindId, era: u8) -> u8 {
    use crate::rules::content::{CivilianPriority, UnitAbility};
    let priority = rules.units.get(kind.0 as usize).and_then(|d| {
        d.module.abilities.iter().find_map(|a| match a {
            UnitAbility::CivilianAttackPriority(p) => Some(*p),
            _ => None,
        })
    });
    match priority {
        Some(CivilianPriority::High) => 3,
        Some(CivilianPriority::HighEarlyGameOnly) if era <= EARLY_GAME_LAST_ERA => 3,
        Some(CivilianPriority::HighEarlyGameOnly) | Some(CivilianPriority::Low) => 1,
        None => 2,
    }
}

fn adjacent_enemy_city(view: &UnitView<'_, '_>) -> Option<ObservedCity> {
    let unit = view.local.unit;
    let dims = view.local.view.dimensions();
    view.local
        .foreign_cities
        .iter()
        .filter(|c| at_war(view, c.owner) && dims.adjacent(unit.tile, c.tile))
        .min_by_key(|c| (c.hp, c.tile))
        .copied()
}

/// Own military units within two tiles of `target`.
fn group_near(view: &UnitView<'_, '_>, target: TileId) -> usize {
    let dims = view.local.view.dimensions();
    view.local
        .own_units
        .iter()
        .filter(|u| u.moves_max > 0 && dims.distance(u.tile, target).is_some_and(|d| d <= 2))
        .count()
}

fn attack_city(view: &UnitView<'_, '_>, city: ObservedCity) -> Decision {
    let unit = view.local.unit;
    let command = Command::AttackCity {
        attacker: unit.handle,
        city: city.handle,
    };
    Decision::act(Actor::Unit(unit.id), Reason::AttackCity, command)
}

fn objective_kind(state: &AiState, target: TileId) -> Option<ObjectiveKind> {
    state
        .relations()
        .iter()
        .filter(|m| m.at_war)
        .find_map(|m| m.objective.filter(|o| o.tile == target).map(|o| o.kind))
}

/// No enemy city is known yet: walk into the enemy's land towards the edge
/// of what is known of it (the objective follows that edge every turn,
/// and becomes the city once one is seen).
fn search_territory(
    view: &UnitView<'_, '_>,
    tools: &mut PlanTools<'_>,
    target: TileId,
) -> Decision {
    let unit = view.local.unit;
    if unit.tile == target {
        return Decision::refuse(Actor::Unit(unit.id), Reason::Rally);
    }
    move_along(view, tools, target, Reason::Advance)
}

/// The objective tile is in sight and holds no foreign city any more
/// (captured — maybe by us — or gone).
fn objective_gone(view: &UnitView<'_, '_>, target: TileId) -> bool {
    match view.local.view.tile(target) {
        crate::knowledge::view::KnownTile::Visible(snapshot) => {
            !(snapshot.city && snapshot.owner.is_some_and(|o| o != view.local.player))
        }
        _ => false,
    }
}

/// The assault task: march to the objective, gather the group within
/// reach, then siege. A visible objective tile without an enemy city ends
/// the task (captured or gone).
pub fn assault_step(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
    target: TileId,
) -> Decision {
    let unit = view.local.unit;
    if objective_kind(state, target) == Some(ObjectiveKind::Territory) {
        return search_territory(view, tools, target);
    }
    if objective_gone(view, target) {
        state.clear_task(unit.id);
        return Decision::refuse(Actor::Unit(unit.id), Reason::TargetGone);
    }
    let city = view.local.foreign_cities.iter().find(|c| c.tile == target);
    match city {
        Some(city) => siege_step(view, state, tools, *city),
        None => approach(view, tools, target, Reason::Advance),
    }
}

/// The objective city is in sight: strike from a neighbour, wait for the
/// group within 3 tiles (unless the city is weak or the wait is long), or
/// come closer.
fn siege_step(
    view: &UnitView<'_, '_>,
    state: &AiState,
    tools: &mut PlanTools<'_>,
    city: ObservedCity,
) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    let target = city.tile;
    let distance = view
        .local
        .view
        .dimensions()
        .distance(unit.tile, target)
        .unwrap_or(i32::MAX);
    if let Some(range) = crate::ai::combat::ranged_range(tools.rules, unit.kind) {
        // Ranged units bombard from their range and come no closer.
        if distance <= range {
            return ranged_step(view, tools).unwrap_or(Decision::refuse(actor, Reason::Rally));
        }
        return approach(view, tools, target, Reason::Advance);
    }
    if distance == 1 {
        if unit.hp >= ATTACK_MIN_HP && unit.can_attack {
            return attack_city(view, city);
        }
        return Decision::refuse(actor, Reason::Rally);
    }
    let waited = state
        .task_of(unit.id)
        .map(|t| view.brief.round.saturating_sub(t.since_round))
        .unwrap_or(0);
    let gathered = group_near(view, target) + 1 >= SIEGE_GROUP;
    if distance <= 3 && !gathered && city.hp > WEAK_CITY_HP && waited < RALLY_ROUNDS {
        return Decision::refuse(actor, Reason::Rally);
    }
    approach(view, tools, target, Reason::Advance)
}
