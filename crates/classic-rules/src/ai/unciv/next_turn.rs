// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/civilization/NextTurnAutomation.kt (`getUnitPriority`,
// `applyPromotions`, `automateCityConquer`, `automateSettlerEscorting`,
// `automateCityBombardment`), UnitAutomation.tryBombardEnemy and the AI weights of
// android/assets/jsons/Civ V - Gods & Kings/UnitPromotions.json.

//! The civilization-level parts of Unciv's unit turn: the order its units
//! move in, the promotions it takes, its cities' strikes, the assault on a
//! city it surrounds and the escort of its settlers.

use crate::ai::planner::Decision;
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{EntityId, TileId, UnitHandle};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::CompiledRuleset;

use super::battle_helper::{attackable_enemies, move_and_attack};
use super::head_towards::head_towards;
use super::sight::{Kind, TurnSight, Unit};
use super::{Memo, Step};

/// A deterministic draw in `0..n` from the round, a unit and a salt (Unciv's
/// `stateBasedRandom`).
pub fn draw(round: u32, unit: u64, salt: u64, n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    let mut z = (u64::from(round) << 40) ^ unit.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z % n as u64) as usize
}

/// `getUnitPriority` (lower moves first), from the turn's sight.
pub fn unit_priority(rules: &CompiledRuleset, sight: &TurnSight, at_war: bool, unit: &crate::knowledge::observation::OwnUnit) -> i32 {
    let kind = Kind { rules, kind: unit.kind };
    let def = rules.units.get(unit.kind.0 as usize);
    if def.is_some_and(|d| d.role == Some(kernel::rules::content::UnitRole::SpaceshipPart)) {
        return 0;
    }
    if kind.is_civilian() && kind.founds_cities() {
        return 1;
    }
    if kind.is_civilian() && !kind.war_great_person() {
        return 2;
    }
    if kind.is_air() {
        return if kind.can_intercept() {
            2
        } else if kind.is_nuclear() {
            3
        } else if !kind.self_destructs() {
            4
        } else {
            5
        };
    }
    let distance = if !at_war { 0 } else { distance_to_closest_enemy(rules, sight, unit.tile, 6) };
    let class = if kind.is_ranged() {
        10
    } else if kind.is_melee() {
        30
    } else if kind.war_great_person() {
        100
    } else {
        1
    };
    distance + i32::from(unit.hp) / 10 - i32::from(unit.level) + class
}

/// `getDistanceToClosestEnemyUnit(tile, max)` over the turn's sight (the
/// larger value when none: 500 000).
fn distance_to_closest_enemy(rules: &CompiledRuleset, sight: &TurnSight, tile: TileId, max: i32) -> i32 {
    let dims_distance = |a: TileId, b: TileId| sight_distance(sight, a, b);
    let units = sight.foreign_units.iter().filter(|u| sight.at_war(u.owner) && Kind { rules, kind: u.kind }.is_military()).map(|u| u.tile);
    let cities = sight.foreign_cities.iter().filter(|c| sight.at_war(c.owner)).map(|c| c.tile);
    units.chain(cities).map(|t| dims_distance(tile, t)).filter(|d| *d <= max).min().unwrap_or(500_000)
}

fn sight_distance(sight: &TurnSight, a: TileId, b: TileId) -> i32 {
    sight.dims.distance(a, b).unwrap_or(i32::MAX / 4)
}

/// The turn's units by Unciv's priority (stable: the driver's order on ties).
pub fn order_units(rules: &CompiledRuleset, sight: &TurnSight, at_war: bool, order: &mut [(EntityId, UnitHandle)]) {
    let priority = |id: EntityId| sight.own_units.iter().find(|u| u.id == id).map_or(i32::MAX, |u| unit_priority(rules, sight, at_war, u));
    order.sort_by_cached_key(|(id, _)| priority(*id));
}

