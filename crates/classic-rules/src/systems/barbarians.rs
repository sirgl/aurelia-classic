//! Barbarians: camps appear in land no major sees, spawn units, and are
//! cleared for gold by whoever walks in. The values are our own;
//! classic/docs/classic/rules.md has no barbarian section, so each constant below
//! says how far it was compared with a public source.
//!
//! The barbarian player is a player of its own kind (always at war with
//! everyone, no diplomacy, no experience, no healing). Its turn runs the
//! `Barbarians` step: learn the technologies most majors know, place camps,
//! count down the camps' spawns. Its units act by commands like everyone's
//! (`ai::barbarians` decides them).

use crate::ids::{PlayerId, TileId, UnitKindId};
use crate::rules::content::{Domain, UnitRole};
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{PlayerChange, TileChange};
use crate::transaction::transaction::WorldTransaction;
use crate::world::barbarians::{BarbarianMode, Camp, ClearedCamp};
use crate::world::player::ClassicPlayers;
use crate::classic::content::BARBARIAN_CAMP;
use crate::classic::ClassicChange;

/// The first barbarian turn places the target camp count divided by this (at
/// least one). Unciv: BarbarianManager.placeBarbarianEncampment, a third on
/// the first turn; equal.
pub const FIRST_WAVE_DIVISOR: usize = 3;
/// A new camp is placed on one turn in this many (one in two). Unciv:
/// BarbarianManager.placeBarbarianEncampment, a coin flip per turn; equal.
pub const NEW_CAMP_ODDS: u64 = 2;
/// A camp is never within this many tiles of a major nation's capital
/// (other cities do not count). Unciv: BarbarianManager.placeBarbarianEncampment,
/// 4 tiles from major capitals; equal.
pub const MIN_DISTANCE_CITY: i32 = 4;
/// Least distance (tiles) between two camps. Unciv: BarbarianManager, 7
/// tiles from a live camp; equal.
pub const MIN_DISTANCE_CAMP: i32 = 7;
/// Unseen land tiles needed per camp: the map's tile count to the power 0.4,
/// rounded down (27 on a 40 x 100 map, 28 on 80 x 52). Unciv:
/// BarbarianManager.placeBarbarianEncampment (`fogTilesPerCamp`); equal.
/// Computed in integers (the largest k with k^5 <= tiles^2), bit-exact on
/// every platform.
pub fn fog_tiles_per_camp(tiles: usize) -> usize {
    let square = (tiles as u128) * (tiles as u128);
    let mut k: u128 = 1;
    while (k + 1).pow(5) <= square {
        k += 1;
    }
    k as usize
}
/// A camp spawns a wandering unit only while at most this many barbarian
/// units stand within `NEARBY_RANGE` tiles (a third may come). Unciv:
/// BarbarianManager.spawnBarbarian (no spawn when more than 2 within 4
/// tiles); equal.
pub const MAX_NEARBY: usize = 2;
/// Unciv: BarbarianManager.spawnBarbarian (4 tiles), equal.
pub const NEARBY_RANGE: i32 = 4;
/// Gold for clearing a camp at the default difficulty level. Unciv:
/// Difficulties.json (Gods & Kings ruleset), clearBarbarianCampReward of
/// Prince, 25; equal.
pub const CAMP_GOLD: u32 = 25;
/// First round in which camps spawn wandering units; camps and their defenders
/// appear from the first barbarian turn. Unciv: BarbarianManager.spawnBarbarian
/// (no wandering units before turn 10, a defender for an empty camp at
/// once); equal.
pub const EARLIEST_RELEASE_ROUND: u32 = 10;
/// Turns between two spawns of a camp: `SPAWN_DELAY_BASE + rand(SPAWN_DELAY_RANDOM)`,
/// halved with raging barbarians, plus the difficulty's delay. Unciv:
/// `BarbarianEncampment.resetCountdown` (8 + 0..4, raging / 2, `Difficulties.json`
/// `barbarianSpawnDelay`); equal, except that Unciv also shortens the delay by
/// the camp's spawned units (at most 3), which our camps do not count.
pub const SPAWN_DELAY_BASE: u8 = 8;
/// Unciv: BarbarianEncampment.resetCountdown (0..4), equal.
pub const SPAWN_DELAY_RANDOM: u64 = 5;
/// Extra spawn delay by the game's difficulty level (Settler .. Deity).
/// Unciv: `Difficulties.json` (`barbarianSpawnDelay` 8 / 5 / 3 / 0 ...); equal.
pub const SPAWN_DELAY_BY_LEVEL: [u8; 8] = [8, 5, 3, 0, 0, 0, 0, 0];
/// Gold of a converted camp unit (the victor's barbarian-conversion stat,
/// the trait text: 25 gold).
/// Unciv: Nations.json (barbarian conversion, 25 gold), equal.
pub const CONVERSION_GOLD: u32 = 25;
/// Cleared camps are remembered this long (city-state quests).
pub const CLEARED_MEMORY_ROUNDS: u32 = 30;

