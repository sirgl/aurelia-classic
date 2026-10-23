//! Workers (R-07 AI stage F): jobs only on the player's own tiles that its
//! cities work (the core's `CITY_WORK_RADIUS`), valued by the improvement's
//! yields and the resource it unlocks per turn of work and travel; a better
//! improvement replaces an old one only for a clear gain; two workers never
//! take the same tile (reservation); a worker flees an enemy in sight.
//! Roads first link cities to the capital where the connection gold pays
//! for them; without improvement work, roads join the nearest cities.

use crate::ai::planner::{Decision, PlanTools, TurnBrief, UnitView};
use crate::ai::planner_map::{find_path, step_points, Blockers, KnownMap, PathOutcome};
use crate::ai::planner_units::{can_stop, move_along, nearest_safe_tile, threatened};
use crate::ai::planner_value::{improvement_value, resource_value, Weights};
use crate::ai::state::{AiState, Reservation, UnitTask, WorkerJob};
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::world::city::CITY_WORK_RADIUS;
use crate::ids::{ImprovementId, PlayerId, TileId};
use crate::knowledge::view::{PlayerView, TileSnapshot};
use crate::navigation::cost::TerrainKinds;
use crate::rules::content::ImprovementKind;
use crate::rules::CompiledRuleset;
use crate::derived::tile_yields::TechSet;
use crate::world::builds::{site, Builder};
use crate::world::map::Route;

/// A new improvement replaces an existing one only for this much more.
pub const REPLACE_GAIN: i32 = 20;
/// Roads are built from this round on, with this much gold income.
pub const ROADS_FROM_ROUND: u32 = 40;
pub const ROADS_MIN_INCOME: i32 = 3;
/// Cities this close are joined by roads.
pub const ROAD_CITY_RANGE: i32 = 7;

/// One decision of a worker.
pub fn decide_worker(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    if unit.work.is_some() {
        return Decision::refuse(actor, Reason::Waiting);
    }
    if threatened(view, tools.rules, unit.tile, 2) {
        state.clear_task(unit.id);
        return match nearest_safe_tile(view, tools) {
            Some(safe) if safe != unit.tile => move_along(view, tools, safe, Reason::Retreat),
            _ => Decision::refuse(actor, Reason::Threatened),
        };
    }
    if let Some(UnitTask::Improve { tile, job }) = state.task_of(unit.id).map(|t| t.task) {
        if job_still_wanted(view, tools.rules, tile, job) {
            return work_at(view, tools, tile, job);
        }
        state.clear_task(unit.id);
    }
    let Some((tile, job)) = choose_job(view, state, tools) else {
        if crate::ai::sea_work::is_sea_builder(tools.rules, unit.kind) {
            return Decision::refuse(actor, Reason::NoJob);
        }
        return step_off_city(view, tools.rules);
    };
    state.set_task(unit.id, UnitTask::Improve { tile, job }, view.brief.round);
    tools.counters.tasks_assigned += 1;
    work_at(view, tools, tile, job)
}

fn work_at(
    view: &UnitView<'_, '_>,
    tools: &mut PlanTools<'_>,
    tile: TileId,
    job: WorkerJob,
) -> Decision {
    let unit = view.local.unit;
    if unit.tile != tile {
        return move_along(view, tools, tile, Reason::MoveToJob);
    }
    let actor = Actor::Unit(unit.id);
    match job {
        WorkerJob::Improvement(improvement) => {
            let command = Command::BuildImprovement {
                unit: unit.handle,
                improvement,
            };
            Decision::act(actor, Reason::BuildImprovement, command)
        }
        WorkerJob::Road => Decision::act(
            actor,
            Reason::BuildRoad,
            Command::BuildRoad { unit: unit.handle },
        ),
        WorkerJob::Chop => Decision::act(
            actor,
            Reason::BuildImprovement,
            Command::ChopForest { unit: unit.handle },
        ),
    }
}

