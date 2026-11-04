//! The deficit (rules.md): the kernel's treasury
//! (`kernel::systems::economy`) asks the module what a deficit costs.
//! Journal: what disbanding a unit writes (UnitChange::Destroyed).

use crate::ids::{PlayerId, UnitHandle};
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;

impl<'a> WorldTransaction<'a> {
    /// With more military units than cities, one of the weakest goes with
    /// probability 1/2. A unit built this turn stays (the deficit runs
    /// before the cities build). Own rule, not compared with a public
    /// source (unverified).
    pub(crate) fn disband_for_deficit(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        let built_now = |u: &UnitHandle| {
            self.journal
                .unit_changes
                .iter()
                .any(|c| matches!(c, crate::transaction::journal::UnitChange::Created(h) if h == u))
        };
        let military: Vec<(UnitHandle, u16, u64)> = self
            .world
            .indexes
            .units_of(player)
            .iter()
            .filter(|u| !built_now(u))
            .filter_map(|u| self.world.units.get(*u).map(|v| (*u, *v.core)))
            .filter_map(|(h, core)| {
                let def = rules.units.get(core.kind.0 as usize)?;
                (def.strength > 0).then_some((h, def.strength, core.id.0))
            })
            .collect();
        let cities = self.world.indexes.cities_of(player).len();
        if military.len() <= cities || self.world.rng.events.below(100) >= 50 {
            return;
        }
        if let Some((unit, _, _)) = military.iter().min_by_key(|(_, s, id)| (*s, *id)) {
            let _ = self.destroy_unit(*unit);
        }
    }
}
