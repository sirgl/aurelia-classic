//! Where the scouts go (classic/docs/ai-scouting.md): the targets of all scouts of
//! an empire are one allocation, not one greedy choice per unit.
//!
//! A scout that needs a target walks the known map once in price order
//! (`planner_map::explore`, the node budget of every search) and collects
//! at most `MAX_CANDIDATES` targets: frontier tiles (known, next to unknown)
//! and the Ancient Ruins it knows. A candidate is scored by what it
//! reveals, the turns to reach it, the leader's pull (`capital_search`),
//! and minus the crowding by the other scouts: how close it lies to their
//! targets and to where they stand. The best candidate that is safe wins.
//! The cost is that of one bounded search plus a sort of the candidates:
//! it does not depend on the map size, on the number of scouts (at most
//! `MAX_RIVALS` are looked at) or on the number of explored tiles.
//!
//! The other half is how many scouts the empire keeps (`wanted`,
//! `wants_recon`): the share of the map that is still unknown decides.

use crate::ai::planner::{role_of, AiRole, PlanTools, UnitView};
use crate::ai::planner_map::{explore, is_frontier, turns_for, unknown_around, KnownMap, Visit};
use crate::ai::planner_units::blockers;
use crate::ai::state::{AiState, UnitTask};
use crate::derived::visibility::SIGHT_RADIUS;
use crate::ids::{ImprovementId, TileId};
use crate::math::{hex_disc, MapDimensions};
use crate::rules::content::ImprovementKind;
use crate::rules::CompiledRuleset;

/// Score of an unknown tile a frontier tile would reveal.
pub const REVEAL_SCORE: i32 = 10;
/// Score lost per turn on the way.
pub const TURN_COST: i32 = 4;
/// What known ruins are worth, in the score of a frontier tile: about twelve
/// unknown tiles (the reward is a citizen, culture, faith, gold or a map).
pub const RUIN_SCORE: i32 = 120;
/// A candidate this close to the target of another scout loses
/// `TARGET_CROWDING` per tile of the missing distance to `TARGET_ZONE`;
/// the same for the tile where another scout stands.
pub const TARGET_ZONE: i32 = 8;
pub const TARGET_CROWDING: i32 = 12;
pub const SCOUT_ZONE: i32 = 5;
pub const SCOUT_CROWDING: i32 = 10;
/// A candidate this close to a hostile unit or a barbarian camp is no target.
pub const DANGER_RADIUS: i32 = 2;
/// A scout that sees a hostile military unit this close aims only at tiles
/// farther from it than it stands.
pub const ALARM_RADIUS: i32 = 3;
/// Candidates one decision looks at (the decision budget caps it lower).
pub const MAX_CANDIDATES: usize = 64;
/// Other scouts one decision takes into account.
pub const MAX_RIVALS: usize = 6;

/// The share of the map explored (percent) from which the empire wants no
/// more scouts: what is left lies over the sea, or in the fog of others.
/// It stays high: the first third of a small map is the rim, and the rivals
/// live in the middle.
pub const EXPLORED_ENOUGH_PERCENT: u32 = 90;
/// The second scout is wanted from this round while less is explored (a round of a Standard game: another speed reads its round as the same share of
/// its game, `SpeedValues::standard_round`).
pub const SECOND_SCOUT_ROUND: u32 = 12;
pub const SECOND_SCOUT_UNTIL_ROUND: u32 = 100;
pub const SECOND_SCOUT_BELOW_PERCENT: u32 = 40;
/// The first recon unit is built from this round (before it the starting
/// warrior explores and the capital builds workers and settlers: a scout
/// at round 12 cost the empires a tenth of their first workers and a third
/// of their luxury kinds at round 60, `aurelia-qa` `happiness_pace`). Leaders
/// looking for a capital build one at once (`planner_cities`). A round of a Standard
/// game: another speed reads its round as the same share of its game
/// (`SpeedValues::standard_round`).
pub const RECON_FROM_ROUND: u32 = 30;
/// Recon units the empire builds for scouting.
pub const RECON_UNITS: usize = 1;