fn job_still_wanted(
    view: &UnitView<'_, '_>,
    rules: &CompiledRuleset,
    tile: TileId,
    job: WorkerJob,
) -> bool {
    let known = view.local.view.tile(tile);
    let Some(snapshot) = known.snapshot() else {
        return false;
    };
    if snapshot.owner.is_some_and(|o| o != view.local.player) {
        return false;
    }
    match job {
        WorkerJob::Road => snapshot.route == Route::None,
        WorkerJob::Chop => removable_feature(rules, view.brief, snapshot).is_some(),
        WorkerJob::Improvement(kind) => {
            snapshot.improvement != Some(kind) && site_ok(&view.local.view, rules, view.brief, tile, kind)
        }
    }
}

/// The terrain kind (rules table index) of a map terrain code.
fn terrain_kind(rules: &CompiledRuleset, snapshot: &TileSnapshot) -> Option<u16> {
    let code = snapshot.terrain.0;
    let index = rules
        .terrains
        .iter()
        .position(|t| t.map_code == Some(code))?;
    Some(index as u16)
}

/// May the improvement stand on this tile by its data (valid terrains, or
/// the tile's resource; a feature must be one it allows)?
pub fn valid_on(
    rules: &CompiledRuleset,
    kinds: TerrainKinds,
    kind: ImprovementId,
    snapshot: &TileSnapshot,
) -> bool {
    let Some(def) = rules.improvements.get(kind.0 as usize) else {
        return false;
    };
    if snapshot.city || kinds.enter_cost(snapshot.terrain).is_none() {
        return false;
    }
    let for_resource = snapshot
        .resource
        .is_some_and(|r| def.resources.contains(&r));
    let terrain_ok = match terrain_kind(rules, snapshot) {
        Some(terrain) => def.valid_terrains.iter().any(|t| t.0 == terrain),
        None => false,
    };
    let any_land = def.valid_terrains.is_empty() && def.resources.is_empty();
    let feature_ok = match snapshot.feature {
        Some(feature) => def.valid_features.contains(&feature) || for_resource,
        None => true,
    };
    (for_resource || terrain_ok || any_land) && feature_ok
}

/// Can the player's workers build this improvement now?
pub(crate) fn buildable(rules: &CompiledRuleset, brief: &TurnBrief, kind: ImprovementId) -> bool {
    let Some(def) = rules.improvements.get(kind.0 as usize) else {
        return false;
    };
    let own_unique = def.kind == ImprovementKind::Unique && def.civilization == Some(brief.civ);
    let kind_ok = def.kind == ImprovementKind::Regular || own_unique;
    let tech_ok = def.required_tech.is_none_or(|t| brief.knows(t));
    // The archaeological dig (prompts its builder) is an archaeologist's.
    // A builder of the sea builds the water improvements at no work time
    // and is used up by them; they are marked unbuildable for workers.
    let worker_build = (!def.unbuildable && def.work_turns > 0 || def.placement.water) && !def.placement.prompt_when_complete;
    kind_ok && tech_ok && worker_build
}

/// The core's own site rule over the player's knowledge
/// (`world::builds::site`: technology, terrain, feature removal, resource,
/// ownership), so a chosen build is one the core accepts.
pub(crate) fn site_ok(view: &PlayerView<'_>, rules: &CompiledRuleset, brief: &TurnBrief, tile: TileId, kind: ImprovementId) -> bool {
    let builder = Builder {
        player: brief.player,
        civ: brief.civ,
        techs: TechSet::of(&brief.techs),
    };
    site(view, rules, &builder, tile, kind).is_ok()
}

/// A job a tile offers: what to do first, its gain and its work turns.
#[derive(Debug, Clone, Copy)]
struct JobOffer {
    job: WorkerJob,
    gain: i32,
    turns: i32,
}

