//! Rebels: an empire whose excess happiness is below -20
//! (`derived::happiness::REBELLION_THRESHOLD`) sees barbarian units rise
//! in its land every few turns. Unciv 4.22.10:
//! `core/src/com/unciv/logic/civilization/managers/TurnManager.kt`
//! (`updateRevolts`, `doRevoltSpawn`, `rateTileForRevoltSpawn`,
//! `getTurnsBeforeRevolt`, lines 179-252) with the global unique
//! `Rebel units may spawn <when below [-20] [Happiness]>` of
//! `GlobalUniques.json` and `ModConstants.baseTurnsUntilRevolt` (4).
//!
//! In the player's resolution, after the happiness snapshot (the
//! `Barbarians` step): a running countdown goes down by one and at zero
//! the rebels rise, whatever the happiness of this turn (Unciv counts the
//! flag down before it looks at the happiness again). Then a player
//! below the threshold with no countdown starts one; a player at or
//! above it loses its countdown. Nothing happens in a game without
//! barbarians, and only nations rebel (city-states keep no happiness).
//!
//! The rising: `1 + rand(100 + 20 × (cities - 1)) / 100` units of one
//! kind, drawn uniformly from the regular land units of the rules that
//! fight in melee and that the unhappy player could build now (its
//! technology, not obsolete for it, its strategic resources, not replaced
//! by its own unique unit): the units of its era. The city is drawn with
//! weight `population + 10` (the largest of `rand(population + 10)`);
//! the tile is the city's own tile that rates best (free land off the
//! city centre: 10, +4 unimproved, +3 more with a resource, +4 with a
//! defence bonus), and each rebel stands on the nearest free land tile
//! from there. The rebels belong to the barbarian player, whose AI moves
//! them like any barbarian.

use crate::classic::ClassicChange;
use crate::ids::{CityHandle, PlayerId, TileId, UnitKindId};
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::barbarians::BarbarianMode;
use crate::world::player::ClassicPlayers;

/// Turns from the first turn below the threshold to the rising, before
/// the random part and the game speed. Unciv: `ModConstants.baseTurnsUntilRevolt`
/// (4); equal.
pub const REVOLT_DELAY_BASE: u32 = 4;
/// The random part of the delay: `rand(3)` more turns. Unciv:
/// TurnManager.getTurnsBeforeRevolt (`nextInt(3)`); equal.
pub const REVOLT_DELAY_RANDOM: u64 = 3;
/// Percent of the spawn roll per city beyond the first: one rebel, and one
/// more with the chance `20 × (cities - 1)` percent (a third from 11
/// cities). Unciv: TurnManager.doRevoltSpawn
/// (`1 + nextInt(100 + 20 * (cities - 1)) / 100`); equal.
pub const REBEL_PERCENT_PER_CITY: u64 = 20;
/// Weight of a city in the draw of the rising city on top of its
/// population. Unciv: TurnManager.doRevoltSpawn (`nextInt(population + 10)`); equal.
pub const REBEL_CITY_WEIGHT: u64 = 10;
/// Rate of a free land tile for the rising, and its additions: no
/// improvement, a resource on an unimproved tile, a defence bonus.
/// Unciv: TurnManager.rateTileForRevoltSpawn (10, +4, +3, +4); equal.
pub const REBEL_TILE_BASE: i32 = 10;
/// Unciv: TurnManager.rateTileForRevoltSpawn (+4 unimproved); equal.
pub const REBEL_TILE_UNIMPROVED: i32 = 4;
/// Unciv: TurnManager.rateTileForRevoltSpawn (+3 resource on an unimproved tile); equal.
pub const REBEL_TILE_RESOURCE: i32 = 3;
/// Unciv: TurnManager.rateTileForRevoltSpawn (+4 defence bonus); equal.
pub const REBEL_TILE_DEFENCE: i32 = 4;
/// Rings searched around the rising tile for a free tile per rebel.
/// Unciv: TileMap.placeUnitNearTile (10 tries over the neighbours of the
/// tiles tried); equal.
pub const REBEL_PLACE_RINGS: usize = 10;

