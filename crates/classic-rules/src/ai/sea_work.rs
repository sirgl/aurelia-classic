//! Work boats for the sea deposits inside the borders (classic/docs/ai-economy.md).
//!
//! A builder that works on water (a work boat, used up by its job) is built
//! for the deposits in the borders that only it can improve: a luxury the
//! empire lacks (the happiness of it at the price of a point) and the yields
//! a food or production deposit gains. The allocator orders the boat by the
//! value of the first deposit no boat is meant for (`sea_values`), the
//! workers send it to a water tile (`planner_workers`), and the land
//! workers' count leaves boats out. Everything is read from the ruleset's
//! declared builders, resources and improvements; no name appears.

use crate::ai::planner::TurnBrief;
use crate::ai::planner_workers::{buildable, site_ok};
use crate::ai::value::{Centi, Prices};
use crate::ids::{ResourceId, UnitKindId};
use crate::knowledge::observation::AIObservation;
use crate::rules::content::{Domain, ResourceClass};
use crate::rules::CompiledRuleset;

/// Rounds a work boat takes to reach its deposit and finish the job.
pub const SEA_JOB_ROUNDS: u32 = 4;

/// A builder that works on water.
pub fn is_sea_builder(rules: &CompiledRuleset, kind: UnitKindId) -> bool {
    rules.units.get(kind.0 as usize).is_some_and(|u| u.builder && u.domain == Domain::Sea)
}

/// What the water deposits inside the borders are worth to improve, best
/// first, one entry per deposit a builder of the sea could improve now: the
/// happiness of a luxury kind the empire lacks (once per kind) at the price
/// of a point, and the yields the improvement adds to the deposit.
pub fn sea_values(obs: &AIObservation<'_>, brief: &TurnBrief, rules: &CompiledRuleset, prices: &Prices) -> Vec<Centi> {
    let view = &obs.view;
    let dims = view.dimensions();
    let mut tiles = Vec::new();
    for city in &obs.own_cities {
        tiles.extend(crate::math::hex_disc(dims, city.tile, crate::world::city::CITY_WORK_RADIUS));
    }
    tiles.sort();
    tiles.dedup();
    let mut kinds: Vec<ResourceId> = Vec::new();
    let mut out = Vec::new();
    for tile in tiles {
        let Some(snapshot) = view.tile(tile).snapshot().copied() else { continue };
        if snapshot.owner != Some(obs.player) || snapshot.city || snapshot.improvement.is_some() || !view.terrain_kinds().is_water(snapshot.terrain.0) {
            continue;
        }
        let Some(resource) = snapshot.resource else { continue };
        let Some(def) = rules.resources.get(resource.0 as usize) else { continue };
        let can = |i: &crate::ids::ImprovementId| buildable(rules, brief, *i) && site_ok(view, rules, brief, tile, *i);
        let improved = match def.improved_yields.iter().find(|(i, _)| can(i)) {
            Some((_, yields)) => *yields,
            None if def.improvements.iter().any(can) => Default::default(),
            None => continue,
        };
        let mut value = prices.food * i64::from(improved.food) + prices.production * i64::from(improved.production) + prices.gold * i64::from(improved.gold);
        if def.class == Some(ResourceClass::Luxury) && def.module.happiness > 0 && obs.economy.resources.total(resource) == 0 && !kinds.contains(&resource) {
            kinds.push(resource);
            value += prices.happiness * i64::from(def.module.happiness);
        }
        if value > 0 {
            out.push(value);
        }
    }
    out.sort_by_key(|v| std::cmp::Reverse(*v));
    out
}