/// The best job of a tile: an improvement over what stands there, or —
/// when only a removable feature (forest, jungle) stands in its way —
/// chopping the feature first (the feature's production is lost).
fn best_job(
    view: &PlayerView<'_>,
    rules: &CompiledRuleset,
    brief: &TurnBrief,
    w: &Weights,
    tile: TileId,
) -> Option<JobOffer> {
    let known = view.tile(tile);
    let snapshot = *known.snapshot()?;
    let direct = best_improvement(view, rules, brief, w, tile, &snapshot).map(|(kind, gain)| JobOffer {
        job: WorkerJob::Improvement(kind),
        gain,
        turns: work_turns(rules, kind),
    });
    let Some(feature) = removable_feature(rules, brief, &snapshot) else {
        return direct;
    };
    let cleared = TileSnapshot {
        feature: None,
        ..snapshot
    };
    let (kind, gain) = best_improvement(view, rules, brief, w, tile, &cleared)?;
    let lost = w.production * feature.yields.production * 10;
    let removal = feature.removal?;
    let chop = JobOffer {
        job: WorkerJob::Chop,
        gain: gain - lost,
        turns: i32::from(crate::world::builds::work_turns(removal.work_time)) + work_turns(rules, kind),
    };
    let chop_is_better = direct.is_none_or(|d| chop.gain * d.turns > d.gain * chop.turns);
    if chop.gain > 0 && chop_is_better {
        return Some(chop);
    }
    direct
}

fn work_turns(rules: &CompiledRuleset, kind: ImprovementId) -> i32 {
    i32::from(rules.improvements[kind.0 as usize].work_turns.max(1))
}

/// The tile's feature when workers may remove it (its removal technology
/// known).
fn removable_feature<'r>(
    rules: &'r CompiledRuleset,
    brief: &TurnBrief,
    snapshot: &TileSnapshot,
) -> Option<&'r crate::rules::compiled::FeatureDefinition> {
    let def = rules.features.get(snapshot.feature?.0 as usize)?;
    let removal = def.removal?;
    removal.tech.is_none_or(|t| brief.knows(t)).then_some(def)
}

/// Gain of the best improvement for a tile over what stands there.
fn best_improvement(
    view: &PlayerView<'_>,
    rules: &CompiledRuleset,
    brief: &TurnBrief,
    w: &Weights,
    tile: TileId,
    snapshot: &TileSnapshot,
) -> Option<(ImprovementId, i32)> {
    let resource_gain = |kind: ImprovementId| match snapshot.resource {
        Some(r) if rules.improvements[kind.0 as usize].resources.contains(&r) => {
            resource_value(rules, r, w)
        }
        _ => 0,
    };
    let current = match snapshot.improvement {
        Some(kind) => improvement_value(rules, kind, w) + resource_gain(kind),
        None => 0,
    };
    let mut best: Option<(ImprovementId, i32)> = None;
    for index in 0..rules.improvements.len() {
        let kind = ImprovementId(index as u16);
        if snapshot.improvement == Some(kind)
            || !buildable(rules, brief, kind)
            || !site_ok(view, rules, brief, tile, kind)
        {
            continue;
        }
        let gain = improvement_value(rules, kind, w) + resource_gain(kind) - current;
        let needed = if snapshot.improvement.is_some() {
            REPLACE_GAIN
        } else {
            1
        };
        if gain >= needed && best.is_none_or(|(_, g)| gain > g) {
            best = Some((kind, gain));
        }
    }
    best
}

fn job_tiles(reservation: Reservation) -> Option<TileId> {
    match reservation {
        Reservation::Job(tile) => Some(tile),
        _ => None,
    }
}

