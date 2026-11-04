//! The battle tile as combat sees it (classic/docs/classic/rules.md):
//! open or rough ground, the tile's defense percent, water and deep water,
//! friendly territory.
//!
//! The predicates of the tile itself (water, hills, rough ground) are the
//! kernel's (`navigation::terrain`). Combat separates coast and ocean: deep
//! water is the ocean, shallow water the coast.

use crate::ids::{PlayerId, TileId};
use crate::navigation::terrain::{is_hills, is_mountain};
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;
use crate::world::player::ClassicPlayers;

/// Defense percent of hills and mountains (+25 %). Unciv: `Terrains.json` (Hill,
/// `defenceBonus` 0.25); equal for hills.
pub const HILLS_DEFENCE_BONUS: i64 = 25;

/// Tile defence (rules.md): hills or mountains +25, else the
/// feature's defense, plus the improvement's defense when the tile is
/// unowned or the defender's. Terrain table defenses of flat land are 0.
pub fn tile_defense_percent(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    tile: TileId,
    defender: PlayerId,
) -> i64 {
    let map = auth.map();
    let high = is_hills(map, tile) || is_mountain(map, tile);
    let mut percent = ground_defense_percent(rules, high, map.feature(tile).flatten());
    let owner = map.owner(tile).flatten();
    if owner.is_none() || owner == Some(defender) {
        if let Some(improvement) = map.working_improvement(tile) {
            if let Some(def) = rules.improvements.get(improvement.0 as usize) {
                percent += i64::from(def.defense_pct);
            }
        }
    }
    percent
}

/// The ground's part of a tile's defense: hills or mountains (`high`)
/// +25 %, else the feature's defense. The AI reads it over the tiles it
/// knows (`ai::defence`).
pub fn ground_defense_percent(rules: &CompiledRuleset, high: bool, feature: Option<crate::ids::FeatureId>) -> i64 {
    if high {
        return HILLS_DEFENCE_BONUS;
    }
    feature.and_then(|f| rules.features.get(f.0 as usize)).map_or(0, |def| i64::from(def.defense_pct))
}

/// Friendly territory: own land, or the land of a player that
/// grants the owner open borders. Barbarians have no friendly land.
pub fn is_friendly_territory(auth: &AuthWorld, owner: PlayerId, tile: TileId) -> bool {
    if auth.players().is_barbarian(owner) {
        return false;
    }
    match auth.map().owner(tile).flatten() {
        Some(host) => host == owner || auth.diplomacy().grants_entry(host, owner),
        None => false,
    }
}
