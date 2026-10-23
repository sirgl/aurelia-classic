//! Cities in combat (classic/docs/classic/rules.md):
//! melee on a city and its capture, the conqueror's decision (annex,
//! puppet, raze, liberate), the city's ranged strike, razing and
//! resistance at the owner's turn.
//!
//! Journal: CityChange::OwnerChanged / StatusChanged / PopulationChanged /
//! Removed, TileChange::OwnerChanged, UnitChange::Destroyed.

use crate::combat::city::{max_hit_points, CITY_CAPTURE_DAMAGE_PERCENT};
use crate::combat::damage::Roll;
use crate::combat::exchange::{AttackPlan, CityStrikePlan};
use crate::command::{CityDecision, CommandError};
use crate::effects::unit::unit_modifiers;
use crate::ids::{CityHandle, PlayerId};
use crate::rules::effects::UnitStat;
use crate::rules::CompiledRuleset;
use crate::classic::ClassicChange;
use crate::transaction::journal::CityChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city::{CityStatus, CityWrite};
use crate::world::player::ClassicPlayers;

/// Citizens a captured city loses: one plus one per this many citizens (a city
/// of 1 loses none). Unciv: `city/managers/CityConquestFunctions.kt`
/// (`conquerCity`: `-1 - population / 4` when the population is above 1); equal.
pub const CAPTURED_CITY_POPULATION_LOSS_DIVISOR: u32 = 4;
/// Gold barbarians take from the treasury of a city's owner when they
/// capture it (at most the treasury). Unciv: Battle.kt, 200; equal.
pub const BARBARIAN_CAPTURE_RANSOM: u32 = 200;

impl<'a> WorldTransaction<'a> {
    /// Damage a city; journaled for the presentation (no derived system
    /// reads city damage).
    pub(crate) fn damage_city(&mut self, city: CityHandle, damage: u16) {
        let Some(core) = self.world.cities.get_mut(city) else {
            return;
        };
        core.damage = core.damage.saturating_add(damage);
        if damage > 0 {
            self.journal.module_changes.push(ClassicChange::CityDamaged { city, damage });
        }
    }

    /// Melee attack on a city (§4.4): both strike; at 0 HP the city falls
    /// (a barbarian takes a ransom instead).
    pub(crate) fn melee_city(
        &mut self,
        plan: &AttackPlan,
        city: CityHandle,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let attacker_roll = self.roll();
        let defender_roll = self.roll();
        let to_city = plan.damage_to_defender(attacker_roll).min(plan.defender_hp);
        let mut to_attacker = plan.damage_to_attacker(defender_roll);
        let falls = to_city >= plan.defender_hp;
        if falls && to_attacker >= plan.attacker_hp {
            // The city falls and the attacker would die: the attacker lives.
            to_attacker = plan.attacker_hp - 1;
        }
        self.damage_city(city, to_city);
        self.city_attack_plunder(plan, to_city, rules);
        let outcome = self.strike_unit(plan.attacker, to_attacker)?;
        if outcome == crate::systems::combat::StrikeOutcome::Killed {
            return Ok(());
        }
        let xp = crate::combat::experience::EXPERIENCE_ATTACKING_CITY_MELEE;
        self.combat_experience(
            plan.attacker,
            xp,
            plan.defender_owner,
            plan.target_tile,
            rules,
        );
        if !falls {
            return self.finish_attack(plan.attacker, rules, true);
        }
        let no_capture = {
            let core = *self
                .world
                .units
                .get(plan.attacker)
                .ok_or(CommandError::UnknownUnit)?
                .core;
            unit_modifiers(self.world, rules, &core)
                .has(crate::rules::effects::UnitAbility::NoCapture)
        };
        if self.world.players.is_barbarian(plan.attacker_owner) {
            self.barbarian_ransom(plan, city, rules)?;
            return Ok(());
        }
        if no_capture {
            if let Some(core) = self.world.cities.get_mut(city) {
                core.damage = core.damage.min(plan.defender_max_hp - 1);
            }
            return self.finish_attack(plan.attacker, rules, true);
        }
        self.capture_city(city, plan.attacker_owner, rules)?;
        self.advance(plan.attacker, plan.target_tile, rules)?;
        self.finish_attack(plan.attacker, rules, false)
    }

