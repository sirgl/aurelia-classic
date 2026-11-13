//! A building sold by its city's owner (`Command::SellBuilding`; Unciv:
//! City.sellBuilding): the building leaves the city, the treasury gets the
//! gold validation priced (`derived::building_sale`), and the city may sell
//! nothing more this turn. The specialists of its slots go back to the
//! automatic assignment (hand-assigned ones count only up to the free slots,
//! `derived::citizens`), its effects and maintenance end with it.
//! At the start of the owner's turn its cities may sell again.
//! Journal: PlayerChange::EffectsChanged (the building may have reached the
//! whole empire), ClassicChange::BuildingSold.

use crate::classic::change::ClassicChange;
use crate::command::CommandError;
use crate::ids::{BuildingKindId, CityHandle, PlayerId};
use crate::transaction::journal::PlayerChange;
use crate::transaction::transaction::WorldTransaction;

impl<'a> WorldTransaction<'a> {
    pub(crate) fn sell_building(
        &mut self,
        player: PlayerId,
        city: CityHandle,
        building: BuildingKindId,
        gold: u32,
    ) -> Result<(), CommandError> {
        let core = self.world.cities.get_mut(city).ok_or(CommandError::UnknownCity)?;
        let Some(at) = core.buildings.iter().position(|b| *b == building) else {
            return Err(crate::derived::building_sale::SaleBlock::NotInCity.into());
        };
        core.buildings.remove(at);
        let id = core.id;
        if let Some(economy) = self.world.players.economy_mut(player) {
            let sold = &mut economy.sold_building_cities;
            if let Err(at) = sold.binary_search(&id) {
                sold.insert(at, id);
            }
        }
        self.world.players.earn(player, gold);
        self.journal.player_changes.push(PlayerChange::EffectsChanged { player });
        self.journal.module_changes.push(ClassicChange::BuildingSold { player, city, building, gold });
        Ok(())
    }

    /// The player's turn starts: its cities may sell a building again.
    pub(crate) fn reopen_building_sales(&mut self, player: PlayerId) {
        if let Some(economy) = self.world.players.economy_mut(player) {
            economy.sold_building_cities.clear();
        }
    }
}
