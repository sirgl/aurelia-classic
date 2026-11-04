//! Damage formulas (classic/docs/classic/rules.md).
//!
//! Strengths are in hundredths (`Base × (100 + M)`), damage in hundredths of
//! HP until the final division. The strength ratio factor
//!
//! ```text
//! r = max(S_me, S_opp) / min(S_me, S_opp)
//! k = (((r + 3) / 4)^4 + 1) / 2        (1 / k when S_opp > S_me)
//! ```
//!
//! is evaluated exactly as a fraction of integers (i128), not with `f64::powf`:
//! the result is the floor of the exact value on every platform (Unciv
//! evaluates it in floating point; the two agree except where the float
//! rounds across an integer, see `ratio_matches_the_float_formula`).
//!
//! Source: Unciv `BattleDamage.kt` (`damageModifier`: the ratio factor above
//! and a base of 24 + 12 × random, i.e. the 2400 and 1200 below in hundredths;
//! wounded units deal 1 % less per 3 missing hit points, air units and the
//! answer to an air strike included, cities never weaken; the damage is
//! rounded to the nearest hit point with no minimum, `roundToInt`;
//! `BattleConstants.kt`: `DAMAGE_TO_CIVILIAN_UNIT` 40), checked 2026-10-21.

use crate::world::unit::UNIT_MAX_HP;

/// Unciv: logic/map/mapunit/MapUnit.kt (health 100), equal.
pub const MAX_HIT_POINTS: i64 = UNIT_MAX_HP as i64;
/// Damage at equal strength, `24 + 12 × random`, in hundredths (melee and
/// ranged alike). Unciv: `BattleDamage.kt` (`damageModifier`); equal.
pub const MELEE_EQUAL_STRENGTH_BASE_DAMAGE: i64 = 2400;
/// Unciv: battle/BattleDamage.kt, equal.
pub const MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE: u64 = 1200;
/// Unciv: battle/BattleDamage.kt, equal.
pub const RANGED_EQUAL_STRENGTH_BASE_DAMAGE: i64 = 2400;
/// Unciv: battle/BattleDamage.kt, equal.
pub const RANGED_EQUAL_STRENGTH_RANDOM_DAMAGE: u64 = 1200;
/// A wounded striker deals this percent less per missing hit point (1 % per
/// 3 HP). Unciv: `BattleConstants.kt` (`DAMAGE_REDUCTION_WOUNDED_UNIT_RATIO_PERCENTAGE`
/// 300, i.e. 1/3); equal up to the integer percent (33 for 33.3).
pub const WOUNDED_DAMAGE_PERCENT: i64 = 33;
/// Factor (percent) of a city's damage to a unit and of melee damage to a
/// city. Unciv: `BattleDamage.kt` (no such factor, i.e. 100); equal.
pub const CITY_ATTACK_DAMAGE_PERCENT: i64 = 100;
/// Unciv: battle/BattleDamage.kt, equal.
pub const MELEE_VS_CITY_DAMAGE_PERCENT: i64 = 100;
/// Damage to a civilian that is attacked. Unciv: `BattleConstants.kt`
/// (`DAMAGE_TO_CIVILIAN_UNIT` 40); equal.
pub const DAMAGE_TO_CIVILIAN_UNIT: u16 = 40;

/// Hundredths of HP to whole HP, rounded to the nearest (Unciv `roundToInt`).
fn to_hit_points(dmg: i64) -> u16 {
    ((dmg.max(0) + 50) / 100).min(i64::from(u16::MAX)) as u16
}

/// The random part of a strike: a draw from `[0, 1200)` of the combat
/// stream, or the expected value the interface shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Roll {
    Drawn(u64),
    Expected,
}

impl Roll {
    /// The random addition after the wounded reduction `dr` (percent of
    /// MAX_HIT_POINTS): `rand × DR / 100`, or `(1200 − 1) × DR / 100 / 2`.
    fn part(self, extra: u64, dr: i64) -> i64 {
        match self {
            Roll::Drawn(r) => r as i64 * dr / MAX_HIT_POINTS,
            Roll::Expected => (extra as i64 - 1) * dr / MAX_HIT_POINTS / 2,
        }
    }
}

/// `floor(dmg × k(S_me, S_opp))` (strengths > 0, dmg ≥ 0).
pub fn scale_by_ratio(dmg: i64, me: i64, opp: i64) -> i64 {
    let me = i128::from(me.max(1));
    let opp = i128::from(opp.max(1));
    let dmg = i128::from(dmg.max(0));
    let (num, den) = if opp > me {
        // k = 2 (4 me)^4 / ((opp + 3 me)^4 + (4 me)^4)
        let a = (4 * me).pow(4);
        (2 * a, (opp + 3 * me).pow(4) + a)
    } else {
        // k = ((me + 3 opp)^4 + (4 opp)^4) / (2 (4 opp)^4)
        let a = (4 * opp).pow(4);
        ((me + 3 * opp).pow(4) + a, 2 * a)
    };
    (dmg * num / den) as i64
}

