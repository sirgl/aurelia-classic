//! The path of a trade route (classic/docs/classic/rules.md): the
//! cheapest path between two cities by the trade step costs, found by
//! Dijkstra within a cost budget (ties to the lower tile id, so the path is
//! deterministic).

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::derived::trade::{ENEMY_WATER_COST, OPEN_SEA_STEP_COST, ROAD_STEP_COST, ROUGH_STEP_COST, STEP_COST};
use crate::ids::{FeatureId, TileId};
use crate::rules::stats::TradeDomain;
use crate::rules::roles::FeatureRole;
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::map::Route;
use crate::world::state::AuthWorld;

/// Origin to destination, both tiles included; None when the cheapest
/// path costs more than `budget`.
pub fn cheapest(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    origin: &CityCore,
    to: TileId,
    domain: TradeDomain,
    budget: u32,
) -> Option<Vec<TileId>> {
    let map = auth.map();
    let woods = crate::navigation::profile::own_land_road_features(auth, rules, origin.owner);
    let n = map.tile_count();
    let mut cost = vec![u32::MAX; n];
    let mut came = vec![u32::MAX; n];
    let mut open = BinaryHeap::new();
    cost[origin.tile.0 as usize] = 0;
    open.push(Reverse((0u32, origin.tile.0)));
    while let Some(Reverse((c, t))) = open.pop() {
        let tile = TileId(t);
        if tile == to {
            return Some(walk_back(&came, origin.tile, to));
        }
        if c > cost[t as usize] {
            continue;
        }
        for next in map.dimensions.neighbors(tile) {
            let Some(step) = step(auth, rules, origin, &woods, tile, next, to, domain) else {
                continue;
            };
            let total = c + step;
            if total <= budget && total < cost[next.0 as usize] {
                cost[next.0 as usize] = total;
                came[next.0 as usize] = t;
                open.push(Reverse((total, next.0)));
            }
        }
    }
    None
}

fn walk_back(came: &[u32], from: TileId, to: TileId) -> Vec<TileId> {
    let mut path = vec![to];
    let mut at = to;
    while at != from {
        at = TileId(came[at.0 as usize]);
        path.push(at);
    }
    path.reverse();
    path
}

/// Cost of one step of a route (None: the step is not allowed).
fn step(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    origin: &CityCore,
    woods: &[FeatureId],
    from: TileId,
    to: TileId,
    destination: TileId,
    domain: TradeDomain,
) -> Option<u32> {
    match domain {
        TradeDomain::Land => land_step(auth, rules, woods, from, to),
        TradeDomain::Sea => sea_step(auth, origin, from, to, destination),
    }
}

/// Cost of a land step: land that is not impassable; `ROAD_STEP_COST` between two route tiles or into
/// a tile of a feature the trader's civilization walks as a road (`woods`: the Iroquois' forests and
/// jungles, whoever owns the tile), `ROUGH_STEP_COST` into hills, forest, jungle or ice, else
/// `STEP_COST`. Own design.
fn land_step(auth: &AuthWorld, rules: &CompiledRuleset, woods: &[FeatureId], from: TileId, to: TileId) -> Option<u32> {
    let map = auth.map();
    crate::navigation::cost::enter_cost_on_map(map, to)?;
    if map.route(from) != Route::None && map.route(to) != Route::None {
        return Some(ROAD_STEP_COST);
    }
    if map.feature(to).flatten().is_some_and(|f| woods.contains(&f)) {
        return Some(ROAD_STEP_COST);
    }
    let feature = map.feature(to).flatten().and_then(|f| rules.roles.feature_role(f));
    let hills = map.terrain(to).is_some_and(|t| map.terrain_kinds().is_hills(t.0));
    let rough = hills || matches!(feature, Some(FeatureRole::Forest | FeatureRole::Jungle | FeatureRole::Ice));
    Some(if rough { ROUGH_STEP_COST } else { STEP_COST })
}

/// Cost of a sea step: water tiles (maps carry no deep ocean) and the two city tiles; `STEP_COST`
/// along the coast, `OPEN_SEA_STEP_COST` away from land, `ENEMY_WATER_COST` more in the water of an
/// enemy. Own design.
fn sea_step(auth: &AuthWorld, origin: &CityCore, from: TileId, to: TileId, destination: TileId) -> Option<u32> {
    let map = auth.map();
    if to == destination {
        return Some(STEP_COST);
    }
    if !map.is_water(to) {
        return None;
    }
    let near_land = |t: TileId| map.dimensions.neighbors(t).into_iter().any(|n| !map.is_water(n));
    let mut cost = if near_land(from) && near_land(to) { STEP_COST } else { OPEN_SEA_STEP_COST };
    if let Some(Some(owner)) = map.owner(to) {
        if auth.diplomacy().at_war(origin.owner, owner) {
            cost += ENEMY_WATER_COST;
        }
    }
    Some(cost)
}