/// The best job: improvement gain per turn of work and travel; roads
/// between near cities when nothing else is left.
fn choose_job(
    view: &UnitView<'_, '_>,
    state: &AiState,
    tools: &mut PlanTools<'_>,
) -> Option<(TileId, WorkerJob)> {
    let unit = view.local.unit;
    let reserved = state.reserved_tiles(job_tiles, Some(unit.id));
    // A builder of the sea works only the water; a land worker only the land.
    let sea = crate::ai::sea_work::is_sea_builder(tools.rules, unit.kind);
    if !sea {
        if let Some(tile) = connection_job(view, state, &reserved, tools) {
            return Some((tile, WorkerJob::Road));
        }
    }
    let w = Weights::for_empire(state.strategy.current, view.brief.gold_income, view.brief.turn_happiness);
    let dims = view.local.view.dimensions();
    let mut best: Option<(i32, TileId, WorkerJob)> = None;
    for tile in own_work_tiles(view.brief, &view.local.view, view.local.player) {
        if reserved.binary_search(&tile).is_ok() || view.local.view.terrain_kinds().is_water(view.local.view.tile(tile).snapshot().map_or(0, |s| s.terrain.0)) != sea {
            continue;
        }
        tools.counters.candidates += 1;
        let Some(offer) = best_job(&view.local.view, tools.rules, view.brief, &w, tile) else {
            continue;
        };
        let travel = dims.distance(unit.tile, tile).unwrap_or(99);
        let score = offer.gain * 10 / (offer.turns + 2 * travel);
        if best.is_none_or(|(s, t, _)| score > s || (score == s && tile < t)) {
            best = Some((score, tile, offer.job));
        }
    }
    if let Some((_, tile, job)) = best {
        return Some((tile, job));
    }
    if sea {
        return None;
    }
    road_job(view, &reserved, tools).map(|tile| (tile, WorkerJob::Road))
}

/// A road that links an own city to the capital and pays for itself: its
/// connection gold (the core's formula in `derived::maintenance`: a base,
/// a share of the capital's population and of the city's) beats the
/// maintenance of its new road tiles in own land. One worker at a time
/// builds such roads; without them the AI's cities stayed unconnected and
/// its treasury ran dry under the ruleset's maintenance.
fn connection_job(
    view: &UnitView<'_, '_>,
    state: &AiState,
    reserved: &[TileId],
    tools: &mut PlanTools<'_>,
) -> Option<TileId> {
    let unit = view.local.unit;
    let road_worker = state.tasks().iter().any(|t| {
        t.unit != unit.id && matches!(t.task, UnitTask::Improve { job: WorkerJob::Road, .. })
    });
    if road_worker || !roads_known(view, tools.rules) {
        return None;
    }
    let known = &view.local.view;
    let capital = known.own_capital()?;
    let connected = known.own_connected_cities();
    let capital_city = view.brief.cities.iter().find(|c| c.handle == capital)?;
    let dims = known.dimensions();
    let mut unconnected: Vec<_> = view.brief.cities.iter().filter(|c| !connected.contains(&c.handle)).collect();
    unconnected.sort_by_key(|c| (dims.distance(c.tile, capital_city.tile), c.id));
    for city in unconnected {
        let gold_times100 = crate::derived::maintenance::CONNECTION_BASE_GOLD_TIMES100
            + i64::from(capital_city.population) * crate::derived::maintenance::CONNECTION_CAPITAL_POP_TIMES100
            + i64::from(city.population) * crate::derived::maintenance::CONNECTION_CITY_POP_TIMES100;
        let Some(roadless) = roadless_way(view, tools, city.tile, capital_city.tile) else {
            continue;
        };
        let own_new = roadless
            .iter()
            .filter(|t| known.tile(**t).snapshot().is_some_and(|s| s.owner == Some(view.local.player)))
            .count() as i64;
        if gold_times100 <= own_new * crate::derived::maintenance::ROAD_MAINTENANCE * 100 {
            continue;
        }
        if let Some(tile) = roadless.into_iter().find(|t| reserved.binary_search(t).is_err()) {
            return Some(tile);
        }
    }
    None
}

/// The player knows the technology of roads (The Wheel).
fn roads_known(view: &UnitView<'_, '_>, rules: &CompiledRuleset) -> bool {
    rules.roles.road_tech.is_none_or(|t| view.brief.knows(t))
}

