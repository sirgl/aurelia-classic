// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/unit/CivilianUnitAutomation.kt,
// SpecificUnitAutomation.kt (`automateSettlerActions`, `automateGreatGeneral`,
// `automateImprovementPlacer`, `conductTradeMission`, `speedupWonderConstruction`,
// `automateAddInCapital`), CityLocationTileRanker.kt, Automation.rankTile and
// logic/battle/GreatGeneralImplementation.getBestAffectedTroopsTile.

//! Unciv's civilian units: settlers rank the sites around them and found
//! where the rank is best within reach; civilians run from enemies they
//! could be captured by; great people use their gift as Unciv's AI does
//! (late in the game a scientist discovers, a merchant trades with a
//! city-state, an engineer hurries a wonder, the others build their great
//! improvement where their stat is wanted, an artist starts a golden age);
//! a great general follows the army; a spaceship part goes to the capital.

use crate::ai::planner::{Decision, PlanTools};
use crate::ai::trace::Reason;
use crate::classic::effects::GreatPersonKind;
use crate::command::Command;
use crate::derived::great_people::GreatPersonAction;
use crate::ids::{ImprovementId, TileId};
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::ResourceClass;

use super::head_towards::{head_towards, move_to_tile};
use super::sight::{actor, Kind, Unit};
use super::unit_automation::{try_explore, wander};
use super::{Memo, Step};

/// What kind of civilian Unciv's automation sees in a unit.
pub fn great_person_kind(u: &Unit<'_, '_>) -> Option<GreatPersonKind> {
    crate::derived::great_people::great_person_of(u.rules(), u.me.kind).map(|g| g.kind)
}

/// `tryRunAwayIfNeccessary`: a civilian an enemy could reach walks to a city,
/// to one of our military units, or away from the danger.
pub fn try_run_away(u: &Unit<'_, '_>) -> Step {
    let rules = u.rules();
    let enemy_in_reach = u.reachable().iter().any(|(t, _)| {
        u.local().foreign_units.iter().any(|f| f.tile == *t && u.at_war(f.owner) && Kind { rules, kind: f.kind }.is_military())
            || u.sight.foreign_cities.iter().any(|c| c.tile == *t && u.at_war(c.owner))
    });
    let guarded = u.own_military_on(u.tile()).is_some_and(|m| m.id != u.me.id) || u.is_city_tile(u.tile());
    if !enemy_in_reach || u.kind.is_military() || guarded {
        return Step::Next;
    }
    let reach: Vec<TileId> = u.reachable().iter().map(|(t, _)| *t).collect();
    if let Some(city) = reach.iter().copied().find(|t| u.sight.own_cities.iter().any(|c| c.tile == *t) && u.can_move_to(*t)) {
        return move_to_tile(u, city, Reason::Retreat);
    }
    if let Some(guard) = reach.iter().copied().find(|t| u.own_military_on(*t).is_some() && !u.own_units().iter().any(|o| o.tile == *t && Kind { rules, kind: o.kind }.is_civilian())) {
        return move_to_tile(u, guard, Reason::Retreat);
    }
    let dangerous = u.dangerous_tiles(3);
    let closest = dangerous.iter().copied().min_by_key(|t| (u.distance(*t, u.tile()), *t)).unwrap_or(u.tile());
    let far = reach
        .iter()
        .copied()
        .filter(|t| u.can_move_to(*t) && dangerous.binary_search(t).is_err())
        .max_by_key(|t| (u.distance(*t, closest), u.friendly_territory(*t), std::cmp::Reverse(*t)));
    match far {
        Some(tile) => move_to_tile(u, tile, Reason::Retreat),
        None => Step::Done(Reason::Threatened),
    }
}

/// Unciv's `ModConstants.minimumCityLocationTileValue`.
const MINIMUM_CITY_LOCATION_TILE_VALUE: f32 = 53.0;

