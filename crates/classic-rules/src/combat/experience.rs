//! Experience, levels, promotions and great general points
//! (classic/docs/classic/rules.md).

use crate::effects::unit::promotion_fits;
use crate::ids::{PlayerId, PromotionId};
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;
use crate::world::unit::UnitCore;
use crate::world::player::ClassicPlayers;

/// Experience step of the promotion ladder (10, 30, 60, 100, ...). Unciv:
/// `map/mapunit/UnitPromotions.kt` (`xpForNextPromotion`); equal.
pub const EXPERIENCE_PER_LEVEL: u32 = 10;
/// Experience per combat: melee 5 to the attacker and 4 to the defender
/// (attacking a city too), ranged 2 and 2 (3 against a city), air strikes 4
/// and 2. Unciv: `battle/Battle.kt` (`addXp` after an attack); equal.
pub const EXPERIENCE_ATTACKING_UNIT_MELEE: u32 = 5;
/// Unciv: battle/Battle.kt (`addXp`), equal.
pub const EXPERIENCE_DEFENDING_UNIT_MELEE: u32 = 4;
/// Unciv: battle/Battle.kt (`addXp`), equal.
pub const EXPERIENCE_ATTACKING_CITY_MELEE: u32 = 5;
/// Unciv: battle/Battle.kt (`addXp`), equal.
pub const EXPERIENCE_ATTACKING_UNIT_RANGED: u32 = 2;
/// Unciv: battle/Battle.kt (`addXp`), equal.
pub const EXPERIENCE_DEFENDING_UNIT_RANGED: u32 = 2;
/// Unciv: battle/Battle.kt (`addXp`), equal.
pub const EXPERIENCE_ATTACKING_CITY_RANGED: u32 = 3;
/// Unciv: battle/Battle.kt (`addXp`), equal.
pub const EXPERIENCE_ATTACKING_UNIT_AIR: u32 = 4;
/// Unciv: battle/Battle.kt (`addXp`), equal.
pub const EXPERIENCE_DEFENDING_UNIT_AIR: u32 = 2;
/// Unciv: battle/Battle.kt (`addXp`), equal.
pub const EXPERIENCE_ATTACKING_CITY_AIR: u32 = 4;
/// Experience of an interceptor that hits an air strike. Unciv:
/// `battle/AirInterception.kt` (2 when it deals damage); equal.
pub const EXPERIENCE_DEFENDING_AIR_SWEEP_GROUND: u32 = 2;
/// Experience of both fighters of an air sweep dogfight. Unciv:
/// `battle/AirInterception.kt` (5 to both); equal.
pub const EXPERIENCE_ATTACKING_AIR_SWEEP: u32 = 5;
/// Experience from fighting barbarians stops here. Unciv: `ModConstants.kt`
/// (`maxXPfromBarbarians`, 30); equal.
pub const MAX_XP_FROM_BARBARIANS: u32 = 30;
/// Hit points restored by the instant-heal promotion. Unciv: `UnitPromotions.json`
/// (Heal Instantly, 50 HP); equal.
pub const INSTANT_HEAL_HIT_POINTS: u16 = 50;
/// Experience points for the first great general and the rise of that threshold
/// after each one (great admirals use the same values). Unciv:
/// `GreatPersonManager.kt` (`pointsForNextGreatGeneral` 200, +50); equal.
pub const POINTS_FOR_NEXT_GREAT_GENERAL: u32 = 200;
/// Unciv: GreatPersonManager.kt (+50), equal.
pub const POINTS_FOR_NEXT_GREAT_GENERAL_INCREASE: u32 = 50;

/// Total experience for the next promotion at `level`: 10, 30, 60, ...
pub fn experience_needed(level: u8) -> u32 {
    let l = u32::from(level.max(1));
    EXPERIENCE_PER_LEVEL * l * (l + 1) / 2
}