    /// CityAttackPlunderModifier: gold = damage × percent / 100.
    fn city_attack_plunder(&mut self, plan: &AttackPlan, damage: u16, rules: &CompiledRuleset) {
        let Some(view) = self.world.units.get(plan.attacker) else {
            return;
        };
        let core = *view.core;
        let percent =
            unit_modifiers(self.world, rules, &core).stat(UnitStat::CityAttackPlunderPercent);
        if percent > 0 {
            let gold = u32::from(damage) * percent as u32 / 100;
            self.world.players.earn(plan.attacker_owner, gold);
        }
    }

    /// A barbarian that brings a city to 0 HP: the city keeps 1 HP, its
    /// owner pays up to 200 gold, the barbarian disappears (§4.4).
    fn barbarian_ransom(
        &mut self,
        plan: &AttackPlan,
        city: CityHandle,
        _rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        if let Some(core) = self.world.cities.get_mut(city) {
            core.damage = plan.defender_max_hp - 1;
        }
        let gold = self
            .world
            .players
            .treasury_of(plan.defender_owner)
            .min(BARBARIAN_CAPTURE_RANSOM);
        self.world.players.spend(plan.defender_owner, gold);
        self.destroy_unit(plan.attacker)?;
        Ok(())
    }

    /// Capture (`acquireCity`, §4.2, "Culture..." §1.6): the conqueror owns
    /// the city and its tiles; it loses `1 + population / 4` citizens; the damage is at
    /// most half the old maximum; buildings survive by their conquest
    /// chance; the old owner's units in the city die; a city that was not
    /// the conqueror's waits for its decision with resistance.
    pub(crate) fn capture_city(
        &mut self,
        city: CityHandle,
        conqueror: PlayerId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let (from, tile, old_max) = {
            let core = self
                .world
                .cities
                .get(city)
                .ok_or(CommandError::UnknownCity)?;
            (
                core.owner,
                core.tile,
                max_hit_points(self.world, rules, core),
            )
        };
        // Before the garrison dies and the city changes hands: the gold of
        // its old owner's city (systems::plunder).
        self.take_capture_gold(city, conqueror, rules);
        self.kill_garrison(city, conqueror, rules);
        let original = self.world.cities.get(city).map(|c| c.original_owner);
        if original != Some(conqueror) {
            // Before the transfer: the victim's cities still count it (§5.11).
            self.warmonger_for_conquest(conqueror, from, rules);
        }
        self.transfer_city(city, from, conqueror, rules)?;
        let survivors = self.surviving_buildings(city, rules);
        let reduction = conquest_reduction(self.world, conqueror, from);
        let core = self
            .world
            .cities
            .get_mut(city)
            .ok_or(CommandError::UnknownCity)?;
        core.buildings = survivors;
        core.population = captured_population(core.population, reduction);
        core.damage = core.damage.min(old_max * CITY_CAPTURE_DAMAGE_PERCENT / 100);
        core.queue.clear();
        core.production = 0;
        core.strike_made = true;
        if core.original_owner == conqueror {
            core.status = CityStatus::Own;
            core.resistance_turns = 0;
        } else {
            core.status = CityStatus::AwaitingDecision;
            core.resistance_turns = (core.population * (100 - reduction) / 100) as u16;
        }
        self.journal
            .city_changes
            .push(CityChange::PopulationChanged { city });
        // Lost buildings may have reached the whole empire of either side.
        for player in [from, conqueror] {
            self.journal
                .player_changes
                .push(crate::transaction::journal::PlayerChange::EffectsChanged { player });
        }
        let _ = tile;
        self.fire(
            rules,
            crate::systems::effects::TriggerEvent::CityConquered {
                player: conqueror,
                city,
                from,
            },
        );
        // The domination test runs at every acquired city.
        self.test_domination(rules);
        Ok(())
    }