/// `CityLocationTileRanker.rankTile`: a tile's worth to a city founded near it.
fn rank_tile(u: &Unit<'_, '_>, tile: TileId, on_coast: bool, new_luxuries: &mut Vec<crate::ids::ResourceId>, prior: f32) -> f32 {
    let view = &u.local().view;
    let rules = u.rules();
    let known = view.tile(tile);
    let Some(s) = known.snapshot().copied() else {
        return prior;
    };
    if s.city {
        return -1.0;
    }
    let kinds = view.terrain_kinds();
    let water = kinds.is_water(s.terrain.0);
    let mut location = 0.0;
    if water && !on_coast {
        location -= 1.0;
    }
    let class = s.resource.and_then(|r| rules.resources.get(r.0 as usize)).and_then(|d| d.class);
    if let Some(r) = s.resource {
        if class == Some(ResourceClass::Luxury) && !u.sight.resources_have.contains(&r) && !new_luxuries.contains(&r) {
            location += 10.0;
            new_luxuries.push(r);
        }
    }
    if s.owner.is_some_and(|o| o != u.local().player) {
        return location;
    }
    let yields = crate::derived::tile_yields::plot_yields(view, rules, tile, &u.sight.builder().techs);
    let mut value = u.sight.rank_yields(&yields.at_least(0));
    value += match class {
        Some(ResourceClass::Bonus) => 1.0,
        Some(ResourceClass::Strategic) => 2.0,
        Some(ResourceClass::Luxury) => 10.0,
        _ => 0.0,
    };
    if s.river || s.feature.and_then(|f| rules.features.get(f.0 as usize)).is_some_and(|f| f.fresh_water) {
        value += 0.5;
    }
    if s.feature.and_then(|f| rules.features.get(f.0 as usize)).and_then(|f| f.removal).is_some_and(|r| r.production > 0) {
        value += 0.7;
    }
    if s.natural_wonder.is_some() {
        value += 4.0;
    }
    value + location
}

/// `rankTileToSettle`.
fn rank_tile_to_settle(u: &Unit<'_, '_>, tile: TileId, own_cities: &[(TileId, bool)], prior: f32) -> f32 {
    let view = &u.local().view;
    let rules = u.rules();
    let dims = u.dims();
    let kinds = view.terrain_kinds();
    let mut value = 0.0;
    for (city, capital) in own_cities {
        let d = u.distance(tile, *city);
        let m = match d {
            7 => 2.0,
            6 => 4.0,
            5 => 8.0,
            4 => 6.0,
            3 => -25.0,
            d if d < 3 => -30.0,
            _ => 0.0,
        };
        value += if *capital { m * 2.0 } else { m };
    }
    let Some(s) = view.tile(tile).snapshot().copied() else {
        return value;
    };
    let neighbours: Vec<TileId> = dims.neighbors(tile).into_iter().collect();
    let water_near = neighbours.iter().any(|n| view.tile(*n).snapshot().is_some_and(|t| kinds.is_water(t.terrain.0)));
    let on_hill = kinds.is_hills(s.terrain.0);
    let mountain_near = neighbours.iter().any(|n| view.tile(*n).snapshot().is_some_and(|t| kinds.is_mountains(t.terrain.0)));
    if water_near {
        value += 3.0;
    }
    if on_hill {
        value += 14.0;
    }
    if mountain_near {
        value += 5.0;
    }
    if s.river {
        value += 20.0;
    }
    if let Some(r) = s.resource.and_then(|r| rules.resources.get(r.0 as usize)) {
        value -= 4.0;
        if r.class == Some(ResourceClass::Bonus) {
            value -= 8.0;
        }
        let vegetation_no_chop = s.feature.and_then(|f| rules.features.get(f.0 as usize)).is_some_and(|f| f.removal.is_some_and(|r| r.production == 0));
        if r.class == Some(ResourceClass::Luxury) && vegetation_no_chop {
            value += 10.0;
        }
    }
    let mut new_luxuries = Vec::new();
    let mut tiles = 0;
    for ring in 0..=2 {
        for t in crate::math::hex_disc(dims, tile, ring).filter(|t| dims.distance(*t, tile) == Some(ring)) {
            tiles += 1;
            value += rank_tile(u, t, water_near, &mut new_luxuries, prior) * (3.0 / (ring as f32 + 1.0));
        }
    }
    value -= (19 - tiles) as f32 * 2.4;
    value
}

