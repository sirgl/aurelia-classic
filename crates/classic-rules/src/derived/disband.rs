//! Disbanding a unit (read-only): whether the player may disband it now and
//! the gold its treasury gets (`Command::DisbandUnit`, `systems::disband`).
//!
//! ```text
//! may      the unit is alive and has moves left (Unciv: UiOnlyUnitActions.getDisbandAction,
//!          the action only `takeIf { unit.hasMovement() }`)
//! gold     on its owner's land ⌊(30·P)^0.75 · (1 + m %)⌋ / 20, P = the kind's production cost at the
//!          game's speed and the owner's handicap, no city modifier; elsewhere none
//!          (Unciv: MapUnit.disband, BaseUnit.getDisbandGold = getBaseGoldCost(civ, null) / 20)
//! ```
//!
//! The base gold cost is the purchase formula before its rounding to 10; a
//! division by 20 of the rounded price gives the same number, so the gold
//! is `hurry_price(P, m) / 20`, m the kind's HurryCostModifier (Unciv: getBaseGoldCost multiplies by it).

use crate::ids::{PlayerId, ProductionItem, UnitHandle, UnitKindId};
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;

/// Unciv: BaseUnit.getDisbandGold divides the base gold cost by 20.
pub const DISBAND_GOLD_DIVISOR: u32 = 20;

/// Why a unit cannot be disbanded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisbandBlock {
    UnknownUnit,
    /// Unciv offers the order only to a unit with movement left.
    NoMovesLeft,
}

impl DisbandBlock {
    pub const ALL: [DisbandBlock; 2] = [DisbandBlock::UnknownUnit, DisbandBlock::NoMovesLeft];

    pub fn reason(self) -> &'static str {
        match self {
            DisbandBlock::UnknownUnit => "no such unit to disband",
            DisbandBlock::NoMovesLeft => "a unit disbands only with moves left",
        }
    }
}

impl From<DisbandBlock> for crate::command::CommandError {
    fn from(block: DisbandBlock) -> Self {
        crate::command::CommandError::RuleViolation(block.reason().into())
    }
}

/// The gold `unit` would bring if disbanded now (0 off its owner's land), or
/// why it cannot be disbanded.
pub fn disband_status(auth: &AuthWorld, rules: &CompiledRuleset, unit: UnitHandle) -> Result<u32, DisbandBlock> {
    let core = *auth.units().get(unit).ok_or(DisbandBlock::UnknownUnit)?.core;
    if core.moves_left == 0 {
        return Err(DisbandBlock::NoMovesLeft);
    }
    let own_land = auth.map().owner(core.tile).flatten() == Some(core.owner);
    Ok(if own_land { disband_gold(auth, rules, core.owner, core.kind) } else { 0 })
}

/// The disband gold of a unit of `kind` of `owner` on its owner's land.
/// Unciv prices it with no city (`getProductionCost(civ, null)`): the game's
/// speed and the owner's handicap count, no city's cost percents. Those two
/// are the owner's; the kernel keeps them in a city's modifiers, so any city
/// of the owner carries them, with its cost percents cleared. A player with
/// no city has no land, and no disband gold.
pub fn disband_gold(auth: &AuthWorld, rules: &CompiledRuleset, owner: PlayerId, kind: UnitKindId) -> u32 {
    let Some(city) = auth.indexes().cities_of(owner).first().and_then(|c| auth.cities().get(*c)) else {
        return 0;
    };
    let mut mods = crate::effects::city::city_modifiers(auth, rules, city);
    mods.cost_percent.fill(0);
    let modifier = rules.units.get(kind.0 as usize).map_or(0, |u| u.hurry_cost_modifier);
    mods.production_cost(rules, ProductionItem::Unit(kind))
        .and_then(|cost| crate::world::city::hurry_price(cost, modifier))
        .map_or(0, |price| price / DISBAND_GOLD_DIVISOR)
}