/// Scouts the empire wants at `round` with `known_percent` of the map
/// explored; the warriors count until a recon unit is built.
pub fn wanted(round: u32, known_percent: u32) -> usize {
    if known_percent >= EXPLORED_ENOUGH_PERCENT {
        0
    } else if (SECOND_SCOUT_ROUND..SECOND_SCOUT_UNTIL_ROUND).contains(&round)
        && known_percent < SECOND_SCOUT_BELOW_PERCENT
    {
        2
    } else {
        1
    }
}

/// Another recon unit is built: fewer than wanted live or are queued (the
/// second scout is a soldier: a recon unit that has seen all it can only
/// garrisons).
pub fn wants_recon(round: u32, known_percent: u32, recon_on_hand: usize) -> bool {
    round >= RECON_FROM_ROUND
        && round <= crate::ai::planner_units::SCOUTING_ENDS_ROUND
        && recon_on_hand < wanted(round, known_percent).min(RECON_UNITS)
}

/// `scout-early` (`Variant::scout_early`): the share of the ruins near home a recon
/// unit claims that the starting warrior would not (measured, classic/docs/ai-scouting.md
/// «The opening»: the books, whose scout comes at round 3, claim 1.11 ruins by round 30 of
/// a Quick game where `standard`'s warrior alone claims 0.76, about a third more).
pub const SCOUT_SHARE_PERCENT: i64 = 33;
/// `scout-early`: the Standard rounds the ruins near home stay unclaimed (the race
/// with the neighbours' units); the scout's reach is its moves over them.
pub const RUINS_RACE_ROUNDS: u32 = 10;

/// `scout-early`: a recon unit is worth building now. None lives or is queued, no war
/// is on, the scouting season lasts, and the ruins it can expect to claim are worth
/// more than its hammers at the turn's prices: the unknown land within its reach
/// (`RUINS_RACE_ROUNDS` of its moves around the first city, its land share read from
/// the known tiles there) over the generator's land per ruin
/// (`scenario::ruins::LAND_TILES_PER_RUINS`) plus the ruins known there, times
/// `SCOUT_SHARE_PERCENT`, times the mean reward (`systems::ruins::REWARDS`, each as
/// likely: gold, culture and faith of the speed at their prices, a citizen at the
/// price of a granted one; experience and the map at none). Without the turn's
/// prices: `wants_recon`.
pub fn recon_worth(obs: &crate::knowledge::observation::AIObservation<'_>, brief: &crate::ai::planner::TurnBrief, rules: &CompiledRuleset, recon_on_hand: usize) -> bool {
    let round = brief.speed.standard_round(brief.round);
    let Some(prices) = brief.prices.as_ref() else {
        return wants_recon(round, brief.known_percent, recon_on_hand);
    };
    if recon_on_hand >= RECON_UNITS || round > crate::ai::planner_units::SCOUTING_ENDS_ROUND || brief.relations.iter().any(|r| r.at_war) {
        return false;
    }
    let Some(home) = obs.own_cities.iter().min_by_key(|c| c.id) else {
        return false;
    };
    // The cheapest recon unit the city may build, and its cost there.
    let Some((kind, cost)) = (0..rules.units.len())
        .map(|i| crate::ids::UnitKindId(i as u16))
        .filter(|k| rules.units[k.0 as usize].role == Some(crate::rules::content::UnitRole::Recon))
        .filter_map(|k| {
            let item = crate::ids::ProductionItem::Unit(k);
            let order = crate::Command::EnqueueProduction { player: obs.player, city: home.handle, item };
            crate::knowledge::public::PlayerQueries::accepts(&obs.view, &order).then(|| obs.view.own_production_cost(home.handle, item)).flatten().map(|c| (k, c))
        })
        .min_by_key(|&(k, c)| (c, k.0))
    else {
        return false;
    };
    let view = &obs.view;
    let dims = view.dimensions();
    let kinds = view.terrain_kinds();
    let reach = i32::from(rules.units[kind.0 as usize].movement.max(1)) * obs.speed.pace_rounds(RUINS_RACE_ROUNDS) as i32;
    let ruins = ruins_improvement(rules);
    let (mut known, mut known_land, mut unknown, mut known_ruins) = (0i64, 0i64, 0i64, 0i64);
    for tile in hex_disc(dims, home.tile, reach) {
        if !view.is_known(tile) {
            unknown += 1;
            continue;
        }
        known += 1;
        let known_tile = view.tile(tile);
        let snapshot = known_tile.snapshot();
        known_land += i64::from(snapshot.is_some_and(|s| !kinds.is_water(s.terrain.0)));
        known_ruins += i64::from(ruins.is_some() && snapshot.is_some_and(|s| s.improvement == ruins));
    }
    let unknown_land = unknown * known_land / known.max(1);
    // Hundredths of a ruin.
    let expected = (unknown_land * 100 / crate::scenario::ruins::LAND_TILES_PER_RUINS as i64 + known_ruins * 100) * SCOUT_SHARE_PERCENT / 100;
    let left = i64::from(prices.rounds_left.max(1));
    let (gold, culture, faith) = crate::systems::ruins::amounts(&obs.speed);
    let rewards = crate::systems::ruins::REWARDS.iter().map(|r| match r {
        crate::systems::ruins::Reward::Citizen => prices.citizen(),
        crate::systems::ruins::Reward::Culture => i64::from(culture) * prices.culture / left,
        crate::systems::ruins::Reward::Faith => i64::from(faith) * prices.faith / left,
        crate::systems::ruins::Reward::Gold => i64::from(gold) * prices.gold / left,
        crate::systems::ruins::Reward::Experience | crate::systems::ruins::Reward::Map => 0,
    });
    let reward = rewards.sum::<i64>() / crate::systems::ruins::REWARDS.len() as i64;
    let worth = expected * reward / 100;
    worth > i64::from(cost) * prices.production / left
}

