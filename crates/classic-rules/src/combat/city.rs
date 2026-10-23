//! Cities in combat (classic/docs/classic/rules.md): strength, hit points,
//! healing, the ranged strike; the capital and the era of a player.

use crate::world::city::garrison;
use crate::effects::{visit_owner_sources, ConditionContext};
use crate::ids::{CityHandle, PlayerId};
use crate::rules::compiled::EraDefinition;
use crate::rules::content::BuildingKind;
use crate::rules::effects::{CityCombatStat, CompiledEffect, EffectSet, Modifier, Selector};
use crate::rules::CompiledRuleset;
use crate::world::city::{CityCore, CITY_BASE_HIT_POINTS};
use crate::world::state::AuthWorld;

/// Base strength of a city, ×100. Unciv: `ModConstants.kt` (`cityStrengthBase`
/// 8.0); equal.
pub const CITY_STRENGTH_BASE: i64 = 800;
/// Strength per citizen, ×100. Unciv: `ModConstants.kt` (`cityStrengthPerPop`
/// 0.4); equal.
pub const CITY_STRENGTH_PER_POP: i64 = 40;
/// A garrison adds its strength × its health / this (a fifth). Unciv:
/// `ModConstants.kt` (`cityStrengthFromGarrison` 0.2); equal.
pub const CITY_STRENGTH_FROM_GARRISON_DIVISOR: i64 = 500;
/// Strength of a city on hills, ×100. Unciv: `Terrains.json` (Hill, +5 Strength
/// for cities built on it); equal.
pub const CITY_HILL_STRENGTH: i64 = 500;
/// Hit points a city heals per turn. Unciv: `CityTurnManager.kt` (+20 at
/// the end of the turn, nothing else); equal.
pub const CITY_HEAL_PER_TURN: i64 = 20;
/// A city's ranged strike is this percent of its strength. Unciv:
/// `CityCombatant.kt` (`getAttackingStrength` × 0.75); equal.
pub const CITY_RANGED_STRENGTH_PERCENT: i64 = 75;
/// Range of a city's strike. Unciv: `ModConstants.kt` (`baseCityBombardRange`
/// 2); equal.
pub const CITY_ATTACK_RANGE: i32 = 2;
/// A captured city keeps at most this percent of its hit points. Unciv:
/// `CityConquestFunctions.kt` (health = max / 2); equal.
pub const CITY_CAPTURE_DAMAGE_PERCENT: u16 = 50;

/// `(p / 100 × 5.5) ^ 2.8 × 100` rounded to the nearest hundredth
/// (`int(t + 0.005)`), for p = percent of the technologies known. Unciv:
/// `ModConstants.kt` (`cityStrengthFromTechsMultiplier` 5.5, `...Exponent` 2.8);
/// equal. A table, not `powf`: bit-exact on every platform
/// (`tech_table_matches_formula` checks it).
pub static CITY_TECH_STRENGTH: [i64; 101] = [
    0, 0, 0, 0, 1, 2, 4, 6, 10, 13, 18, 24, 31, 39, 48, 58, 69, 82, 97, 113, 130, 149, 170, 193,
    217, 243, 272, 302, 335, 369, 406, 445, 486, 530, 576, 625, 677, 731, 787, 847, 909, 974, 1042,
    1113, 1187, 1264, 1345, 1428, 1515, 1605, 1698, 1795, 1895, 1999, 2107, 2218, 2333, 2451, 2574,
    2700, 2830, 2964, 3102, 3244, 3390, 3541, 3696, 3855, 4018, 4185, 4358, 4534, 4715, 4901, 5091,
    5286, 5486, 5691, 5900, 6114, 6333, 6558, 6787, 7021, 7261, 7505, 7755, 8010, 8271, 8537, 8808,
    9085, 9367, 9655, 9948, 10248, 10552, 10863, 11180, 11502, 11830,
];

