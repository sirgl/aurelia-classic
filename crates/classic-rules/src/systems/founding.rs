//! Nation traits of founding a city (the founding itself is the
//! kernel's, `kernel::systems::city_founding`; the module adds these at
//! `KernelEvent::CityFounded`): the Shoshone claim extra tiles
//! (`FoundingTerritoryTiles`, chosen as border growth chooses), Indonesia's
//! first cities on another landmass than its original capital get a unique
//! luxury on their tile (`UniqueLuxury*`: the nation's luxuries in
//! turn, unverified). Journal: TileChange::OwnerChanged,
//! TileChange::ResourceSet.

use crate::ids::{CityHandle, PlayerId, ResourceId, TileId, UnitKindId};
use crate::rules::content::UnitAbility;
use crate::world::state::AuthWorld;
use crate::rules::stats::{SocietyStat, Stat};
use crate::rules::CompiledRuleset;
use crate::transaction::journal::TileChange;
use crate::transaction::transaction::WorldTransaction;

/// The kind founds cities abroad (the Conquistador).
pub(crate) fn founds_abroad(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    rules.units.get(kind.0 as usize).is_some_and(|d| d.module.abilities.contains(&UnitAbility::FoundAbroad))
}

/// A unit that founds only abroad may not found on the landmass of its
/// owner's capital.
pub(crate) fn abroad_problem(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    owner: PlayerId,
    kind: UnitKindId,
    tile: TileId,
) -> Option<&'static str> {
    if !founds_abroad(rules, kind) {
        return None;
    }
    let capital = crate::world::city::capital_of(auth, rules, owner).and_then(|c| auth.cities().get(c))?;
    let home = auth.map().land_area(capital.tile)?;
    (auth.map().land_area(tile) == Some(home)).then_some("founds only abroad")
}

impl<'a> WorldTransaction<'a> {
    /// The Shoshone: the city claims `FoundingTerritoryTiles` more tiles,
    /// each the best border candidate.
    pub(crate) fn claim_extra_founding_tiles(&mut self, owner: PlayerId, city: CityHandle, rules: &CompiledRuleset) {
        let stats = &crate::effects::player::player_modifiers(self.world, rules, owner).stats;
        let tiles = stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::FoundingTerritoryTiles)));
        let Some(center) = self.world.cities.get(city).map(|c| c.tile) else {
            return;
        };
        let dims = self.world.map.dimensions;
        for _ in 0..tiles.max(0) {
            let Some(tile) = crate::world::territory::border_candidate(self.world, rules, owner, city) else {
                return;
            };
            let ring = match (dims.to_coord(center), dims.to_coord(tile)) {
                (Some(a), Some(b)) => a.distance(b) as u8,
                _ => return,
            };
            crate::transaction::journal::claim_tile(&mut *self.0.world, &mut self.0.journal, city, tile);
            let _ = self.world.cities.extend_radius(city, ring);
        }
    }

    /// Indonesia: the first `UniqueLuxuryCities` cities founded (on another
    /// landmass than the original capital when `UniqueLuxuryNewLandmass`)
    /// get `UniqueLuxuryQuantity` copies of one of the nation's
    /// luxuries on their tile.
    pub(crate) fn place_unique_luxury(&mut self, owner: PlayerId, city: CityHandle, rules: &CompiledRuleset) {
        let stats = &crate::effects::player::player_modifiers(self.world, rules, owner).stats;
        let limit = stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::UniqueLuxuryCities)));
        let given = self
            .world
            .players
            .economy_of(owner)
            .map(|e| e.owed.unique_luxury_cities)
            .unwrap_or(0);
        if limit <= i32::from(given) {
            return;
        }
        let Some(tile) = self.world.cities.get(city).map(|c| c.tile) else {
            return;
        };
        let new_landmass_only = stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::UniqueLuxuryNewLandmass))) > 0;
        if new_landmass_only && !self.on_new_landmass(owner, tile) {
            return;
        }
        let nation = self.world.players.nation_of(owner);
        let luxuries: Vec<ResourceId> = (0..rules.resources.len())
            .map(|i| ResourceId(i as u16))
            .filter(|r| rules.resources[r.0 as usize].nation == Some(nation))
            .collect();
        if luxuries.is_empty() {
            return;
        }
        let resource = luxuries[usize::from(given) % luxuries.len()];
        let quantity = stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::UniqueLuxuryQuantity))).clamp(1, i32::from(u8::MAX)) as u8;
        self.world.map.set_resource_amount(tile, Some(resource), quantity);
        self.journal.tile_changes.push(TileChange::ResourceSet { tile });
        if let Some(economy) = self.world.players.economy_mut(owner) {
            economy.owed.unique_luxury_cities = given.saturating_add(1);
        }
    }

    /// `tile` is on another landmass than the owner's original capital
    /// (false without one: the first city is where the nation starts).
    fn on_new_landmass(&self, owner: PlayerId, tile: TileId) -> bool {
        let capital = self
            .world
            .cities
            .iter()
            .find(|(_, c)| c.original_owner == owner && c.original_capital)
            .map(|(_, c)| c.tile);
        let Some(capital) = capital else {
            return false;
        };
        let map = &self.world.map;
        capital != tile && map.land_area(capital) != map.land_area(tile)
    }
}

