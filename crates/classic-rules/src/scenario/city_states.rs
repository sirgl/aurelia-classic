//! City-states on a new map (placed after
//! the civilizations): each is a player of the city-state kind with one
//! city and a warrior, at land no closer than CITY_STATE_MIN_DISTANCE to
//! any city. The city-states are chosen from the rules by the seed.

use crate::ids::{PlayerId, TileId};
use crate::rules::content::CityStateId;
use crate::rules::CompiledRuleset;
use crate::world::city_states::Personality;
use crate::world::player::PlayerKind;
use crate::world::state::GameState;
use crate::world::player::ClassicKind;
use kernel::world::WorldBuilder;
use crate::classic::Classic;

/// No city-state closer than this to another city (start or city-state).
pub const CITY_STATE_MIN_DISTANCE: i32 = 6;
/// Keep this many rows and columns from the map edge.
const EDGE_MARGIN: i32 = 2;

/// Place `count` city-states as players `first..first + count`.
pub fn place_city_states(
    game: &mut WorldBuilder<'_, Classic>,
    first: u8,
    count: u8,
    seed: u64,
) {
    let rules = game.rules();
    if count == 0 || rules.module.city_states.is_empty() {
        return;
    }
    let chosen = choose(rules, count as usize, seed);
    let warrior = rules.roles.starting_unit(crate::rules::roles::StartingUnit::Defense);
    let mut taken: Vec<TileId> = game
        .state()
        .view()
        .auth
        .units()
        .iter_cores()
        .map(|u| u.tile)
        .collect();
    for (i, cs) in chosen.into_iter().enumerate() {
        let player = PlayerId(first + i as u8);
        game.set_player_kind(player, PlayerKind::Module(ClassicKind::CityState(cs)));
        let personality = Personality::ALL[(mix(seed, u64::from(player.0)) % 4) as usize];
        if let Some(minor) = game.auth_mut().module.city_states.minor_mut(player) {
            minor.personality = personality;
        }
        let Some(tile) = site(game.state(), &taken) else {
            continue;
        };
        taken.push(tile);
        game.found_city(player, tile);
        if let Some(kind) = warrior {
            game.spawn_unit(player, tile, kind);
        }
    }
}

/// `count` distinct city-states, shuffled by the seed.
fn choose(rules: &CompiledRuleset, count: usize, seed: u64) -> Vec<CityStateId> {
    let mut pool: Vec<CityStateId> = (0..rules.module.city_states.len())
        .map(|i| CityStateId(i as u16))
        .collect();
    let mut state = seed ^ 0xC17_57A7E;
    for i in (1..pool.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let j = (state % (i as u64 + 1)) as usize;
        pool.swap(i, j);
    }
    pool.truncate(count);
    pool
}

/// The best free site: land a city may stand on, away from the edge and at
/// least CITY_STATE_MIN_DISTANCE from every start and city; among those
/// the farthest from them (deterministic: lowest tile id on ties).
fn site(game: &GameState, taken: &[TileId]) -> Option<TileId> {
    let auth = game.view().auth;
    let dims = auth.map().dimensions;
    let mut best: Option<(i32, TileId)> = None;
    for i in 0..auth.map().tile_count() {
        let tile = TileId(i as u32);
        let Some(c) = dims.to_coord(tile) else {
            continue;
        };
        let inside = c.q >= EDGE_MARGIN
            && c.r >= EDGE_MARGIN
            && c.q < dims.width as i32 - EDGE_MARGIN
            && c.r < dims.height as i32 - EDGE_MARGIN;
        if !inside || auth.map().natural_wonder(tile).is_some() {
            continue;
        }
        if crate::world::territory::city_site_check(auth, PlayerId(u8::MAX), tile).is_err() {
            continue;
        }
        let nearest = taken
            .iter()
            .chain(auth.cities().iter().map(|(_, c)| &c.tile))
            .map(|t| dims.distance(*t, tile).unwrap_or(i32::MAX))
            .min()
            .unwrap_or(i32::MAX);
        if nearest < CITY_STATE_MIN_DISTANCE {
            continue;
        }
        if best.map(|(d, _)| nearest > d).unwrap_or(true) {
            best = Some((nearest, tile));
        }
    }
    best.map(|(_, t)| t)
}

/// A deterministic mix of the seed and a number (personalities).
fn mix(seed: u64, n: u64) -> u64 {
    let mut x = seed ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^ (x >> 33)
}