/// The improvement of Ancient Ruins, when the rules have it.
pub fn ruins_improvement(rules: &CompiledRuleset) -> Option<ImprovementId> {
    rules
        .improvements
        .iter()
        .position(|d| d.kind == ImprovementKind::AncientRuins)
        .map(|i| ImprovementId(i as u16))
}

fn ruins_known_at(view: &UnitView<'_, '_>, ruins: Option<ImprovementId>, tile: TileId) -> bool {
    ruins.is_some()
        && view
            .local
            .view
            .tile(tile)
            .snapshot()
            .is_some_and(|s| s.improvement == ruins)
}

/// The target still deserves a visit: ruins stand on it, or it is next to
/// unknown ground, and nobody stands on it.
pub fn target_valid(view: &UnitView<'_, '_>, rules: &CompiledRuleset, target: TileId) -> bool {
    let known = &view.local.view;
    let worth =
        is_frontier(known, target) || ruins_known_at(view, ruins_improvement(rules), target);
    worth
        && view.local.unit.tile != target
        && !occupied(view, target)
        && !toward_hostile(view, rules, target)
}

/// A hostile military unit is within `ALARM_RADIUS` of the scout and the
/// tile is no farther from it than the scout is.
fn toward_hostile(view: &UnitView<'_, '_>, rules: &CompiledRuleset, tile: TileId) -> bool {
    let dims = view.local.view.dimensions();
    let unit = view.local.unit;
    view.local.foreign_units.iter().any(|u| {
        let at_war = view.local.relation(u.owner).is_some_and(|r| r.at_war);
        let here = dims.distance(u.tile, unit.tile).unwrap_or(i32::MAX);
        let there = dims.distance(u.tile, tile).unwrap_or(i32::MAX);
        at_war
            && here <= ALARM_RADIUS
            && there <= here
            && role_of(rules, u.kind) == AiRole::Military
    })
}

/// A foreign unit stands on the tile (a scout cannot stop there).
fn occupied(view: &UnitView<'_, '_>, tile: TileId) -> bool {
    view.brief.sightings.iter().any(|s| s.tile == tile)
}

#[derive(Clone, Copy)]
struct Rival {
    at: TileId,
    target: TileId,
}

/// The other scouts of the empire: where each stands and where it goes.
struct Rivals {
    list: [Rival; MAX_RIVALS],
    len: usize,
}

