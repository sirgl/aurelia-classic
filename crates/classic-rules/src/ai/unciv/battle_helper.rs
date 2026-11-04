// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/unit/BattleHelper.kt and
// core/src/com/unciv/logic/battle/TargetHelper.kt (`getAttackableEnemies`).

//! Unciv's choice of a blow (`BattleHelper`): every attack the unit could make
//! from a tile it reaches with moves left (`TargetHelper.getAttackableEnemies`),
//! those it survives, valued by Unciv's city and unit attack values, the best
//! one above 30. Unciv's `BattleDamage` is the rules' own combat preview
//! (`PlayerQueries::attack_preview_from`): expected damage both ways.

use crate::ai::planner::{Decision, PlanTools};
use crate::ai::trace::Reason;
use crate::command::Command;
use crate::ids::TileId;
use crate::knowledge::public::PlayerQueries;
use kernel::world::unit::MOVE_DENOMINATOR;

use super::sight::{actor, own_military_near, AttackableTile, Kind, Unit};
use super::Memo;

/// `TargetHelper.getAttackableEnemies(unit, distanceToTiles, tilesToCheck, stayOnTile)`.
pub fn attackable_enemies(u: &Unit<'_, '_>, tiles_to_check: Option<&[TileId]>, stay_on_tile: bool) -> Vec<AttackableTile> {
    let local = u.local();
    let must_set_up = local.view.must_set_up_first(u.me.handle) || u.me.stance == crate::world::unit::Stance::SetUp;
    let set_up_now = u.me.stance == crate::world::unit::Stance::SetUp;
    let den = i32::from(MOVE_DENOMINATOR);
    let mut from: Vec<(TileId, i32)> = if stay_on_tile || u.kind.is_air() {
        vec![(u.tile(), i32::from(u.me.moves_left))]
    } else {
        let mut reached: Vec<(TileId, i32)> = u.reachable().iter().map(|(t, l)| (*t, i32::from(*l))).collect();
        // Sorted by the movement spent getting there (least first).
        reached.sort_by_key(|(t, left)| (std::cmp::Reverse(*left), *t));
        reached
            .into_iter()
            .map(|(tile, left)| {
                let cost = match (must_set_up, tile == u.tile()) {
                    (true, true) if !set_up_now => den,
                    (true, false) => den,
                    _ => 0,
                };
                (tile, left - cost)
            })
            .filter(|(_, left)| *left > 0)
            .filter(|(tile, _)| *tile == u.tile() || u.can_move_to(*tile))
            .collect()
    };
    if u.kind.is_air() {
        from.truncate(1);
    }
    // The tiles holding something attackable the player sees: an enemy unit at
    // war, or an enemy city at war.
    let mut targets: Vec<TileId> = local
        .foreign_units
        .iter()
        .filter(|f| u.at_war(f.owner) && local.view.is_visible(f.tile))
        .map(|f| f.tile)
        .chain(local.foreign_cities.iter().filter(|c| u.at_war(c.owner)).map(|c| c.tile))
        .filter(|t| tiles_to_check.is_none_or(|check| check.contains(t)))
        .collect();
    targets.sort();
    targets.dedup();
    let range = u.kind.range();
    let reach = if u.kind.is_melee() { 1 } else { range };
    let mut out = Vec::new();
    for (tile, left) in from {
        for &target in &targets {
            if target == tile || u.distance(tile, target) > reach {
                continue;
            }
            if let Some(preview) = u.preview_from(tile, target) {
                out.push(AttackableTile { from: tile, target, moves_after: left as f32 / den as f32, preview });
            }
        }
    }
    out
}

/// `BattleHelper.tryAttackNearbyEnemy`: the best blow the unit survives, its
/// order (or the walk to its tile first, `moveAndAttack`). None: no blow worth
/// making.
pub fn try_attack_nearby_enemy(u: &Unit<'_, '_>, memo: &mut Memo, stay_on_tile: bool) -> Option<Decision> {
    if !u.me.can_attack || !u.kind.is_military() {
        return None;
    }
    let enemies: Vec<AttackableTile> = attackable_enemies(u, None, stay_on_tile)
        .into_iter()
        // Only take enemies we can fight without dying or are made to die.
        .filter(|a| u.kind.self_destructs() || (a.preview.to_attacker.1 < u.health() && u.damage_from_terrain(a.from) <= 0))
        .collect();
    let chosen = choose_attack_target(u, &enemies)?;
    let military_there = u.foreign_military_on(chosen.target).is_some();
    let city_there = u.foreign_city_at(chosen.target).is_some();
    if !military_there && !city_there && u.kind.is_ranged() && u.can_move_to(chosen.target) && u.reaches_now(chosen.target) {
        // Ranged units should move to capture a civilian unit instead of attacking it.
        let command = Command::MoveUnit { unit: u.me.handle, destination: chosen.target };
        return Some(Decision::act(actor(&u.me), Reason::AttackUnit, command));
    }
    move_and_attack(u, memo, &chosen)
}

/// `Battle.moveAndAttack`: the walk to the tile to attack from (the blow is
/// remembered for the next decision), or the blow.
pub fn move_and_attack(u: &Unit<'_, '_>, memo: &mut Memo, attack: &AttackableTile) -> Option<Decision> {
    if attack.from != u.tile() {
        memo.pending = Some((attack.from, attack.target));
        let command = Command::MoveUnit { unit: u.me.handle, destination: attack.from };
        return Some(Decision::act(actor(&u.me), Reason::Advance, command));
    }
    memo.pending = None;
    attack_command(u, attack.target)
}

