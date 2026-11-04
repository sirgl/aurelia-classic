//! Worker decisions from the observation (platform/structure.md §22–23): what an
//! idle worker builds and where. The site and yield rules are the core's
//! own (`world::builds::site`, `derived::tile_yields`), read through the
//! player's knowledge (`PlayerView` as `TileFacts`): no hidden state.
//!
//! ```text
//! candidates  own tiles within WORKER_SEARCH_RADIUS of the worker
//! value       best improvement by the weighted yield gain per worker turn
//!             (food 12, production 8, gold 5, science 6, culture 6,
//!             faith 5 per point, ×100 / turns), minus 10 per hex away;
//!             connecting a known resource adds STRATEGIC_CONNECTION, or
//!             LUXURY_CONNECTION for a luxury the empire lacks
//! skip        tiles another own worker is already working on
//! ```
//!
//! Deterministic: candidates in tile order, ties to the lower tile id and
//! the lower improvement id.

use crate::command::Command;
use crate::derived::tile_yields::{plot_yields, plot_yields_with, TechSet};
use crate::ids::{ImprovementId, TileId};
use crate::knowledge::observation::{AIObservation, OwnUnit};
use crate::math::hex_disc;
use crate::rules::content::{Domain, ImprovementKind, ResourceClass, Yields};
use crate::rules::CompiledRuleset;
use crate::world::builds::{site, Builder};
use crate::world::facts::TileFacts;

/// How far a worker looks for work.
pub const WORKER_SEARCH_RADIUS: i32 = 6;
/// Value lost per hex the worker must walk.
pub const DISTANCE_PENALTY: i32 = 10;
/// Yield value of connecting a strategic resource (its units).
pub const STRATEGIC_CONNECTION: i32 = 60;
/// Yield value of connecting a luxury the empire does not have (happiness).
pub const LUXURY_CONNECTION: i32 = 80;

/// What a worker should do and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerJob {
    pub tile: TileId,
    pub improvement: ImprovementId,
    /// Weighted gain per worker turn, distance included.
    pub score: i32,
}

/// Weighted value of yields for the default economy.
pub fn yield_value(y: &Yields) -> i32 {
    y.food * 12 + y.production * 8 + y.gold * 5 + y.science * 6 + y.culture * 6 + y.module * 5
}

/// The best job for an idle land worker of the observing player (None:
/// nothing worth building in reach).
pub fn best_worker_job(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    worker: &OwnUnit,
) -> Option<WorkerJob> {
    let is_land_worker = rules
        .units
        .get(worker.kind.0 as usize)
        .map(|u| u.builder && u.domain == Domain::Land)
        .unwrap_or(false);
    if !is_land_worker || worker.work.is_some() {
        return None;
    }
    let builder = Builder {
        player: obs.player,
        nation: obs.economy.nation,
        techs: TechSet::of(&obs.economy.techs),
    };
    let busy: Vec<TileId> = obs
        .own_units
        .iter()
        .filter_map(|u| u.work.map(|w| w.tile))
        .collect();
    let dims = obs.view.map_dimensions();
    let mut best: Option<WorkerJob> = None;
    for tile in hex_disc(dims, worker.tile, WORKER_SEARCH_RADIUS) {
        if obs.view.owner(tile) != Some(obs.player) || busy.contains(&tile) {
            continue;
        }
        let Some((improvement, gain)) = best_build(obs, rules, &builder, tile) else {
            continue;
        };
        let distance = dims.distance(worker.tile, tile).unwrap_or(0);
        let score = gain - distance * DISTANCE_PENALTY;
        if score <= 0 {
            continue;
        }
        let better = match best {
            None => true,
            Some(b) => score > b.score,
        };
        if better {
            best = Some(WorkerJob {
                tile,
                improvement,
                score,
            });
        }
    }
    best
}

/// The best improvement on one tile by gain per worker turn.
fn best_build(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    builder: &Builder,
    tile: TileId,
) -> Option<(ImprovementId, i32)> {
    let now = yield_value(&plot_yields(&obs.view, rules, tile, &builder.techs).at_least(0));
    let mut best: Option<(ImprovementId, i32)> = None;
    for (i, def) in rules.improvements.iter().enumerate() {
        let regular = matches!(def.kind, ImprovementKind::Regular | ImprovementKind::Unique);
        if !regular || def.unbuildable || def.placement.water || def.placement.prompt_when_complete {
            continue;
        }
        let improvement = ImprovementId(i as u16);
        let Ok(plan) = site(&obs.view, rules, builder, tile, improvement) else {
            continue;
        };
        let removes = plan.removes.is_some();
        let then = plot_yields_with(&obs.view, rules, tile, &builder.techs, improvement, removes);
        let value = yield_value(&then.at_least(0)) - now + connection_value(obs, rules, builder, tile, improvement);
        let gain = value * 100 / i32::from(plan.work_turns.max(1));
        if gain > 0 && best.map(|(_, g)| gain > g).unwrap_or(true) {
            best = Some((improvement, gain));
        }
    }
    best
}

/// What connecting the tile's resource with `improvement` is worth: 0 when
/// the resource is unknown, not the improvement's, or connected already.
fn connection_value(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    builder: &Builder,
    tile: TileId,
    improvement: ImprovementId,
) -> i32 {
    let Some(resource) = obs.view.resource(tile) else {
        return 0;
    };
    let Some(def) = rules.resources.get(resource.0 as usize) else {
        return 0;
    };
    let connects = |i: Option<ImprovementId>| i.is_some_and(|i| def.improvements.contains(&i));
    let known = crate::derived::tile_yields::resource_revealed(rules, resource, &builder.techs);
    if !known || !connects(Some(improvement)) || connects(obs.view.improvement(tile)) {
        return 0;
    }
    match def.class {
        Some(ResourceClass::Strategic) => STRATEGIC_CONNECTION,
        Some(ResourceClass::Luxury) if obs.economy.resources.total(resource) == 0 => LUXURY_CONNECTION,
        _ => 0,
    }
}

/// The order that carries a job out: build on the spot, or walk there.
pub fn worker_command(worker: &OwnUnit, job: &WorkerJob) -> Command {
    if worker.tile == job.tile {
        Command::BuildImprovement {
            unit: worker.handle,
            improvement: job.improvement,
        }
    } else {
        Command::MarchTo {
            unit: worker.handle,
            destination: job.tile,
        }
    }
}

/// Orders for every idle land worker of the observing player.
pub fn worker_commands(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let mut out = Vec::new();
    for worker in &obs.own_units {
        if worker.goal.is_some() {
            continue;
        }
        if let Some(job) = best_worker_job(obs, rules, worker) {
            out.push(worker_command(worker, &job));
        }
    }
    out
}
