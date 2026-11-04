//! Instant grants (`Modifier::Grant`, classic/docs/classic/rules.md) and the
//! free choices they owe: technologies (Great Library, Oxford University,
//! Rationalism, Assyria) and great people (Liberty, Leaning Tower of Pisa,
//! the Maya Long Count) wait in `PlayerEconomy::owed` until the player
//! chooses (`ChooseFreeTechnology`, `ChooseFreeGreatPerson`; options in
//! `derived::grants`). Golden ages, citizens and the Terracotta Army's
//! units apply at once.
//!
//! ```text
//! GoldenAge(turns)    a golden age of turns × (100 + GoldenAgeLengthPercent) %
//! Population(n)       the city grows by n citizens
//! MilitaryUnitCopies  one unit of each land combat kind the owner has, in
//!                     the city
//! ConquestTechnology  a free technology the city's previous owner knows,
//!                     once per city (Assyria)
//! free great person   appears in the capital and counts as born (raises the
//!                     next threshold; unverified)
//! ```
//!
//! A free technology that nothing fits any more (all known) is dropped
//! after every technology learned. Journal: PlayerChange::TechnologyChosen
//! (via `learn`), UnitChange::Created, CityChange::PopulationChanged,
//! ClassicChange::GoldenAgeChanged.

use crate::classic::effects::ClassicGrant;
use crate::command::CommandError;
use crate::derived::grants::{free_great_person_status, ChoiceBlock, free_technology_status, technology_options};
use crate::ids::{CityHandle, PlayerId, TechId, UnitKindId};
use crate::rules::effects::Grant;
use crate::classic::effects::GreatPersonKind;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::CityChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::player_economy::{FreeGreatPerson, FreeTechnology};

impl<'a> WorldTransaction<'a> {
    /// A Player-scope grant (ConquestTechnology: `grant_conquest_technology`).
    pub(crate) fn grant_to_player(&mut self, rules: &CompiledRuleset, player: PlayerId, grant: Grant) {
        match grant {
            Grant::FreeTechnologies(count) => {
                self.owe_technologies(rules, player, FreeTechnology::Any, count)
            }
            Grant::Module(ClassicGrant::FreeGreatPeople(count)) => {
                self.owe_great_people(player, FreeGreatPerson::Any, count)
            }
            Grant::Module(ClassicGrant::LongCountGreatPerson) => self.owe_great_people(player, FreeGreatPerson::LongCount, 1),
            Grant::Module(ClassicGrant::GoldenAge { turns }) => {
                let turns = crate::systems::golden_age::golden_age_length(self.world, rules, player, turns);
                self.start_golden_age(player, turns);
            }
            Grant::ConquestTechnology | Grant::MilitaryUnitCopies | Grant::Population(_) => {
                unreachable!("compiler: {grant:?} is not a plain player grant")
            }
        }
    }

    /// A City-scope grant.
    pub(crate) fn grant_to_city(&mut self, rules: &CompiledRuleset, city: CityHandle, grant: Grant) {
        match grant {
            Grant::Population(count) => self.grow_by(city, u32::from(count)),
            Grant::MilitaryUnitCopies => self.copy_military_units(rules, city),
            other => unreachable!("compiler: {other:?} targets a player"),
        }
    }

    /// Assyria: the previous owner's technologies, once per city.
    pub(crate) fn grant_conquest_technology(
        &mut self,
        rules: &CompiledRuleset,
        player: PlayerId,
        city: CityHandle,
        from: PlayerId,
    ) {
        let Some(id) = self.world.cities.get(city).map(|c| c.id) else {
            return;
        };
        let given = self
            .world
            .players
            .economy_of(player)
            .is_some_and(|e| e.owed.conquest_tech_cities.contains(&id));
        if given {
            return;
        }
        let free = FreeTechnology::KnownBy(from);
        if technology_options(self.world, rules, player, free).is_empty() {
            return;
        }
        let Some(economy) = self.world.players.economy_mut(player) else {
            return;
        };
        let at = economy.owed.conquest_tech_cities.partition_point(|c| *c < id);
        economy.owed.conquest_tech_cities.insert(at, id);
        economy.owed.technologies.push(free);
    }

    fn owe_technologies(&mut self, rules: &CompiledRuleset, player: PlayerId, free: FreeTechnology, count: u8) {
        if let Some(economy) = self.world.players.economy_mut(player) {
            for _ in 0..count {
                economy.owed.technologies.push(free);
            }
        }
        self.drop_unfit_free_technologies(rules, player);
    }