/// `CityLocationTileRanker.getBestTilesToFoundCity` and
/// `automateSettlerActions`.
pub fn automate_settler(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let found = Command::FoundCity { settler: u.me.handle };
    let view = &u.local().view;
    let own_kind = view.own_kind();
    if own_kind != crate::world::player::PlayerKind::Major && u.sight.own_cities.is_empty() {
        // City-state starts are predetermined: trust the tile.
        if view.accepts(&found) {
            return Step::Act(Decision::act(actor(&u.me), Reason::FoundCity, found));
        }
    }
    let round = u.sight.round;
    let range = if u.sight.own_cities.is_empty() && round < 4 {
        (3 - round as i32).max(1)
    } else {
        let home = u.sight.own_cities.iter().map(|c| u.distance(c.tile, u.tile())).min().unwrap_or(0);
        (8 - home).clamp(1, 5)
    };
    let dims = u.dims();
    let own_cities: Vec<(TileId, bool)> = u.sight.own_cities.iter().map(|c| (c.tile, Some(c.tile) == u.sight.capital)).filter(|(t, _)| u.distance(*t, u.tile()) <= 7 + range).collect();
    let known_cities: Vec<TileId> = u.sight.own_cities.iter().map(|c| c.tile).chain(u.sight.foreign_cities.iter().map(|c| c.tile)).collect();
    // The unexplored tiles are worth the average explored tile around.
    let mut base: Vec<f32> = Vec::new();
    for t in crate::math::hex_disc(dims, u.tile(), range + 2) {
        if view.is_known(t) {
            base.push(rank_tile(u, t, false, &mut Vec::new(), 0.0));
        }
    }
    let prior = if base.is_empty() { 0.0 } else { base.iter().sum::<f32>() / base.len() as f32 };
    let kinds = view.terrain_kinds();
    let me = u.local().player;
    let mut ranked: Vec<(TileId, f32)> = Vec::new();
    for t in crate::math::hex_disc(dims, u.tile(), range) {
        let known = view.tile(t);
        let Some(s) = known.snapshot() else {
            continue;
        };
        if kinds.is_water(s.terrain.0) || kinds.enter_cost(s.terrain).is_none() || s.city || s.owner.is_some_and(|o| o != me) {
            continue;
        }
        if known_cities.iter().any(|c| u.distance(*c, t) <= crate::world::territory::MIN_CITY_RANGE) {
            continue;
        }
        if t != u.tile() && !u.can_move_to(t) {
            continue;
        }
        let mut value = rank_tile_to_settle(u, t, &own_cities, prior);
        let distance = (u.distance(u.tile(), t) as f32 * 3.0).clamp(0.0, 99.0);
        value *= (100.0 - distance) / 100.0;
        // Unciv's `minimumCityLocationTileValue` (ModConstants: 53): no snow fields.
        if value >= MINIMUM_CITY_LOCATION_TILE_VALUE {
            ranked.push((t, value));
        }
    }
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let best = ranked.iter().take(super::CANDIDATE_LIMIT).copied().find(|(t, _)| u.can_reach(tools, *t));
    let here = ranked.iter().find(|(t, _)| *t == u.tile()).map(|(_, v)| *v);
    // The first turn of the game: settle in place (Unciv's special case for the
    // first settler; with several, the one on the best ranked tile).
    if u.sight.own_cities.is_empty() && round <= 1 && here.is_some() && u.has_movement() {
        let rival = u.own_units().iter().filter(|o| o.kind == u.me.kind && o.moves_left > 0).filter_map(|o| ranked.iter().find(|(t, _)| *t == o.tile).map(|(_, v)| *v)).fold(f32::MIN, f32::max);
        if here.is_some_and(|v| v >= rival) && view.accepts(&found) {
            return Step::Act(Decision::act(actor(&u.me), Reason::FoundCity, found));
        }
    }
    let mut location = None;
    if let Some(v) = here {
        if best.is_none_or(|(_, b)| v >= b - 2.0) {
            location = Some(u.tile());
        }
    }
    if location.is_none() {
        if let Some((tile, _)) = best {
            if u.turns_to(tools, tile).is_some_and(|t| (1..=3).contains(&t)) {
                location = Some(tile);
            }
        }
    }
    if location.is_none() {
        let dangerous = u.dangerous_tiles(3);
        let floor = best.map_or(f32::MIN, |(_, b)| b - 5.0);
        location = ranked
            .iter()
            .filter(|(_, v)| *v >= floor)
            .take(super::CANDIDATE_LIMIT)
            .map(|(t, _)| *t)
            .find(|t| (dangerous.binary_search(t).is_err() || *t == u.tile()) && u.turns_to(tools, *t).is_some_and(|n| (1..=3).contains(&n)));
    }
    if location.is_none() {
        if let Some((tile, _)) = best {
            if u.turns_to(tools, tile).is_some_and(|t| (1..=8).contains(&t)) {
                location = Some(tile);
            }
        }
    }
    let Some(location) = location else {
        // Every site within reach is taken: toward the frontier, else explore, else wander.
        let frontier = u.sight.own_cities.iter().map(|c| c.tile).max_by_key(|c| {
            let ring = crate::world::territory::MIN_CITY_RANGE + 1;
            crate::math::hex_disc(dims, *c, ring).filter(|t| dims.distance(*t, *c) == Some(ring)).filter(|t| view.tile(*t).snapshot().is_some_and(|s| s.owner.is_none() && !kinds.is_water(s.terrain.0))).count()
        });
        if let Some(city) = frontier {
            if city != u.tile() && u.can_reach(tools, city) {
                if let Step::Act(d) = head_towards(u, tools, city, Reason::MoveToSite) {
                    return Step::Act(d);
                }
            }
        }
        return match try_explore(u, tools) {
            Step::Next => wander(u, false),
            other => other,
        };
    };
    if location == u.tile() && u.has_movement() {
        if view.accepts(&found) {
            return Step::Act(Decision::act(actor(&u.me), Reason::FoundCity, found));
        }
        return Step::Done(Reason::NoSettleSite);
    }
    match try_run_away(u) {
        Step::Next => {}
        other => return other,
    }
    head_towards(u, tools, location, Reason::MoveToSite)
}

