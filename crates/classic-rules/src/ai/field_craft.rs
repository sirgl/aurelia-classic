//! Field craft (`Variant::FieldCraft`; the tactics exam, the M4 lab's exam/REPORT.md):
//! what a few units do in the field that the game's AI left out. Each rule is a class, written from the
//! rules' numbers; nothing here names a nation, a unit, a building or an improvement, and everything
//! reads what the unit's seat sees.
//!
//! The sortie: a military unit at war that the empire does not need where it is (a spare garrison, a
//! wounded unit, a unit with no task) pillages the enemy's improvements within a short walk when the
//! pillage pays: the enemy's yields lost until a worker restores the tile and the gold of the pillage
//! (`raid::improvement_value`, the allocator's prices) and the hit points the pillage heals
//! (`PILLAGE_HEAL_HP`, priced as the unit's cost) against the unit's expected loss where it would stand
//! (`combat::Exposure`: the enemy units and the cities that strike that tile, the chance of its
//! death times its worth). A city's fire is therefore paid for out of the heal and the damage, not
//! forbidden by a ring around it.

use crate::ai::combat::Exposure;
use crate::ai::danger::Threats;
use crate::ai::planner::{Decision, PlanTools, UnitView};
use crate::ai::planner_units::{move_along, spare_garrison};
use crate::ai::state::{AiState, UnitTask};
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::public::PlayerQueries;
use crate::systems::combat::actions::PILLAGE_HEAL_HP;
use crate::world::unit::UNIT_MAX_HP;

/// Rounds of walking a sortie looks ahead for prey: a trip that takes longer is the raid's
/// (`raid::MAX_ARRIVAL_ROUNDS` is the raid's own limit, a war lasts a few rounds).
pub const SORTIE_ROUNDS: i32 = 3;
/// Prey tried a turn, nearest first: the way to one may not exist (the sea, a border) and the
/// nearest few may all lie across it.
pub const SORTIE_TRIES: usize = 8;

/// The unit stands on the land of a nation at war with us: it is out on a sortie.
pub fn abroad(view: &UnitView<'_, '_>) -> bool {
    let tile = view.local.unit.tile;
    view.local.view.tile(tile).snapshot().is_some_and(|s| s.owner.is_some_and(|o| view.local.relation(o).is_some_and(|r| r.at_war)))
}

/// A friendly unit that depends on this one: a civilian of ours, or a ranged unit of ours, outside a city within
/// one tile of the unit that has no other friendly melee unit on or next to its tile. Leaving it would expose it
/// (a civilian is captured, a ranged unit is struck in melee), so the unit stays. With `danger` the ward counts only
/// when a hostile fighter is within that many tiles of it (a ward at peace at home is not left exposed).
pub fn has_dependant(view: &UnitView<'_, '_>, rules: &crate::rules::CompiledRuleset, danger: Option<i32>) -> bool {
    let me = view.local.unit;
    let dims = view.local.view.dimensions();
    let role = |k| crate::ai::planner::role_of(rules, k);
    let melee = |u: &kernel::ai::observation::OwnUnit| role(u.kind) == crate::ai::planner::AiRole::Military && crate::ai::combat::ranged_range(rules, u.kind).is_none();
    view.local.own_units.iter().filter(|w| w.id != me.id).any(|w| {
        let weak = !matches!(role(w.kind), crate::ai::planner::AiRole::Military | crate::ai::planner::AiRole::Naval) || (role(w.kind) == crate::ai::planner::AiRole::Military && !melee(w));
        weak && dims.distance(w.tile, me.tile).is_some_and(|d| d <= 1)
            && danger.is_none_or(|radius| crate::ai::planner_units::threatened(view, rules, w.tile, radius))
            && !view.brief.cities.iter().any(|c| c.tile == w.tile)
            && !view.local.own_units.iter().any(|o| o.id != me.id && o.id != w.id && melee(o) && dims.distance(o.tile, w.tile).is_some_and(|d| d <= 1))
    })
}

