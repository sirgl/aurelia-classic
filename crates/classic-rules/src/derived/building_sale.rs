//! Selling a building (read-only): whether the player may sell it now and
//! the gold it brings (`Command::SellBuilding`, `systems::building_sale`).
//!
//! ```text
//! may      an own city that takes orders (not a puppet, not awaiting its conqueror's decision),
//!          the building stands in it, no wonder and not the palace, no great work in its
//!          slots, no other sale in the city this turn
//! gold     the building's cost / 10
//! ```
//!
//! Unciv 4.22.10: City.sellBuilding / getGoldForSellingBuilding (`cost / 10`),
//! Building.isSellable (`!isAnyWonder() && !Unsellable`; the palace is a
//! national wonder there), ConstructionInfoTable.showSellButton (not a puppet,
//! `hasSoldBuildingThisTurn`), TurnManager.startTurn (the flag clears).
//! Unciv has no great works; Civ V forbids selling a building that holds one
//! (CvCityBuildings::IsBuildingSellable, «Great Work present in this one?»):
//! the rule here, so a sale never drops a work.

use crate::ids::{BuildingKindId, CityHandle, PlayerId};
use crate::rules::content::BuildingKind;
use crate::rules::CompiledRuleset;
use crate::world::city::CityStatus;
use crate::world::state::AuthWorld;

/// Unciv: City.getGoldForSellingBuilding, `cost / 10` (Civ V: BUILDING_SALE_DIVISOR).
pub const BUILDING_SALE_DIVISOR: u32 = 10;

/// Why a building cannot be sold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaleBlock {
    UnknownCity,
    NotYourCity,
    /// A puppet, or a captured city before its conqueror decides.
    CityTakesNoOrders,
    NotInCity,
    /// World and national wonders are never sold.
    Wonder,
    Palace,
    /// A great work sits in the building's slots.
    HoldsGreatWork,
    /// The city has sold a building this turn already.
    SoldThisTurn,
}

impl SaleBlock {
    pub const ALL: [SaleBlock; 8] = [
        SaleBlock::UnknownCity,
        SaleBlock::NotYourCity,
        SaleBlock::CityTakesNoOrders,
        SaleBlock::NotInCity,
        SaleBlock::Wonder,
        SaleBlock::Palace,
        SaleBlock::HoldsGreatWork,
        SaleBlock::SoldThisTurn,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            SaleBlock::UnknownCity => "no such city to sell in",
            SaleBlock::NotYourCity => "only the owner sells a city's buildings",
            SaleBlock::CityTakesNoOrders => "this city sells nothing",
            SaleBlock::NotInCity => "the building is not in the city",
            SaleBlock::Wonder => "wonders are not for sale",
            SaleBlock::Palace => "the palace is not for sale",
            SaleBlock::HoldsGreatWork => "a great work is in the building",
            SaleBlock::SoldThisTurn => "the city sold a building this turn",
        }
    }
}

impl From<SaleBlock> for crate::command::CommandError {
    fn from(block: SaleBlock) -> Self {
        crate::command::CommandError::RuleViolation(block.reason().into())
    }
}

/// The gold a sale of a building of this kind brings.
pub fn sale_gold(rules: &CompiledRuleset, building: BuildingKindId) -> u32 {
    rules.buildings.get(building.0 as usize).map_or(0, |def| def.cost / BUILDING_SALE_DIVISOR)
}

/// The kind may ever be sold (a property of the content: no wonder, not the
/// palace).
pub fn sellable_kind(rules: &CompiledRuleset, building: BuildingKindId) -> Result<(), SaleBlock> {
    let def = rules.buildings.get(building.0 as usize).ok_or(SaleBlock::NotInCity)?;
    match def.kind {
        BuildingKind::Palace => Err(SaleBlock::Palace),
        BuildingKind::WorldWonder | BuildingKind::NationalWonder => Err(SaleBlock::Wonder),
        _ if def.wonder => Err(SaleBlock::Wonder),
        _ => Ok(()),
    }
}

/// The city sold a building on its owner's current turn.
pub fn sold_this_turn(auth: &AuthWorld, player: PlayerId, city: CityHandle) -> bool {
    let Some(id) = auth.cities().get(city).map(|core| core.id) else {
        return false;
    };
    auth.players().economy_of(player).is_some_and(|e| e.sold_building_cities.binary_search(&id).is_ok())
}

/// The gold the sale of `building` in `city` brings now, or why `player`
/// cannot sell it.
pub fn sale_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    city: CityHandle,
    building: BuildingKindId,
) -> Result<u32, SaleBlock> {
    let core = auth.cities().get(city).ok_or(SaleBlock::UnknownCity)?;
    if core.owner != player {
        return Err(SaleBlock::NotYourCity);
    }
    if matches!(core.status, CityStatus::Puppet | CityStatus::AwaitingDecision) {
        return Err(SaleBlock::CityTakesNoOrders);
    }
    if !core.buildings.contains(&building) {
        return Err(SaleBlock::NotInCity);
    }
    sellable_kind(rules, building)?;
    if auth.module().tourism().city(city).count_in(building) > 0 {
        return Err(SaleBlock::HoldsGreatWork);
    }
    if sold_this_turn(auth, player, city) {
        return Err(SaleBlock::SoldThisTurn);
    }
    Ok(sale_gold(rules, building))
}
