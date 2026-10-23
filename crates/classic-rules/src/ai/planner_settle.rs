//! Expansion (R-07 AI stage E): city sites on the known map and settler
//! decisions.
//!
//! A site is legal by the core's own rules (`enter_cost`, MIN_CITY_RANGE,
//! foreign borders) applied to what the player knows; its score adds the
//! yields of the tiles a city there would work (core yield table, feature
//! and resource data), fresh water, hills, coast and resources, and weighs
//! the distance to the player's cities (a spacing of 3–5 tiles is
//! preferred) and to
//! foreign cities. Sites held by another settler (and their surroundings)
//! are reserved. A settler walks to its site, founds there, and gives up a
//! site that became illegal; without any site it waits in a city and
//! expansion pauses (no settlers piling up).

use crate::ai::planner::{Decision, PlanTools, UnitView};
use crate::ai::planner_map::{explore, Blockers, KnownMap, SearchLimit, Visit};
use crate::ai::planner_units::{move_along, nearest_safe_tile, threatened};
use crate::ai::state::{AiState, Reservation, UnitTask};
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::derived::tile_yields::nature_yields;
use crate::world::city::CITY_WORK_RADIUS;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::view::{KnownTile, PlayerView, TileSnapshot};
use crate::rules::content::ResourceClass;
use crate::rules::CompiledRuleset;
use crate::world::territory::MIN_CITY_RANGE;

/// Sites farther than this many turns from the settler are not considered.
pub const SITE_MAX_TURNS: u32 = 12;
/// Score points one turn of the settler's walk costs. Sites score about 650
/// and differ by tens, so a price of 3 sent settlers seven tiles to the
/// best of them (`inspect standard continents 4 0`: the second city at
/// round 40 instead of 28).
pub const TRAVEL_COST: i32 = 30;
/// Tiles from the nearest own city a site is free of the distance penalty.
pub const FREE_DISTANCE: i32 = 4;
/// Score points a tile beyond `FREE_DISTANCE` costs.
pub const DISTANCE_PENALTY: i32 = 40;
/// A site below this score is not worth a city.
pub const SITE_MIN_SCORE: i32 = 20;

/// A scored city site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    pub tile: TileId,
    /// Site quality.
    pub score: i32,
    /// Score minus the travel cost from the search origin.
    pub value: i32,
}

/// What site scoring needs besides the view.
pub struct SiteContext<'v, 'a> {
    pub view: &'v PlayerView<'a>,
    /// How the settler moves (embarking once the player knows how).
    pub profile: crate::navigation::MoveProfile,
    pub rules: &'v CompiledRuleset,
    pub player: PlayerId,
    /// Own city tiles.
    pub own_cities: &'v [TileId],
    /// Remembered foreign city tiles.
    pub foreign_cities: &'v [TileId],
    /// Sites held by other settlers (sorted).
    pub reserved: &'v [TileId],
    /// Hostile military units in sight (barbarians included): no site near
    /// them (a settler sent there only walked back and forth).
    pub danger: &'v [TileId],
    /// `tile_score` of the land tiles seen so far in this context: the
    /// candidates of one search share most of the tiles of their areas.
    pub tile_memo: TileScoreMemo,
}

/// Scores of land tiles for one `SiteContext` (the view does not change
/// while a context lives). Indexed by tile, filled on demand.
#[derive(Default)]
pub struct TileScoreMemo(std::cell::RefCell<Vec<i32>>);

const NOT_SCORED: i32 = i32::MIN;

impl TileScoreMemo {
    fn get_or(&self, tile: TileId, tiles: usize, compute: impl FnOnce() -> i32) -> i32 {
        let slot = tile.0 as usize;
        {
            let mut memo = self.0.borrow_mut();
            if memo.len() < tiles {
                memo.resize(tiles, NOT_SCORED);
            }
            if memo[slot] != NOT_SCORED {
                return memo[slot];
            }
        }
        let value = compute();
        self.0.borrow_mut()[slot] = value;
        value
    }
}