/// The sortie of a free military unit, if one pays.
pub fn sortie(view: &UnitView<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Option<Decision> {
    if !view.brief.field_craft {
        return None;
    }
    let unit = view.local.unit;
    let rules = tools.rules;
    if unit.moves_left == 0 || !unit.can_attack || !rules.units[unit.kind.0 as usize].combat.pillage {
        return None;
    }
    let prices_known = view.brief.prices.is_some();
    let enemies: Vec<PlayerId> = view.brief.relations.iter().filter(|r| r.at_war && r.kind == crate::world::player::PlayerKind::Major).map(|r| r.other).collect();
    if enemies.is_empty() || !prices_known {
        return None;
    }
    // A unit with a task of the empire's keeps it; a spare garrison, a wounded unit and an idle one are
    // free, and so is a unit already out on the enemy's land (its garrison task was given when its moves
    // ran out; it is on a sortie).
    let abroad = abroad(view);
    let free = match state.task_of(unit.id).map(|t| t.task) {
        None | Some(UnitTask::Heal { .. }) => true,
        Some(UnitTask::Defend { city }) => abroad || spare_garrison(view, state, city),
        // The task has the unit wait (`decide_military` asks only then): the muster of an operation
        // still forming, an assault gathering its group.
        Some(UnitTask::Assault { .. }) => true,
        Some(UnitTask::Operation { target }) => state.operation(target).is_some_and(|o| o.phase == crate::ai::state::OperationPhase::Forming),
        Some(_) => false,
    };
    if !free || has_dependant(view, rules, None) {
        return None;
    }
    trip(view, tools, SORTIE_ROUNDS)
}

/// The pillage trip of a unit that is free to go (the freedom is the caller's), looking `rounds` of its walking
/// for prey: to the nearest prey that pays, or the pillage where it stands.
pub(crate) fn trip(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, rounds: i32) -> Option<Decision> {
    let unit = view.local.unit;
    let rules = tools.rules;
    if unit.moves_left == 0 || !unit.can_attack || !rules.units[unit.kind.0 as usize].combat.pillage {
        return None;
    }
    let prices = view.brief.prices.as_ref()?;
    let enemies: Vec<PlayerId> = view.brief.relations.iter().filter(|r| r.at_war && r.kind == crate::world::player::PlayerKind::Major).map(|r| r.other).collect();
    if enemies.is_empty() {
        return None;
    }
    let dims = view.local.view.dimensions();
        let reach = i32::from(rules.units[unit.kind.0 as usize].movement.max(1)) * rounds;
    let mut prey: Vec<(i32, TileId, crate::ids::ImprovementId)> = Vec::new();
    for tile in crate::math::tiles_in_hex_radius(&dims, unit.tile, reach) {
        let Some(snapshot) = view.local.view.tile(tile).snapshot().copied() else { continue };
        if !snapshot.owner.is_some_and(|o| enemies.contains(&o)) {
            continue;
        }
        if snapshot.city || view.local.view.known_pillaged(tile) {
            continue;
        }
        let Some(improvement) = snapshot.improvement else { continue };
        let Some(def) = rules.improvements.get(improvement.0 as usize) else { continue };
        if matches!(def.kind, crate::rules::content::ImprovementKind::AncientRuins) || def.kind == crate::classic::content::BARBARIAN_CAMP {
            continue;
        }
        // A tile an enemy fighter stands on is not prey; the fight is the tactics'.
        if view.local.foreign_units.iter().any(|f| f.tile == tile && view.local.relation(f.owner).is_some_and(|r| r.at_war)) {
            continue;
        }
        prey.push((dims.distance(unit.tile, tile).unwrap_or(i32::MAX), tile, improvement));
    }
    prey.sort_by_key(|(d, t, _)| (*d, *t));
    // What the unit is worth: its cost in the allocator's hammers.
    let price = i64::from(rules.units[unit.kind.0 as usize].cost) * prices.production / i64::from(prices.rounds_left.max(1));
    // The threats are read with the prey tiles as places the unit may be: the cities and the fighters
    // that strike there are modelled even when none is in the unit's sight yet (`unit_threats` would
    // see nothing), the remembered cities among them.
    let mut movers: Vec<(TileId, i32)> = vec![(unit.tile, crate::ai::combat::steps_left(&view.local.view, &unit))];
    movers.extend(prey.iter().take(SORTIE_TRIES).map(|(_, t, _)| (*t, 0)));
    let threats = view.brief.danger.as_ref().map(|sight| Threats::around(view.local, rules, view.brief, sight, &movers));
    let exposure = threats.as_ref().map(|t| Exposure::of(t, rules, view.brief.fight.source, &unit));
    let healed = |hp: u16| (hp.saturating_add(PILLAGE_HEAL_HP)).min(UNIT_MAX_HP);
    // The trip: the nearest prey, up to what one unit pillages in a raid (`raid::PILLAGES_PER_RAIDER`).
    let gain: (i64, i64) = prey.iter().take(crate::ai::raid::PILLAGES_PER_RAIDER).map(|(_, _, i)| crate::ai::raid::improvement_value(prices, rules, *i)).fold((0, 0), |(d, g), (vd, vg)| (d + vd, g + vg));
    let heal = price * i64::from(healed(unit.hp) - unit.hp) / i64::from(UNIT_MAX_HP);
    // Risk: the raid's floor for a trip (`raid::LOSS_MIN_PERCENT` of the unit's price) and the
    // chance of death where it would pillage times the price (the damage is not priced: a hurt unit
    // rests and returns, a dead one does not). The unit pillages with the hit points it will have
    // after the pillage (the heal comes with it).
    let scored: Vec<(i64, i32, TileId)> = prey
        .iter()
        .take(SORTIE_TRIES)
        .map(|(d, tile, _)| {
            let danger = exposure.as_ref().map(|e| e.danger(*tile, healed(unit.hp), false, &[]));
            let death = danger.map_or(0, |d| i64::from(d.death_permille));
            (price * crate::ai::raid::LOSS_MIN_PERCENT / 100 + price * death / 1000, *d, *tile)
        })
        .collect();
    for (risk, _, tile) in scored {
        if gain.0 + gain.1 + heal <= risk {
            continue;
        }
        let actor = Actor::Unit(unit.id);
        if tile == unit.tile {
            let command = Command::Pillage { unit: unit.handle };
            if view.local.view.accepts(&command) {
                return Some(Decision::act(actor, Reason::Pillage, command));
            }
            continue;
        }
        let step = move_along(view, tools, tile, Reason::Raid);
        if step.command.is_some() {
            return Some(step);
        }
    }
    None
}


// ---- Guarding ----

/// Hit points a unit needs to count as a guard of a tile: below the health of the wounded
/// (`planner_units::HEAL_BELOW_HP`) it is one more thing to capture.
pub const GUARD_MIN_HP: u16 = crate::ai::planner_units::HEAL_BELOW_HP;

/// A military unit of ours with hit points to guard stands on `tile` besides the asked one.
pub fn guarded(view: &UnitView<'_, '_>, rules: &crate::rules::CompiledRuleset, tile: TileId) -> bool {
    let me = view.local.unit.id;
    view.local.own_units.iter().any(|u| u.id != me && u.tile == tile && u.hp >= GUARD_MIN_HP && crate::ai::planner::role_of(rules, u.kind) == crate::ai::planner::AiRole::Military)
}

/// Tiles from which a unit of `kind` strikes: what it walks and its reach.
fn strike_reach(rules: &crate::rules::CompiledRuleset, kind: crate::ids::UnitKindId) -> i32 {
    i32::from(rules.units[kind.0 as usize].movement.max(1)) + crate::ai::combat::ranged_range(rules, kind).unwrap_or(1)
}

// ---- Taking ground ----

/// Rounds a hostile unit needs to stand next to `tile`: the distance less one over its walk.
fn foe_rounds(rules: &crate::rules::CompiledRuleset, dims: &crate::math::MapDimensions, kind: crate::ids::UnitKindId, at: TileId, tile: TileId) -> i32 {
    let walk = i32::from(rules.units[kind.0 as usize].movement.max(1));
    ((dims.distance(at, tile).unwrap_or(i32::MAX / 2) - 1).max(0) + walk - 1) / walk
}

/// A military unit outside a city with a hostile fighter coming (within four of its turns) takes the
/// best defensive ground it reaches before the fighter does and holds it, and a ranged unit stands beside
/// or behind the nearest melee unit of ours with its back to the fighters: the defence of a tile is the
/// rules' own number (`siege_tactics::ground_percent`: hills, features, improvements), so the class is
/// every ground the rules make a better place to be struck on. The attack of the moment is the tactics'
/// (`tactics::immediate` asks first); this decides only where the unit waits for the blow.
pub fn stance(view: &UnitView<'_, '_>, state: &AiState, tools: &mut PlanTools<'_>) -> Option<Decision> {
    if !view.brief.field_craft {
        return None;
    }
    let unit = view.local.unit;
    let rules = tools.rules;
    if unit.moves_left == 0 || view.brief.cities.iter().any(|c| c.tile == unit.tile) {
        return None;
    }
    // Tasks that a standing fight does not outrank: the empire's own marches and raids.
    if !matches!(state.task_of(unit.id).map(|t| t.task), None | Some(UnitTask::Defend { .. }) | Some(UnitTask::Assault { .. })) {
        return None;
    }
    let dims = view.local.view.dimensions();
    let owner = view.local.player;
    let foes: Vec<(TileId, crate::ids::UnitKindId)> = view
        .local
        .foreign_units
        .iter()
        .filter(|f| view.local.relation(f.owner).is_some_and(|r| r.at_war) && crate::ai::planner::role_of(rules, f.kind) == crate::ai::planner::AiRole::Military)
        .filter(|f| dims.distance(f.tile, unit.tile).is_some_and(|d| d <= 4 * strike_reach(rules, f.kind)))
        .map(|f| (f.tile, f.kind))
        .collect();
    if foes.is_empty() {
        return None;
    }
    let ground = |t: TileId| crate::ai::siege_tactics::ground_percent(&view.local.view, rules, t, owner);
    let blocked = |t: TileId| {
        view.local.foreign_units.iter().any(|f| f.tile == t)
            || view.brief.cities.iter().any(|c| c.tile == t)
            || view.local.foreign_cities.iter().any(|c| c.tile == t)
            || view.local.own_units.iter().any(|u| u.id != unit.id && u.tile == t)
    };
    let walk = i32::from(rules.units[unit.kind.0 as usize].movement.max(1));
    let nearest_foe = |t: TileId| foes.iter().map(|(at, _)| dims.distance(*at, t).unwrap_or(i32::MAX / 2)).min().unwrap_or(i32::MAX / 2);
    let ranged = crate::ai::combat::ranged_range(rules, unit.kind).is_some();
    // Candidates, best first: (rank key, tile).
    let mut options: Vec<((i64, i64, i32), TileId)> = Vec::new();
    if ranged {
        // Beside or behind the nearest melee unit of ours: next to it, as far from the fighters as the
        // ground allows (capped at their reach, past which distance buys nothing), then the best ground.
        let anchor = view
            .local
            .own_units
            .iter()
            .filter(|u| u.id != unit.id && u.hp >= GUARD_MIN_HP && crate::ai::planner::role_of(rules, u.kind) == crate::ai::planner::AiRole::Military && crate::ai::combat::ranged_range(rules, u.kind).is_none())
            .filter(|u| dims.distance(unit.tile, u.tile).is_some_and(|d| d <= 3 * walk))
            .min_by_key(|u| (dims.distance(unit.tile, u.tile).unwrap_or(i32::MAX), u.id.0))?;
        for t in crate::math::tiles_in_hex_radius(&dims, unit.tile, walk) {
            if (t != unit.tile && blocked(t)) || dims.distance(t, anchor.tile).unwrap_or(i32::MAX) != 1 {
                continue;
            }
            if view.local.view.tile(t).snapshot().is_none() {
                continue;
            }
            options.push(((-i64::from(nearest_foe(t).min(3)), -ground(t), dims.distance(unit.tile, t).unwrap_or(0)), t));
        }
    } else {
        // Ground reached before any fighter stands next to it (one of its turns to spare is not needed:
        // the unit settles on arrival and fortifying takes effect with its first full turn there).
        for t in crate::math::tiles_in_hex_radius(&dims, unit.tile, 2 * walk) {
            if (t != unit.tile && blocked(t)) || view.local.view.tile(t).snapshot().is_none() {
                continue;
            }
            let own = (dims.distance(unit.tile, t).unwrap_or(i32::MAX / 2) + walk - 1) / walk;
            let theirs = foes.iter().map(|(at, k)| foe_rounds(rules, &dims, *k, *at, t)).min().unwrap_or(i32::MAX);
            if t != unit.tile && own >= theirs {
                continue;
            }
            options.push(((-ground(t), 0, dims.distance(unit.tile, t).unwrap_or(0)), t));
        }
    }
    options.sort();
    let here = options.iter().position(|(_, t)| *t == unit.tile);
    let best_key = options.first()?.0;
    if here.is_some_and(|i| options[i].0 == best_key) {
        return Some(crate::ai::planner_units::hold(view, Reason::Escort));
    }
    if ranged && here.is_none() && options.is_empty() {
        return None;
    }
    for (key, t) in options {
        if t == unit.tile {
            return Some(crate::ai::planner_units::hold(view, Reason::Escort));
        }
        let _ = key;
        let step = move_along(view, tools, t, Reason::Escort);
        if step.command.is_some() {
            return Some(step);
        }
    }
    None
}
