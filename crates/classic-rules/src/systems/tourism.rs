//! Great works, cultural influence and public opinion (owner of
//! `world::tourism`; rules: derived::tourism). Commands: CreateGreatWork,
//! WriteTreatise, PerformConcert, SwitchIdeology. Turn step Tourism:
//! influence on every met civilization, then the public opinion of the
//! player's ideology. Completing a building with `FreeGreatWork` puts a
//! work of art into it. Journal: PolicyAdopted on an ideology switch (the
//! tenets' effects change); works and influence feed no cached number.

use crate::command::CommandError;
use crate::derived::tourism::{
    concert_share, concert_status, great_work_status, influence_toward, met, public_opinion,
    switch_ideology_status, treatise_status, FreeSlot, PlayerTourismTotals, TourismBlock,
};
use crate::ids::{BuildingKindId, CityHandle, PlayerId, UnitHandle};
use crate::rules::content::{BuildingAbility, GreatWorkSlot};
use crate::rules::CompiledRuleset;
use crate::classic::ClassicChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::tourism::{GreatWork, GreatWorkClass, PlacedWork};

fn tourism_rule(block: TourismBlock) -> CommandError {
    CommandError::RuleViolation(block.reason().into())
}

impl<'a> WorldTransaction<'a> {
    /// A Great Writer, Artist or Musician creates its work in the nearest
    /// free slot of its type and is spent.
    pub(crate) fn create_great_work(
        &mut self,
        unit: UnitHandle,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let (class, slot) = great_work_status(self.world, rules, unit).map_err(tourism_rule)?;
        let owner = self.unit_owner_of(unit)?;
        self.place_new_work(owner, class, slot, rules);
        self.expend_unit(unit, rules).map(|_| ())
    }

    /// A Great Writer's Political Treatise: the culture of the last turns
    /// into the pool; the writer is spent.
    pub(crate) fn write_treatise(
        &mut self,
        unit: UnitHandle,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let culture = treatise_status(self.world, rules, unit).map_err(tourism_rule)?;
        let owner = self.unit_owner_of(unit)?;
        self.gain_culture(owner, culture);
        self.expend_unit(unit, rules).map(|_| ())
    }

    /// A Great Musician's concert in another civilization's land: influence
    /// on it and a share on every other civilization the player met.
    pub(crate) fn perform_concert(
        &mut self,
        unit: UnitHandle,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let (target, blast) = concert_status(self.world, rules, unit).map_err(tourism_rule)?;
        let core = *self
            .world
            .units
            .get(unit)
            .ok_or(CommandError::UnknownUnit)?
            .core;
        let others = blast * concert_share(rules, core.kind) / 100;
        for p in 0..self.world.players.count() as u8 {
            let other = PlayerId(p);
            let amount = match other {
                o if o == target => blast,
                o if o == core.owner || !self.world.players.is_alive(o) => continue,
                o if !met(self.world, core.owner, o) => continue,
                _ => others,
            };
            self.add_influence(core.owner, other, amount);
        }
        self.expend_unit(unit, rules).map(|_| ())
    }

    /// The player follows the ideology its people prefer: the old tenets go
    /// (all but `tenets_lost_on_switch` come back as free tenets of the new
    /// one) and the culture pool burns.
    pub(crate) fn switch_ideology(
        &mut self,
        player: PlayerId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let preferred = switch_ideology_status(self.world, player).map_err(tourism_rule)?;
        let Some(state) = self.world.module.culture.player_mut(player) else {
            return Err(tourism_rule(TourismBlock::UnknownPlayer));
        };
        let Some(old) = state.ideology else {
            return Err(tourism_rule(TourismBlock::NoIdeology));
        };
        let in_old = |p: &crate::ids::PolicyId| rules.module.policies[p.0 as usize].branch == Some(old);
        let old_tenets = state.policies.iter().filter(|p| in_old(p)).count() as u16;
        state.policies.retain(|p| !in_old(p));
        state.branches.retain(|b| *b != old);
        state.branches.push(preferred);
        state.ideology = Some(preferred);
        state.culture = 0;
        state.free_tenets = old_tenets.saturating_sub(crate::derived::tourism::tenets_lost_on_switch(old_tenets));
        state.free_used = state.free_used.min(state.policies.len() as u16);
        self.update_public_opinion(player, rules);
        self.journal
            .module_changes
            .push(ClassicChange::PolicyAdopted { player });
        Ok(())
    }

