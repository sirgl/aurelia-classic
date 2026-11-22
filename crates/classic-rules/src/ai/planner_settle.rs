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
//!
//! `Variant::SiteNet` keeps all of this but the quality of a site worth a
//! city (`SITE_MIN_SCORE` by the score above, the same set): the site
//! net's expected population 50 rounds after the founding, in score points
//! by a linear map fitted on the training games (`site_net::planner_score`),
//! held within one citizen of the land score (`bounded_net`), plus the same
//! distance terms, the danger the site is in (`SiteContext::rival_risk`) and
//! a dearer walk (`NET_TRAVEL_RISK`) (`SiteQuality`).

use crate::ai::planner::{Decision, PlanTools, UnitView};
use crate::ai::planner_map::{explore, Blockers, KnownMap, SearchLimit, Visit};
use crate::ai::site::SiteInputs;
use crate::ai::site_net::SiteNetPlay;
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
        distance_terms(nearest(self.own_cities), nearest(self.foreign_cities))
    }

    /// The quality of a legal site worth a city by the site net: its
    /// expected population 50 rounds after the founding
    /// (`site_net::predict`) in score points (`site_net::planner_score`),
    /// with the same distance terms as `score_legal`. `heuristic` is the
    /// site's `score_legal` (a feature of the net: `site::site_features`
    /// asks the same of a context without reservations and danger, legal
    /// wherever this one is).
    /// `read` says how (`NetRead`).
    fn net_score(&self, tile: TileId, inputs: &SiteInputs<'_>, read: &NetRead<'_>, heuristic: i32) -> i32 {
        #[cfg(feature = "experimental-ai")]
        {
            let _span = prof::span("classic/site_net");
            let mut features = crate::ai::site::site_features_with(self.view, self.rules, self.player, tile, inputs, Some(heuristic));
            if let NetRead::OwnLand = read {
                features[crate::ai::site::FOREIGN_TILES] = 0;
            }
            let distance = self.distance_score(tile);
            let net = crate::ai::site_net::planner_score(crate::ai::site_net::predict(&features));
            let score = bounded_net(net, heuristic - distance) + distance + self.rival_risk(tile);
            return match read {
                NetRead::Science(bonuses) => score + crate::ai::tall::site_bonus(self.view, self.rules, tile, bonuses),
                NetRead::Fitted | NetRead::OwnLand => score,
            };
        }
        // Without the site net's table (`experimental-ai`) a site is worth its heuristic.
        #[cfg(not(feature = "experimental-ai"))]
        {
            let _ = (tile, inputs, read);
            heuristic
        }
    }

    /// The danger a site on the site net's path is in, in score points (0 or below): the net
    /// does not read the distance to a rival city (its answer is flat from 3 to 10 tiles,
    /// `ai-econ-shadow --probe`) nor the armies in sight, and `distance_terms` only takes 15
    /// near a rival, so the net founded 35 % of its cities within 5 tiles of a rival (18 % for
    /// the land score, which also pays for each rival tile in the radius).
    #[cfg(feature = "experimental-ai")]
    fn rival_risk(&self, tile: TileId) -> i32 {
        let dims = self.view.dimensions();
        let nearest_rival = self.foreign_cities.iter().filter_map(|t| dims.distance(*t, tile)).min();
        let mut risk = 0;
        // A city this close to a rival is the one its next war takes first.
        if let Some(d) = nearest_rival.filter(|d| *d < RIVAL_SAFE_DISTANCE) {
            risk -= (RIVAL_SAFE_DISTANCE - d) * RIVAL_PENALTY;
        }
        // A hostile army in sight a few tiles out reaches the settler or the new city in a turn
        // or two (`legal` refuses only within `DANGER_RANGE`).
        let armies = self.danger.iter().filter(|t| dims.distance(**t, tile).is_some_and(|d| d <= ARMY_RANGE)).count();
        risk -= ARMY_PENALTY * armies as i32;
        risk
    }
}

