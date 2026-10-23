//! Unit upgrades for gold (classic/docs/classic/rules.md) and obsolescence. The upgraded unit keeps its damage, level,
//! experience and promotions (but those LostWithUpgrade), gains the new
//! kind's free promotions and ends its turn.

use crate::command::CommandError;
use crate::ids::{UnitHandle, UnitKindId};
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::state::AuthWorld;
use crate::world::unit::UnitCore;

/// Gold price of an upgrade: this base plus `UNIT_UPGRADE_COST_PER_PRODUCTION`
/// per point of production cost the new unit has over the old, times the
/// owner's upgrade cost modifiers, rounded down to 5 (no era term, exponent 1,
/// standard speed). Unciv: `models/ModConstants.kt` (`unitUpgradeCost`: base 10,
/// perProduction 2, eraMultiplier 0, exponent 1, roundTo 5),
/// `map/mapunit/UnitUpgradeManager.kt`; equal.
pub const BASE_UNIT_UPGRADE_COST: u32 = 10;
/// Unciv: models/ModConstants.kt (`unitUpgradeCost`), equal.
pub const UNIT_UPGRADE_COST_PER_PRODUCTION: u32 = 2;

/// What an upgrade costs and gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpgradeOffer {
    pub to: UnitKindId,
    pub price: u32,
}

/// The unit the player's civilization gets for `kind`'s upgrade (its unique
/// variant of the class when it has one).
pub fn upgrade_target(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: &UnitCore,
) -> Option<UnitKindId> {
    let def = rules.units.get(unit.kind.0 as usize)?;
    let next = *def.upgrades_to.first()?;
    let civ = auth.players().civ_of(unit.owner);
    let key = &rules.units.get(next.0 as usize)?.key;
    rules.civ_variant(civ, key).or(Some(next))
}

/// Can `unit` upgrade now, and for how much? Err: the reason.
pub fn upgrade_offer(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: &UnitCore,
) -> Result<UpgradeOffer, &'static str> {
    let to = upgrade_target(auth, rules, unit).ok_or("no upgrade")?;
    let new = &rules.units[to.0 as usize];
    if let Some(tech) = new.required_tech {
        if !auth.players().techs_of(unit.owner).contains(&tech) {
            return Err("upgrade not researched");
        }
    }
    if auth.map().owner(unit.tile).flatten() != Some(unit.owner) {
        return Err("upgrade only in own territory");
    }
    if unit.moves_left == 0 {
        return Err("no moves left");
    }
    if !upgrade_resources_free(auth, rules, unit, to) {
        return Err("upgrade lacks a strategic resource");
    }
    Ok(UpgradeOffer {
        to,
        price: upgrade_price(auth, rules, unit, to),
    })
}

/// The new kind's resources are free, counting what the old unit holds
/// (available + in use by the old unit ≥ needed by the new one).
fn upgrade_resources_free(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: &UnitCore,
    to: crate::ids::UnitKindId,
) -> bool {
    let needs = &rules.units[to.0 as usize].required_resources;
    if needs.is_empty() {
        return true;
    }
    let held = rules
        .units
        .get(unit.kind.0 as usize)
        .map(|d| d.required_resources.as_slice())
        .unwrap_or(&[]);
    let ledger = crate::derived::resources::resource_ledger(auth, rules, unit.owner);
    needs.iter().all(|(resource, amount)| {
        let freed: u16 = held.iter().filter(|(r, _)| r == resource).map(|(_, n)| *n).sum();
        ledger.available(*resource) + i32::from(freed) >= i32::from(*amount)
    })
}

/// `10 + max(0, cost(new) − cost(old)) × 2`, × the owner's upgrade cost
/// percent, rounded down to 5.
pub fn upgrade_price(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: &UnitCore,
    to: UnitKindId,
) -> u32 {
    let old = rules
        .units
        .get(unit.kind.0 as usize)
        .map(|d| d.cost)
        .unwrap_or(0);
    let new = rules.units.get(to.0 as usize).map(|d| d.cost).unwrap_or(0);
    let base = BASE_UNIT_UPGRADE_COST + new.saturating_sub(old) * UNIT_UPGRADE_COST_PER_PRODUCTION;
    let mods = crate::effects::player::player_modifiers(auth, rules, unit.owner);
    let price = mods
        .upgrade_cost
        .evaluate(base as i32, crate::rules::modifiers::Caps::at_least(0)) as u32;
    price / 5 * 5
}

/// Is the unit kind obsolete for `owner` (its obsolete technology known)?
pub fn is_obsolete(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    owner: crate::ids::PlayerId,
    kind: UnitKindId,
) -> bool {
    let Some(def) = rules.units.get(kind.0 as usize) else {
        return false;
    };
    def.obsolete_tech
        .is_some_and(|t| auth.players().techs_of(owner).contains(&t))
}

impl<'a> WorldTransaction<'a> {
    pub(crate) fn upgrade_unit(
        &mut self,
        unit: UnitHandle,
        to: UnitKindId,
        price: u32,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let old = *self
            .world
            .units
            .get(unit)
            .ok_or(CommandError::UnknownUnit)?
            .core;
        if !self.world.players.spend(old.owner, price) {
            return Err(CommandError::RuleViolation("not enough gold".into()));
        }
        // An aircraft upgraded on its carrier stays on it.
        let carrier = crate::systems::cargo::carrier_of(self.world, unit);
        self.destroy_unit(unit)?;
        let new = self.spawn_unit(old.owner, old.tile, to, rules);
        self.set_carrier(new, carrier);
        let core = self
            .world
            .units
            .get_mut_core(new)
            .ok_or(CommandError::UnknownUnit)?;
        core.hp = old.hp;
        core.experience = old.experience;
        core.level = old.level;
        core.moves_left = 0;
        for promotion in old.promotions.iter() {
            let lost = rules
                .promotions
                .get(promotion.0 as usize)
                .is_some_and(|d| d.lost_with_upgrade);
            if !lost {
                core.promotions.insert(promotion);
            }
        }
        Ok(())
    }
}