    fn owe_great_people(&mut self, player: PlayerId, free: FreeGreatPerson, count: u8) {
        if let Some(economy) = self.world.players.economy_mut(player) {
            for _ in 0..count {
                economy.owed.great_people.push(free);
            }
        }
    }

    /// Owed free technologies nothing fits any more leave the list.
    pub(crate) fn drop_unfit_free_technologies(&mut self, rules: &CompiledRuleset, player: PlayerId) {
        let owed = crate::derived::grants::owed(self.world, player).technologies;
        if owed.is_empty() {
            return;
        }
        let kept: Vec<FreeTechnology> = owed
            .iter()
            .copied()
            .filter(|free| !technology_options(self.world, rules, player, *free).is_empty())
            .collect();
        if let Some(economy) = self.world.players.economy_mut(player) {
            economy.owed.technologies = kept;
        }
    }

    /// The player takes `tech` as its next free technology.
    pub(crate) fn choose_free_technology(
        &mut self,
        player: PlayerId,
        tech: TechId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        free_technology_status(self.world, rules, player, tech)
            .map_err(|b| CommandError::RuleViolation(b.reason().into()))?;
        if let Some(economy) = self.world.players.economy_mut(player) {
            economy.owed.technologies.remove(0);
        }
        self.learn(player, tech, rules);
        Ok(())
    }

    /// The player takes a great person of `kind` as its next free one: it
    /// appears in the capital.
    pub(crate) fn choose_free_great_person(
        &mut self,
        player: PlayerId,
        kind: GreatPersonKind,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        free_great_person_status(self.world, rules, player, kind)
            .map_err(|b| CommandError::RuleViolation(b.reason().into()))?;
        let capital = crate::world::city::capital_of(self.world, rules, player)
            .ok_or(CommandError::UnknownCity)?;
        let tile = self.world.cities.get(capital).ok_or(CommandError::UnknownCity)?.tile;
        let def = rules
            .module.great_people
            .iter()
            .find(|d| d.kind == kind)
            .ok_or(ChoiceBlock::UnknownGreatPerson)?;
        self.take_owed_great_person(rules, player, kind);
        let nation = self.world.players.nation_of(player);
        let key = &rules.units[def.unit.0 as usize].key;
        let unit = rules.nation_variant(nation, key).unwrap_or(def.unit);
        self.spawn_at_city(player, tile, unit, rules);
        if let Some(economy) = self.world.players.economy_mut(player) {
            crate::derived::great_people::count_birth(economy, kind);
        }
        Ok(())
    }

    /// The head of the owed great people is used; a Long Count one marks
    /// its kind (the set starts over once every kind was given).
    fn take_owed_great_person(&mut self, rules: &CompiledRuleset, player: PlayerId, kind: GreatPersonKind) {
        let all = crate::derived::grants::great_person_options(
            rules,
            &Default::default(),
            FreeGreatPerson::Any,
        )
        .iter()
        .fold(0u16, |bits, k| bits | 1 << k.index());
        let Some(economy) = self.world.players.economy_mut(player) else {
            return;
        };
        let free = economy.owed.great_people.remove(0);
        if free != FreeGreatPerson::LongCount {
            return;
        }
        let bit = 1u16 << kind.index();
        if economy.owed.long_count_given & all == all {
            economy.owed.long_count_given = bit;
        } else {
            economy.owed.long_count_given |= bit;
        }
    }

    /// The city grows by `count` citizens.
    pub(crate) fn grow_by(&mut self, city: CityHandle, count: u32) {
        let Some(core) = self.world.cities.get_mut(city) else {
            return;
        };
        core.population = core.population.saturating_add(count);
        self.journal
            .city_changes
            .push(CityChange::PopulationChanged { city });
    }

    /// One unit of each land combat kind the owner has, in kind order.
    fn copy_military_units(&mut self, rules: &CompiledRuleset, city: CityHandle) {
        let Some(core) = self.world.cities.get(city) else {
            return;
        };
        let (owner, tile) = (core.owner, core.tile);
        let mut kinds: Vec<UnitKindId> = Vec::new();
        for unit in self.world.indexes.units_of(owner) {
            let Some(view) = self.world.units.get(*unit) else {
                continue;
            };
            let kind = view.core.kind;
            if is_land_combat(rules, kind) && !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        kinds.sort_by_key(|k| k.0);
        for kind in kinds {
            self.spawn_at_city(owner, tile, kind, rules);
        }
    }
}

/// A land unit that fights.
fn is_land_combat(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    let Some(def) = rules.units.get(kind.0 as usize) else {
        return false;
    };
    let fights = def.strength > 0 || def.ranged_strength > 0;
    def.domain == crate::rules::content::Domain::Land && fights && !rules.is_civilian(kind)
}
