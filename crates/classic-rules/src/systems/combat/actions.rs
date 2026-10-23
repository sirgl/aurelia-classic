//! Unit actions of the military: pillage (classic/docs/classic/rules.md),
//! the great general's citadel and its damage, nuclear strikes (§2.8).
//!
//! Pillage damages the improvement (`MapState::pillaged`: no yields,
//! connection or defence until a worker repairs it) or removes one that is
//! destroyed when pillaged (fishing boats, fort, offshore platform); without
//! an intact improvement it removes the road.

use crate::classic::ClassicChange;
use crate::navigation::terrain::is_water;
use crate::command::CommandError;
use crate::effects::unit::unit_modifiers;
use crate::ids::{PlayerId, TileId, UnitHandle};
use crate::rules::effects::UnitAbility;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{CityChange, TileChange};
use crate::transaction::transaction::WorldTransaction;
use crate::world::unit::{MOVE_DENOMINATOR, UNIT_MAX_HP};
use crate::classic::content::GREAT_PERSON_IMPROVEMENT;

/// Hit points a unit heals by pillaging an improvement. Unciv:
/// `map/mapunit/actions/UnitActionsPillage.kt` (25, improvements only); equal.
pub const PILLAGE_HEAL_HP: u16 = 25;

/// Hit points a unit that crosses mountains loses for ending its turn on
/// one (the Phoenician Heritage text); it never dies of it.
/// Unciv: Nations.json (the mountain-crossing unique, 50 damage), equal.
pub const MOUNTAIN_TURN_DAMAGE: u16 = 50;

/// Unciv: battle/Nuke.kt (default blast radius 2), equal.
pub const NUKE_BLAST_RADIUS: i32 = 2;
/// Unciv: battle/Nuke.kt (30 plus two rolls below 40), equal.
pub const NUKE_UNIT_DAMAGE_BASE: u64 = 30;
/// Unciv: battle/Nuke.kt (30 plus two rolls below 40), equal.
pub const NUKE_UNIT_DAMAGE_RAND: u64 = 40;
pub const NUKE_NON_COMBAT_DEATH_THRESHOLD: u16 = 6;
/// Unciv: battle/Nuke.kt (half the city's health), equal.
pub const NUKE_CITY_HIT_POINT_DAMAGE: u16 = 50;
/// Unciv: battle/Nuke.kt (30 plus two rolls below 20 percent), equal.
pub const NUKE_LEVEL1_POPULATION_DEATH_BASE: u64 = 30;
/// Unciv: battle/Nuke.kt (30 plus two rolls below 20 percent), equal.
pub const NUKE_LEVEL1_POPULATION_DEATH_RAND: u64 = 20;
/// Unciv: battle/Nuke.kt (60 plus two rolls below 10 percent), equal.
pub const NUKE_LEVEL2_POPULATION_DEATH_BASE: u64 = 60;
/// Unciv: battle/Nuke.kt (60 plus two rolls below 10 percent), equal.
pub const NUKE_LEVEL2_POPULATION_DEATH_RAND: u64 = 10;
/// Unciv: battle/Nuke.kt (a strength-2 nuke destroys a city under 5 citizens), equal.
pub const NUKE_LEVEL2_ELIM_POPULATION_THRESHOLD: u32 = 5;
/// Unciv: battle/Nuke.kt (fallout chance 0.5), equal.
pub const NUKE_FALLOUT_PROB: u64 = 50;

impl<'a> WorldTransaction<'a> {
    /// Sell the exotic cargo (Portugal's Nau): the cargo promotion is used
    /// up, the owner earns the gold, the unit the experience, and its turn
    /// ends.
    pub(crate) fn sell_exotic_goods(
        &mut self,
        unit: UnitHandle,
        cargo: crate::ids::PromotionId,
        gold: u32,
        experience: u16,
    ) -> Result<(), CommandError> {
        let owner = self.world.units.get(unit).ok_or(CommandError::UnknownUnit)?.core.owner;
        if let Some(core) = self.world.units.get_mut_core(unit) {
            core.promotions.remove(cargo);
            core.moves_left = 0;
        }
        self.grant_experience(unit, experience);
        self.world.players.earn(owner, gold);
        Ok(())
    }