/// The roadless tiles (no city) of the known land way between two tiles.
fn roadless_way(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, from: TileId, to: TileId) -> Option<Vec<TileId>> {
    let known = &view.local.view;
    let outcome = find_path(
        &KnownMap::land(known),
        from,
        to,
        &Blockers::default(),
        tools.scratch,
        tools.settings.budget.search(),
        &mut tools.counters.search,
    );
    let PathOutcome::Found { tiles, .. } = outcome else {
        return None;
    };
    let open = |t: &TileId| known.tile(*t).snapshot().is_some_and(|s| s.route == Route::None && !s.city);
    Some(tiles.into_iter().filter(open).collect())
}

/// A worker without work does not stand on a city tile (it would block the
/// settlers and workers passing through): it steps to a free own tile next
/// to it.
fn step_off_city(view: &UnitView<'_, '_>, rules: &CompiledRuleset) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    let on_city = view.local.own_cities.iter().any(|c| c.tile == unit.tile);
    if !on_city || unit.moves_left == 0 {
        return Decision::refuse(actor, Reason::NoJob);
    }
    let dims = view.local.view.dimensions();
    for tile in dims.neighbors(unit.tile) {
        let known = view.local.view.tile(tile);
        let own = known
            .snapshot()
            .is_some_and(|s| s.owner == Some(view.local.player) && !s.city);
        let passable = step_points(&KnownMap::land(&view.local.view), unit.tile, tile).is_some();
        if own && passable && can_stop(view, rules, tile) {
            let command = Command::MoveUnit {
                unit: unit.handle,
                destination: tile,
            };
            return Decision::act(actor, Reason::Idle, command);
        }
    }
    Decision::refuse(actor, Reason::NoJob)
}

/// The player's own tiles its cities work (sorted, no city tiles).
fn own_work_tiles(brief: &TurnBrief, view: &PlayerView<'_>, player: PlayerId) -> Vec<TileId> {
    let dims = view.dimensions();
    let mut tiles = Vec::new();
    for city in &brief.cities {
        for tile in crate::math::hex_disc(dims, city.tile, CITY_WORK_RADIUS) {
            let known = view.tile(tile);
            let owned = known
                .snapshot()
                .is_some_and(|s| s.owner == Some(player) && !s.city);
            if owned {
                tiles.push(tile);
            }
        }
    }
    tiles.sort();
    tiles.dedup();
    tiles
}

/// The first roadless tile on the known way from the city nearest to the
/// worker to its nearest other city.
fn road_job(
    view: &UnitView<'_, '_>,
    reserved: &[TileId],
    tools: &mut PlanTools<'_>,
) -> Option<TileId> {
    let brief = view.brief;
    if brief.round < ROADS_FROM_ROUND || brief.gold_income < ROADS_MIN_INCOME || !roads_known(view, tools.rules) {
        return None;
    }
    let (home, other) = road_ends(view)?;
    let limit = tools.settings.budget.search();
    let blockers = Blockers::default();
    let known = &view.local.view;
    let outcome = find_path(
        &KnownMap::land(known),
        home,
        other,
        &blockers,
        tools.scratch,
        limit,
        &mut tools.counters.search,
    );
    let PathOutcome::Found { tiles, .. } = outcome else {
        return None;
    };
    tiles.into_iter().find(|t| {
        let tile = known.tile(*t);
        let open = tile
            .snapshot()
            .is_some_and(|s| s.route == Route::None && !s.city);
        open && reserved.binary_search(t).is_err()
    })
}

/// The city nearest to the worker and its nearest other city within
/// `ROAD_CITY_RANGE`.
fn road_ends(view: &UnitView<'_, '_>) -> Option<(TileId, TileId)> {
    let brief = view.brief;
    let dims = view.local.view.dimensions();
    let home = brief.nearest_city(dims, view.local.unit.tile)?;
    let near = |c: &&crate::ai::planner::CityBrief| {
        let distance = dims.distance(c.tile, home.tile);
        c.id != home.id && distance.is_some_and(|d| d <= ROAD_CITY_RANGE)
    };
    let other = brief
        .cities
        .iter()
        .filter(near)
        .min_by_key(|c| (dims.distance(c.tile, home.tile), c.id))?;
    Some((home.tile, other.tile))
}