impl<'a> WorldTransaction<'a> {
    /// The rebellion step of a nation's resolution (§ module docs).
    pub(crate) fn revolt_turn(&mut self, player: PlayerId, rules: &CompiledRuleset) {
        if !self.world.players.is_major(player) {
            return;
        }
        let Some(barbarian) = self.world.players.barbarian() else {
            return;
        };
        if self.world.module.barbarians.mode == BarbarianMode::None {
            return;
        }
        if let Some(left) = self.world.module.barbarians.revolt_countdown(player) {
            let left = left.saturating_sub(1);
            if left == 0 {
                self.world.module.barbarians.set_revolt(player, None);
                self.rise(player, barbarian, rules);
            } else {
                self.world.module.barbarians.set_revolt(player, Some(left));
            }
        }
        let excess = self
            .world
            .players
            .economy_of(player)
            .map_or(0, |e| e.excess_happiness);
        if !crate::derived::happiness::uprising_possible(excess) {
            self.world.module.barbarians.set_revolt(player, None);
        } else if self.world.module.barbarians.revolt_countdown(player).is_none() {
            let delay = self.revolt_delay();
            self.world.module.barbarians.set_revolt(player, Some(delay));
        }
    }

    /// `(REVOLT_DELAY_BASE + rand(REVOLT_DELAY_RANDOM))` × the game pace,
    /// never shortened (Unciv: `speed.modifier.coerceAtLeast(1f)`), at
    /// least one turn.
    fn revolt_delay(&mut self) -> u8 {
        let turns = REVOLT_DELAY_BASE + self.world.module.barbarians.rng.below(REVOLT_DELAY_RANDOM) as u32;
        let pace = self.world.settings().speed_values().pace_percent.max(100);
        kernel::rules::speeds::SpeedValues::scale(turns, pace).clamp(1, u32::from(u8::MAX)) as u8
    }

    /// Rebels of `player`'s era rise near one of its cities for the
    /// barbarian player (§ module docs).
    fn rise(&mut self, player: PlayerId, barbarian: PlayerId, rules: &CompiledRuleset) {
        let cities: Vec<CityHandle> = self.world.indexes.cities_of(player).to_vec();
        if cities.is_empty() {
            return;
        }
        let rng = &mut self.world.module.barbarians.rng;
        let span = 100 + REBEL_PERCENT_PER_CITY * (cities.len() as u64 - 1);
        let count = 1 + rng.below(span) / 100;
        // The largest roll wins; the first city on a tie.
        let mut city: Option<(u64, CityHandle)> = None;
        for handle in &cities {
            let population = self.world.cities.get(*handle).map_or(0, |c| u64::from(c.population));
            let roll = self.world.module.barbarians.rng.below(population + REBEL_CITY_WEIGHT);
            if city.is_none_or(|(best, _)| roll > best) {
                city = Some((roll, *handle));
            }
        }
        let Some((_, city)) = city else { return };
        let Some(tile) = self.rebel_tile(city, player, rules) else {
            return;
        };
        let kinds = rebel_kinds(self.world, rules, player);
        if kinds.is_empty() {
            return;
        }
        let kind = kinds[self.world.module.barbarians.rng.below(kinds.len() as u64) as usize];
        let mut risen = 0u8;
        for _ in 0..count {
            let Some(spot) = self.rebel_spot(barbarian, tile, kind, rules) else {
                break;
            };
            self.spawn_unit(barbarian, spot, kind, rules);
            risen += 1;
        }
        if risen > 0 {
            self.journal.module_changes.push(ClassicChange::RebelsRose { player, city, tile, kind, count: risen });
        }
    }

    /// The city's own tile that rates best for the rising; the first in
    /// ring order on a tie (Unciv: `maxByOrNull` over the city's tiles).
    fn rebel_tile(&self, city: CityHandle, player: PlayerId, rules: &CompiledRuleset) -> Option<TileId> {
        let core = self.world.cities.get(city)?;
        let map = &self.world.map;
        let mut best: Option<(i32, TileId)> = None;
        for tile in crate::math::hex_disc(map.dimensions, core.tile, crate::world::city::MAX_TERRITORY_RADIUS) {
            if map.owning_city(tile) != Some(city) {
                continue;
            }
            let rate = self.rebel_tile_rate(tile, player, rules);
            if best.is_none_or(|(b, _)| rate > b) {
                best = Some((rate, tile));
            }
        }
        best.map(|(_, t)| t)
    }