    /// Pillage the tile (§7.8): the improvement, else the road; gold from a
    /// foreign improvement; 25 HP; one move point (none with FreePillageMoves).
    pub(crate) fn pillage(
        &mut self,
        unit: UnitHandle,
        tile: TileId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let core = *self
            .world
            .units
            .get(unit)
            .ok_or(CommandError::UnknownUnit)?
            .core;
        let improvement = self.world.map.working_improvement(tile);
        let host = self.world.map.owner(tile).flatten();
        match improvement {
            Some(imp) => {
                let destroyed = rules
                    .improvements
                    .get(imp.0 as usize)
                    .is_some_and(|d| d.placement.destroyed_when_pillaged);
                if destroyed {
                    self.world.map.set_improvement(tile, None);
                } else {
                    self.world.map.pillage_improvement(tile);
                }
                self.journal
                    .tile_changes
                    .push(TileChange::ImprovementBuilt { tile });
                let gold = rules
                    .improvements
                    .get(imp.0 as usize)
                    .map(|d| u64::from(d.pillage_gold))
                    .unwrap_or(0);
                if gold > 0 && host != Some(core.owner) {
                    let amount =
                        self.world.rng.combat.below(gold) + self.world.rng.combat.below(gold);
                    self.world.players.earn(core.owner, amount as u32);
                }
            }
            None => {
                self.world.map.set_route(tile, crate::world::map::Route::None);
                self.journal
                    .tile_changes
                    .push(TileChange::RoadBuilt { tile });
            }
        }
        let free = unit_modifiers(self.world, rules, &core).has(UnitAbility::FreePillageMoves);
        let c = self
            .world
            .units
            .get_mut_core(unit)
            .ok_or(CommandError::UnknownUnit)?;
        if improvement.is_some() {
            c.hp = (c.hp + PILLAGE_HEAL_HP).min(UNIT_MAX_HP);
        }
        if !free {
            c.moves_left = c.moves_left.saturating_sub(MOVE_DENOMINATOR);
        }
        c.stance = crate::world::unit::Stance::Ready;
        Ok(())
    }

    /// The great general builds a citadel: the tile and the tiles around it
    /// become the owner's (to its nearest city), the general is consumed.
    pub(crate) fn build_citadel(
        &mut self,
        unit: UnitHandle,
        tile: TileId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let owner = self
            .world
            .units
            .get(unit)
            .ok_or(CommandError::UnknownUnit)?
            .core
            .owner;
        let citadel = citadel_of(rules).ok_or(CommandError::RuleViolation(
            "no citadel in the rules".into(),
        ))?;
        let radius = i32::from(rules.improvements[citadel.0 as usize].culture_bomb_radius);
        self.expend_unit(unit, rules)?;
        self.world.map.set_improvement(tile, Some(citadel));
        self.journal
            .tile_changes
            .push(TileChange::ImprovementBuilt { tile });
        let Some(city) = nearest_own_city(self.world, owner, tile) else {
            return Ok(());
        };
        let dims = self.world.map.dimensions;
        for near in crate::math::tiles_in_hex_radius(&dims, tile, radius) {
            let foreign_city = self.world.indexes.city_on_tile(near).is_some();
            if foreign_city || is_water(&self.world.map, near) && near != tile {
                continue;
            }
            let previous = self.world.map.owning_city(near);
            if previous == Some(city) {
                continue;
            }
            crate::transaction::journal::claim_tile(&mut *self.0.world, &mut self.0.journal, city, near);
        }
        Ok(())
    }

    /// End of a player's turn: its units next to an enemy citadel take the
    /// citadel's damage (§5.3).
    pub(crate) fn citadel_damage(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        if !rules.improvements.iter().any(|d| d.nearby_enemy_damage > 0) {
            return;
        }
        let units = self.world.indexes.units_of(player).to_vec();
        for unit in units {
            let Some(view) = self.world.units.get(unit) else {
                continue;
            };
            let tile = view.core.tile;
            let mut damage = 0u16;
            for near in self.world.map.dimensions.neighbors(tile) {
                let Some(imp) = self.world.map.improvement(near).flatten() else {
                    continue;
                };
                let hurt = rules
                    .improvements
                    .get(imp.0 as usize)
                    .map(|d| d.nearby_enemy_damage)
                    .unwrap_or(0);
                let host = self.world.map.owner(near).flatten();
                if hurt > 0 && host.is_some_and(|h| self.world.diplomacy.at_war(h, player)) {
                    damage = damage.max(hurt);
                }
            }
            if damage > 0 {
                let _ = self.strike_unit(unit, damage);
            }
        }
    }