/// Combat properties a city gets from its buildings and the owner's
/// sources (`CityCombatStat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CityCombatMods {
    pub defense: i64,
    pub max_hit_points: i64,
    pub defense_percent: i64,
    pub garrison_strike_percent: i64,
    pub nuke_damage_percent: i64,
    pub air_capacity: i64,
    pub ranged_strike_percent: i64,
}

impl CityCombatMods {
    fn add(&mut self, stat: CityCombatStat, amount: i64) {
        match stat {
            CityCombatStat::Defense => self.defense += amount,
            CityCombatStat::MaxHitPoints => self.max_hit_points += amount,
            CityCombatStat::DefensePercent => self.defense_percent += amount,
            CityCombatStat::GarrisonRangedStrikePercent => self.garrison_strike_percent += amount,
            CityCombatStat::NukeDamagePercent => self.nuke_damage_percent += amount,
            CityCombatStat::AirCapacity => self.air_capacity += amount,
            CityCombatStat::RangedStrikePercent => self.ranged_strike_percent += amount,
        }
    }

    /// Building defense after the defense percent.
    pub fn modified_defense(&self) -> i64 {
        self.defense * (100 + self.defense_percent) / 100
    }
}

fn city_bucket(set: &EffectSet) -> &[CompiledEffect] {
    &set.passive_city
}

/// The city's combat modifiers: its own buildings (SourceCity) and the
/// owner-wide sources (OwnerCities).
pub fn city_combat_mods(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city: &CityCore,
) -> CityCombatMods {
    let mut mods = CityCombatMods::default();
    let at = ConditionContext {
        owner: city.owner,
        city: Some(city),
        tile: None,
        unit: None,
        producing: None,
    };
    let mut add = |effect: &CompiledEffect, times: u32| {
        if let Modifier::ModifyCityCombat { stat, amount } = effect.modifier {
            if crate::effects::effect_holds(auth, rules, at, effect) {
                mods.add(stat, i64::from(amount) * i64::from(times));
            }
        }
    };
    visit_owner_sources(
        auth,
        rules,
        city.owner,
        city_bucket,
        &mut |effect, times| {
            if effect.selector == Selector::OwnerCities {
                add(effect, times);
            }
        },
    );
    for building in &city.buildings {
        let Some(def) = rules.buildings.get(building.0 as usize) else {
            continue;
        };
        for effect in &def.effects.passive_city {
            if effect.selector == Selector::SourceCity {
                add(effect, 1);
            }
        }
    }
    // The beliefs of the city's majority (Goddess of Protection).
    crate::classic::religion_effects::visit_city_beliefs(auth, rules, city, city_bucket, &mut add);
    mods
}

/// Maximum hit points: 200 + the buildings' extra hit points.
pub fn max_hit_points(auth: &AuthWorld, rules: &CompiledRuleset, city: &CityCore) -> u16 {
    let extra = city_combat_mods(auth, rules, city).max_hit_points;
    (i64::from(CITY_BASE_HIT_POINTS) + extra).clamp(1, i64::from(u16::MAX)) as u16
}

/// Current hit points.
pub fn hit_points(auth: &AuthWorld, rules: &CompiledRuleset, city: &CityCore) -> u16 {
    max_hit_points(auth, rules, city).saturating_sub(city.damage)
}


/// City strength in hundredths (§4.1).
pub fn city_strength(auth: &AuthWorld, rules: &CompiledRuleset, city: &CityCore) -> i64 {
    let mods = city_combat_mods(auth, rules, city);
    let mut s =
        CITY_STRENGTH_BASE + i64::from(city.population) * CITY_STRENGTH_PER_POP;
    s += mods.modified_defense();
    if let Some(unit) = garrison(auth, rules, city) {
        if let Some(view) = auth.units().get(unit) {
            let combat = rules
                .units
                .get(view.core.kind.0 as usize)
                .map(|d| i64::from(d.strength))
                .unwrap_or(i64::from(crate::rules::compiled::DEFAULT_STRENGTH));
            let g = combat * 100 * i64::from(view.core.hp) / 100;
            s += g * 100 / CITY_STRENGTH_FROM_GARRISON_DIVISOR;
        }
    }
    s += tech_strength(auth, rules, city.owner);
    if crate::navigation::terrain::is_hills(auth.map(), city.tile) {
        s += CITY_HILL_STRENGTH;
    }
    s.max(1)
}