/// Damage reduction of a wounded striker, percent: `100 − damage × W / 100`
/// where W = WOUNDED_DAMAGE_PERCENT + `wounded_mod` (Japan: −33).
pub fn wounded_factor(damage: u16, wounded_mod: i64) -> i64 {
    let w = (WOUNDED_DAMAGE_PERCENT + wounded_mod).max(0);
    MAX_HIT_POINTS - i64::from(damage) * w / 100
}

/// Who strikes in a melee exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeleeSide {
    UnitVsUnit,
    UnitVsCity,
    CityVsUnit,
}

/// Melee damage `me` deals to `opp` (§2.3). `my_damage` is the striker's
/// own damage (ignored for a city: a city never weakens).
pub fn melee_damage(
    me: i64,
    opp: i64,
    my_damage: u16,
    wounded_mod: i64,
    roll: Roll,
    side: MeleeSide,
) -> u16 {
    let dr = match side {
        MeleeSide::CityVsUnit => MAX_HIT_POINTS,
        MeleeSide::UnitVsUnit | MeleeSide::UnitVsCity => wounded_factor(my_damage, wounded_mod),
    };
    let base = MELEE_EQUAL_STRENGTH_BASE_DAMAGE * dr / MAX_HIT_POINTS;
    let dmg = base + roll.part(MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE, dr);
    let mut dmg = scale_by_ratio(dmg, me, opp);
    match side {
        MeleeSide::CityVsUnit => dmg = dmg * CITY_ATTACK_DAMAGE_PERCENT / 100,
        MeleeSide::UnitVsCity => dmg = dmg * MELEE_VS_CITY_DAMAGE_PERCENT / 100,
        MeleeSide::UnitVsUnit => {}
    }
    to_hit_points(dmg)
}

/// Ranged damage of a unit (§2.5), before the target's HP cap.
pub fn ranged_damage(me: i64, opp: i64, my_damage: u16, wounded_mod: i64, roll: Roll) -> u16 {
    let dr = wounded_factor(my_damage, wounded_mod);
    let base = RANGED_EQUAL_STRENGTH_BASE_DAMAGE * dr / MAX_HIT_POINTS;
    let dmg = base + roll.part(RANGED_EQUAL_STRENGTH_RANDOM_DAMAGE, dr);
    to_hit_points(scale_by_ratio(dmg, me, opp))
}

/// A city's ranged strike on a unit (§2.6): no wounded reduction.
pub fn city_strike_damage(city: i64, opp: i64, roll: Roll) -> u16 {
    let base = RANGED_EQUAL_STRENGTH_BASE_DAMAGE;
    let dmg = base
        + roll.part(
            RANGED_EQUAL_STRENGTH_RANDOM_DAMAGE,
            MAX_HIT_POINTS,
        );
    to_hit_points(scale_by_ratio(dmg, city, opp))
}

/// Air strike damage (§2.7): the wounded reduction of every unit, counting
/// the interception damage taken just before.
pub fn air_damage(me: i64, opp: i64, my_damage: u16, interception: u16, roll: Roll) -> u16 {
    let dr = wounded_factor(my_damage.saturating_add(interception).min(UNIT_MAX_HP), 0);
    let base = RANGED_EQUAL_STRENGTH_BASE_DAMAGE * dr / MAX_HIT_POINTS;
    let dmg = base + roll.part(RANGED_EQUAL_STRENGTH_RANDOM_DAMAGE, dr);
    to_hit_points(scale_by_ratio(dmg, me, opp))
}

/// Damage an interceptor deals to an air attacker (§2.7).
pub fn interception_damage(
    interceptor: i64,
    attacker: i64,
    interceptor_damage: u16,
    taken_percent: i64,
    roll: Roll,
) -> u16 {
    let dr = MAX_HIT_POINTS - i64::from(interceptor_damage);
    let base = MELEE_EQUAL_STRENGTH_BASE_DAMAGE * dr / MAX_HIT_POINTS;
    let dmg = base + roll.part(MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE, dr);
    let dmg = to_hit_points(scale_by_ratio(dmg, interceptor, attacker));
    // Unciv: the rounded damage times the factor, truncated (AirInterception.kt).
    (i64::from(dmg) * (100 + taken_percent).max(0) / 100) as u16
}

/// Damage of the target's answer to an air strike (§2.7): a unit target
/// (`city` false) deals less when wounded (its `damage`), a city never
/// weakens.
pub fn air_defense_damage(target: i64, attacker: i64, damage: u16, city: bool, roll: Roll) -> u16 {
    let dr = if city { MAX_HIT_POINTS } else { wounded_factor(damage.min(UNIT_MAX_HP), 0) };
    let base = MELEE_EQUAL_STRENGTH_BASE_DAMAGE * dr / MAX_HIT_POINTS;
    let dmg = base + roll.part(MELEE_EQUAL_STRENGTH_RANDOM_DAMAGE, dr);
    to_hit_points(scale_by_ratio(dmg, target, attacker))
}