/// Tiles from the nearest rival city a site on the net's path is free of `rival_risk`.
pub const RIVAL_SAFE_DISTANCE: i32 = 7;
/// Score points a tile closer than `RIVAL_SAFE_DISTANCE` costs: at 4 tiles 3 × 30 = 90, three
/// turns of travel, about what the land score takes for 15 rival tiles in the radius.
pub const RIVAL_PENALTY: i32 = 30;
/// Hostile military units in sight within this many tiles of a site weigh on it.
pub const ARMY_RANGE: i32 = 6;
/// Score points each such unit costs: two turns of travel.
pub const ARMY_PENALTY: i32 = 60;
/// Score points each turn of a settler's walk costs on the site net's path on top of
/// `TRAVEL_COST`: the net's sites lie farther out and 11.5 % of its settlers never founded a
/// city (5.1 % on the land score); a turn on the road is a turn an unescorted settler can be
/// caught.
pub const NET_TRAVEL_RISK: i32 = 10;

/// The site net's points held within one citizen at +50 rounds
/// (`site_weights::SCORE_PER_CITIZEN`) of the land score (`land`, the site's `score_legal`
/// without the distance terms): the map from citizens to points matches the land score's mean
/// and spread on the training games, so an answer farther from the land score than that is the
/// net reading outside its data, not a better site.
#[cfg(feature = "experimental-ai")]
fn bounded_net(net: i32, land: i32) -> i32 {
    let band = crate::ai::site_weights::SCORE_PER_CITIZEN;
    net.clamp(land - band, land + band)
}

/// The shadow tool's record (`ai::econ_shadow`): the site net's best site against the one the
/// standard quality (the science bonus on a tall seat, else the land score) ranks first.
fn shadow_site(ctx: &SiteContext<'_, '_>, view: &UnitView<'_, '_>, tools: &mut PlanTools<'_>, limit: u32, net: &[Site]) {
    let (unit, brief) = (view.local.unit, view.brief);
    let quality = if brief.tall_sites.is_empty() { SiteQuality::Heuristic } else { SiteQuality::Science(&brief.tall_sites) };
    let standard = best_sites_by(ctx, unit.tile, unit.moves_max, tools, limit, &quality);
    let (Some(net), Some(standard)) = (net.first(), standard.first()) else {
        return;
    };
    let dims = ctx.view.dimensions();
    let case = |site: &Site| {
        let nearest = |tiles: &[TileId]| tiles.iter().filter_map(|t| dims.distance(*t, site.tile)).min();
        crate::ai::econ_shadow::SiteCase {
            tile: site.tile,
            own_distance: nearest(ctx.own_cities),
            foreign_distance: nearest(ctx.foreign_cities),
            heuristic: ctx.score(site.tile).unwrap_or(0),
            value: site.value,
        }
    };
    crate::ai::econ_shadow::push(crate::ai::econ_shadow::Case::Site { player: ctx.player, round: brief.round, settler: unit.id.0, net: case(net), standard: case(standard) });
}

/// The score terms of a site's distances to the nearest own and foreign
/// city (None: no such city known): beyond the free distance a penalty a
/// tile, a small one next to the closest legal spacing, and one near a
/// foreign city.
pub fn distance_terms(own: Option<i32>, foreign: Option<i32>) -> i32 {
    let mut score = 0;
    if let Some(d) = own {
        let free = crate::ai::genome::get_i32(crate::ai::genome::Knob::SettleFreeDistance);
        if d > free {
            score -= (d - free) * crate::ai::genome::get_i32(crate::ai::genome::Knob::SettleDistancePenalty);
        }
        if d <= MIN_CITY_RANGE + 1 {
            score -= 6;
        }
    }
    if foreign.is_some_and(|d| d <= 5) {
        score -= 15;
    }
    score
}

/// What gives a legal site worth a city its quality in a search.
#[derive(Debug, Clone, Copy)]
pub enum SiteQuality<'c> {
    /// `SiteContext::score`.
    Heuristic,
    /// The site net on the site's features with these inputs, read so
    /// (`SiteContext::net_score`, `Variant::site_net`).
    Net(SiteInputs<'c>, NetRead<'c>),
    /// `SiteContext::score` plus what the science buildings whose site
    /// condition the site meets add (`ai::tall`, `TurnBrief::tall_sites`).
    Science(&'c [(crate::ids::BuildingKindId, i32)]),
}

/// How a search reads the site net (`site_net::SiteNetPlay` with the seat's
/// science bonuses).
#[derive(Debug, Clone, Copy)]
pub enum NetRead<'c> {
    /// The net's answer as fitted.
    Fitted,
    /// The answer plus what the science buildings whose site condition the
    /// site meets add (`ai::tall::site_bonus`, `TurnBrief::tall_sites`).
    Science(&'c [(crate::ids::BuildingKindId, i32)]),
    /// The answer with the input `foreign_tiles` read as 0.
    OwnLand,
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
    best_sites_by(ctx, origin, moves_max, tools, limit, &SiteQuality::Heuristic)
}