/// The technology part of city strength for `owner`.
pub fn tech_strength(auth: &AuthWorld, rules: &CompiledRuleset, owner: PlayerId) -> i64 {
    let total = rules.technologies.len().max(1);
    let known = auth.players().techs_of(owner).len().min(total);
    CITY_TECH_STRENGTH[known * 100 / total]
}

/// Strength of the city's ranged strike (§4.3): the whole city strength
/// (buildings included, Unciv `CityCombatant.getAttackingStrength`) ×75 %,
/// the garrisoned city bonus of the owner's policies, then the majority's
/// beliefs.
pub fn city_ranged_strength(auth: &AuthWorld, rules: &CompiledRuleset, city: &CityCore) -> i64 {
    let mods = city_combat_mods(auth, rules, city);
    let mut v = city_strength(auth, rules, city);
    v = v * CITY_RANGED_STRENGTH_PERCENT / 100;
    if garrison(auth, rules, city).is_some() && mods.garrison_strike_percent != 0 {
        v = v * (100 + mods.garrison_strike_percent) / 100;
    }
    if mods.ranged_strike_percent > 0 {
        v = v * (100 + mods.ranged_strike_percent) / 100;
    }
    v.max(1)
}

/// Hit points a city heals at the start of its owner's turn (§4.2).
pub fn heal_per_turn(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    city: &CityCore,
    handle: CityHandle,
) -> u16 {
    // Unciv heals every city by a flat 20 (no capital or walls term).
    let _ = (auth, rules, city, handle);
    CITY_HEAL_PER_TURN.clamp(0, i64::from(u16::MAX)) as u16
}

/// The owner's capital: its city with a palace, else its original capital,
/// else its first city.
pub fn capital_of(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    owner: PlayerId,
) -> Option<CityHandle> {
    let cities = auth.indexes().cities_of(owner);
    let palace = cities.iter().copied().find(|c| {
        auth.cities().get(*c).is_some_and(|core| {
            core.buildings.iter().any(|b| {
                rules
                    .buildings
                    .get(b.0 as usize)
                    .is_some_and(|d| d.kind == BuildingKind::Palace)
            })
        })
    });
    if palace.is_some() {
        return palace;
    }
    let original = cities.iter().copied().find(|c| {
        auth.cities()
            .get(*c)
            .is_some_and(|core| core.original_capital && core.original_owner == owner)
    });
    original.or_else(|| cities.first().copied())
}

/// The player's current era: the latest era of a technology it knows,
/// else the first era.
pub fn current_era<'r>(
    auth: &AuthWorld,
    rules: &'r CompiledRuleset,
    owner: PlayerId,
) -> Option<&'r EraDefinition> {
    let mut best = rules.eras.iter().min_by_key(|e| e.index)?;
    for tech in auth.players().techs_of(owner) {
        let era = rules
            .technologies
            .get(tech.0 as usize)
            .and_then(|t| t.content_era)
            .and_then(|e| rules.eras.get(e.0 as usize));
        if let Some(era) = era {
            if era.index > best.index {
                best = era;
            }
        }
    }
    Some(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tech_table_matches_formula() {
        for (p, &actual) in CITY_TECH_STRENGTH.iter().enumerate() {
            let t = (p as f64 / 100.0 * 5.5).powf(2.8) * 100.0;
            assert_eq!(actual, (t + 0.005) as i64, "p={p}");
        }
        // classic/docs/classic/rules.md: 10 / 20 / 40 / 81 of 81 techs.
        assert_eq!(CITY_TECH_STRENGTH[10 * 100 / 81], 31);
        assert_eq!(CITY_TECH_STRENGTH[20 * 100 / 81], 217);
        assert_eq!(CITY_TECH_STRENGTH[40 * 100 / 81], 1605);
        assert_eq!(CITY_TECH_STRENGTH[100], 11830);
    }
}