impl<'a> WorldTransaction<'a> {
    /// The barbarian player's turn step (no-op for everybody else).
    pub(crate) fn barbarian_turn(
        &mut self,
        player: PlayerId,
        rules: &CompiledRuleset,
        derived: &crate::derived::DerivedWorld,
    ) {
        if !self.world.players.is_barbarian(player)
            || self.world.module.barbarians.mode == BarbarianMode::None
        {
            return;
        }
        self.barbarian_techs(player);
        self.place_camps(player, rules, derived);
        self.camp_spawns(player, rules);
        let round = self.world.turn.round;
        self.world
            .module
            .barbarians
            .cleared
            .retain(|c| round < c.round + CLEARED_MEMORY_ROUNDS);
    }

    /// Barbarians know every technology that half the alive majors know.
    fn barbarian_techs(&mut self, player: PlayerId) {
        let majors: Vec<PlayerId> = (0..self.world.players.count())
            .map(|p| PlayerId(p as u8))
            .filter(|p| self.world.players.is_major(*p) && self.world.players.is_alive(*p))
            .collect();
        if majors.is_empty() {
            return;
        }
        let mut counts: std::collections::BTreeMap<u16, usize> = Default::default();
        for p in &majors {
            for t in self.world.players.techs_of(*p) {
                *counts.entry(t.0).or_default() += 1;
            }
        }
        for (tech, n) in counts {
            if n * 2 >= majors.len() {
                let _ = self
                    .world
                    .players
                    .learn_tech(player, crate::ids::TechId(tech));
            }
        }
    }

    /// New camps in fog (§ module docs): the first barbarian turn a third
    /// of the target, then one in two turns while below the target.
    fn place_camps(
        &mut self,
        player: PlayerId,
        rules: &CompiledRuleset,
        derived: &crate::derived::DerivedWorld,
    ) {
        let Some(camp_kind) = camp_improvement(rules) else {
            return;
        };
        let (fog, mut candidates) = self.camp_candidates(rules, derived, camp_kind);
        let target = fog / fog_tiles_per_camp(self.world.map.tile_count());
        let have = self.world.module.barbarians.camps.len();
        let to_add = if !self.world.module.barbarians.released {
            self.world.module.barbarians.released = true;
            (target.saturating_sub(have) / FIRST_WAVE_DIVISOR).max(1)
        } else if have < target && self.world.module.barbarians.rng.below(NEW_CAMP_ODDS) == 0 {
            1
        } else {
            0
        };
        for _ in 0..to_add {
            if candidates.is_empty() {
                break;
            }
            let pick = self.world.module.barbarians.rng.below(candidates.len() as u64) as usize;
            let tile = candidates.swap_remove(pick);
            let dims = self.world.map.dimensions;
            candidates.retain(|t| dims.distance(*t, tile).unwrap_or(i32::MAX) >= MIN_DISTANCE_CAMP);
            self.found_camp(player, tile, camp_kind, rules);
        }
    }