/// The order of a blow from where the unit stands: a shot (a siege unit sets
/// up first), a charge on the city, or a charge on the unit there.
pub fn attack_command(u: &Unit<'_, '_>, target: TileId) -> Option<Decision> {
    let view = &u.local().view;
    let unit = u.me.handle;
    let id = actor(&u.me);
    if u.kind.is_ranged() || u.kind.is_air() {
        if view.must_set_up_first(unit) {
            let set_up = Command::SetUpRangedAttack { unit };
            return view.accepts(&set_up).then(|| Decision::act(id, Reason::AttackUnit, set_up));
        }
        let shot = Command::RangedAttack { unit, target };
        let reason = if u.is_city_tile(target) { Reason::AttackCity } else { Reason::AttackUnit };
        return view.accepts(&shot).then(|| Decision::act(id, reason, shot));
    }
    if let Some(city) = u.local().foreign_cities.iter().find(|c| c.tile == target) {
        let charge = Command::AttackCity { attacker: unit, city: city.handle };
        return view.accepts(&charge).then(|| Decision::act(id, Reason::AttackCity, charge));
    }
    let defender = u.foreign_on(target)?;
    let charge = Command::AttackUnit { attacker: unit, defender: defender.handle };
    view.accepts(&charge).then(|| Decision::act(id, Reason::AttackUnit, charge))
}

/// `BattleHelper.tryDisembarkUnitToAttackPosition`: an embarked melee land unit
/// lands where it can attack next turn.
pub fn try_disembark_unit_to_attack_position(u: &Unit<'_, '_>) -> Option<Decision> {
    if !u.kind.is_melee() || !u.kind.is_land() || !u.embarked() {
        return None;
    }
    let enemies: Vec<AttackableTile> = attackable_enemies(u, None, false)
        .into_iter()
        .filter(|a| a.preview.to_attacker.1 < u.health())
        .filter(|a| u.is_land_tile(a.from))
        .collect();
    let chosen = choose_attack_target(u, &enemies)?;
    if chosen.from == u.tile() {
        return None;
    }
    let command = Command::MoveUnit { unit: u.me.handle, destination: chosen.from };
    Some(Decision::act(actor(&u.me), Reason::Advance, command))
}

/// `BattleHelper.chooseAttackTarget`: the highest attack value, if above 30.
pub fn choose_attack_target(u: &Unit<'_, '_>, attackable: &[AttackableTile]) -> Option<AttackableTile> {
    let mut highest = 0;
    let mut best = None;
    for attack in attackable {
        let value = if let Some(city) = u.foreign_city_at(attack.target) {
            city_attack_value(u, city.tile, city.owner, city.hp.unwrap_or(attack.preview.defender_hp), attack)
        } else {
            unit_attack_value(u, attack)
        };
        if value > highest {
            highest = value;
            best = Some(*attack);
        }
    }
    if highest > 30 {
        best
    } else {
        None
    }
}

/// `BattleHelper.getCityAttackValue`.
fn city_attack_value(u: &Unit<'_, '_>, city: TileId, owner: crate::ids::PlayerId, health: u16, attack: &AttackableTile) -> i32 {
    let view = &u.local().view;
    let can_capture = u.kind.is_melee() && view.captures_cities(u.me.handle);
    if health == 1 {
        return if can_capture { 10_000 } else { 0 };
    }
    if can_capture && health <= attack.preview.uncapped_to_defender.max(1) {
        return 10_000;
    }
    if u.kind.is_melee() {
        let damage = i32::from(attack.preview.to_attacker.1);
        let hp = i32::from(u.health());
        if hp - damage * 2 <= 0 && !u.kind.self_destructs() {
            // The more friendly units around the city, the more willing we should be to just attack it.
            let friendly = own_military_near(u, city, 3).len() as i32;
            if friendly < 5 {
                let modifier = 1.0 + 1.0 / f64::from(friendly.max(1));
                if f64::from(hp) - f64::from(damage) * modifier <= 0.0 {
                    return 0;
                }
            }
        }
    }
    let mut value = 100;
    if u.kind.is_probably_siege() {
        value += 100;
    } else if u.kind.is_ranged() {
        value += 10;
    }
    value -= (i32::from(health) - 60) / 2;
    // Units around the city: the enemy's against us, ours (at war with the city's owner) for us.
    let rules = u.rules();
    for f in &u.local().foreign_units {
        if u.distance(f.tile, city) <= 2 && (Kind { rules, kind: f.kind }).is_military() && u.at_war(f.owner) {
            value -= 5;
        }
    }
    let ours = own_military_near(u, city, 2).len() as i32;
    if u.at_war(owner) {
        value += 15 * ours;
    }
    value
}

/// `BattleHelper.getUnitAttackValue`.
fn unit_attack_value(u: &Unit<'_, '_>, attack: &AttackableTile) -> i32 {
    let rules = u.rules();
    let mut value = i32::MIN / 2;
    if let Some(military) = u.foreign_military_on(attack.target) {
        value = 200 - i32::from(military.hp) + i32::from(attack.preview.to_defender.1);
    } else if let Some(civilian) = u.foreign_on(attack.target) {
        value = 50;
        let kind = Kind { rules, kind: civilian.kind };
        if u.kind.is_melee() || u.reaches_now(attack.target) {
            if kind.great_person() {
                value += 150;
            }
            if kind.founds_cities() {
                value += 60;
            }
        } else if u.kind.is_ranged() {
            return 10; // Don't shoot civilians that we can capture!
        }
    }
    value + (attack.moves_after * 5.0) as i32
}

/// `UnitAutomation.tryAttacking`'s blow, tried once a decision (the unit's
/// next decision tries again while it has attacks left).
pub fn try_attacking(u: &Unit<'_, '_>, memo: &mut Memo, _tools: &mut PlanTools<'_>) -> Option<Decision> {
    try_attack_nearby_enemy(u, memo, false)
}
