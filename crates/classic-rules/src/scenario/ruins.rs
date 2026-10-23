//! Ancient Ruins on a new map: the places a unit finds a reward by walking
//! onto them (`systems::ruins`). The map holds about one ruin per
//! `LAND_TILES_PER_RUINS` tiles of passable land, on free land away from
//! every start and city, no two of them close together. The sites come from
//! the seed alone, so a map and its seed always carry the same ruins.

use crate::ids::{ImprovementId, TileId};
use crate::rules::content::ImprovementKind;
use crate::rules::CompiledRuleset;
use crate::world::state::GameState;
use crate::classic::Classic;
use kernel::world::WorldBuilder;

/// One ruin for this many tiles of land a unit can enter.
pub const LAND_TILES_PER_RUINS: usize = 80;
/// No ruin closer than this to a start, a city or a city-state's warrior.
pub const MIN_DISTANCE_FROM_START: i32 = 4;
/// No ruin closer than this to another one.
pub const MIN_DISTANCE_BETWEEN: i32 = 4;

/// Lay the ruins on the free land of `game`.
pub fn place_ruins(game: &mut WorldBuilder<'_, Classic>, seed: u64) {
    let rules = game.rules();
    let Some(ruins) = ruins_improvement(rules) else {
        return;
    };
    let sites = sites(game.state(), seed);
    let placed: Vec<(TileId, ImprovementId)> = sites.into_iter().map(|tile| (tile, ruins)).collect();
    game.set_improvements(&placed);
}

fn ruins_improvement(rules: &CompiledRuleset) -> Option<ImprovementId> {
    rules
        .improvements
        .iter()
        .position(|d| d.kind == ImprovementKind::AncientRuins)
        .map(|i| ImprovementId(i as u16))
}

/// The chosen tiles, in the order they were drawn.
fn sites(game: &GameState, seed: u64) -> Vec<TileId> {
    let auth = game.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    let land: Vec<TileId> = (0..map.tile_count())
        .map(|i| TileId(i as u32))
        .filter(|t| !map.is_water(*t) && crate::navigation::cost::enter_cost_on_map(map, *t).is_some())
        .collect();
    let starts: Vec<TileId> = auth
        .units()
        .iter_cores()
        .map(|u| u.tile)
        .chain(auth.cities().iter().map(|(_, c)| c.tile))
        .collect();
    let far = |tile: TileId, from: &[TileId], least: i32| from.iter().all(|t| dims.distance(*t, tile).unwrap_or(i32::MAX) >= least);
    let mut free: Vec<TileId> = land
        .iter()
        .copied()
        .filter(|t| {
            map.natural_wonder(*t).is_none()
                && map.resource(*t).flatten().is_none()
                && map.improvement(*t).flatten().is_none()
                && far(*t, &starts, MIN_DISTANCE_FROM_START)
        })
        .collect();
    shuffle(&mut free, seed);
    let wanted = land.len() / LAND_TILES_PER_RUINS;
    let mut chosen: Vec<TileId> = Vec::with_capacity(wanted);
    for tile in free {
        if chosen.len() == wanted {
            break;
        }
        if far(tile, &chosen, MIN_DISTANCE_BETWEEN) {
            chosen.push(tile);
        }
    }
    chosen
}

/// Fisher-Yates by a xorshift stream of the seed (its own stream: the map's
/// generator streams are untouched).
fn shuffle(tiles: &mut [TileId], seed: u64) {
    let mut state = seed ^ 0xA9C1_E47_5B0D;
    for i in (1..tiles.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        tiles.swap(i, (state % (i as u64 + 1)) as usize);
    }
}