    /// (Fog tiles, valid camp tiles): land no major or city-state sees;
    /// valid camp tiles also: unowned, free, the camp's terrain, far from
    /// major capitals and other camps. Sorted by tile id.
    fn camp_candidates(
        &self,
        rules: &CompiledRuleset,
        derived: &crate::derived::DerivedWorld,
        camp: crate::ids::ImprovementId,
    ) -> (usize, Vec<TileId>) {
        let map = &self.world.map;
        let dims = map.dimensions;
        let watchers: Vec<PlayerId> = (0..self.world.players.count())
            .map(|p| PlayerId(p as u8))
            .filter(|p| !self.world.players.is_barbarian(*p))
            .collect();
        let def = &rules.improvements[camp.0 as usize];
        let capitals: Vec<TileId> = watchers
            .iter()
            .filter(|p| self.world.players.is_major(**p))
            .filter_map(|p| crate::combat::city::capital_of(self.world, rules, *p))
            .filter_map(|c| self.world.cities.get(c).map(|core| core.tile))
            .collect();
        let mut fog = 0;
        let mut out = Vec::new();
        for i in 0..map.tile_count() {
            let tile = TileId(i as u32);
            if crate::navigation::cost::enter_cost_on_map(map, tile).is_none() {
                continue;
            }
            if watchers
                .iter()
                .any(|p| crate::knowledge::view::is_visible(derived, *p, tile))
            {
                continue;
            }
            fog += 1;
            let free = map.owner(tile).flatten().is_none()
                && map.improvement(tile).flatten().is_none()
                && self.world.indexes.units_on(tile).is_empty()
                && map.natural_wonder(tile).is_none();
            if !free || !camp_terrain_ok(rules, map, tile, def) {
                continue;
            }
            let near_city = capitals
                .iter()
                .any(|c| dims.distance(*c, tile).unwrap_or(i32::MAX) <= MIN_DISTANCE_CITY);
            let near_camp = self
                .world
                .module
                .barbarians
                .camps
                .iter()
                .any(|c| dims.distance(c.tile, tile).unwrap_or(i32::MAX) < MIN_DISTANCE_CAMP);
            if !near_city && !near_camp {
                out.push(tile);
            }
        }
        (fog, out)
    }

    fn found_camp(
        &mut self,
        player: PlayerId,
        tile: TileId,
        camp: crate::ids::ImprovementId,
        rules: &CompiledRuleset,
    ) {
        self.world.map.set_improvement(tile, Some(camp));
        self.journal
            .tile_changes
            .push(TileChange::ImprovementBuilt { tile });
        self.notice_camp(tile, rules);
        let spawn_in = self.spawn_delay(rules);
        self.world.module.barbarians.camps.push(Camp { tile, spawn_in });
        if let Some(kind) = barbarian_unit(self.world, rules, player, Domain::Land) {
            self.spawn_unit(player, tile, kind, rules);
        }
    }

    /// Players who see new camps (`PlayerRule::SeeBarbarianCamps`,
    /// Honor) remember the camp's tile if they explored it.
    fn notice_camp(&mut self, tile: TileId, rules: &CompiledRuleset) {
        for p in 0..self.world.players.count() {
            let player = PlayerId(p as u8);
            if !self.world.players.is_major(player) {
                continue;
            }
            let mods = crate::effects::player::player_modifiers(self.world, rules, player);
            let rule = crate::rules::stats::Stat::Module(crate::rules::stats::ClassicStat::Rule(crate::rules::stats::PlayerRule::SeeBarbarianCamps));
            if mods.stats.get(rule) > 0 {
                self.journal
                    .player_changes
                    .push(PlayerChange::TileNoticed { player, tile });
            }
        }
    }

    /// Harness-only: an empty camp on `tile` (no defender).
    #[cfg(any(test, feature = "harness"))]
    pub(crate) fn place_empty_camp(&mut self, tile: TileId, rules: &CompiledRuleset) {
        let Some(camp) = camp_improvement(rules) else {
            return;
        };
        self.world.map.set_improvement(tile, Some(camp));
        self.journal
            .tile_changes
            .push(TileChange::ImprovementBuilt { tile });
        self.notice_camp(tile, rules);
        let spawn_in = self.spawn_delay(rules);
        self.world.module.barbarians.camps.push(Camp { tile, spawn_in });
    }

    fn spawn_delay(&mut self, rules: &CompiledRuleset) -> u8 {
        let mut delay = SPAWN_DELAY_BASE + self.world.module.barbarians.rng.below(SPAWN_DELAY_RANDOM) as u8;
        if self.world.module.barbarians.mode == BarbarianMode::Raging {
            delay /= 2;
        }
        let level = self.world.settings().level_index(rules);
        let delay = delay + level.and_then(|l| SPAWN_DELAY_BY_LEVEL.get(usize::from(l)).copied()).unwrap_or(0);
        // × the game speed's barbarian percent (Unciv: resetCountdown, `barbarianModifier`, `toInt`; equal).
        let percent = self.world.settings().speed_values().barbarian_percent;
        if percent == 100 {
            return delay;
        }
        kernel::rules::speeds::SpeedValues::scale(u32::from(delay), percent).clamp(1, u32::from(u8::MAX)) as u8
    }