/// Rounds a settler that met danger waits for an escort before it gives
/// its site up.
pub const ESCORT_WAIT_ROUNDS: u32 = 10;

/// No site this close to a hostile military unit in sight.
pub const DANGER_RANGE: i32 = 3;

impl SiteContext<'_, '_> {
    fn near_any(&self, tiles: &[TileId], tile: TileId, range: i32) -> bool {
        let dims = self.view.dimensions();
        tiles
            .iter()
            .any(|t| dims.distance(*t, tile).is_some_and(|d| d <= range))
    }

    /// Legal as far as the player knows (the core's `city_site_check`
    /// rules on known tiles), and not taken by another settler.
    pub fn legal(&self, tile: TileId) -> bool {
        let _span = prof::span("classic/site_legal");
        let known = self.view.tile(tile);
        let Some(snapshot) = known.snapshot() else {
            return false;
        };
        if self
            .view
            .terrain_kinds()
            .enter_cost(snapshot.terrain)
            .is_none()
            || snapshot.city
        {
            return false;
        }
        if snapshot.owner.is_some_and(|owner| owner != self.player) {
            return false;
        }
        let range = MIN_CITY_RANGE;
        !self.near_any(self.own_cities, tile, range)
            && !self.near_any(self.foreign_cities, tile, range)
            && !self.near_any(self.reserved, tile, range)
            && !self.near_any(self.danger, tile, DANGER_RANGE)
            && !self.city_known_near(tile, range)
    }

    /// A city remembered on the map near the tile (also one not in the
    /// lists: a city tile seen once).
    fn city_known_near(&self, tile: TileId, range: i32) -> bool {
        let dims = self.view.dimensions();
        crate::math::hex_disc(dims, tile, range).any(|t| match self.view.tile(t) {
            KnownTile::Unknown => false,
            known => known.snapshot().is_some_and(|s| s.city),
        })
    }

    /// Quality of a legal site; None when illegal.
    pub fn score(&self, tile: TileId) -> Option<i32> {
        let _span = prof::span("classic/site_score");
        if !self.legal(tile) {
            return None;
        }
        self.score_legal(tile)
    }

    /// `score` of a tile `legal` already accepted.
    fn score_legal(&self, tile: TileId) -> Option<i32> {
        let snapshot = *self.view.tile(tile).snapshot()?;
        let mut score = self.center_score(&snapshot);
        let dims = self.view.dimensions();
        for near in crate::math::hex_disc(dims, tile, CITY_WORK_RADIUS + 1) {
            if near == tile {
                continue;
            }
            let ring = dims.distance(tile, near).unwrap_or(0);
            let weight = if ring <= CITY_WORK_RADIUS { 3 } else { 1 };
            score += weight * self.tile_score(near, tile);
        }
        Some(score + self.distance_score(tile))
    }

    fn center_score(&self, snapshot: &TileSnapshot) -> i32 {
        let mut score = 0;
        if snapshot.river {
            score += 12;
        }
        if self.view.terrain_kinds().is_hills(snapshot.terrain.0) {
            score += 4;
        }
        score
    }

    /// Worth of one tile in the city's area (yields by the core's tables,
    /// a known resource, fresh water features).
    fn tile_score(&self, tile: TileId, center: TileId) -> i32 {
        let known = self.view.tile(tile);
        let Some(snapshot) = known.snapshot() else {
            return 1;
        };
        if snapshot.owner.is_some_and(|o| o != self.player) {
            return -2;
        }
        if self.view.terrain_kinds().is_water(snapshot.terrain.0) {
            let coastal = self.view.dimensions().adjacent(tile, center);
            return if coastal { 2 } else { 1 };
        }
        // A land tile is worth the same to every candidate site.
        let tiles = self.view.dimensions().tile_count();
        self.tile_memo
            .get_or(tile, tiles, || self.land_tile_score(&snapshot, tile))
    }