    /// Owner, indexes and the city's tiles go to the new owner; its trade
    /// routes end (conquest, buyout, deals, liberation).
    pub(crate) fn transfer_city(
        &mut self,
        city: CityHandle,
        from: PlayerId,
        to: PlayerId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        self.evict_spies(city);
        let tile = self
            .world
            .cities
            .get(city)
            .ok_or(CommandError::UnknownCity)?
            .tile;
        match self.world.cities.set_owner(city, to) {
            CityWrite::Applied => {}
            CityWrite::TargetGone | CityWrite::QueueFull => return Err(CommandError::UnknownCity),
        }
        let round = self.world.turn.round;
        if let Some(core) = self.world.cities.get_mut(city) {
            core.acquired_round = round;
        }

        self.world.indexes.unindex_city_owner(from, city);
        self.world.indexes.index_city(to, tile, city);
        self.journal
            .city_changes
            .push(CityChange::OwnerChanged { city, from, to });
        crate::transaction::journal::transfer_city_tiles(&mut *self.0.world, &mut self.0.journal, city);
        // After OwnerChanged: the yield store moves a city's total between
        // owners there, a recompute journaled before it would count the
        // city's old yields against the new owner.
        self.clear_city_trade_routes(city, rules);
        // The routes counted for the old owner too (trading partners).
        if rules.effect_index.module.trade_counts {
            self.journal
                .player_changes
                .push(crate::transaction::journal::PlayerChange::EffectsChanged { player: from });
        }
        Ok(())
    }

    /// The old owner's units in a falling city die; civilians are captured.
    fn kill_garrison(&mut self, city: CityHandle, conqueror: PlayerId, rules: &CompiledRuleset) {
        let Some(core) = self.world.cities.get(city) else {
            return;
        };
        let (tile, owner) = (core.tile, core.owner);
        let mut units: Vec<_> = self.world.indexes.units_on(tile).to_vec();
        units.sort_by_key(|u| self.world.units.get(*u).map(|v| v.core.id));
        for unit in units {
            let Some(view) = self.world.units.get(unit) else {
                continue;
            };
            if view.core.owner != owner {
                continue;
            }
            let _ = self.destroy_unit(unit);
        }
        let _ = (conqueror, rules);
    }

    /// Buildings that survive the capture: never-captured ones are lost,
    /// the others survive with their ConquestProb (combat stream, building
    /// order).
    fn surviving_buildings(
        &mut self,
        city: CityHandle,
        rules: &CompiledRuleset,
    ) -> Vec<crate::ids::BuildingKindId> {
        let Some(core) = self.world.cities.get(city) else {
            return Vec::new();
        };
        let buildings = core.buildings.clone();
        let mut kept = Vec::with_capacity(buildings.len());
        for building in buildings {
            let Some(def) = rules.buildings.get(building.0 as usize) else {
                continue;
            };
            use crate::rules::content::BuildingKind;
            if def.kind == BuildingKind::Palace {
                continue;
            }
            let wonder = matches!(def.kind, BuildingKind::WorldWonder);
            if wonder {
                kept.push(building);
                continue;
            }
            if def.never_capture {
                continue;
            }
            let chance = u64::from(def.conquest_survival_pct);
            if chance >= 100 || self.world.rng.combat.below(100) < chance {
                kept.push(building);
            }
        }
        kept
    }