    /// End of a player's turn: a land unit that may cross mountains and
    /// stands on one loses [`MOUNTAIN_TURN_DAMAGE`] hit points, at most down
    /// to one.
    pub(crate) fn mountain_damage(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        let kinds = self.world.map.terrain_kinds();
        for unit in self.world.indexes.units_of(player).to_vec() {
            let Some(core) = self.world.units.get(unit).map(|view| *view.core) else {
                continue;
            };
            let on_mountain = self.world.map.terrain(core.tile).is_some_and(|t| kinds.is_mountains(t.0));
            if !on_mountain || !unit_modifiers(self.world, rules, &core).has(UnitAbility::CrossMountains) {
                continue;
            }
            let _ = self.strike_unit(unit, MOUNTAIN_TURN_DAMAGE.min(core.hp.saturating_sub(1)));
        }
    }

    /// Nuclear strike (§2.8): units in radius 2 are hit (level 2 or the
    /// target tile: destroyed), cities lose hit points and population (a
    /// level-2 hit destroys a city under 5 pop unless an original capital),
    /// land tiles get fallout (centre always, else 50 %).
    pub(crate) fn nuke(
        &mut self,
        unit: UnitHandle,
        target: TileId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let core = *self
            .world
            .units
            .get(unit)
            .ok_or(CommandError::UnknownUnit)?
            .core;
        let level = rules
            .units
            .get(core.kind.0 as usize)
            .map(|d| d.combat.nuke_level)
            .unwrap_or(1);
        self.destroy_unit(unit)?;
        let dims = self.world.map.dimensions;
        let mut tiles: Vec<TileId> =
            crate::math::tiles_in_hex_radius(&dims, target, NUKE_BLAST_RADIUS);
        tiles.sort_by_key(|t| t.0);
        // Who is hit, before the blast takes the units: they remember it too.
        let mut victims: Vec<crate::ids::PlayerId> = Vec::new();
        for tile in &tiles {
            victims.extend(self.world.indexes.units_on(*tile).iter().filter_map(|u| self.world.units.get(*u).map(|v| v.core.owner)));
            victims.extend(self.world.indexes.city_on_tile(*tile).and_then(|c| self.world.cities.get(c)).map(|c| c.owner));
            victims.extend(self.world.map.owner(*tile).flatten());
        }
        victims.sort();
        victims.dedup();
        self.warmonger_for_nuke(core.owner, level, &victims, rules);
        for tile in &tiles {
            self.nuke_units(*tile, target, level, rules);
        }
        for tile in &tiles {
            if let Some(city) = self.world.indexes.city_on_tile(*tile) {
                self.nuke_city(city, level, rules);
            }
        }
        let fallout = rules.roles.feature(crate::rules::roles::FeatureRole::Fallout);
        for tile in tiles {
            let city = self.world.indexes.city_on_tile(tile).is_some();
            let land = crate::navigation::cost::enter_cost_on_map(&self.world.map, tile).is_some();
            if city || !land {
                continue;
            }
            if tile != target && self.world.rng.combat.below(100) >= NUKE_FALLOUT_PROB {
                continue;
            }
            if self.world.map.improvement(tile).flatten().is_some() {
                self.world.map.set_improvement(tile, None);
                self.journal
                    .tile_changes
                    .push(TileChange::ImprovementBuilt { tile });
            }
            if let Some(f) = fallout {
                self.world
                    .map
                    .set_feature(tile, Some(f));
                self.journal
                    .tile_changes
                    .push(TileChange::FeatureSet { tile });
            }
        }
        Ok(())
    }

