//! Workers (R-07 AI stage F): jobs only on the player's own tiles that its
//! cities work (the core's `CITY_WORK_RADIUS`), valued by the improvement's
//! yields and the resource it unlocks per turn of work and travel; a better
//! improvement replaces an old one only for a clear gain; two workers never
//! take the same tile (reservation); a worker flees an enemy in sight.
//! Roads first link cities to the capital where the connection gold pays
//! for them; without improvement work, roads join the nearest cities.
//! `WorkersFit` (the `workers-fit` variant, `standard` since the tenth
//! promotion; classic/docs/ai-workers.md):
//! a worker leaves the reach of a hostile unit also in the middle of a
//! build, waits in an own city when it has no job, repairs pillaged
//! improvements, and the allocator wants a worker only for the work the
//! rules still offer (`WorkLoad`).

use crate::ai::planner::{Decision, PlanTools, TurnBrief, UnitView};
use crate::ai::planner_map::{explore, find_path, step_points, Blockers, KnownMap, PathOutcome, Visit};
use crate::ai::planner_units::{blockers, can_stop, move_along, nearest_safe_tile, threatened};
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
use crate::world::unit::MOVE_DENOMINATOR;

/// A new improvement replaces an existing one only for this much more.
pub const REPLACE_GAIN: i32 = 20;
/// Roads are built from this round on, with this much gold income (a round of a Standard game: another speed reads its round as the same share of
/// its game, `SpeedValues::standard_round`).
pub const ROADS_FROM_ROUND: u32 = 40;
pub const ROADS_MIN_INCOME: i32 = 3;
/// Cities this close are joined by roads.
pub const ROAD_CITY_RANGE: i32 = 7;

/// The switches of the `workers-fit` variant (classic/docs/ai-workers.md); the
/// game's AI carries them since the tenth promotion (`Variant::workers_fit`),
/// `pre-workers` none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkersFit {
    /// The allocator wants a worker more only while the work the job finder
    /// offers in the cities' radius gives every worker, the new one counted,
    /// the jobs it counts a worker to serve (`WorkLoad`).
    pub demand: bool,
    /// A worker without a job waits in the nearest own city it may stop in
    /// (safe from capture, off the tiles of the others' jobs) instead of
    /// stepping off the city.
    pub park: bool,
    /// A worker leaves a hostile military unit's reach (the unit's movement in
    /// the rules) before it waits on its build: a build under way does not
    /// keep it in the open.
    pub flee: bool,
    /// A pillaged own improvement is a job: its repair (the site rule's repair
    /// time), worth the improvement's whole value.
    pub repair: bool,
}

impl WorkersFit {
    pub const NONE: WorkersFit = WorkersFit { demand: false, park: false, flee: false, repair: false };
    pub const ALL: WorkersFit = WorkersFit { demand: true, park: true, flee: true, repair: true };

    pub fn any(self) -> bool {
        self != Self::NONE
    }
}