/// `GreatGeneralImplementation.getBestAffectedTroopsTile` and
/// `automateGreatGeneral`: to the tile of our military unit with the most of
/// ours around it and the fewest others, near the enemy city we march on.
pub fn automate_great_general(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let target = super::head_towards::enemy_cities_by_priority(u).into_iter().find(|c| u.approach(tools, c.tile).is_some());
    let rules = u.rules();
    let best = u
        .reachable()
        .iter()
        .map(|(t, _)| *t)
        .filter(|t| u.own_military_on(*t).is_some_and(|m| m.id != u.me.id) && !u.own_units().iter().any(|o| o.tile == *t && Kind { rules, kind: o.kind }.is_civilian()))
        .max_by_key(|t| {
            let ours = crate::math::hex_disc(u.dims(), *t, 2).filter(|n| u.own_military_on(*n).is_some()).count() as i32;
            let others = crate::math::hex_disc(u.dims(), *t, 2).filter(|n| u.foreign_military_on(*n).is_some()).count() as i32;
            let away = target.map_or(0, |c| 3 * u.distance(*t, c.tile));
            (2 * ours - others - away, std::cmp::Reverse(*t))
        });
    match best {
        Some(tile) => head_towards(u, tools, tile, Reason::Escort),
        None => Step::Next,
    }
}

