//! Paradrop (the promotions «May Paradrop» and «Skyranger»
//! give the unit stat `ParadropRange`): a land unit that has not moved or
//! attacked this turn jumps from an own city, or from an own fort or
//! citadel, to a known land tile within its range; the jump ends its turn.
//! Command Paradrop; journal UnitChange::Moved (the `from` and `to` tiles
//! are not neighbours: the presentation shows a jump).
//!
//! [unverified] against the original game: the base (city or fort), the
//! range being a hex distance of at most the stat, no jump into a city or
//! onto a tile held by another player's unit, the border rule of ordinary
//! movement. The rules of Civ V were not available to check.

use crate::command::CommandError;
use crate::effects::unit::unit_modifiers;
use crate::ids::{ImprovementId, TileId, UnitHandle};
use crate::rules::content::Domain;
use crate::rules::effects::UnitStat;
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::state::{AuthWorld, GameView};

/// Why a paradrop is not possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParadropBlock {
    UnknownUnit,
    /// Not a land unit with a paradrop range.
    CannotParadrop,
    /// The unit moved or attacked this turn.
    AlreadyMoved,
    /// An embarked unit jumps from nowhere.
    Embarked,
    /// The unit does not stand in an own city or on an own fort or citadel.
    NoBase,
    /// The target is the unit's own tile.
    SameTile,
    /// The target is farther than the range.
    OutOfRange,
    /// The player has never seen the target.
    UnknownTile,
    /// Water, or ground the unit cannot enter.
    NotLand,
    /// Foreign land that is closed to the unit's owner.
    ClosedBorders,
    /// Another player's unit or city holds the target.
    TargetOccupied,
    /// The unit may not stop there (one unit of a kind per tile).
    TargetFull,
}

impl ParadropBlock {
    pub const ALL: [ParadropBlock; 12] = [
        ParadropBlock::UnknownUnit,
        ParadropBlock::CannotParadrop,
        ParadropBlock::AlreadyMoved,
        ParadropBlock::Embarked,
        ParadropBlock::NoBase,
        ParadropBlock::SameTile,
        ParadropBlock::OutOfRange,
        ParadropBlock::UnknownTile,
        ParadropBlock::NotLand,
        ParadropBlock::ClosedBorders,
        ParadropBlock::TargetOccupied,
        ParadropBlock::TargetFull,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            ParadropBlock::UnknownUnit => "unknown unit",
            ParadropBlock::CannotParadrop => "the unit cannot paradrop",
            ParadropBlock::AlreadyMoved => "the unit has moved or attacked this turn",
            ParadropBlock::Embarked => "an embarked unit cannot paradrop",
            ParadropBlock::NoBase => "the unit must stand in an own city or on an own fort",
            ParadropBlock::SameTile => "the unit is already there",
            ParadropBlock::OutOfRange => "the target is beyond the paradrop range",
            ParadropBlock::UnknownTile => "the target tile is unknown",
            ParadropBlock::NotLand => "the unit cannot land there",
            ParadropBlock::ClosedBorders => "the target is in closed foreign territory",
            ParadropBlock::TargetOccupied => "another player holds the target tile",
            ParadropBlock::TargetFull => "the unit cannot stop on the target tile",
        }
    }
}

impl From<ParadropBlock> for CommandError {
    fn from(block: ParadropBlock) -> Self {
        CommandError::RuleViolation(block.reason().into())
    }
}

/// An improvement a paratrooper may jump from: a fort (an improvement that
/// is built outside borders) or the great general's citadel.
pub fn is_jump_base(rules: &CompiledRuleset, improvement: ImprovementId) -> bool {
    let fort = rules
        .improvements
        .get(improvement.0 as usize)
        .is_some_and(|d| d.placement.outside_borders);
    fort || crate::systems::combat::actions::citadel_of(rules) == Some(improvement)
}