    fn nuke_units(&mut self, tile: TileId, target: TileId, level: u8, rules: &CompiledRuleset) {
        let mut units: Vec<UnitHandle> = self.world.indexes.units_on(tile).to_vec();
        units.sort_by_key(|u| self.world.units.get(*u).map(|v| v.core.id));
        for unit in units {
            let Some(view) = self.world.units.get(unit) else {
                continue;
            };
            let civilian = rules.is_civilian(view.core.kind);
            let mut damage = if level == 1 && tile != target {
                NUKE_UNIT_DAMAGE_BASE
                    + self.world.rng.combat.below(NUKE_UNIT_DAMAGE_RAND)
                    + self.world.rng.combat.below(NUKE_UNIT_DAMAGE_RAND)
            } else {
                u64::from(UNIT_MAX_HP)
            };
            if let Some(city) = self.world.indexes.city_on_tile(tile) {
                if let Some(core) = self.world.cities.get(city) {
                    let modifier = crate::combat::city::city_combat_mods(self.world, rules, core)
                        .nuke_damage_percent;
                    damage = damage * (100 + modifier).max(0) as u64 / 100;
                }
            }
            let damage = damage.min(u64::from(u16::MAX)) as u16;
            if civilian {
                if damage >= NUKE_NON_COMBAT_DEATH_THRESHOLD {
                    let _ = self.destroy_unit(unit);
                }
                continue;
            }
            let _ = self.strike_unit(unit, damage);
        }
    }

    fn nuke_city(&mut self, city: crate::ids::CityHandle, level: u8, rules: &CompiledRuleset) {
        let Some(core) = self.world.cities.get(city) else {
            return;
        };
        let capital = core.original_capital;
        let pop = core.population;
        if !capital && (level > 2 || (level == 2 && pop < NUKE_LEVEL2_ELIM_POPULATION_THRESHOLD)) {
            self.remove_city(city, rules);
            return;
        }
        let max = crate::combat::city::max_hit_points(self.world, rules, core);
        let modifier =
            crate::combat::city::city_combat_mods(self.world, rules, core).nuke_damage_percent;
        let (base, rand) = if level >= 2 {
            (
                NUKE_LEVEL2_POPULATION_DEATH_BASE,
                NUKE_LEVEL2_POPULATION_DEATH_RAND,
            )
        } else {
            (
                NUKE_LEVEL1_POPULATION_DEATH_BASE,
                NUKE_LEVEL1_POPULATION_DEATH_RAND,
            )
        };
        let percent = base + self.world.rng.combat.below(rand) + self.world.rng.combat.below(rand);
        let lost = u64::from(pop) * percent / 100 * (100 + modifier).max(0) as u64 / 100;
        let Some(core) = self.world.cities.get_mut(city) else {
            return;
        };
        let left = max.saturating_sub(core.damage);
        let before = core.damage;
        core.damage = (core.damage + left * NUKE_CITY_HIT_POINT_DAMAGE / 100).min(max - 1);
        let damage = core.damage - before;
        core.population = (u64::from(core.population).saturating_sub(lost)).max(1) as u32;
        self.journal
            .city_changes
            .push(CityChange::PopulationChanged { city });
        if damage > 0 {
            self.journal.module_changes.push(ClassicChange::CityDamaged { city, damage });
        }
    }
}

/// The citadel improvement of the rules (a great person improvement with a
/// culture bomb).
pub fn citadel_of(rules: &CompiledRuleset) -> Option<crate::ids::ImprovementId> {
    let index = rules
        .improvements
        .iter()
        .position(|d| d.kind == GREAT_PERSON_IMPROVEMENT && d.nearby_enemy_damage > 0)?;
    Some(crate::ids::ImprovementId(index as u16))
}

fn nearest_own_city(
    auth: &crate::world::state::AuthWorld,
    owner: PlayerId,
    tile: TileId,
) -> Option<crate::ids::CityHandle> {
    let dims = auth.map().dimensions;
    auth.indexes()
        .cities_of(owner)
        .iter()
        .copied()
        .filter_map(|c| {
            auth.cities()
                .get(c)
                .map(|core| (dims.distance(core.tile, tile).unwrap_or(i32::MAX), c))
        })
        .min_by_key(|(d, c)| (*d, c.index))
        .map(|(_, c)| c)
}