/// `automateImprovementPlacer`: the great improvement on the best tile of a
/// city (the cities in order of size), walked to first.
pub fn automate_improvement_placer(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let Some(gp) = crate::derived::great_people::great_person_of(u.rules(), u.me.kind) else {
        return Step::Next;
    };
    let view = &u.local().view;
    let rules = u.rules();
    let builder = u.sight.builder();
    let dims = u.dims();
    let improvements: Vec<ImprovementId> = gp.improvements.clone();
    if improvements.is_empty() {
        return Step::Next;
    }
    // Here already: build it.
    for imp in &improvements {
        let command = Command::GreatPersonAction { unit: u.me.handle, action: GreatPersonAction::Construct(*imp) };
        if view.accepts(&command) && u.owner_of(u.tile()) == Some(u.local().player) && !u.is_city_tile(u.tile()) {
            return Step::Act(Decision::act(actor(&u.me), Reason::BuildImprovement, command));
        }
    }
    let mut cities: Vec<&super::sight::OwnCitySight> = u.sight.own_cities.iter().collect();
    cities.sort_by_key(|c| std::cmp::Reverse(c.hp));
    for city in cities {
        let tiles: Vec<TileId> = crate::math::hex_disc(dims, city.tile, 3)
            .filter(|t| *t != city.tile && u.owner_of(*t) == Some(u.local().player) && u.is_land_tile(*t))
            .filter(|t| *t == u.tile() || u.can_move_to(*t))
            .filter(|t| improvements.iter().any(|imp| crate::world::builds::site(view, rules, &builder, *t, *imp).is_ok()))
            .filter(|t| !view.tile(*t).snapshot().and_then(|s| s.improvement).is_some_and(|i| rules.improvements.get(i.0 as usize).is_some_and(|d| d.kind == crate::classic::content::GREAT_PERSON_IMPROVEMENT)))
            .collect();
        if tiles.is_empty() {
            continue;
        }
        let Some(turns) = u.turns_to(tools, city.tile) else {
            continue;
        };
        if turns > 2 && u.distance(u.tile(), city.tile) > 3 {
            let enemies_near = u.local().foreign_units.iter().any(|f| u.at_war(f.owner) && Kind { rules, kind: f.kind }.is_military() && u.distance(f.tile, u.tile()) <= 5);
            if u.own_military_on(u.tile()).is_none() && enemies_near {
                return Step::Done(Reason::Threatened);
            }
            return head_towards(u, tools, city.tile, Reason::MoveToJob);
        }
        let mut ranked: Vec<(f32, TileId)> = tiles.iter().map(|t| (rank_any_tile(u, *t), *t)).collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        if let Some((_, tile)) = ranked.into_iter().find(|(_, t)| u.can_reach(tools, *t)) {
            if tile == u.tile() {
                return Step::Next;
            }
            return head_towards(u, tools, tile, Reason::MoveToJob);
        }
    }
    Step::Next
}

/// `Automation.rankTile`: a tile's yields, and what an improvement there would add.
pub fn rank_any_tile(u: &Unit<'_, '_>, tile: TileId) -> f32 {
    let view = &u.local().view;
    let rules = u.rules();
    let known = view.tile(tile);
    let Some(s) = known.snapshot().copied() else {
        return 0.0;
    };
    if s.owner.is_some_and(|o| o != u.local().player) {
        return 0.0;
    }
    let yields = crate::derived::tile_yields::plot_yields(view, rules, tile, &u.sight.builder().techs);
    let mut rank = u.sight.rank_yields(&yields);
    if s.improvement.is_none() {
        rank += 0.5;
    }
    if view.known_pillaged(tile) {
        rank += 0.6;
    }
    if let Some(r) = s.resource.and_then(|r| rules.resources.get(r.0 as usize)) {
        if r.class != Some(ResourceClass::Bonus) {
            rank += 1.0;
        }
        if s.improvement.is_none() {
            rank += 1.0;
        }
        if view.known_pillaged(tile) {
            rank += 1.1;
        }
    }
    rank
}

/// `conductTradeMission`: on a city-state's land, trade; else walk to the
/// nearest city-state we are at peace with, at most ten turns away.
fn conduct_trade_mission(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let view = &u.local().view;
    let mission = Command::GreatPersonAction { unit: u.me.handle, action: GreatPersonAction::TradeMission };
    if view.accepts(&mission) {
        return Step::Act(Decision::act(actor(&u.me), Reason::SystemAdvisor, mission));
    }
    let mut capitals: Vec<TileId> = u.local().relations.iter().filter(|r| !r.at_war && matches!(r.kind, crate::world::player::PlayerKind::Module(crate::world::player::ClassicKind::CityState(_)))).filter_map(|r| u.sight.foreign_cities.iter().find(|c| c.owner == r.other).map(|c| c.tile)).collect();
    capitals.sort_by_key(|t| (u.distance(*t, u.tile()), *t));
    for capital in capitals.into_iter().take(super::CANDIDATE_LIMIT) {
        let near: Option<TileId> = u.dims().neighbors(capital).into_iter().filter(|t| u.owner_of(*t) == u.owner_of(capital) && u.can_move_to(*t)).min_by_key(|t| (u.distance(*t, u.tile()), *t));
        if let Some(goal) = near {
            if u.turns_to(tools, goal).is_some_and(|n| (1..=10).contains(&n)) {
                return head_towards(u, tools, goal, Reason::MoveToJob);
            }
        }
    }
    Step::Next
}