/// The paradrop range of the unit (0: it cannot jump).
pub fn paradrop_range(auth: &AuthWorld, rules: &CompiledRuleset, unit: UnitHandle) -> i32 {
    let Some(view) = auth.units().get(unit) else {
        return 0;
    };
    let land = rules
        .units
        .get(view.core.kind.0 as usize)
        .is_some_and(|d| d.domain == Domain::Land);
    if !land {
        return 0;
    }
    unit_modifiers(auth, rules, view.core).stat(UnitStat::ParadropRange).max(0)
}

/// May the unit jump to `target`? Pure: the dispatcher validates with it
/// and the interface asks it for the reason.
pub fn paradrop_status(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    unit: UnitHandle,
    target: TileId,
) -> Result<(), ParadropBlock> {
    let auth = view.auth;
    let core = *auth.units().get(unit).ok_or(ParadropBlock::UnknownUnit)?.core;
    let range = paradrop_range(auth, rules, unit);
    if range == 0 {
        return Err(ParadropBlock::CannotParadrop);
    }
    if core.has_moved() || core.moves_left == 0 || core.attacks_made > 0 {
        return Err(ParadropBlock::AlreadyMoved);
    }
    let map = auth.map();
    if crate::navigation::terrain::is_water(map, core.tile) {
        return Err(ParadropBlock::Embarked);
    }
    let in_own_city = auth
        .indexes()
        .city_on_tile(core.tile)
        .and_then(|c| auth.cities().get(c))
        .is_some_and(|c| c.owner == core.owner);
    // A fort is built in own or neutral land and claims nothing: its tile is
    // unowned when it stands outside the borders; foreign land makes it foreign.
    let on_own_fort = map.owner(core.tile).flatten().is_none_or(|o| o == core.owner)
        && map.working_improvement(core.tile).is_some_and(|i| is_jump_base(rules, i));
    if !(in_own_city || on_own_fort) {
        return Err(ParadropBlock::NoBase);
    }
    let distance = map.dimensions.distance(core.tile, target).unwrap_or(i32::MAX);
    if distance == 0 {
        return Err(ParadropBlock::SameTile);
    }
    if distance > range {
        return Err(ParadropBlock::OutOfRange);
    }
    // The player jumps onto ground it has seen: now, or at some time.
    let known = crate::knowledge::view::is_visible(view.derived, core.owner, target)
        || auth.knowledge().get(core.owner.0 as usize).is_some_and(|k| k.is_explored(target));
    if !known {
        return Err(ParadropBlock::UnknownTile);
    }
    let profile = crate::navigation::MoveProfile::of_unit(auth, rules, &core);
    if crate::navigation::terrain::is_water(map, target) || !crate::navigation::cost::destination_ok(auth, &profile, target) {
        return Err(ParadropBlock::NotLand);
    }
    if !crate::world::territory::may_enter(auth, core.owner, target) {
        return Err(ParadropBlock::ClosedBorders);
    }
    let foreign_city = auth
        .indexes()
        .city_on_tile(target)
        .and_then(|c| auth.cities().get(c))
        .is_some_and(|c| c.owner != core.owner);
    let foreign_unit = auth
        .indexes()
        .units_on(target)
        .iter()
        .any(|u| auth.units().get(*u).is_some_and(|v| v.core.owner != core.owner));
    if foreign_city || foreign_unit {
        return Err(ParadropBlock::TargetOccupied);
    }
    crate::world::stacking::can_stop_at(auth, rules, core.owner, core.kind, Some(unit), target)
        .map_err(|_| ParadropBlock::TargetFull)
}

impl<'a> WorldTransaction<'a> {
    /// The unit lands on `to` (validated): its turn ends, its orders end,
    /// the landing counts as a step (an empty barbarian camp falls, a
    /// natural wonder is found).
    pub(crate) fn paradrop(&mut self, unit: UnitHandle, to: TileId, rules: &CompiledRuleset) -> Result<(), CommandError> {
        let owner = self.world.units.get(unit).ok_or(CommandError::UnknownUnit)?.core.owner;
        self.relocate_unit(unit, to, rules);
        let core = self.world.units.get_mut_core(unit).ok_or(CommandError::UnknownUnit)?;
        core.moves_left = 0;
        self.clear_camp_at(owner, to, rules);
        self.touch_natural_wonders(unit, to, rules);
        Ok(())
    }
}