impl Rivals {
    fn of(view: &UnitView<'_, '_>, state: &AiState) -> Self {
        let me = view.local.unit.id;
        let blank = Rival {
            at: TileId(0),
            target: TileId(0),
        };
        let mut rivals = Self {
            list: [blank; MAX_RIVALS],
            len: 0,
        };
        for record in state.tasks() {
            let UnitTask::Scout { target } = record.task else {
                continue;
            };
            if record.unit == me || rivals.len == MAX_RIVALS {
                continue;
            }
            rivals.list[rivals.len] = Rival {
                at: position_of(view, record.unit, target),
                target,
            };
            rivals.len += 1;
        }
        rivals
    }

    fn all(&self) -> &[Rival] {
        &self.list[..self.len]
    }

    /// Another scout already goes to the tile.
    fn holds(&self, tile: TileId) -> bool {
        self.all().iter().any(|r| r.target == tile)
    }

    /// Score lost on `tile` for lying near where the others go and stand.
    fn crowding(&self, dims: MapDimensions, tile: TileId) -> i32 {
        let zone = |other: TileId, radius: i32, weight: i32| {
            let d = dims.distance(other, tile).unwrap_or(radius);
            (radius - d).max(0) * weight
        };
        self.all()
            .iter()
            .map(|r| {
                zone(r.target, TARGET_ZONE, TARGET_CROWDING)
                    + zone(r.at, SCOUT_ZONE, SCOUT_CROWDING)
            })
            .sum()
    }
}

/// Where an own unit stands: fresh when it is in the local observation,
/// else as at the start of the turn, else `fallback`.
fn position_of(view: &UnitView<'_, '_>, unit: crate::ids::EntityId, fallback: TileId) -> TileId {
    let near = view
        .local
        .own_units
        .iter()
        .find(|u| u.id == unit)
        .map(|u| u.tile);
    near.or_else(|| {
        view.brief
            .units
            .iter()
            .find(|u| u.id == unit)
            .map(|u| u.tile)
    })
    .unwrap_or(fallback)
}

#[derive(Clone, Copy)]
struct Candidate {
    score: i32,
    tile: TileId,
}

/// Candidates of one decision, best first after `sort`; fixed size.
struct Candidates {
    list: [Candidate; MAX_CANDIDATES],
    len: usize,
}

impl Candidates {
    fn new() -> Self {
        Self {
            list: [Candidate {
                score: 0,
                tile: TileId(0),
            }; MAX_CANDIDATES],
            len: 0,
        }
    }

    fn push(&mut self, candidate: Candidate) {
        self.list[self.len] = candidate;
        self.len += 1;
    }

    /// Best score first, the lower tile on a tie.
    fn ranked(&mut self) -> &[Candidate] {
        let list = &mut self.list[..self.len];
        list.sort_unstable_by(|a, b| b.score.cmp(&a.score).then(a.tile.cmp(&b.tile)));
        list
    }
}

/// The state of one search: frontier tiles and ruins in price order.
struct Search<'v, 'o, 'a> {
    view: &'v UnitView<'o, 'a>,
    rivals: Rivals,
    ruins: Option<ImprovementId>,
    limit: usize,
    candidates: Candidates,
}

impl Search<'_, '_, '_> {
    fn visit(&mut self, tile: TileId, points: u32) -> Visit {
        if self.candidates.len >= self.limit {
            return Visit::Stop;
        }
        let unit = self.view.local.unit;
        let known = &self.view.local.view;
        let ruins = ruins_known_at(self.view, self.ruins, tile);
        if tile == unit.tile || !(ruins || is_frontier(known, tile)) || self.rivals.holds(tile) {
            return Visit::Continue;
        }
        let turns = turns_for(points, unit.moves_max) as i32;
        let worth = if ruins {
            RUIN_SCORE
        } else {
            let pull = match &self.view.brief.search {
                None => 0,
                Some(search) => match search.pull(known.dimensions(), tile) {
                    Some(pull) => pull,
                    None => return Visit::Continue,
                },
            };
            unknown_around(known, tile, SIGHT_RADIUS) as i32 * REVEAL_SCORE + pull
        };
        let crowding = self.rivals.crowding(known.dimensions(), tile);
        self.candidates.push(Candidate {
            score: worth - TURN_COST * turns - crowding,
            tile,
        });
        Visit::Continue
    }
}