    /// The conqueror's decision on a captured city.
    pub(crate) fn decide_city(
        &mut self,
        city: CityHandle,
        decision: CityDecision,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let core = self
            .world
            .cities
            .get(city)
            .ok_or(CommandError::UnknownCity)?;
        if core.status != CityStatus::AwaitingDecision {
            return Err(CommandError::RuleViolation("no decision pending".into()));
        }
        let (owner, original) = (core.owner, core.original_owner);
        let status = match decision {
            CityDecision::Annex => CityStatus::Occupied,
            CityDecision::Puppet => CityStatus::Puppet,
            CityDecision::Raze => CityStatus::Razing,
            CityDecision::Liberate => {
                self.transfer_city(city, owner, original, rules)?;
                self.warmonger_for_liberation(owner, original, rules);
                let core = self
                    .world
                    .cities
                    .get_mut(city)
                    .ok_or(CommandError::UnknownCity)?;
                core.status = CityStatus::Own;
                core.resistance_turns = 0;
                self.journal
                    .city_changes
                    .push(CityChange::StatusChanged { city });
                let _ = rules;
                return Ok(());
            }
        };
        let core = self
            .world
            .cities
            .get_mut(city)
            .ok_or(CommandError::UnknownCity)?;
        core.status = status;
        if status == CityStatus::Puppet {
            core.resistance_turns = 0;
        }
        self.journal
            .city_changes
            .push(CityChange::StatusChanged { city });
        Ok(())
    }

    /// The city's ranged strike (§4.3): no answer.
    pub(crate) fn city_strike(
        &mut self,
        plan: &CityStrikePlan,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let roll = self.roll();
        let damage = plan.damage(roll);
        let owner = self
            .world
            .cities
            .get(plan.city)
            .ok_or(CommandError::UnknownCity)?
            .owner;
        if let Some(core) = self.world.cities.get_mut(plan.city) {
            core.strike_made = true;
        }
        let tile = self
            .world
            .units
            .get(plan.target)
            .ok_or(CommandError::UnknownUnit)?
            .core
            .tile;
        let outcome = self.strike_unit(plan.target, damage)?;
        if outcome == crate::systems::combat::StrikeOutcome::Damaged && !plan.civilian {
            let xp = crate::combat::experience::EXPERIENCE_DEFENDING_UNIT_RANGED;
            self.combat_experience(plan.target, xp, owner, tile, rules);
        }
        let _ = Roll::Expected;
        Ok(())
    }

    /// Start of the owner's turn for its cities (§4.2 and conquest): heal,
    /// a new strike, one turn less of resistance, razing burns a citizen
    /// (a city at 0 is removed). A decision still pending at the end of the
    /// conqueror's turn becomes a puppet (`resolve_pending_decisions`).
    pub(crate) fn conquest_and_city_heal(&mut self, owner: PlayerId, rules: &CompiledRuleset) {
        let cities = self.world.indexes.cities_of(owner).to_vec();
        for city in cities {
            self.heal_city(city, rules);
            let Some(core) = self.world.cities.get_mut(city) else {
                continue;
            };
            core.strike_made = false;
            if core.resistance_turns > 0 {
                core.resistance_turns -= 1;
            }
            if core.status == CityStatus::Razing {
                self.burn_city(city, rules);
            }
        }
    }

    fn heal_city(&mut self, city: CityHandle, rules: &CompiledRuleset) {
        let Some(core) = self.world.cities.get(city) else {
            return;
        };
        if core.damage == 0 {
            return;
        }
        let heal = crate::combat::city::heal_per_turn(self.world, rules, core, city);
        if let Some(core) = self.world.cities.get_mut(city) {
            core.damage = core.damage.saturating_sub(heal);
        }
    }

    /// One citizen burns (the Huns: `RazeSpeedPercent` more); the last one
    /// removes the city and frees its tiles.
    fn burn_city(&mut self, city: CityHandle, rules: &CompiledRuleset) {
        let Some(owner) = self.world.cities.get(city).map(|c| c.owner) else {
            return;
        };
        let burned = razed_per_turn(self.world, rules, owner);
        let Some(core) = self.world.cities.get_mut(city) else {
            return;
        };
        if core.population > burned {
            core.population -= burned;
            self.journal
                .city_changes
                .push(CityChange::PopulationChanged { city });
            return;
        }
        self.remove_city(city, rules);
    }