    /// Each camp counts down; at zero it spawns a defender if the camp is
    /// empty, else (from `EARLIEST_RELEASE_ROUND` on) a wandering unit (a
    /// ship at a coastal camp one time in six) unless more than two
    /// barbarians are near. A camp that could not spawn tries again next
    /// turn (Unciv: `BarbarianEncampment.update`).
    fn camp_spawns(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        let camps = self.world.module.barbarians.camps.clone();
        for (i, camp) in camps.iter().enumerate() {
            let left = camp.spawn_in.saturating_sub(1);
            if left > 0 {
                self.world.module.barbarians.camps[i].spawn_in = left;
                continue;
            }
            // Retry next turn unless a unit spawns below.
            self.world.module.barbarians.camps[i].spawn_in = 1;
            let empty = !self.world.indexes.units_on(camp.tile).iter().any(|u| {
                self.world.units.get(*u).is_some_and(|v| v.core.owner == player && !rules.is_civilian(v.core.kind))
            });
            if empty {
                if let Some(kind) = barbarian_unit(self.world, rules, player, Domain::Land) {
                    self.spawn_unit(player, camp.tile, kind, rules);
                    self.world.module.barbarians.camps[i].spawn_in = self.spawn_delay(rules);
                }
                continue;
            }
            if self.world.turn.round < EARLIEST_RELEASE_ROUND {
                continue;
            }
            if self.barbarians_near(player, camp.tile) > MAX_NEARBY {
                continue;
            }
            let coastal = self
                .world
                .map
                .dimensions
                .neighbors(camp.tile)
                .into_iter()
                .find(|t| crate::navigation::terrain::is_water(&self.world.map, *t));
            let sea = coastal.is_some() && self.world.module.barbarians.rng.below(6) == 0;
            let (domain, at) = match (sea, coastal) {
                (true, Some(water)) => (Domain::Sea, water),
                _ => (Domain::Land, camp.tile),
            };
            let Some(kind) = barbarian_unit(self.world, rules, player, domain) else {
                continue;
            };
            let spot = self.free_tile_near(player, at, kind, rules);
            if crate::world::stacking::can_stop_at(self.world, rules, player, kind, None, spot)
                .is_ok()
            {
                self.spawn_unit(player, spot, kind, rules);
                self.world.module.barbarians.camps[i].spawn_in = self.spawn_delay(rules);
            }
        }
    }

    fn barbarians_near(&self, player: PlayerId, tile: TileId) -> usize {
        let dims = self.world.map.dimensions;
        self.world
            .indexes
            .units_of(player)
            .iter()
            .filter(|u| {
                self.world.units.get(**u).is_some_and(|v| {
                    dims.distance(v.core.tile, tile).unwrap_or(i32::MAX) <= NEARBY_RANGE
                })
            })
            .count()
    }

    /// Furor Teutonicus: a land barbarian unit killed inside its camp joins
    /// the victor with the chance of the victor's
    /// barbarian-conversion stat, who also earns `CONVERSION_GOLD`.
    /// The unit comes back with `CAPTURED_UNIT_HP` and no moves. The
    /// roll is drawn only where the rule can apply.
    pub(crate) fn convert_camp_unit(&mut self, victor: PlayerId, victim: crate::systems::effects::Victim, rules: &CompiledRuleset) {
        let on_camp = self.world.module.barbarians.camps.iter().any(|c| c.tile == victim.tile);
        let land = rules.units.get(victim.kind.0 as usize).is_some_and(|d| d.domain == Domain::Land);
        if !on_camp || !land || !self.world.players.is_barbarian(victim.owner) || !self.world.players.is_major(victor) {
            return;
        }
        let percent = crate::effects::player::player_modifiers(self.world, rules, victor)
            .stats
            .get(crate::rules::stats::Stat::Module(crate::rules::stats::ClassicStat::LandBarbarianConversionPercent));
        if percent <= 0 || self.world.rng.combat.below(100) as i32 >= percent {
            return;
        }
        self.world.players.earn(victor, CONVERSION_GOLD);
        self.journal.module_changes.push(ClassicChange::CampUnitConverted { player: victor, tile: victim.tile, gold: CONVERSION_GOLD });
        let spot = self.free_tile_near(victor, victim.tile, victim.kind, rules);
        let handle = self.spawn_unit(victor, spot, victim.kind, rules);
        if let Some(core) = self.world.units.get_mut_core(handle) {
            core.hp = crate::systems::combat::CAPTURED_UNIT_HP;
            core.moves_left = 0;
        }
    }

