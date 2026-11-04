//! Carriers (classic/docs/classic/rules.md): an
//! aircraft or missile is based on an own carrier within twice its range;
//! the carrier takes as many as its Cargo stat allows, they sail with it,
//! go down with it and keep their carrier through an upgrade. A carried
//! unit does not count against a city's air capacity.

use crate::classic::unit::ClassicUnit;
use crate::ids::{EntityId, TileId, UnitHandle};
use crate::rules::effects::UnitStat;
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::validation::world::ValidationError;
use crate::world::state::AuthWorld;

/// The carrier a unit is based on.
pub fn carrier_of(auth: &AuthWorld, unit: UnitHandle) -> Option<EntityId> {
    auth.units().module_of(unit).and_then(|state| state.carried_by)
}

/// Aircraft and missiles based on `carrier`, in unit order.
pub fn cargo_of(auth: &AuthWorld, carrier: UnitHandle) -> Vec<UnitHandle> {
    let Some(core) = auth.units().get(carrier).map(|u| *u.core) else {
        return Vec::new();
    };
    auth.indexes()
        .units_on(core.tile)
        .iter()
        .copied()
        .filter(|u| carrier_of(auth, *u) == Some(core.id))
        .collect()
}

/// How many units `carrier` may carry (the Cargo stat of its promotions),
/// 0 for a unit that carries nothing.
pub fn cargo_space(auth: &AuthWorld, rules: &CompiledRuleset, carrier: UnitHandle) -> i64 {
    let Some(view) = auth.units().get(carrier) else {
        return 0;
    };
    let carries = rules.units.get(view.core.kind.0 as usize).and_then(|d| d.combat.carries);
    if carries.is_none() {
        return 0;
    }
    let space = crate::effects::unit::unit_modifiers(auth, rules, view.core).stat(UnitStat::Cargo);
    i64::from(space.max(0))
}

impl<'a> WorldTransaction<'a> {
    /// Base `unit` on `carrier` (None: in a city).
    pub(crate) fn set_carrier(&mut self, unit: UnitHandle, carrier: Option<EntityId>) {
        if let Some(state) = self.world.units.module_mut(unit) {
            *state = ClassicUnit { carried_by: carrier };
        }
    }

    /// A carrier stepped onto `to`: the aircraft and missiles based on it
    /// follow (journaled steps, no moves spent).
    pub(crate) fn carry_cargo(&mut self, carrier: UnitHandle, to: TileId, rules: &CompiledRuleset) {
        let Some(core) = self.world.units.get(carrier).map(|u| *u.core) else {
            return;
        };
        if !rules.units.get(core.kind.0 as usize).is_some_and(|d| d.combat.carries.is_some()) {
            return;
        }
        let cargo: Vec<UnitHandle> = self
            .world
            .indexes
            .units_of(core.owner)
            .iter()
            .copied()
            .filter(|u| {
                carrier_of(self.world, *u) == Some(core.id) && self.world.units.get(*u).is_some_and(|v| v.core.tile != to)
            })
            .collect();
        for unit in cargo {
            self.relocate_unit(unit, to, rules);
        }
    }

    /// The aircraft and missiles based on a carrier about to be removed go
    /// down with it (before it: the carrier's own entry stays the last).
    pub(crate) fn destroy_cargo(&mut self, carrier: UnitHandle) {
        for unit in cargo_of(self.world, carrier) {
            let _ = self.destroy_unit(unit);
        }
    }
}

/// Every carried unit stands on the tile of a living carrier of its owner
/// that is not carried itself.
pub fn validate_cargo(auth: &AuthWorld) -> Result<(), ValidationError> {
    for (handle, core) in auth.units().iter_handles() {
        let Some(carrier) = carrier_of(auth, handle) else {
            continue;
        };
        let host = auth.units().iter_handles().find(|(_, c)| c.id == carrier);
        let fits = host.is_some_and(|(h, c)| c.owner == core.owner && c.tile == core.tile && carrier_of(auth, h).is_none());
        if !fits {
            return Err(ValidationError::Module(format!("unit {} carried by a missing carrier {}", core.id.0, carrier.0)));
        }
    }
    Ok(())
}