/// Unciv's AI weight of a promotion (`[x]% weight to this choice for AI
/// decisions` in its Gods & Kings promotions), by the promotion's name.
fn promotion_weight(name: &str) -> f64 {
    match name {
        "Accuracy I" | "Barrage I" | "Shock I" | "Drill I" | "Operational Range" => 0.1,
        "Charge" | "Evasion" => 3.0,
        "Formation I" | "Medic" => 0.5,
        "Siege I" | "Air Targeting I" | "Sortie" | "Cover I" | "Ambush I" => 0.0,
        _ => 1.0,
    }
}

/// `applyPromotions` for every unit that may be promoted: an instant heal for
/// a unit at 60 HP or less, else a draw weighted by Unciv's AI weights.
pub fn promotions(obs: &AIObservation<'_>, rules: &CompiledRuleset, units: impl Fn(&crate::knowledge::observation::OwnUnit) -> bool) -> Vec<Decision> {
    let mut out = Vec::new();
    for unit in obs.own_units.iter().filter(|u| units(u)) {
        let available = obs.view.promotions_available(unit.handle);
        if available.is_empty() {
            continue;
        }
        let def = |p: &crate::ids::PromotionId| &rules.promotions[p.0 as usize];
        let kind = Kind { rules, kind: unit.kind };
        let heals: Vec<_> = available.iter().copied().filter(|p| def(p).heals_instantly()).collect();
        let moves_after_attacking = rules.units.get(unit.kind.0 as usize).is_some_and(|d| d.effects.passive_unit.iter().any(|e| e.modifier == crate::rules::effects::Modifier::GrantAbility(crate::rules::effects::UnitAbility::MoveAfterAttacking)));
        let pool: Vec<_> = if unit.hp <= 60 && !heals.is_empty() && !(kind.is_air() || moves_after_attacking) {
            heals
        } else {
            available.iter().copied().filter(|p| !def(p).heals_instantly()).collect()
        };
        if pool.is_empty() {
            continue;
        }
        let weights: Vec<f64> = pool.iter().map(|p| promotion_weight(&def(p).name)).collect();
        let total: f64 = weights.iter().sum();
        let pick = if total <= 0.0 {
            pool[draw(obs.round, unit.id.0, 0x7072, pool.len())]
        } else {
            let roll = draw(obs.round, unit.id.0, 0x7077, 1 << 20) as f64 / f64::from(1u32 << 20) * total;
            let mut acc = 0.0;
            let mut chosen = pool[pool.len() - 1];
            for (p, w) in pool.iter().zip(&weights) {
                acc += w;
                if roll < acc {
                    chosen = *p;
                    break;
                }
            }
            chosen
        };
        out.push(Decision::act(Actor::Unit(unit.id), Reason::SystemAdvisor, Command::Promote { unit: unit.handle, promotion: pick }));
    }
    out
}