    /// A unit of a nation or city-state entered a barbarian camp: the
    /// camp is gone and the player earns the camp gold (times its plunder
    /// percent: Songhai triple, `systems::plunder`).
    pub(crate) fn clear_camp_at(&mut self, player: PlayerId, tile: TileId, rules: &CompiledRuleset) {
        if self.world.players.is_barbarian(player) {
            return;
        }
        let Some(index) = self
            .world
            .module
            .barbarians
            .camps
            .iter()
            .position(|c| c.tile == tile)
        else {
            return;
        };
        self.world.module.barbarians.camps.remove(index);
        self.world.map.set_improvement(tile, None);
        self.journal
            .tile_changes
            .push(TileChange::ImprovementBuilt { tile });
        let round = self.world.turn.round;
        self.world.module.barbarians.cleared.push(ClearedCamp {
            tile,
            player,
            round,
        });
        let mut gold = 0;
        if self.world.players.is_major(player) {
            let percent = crate::systems::plunder::plunder_percent(self.world, rules, player);
            // × the game speed's gold percent (Unciv: MapUnit, camp gold × `goldCostModifier`; equal).
            let base = kernel::rules::speeds::SpeedValues::scale(CAMP_GOLD, self.world.settings().speed_values().gold_percent);
            gold = crate::systems::plunder::scaled_by_percent(base, percent);
            self.world.players.earn(player, gold);
        }
        self.journal.module_changes.push(ClassicChange::CampCleared { player, tile, gold });
    }
}

/// The barbarian camp improvement of the rules.
pub fn camp_improvement(rules: &CompiledRuleset) -> Option<crate::ids::ImprovementId> {
    let i = rules
        .improvements
        .iter()
        .position(|d| d.kind == BARBARIAN_CAMP)?;
    Some(crate::ids::ImprovementId(i as u16))
}

fn camp_terrain_ok(
    rules: &CompiledRuleset,
    map: &crate::world::map::MapState,
    tile: TileId,
    def: &crate::rules::compiled::ImprovementDefinition,
) -> bool {
    if let Some(feature) = map.feature(tile).flatten() {
        return def.valid_features.is_empty() || def.valid_features.contains(&feature);
    }
    if def.valid_terrains.is_empty() {
        return true;
    }
    crate::navigation::terrain::terrain_kind(rules, map, tile)
        .is_some_and(|t| def.valid_terrains.contains(&t))
}

/// The strongest regular unit of `domain` the barbarians can field: a
/// fighting role, buildable, its technology known, not obsolete for them.
/// (The barbarian-only variants — brute, galley — are not in the catalog:
/// barbarians field the regular units [approximation].)
pub fn barbarian_unit(
    auth: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    domain: Domain,
) -> Option<UnitKindId> {
    let techs = auth.players().techs_of(player);
    let mut best: Option<(u16, UnitKindId)> = None;
    for (i, def) in rules.units.iter().enumerate() {
        let fighting = matches!(
            def.role,
            Some(UnitRole::Melee | UnitRole::Archer | UnitRole::Mounted | UnitRole::NavalMelee)
        );
        if !fighting || def.domain != domain || def.unbuildable || def.nation.is_some() {
            continue;
        }
        if def.required_tech.is_some_and(|t| !techs.contains(&t)) {
            continue;
        }
        if def.obsolete_tech.is_some_and(|t| techs.contains(&t)) {
            continue;
        }
        let kind = UnitKindId(i as u16);
        if best.map(|(s, _)| def.strength > s).unwrap_or(true) {
            best = Some((def.strength, kind));
        }
    }
    best.map(|(_, k)| k)
}

#[cfg(test)]
mod tests {
    use super::fog_tiles_per_camp;

    /// Unciv `fogTilesPerCamp = (tiles ^ 0.4).toInt()`.
    #[test]
    fn fog_tiles_per_camp_is_the_tile_count_to_the_power_0_4() {
        assert_eq!(fog_tiles_per_camp(600), 12);
        assert_eq!(fog_tiles_per_camp(4000), 27);
        assert_eq!(fog_tiles_per_camp(4160), 28);
        for n in [1usize, 2, 31, 32, 33, 243, 1024, 1536, 2288, 6000, 10_240] {
            assert_eq!(fog_tiles_per_camp(n), (n as f64).powf(0.4) as usize, "n={n}");
        }
    }
}