    /// Raze a city completely: its tiles become unowned, its units go,
    /// the city leaves the store.
    pub(crate) fn remove_city(&mut self, city: CityHandle, rules: &CompiledRuleset) {
        self.evict_spies(city);
        self.clear_city_trade_routes(city, rules);
        let Some(core) = self.world.cities.get(city) else {
            return;
        };
        let (owner, tile) = (core.owner, core.tile);
        crate::transaction::journal::release_city_tiles(&mut *self.0.world, &mut self.0.journal, city);
        self.world.indexes.unindex_city_owner(owner, city);
        self.world.indexes.unindex_city_tile(tile);
        self.world.cities.remove(city);
        self.journal
            .city_changes
            .push(CityChange::Removed { city, owner, tile });
    }

    /// End of the conqueror's turn: an undecided city becomes a puppet.
    pub(crate) fn resolve_pending_decisions(&mut self, owner: PlayerId) {
        let cities = self.world.indexes.cities_of(owner).to_vec();
        for city in cities {
            let pending = self
                .world
                .cities
                .get(city)
                .is_some_and(|c| c.status == CityStatus::AwaitingDecision);
            if !pending {
                continue;
            }
            if let Some(core) = self.world.cities.get_mut(city) {
                core.status = CityStatus::Puppet;
                core.resistance_turns = 0;
            }
            self.journal
                .city_changes
                .push(CityChange::StatusChanged { city });
        }
    }
}

/// Citizens a razing city of `owner` loses each turn (1 × (100 + percent)
/// / 100, at least 1).
fn razed_per_turn(auth: &crate::world::state::AuthWorld, rules: &CompiledRuleset, owner: PlayerId) -> u32 {
    use crate::rules::stats::{SocietyStat, Stat};
    let mods = crate::effects::player::player_modifiers(auth, rules, owner);
    let percent = mods.stats.get(Stat::Module(crate::rules::stats::ClassicStat::Society(SocietyStat::RazeSpeedPercent)));
    ((100 + percent.max(0)) / 100).max(1) as u32
}

/// The population a captured city keeps (Unciv's loss `1 + population / 4` when
/// above 1); `reduction` percent of the loss is spared.
pub(crate) fn captured_population(population: u32, reduction: u32) -> u32 {
    if population <= 1 {
        return population.max(1);
    }
    let loss = 1 + population / CAPTURED_CITY_POPULATION_LOSS_DIVISOR;
    let loss = loss * (100 - reduction.min(100)) / 100;
    population.saturating_sub(loss).max(1)
}

/// Reduction of the population loss when `conqueror` takes a city of `victim`, percent: the cultural
/// influence of the conqueror on the victim ([`crate::derived::tourism::conquest_reduction`]).
fn conquest_reduction(
    auth: &crate::world::state::AuthWorld,
    conqueror: PlayerId,
    victim: PlayerId,
) -> u32 {
    if !auth.players().is_major(victim) {
        return 0;
    }
    crate::derived::tourism::conquest_reduction(crate::derived::tourism::influence_level(auth, conqueror, victim))
}

#[cfg(test)]
mod tests {
    use super::captured_population;

    /// Unciv `CityConquestFunctions.conquerCity`: 2-3 lose 1, 4-7 lose 2, 8-11 lose 3.
    #[test]
    fn captured_city_loses_one_plus_a_quarter() {
        let kept: Vec<u32> = (1..=12).map(|p| captured_population(p, 0)).collect();
        assert_eq!(kept, vec![1, 1, 2, 2, 3, 4, 5, 5, 6, 7, 8, 8]);
        assert_eq!(captured_population(8, 100), 8, "a full reduction spares every citizen");
    }
}