/// `automateCityBombardment` / `tryBombardEnemy`: every city strikes the
/// visible enemy (not a civilian) it hurts most.
pub fn city_bombardment(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Decision> {
    let mut out = Vec::new();
    let at_war = |owner| obs.relation(owner).is_some_and(|r| r.at_war);
    for city in &obs.own_cities {
        let best = obs
            .visible_foreign_units
            .iter()
            .filter(|u| at_war(u.owner) && Kind { rules, kind: u.kind }.is_military())
            .filter_map(|u| obs.view.city_strike_damage(city.handle, u.tile).map(|d| (d, u.tile)))
            .max_by_key(|(d, t)| (*d, std::cmp::Reverse(*t)));
        if let Some((_, target)) = best {
            let strike = Command::CityStrike { city: city.handle, target };
            if obs.view.accepts(&strike) {
                out.push(Decision::act(Actor::Player, Reason::AttackUnit, strike));
            }
        }
    }
    out
}

/// `automateCityConquer` for one unit: around an enemy city where we have more
/// than four military units within 4 tiles, the siege units strike the city
/// first, then the melee units take it when it is low or strike the enemy
/// unit most surrounded by ours.
pub fn city_conquer(u: &Unit<'_, '_>, memo: &mut Memo) -> Step {
    if u.kind.is_air() {
        return Step::Next;
    }
    let rules = u.rules();
    let cities: Vec<super::sight::KnownCity> = u
        .sight
        .foreign_cities
        .iter()
        .copied()
        .filter(|c| u.at_war(c.owner) && c.handle.is_some())
        .filter(|c| u.sight.own_units.iter().filter(|o| Kind { rules, kind: o.kind }.is_military() && !Kind { rules, kind: o.kind }.is_air() && sight_distance(u.sight, o.tile, c.tile) <= 4).count() > 4)
        .collect();
    for city in cities {
        let health = city.hp.unwrap_or(0);
        let fit = !u.kind.is_melee() || u.health() > 30;
        if !fit {
            continue;
        }
        let d = u.distance(u.tile(), city.tile);
        if health != 1 && d <= 4 && u.kind.is_probably_siege() {
            let must = u.local().view.must_set_up_first(u.me.handle);
            if !must {
                if let Some(a) = attackable_enemies(u, Some(&[city.tile]), false).first() {
                    if let Some(decision) = move_and_attack(u, memo, a) {
                        return Step::Act(decision);
                    }
                }
            }
        }
        if d <= 5 && u.kind.is_melee() {
            let max = crate::world::city::CITY_BASE_HIT_POINTS;
            if health < max / 5 {
                if let Some(a) = attackable_enemies(u, Some(&[city.tile]), false).first() {
                    if let Some(decision) = move_and_attack(u, memo, a) {
                        return Step::Act(decision);
                    }
                }
            }
            let near: Vec<TileId> = crate::math::hex_disc(u.dims(), city.tile, 4).collect();
            let attackable = attackable_enemies(u, Some(&near), false);
            let most = attackable.iter().max_by_key(|a| (u.dims().neighbors(a.target).into_iter().filter(|t| u.own_military_on(*t).is_some()).count(), std::cmp::Reverse(a.target), std::cmp::Reverse(a.from)));
            if let Some(a) = most {
                if let Some(decision) = move_and_attack(u, memo, a) {
                    return Step::Act(decision);
                }
            }
        }
    }
    Step::Next
}

/// `automateSettlerEscorting`: the healthiest strong unit within 3 tiles of a
/// settler, from the core of the empire, walks to it.
pub fn settler_escort(u: &Unit<'_, '_>, tools: &mut crate::ai::planner::PlanTools<'_>) -> Step {
    let rules = u.rules();
    let Some(capital) = u.sight.capital else {
        return Step::Next;
    };
    let settlers: Vec<TileId> = u.sight.own_units.iter().filter(|o| Kind { rules, kind: o.kind }.is_civilian() && Kind { rules, kind: o.kind }.founds_cities()).map(|o| o.tile).collect();
    for settler in settlers {
        let mut candidates: Vec<&crate::knowledge::observation::OwnUnit> = u
            .sight
            .own_units
            .iter()
            .filter(|o| {
                let kind = Kind { rules, kind: o.kind };
                kind.is_military() && !kind.is_air() && o.hp >= super::sight::FULL_HP && sight_distance(u.sight, o.tile, settler) <= 3 && sight_distance(u.sight, o.tile, capital) < sight_distance(u.sight, settler, capital)
            })
            .collect();
        candidates.sort_by_key(|o| (sight_distance(u.sight, o.tile, settler), o.id));
        // `maxByOrNull` keeps the first of the strongest.
        let mut best: Option<&crate::knowledge::observation::OwnUnit> = None;
        for c in candidates {
            let strength = Kind { rules, kind: c.kind }.strength();
            if best.is_none_or(|b| strength > Kind { rules, kind: b.kind }.strength()) {
                best = Some(c);
            }
        }
        if best.is_some_and(|b| b.id == u.me.id) {
            return match head_towards(u, tools, settler, Reason::Escort) {
                Step::Done(_) => Step::Next,
                other => other,
            };
        }
    }
    Step::Next
}