/// `best_sites` with the sites worth a city (`SITE_MIN_SCORE` by
/// `SiteContext::score`, the same set) valued by `quality`.
pub fn best_sites_by(
    ctx: &SiteContext<'_, '_>,
    origin: TileId,
    moves_max: u16,
    tools: &mut PlanTools<'_>,
    limit: u32,
    quality: &SiteQuality<'_>,
) -> Vec<Site> {
    search_sites(ctx, origin, moves_max, tools, limit, usize::MAX, quality)
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
    search_sites(ctx, origin, moves_max, tools, limit, cap, &SiteQuality::Heuristic).len().min(cap)
}

fn search_sites(
    ctx: &SiteContext<'_, '_>,
    origin: TileId,
    moves_max: u16,
    tools: &mut PlanTools<'_>,
    limit: u32,
    cap: usize,
    quality: &SiteQuality<'_>,
) -> Vec<Site> {
    let mut search = SiteSearch {
        ctx,
        quality,
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
    sites.sort_by_key(|s| (std::cmp::Reverse(s.value), s.tile));
    sites
}

/// The state of one site search: legal tiles in price order are scored
/// until `limit` of them were or the travel gets too long.
struct SiteSearch<'c, 'v, 'a> {
    ctx: &'c SiteContext<'v, 'a>,
    quality: &'c SiteQuality<'c>,
    moves_max: u16,
    max_points: u32,
    limit: u32,
    scored: u32,
    /// Sites found with a score of at least `SITE_MIN_SCORE`, and how many
    /// the caller wants at most.
    worth_a_city: usize,
    cap: usize,
    /// The sites worth a city.
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
        let Some(heuristic) = self.ctx.score_legal(tile) else {
            return Visit::Continue;
        };
        if heuristic < SITE_MIN_SCORE {
            return Visit::Continue;
        }
        self.worth_a_city += 1;
        let score = match self.quality {
            SiteQuality::Heuristic => heuristic,
            SiteQuality::Net(inputs, read) => self.ctx.net_score(tile, inputs, read, heuristic),
            SiteQuality::Science(bonuses) => heuristic + crate::ai::tall::site_bonus(self.ctx.view, self.ctx.rules, tile, bonuses),
        };
        let turns = crate::ai::planner_map::turns_for(points, self.moves_max) as i32;
        let travel_risk = match self.quality {
            SiteQuality::Net(..) => NET_TRAVEL_RISK,
            SiteQuality::Heuristic | SiteQuality::Science(_) => 0,
        };
        let value = score - (crate::ai::genome::get_i32(crate::ai::genome::Knob::SettleTravelCost) + travel_risk) * turns;
        self.sites.push(Site { tile, score, value });
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
    let brief = view.brief;
    let sites = with_context(view, state, rules, |ctx| {
        // The site net reads what the site dataset recorded at a founding
        // (`aurelia-qa` `site`): the brief's nation, technologies and
        // excess happiness, the decision's round and the context's cities.
        let inputs = SiteInputs {
            nation: brief.nation,
            techs: &brief.techs,
            own_cities: ctx.own_cities,
            foreign_cities: ctx.foreign_cities,
            round: view.local.round,
            excess_happiness: brief.happiness,
        };
        let tall = !brief.tall_sites.is_empty();
        let quality = match brief.site_net {
            SiteNetPlay::Off if tall => SiteQuality::Science(&brief.tall_sites),
            SiteNetPlay::Off => SiteQuality::Heuristic,
            SiteNetPlay::Tall if tall => SiteQuality::Net(inputs, NetRead::Science(&brief.tall_sites)),
            SiteNetPlay::Fitted | SiteNetPlay::Tall => SiteQuality::Net(inputs, NetRead::Fitted),
            SiteNetPlay::OwnLand => SiteQuality::Net(inputs, NetRead::OwnLand),
        };
        let sites = best_sites_by(ctx, unit.tile, unit.moves_max, tools, limit, &quality);
        if brief.site_net != SiteNetPlay::Off && crate::ai::econ_shadow::enabled() {
            shadow_site(ctx, view, tools, limit, &sites);
        }
        sites
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