    /// Terrain, feature, lake, natural wonder and resource of a land tile.
    fn land_tile_score(&self, snapshot: &TileSnapshot, tile: TileId) -> i32 {
        // Terrain, feature, lake and natural wonder by the core's rules.
        let nature = nature_yields(self.view, self.rules, tile);
        let mut score = 3 * nature.food + 2 * nature.production;
        if let Some(feature) = snapshot
            .feature
            .and_then(|f| self.rules.features.get(f.0 as usize))
        {
            if feature.fresh_water {
                score += 4;
            }
        }
        if let Some(resource) = snapshot.resource {
            score += self.resource_score(resource);
        }
        score
    }

    fn resource_score(&self, resource: crate::ids::ResourceId) -> i32 {
        let Some(def) = self.rules.resources.get(resource.0 as usize) else {
            return 0;
        };
        let class = match def.class {
            Some(ResourceClass::Luxury) => 6,
            Some(ResourceClass::Strategic) => 4,
            _ => 2,
        };
        let y = def.yields;
        class + 3 * y.food + 2 * y.production + y.gold
    }

    /// Keeps 3–5 tiles to its own cities and away from foreign ones.
    fn distance_score(&self, tile: TileId) -> i32 {
        let dims = self.view.dimensions();
        let nearest = |tiles: &[TileId]| tiles.iter().filter_map(|t| dims.distance(*t, tile)).min();
        let mut score = 0;
        if let Some(d) = nearest(self.own_cities) {
            let free = crate::ai::genome::get_i32(crate::ai::genome::Knob::SettleFreeDistance);
            if d > free {
                score -= (d - free) * crate::ai::genome::get_i32(crate::ai::genome::Knob::SettleDistancePenalty);
            }
            if d <= MIN_CITY_RANGE + 1 {
                score -= 6;
            }
        }
        if nearest(self.foreign_cities).is_some_and(|d| d <= 5) {
            score -= 15;
        }
        score
    }
}

/// The best sites reachable from `origin` over the known map, best first
/// (value = score − 3 per turn of travel); at most `limit` candidates are
/// scored (deterministic budget).
pub fn best_sites(
    ctx: &SiteContext<'_, '_>,
    origin: TileId,
    moves_max: u16,
    tools: &mut PlanTools<'_>,
    limit: u32,
) -> Vec<Site> {
    search_sites(ctx, origin, moves_max, tools, limit, usize::MAX)
}

/// How many sites `best_sites` would return, at most `cap`: the search
/// stops as soon as `cap` sites worth a city are found (the answer
/// `min(count, cap)` does not depend on the sites after them).
pub fn count_sites(
    ctx: &SiteContext<'_, '_>,
    origin: TileId,
    moves_max: u16,
    tools: &mut PlanTools<'_>,
    limit: u32,
    cap: usize,
) -> usize {
    search_sites(ctx, origin, moves_max, tools, limit, cap).len().min(cap)
}

fn search_sites(
    ctx: &SiteContext<'_, '_>,
    origin: TileId,
    moves_max: u16,
    tools: &mut PlanTools<'_>,
    limit: u32,
    cap: usize,
) -> Vec<Site> {
    let mut search = SiteSearch {
        ctx,
        moves_max,
        max_points: SITE_MAX_TURNS * u32::from(moves_max.max(1)),
        limit,
        scored: 0,
        worth_a_city: 0,
        cap,
        sites: Vec::new(),
    };
    let nodes = SearchLimit {
        max_nodes: tools.settings.budget.path_nodes,
    };
    let mut visit = |tile: TileId, points: u32| search.visit(tile, points);
    let blockers = Blockers::default();
    let map = KnownMap {
        view: ctx.view,
        profile: ctx.profile,
    };
    explore(
        &map,
        origin,
        &blockers,
        tools.scratch,
        nodes,
        &mut tools.counters.search,
        &mut visit,
    );
    tools.counters.candidates += u64::from(search.scored);
    let mut sites = search.sites;
    sites.retain(|s| s.score >= SITE_MIN_SCORE);
    sites.sort_by_key(|s| (std::cmp::Reverse(s.value), s.tile));
    sites
}