    /// Unciv: TurnManager.rateTileForRevoltSpawn; -1 for water, a tile
    /// with any unit, the city centre and impassable land.
    fn rebel_tile_rate(&self, tile: TileId, player: PlayerId, rules: &CompiledRuleset) -> i32 {
        let map = &self.world.map;
        let centre = self.world.cities.iter().any(|(_, c)| c.tile == tile);
        if crate::navigation::terrain::is_water(map, tile)
            || !self.world.indexes.units_on(tile).is_empty()
            || centre
            || crate::navigation::cost::enter_cost_on_map(map, tile).is_none()
        {
            return -1;
        }
        let mut rate = REBEL_TILE_BASE;
        if map.improvement(tile).flatten().is_none() {
            rate += REBEL_TILE_UNIMPROVED;
            if map.resource(tile).flatten().is_some() {
                rate += REBEL_TILE_RESOURCE;
            }
        }
        if crate::combat::terrain::tile_defense_percent(self.world, rules, tile, player) > 0 {
            rate += REBEL_TILE_DEFENCE;
        }
        rate
    }

    /// The nearest land tile from `tile` where a rebel of `kind` may stand:
    /// the tile itself, then outward over passable tiles, at most
    /// `REBEL_PLACE_RINGS` rings (Unciv: TileMap.placeUnitNearTile). None:
    /// that rebel does not rise.
    fn rebel_spot(&self, barbarian: PlayerId, tile: TileId, kind: UnitKindId, rules: &CompiledRuleset) -> Option<TileId> {
        use crate::world::stacking::can_stop_at;
        let map = &self.world.map;
        let profile = crate::navigation::MoveProfile::of_kind(self.world, rules, barbarian, kind);
        let fits = |t: TileId| {
            !crate::navigation::terrain::is_water(map, t)
                && crate::navigation::cost::destination_ok(self.world, &profile, t)
                && can_stop_at(self.world, rules, barbarian, kind, None, t).is_ok()
        };
        if fits(tile) {
            return Some(tile);
        }
        let dims = map.dimensions;
        let mut seen = vec![tile];
        let mut ring = vec![tile];
        for _ in 0..REBEL_PLACE_RINGS {
            let mut next = Vec::new();
            for t in &ring {
                for n in dims.neighbors(*t) {
                    if seen.contains(&n) || crate::navigation::cost::enter_cost_on_map(map, n).is_none() {
                        continue;
                    }
                    seen.push(n);
                    next.push(n);
                }
            }
            if let Some(spot) = next.iter().copied().find(|t| fits(*t)) {
                return Some(spot);
            }
            if next.is_empty() {
                return None;
            }
            ring = next;
        }
        None
    }
}

/// The unit kinds rebels of `player` may be: regular land units of the
/// rules that fight in melee (a strength and no ranged strength) and that
/// `player` could build now (Unciv: TurnManager.doRevoltSpawn, `uniqueTo
/// == null && isMelee() && isLandUnit && isBuildable(civ)`), in rules order.
pub fn rebel_kinds(auth: &crate::world::state::AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Vec<UnitKindId> {
    let techs = auth.players().techs_of(player);
    let nation = auth.players().nation_of(player);
    let mut out = Vec::new();
    for (i, def) in rules.units.iter().enumerate() {
        let kind = UnitKindId(i as u16);
        if def.domain != Domain::Land || def.strength == 0 || def.ranged_strength > 0 || def.nation.is_some() {
            continue;
        }
        if rules.production_gate(crate::ids::ProductionItem::Unit(kind), nation).is_some() || rules.replaced_for(nation, kind) {
            continue;
        }
        if def.required_tech.is_some_and(|t| !techs.contains(&t)) || def.obsolete_tech.is_some_and(|t| techs.contains(&t)) {
            continue;
        }
        if def.required_project.is_some() {
            continue;
        }
        if crate::derived::resources::production_resources(auth, rules, player, crate::ids::ProductionItem::Unit(kind)).is_err() {
            continue;
        }
        out.push(kind);
    }
    out
}
