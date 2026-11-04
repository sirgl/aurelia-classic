// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/unit/WorkerAutomation.kt (`automateWorkerAction`,
// `findTileToWork`, `getBasePriority`, `getImprovementPriority`, `chooseImprovement`,
// `getImprovementRanking`, `tryHeadTowardsUndevelopedCity`, `automateWorkBoats`) and, reduced,
// RoadBetweenCitiesAutomation.kt (a road to the cities not connected to the capital).

//! Unciv's workers: the tile within three of the worker whose base priority
//! (inside our borders, yields, a resource, fresh water) is highest and that
//! has an improvement worth building, the best of that group by the
//! improvement's rank (the yields it adds by `rankStatsValue`, a resource it
//! connects); else toward a city with fewer workers and work to do; else a
//! road toward an unconnected city. The yields and the site rule are the
//! rules' own over the player's knowledge (`derived::tile_yields`,
//! `world::builds::site`).

use crate::ai::planner::{Decision, PlanTools};
use crate::ai::trace::Reason;
use crate::command::Command;
use crate::derived::tile_yields::{plot_yields, plot_yields_with};
use crate::ids::{ImprovementId, TileId};
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::{Domain, ImprovementKind, ResourceClass, Yields};
use crate::world::builds::site;

use super::head_towards::head_towards;
use super::sight::{actor, Unit};
use super::unit_automation::wander;
use super::Step;

fn sub(a: &Yields, b: &Yields) -> Yields {
    Yields { food: a.food - b.food, production: a.production - b.production, gold: a.gold - b.gold, science: a.science - b.science, culture: a.culture - b.culture, module: a.module - b.module }
}

/// Is a land builder of ours other than `u` on `tile`?
fn other_worker_on(u: &Unit<'_, '_>, tile: TileId) -> bool {
    let rules = u.rules();
    u.own_units().iter().any(|o| o.tile == tile && rules.units.get(o.kind.0 as usize).is_some_and(|d| d.builder))
}

/// `isAutomationWorkableTile`.
fn workable(u: &Unit<'_, '_>, tile: TileId, dangerous: &[TileId]) -> bool {
    if dangerous.binary_search(&tile).is_ok() {
        return false;
    }
    if tile != u.tile() && other_worker_on(u, tile) {
        return false;
    }
    let me = u.local().player;
    if u.owner_of(tile).is_some_and(|o| o != me) || u.is_city_tile(tile) {
        return false;
    }
    if !u.is_land_tile(tile) {
        return false;
    }
    // Only tiles a city of ours could work.
    u.sight.own_cities.iter().any(|c| u.distance(c.tile, tile) <= 3)
}

/// `getBasePriority` (without the worker's distance term).
fn base_priority(u: &Unit<'_, '_>, tile: TileId) -> f32 {
    let view = &u.local().view;
    let rules = u.rules();
    let known = view.tile(tile);
    let Some(s) = known.snapshot().copied() else {
        return 0.0;
    };
    let mut priority = 0.0;
    if s.owner == Some(u.local().player) {
        priority += 1.0;
        let y = plot_yields(view, rules, tile, &u.sight.builder().techs);
        if y.food > 0 || y.production > 0 || y.gold > 0 || y.science > 0 || y.culture > 0 || y.module > 0 {
            priority += 2.0;
        }
        if let Some(feature) = s.feature.and_then(|f| rules.features.get(f.0 as usize)) {
            match feature.removal {
                Some(r) if r.production > 0 => priority += 0.5,
                Some(_) => priority -= 0.3,
                None => {}
            }
        }
        if s.river || s.feature.and_then(|f| rules.features.get(f.0 as usize)).is_some_and(|f| f.fresh_water) {
            priority += 1.0;
        }
    }
    if let Some(r) = s.resource.and_then(|r| rules.resources.get(r.0 as usize)) {
        priority += 1.0;
        if r.class == Some(ResourceClass::Luxury) {
            priority += 5.0;
        }
    }
    priority
}

fn unit_specific(u: &Unit<'_, '_>, tile: TileId) -> f32 {
    2.0 - (u.distance(tile, u.tile()) as f32 / 2.0).clamp(0.0, 2.0) * 3.0
}

/// What a worker would do on a tile.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Work {
    Build(ImprovementId),
    Chop,
}

