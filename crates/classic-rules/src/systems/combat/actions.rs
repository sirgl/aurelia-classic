//! Unit actions of the military: pillage (classic/docs/classic/rules.md),
//! the great general's citadel and its damage, nuclear strikes (§2.8).
//!
//! Pillage damages the improvement (`MapState::pillaged`: no yields,
//! connection or defence until a worker repairs it) or removes one that is
//! destroyed when pillaged (fishing boats, fort, offshore platform); without
//! an intact improvement it removes the road.

use crate::classic::ClassicChange;
use crate::command::CommandError;
use crate::effects::unit::unit_modifiers;
use crate::ids::{PlayerId, TileId, UnitHandle};
use crate::rules::effects::UnitAbility;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{CityChange, TileChange};
use crate::transaction::transaction::WorldTransaction;
use crate::world::relations::{GrievanceKind, STOLEN_TILE_POINTS};
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
                    if amount > 0 {
                        self.journal.module_changes.push(crate::classic::ClassicChange::Plundered {
                            player: core.owner,
                            tile,
                            source: crate::systems::plunder::PlunderSource::Improvement,
                            gold: amount as u32,
                        });
                    }
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
    /// become the owner's (`take_over_tiles`), the general is consumed.
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
        self.expend_unit(unit, rules)?;
        self.raise_citadel(owner, tile, citadel, rules);
        Ok(())
    }

    /// The citadel stands on `tile` for `owner` and its culture bomb takes the tiles around it (what
    /// `build_citadel` does once the general is consumed; the stress exam raises one with no general).
    pub(crate) fn raise_citadel(&mut self, owner: PlayerId, tile: TileId, citadel: crate::ids::ImprovementId, rules: &CompiledRuleset) {
        let radius = i32::from(rules.improvements[citadel.0 as usize].culture_bomb_radius);
        self.remove_camp_at(tile);
        self.place_improvement(owner, tile, citadel, rules);
        self.take_over_tiles(owner, tile, radius, rules);
    }

    /// A culture bomb (the citadel): every tile within `radius` of `tile`,
    /// the tile itself included, that is not a city and not already the
    /// owner's becomes the owner's, foreign land too. All go to one city: of
    /// the owner's cities that hold a tile next to or among the taken ones,
    /// the nearest to `tile` (a razing city counts 5 tiles farther), else
    /// the owner's nearest city; with no city nothing is taken. A city-state
    /// that loses a tile loses 15 influence with the taker per tile; a major
    /// that loses a tile holds a grievance against the taker, 10 opinion
    /// points per tile, fading by itself (`GrievanceKind::StolenLand`).
    /// Unciv: UniqueTriggerActivation.kt `OneTimeTakeOverTilesInRadius`
    /// (Citadel: "Gain control over [all] tiles in a [1]-tile radius"; the
    /// `StealingTerritory` modifier, −10 per tile; −15 influence of a
    /// city-state); equal. Third nations hold nothing (as in Unciv).
    pub(crate) fn take_over_tiles(&mut self, owner: PlayerId, tile: TileId, radius: i32, rules: &CompiledRuleset) {
        if radius < 0 {
            return;
        }
        let dims = self.world.map.dimensions;
        let taken: Vec<TileId> = crate::math::hex_disc(dims, tile, radius)
            .filter(|&t| self.world.indexes.city_on_tile(t).is_none() && self.world.map.owner(t).flatten() != Some(owner))
            .collect();
        if taken.is_empty() {
            return;
        }
        let cities = take_over_cities(self.world, owner, tile, &taken);
        let mut guests = Vec::new();
        for near in taken {
            // A tile goes to the first city, in the order of Unciv's `cityPriority`, that holds it
            // within its culture radius; a tile no city of the owner reaches stays as it is (a
            // city's tiles lie within `CULTURE_RADIUS_MAX`, `validate` and the capture of a city
            // rely on it).
            let Some(city) = cities.iter().copied().find(|c| {
                self.world.cities.get(*c).is_some_and(|core| dims.distance(core.tile, near).is_some_and(|d| d <= CITADEL_CITY_REACH))
            }) else {
                continue;
            };
            let from = self.world.map.owner(near).flatten();
            crate::transaction::journal::claim_tile(&mut *self.0.world, &mut self.0.journal, city, near);
            if let Some(victim) = from.filter(|&v| crate::world::player::ClassicPlayers::is_city_state(&self.world.players, v)) {
                self.change_influence(victim, owner, -CITADEL_CITY_STATE_INFLUENCE * 100, rules);
            }
            if let Some(victim) = from {
                self.add_grievance(victim, owner, GrievanceKind::StolenLand, STOLEN_TILE_POINTS);
            }
            // Unciv takeOwnership: a barbarian encampment on the tile is removed.
            self.remove_camp_at(near);
            for unit in self.world.indexes.units_on(near).to_vec() {
                if let Some(guest) = self.world.units.get(unit).map(|u| u.core.owner).filter(|g| *g != owner) {
                    if !guests.contains(&guest) {
                        guests.push(guest);
                    }
                }
            }
        }
        // ... and a unit that may no longer stand there goes to the nearest tile it may enter.
        for guest in guests {
            self.expel_units(owner, guest, rules);
        }
    }

    /// End of a player's turn, after its moves and healing: its units next
    /// to an enemy citadel take the citadel's damage (§5.3) — the strongest
    /// one, they do not add up; a pillaged citadel does not hurt. A unit
    /// that walks away during its turn is not hurt. Unciv:
    /// UnitTurnManager.kt `doCitadelDamage` (in `endTurn`, after
    /// `healUnit`; "Adjacent enemy units ending their turn take [30]
    /// damage"); equal.
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
                if self.world.map.pillaged(near) {
                    continue;
                }
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
        let pop = core.population;
        // Unciv Nuke.doNukeExplosionDamageToCity: `canBeDestroyed(true)`; an original capital or a
        // holy city survives with the damage and the losses below.
        let destroyed = level > 2 || (level == 2 && pop < NUKE_LEVEL2_ELIM_POPULATION_THRESHOLD);
        if destroyed && crate::combat::city::can_be_destroyed(self.world, rules, city, true) {
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

/// Influence a city-state loses with the player whose culture bomb takes one
/// of its tiles. Unciv: UniqueTriggerActivation.kt
/// (`OneTimeTakeOverTilesInRadius`, `addInfluence(-15f)`); equal.
pub const CITADEL_CITY_STATE_INFLUENCE: i32 = 15;

/// Furthest distance from a city of the owner at which a culture bomb gives that city a tile: the
/// city's culture radius (`CULTURE_RADIUS_MAX`; a city's tiles lie within it, `validate` and the
/// capture of a city rely on it). Differs from Unciv: Unciv gives the tile to the city nearest to
/// the citadel whatever the distance, here a tile no city reaches stays as it was, so a citadel 5
/// tiles from its city takes its own tile and the tiles around it that lie within 5 of the city,
/// not the ones at 6 (`tests/review_citadel_holes2.rs`, `tests/citadel_opinion.rs`).
pub const CITADEL_CITY_REACH: i32 = crate::world::city::CULTURE_RADIUS_MAX as i32;

/// Extra distance of a razing city when the culture bomb picks the city
/// that gets the tiles. Unciv: UniqueTriggerActivation.kt (`cityPriority`);
/// equal.
pub const RAZING_CITY_EXTRA_DISTANCE: i32 = 5;

/// The cities that may get the tiles of a culture bomb at `tile` (see `take_over_tiles`), the best
/// first: the nearest own city holding a tile among `taken` or next to one, then the other own
/// cities by the same distance (ties: the older city).
fn take_over_cities(
    auth: &crate::world::state::AuthWorld,
    owner: PlayerId,
    tile: TileId,
    taken: &[TileId],
) -> Vec<crate::ids::CityHandle> {
    let dims = auth.map().dimensions;
    let priority = |c: crate::ids::CityHandle| {
        let core = auth.cities().get(c)?;
        let razing = core.status == crate::world::city::CityStatus::Razing;
        let distance = dims.distance(core.tile, tile).unwrap_or(i32::MAX / 2);
        Some((distance + if razing { RAZING_CITY_EXTRA_DISTANCE } else { 0 }, c.index))
    };
    let mut bordering: Vec<crate::ids::CityHandle> = taken
        .iter()
        .flat_map(|&t| dims.neighbors(t).into_iter().chain(std::iter::once(t)))
        .filter(|&t| auth.map().owner(t).flatten() == Some(owner))
        .filter_map(|t| auth.map().owning_city(t))
        .collect();
    bordering.sort_by_key(|c| c.index);
    bordering.dedup();
    let mut ranked: Vec<((i32, u32), crate::ids::CityHandle)> = Vec::new();
    let near = bordering.clone();
    let others: Vec<_> = auth.indexes().cities_of(owner).iter().copied().filter(|c| !near.contains(c)).collect();
    for group in [bordering, others] {
        let mut part: Vec<_> = group.into_iter().filter_map(|c| priority(c).map(|p| (p, c))).collect();
        part.sort_by_key(|(p, _)| *p);
        ranked.extend(part);
    }
    ranked.into_iter().map(|(_, c)| c).collect()
}
