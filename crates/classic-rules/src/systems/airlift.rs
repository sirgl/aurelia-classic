//! Airlift (the Airport's airlift): a land unit
//! that has not moved this turn flies from an own city with an airlift
//! building to another one; the flight ends its turn, so each unit flies
//! at most once a turn. Command Airlift; journal UnitChange::Moved.

use crate::command::CommandError;
use crate::ids::{CityHandle, TileId, UnitHandle};
use crate::rules::content::{BuildingAbility, Domain};
use crate::rules::CompiledRuleset;
use crate::transaction::journal::UnitChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city::CityCore;
use crate::world::state::AuthWorld;

/// Why an airlift is not possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AirliftBlock {
    UnknownUnit,
    UnknownCity,
    /// Only land units fly by airlift.
    NotLandUnit,
    /// The unit spent moves this turn (or flew already).
    AlreadyMoved,
    /// The unit does not stand in an own city that allows airlifts.
    NoAirportHere,
    /// The target is not another own city that allows airlifts.
    NoAirportThere,
    /// The unit may not stop in the target city (one unit per tile).
    TargetOccupied,
}

impl AirliftBlock {
    pub const ALL: [AirliftBlock; 7] = [
        AirliftBlock::UnknownUnit,
        AirliftBlock::UnknownCity,
        AirliftBlock::NotLandUnit,
        AirliftBlock::AlreadyMoved,
        AirliftBlock::NoAirportHere,
        AirliftBlock::NoAirportThere,
        AirliftBlock::TargetOccupied,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            AirliftBlock::UnknownUnit => "unknown unit",
            AirliftBlock::UnknownCity => "unknown city",
            AirliftBlock::NotLandUnit => "only land units fly by airlift",
            AirliftBlock::AlreadyMoved => "the unit has moved this turn",
            AirliftBlock::NoAirportHere => "the unit must stand in an own city with an airport",
            AirliftBlock::NoAirportThere => "the target must be another own city with an airport",
            AirliftBlock::TargetOccupied => "the unit cannot stop in the target city",
        }
    }
}

impl From<AirliftBlock> for CommandError {
    fn from(block: AirliftBlock) -> Self {
        CommandError::RuleViolation(block.reason().into())
    }
}

/// Where the unit lands, or why it cannot fly.
pub fn airlift_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    city: CityHandle,
) -> Result<TileId, AirliftBlock> {
    let core = *auth.units().get(unit).ok_or(AirliftBlock::UnknownUnit)?.core;
    let target = auth.cities().get(city).ok_or(AirliftBlock::UnknownCity)?;
    let domain = rules.units.get(core.kind.0 as usize).map(|d| d.domain).unwrap_or_default();
    if domain != Domain::Land {
        return Err(AirliftBlock::NotLandUnit);
    }
    if core.has_moved() || core.moves_left == 0 {
        return Err(AirliftBlock::AlreadyMoved);
    }
    let here = auth.indexes().city_on_tile(core.tile).and_then(|h| auth.cities().get(h));
    if !here.is_some_and(|c| c.owner == core.owner && allows_airlift(rules, c)) {
        return Err(AirliftBlock::NoAirportHere);
    }
    let other = target.tile != core.tile;
    if !(other && target.owner == core.owner && allows_airlift(rules, target)) {
        return Err(AirliftBlock::NoAirportThere);
    }
    let stop = crate::world::stacking::can_stop_at(auth, rules, core.owner, core.kind, Some(unit), target.tile);
    if stop.is_err() {
        return Err(AirliftBlock::TargetOccupied);
    }
    Ok(target.tile)
}

/// A building of the city allows airlifts.
pub fn allows_airlift(rules: &CompiledRuleset, city: &CityCore) -> bool {
    city.buildings.iter().any(|b| {
        rules
            .buildings
            .get(b.0 as usize)
            .is_some_and(|d| d.module.abilities.contains(&BuildingAbility::Airlift))
    })
}

impl<'a> WorldTransaction<'a> {
    /// The unit lands in the target city with no moves left.
    pub(crate) fn airlift(&mut self, unit: UnitHandle, city: CityHandle, rules: &CompiledRuleset) -> Result<(), CommandError> {
        let to = airlift_status(self.world, rules, unit, city)?;
        let core = self.world.units.get_mut_core(unit).ok_or(CommandError::UnknownUnit)?;
        let from = core.tile;
        core.tile = to;
        core.moves_left = 0;
        core.stance = crate::world::unit::Stance::Ready;
        core.work = None;
        core.goal = None;
        self.world.indexes.move_unit(unit, from, to);
        self.journal.unit_changes.push(UnitChange::Moved { unit, from, to });
        Ok(())
    }
}