/// One decision of a worker.
pub fn decide_worker(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    let fit = view.brief.workers;
    // `flee`: the reach of the hostile units first, a build under way too
    // (the most captured workers were waiting on their build).
    if fit.flee && !on_own_city(view) && in_hostile_reach(view, tools.rules, unit.tile) {
        state.clear_task(unit.id);
        if unit.moves_left == 0 {
            return Decision::refuse(actor, Reason::Threatened);
        }
        if let Some(safe) = flee_tile(view, tools) {
            let command = Command::MoveUnit { unit: unit.handle, destination: safe };
            return Decision::act(actor, Reason::Retreat, command);
        }
        return match nearest_safe_tile(view, tools) {
            Some(safe) if safe != unit.tile => move_along(view, tools, safe, Reason::Retreat),
            _ => Decision::refuse(actor, Reason::Threatened),
        };
    }
    if unit.work.is_some() {
        return Decision::refuse(actor, Reason::Waiting);
    }
    if !fit.flee && threatened(view, tools.rules, unit.tile, 2) {
        state.clear_task(unit.id);
        return match nearest_safe_tile(view, tools) {
            Some(safe) if safe != unit.tile => move_along(view, tools, safe, Reason::Retreat),
            _ => Decision::refuse(actor, Reason::Threatened),
        };
    }
    if let Some(UnitTask::Improve { tile, job }) = state.task_of(unit.id).map(|t| t.task) {
        // `flee`: a job in a hostile unit's reach waits until it is clear.
        let exposed = fit.flee && in_hostile_reach(view, tools.rules, tile);
        if !exposed && job_still_wanted(view, tools.rules, tile, job) {
            let decision = work_at(view, tools, tile, job);
            // With jobs by path a job the unit's way no longer reaches is
            // given up; the next one is a job the way reaches.
            if !view.brief.effects.jobs || decision.reason != Reason::NoPath {
                return decision;
            }
            tools.counters.tasks_dropped += 1;
        }
        state.clear_task(unit.id);
    }
    let Some((tile, job)) = choose_job(view, state, tools) else {
        if crate::ai::sea_work::is_sea_builder(tools.rules, unit.kind) {
            return Decision::refuse(actor, Reason::NoJob);
        }
        if fit.park {
            return park(view, tools);
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
            let repair = view.brief.workers.repair && view.local.view.known_pillaged(tile);
            (snapshot.improvement != Some(kind) || repair) && site_ok(&view.local.view, rules, view.brief, tile, kind)
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
    let own_unique = def.kind == ImprovementKind::Unique && def.nation == Some(brief.nation);
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
        nation: brief.nation,
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
    if let Some(offer) = repair_job(view, rules, brief, w, tile, &snapshot) {
        return Some(offer);
    }
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

/// `repair`: the repair of the tile's pillaged improvement, worth all the
/// improvement gives (a pillaged one gives nothing), in the site rule's
/// repair time. None without the switch or a pillaged improvement.
fn repair_job(
    view: &PlayerView<'_>,
    rules: &CompiledRuleset,
    brief: &TurnBrief,
    w: &Weights,
    tile: TileId,
    snapshot: &TileSnapshot,
) -> Option<JobOffer> {
    let kind = snapshot.improvement?;
    if !brief.workers.repair || !view.known_pillaged(tile) || !buildable(rules, brief, kind) {
        return None;
    }
    let builder = Builder { player: brief.player, nation: brief.nation, techs: TechSet::of(&brief.techs) };
    let plan = site(view, rules, &builder, tile, kind).ok()?;
    let resource = match snapshot.resource {
        Some(r) if rules.improvements[kind.0 as usize].resources.contains(&r) => resource_value(rules, r, w),
        _ => 0,
    };
    let gain = improvement_value(rules, kind, w) + resource;
    (gain > 0).then_some(JobOffer { job: WorkerJob::Improvement(kind), gain, turns: i32::from(plan.work_turns.max(1)) })
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
    // With jobs by path a tile the unit's own way does not reach is no job
    // for it: closed while the known map and the units it sees close it.
    let reach = view.brief.effects.jobs.then(|| Reach::of(view, tools));
    // A builder of the sea works only the water; a land worker only the land.
    let sea = crate::ai::sea_work::is_sea_builder(tools.rules, unit.kind);
    if !sea {
        if let Some(tile) = connection_job(view, state, &reserved, tools, reach.as_ref()) {
            return Some((tile, WorkerJob::Road));
        }
    }
    let w = Weights::for_empire(state.strategy.current, view.brief.gold_income, view.brief.turn_happiness);
    let dims = view.local.view.dimensions();
    let mut best: Option<(i32, TileId, WorkerJob)> = None;
    let flee = view.brief.workers.flee;
    for tile in own_work_tiles(view.brief, &view.local.view, view.local.player) {
        if reserved.binary_search(&tile).is_ok() || view.local.view.terrain_kinds().is_water(view.local.view.tile(tile).snapshot().map_or(0, |s| s.terrain.0)) != sea {
            continue;
        }
        if flee && in_hostile_reach(view, tools.rules, tile) {
            continue;
        }
        // The way in plain steps of the unit's own path, or the distance.
        let travel = match &reach {
            Some(reach) => match reach.steps(tile) {
                Some(steps) => steps,
                None => continue,
            },
            None => dims.distance(unit.tile, tile).unwrap_or(99),
        };
        tools.counters.candidates += 1;
        let Some(offer) = best_job(&view.local.view, tools.rules, view.brief, &w, tile) else {
            continue;
        };
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
    road_job(view, &reserved, tools, reach.as_ref()).map(|tile| (tile, WorkerJob::Road))
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
    reach: Option<&Reach>,
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
        if let Some(tile) = roadless.into_iter().find(|t| reserved.binary_search(t).is_err() && reach.is_none_or(|r| r.steps(*t).is_some())) {
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

/// The worker stands on a city of its owner.
fn on_own_city(view: &UnitView<'_, '_>) -> bool {
    view.brief.cities.iter().any(|c| c.tile == view.local.unit.tile)
}

/// `flee`: a military unit of a player at war with the worker's owner whose
/// movement in the rules reaches the worker's tile (a capture is a step onto
/// it). The game's AI flees a fixed two tiles, and only between builds.
fn in_hostile_reach(view: &UnitView<'_, '_>, rules: &CompiledRuleset, tile: TileId) -> bool {
    hostile_gap(view, rules, tile) <= 0
}

/// The hostile military units the worker sees, with their reach in tiles
/// (their movement in the rules).
fn hostiles<'v>(view: &'v UnitView<'_, '_>, rules: &'v CompiledRuleset) -> impl Iterator<Item = (TileId, i32)> + 'v {
    view.local.foreign_units.iter().filter_map(move |u| {
        let at_war = view.local.relation(u.owner).is_some_and(|r| r.at_war);
        let military = crate::ai::planner::role_of(rules, u.kind) == crate::ai::planner::AiRole::Military;
        let reach = rules.units.get(u.kind.0 as usize).map_or(1, |d| i32::from(d.movement.max(1)));
        (at_war && military).then_some((u.tile, reach))
    })
}

/// Tiles between `tile` and the nearest hostile unit's reach: 0 or less in
/// its reach, `i32::MAX` with no hostile unit in sight.
fn hostile_gap(view: &UnitView<'_, '_>, rules: &CompiledRuleset, tile: TileId) -> i32 {
    let dims = view.local.view.dimensions();
    hostiles(view, rules).map(|(at, reach)| dims.distance(at, tile).unwrap_or(i32::MAX) - reach).min().unwrap_or(i32::MAX)
}

/// `flee`: where the worker gets out of every hostile unit's reach this turn,
/// among the tiles its moves left reach by its own path search: an own city
/// first (a civilian in a city is not captured while the city stands), else
/// the tile farthest from the hostile units' reach. None: no tile it reaches
/// is out of their reach.
fn flee_tile(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>) -> Option<TileId> {
    let unit = view.local.unit;
    let rules = tools.rules;
    let map = KnownMap::of_unit(&view.local.view, unit.handle);
    let moves = u32::from(unit.moves_left);
    let mut tiles = Vec::new();
    let mut visit = |tile: TileId, cost: u32| {
        if cost > moves {
            return Visit::Stop;
        }
        tiles.push(tile);
        Visit::Continue
    };
    explore(&map, unit.tile, &blockers(view), tools.scratch, tools.settings.budget.search(), &mut tools.counters.search, &mut visit);
    let city = |tile: TileId| view.brief.cities.iter().any(|c| c.tile == tile);
    tiles
        .into_iter()
        .filter(|t| *t != unit.tile && can_stop(view, rules, *t))
        .map(|t| (city(t), hostile_gap(view, rules, t), t))
        .filter(|(city, gap, _)| *city || *gap > 0)
        .max_by_key(|(city, gap, t)| (*city, *gap, std::cmp::Reverse(t.0)))
        .map(|(_, _, t)| t)
}

/// `park`: a worker without a job waits in a city of its owner, the nearest
/// one no other own civilian stands in (one civilian a tile), safe from
/// capture and off the tiles of the other workers' jobs. It stays where it
/// stands when none is free.
fn park(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    if on_own_city(view) {
        return Decision::refuse(actor, Reason::NoJob);
    }
    let rules = tools.rules;
    let dims = view.local.view.dimensions();
    let civilian = |kind| crate::world::stacking::stack_class(rules, kind);
    let mine = civilian(unit.kind);
    let taken = |tile: TileId| view.brief.units.iter().any(|u| u.id != unit.id && u.tile == tile && civilian(u.kind) == mine);
    let free = view
        .brief
        .cities
        .iter()
        .filter(|c| !taken(c.tile) && (dims.distance(c.tile, unit.tile).unwrap_or(i32::MAX) > 1 || can_stop(view, rules, c.tile)))
        .min_by_key(|c| (dims.distance(c.tile, unit.tile).unwrap_or(i32::MAX), c.id));
    match free {
        Some(city) => move_along(view, tools, city.tile, Reason::Idle),
        None => Decision::refuse(actor, Reason::NoJob),
    }
}

/// The work a worker more would find (`WorkersFit::demand`, the allocator's
/// worker): the job finder's offers on the own land tiles in the cities'
/// work radius (the rules' improvements, the player's technologies, the site
/// rule, the gain over what stands; repairs with `repair`), in their build
/// turns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkLoad {
    /// Build turns of all the offers.
    pub turns: u32,
    /// The offers.
    pub jobs: u32,
    /// Build turns of a job: the mean of the offers, or with none the mean
    /// of the improvements the workers may build now.
    pub mean: u32,
}

impl WorkLoad {
    pub fn of(view: &PlayerView<'_>, rules: &CompiledRuleset, brief: &TurnBrief, strategy: crate::ai::state::Strategy) -> WorkLoad {
        let w = Weights::for_empire(strategy, brief.gold_income, brief.turn_happiness);
        let kinds = view.terrain_kinds();
        let (mut turns, mut jobs) = (0u32, 0u32);
        for tile in own_work_tiles(brief, view, brief.player) {
            let land = view.tile(tile).snapshot().is_some_and(|s| !kinds.is_water(s.terrain.0));
            if let Some(offer) = land.then(|| best_job(view, rules, brief, &w, tile)).flatten() {
                turns += offer.turns.max(1) as u32;
                jobs += 1;
            }
        }
        let mean = if jobs > 0 {
            turns.div_ceil(jobs)
        } else {
            let own: Vec<u32> = (0..rules.improvements.len())
                .map(|i| ImprovementId(i as u16))
                .filter(|k| buildable(rules, brief, *k) && !rules.improvements[k.0 as usize].placement.water)
                .map(|k| work_turns(rules, k) as u32)
                .collect();
            (own.iter().sum::<u32>()).div_ceil(own.len().max(1) as u32).max(1)
        };
        WorkLoad { turns, jobs, mean }
    }

    /// Does a worker more, ready in `delay` rounds, find its share of the
    /// work? `workers` land workers live or are queued, `settlers` settlers
    /// too; a settler brings a new city's first ring of tiles
    /// (`CITY_TERRITORY_RADIUS`) at the mean build turns; the work the
    /// workers do in the meantime is gone. The share is `per_worker` jobs
    /// of the mean build turns, for each worker the new one counted.
    pub fn wants(&self, workers: usize, settlers: usize, delay: u32, per_worker: u32) -> bool {
        let r = crate::derived::yields::CITY_TERRITORY_RADIUS as u32;
        let ring = 3 * r * (r + 1);
        let coming = settlers as u32 * ring * self.mean;
        let done = workers as u32 * delay;
        let left = (self.turns + coming).saturating_sub(done);
        left >= per_worker * self.mean * (workers as u32 + 1)
    }
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
    reach: Option<&Reach>,
) -> Option<TileId> {
    let brief = view.brief;
    if brief.speed.standard_round(brief.round) < ROADS_FROM_ROUND || brief.gold_income < ROADS_MIN_INCOME || !roads_known(view, tools.rules) {
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
        open && reserved.binary_search(t).is_err() && reach.is_none_or(|r| r.steps(*t).is_some())
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

/// Where the unit's own way leads (`EffectChecks::jobs`): every tile one
/// search over the known map reaches with the unit's movement profile
/// around the foreign units it sees, with its price in move points. A tile
/// it does not reach is no job: the same search would find no path there.
pub(crate) struct Reach {
    /// Sorted by tile.
    points: Vec<(TileId, u32)>,
}

impl Reach {
    pub(crate) fn of(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>) -> Reach {
        let unit = view.local.unit;
        let map = KnownMap::of_unit(&view.local.view, unit.handle);
        let mut points = Vec::new();
        let mut visit = |tile: TileId, cost: u32| {
            points.push((tile, cost));
            Visit::Continue
        };
        explore(&map, unit.tile, &blockers(view), tools.scratch, tools.settings.budget.search(), &mut tools.counters.search, &mut visit);
        points.sort();
        points.dedup_by_key(|p| p.0);
        Reach { points }
    }

    /// The way to `tile` in plain steps (a step of one move each); None when
    /// the way does not reach it.
    pub(crate) fn steps(&self, tile: TileId) -> Option<i32> {
        let at = self.points.binary_search_by_key(&tile, |p| p.0).ok()?;
        Some(self.points[at].1.div_ceil(u32::from(MOVE_DENOMINATOR)) as i32)
    }
}

/// The jobs a worker could take on `tiles` now: the land tiles where the
/// workers' own job finder (`best_job`: the rules' improvements, the
/// player's technologies, the site rule, the gain over what stands there)
/// offers one. The allocator's demand for workers (`EffectChecks::demand`).
pub fn open_jobs(view: &PlayerView<'_>, rules: &CompiledRuleset, brief: &TurnBrief, strategy: crate::ai::state::Strategy, tiles: impl Iterator<Item = TileId>) -> u32 {
    let w = Weights::for_empire(strategy, brief.gold_income, brief.turn_happiness);
    let kinds = view.terrain_kinds();
    let land = |tile: TileId| view.tile(tile).snapshot().is_some_and(|s| !s.city && !kinds.is_water(s.terrain.0));
    tiles.filter(|t| land(*t) && best_job(view, rules, brief, &w, *t).is_some()).count() as u32
}

#[cfg(test)]
mod tests {
    use super::WorkLoad;

    /// A worker more needs its share of the work: the jobs a worker serves at
    /// the mean build turns, for every worker the new one counted; the work
    /// done before it is ready is gone, and a settler brings a city's first
    /// ring of tiles.
    #[test]
    fn a_worker_more_needs_its_share_of_the_work() {
        let load = WorkLoad { turns: 56, jobs: 8, mean: 7 };
        assert!(load.wants(1, 0, 0, 4), "56 turns hold two workers' 4 × 7");
        assert!(!load.wants(2, 0, 0, 4), "but not three");
        assert!(!load.wants(1, 0, 1, 4), "one round of the first worker's work is gone");
        assert!(load.wants(2, 1, 0, 4), "a settler's ring (6 × 7 turns) holds the third");
    }
}