/// `speedupWonderConstruction`: an engineer walks to a city of three citizens
/// or more building a wonder, and hurries it there.
fn speedup_wonder(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let view = &u.local().view;
    let hurry = Command::GreatPersonAction { unit: u.me.handle, action: GreatPersonAction::HurryProduction };
    if u.is_city_tile(u.tile()) && view.accepts(&hurry) {
        return Step::Act(Decision::act(actor(&u.me), Reason::Hurry, hurry));
    }
    let cities: Vec<TileId> = u.sight.own_cities.iter().filter(|c| view.engineer_may_hurry(c.handle)).map(|c| c.tile).collect();
    let best = cities.into_iter().filter(|t| *t == u.tile() || u.can_move_to(*t)).filter_map(|t| u.turns_to(tools, t).filter(|n| *n <= 5).map(|n| (n, t))).min();
    match best {
        Some((_, city)) => head_towards(u, tools, city, Reason::MoveToJob),
        None => Step::Next,
    }
}

/// `automateAddInCapital`: a spaceship part walks to the capital and is installed.
fn add_in_capital(u: &Unit<'_, '_>, tools: &mut PlanTools<'_>) -> Step {
    let Some(capital) = u.sight.capital else {
        return Step::Next;
    };
    let install = Command::BuildSpaceshipPart { unit: u.me.handle };
    if u.tile() == capital && u.local().view.accepts(&install) {
        return Step::Act(Decision::act(actor(&u.me), Reason::ProduceProject, install));
    }
    head_towards(u, tools, capital, Reason::MoveToJob)
}

/// `CivilianUnitAutomation.automateCivilianUnit` for the civilians Unciv
/// knows; None for the Brave New World units it does not (great writers,
/// artists and musicians with their great works, archaeologists, trade units:
/// the classic AI's), and for workers and religious units (their own modules).
pub fn automate_civilian(u: &Unit<'_, '_>, memo: &mut Memo, tools: &mut PlanTools<'_>) -> Option<Decision> {
    let _ = memo;
    let rules = u.rules();
    let def = rules.units.get(u.me.kind.0 as usize)?;
    let step = if def.role == Some(kernel::rules::content::UnitRole::SpaceshipPart) {
        add_in_capital(u, tools)
    } else if def.founder {
        automate_settler(u, tools)
    } else {
        let kind = great_person_kind(u)?;
        match try_run_away(u) {
            Step::Next => automate_great_person(u, kind, tools),
            other => other,
        }
    };
    Some(match step {
        Step::Act(d) => d,
        Step::Done(reason) => Decision::refuse(actor(&u.me), reason),
        Step::Next => Decision::refuse(actor(&u.me), Reason::Idle),
    })
}

/// The great people of Gods & Kings as Unciv automates them.
fn automate_great_person(u: &Unit<'_, '_>, kind: GreatPersonKind, tools: &mut PlanTools<'_>) -> Step {
    let view = &u.local().view;
    let late = u.sight.techs.len() as f32 / u.rules().technologies.len().max(1) as f32 >= 0.55;
    let act = |action: GreatPersonAction| {
        let command = Command::GreatPersonAction { unit: u.me.handle, action };
        view.accepts(&command).then(|| Decision::act(actor(&u.me), Reason::SystemAdvisor, command))
    };
    match kind {
        GreatPersonKind::General | GreatPersonKind::Admiral => return automate_great_general(u, tools),
        GreatPersonKind::Scientist if late => {
            if let Some(d) = act(GreatPersonAction::DiscoverTechnology) {
                return Step::Act(d);
            }
        }
        GreatPersonKind::Merchant if late => {
            if let Step::Act(d) = conduct_trade_mission(u, tools) {
                return Step::Act(d);
            }
        }
        GreatPersonKind::Engineer => {
            if let Step::Act(d) = speedup_wonder(u, tools) {
                return Step::Act(d);
            }
        }
        _ => {}
    }
    if let Step::Act(d) = automate_improvement_placer(u, tools) {
        return Step::Act(d);
    }
    if let Some(d) = act(GreatPersonAction::StartGoldenAge) {
        return Step::Act(d);
    }
    Step::Done(Reason::Idle)
}
