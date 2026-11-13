//! A unit disbanded by its player (`Command::DisbandUnit`; Unciv:
//! MapUnit.disband): it leaves the world as a unit disbanded for a deficit
//! does (`economy::disband_for_deficit`), with its cargo, and the treasury
//! gets the disband gold validation priced (`derived::disband`). In an own
//! city the aircraft and missiles based on a disbanded carrier stay, based
//! in the city, while the city has air room for them (Unciv: «the carried
//! units can still stay in the city»); the rest go down with it (Unciv flies
//! them to a free tile within reach when they have moves: not done here,
//! the classic carrier rule keeps them).
//! Journal: UnitChange::Destroyed (the cargo that goes, then the unit).

use crate::command::CommandError;
use crate::ids::UnitHandle;
use crate::rules::CompiledRuleset;
use crate::systems::combat::air::{air_slots, aircraft_in_city, city_air_capacity};
use crate::transaction::transaction::WorldTransaction;

impl<'a> WorldTransaction<'a> {
    pub(crate) fn disband_unit(&mut self, unit: UnitHandle, gold: u32, rules: &CompiledRuleset) -> Result<(), CommandError> {
        let core = *self.world.units.get(unit).ok_or(CommandError::UnknownUnit)?.core;
        let city = self.world.indexes.city_on_tile(core.tile).filter(|c| self.world.cities.get(*c).is_some_and(|k| k.owner == core.owner));
        if let Some(city) = city {
            for carried in crate::systems::cargo::cargo_of(self.world, unit) {
                let Some(def) = self.world.units.get(carried).and_then(|v| rules.units.get(v.core.kind.0 as usize)) else {
                    continue;
                };
                let room = city_air_capacity(self.world, rules, city) - aircraft_in_city(self.world, rules, city);
                if air_slots(def) <= room {
                    self.set_carrier(carried, None);
                }
            }
        }
        self.destroy_unit(unit)?;
        self.world.players.earn(core.owner, gold);
        Ok(())
    }
}