/// The state of one site search: legal tiles in price order are scored
/// until `limit` of them were or the travel gets too long.
struct SiteSearch<'c, 'v, 'a> {
    ctx: &'c SiteContext<'v, 'a>,
    moves_max: u16,
    max_points: u32,
    limit: u32,
    scored: u32,
    /// Sites found with a score of at least `SITE_MIN_SCORE`, and how many
    /// the caller wants at most.
    worth_a_city: usize,
    cap: usize,
    sites: Vec<Site>,
}

impl SiteSearch<'_, '_, '_> {
    fn visit(&mut self, tile: TileId, points: u32) -> Visit {
        if points > self.max_points || self.scored >= self.limit || self.worth_a_city >= self.cap {
            return Visit::Stop;
        }
        if !self.ctx.legal(tile) {
            return Visit::Continue;
        }
        self.scored += 1;
        // `legal` just accepted the tile: score it without asking again.
        if let Some(score) = self.ctx.score_legal(tile) {
            if score >= SITE_MIN_SCORE {
                self.worth_a_city += 1;
            }
            let turns = crate::ai::planner_map::turns_for(points, self.moves_max) as i32;
            let value = score - crate::ai::genome::get_i32(crate::ai::genome::Knob::SettleTravelCost) * turns;
            self.sites.push(Site { tile, score, value });
        }
        Visit::Continue
    }
}

fn site_tiles(reservation: Reservation) -> Option<TileId> {
    match reservation {
        Reservation::Site(tile) => Some(tile),
        _ => None,
    }
}

/// One decision of a settler.
pub fn decide_settler(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
) -> Decision {
    let unit = view.local.unit;
    let actor = Actor::Unit(unit.id);
    let escort = escorted(view, state);
    if threatened(view, tools.rules, unit.tile, 2) && !escort {
        // Danger on the way: back to safety, and the next tries wait for an
        // escort (`planner_units::unescorted_settler`); after
        // TASK_FAILURE_LIMIT such retreats the site is given up. Without
        // this the settler stepped out and back every turn for the rest of
        // the game (full-game harness).
        state.note_failure(unit.id);
        return retreat(view, tools, actor);
    }
    let record = state.task_of(unit.id).copied();
    let met_danger = record.is_some_and(|t| t.failures > 0);
    if met_danger && !escort {
        let waited = record.map(|t| view.brief.round.saturating_sub(t.since_round)).unwrap_or(0);
        if waited < ESCORT_WAIT_ROUNDS {
            return Decision::refuse(actor, Reason::Threatened);
        }
        // No escort came: another site (away from what is in sight now).
        state.clear_task(unit.id);
    }
    // A very unhappy empire founds no city (the core refuses it): wait.
    let mood = crate::derived::happiness::mood(view.brief.turn_happiness);
    if mood == crate::derived::happiness::Mood::VeryUnhappy {
        return Decision::refuse(actor, Reason::Waiting);
    }
    if let Some(UnitTask::Settle { site }) = state.task_of(unit.id).map(|t| t.task) {
        return continue_to_site(view, state, tools, site);
    }
    if view.brief.cities.is_empty()
        && with_context(view, state, tools.rules, |ctx| ctx.legal(unit.tile))
    {
        return Decision::act(
            actor,
            Reason::FoundCity,
            Command::FoundCity {
                settler: unit.handle,
            },
        );
    }
    choose_site(view, state, tools)
}