    /// Turn step Tourism: the influence of this turn on every civilization
    /// (rules.md, tourism), then the public opinion.
    pub(crate) fn tourism_turn(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        self.archaeology_turn(player, rules);
        let lifetime = self.world.module.culture.of(player).lifetime;
        let totals = PlayerTourismTotals::of(self.world, rules, player);
        let gains: Vec<u64> = (0..self.world.players.count() as u8)
            .map(|t| influence_toward(self.world, rules, &totals, PlayerId(t)))
            .collect();
        if let Some(state) = self.world.module.tourism.player_mut(player) {
            state.lifetime_before = lifetime;
            state.influence_before.clone_from(&state.influence);
            for (slot, gain) in state.influence.iter_mut().zip(&gains) {
                // What a nation stops spreading is forgotten a little each turn (derived::tourism::faded).
                *slot = crate::derived::tourism::faded(*slot).saturating_add(*gain);
            }
        }
        self.update_public_opinion(player, rules);
    }

    /// A completed building with `FreeGreatWork` gets a work of art by its
    /// owner in its own slot (the Parthenon frieze).
    pub(crate) fn place_free_great_work(
        &mut self,
        city: CityHandle,
        building: BuildingKindId,
        rules: &CompiledRuleset,
    ) {
        let def = &rules.buildings[building.0 as usize];
        let free = def.module.abilities.contains(&BuildingAbility::FreeGreatWork);
        let art_slot =
            matches!(def.module.great_work_slots, Some((GreatWorkSlot::ArtArtifact, n)) if n > 0);
        let Some(owner) = self.world.cities.get(city).map(|c| c.owner) else {
            return;
        };
        if free && art_slot && self.world.module.tourism.city(city).count_in(building) == 0 {
            self.place_new_work(
                owner,
                GreatWorkClass::Art,
                FreeSlot { city, building },
                rules,
            );
        }
    }

    fn place_new_work(&mut self, owner: PlayerId, class: GreatWorkClass, slot: FreeSlot, rules: &CompiledRuleset) {
        let era = crate::world::player::player_era(self.world, rules, owner);
        self.place_work(
            GreatWork {
                class,
                era,
                creator: owner,
            },
            slot,
        );
    }

    /// A new work into a free slot.
    pub(crate) fn place_work(&mut self, work: GreatWork, slot: FreeSlot) {
        let work = self.world.module.tourism.add_work(work);
        self.world.module.tourism.city_mut(slot.city).works.push(PlacedWork {
            building: slot.building,
            work,
        });
        self.journal
            .module_changes
            .push(ClassicChange::GreatWorksChanged { city: slot.city });
    }

    fn update_public_opinion(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        let result = public_opinion(self.world, rules, player);
        if let Some(state) = self.world.module.tourism.player_mut(player) {
            state.opinion = result.opinion;
            state.dissatisfaction = result.dissatisfaction;
            state.preferred = result.preferred;
        }
    }

    /// Influence on every other living civilization the player met
    /// (Futurism, rules "Culture" §1.5).
    pub(crate) fn tourism_burst(&mut self, player: PlayerId, amount: u64) {
        for p in 0..self.world.players.count() as u8 {
            let other = PlayerId(p);
            let known = met(self.world, player, other);
            if other == player || !known || !self.world.players.is_major(other) || !self.world.players.is_alive(other) {
                continue;
            }
            self.add_influence(player, other, amount);
        }
    }

    fn add_influence(&mut self, from: PlayerId, on: PlayerId, amount: u64) {
        if let Some(state) = self.world.module.tourism.player_mut(from) {
            if let Some(slot) = state.influence.get_mut(on.0 as usize) {
                *slot = slot.saturating_add(amount);
            }
        }
    }

    /// Instant culture (treatise, digs) into the pool and the lifetime sum.
    pub(crate) fn gain_culture(&mut self, player: PlayerId, amount: u32) {
        if let Some(state) = self.world.module.culture.player_mut(player) {
            state.culture = state.culture.saturating_add(amount);
            state.lifetime = state.lifetime.saturating_add(u64::from(amount));
        }
    }

    fn unit_owner_of(&self, unit: UnitHandle) -> Result<PlayerId, CommandError> {
        self.world
            .units
            .get(unit)
            .map(|v| v.core.owner)
            .ok_or(CommandError::UnknownUnit)
    }
}
