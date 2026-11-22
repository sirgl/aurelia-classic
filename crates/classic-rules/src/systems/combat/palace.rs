//! The palace of a nation when a city leaves it or arrives (Unciv 4.22.10,
//! `CityConquestFunctions.moveToCiv`, `Civilization.moveCapitalToNextLargest` and `moveCapitalTo`;
//! the rule is in `world::capital`): the palace never travels with a city, and a nation with a city
//! always holds one.
//!
//! Journal: CityChange::StatusChanged (a puppet that is annexed),
//! CityChange::BuildingAdded (the city that gets it), PlayerChange::EffectsChanged (the nation:
//! what is scoped to its capital changes).

use crate::ids::{CityHandle, PlayerId};
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{CityChange, PlayerChange};
use crate::transaction::transaction::WorldTransaction;
use crate::world::capital::{give_palace, holds_palace, next_capital, palace_building, palace_cities, remove_palace};
use crate::world::city::CityStatus;

impl<'a> WorldTransaction<'a> {
    /// A city leaves its owner (a transfer): it keeps no palace. The owner's next capital is found
    /// by `restore_palace` once the city is out of its list.
    pub(crate) fn strip_palace(&mut self, city: CityHandle, rules: &CompiledRuleset) {
        let Some(core) = self.world.cities.get_mut(city) else {
            return;
        };
        if !holds_palace(rules, core) {
            return;
        }
        let owner = core.owner;
        remove_palace(rules, core);
        // No city change here: the transfer journals `OwnerChanged` after this, and a yield
        // recompute of the city journaled before it would count the old owner's yields against
        // the new one (see `transfer_city`).
        self.journal.player_changes.push(PlayerChange::EffectsChanged { player: owner });
    }

    /// A nation with a city and no palace gives it to `next_capital`; a puppet is annexed, as
    /// Unciv does. A nation that holds one is left alone.
    pub(crate) fn restore_palace(&mut self, owner: PlayerId, rules: &CompiledRuleset) {
        let Some(palace) = palace_building(rules) else {
            return;
        };
        let has_cities = !self.world.indexes.cities_of(owner).is_empty();
        if !has_cities || !palace_cities(self.world, rules, owner).is_empty() {
            return;
        }
        let Some(city) = next_capital(self.world, rules, owner) else {
            return;
        };
        self.annex_new_capital(city);
        give_palace(self.world, city, palace);
        self.journal.city_changes.push(CityChange::BuildingAdded { city, building: palace });
        self.journal.player_changes.push(PlayerChange::EffectsChanged { player: owner });
    }

    /// The city becomes the capital: a puppet is annexed (Unciv `moveCapitalToNextLargest`) and a
    /// burning city stops burning (`moveCapitalTo`: `isBeingRazed = false`); a capital cannot be
    /// destroyed, so a city left burning would never finish.
    fn annex_new_capital(&mut self, city: CityHandle) {
        let Some(core) = self.world.cities.get_mut(city) else {
            return;
        };
        if matches!(core.status, CityStatus::Puppet | CityStatus::Razing) {
            core.status = CityStatus::Occupied;
            self.journal.city_changes.push(CityChange::StatusChanged { city });
        }
    }
}