/// A known barbarian camp or a hostile military unit lies near the tile.
fn dangerous(view: &UnitView<'_, '_>, tile: TileId) -> bool {
    let known = &view.local.view;
    let dims = known.dimensions();
    let hostile = view.brief.sightings.iter().any(|s| {
        let at_war = view.local.relation(s.owner).is_some_and(|r| r.at_war);
        s.military
            && at_war
            && dims
                .distance(s.tile, tile)
                .is_some_and(|d| d <= DANGER_RADIUS)
    });
    let camp = crate::systems::barbarians::camp_improvement(known.rules());
    hostile
        || camp.is_some_and(|camp| {
            hex_disc(dims, tile, DANGER_RADIUS).any(|t| {
                known
                    .tile(t)
                    .snapshot()
                    .is_some_and(|s| s.improvement == Some(camp))
            })
        })
}

/// The target of the unit: the best safe candidate of the allocation.
pub fn scout_target(
    view: &UnitView<'_, '_>,
    state: &AiState,
    tools: &mut PlanTools<'_>,
) -> Option<TileId> {
    let unit = view.local.unit;
    let mut search = Search {
        view,
        rivals: Rivals::of(view, state),
        ruins: ruins_improvement(tools.rules),
        limit: (tools.settings.budget.candidates as usize).min(MAX_CANDIDATES),
        candidates: Candidates::new(),
    };
    let blockers = blockers(view);
    let limit = tools.settings.budget.search();
    explore(
        &KnownMap::of_unit(&view.local.view, unit.handle),
        unit.tile,
        &blockers,
        tools.scratch,
        limit,
        &mut tools.counters.search,
        &mut |tile, points| search.visit(tile, points),
    );
    tools.counters.candidates += search.candidates.len as u64;
    search
        .candidates
        .ranked()
        .iter()
        .find(|c| !dangerous(view, c.tile) && !toward_hostile(view, tools.rules, c.tile))
        .map(|c| c.tile)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rivals(list: &[Rival]) -> Rivals {
        let mut out = Rivals {
            list: [Rival {
                at: TileId(0),
                target: TileId(0),
            }; MAX_RIVALS],
            len: list.len(),
        };
        out.list[..list.len()].copy_from_slice(list);
        out
    }

    /// Crowding is the sum of the target zone and the scout zone: it falls
    /// with the distance and is gone beyond the zones.
    #[test]
    fn crowding_falls_with_the_distance_to_the_others() {
        let dims = MapDimensions {
            width: 40,
            height: 30,
        };
        let tile = |q: u32, r: u32| TileId(r * dims.width + q);
        let others = rivals(&[Rival {
            at: tile(5, 5),
            target: tile(20, 15),
        }]);
        let at = |q: u32| others.crowding(dims, tile(q, 15));
        assert_eq!(at(20), TARGET_ZONE * TARGET_CROWDING);
        assert!(at(20) > at(22) && at(22) > at(26));
        assert_eq!(at(29), 0);
        assert_eq!(rivals(&[]).crowding(dims, tile(20, 15)), 0);
    }

    #[test]
    fn a_scout_holds_only_the_target_it_goes_to() {
        let others = rivals(&[Rival {
            at: TileId(1),
            target: TileId(9),
        }]);
        assert!(others.holds(TileId(9)));
        assert!(!others.holds(TileId(1)));
    }

    #[test]
    fn the_best_candidate_comes_first_and_a_tie_goes_to_the_lower_tile() {
        let mut candidates = Candidates::new();
        for (score, tile) in [(5, 7), (9, 4), (9, 2), (1, 1)] {
            candidates.push(Candidate {
                score,
                tile: TileId(tile),
            });
        }
        let order: Vec<u32> = candidates.ranked().iter().map(|c| c.tile.0).collect();
        assert_eq!(order, vec![2, 4, 7, 1]);
    }
}
