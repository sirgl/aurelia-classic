// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/unit/AirUnitAutomation.kt (`automateFighter`,
// `automateBomber`, `automateMissile`, `automateNukes`, `getNukeLocationValue`,
// `tryRelocateToCitiesWithEnemyNearBy`).

//! Unciv's air arm: a wounded aircraft waits and heals; a bomber or a missile
//! strikes the best target in range (Unciv's `BattleHelper` from where it
//! stands), else rebases to an own city with an enemy in range; a fighter
//! does the same and otherwise stands to intercept (the classic rules' stance
//! for Unciv's automatic interception); a nuclear weapon is used only at war
//! on the tile whose blast is worth more than 500 and touches no neutral.

use crate::ai::planner::{Decision, PlanTools};
use crate::ai::trace::Reason;
use crate::command::Command;
use crate::ids::TileId;
use crate::knowledge::public::PlayerQueries;

use super::battle_helper::try_attack_nearby_enemy;
use super::sight::{actor, Kind, Unit};
use super::Memo;

/// The air unit's decision.
pub fn automate_air(u: &Unit<'_, '_>, memo: &mut Memo, tools: &mut PlanTools<'_>) -> Decision {
    let _ = tools;
    let done = |reason| Decision::refuse(actor(&u.me), reason);
    if u.kind.is_nuclear() {
        return automate_nuke(u).unwrap_or_else(|| relocate(u).unwrap_or_else(|| done(Reason::Idle)));
    }
    if u.health() < 75 {
        return done(Reason::Heal);
    }
    if let Some(d) = try_attack_nearby_enemy(u, memo, true) {
        return d;
    }
    let at_war = u.sight.is_at_war(&u.local().relations);
    if !u.kind.can_intercept() && (u.health() <= 90 || (u.health() < 100 && !at_war)) {
        return done(Reason::Heal);
    }
    if let Some(d) = relocate(u) {
        return d;
    }
    if u.kind.can_intercept() {
        let intercept = Command::Intercept { unit: u.me.handle };
        if u.me.stance != crate::world::unit::Stance::Intercept && u.local().view.accepts(&intercept) {
            return Decision::act(actor(&u.me), Reason::Fortify, intercept);
        }
    }
    done(Reason::Idle)
}

/// `tryRelocateToCitiesWithEnemyNearBy`: to an own city it reaches with an
/// enemy in its range from there.
fn relocate(u: &Unit<'_, '_>) -> Option<Decision> {
    let range = u.kind.range();
    let view = &u.local().view;
    let mut cities: Vec<&super::sight::OwnCitySight> = u.sight.own_cities.iter().filter(|c| c.tile != u.tile()).collect();
    cities.sort_by_key(|c| (u.distance(c.tile, u.tile()), c.tile));
    for city in cities {
        let enemy_near = u.sight.foreign_units.iter().any(|f| u.at_war(f.owner) && u.distance(f.tile, city.tile) <= range && Kind { rules: u.rules(), kind: f.kind }.is_military())
            || u.sight.foreign_cities.iter().any(|c| u.at_war(c.owner) && u.distance(c.tile, city.tile) <= range);
        if !enemy_near {
            continue;
        }
        let command = Command::Rebase { unit: u.me.handle, city: city.handle };
        if view.accepts(&command) {
            return Some(Decision::act(actor(&u.me), Reason::Advance, command));
        }
    }
    None
}

/// `automateNukes` / `getNukeLocationValue` over what the player sees.
fn automate_nuke(u: &Unit<'_, '_>) -> Option<Decision> {
    if !u.sight.is_at_war(&u.local().relations) {
        return None;
    }
    let range = u.kind.range();
    let dims = u.dims();
    let rules = u.rules();
    let me = u.local().player;
    let blast = rules.units.get(u.me.kind.0 as usize).map_or(1, |d| i32::from(d.combat.nuke_level.max(1)));
    let mut best: Option<(i32, TileId)> = None;
    for target in crate::math::hex_disc(dims, u.tile(), range).filter(|t| u.distance(*t, u.tile()) >= 2) {
        let command = Command::Nuke { unit: u.me.handle, target };
        if !u.local().view.accepts(&command) {
            continue;
        }
        let tiles: Vec<TileId> = crate::math::hex_disc(dims, target, blast).collect();
        let owners: Vec<crate::ids::PlayerId> = tiles.iter().filter_map(|t| u.owner_of(*t)).chain(u.local().foreign_units.iter().filter(|f| tiles.contains(&f.tile)).map(|f| f.owner)).collect();
        if owners.iter().any(|o| *o != me && !u.at_war(*o)) || !owners.iter().any(|o| u.at_war(*o)) {
            continue;
        }
        let mut value = -500;
        for t in &tiles {
            let ground = *t == target;
            for f in u.local().foreign_units.iter().filter(|f| f.tile == *t) {
                let military = Kind { rules, kind: f.kind }.is_military();
                value += if military { if ground { 80 } else { 50 } } else { 25 };
            }
            for o in u.own_units().iter().filter(|o| o.tile == *t) {
                value -= if (Kind { rules, kind: o.kind }).is_military() { if ground { 200 } else { 150 } } else { 100 };
            }
            if u.is_city_tile(*t) {
                value += if u.owner_of(*t) == Some(me) { -100_000 } else { 250 };
            } else if let Some(owner) = u.owner_of(*t) {
                value += if owner == me { -40 } else { 10 };
            }
        }
        if best.is_none_or(|(b, _)| value > b) {
            best = Some((value, target));
        }
    }
    let (value, target) = best?;
    (value > 0).then(|| Decision::act(actor(&u.me), Reason::AttackCity, Command::Nuke { unit: u.me.handle, target }))
}