/// The promotions the unit may choose now (choosable, its role allowed,
/// prerequisites held, technology known, not taken yet).
pub fn available_promotions(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: &UnitCore,
) -> Vec<PromotionId> {
    let techs = auth.players().techs_of(unit.owner);
    let mut out = Vec::new();
    for (i, def) in rules.promotions.iter().enumerate() {
        let id = PromotionId(i as u16);
        if !def.choosable || unit.promotions.contains(id) {
            continue;
        }
        // A promotion is chosen only by the unit
        // combat types (or civilian units) it lists; one listing none
        // (Air Recon, cargo) is granted, never chosen.
        let listed = !def.unit_roles.is_empty() || def.civilian_units.contains(&unit.kind);
        if !listed || !promotion_fits(rules, id, unit.kind) {
            continue;
        }
        if !def.requires.iter().all(|p| unit.promotions.contains(*p)) {
            continue;
        }
        let any_ok = def.requires_any_of.is_empty()
            || def
                .requires_any_of
                .iter()
                .any(|p| unit.promotions.contains(*p));
        if !any_ok {
            continue;
        }
        if def.required_tech.is_some_and(|t| !techs.contains(&t)) {
            continue;
        }
        out.push(id);
    }
    out
}

/// Experience the unit needs for its next promotion: `experience_needed`
/// changed by its experience-needed percent (the Zulu: -25 %).
pub fn experience_needed_by(auth: &AuthWorld, rules: &CompiledRuleset, unit: &UnitCore) -> u32 {
    use crate::classic::effects::ClassicUnitStat;
    use crate::rules::effects::UnitStat;
    let base = experience_needed(unit.level);
    let percent = crate::effects::unit::unit_modifiers(auth, rules, unit)
        .stat(UnitStat::Module(ClassicUnitStat::ExperienceNeededPercent));
    let scaled = i64::from(base) * i64::from(100 + percent.max(-100)) / 100;
    scaled.max(1) as u32
}

/// May the unit take a promotion now (enough experience and a choice)?
pub fn promotion_ready(auth: &AuthWorld, rules: &CompiledRuleset, unit: &UnitCore) -> bool {
    unit.experience >= experience_needed_by(auth, rules, unit)
        && !available_promotions(auth, rules, unit).is_empty()
}

/// Experience a unit gets from one combat (§6.2):
/// barbarians get none; against barbarians at most up to
/// `MAX_XP_FROM_BARBARIANS`; `percent` is the unit's experience percent.
pub fn experience_gain(base: u32, current: u32, percent: i32, against_barbarian: bool) -> u32 {
    let mut xp = base;
    if percent != 0 {
        xp = (i64::from(xp) * i64::from((100 + percent).max(0)) / 100) as u32;
    }
    if against_barbarian {
        if current >= MAX_XP_FROM_BARBARIANS {
            return 0;
        }
        return xp.min(MAX_XP_FROM_BARBARIANS - current);
    }
    xp
}

/// Threshold of the next great general (or admiral) of a player who had
/// `born` of them (Standard speed): 200, 250, 300, ... Unciv:
/// GreatPersonManager.getNewGreatPerson (+50 flat per one born), equal.
pub fn great_general_threshold(born: u16) -> u32 {
    POINTS_FOR_NEXT_GREAT_GENERAL + POINTS_FOR_NEXT_GREAT_GENERAL_INCREASE * u32::from(born)
}

/// Is `player` a barbarian (no experience, no great generals)?
pub fn is_barbarian(auth: &AuthWorld, player: PlayerId) -> bool {
    auth.players().is_barbarian(player)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_the_rules_table() {
        // classic/docs/classic/rules.md.
        let needed: Vec<u32> = (1..=8).map(experience_needed).collect();
        assert_eq!(needed, [10, 30, 60, 100, 150, 210, 280, 360]);
    }

    #[test]
    fn barbarian_experience_stops_at_thirty() {
        assert_eq!(experience_gain(5, 0, 0, true), 5);
        assert_eq!(experience_gain(5, 28, 0, true), 2);
        assert_eq!(experience_gain(5, 30, 0, true), 0);
        assert_eq!(experience_gain(5, 30, 0, false), 5);
        assert_eq!(experience_gain(4, 0, 50, false), 6);
    }

    #[test]
    fn great_general_thresholds_grow() {
        assert_eq!(great_general_threshold(0), 200);
        // Unciv: GreatPersonManager, 200 + 50 per one born.
        assert_eq!(great_general_threshold(1), 250);
        assert_eq!(great_general_threshold(2), 300);
        assert_eq!(great_general_threshold(12), 800);
    }
}