/// `getImprovementRanking`: the yields the improvement adds, ranked, and the
/// resource it connects.
fn improvement_ranking(u: &Unit<'_, '_>, tile: TileId, improvement: ImprovementId, removes: bool) -> f32 {
    let view = &u.local().view;
    let rules = u.rules();
    if u.owner_of(tile) != Some(u.local().player) {
        return 0.0;
    }
    let techs = u.sight.builder().techs;
    let now = plot_yields(view, rules, tile, &techs);
    let then = plot_yields_with(view, rules, tile, &techs, improvement, removes);
    let mut value = u.sight.rank_yields(&sub(&then, &now));
    let known = view.tile(tile);
    let Some(s) = known.snapshot() else {
        return value;
    };
    if let Some(r) = s.resource.and_then(|r| rules.resources.get(r.0 as usize)) {
        let current_connects = s.improvement.is_some_and(|i| r.improvements.contains(&i));
        let new_connects = r.improvements.contains(&improvement);
        if r.class != Some(ResourceClass::Bonus) {
            if current_connects {
                value -= 2.0;
            }
            if new_connects {
                value += 2.0;
            }
        } else {
            if current_connects {
                value -= 0.3;
            }
            if new_connects && !crate::math::hex_disc(u.dims(), tile, 4).any(|t| view.tile(t).snapshot().and_then(|x| x.improvement) == Some(improvement)) {
                value += 0.3;
            }
        }
    }
    value
}

/// `chooseImprovement`: the work worth doing on a tile and its rank (None:
/// nothing; the resource's improvement first for a strategic or luxury).
fn choose_work(u: &Unit<'_, '_>, tile: TileId) -> Option<(Work, f32)> {
    let view = &u.local().view;
    let rules = u.rules();
    let builder = u.sight.builder();
    // Our own worker already on it: keep its improvement.
    if let Some(w) = u.own_units().iter().chain(std::iter::once(&u.me)).filter_map(|o| o.work).find(|w| w.tile == tile) {
        let _ = w;
    }
    let known = view.tile(tile);
    let s = known.snapshot().copied()?;
    let mut potential: Vec<(ImprovementId, bool, f32)> = Vec::new();
    for (i, def) in rules.improvements.iter().enumerate() {
        if !matches!(def.kind, ImprovementKind::Regular | ImprovementKind::Unique) || def.unbuildable || def.placement.water {
            continue;
        }
        let improvement = ImprovementId(i as u16);
        let Ok(plan) = site(view, rules, &builder, tile, improvement) else {
            continue;
        };
        let removes = plan.removes.is_some();
        potential.push((improvement, removes, improvement_ranking(u, tile, improvement, removes)));
    }
    let resource = s.resource.and_then(|r| rules.resources.get(r.0 as usize));
    let best_buildable = potential.iter().filter(|(_, _, v)| *v > 0.0).copied().max_by(|a, b| a.2.total_cmp(&b.2).then(b.0 .0.cmp(&a.0 .0)));
    if let Some(r) = resource.filter(|r| r.class != Some(ResourceClass::Bonus)) {
        let for_resource = potential.iter().filter(|(i, _, _)| r.improvements.contains(i)).copied().max_by(|a, b| a.2.total_cmp(&b.2));
        match for_resource {
            Some((i, _, v)) => return if s.improvement == Some(i) { None } else { Some((Work::Build(i), v.max(0.1))) },
            None if !r.improvements.is_empty() => return None,
            None => {}
        }
    }
    let (best, _, best_value) = best_buildable?;
    if let Some(current) = s.improvement {
        let current_value = improvement_ranking(u, tile, current, false);
        if current_value > best_value && !view.known_pillaged(tile) {
            return None;
        }
    }
    // A feature that yields less than nothing is removed.
    if let Some(feature) = s.feature.and_then(|f| rules.features.get(f.0 as usize)) {
        if feature.removal.is_some() && u.sight.rank_yields(&feature.yields) < 0.0 {
            return Some((Work::Chop, best_value));
        }
    }
    Some((Work::Build(best), best_value))
}

/// `getFullPriority` of a tile with work to do (None: no work).
fn full_priority(u: &Unit<'_, '_>, tile: TileId) -> Option<(Work, f32)> {
    let (work, rank) = choose_work(u, tile)?;
    (rank > 0.0).then(|| (work, base_priority(u, tile) + unit_specific(u, tile) + rank))
}

