//! Work boats for the sea deposits inside the borders (classic/docs/ai-economy.md).
//!
//! A builder that works on water (a work boat, used up by its job) is built
//! for the deposits in the borders that only it can improve: a luxury the
//! empire lacks (the happiness of it at the price of a point) and the yields
//! a food or production deposit gains. The allocator orders the boat by the
//! value of the first deposit no boat is meant for (`sea_values`), the
//! workers send it to a water tile (`planner_workers`), and the land
//! workers' count leaves boats out. Everything is read from the ruleset's
//! declared builders, resources and improvements; no name appears. With
//! jobs by path (`EffectChecks::jobs`) a deposit counts only where a boat
//! sails by its kind's own movement rules (`sails_to`: a boat that may not
//! enter deep water before a technology is not built for a deposit there).

use crate::ai::planner::TurnBrief;
use crate::ai::planner_workers::{buildable, site_ok};
use crate::ai::value::{Centi, Prices};
use crate::ai::planner_map::{find_path, Blockers, KnownMap, MapScratch, PathOutcome, SearchStats};
use crate::ids::{ResourceId, TileId, UnitKindId};
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
    // With jobs by path a deposit counts only where a boat sails by its own
    // movement rules (`EffectChecks::jobs`).
    let mut sails = brief.effects.jobs.then(|| Sailing::new(obs, brief, rules));
    for tile in tiles {
        let Some(snapshot) = view.tile(tile).snapshot().copied() else { continue };
        if snapshot.owner != Some(obs.player) || snapshot.city || snapshot.improvement.is_some() || !view.terrain_kinds().is_water(snapshot.terrain.0) {
            continue;
        }
        let Some(resource) = snapshot.resource else { continue };
        let Some(def) = rules.resources.get(resource.0 as usize) else { continue };
        if sails.as_mut().is_some_and(|s| !s.reaches(tile)) {
            continue;
        }
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

/// Can a builder of the sea the player trains now sail to `tile` from one
/// of its nearest coastal cities, by its own movement rules?
pub fn sails_to(obs: &AIObservation<'_>, brief: &TurnBrief, rules: &CompiledRuleset, tile: TileId) -> bool {
    Sailing::new(obs, brief, rules).reaches(tile)
}

/// Coastal cities a boat may set out from, nearest first.
const SAILING_CITIES: usize = 3;

/// The ways of the builders of the sea the player could train now: a
/// deposit is theirs when a boat of one of their kinds finds a path to it
/// over the known map from one of the nearest own coastal cities, by the
/// kind's own movement rules (a boat that may not enter deep water before a
/// technology does not reach a deposit there).
struct Sailing<'o, 'a> {
    obs: &'o AIObservation<'a>,
    kinds: Vec<UnitKindId>,
    ports: Vec<TileId>,
    scratch: MapScratch,
    stats: SearchStats,
}

impl<'o, 'a> Sailing<'o, 'a> {
    fn new(obs: &'o AIObservation<'a>, brief: &TurnBrief, rules: &CompiledRuleset) -> Self {
        let kinds = (0..rules.units.len())
            .map(|i| UnitKindId(i as u16))
            .filter(|k| {
                let def = &rules.units[k.0 as usize];
                is_sea_builder(rules, *k) && def.required_tech.is_none_or(|t| brief.knows(t)) && def.obsolete_tech.is_none_or(|t| !brief.knows(t))
            })
            .collect();
        let ports = obs.own_cities.iter().map(|c| c.tile).filter(|t| crate::ai::fleet::coastal(&obs.view, *t)).collect();
        Self { obs, kinds, ports, scratch: MapScratch::default(), stats: SearchStats::default() }
    }

    fn reaches(&mut self, tile: TileId) -> bool {
        let dims = self.obs.view.dimensions();
        let mut ports = self.ports.clone();
        ports.sort_by_key(|p| (dims.distance(*p, tile).unwrap_or(i32::MAX), *p));
        let limit = crate::ai::planner::DecisionBudget::default().search();
        for &kind in &self.kinds {
            let map = KnownMap::of_kind(&self.obs.view, kind);
            for &port in ports.iter().take(SAILING_CITIES) {
                let outcome = find_path(&map, port, tile, &Blockers::default(), &mut self.scratch, limit, &mut self.stats);
                if matches!(outcome, PathOutcome::Found { .. }) {
                    return true;
                }
            }
        }
        false
    }
}