/// A military unit stands with the settler: one on the move (not a
/// fortified garrison), or one whose task is to escort it.
fn escorted(view: &UnitView<'_, '_>, state: &AiState) -> bool {
    let unit = view.local.unit;
    let dims = view.local.view.dimensions();
    let escort_of = |id| matches!(state.task_of(id).map(|t| t.task), Some(UnitTask::Escort { ward }) if ward == unit.id);
    view.local
        .own_units
        .iter()
        .filter(|u| u.moves_max > 0)
        .filter(|u| crate::ai::planner::role_of(view.local.view.rules(), u.kind) == crate::ai::planner::AiRole::Military)
        // On the settler's tile, or its escort close enough to catch up.
        .filter(|u| u.tile == unit.tile || (escort_of(u.id) && dims.distance(u.tile, unit.tile).is_some_and(|d| d <= ESCORT_CATCH_UP)))
        .any(|u| !u.fortified || escort_of(u.id))
}

/// An escort this close to its settler walks with it.
const ESCORT_CATCH_UP: i32 = 2;

fn retreat(view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, actor: Actor) -> Decision {
    match nearest_safe_tile(view, tools) {
        Some(tile) if tile != view.local.unit.tile => {
            move_along(view, tools, tile, Reason::Retreat)
        }
        _ => Decision::refuse(actor, Reason::Threatened),
    }
}

fn continue_to_site(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
    site: TileId,
) -> Decision {
    let unit = view.local.unit;
    let legal = with_context(view, state, tools.rules, |ctx| ctx.legal(site));
    if !legal {
        state.clear_task(unit.id);
        return choose_site(view, state, tools);
    }
    if unit.tile == site {
        return Decision::act(
            Actor::Unit(unit.id),
            Reason::FoundCity,
            Command::FoundCity {
                settler: unit.handle,
            },
        );
    }
    move_along(view, tools, site, Reason::MoveToSite)
}

fn choose_site(
    view: &UnitView<'_, '_>,
    state: &mut AiState,
    tools: &mut PlanTools<'_>,
) -> Decision {
    let unit = view.local.unit;
    let limit = tools.settings.budget.candidates;
    let rules = tools.rules;
    let sites = with_context(view, state, rules, |ctx| {
        best_sites(ctx, unit.tile, unit.moves_max, tools, limit)
    });
    let Some(site) = sites.first() else {
        state.clear_task(unit.id);
        state.strategy.no_site_round = Some(view.brief.round);
        return Decision::refuse(Actor::Unit(unit.id), Reason::NoSettleSite);
    };
    state.set_task(
        unit.id,
        UnitTask::Settle { site: site.tile },
        view.brief.round,
    );
    tools.counters.tasks_assigned += 1;
    if site.tile == unit.tile {
        return Decision::act(
            Actor::Unit(unit.id),
            Reason::FoundCity,
            Command::FoundCity {
                settler: unit.handle,
            },
        );
    }
    move_along(view, tools, site.tile, Reason::MoveToSite)
}

/// Run `f` with the site context of this decision (own cities from the
/// brief, remembered foreign cities, other settlers' sites).
pub fn with_context<R>(
    view: &UnitView<'_, '_>,
    state: &AiState,
    rules: &CompiledRuleset,
    f: impl FnOnce(&SiteContext<'_, '_>) -> R,
) -> R {
    let own: Vec<TileId> = view.brief.cities.iter().map(|c| c.tile).collect();
    let foreign: Vec<TileId> = state.seen_cities().iter().map(|c| c.tile).collect();
    let reserved = state.reserved_tiles(site_tiles, Some(view.local.unit.id));
    let danger: Vec<TileId> = view
        .local
        .foreign_units
        .iter()
        .filter(|u| view.local.relation(u.owner).is_some_and(|r| r.at_war))
        .filter(|u| crate::ai::planner::role_of(rules, u.kind) == crate::ai::planner::AiRole::Military)
        .map(|u| u.tile)
        .collect();
    let ctx = SiteContext {
        view: &view.local.view,
        profile: KnownMap::of_unit(&view.local.view, view.local.unit.handle).profile,
        rules,
        player: view.local.player,
        own_cities: &own,
        foreign_cities: &foreign,
        reserved: &reserved,
        danger: &danger,
        tile_memo: Default::default(),
    };
    f(&ctx)
}