/// The mutual-kill rule of melee (§2.3): when both would die, the side that
/// is "overkilled" more dies and the other keeps 1 HP; on a tie the
/// defender survives. Returns the adjusted (to defender, to attacker) damage.
pub fn resolve_mutual_kill(
    to_defender: u16,
    defender_hp: u16,
    to_attacker: u16,
    attacker_hp: u16,
) -> (u16, u16) {
    let max = MAX_HIT_POINTS as u16;
    let defender_total = to_defender.saturating_add(max - defender_hp);
    let attacker_total = to_attacker.saturating_add(max - attacker_hp);
    if defender_total < max || attacker_total < max {
        return (to_defender, to_attacker);
    }
    if defender_total > attacker_total {
        (to_defender, attacker_hp - 1)
    } else {
        (defender_hp - 1, to_attacker)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The float formula (as in Unciv's `damageModifier`; oracle for the exact fraction).
    fn float_ratio(dmg: i64, me: i64, opp: i64) -> i64 {
        let (me, opp) = (me as f64, opp as f64);
        let r = if opp > me { opp / me } else { me / opp };
        let mut k = ((r + 3.0) / 4.0).powf(4.0);
        k = (k + 1.0) / 2.0;
        if opp > me {
            k = 1.0 / k;
        }
        (dmg as f64 * k) as i64
    }

    #[test]
    fn ratio_matches_the_float_formula() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut mismatches = 0;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let me = 100 + (seed % 20_000) as i64;
            let opp = 100 + ((seed >> 20) % 20_000) as i64;
            let dmg = (seed >> 40) as i64 % 3600;
            let exact = scale_by_ratio(dmg, me, opp);
            let float = float_ratio(dmg, me, opp);
            assert!(
                (exact - float).abs() <= 1,
                "{dmg} {me} {opp}: {exact} vs {float}"
            );
            if exact != float {
                mismatches += 1;
            }
        }
        assert!(
            mismatches < 20,
            "the float rounds across integers rarely: {mismatches}"
        );
    }

    #[test]
    fn damage_table_of_the_rules() {
        // R, min, max, expected at full HP: Unciv BattleDamage.damageModifier,
        // (24 + 12 r) x k(R) rounded to the nearest (roundToInt), r = 0, 1199/1200, 1/2.
        let rows = [
            (1, 3, 8, 12, 10),
            (1, 2, 14, 21, 17),
            (2, 3, 18, 28, 23),
            (1, 1, 24, 36, 30),
            (5, 4, 27, 41, 34),
            (3, 2, 31, 47, 39),
            (2, 1, 41, 62, 52),
            (3, 1, 73, 109, 91),
            (4, 1, 125, 187, 156),
        ];
        for (num, den, min, max, expected) in rows {
            let (me, opp) = (num * 1200, den * 1200);
            let side = MeleeSide::UnitVsUnit;
            assert_eq!(
                melee_damage(me, opp, 0, 0, Roll::Drawn(0), side),
                min,
                "R={num}/{den} min"
            );
            assert_eq!(
                melee_damage(me, opp, 0, 0, Roll::Drawn(1199), side),
                max,
                "R={num}/{den} max"
            );
            assert_eq!(
                melee_damage(me, opp, 0, 0, Roll::Expected, side),
                expected,
                "R={num}/{den}"
            );
        }
    }

    #[test]
    fn worked_example_of_the_rules() {
        // §2.4: swordsman 1400 vs fortified spearman in a forest 1595 (Unciv's rounding: 27.6 -> 28).
        assert_eq!(
            melee_damage(1400, 1595, 0, 0, Roll::Expected, MeleeSide::UnitVsUnit),
            28
        );
        assert_eq!(
            melee_damage(1595, 1400, 0, 0, Roll::Expected, MeleeSide::UnitVsUnit),
            32
        );
        // A unit with 60 damage strikes at 81 %.
        assert_eq!(wounded_factor(60, 0), 81);
        // §4.5: city strike 1482 on a warrior 800: 38..57, expected 47 (Unciv's rounding).
        assert_eq!(city_strike_damage(1482, 800, Roll::Drawn(0)), 38);
        assert_eq!(city_strike_damage(1482, 800, Roll::Drawn(1199)), 57);
        assert_eq!(city_strike_damage(1482, 800, Roll::Expected), 47);
    }

    #[test]
    fn mutual_kill_keeps_the_less_overkilled_side() {
        // Both die: defender takes 60 on 40 HP (total 120), attacker 50 on 30 (total 120): tie → defender lives.
        assert_eq!(resolve_mutual_kill(60, 40, 50, 30), (39, 50));
        // Defender overkilled more: attacker survives with 1 HP.
        assert_eq!(resolve_mutual_kill(90, 40, 50, 30), (90, 29));
        // Only one dies: unchanged.
        assert_eq!(resolve_mutual_kill(30, 40, 50, 30), (30, 50));
    }
}