/// The order that starts the work on the worker's tile.
fn start_work(u: &Unit<'_, '_>, work: Work) -> Option<Decision> {
    let view = &u.local().view;
    let command = match work {
        Work::Build(improvement) => Command::BuildImprovement { unit: u.me.handle, improvement },
        Work::Chop => Command::ChopForest { unit: u.me.handle },
    };
    view.accepts(&command).then(|| Decision::act(actor(&u.me), Reason::BuildImprovement, command))
}

/// `automateWorkerAction`.
pub fn automate_worker(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Decision {
    let step = worker_step(u, tools);
    match step {
        Step::Act(d) => d,
        Step::Done(reason) => Decision::refuse(actor(&u.me), reason),
        Step::Next => Decision::refuse(actor(&u.me), Reason::NoJob),
    }
}

fn worker_step(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let rules = u.rules();
    if rules.units.get(u.me.kind.0 as usize).is_some_and(|d| d.domain == Domain::Sea) {
        return automate_work_boat(u, tools);
    }
    let dangerous = u.dangerous_tiles(3);
    // Working a good tile already: finish it.
    if u.me.work.is_some() && dangerous.binary_search(&u.tile()).is_err() {
        return Step::Done(Reason::BuildImprovement);
    }
    if !u.has_movement() {
        return Step::Done(Reason::Waiting);
    }
    match super::civilian::try_run_away(u) {
        Step::Next => {}
        other => return other,
    }
    if let Some((tile, work)) = find_tile_to_work(u, tools, &dangerous) {
        if tile == u.tile() {
            if let Some(d) = start_work(u, work) {
                return Step::Act(d);
            }
        } else {
            return head_towards(u, tools, tile, Reason::MoveToJob);
        }
    }
    match try_head_towards_undeveloped_city(u, tools, &dangerous) {
        Step::Next => {}
        other => return other,
    }
    match try_connecting_cities(u, tools) {
        Step::Next => {}
        other => return other,
    }
    if u.local().view.own_kind() != crate::world::player::PlayerKind::Major {
        return wander(u, true);
    }
    Step::Done(Reason::NoJob)
}

/// `findTileToWork`: the worker's tile when its base priority is 3 or more
/// and it has work; else the tiles within 3 by base priority group, the best
/// reachable of the first group with work.
fn find_tile_to_work(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>, dangerous: &[TileId]) -> Option<(TileId, Work)> {
    let here = u.tile();
    if workable(u, here, dangerous) && base_priority(u, here) + unit_specific(u, here) >= 3.0 {
        if let Some((work, _)) = full_priority(u, here) {
            return Some((here, work));
        }
    }
    let me = u.local().player;
    let mut groups: Vec<(i32, Vec<TileId>)> = Vec::new();
    for t in crate::math::hex_disc(u.dims(), here, 3) {
        let owner_ok = u.owner_of(t).is_none_or(|o| o == me);
        if !owner_ok || !workable(u, t, dangerous) {
            continue;
        }
        let p = base_priority(u, t) + unit_specific(u, t);
        if p < 0.0 {
            continue;
        }
        let key = (p * 100.0) as i32;
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(t),
            None => groups.push((key, vec![t])),
        }
    }
    groups.sort_by_key(|(k, _)| std::cmp::Reverse(*k));
    for (_, mut tiles) in groups {
        tiles.sort_by_key(|t| (u.distance(*t, here), *t));
        let mut best: Option<(TileId, Work, f32)> = None;
        for t in tiles {
            let Some((work, priority)) = full_priority(u, t) else {
                continue;
            };
            if t == here {
                return Some((here, work));
            }
            if !u.can_reach(tools, t) {
                continue;
            }
            if best.is_none_or(|(_, _, b)| priority > b) {
                best = Some((t, work, priority));
            }
        }
        if let Some((t, work, _)) = best {
            return Some((t, work));
        }
    }
    None
}

