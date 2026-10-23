//! Healing of units (classic/docs/classic/rules.md): at the end of the
//! owner's turn, before the moves are restored. A unit heals when it did
//! not spend moves this turn (or has AlwaysHeal); embarked units and
//! barbarians never heal; ships heal only in friendly land (or with
//! HealOutsideFriendly). The kernel heals by this amount
//! (`ModuleSystems::heal_amount`).

use crate::classic::effects::ClassicCondition;
use crate::combat::terrain::is_friendly_territory;
use crate::navigation::terrain::is_water;
use crate::effects::unit::{own_stat, unit_modifiers};
use crate::ids::{PlayerId, TileId};
use crate::rules::content::Domain;
use crate::classic::religion_effects::near_following_city;
use crate::rules::effects::{Condition, UnitAbility, UnitStat};
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;
use crate::world::unit::{UnitCore, UNIT_MAX_HP};
use crate::world::player::ClassicPlayers;

/// Hit points a resting unit heals per turn in a city (classic/docs/classic/rules.md). Unciv: MapUnit.rankTileForHealing, 25; equal.
pub const HEALING_IN_CITY_CENTER: i32 = 25;
/// Heal per turn on own land, or land of a player granting open borders.
/// Unciv: MapUnit.rankTileForHealing, 20; equal.
pub const HEALING_FRIENDLY_TERRITORY: i32 = 20;
/// Heal per turn on unowned land or land of a player not at war. Unciv:
/// MapUnit.rankTileForHealing, 10; equal.
pub const HEALING_NEUTRAL_TERRITORY: i32 = 10;
/// Heal per turn on land of a player at war. Unciv:
/// MapUnit.rankTileForHealing, 10; equal.
pub const HEALING_ENEMY_TERRITORY: i32 = 10;

/// HP the unit regains at the end of its owner's turn (0 when it does not
/// heal).
pub fn heal_amount(auth: &AuthWorld, rules: &CompiledRuleset, unit: &UnitCore) -> i32 {
    if unit.hp >= UNIT_MAX_HP || auth.players().is_barbarian(unit.owner) {
        return 0;
    }
    let def = rules.units.get(unit.kind.0 as usize);
    let domain = def.map(|d| d.domain).unwrap_or_default();
    if domain == Domain::Air && !in_city(auth, unit) {
        return 0;
    }
    if domain == Domain::Land && is_water(auth.map(), unit.tile) {
        return 0;
    }
    let mods = unit_modifiers(auth, rules, unit);
    if unit.has_moved() && !mods.has(UnitAbility::AlwaysHeal) {
        return 0;
    }
    let friendly = is_friendly_territory(auth, unit.owner, unit.tile);
    if domain == Domain::Sea && !friendly && !mods.has(UnitAbility::HealOutsideFriendly) {
        return 0;
    }
    heal_rate(auth, rules, unit, &mods)
}

fn in_city(auth: &AuthWorld, unit: &UnitCore) -> bool {
    auth.indexes().city_on_tile(unit.tile).is_some()
}

/// Heal rate of a unit on its tile (§5.2): base by place, the unit's place bonus,
/// the best heal of a neighbouring medic.
fn heal_rate(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: &UnitCore,
    mods: &crate::effects::unit::UnitModifiers,
) -> i32 {
    let tile = unit.tile;
    let host = auth.map().owner(tile).flatten();
    // Beliefs heal near a city following the owner's creed (Faith Healers).
    let here = |condition: Condition| match condition {
        Condition::Module(ClassicCondition::NearFollowingCity(relation)) => near_following_city(auth, unit.owner, tile, relation),
        _ => false,
    };
    let friendly_extra = mods.stat_where(UnitStat::HealFriendlyLand, here);
    let neutral_extra = mods.stat_where(UnitStat::HealNeutralLand, here);
    let (base, extra) = if in_city(auth, unit) {
        let own = host == Some(unit.owner);
        (
            HEALING_IN_CITY_CENTER,
            if own { friendly_extra } else { neutral_extra },
        )
    } else if !is_friendly_territory(auth, unit.owner, tile) {
        match host {
            Some(h) if auth.diplomacy().at_war(unit.owner, h) => {
                (HEALING_ENEMY_TERRITORY, mods.stat_where(UnitStat::HealEnemyLand, here))
            }
            _ => (HEALING_NEUTRAL_TERRITORY, neutral_extra),
        }
    } else {
        (HEALING_FRIENDLY_TERRITORY, friendly_extra)
    };
    base + extra + medic_bonus(auth, rules, unit.owner, tile)
}

/// The best AdjacentTileHeal of the owner's units on the six neighbours.
fn medic_bonus(auth: &AuthWorld, rules: &CompiledRuleset, owner: PlayerId, tile: TileId) -> i32 {
    let mut best = 0;
    for near in auth.map().dimensions.neighbors(tile) {
        for u in auth.indexes().units_on(near) {
            let Some(view) = auth.units().get(*u) else {
                continue;
            };
            if view.core.owner == owner {
                best = best.max(own_stat(rules, view.core, UnitStat::HealAdjacent));
            }
        }
    }
    best
}