/// `tryHeadTowardsUndevelopedCity`: a city with fewer workers around than
/// ours and a tile with work to do.
fn try_head_towards_undeveloped_city(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>, dangerous: &[TileId]) -> Step {
    let rules = u.rules();
    let is_worker = |kind: crate::ids::UnitKindId| rules.units.get(kind.0 as usize).is_some_and(|d| d.builder && d.domain == Domain::Land);
    let closest = |tile: TileId| u.sight.own_cities.iter().map(|c| c.tile).min_by_key(|c| (u.distance(*c, tile), *c));
    let workers_of = |city: TileId| u.sight.own_units.iter().filter(|o| is_worker(o.kind) && closest(o.tile) == Some(city)).count();
    let Some(mine) = closest(u.tile()) else {
        return Step::Next;
    };
    let mut cities: Vec<TileId> = u.sight.own_cities.iter().map(|c| c.tile).collect();
    cities.sort_by_key(|c| (u.distance(*c, u.tile()), *c));
    for city in cities {
        if city == mine || workers_of(city) >= workers_of(mine) {
            continue;
        }
        let has_work = crate::math::hex_disc(u.dims(), city, 3).any(|t| workable(u, t, dangerous) && !other_worker_on(u, t) && choose_work(u, t).is_some_and(|(_, r)| r > 0.0));
        if has_work && u.can_reach(tools, city) {
            return head_towards(u, tools, city, Reason::MoveToJob);
        }
    }
    Step::Next
}

/// The cities our roads do not link to the capital: a road on the next tile of
/// the way (reduced `RoadBetweenCitiesAutomation`).
fn try_connecting_cities(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let view = &u.local().view;
    let Some(capital) = u.sight.capital else {
        return Step::Next;
    };
    let connected = view.own_connected_cities();
    let unconnected: Vec<TileId> = u.sight.own_cities.iter().filter(|c| !connected.contains(&c.handle)).map(|c| c.tile).collect();
    let road_here = Command::BuildRoad { unit: u.me.handle };
    for city in unconnected {
        if u.distance(city, capital) > 12 {
            continue;
        }
        // The worker walks the way from the city toward the capital and builds where a road is missing.
        let on_way = u.distance(u.tile(), city) + u.distance(u.tile(), capital) <= u.distance(city, capital) + 1;
        let missing_here = view.tile(u.tile()).snapshot().is_some_and(|s| s.route == crate::world::map::Route::None && !s.city);
        if on_way && missing_here && view.accepts(&road_here) {
            return Step::Act(Decision::act(actor(&u.me), Reason::BuildRoad, road_here));
        }
        let goal = if u.distance(u.tile(), city) <= u.distance(u.tile(), capital) { capital } else { city };
        if goal != u.tile() && u.can_reach(tools, goal) {
            let step = head_towards(u, tools, goal, Reason::BuildRoad);
            if matches!(step, Step::Act(_)) {
                return step;
            }
        }
    }
    Step::Next
}

/// `automateWorkBoats`: the nearest water resource of our cities a work boat
/// can improve, walked to and improved.
fn automate_work_boat(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let view = &u.local().view;
    let rules = u.rules();
    let builder = u.sight.builder();
    let kinds = view.terrain_kinds();
    let mut targets: Vec<(TileId, ImprovementId)> = Vec::new();
    for city in &u.sight.own_cities {
        for t in crate::math::hex_disc(u.dims(), city.tile, 3) {
            let known = view.tile(t);
            let Some(s) = known.snapshot() else {
                continue;
            };
            if s.owner != Some(u.local().player) || !kinds.is_water(s.terrain.0) {
                continue;
            }
            let Some(r) = s.resource.and_then(|r| rules.resources.get(r.0 as usize)) else {
                continue;
            };
            if s.improvement.is_some_and(|i| r.improvements.contains(&i)) {
                continue;
            }
            if let Some(i) = r.improvements.iter().copied().find(|i| site(view, rules, &builder, t, *i).is_ok()) {
                if t == u.tile() || u.can_move_to(t) {
                    targets.push((t, i));
                }
            }
        }
    }
    targets.sort_by_key(|(t, _)| (u.distance(*t, u.tile()), *t));
    targets.dedup();
    let Some((tile, improvement)) = targets.into_iter().take(super::CANDIDATE_LIMIT).find(|(t, _)| *t == u.tile() || u.can_reach(tools, *t)) else {
        return super::unit_automation::try_explore(u, tools);
    };
    if tile == u.tile() {
        let build = Command::BuildImprovement { unit: u.me.handle, improvement };
        return if view.accepts(&build) { Step::Act(Decision::act(actor(&u.me), Reason::BuildImprovement, build)) } else { Step::Done(Reason::NoJob) };
    }
    head_towards(u, tools, tile, Reason::MoveToJob)
}
