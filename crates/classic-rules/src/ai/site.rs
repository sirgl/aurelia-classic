//! Features of a city site for a learned site-value model: what the player
//! knows of a site at the moment it chooses it, all of it from public data
//! (the player's view, the rules' tables, the observation's own numbers).
//! The dataset tool (`aurelia-qa` `site`) pairs them with how the city
//! founded there developed; the settling planner of `Variant::SiteNet`
//! reads them through `ai::site_net` (`planner_settle`).
//!
//! The features of the land (all but the distances, the moment and the
//! heuristic) read only the known tiles within `CITY_WORK_RADIUS + 1` of
//! the site and cost about 100 µs; `land_features_memo` remembers them by
//! a fingerprint of exactly those tiles, so a site asked again is answered
//! the same without the work (a settler chooses again every turn, mostly
//! among the same sites).
//!
//! Features, in `SITE_NAMES` order (one `i32` each; "territory" is
//! `territory::territory` of the site within `CITY_WORK_RADIUS`: known
//! tiles that are the player's or nobody's; channels are the six of
//! `Yields`: food, production, gold, science, culture and the rules
//! module's yield, faith):
//!
//! | features | what |
//! |---|---|
//! | `nature_r{0..3}_{channel}` | sums of `nature_yields` (terrain, feature, lake, natural wonder; each tile at least 0, as the engine clamps a tile) of the territory tiles at hex distance 0 (the centre), 1, 2, 3 |
//! | `potential_r{0..3}_{channel}` | the same with the revealed resource and the best improvement the rules allow on each tile (`world::builds::site` with every technology, `plot_yields_with`; best by `improvements::yield_value`, or the tile as it is); the centre: nature and resource, at least the city minimum |
//! | `bonus_tiles_{channel}` | flat Tile-scope yields of the buildings the player could ever build, over the territory developed as above (`territory::building_tile_bonus`, summed over the buildings) |
//! | `bonus_city_{channel}` | flat City-scope yields of the same buildings with a condition (an unconditional one is the same for every site of a nation) that the site meets |
//! | `resources_{bonus,strategic,luxury,artifact}` | revealed resources in the territory by `ResourceClass` |
//! | `terrain_{00..}` | territory tiles of each terrain kind, by the rules' terrain table index (`TERRAIN_SLOTS`; a later index counts in the last slot) |
//! | `feature_{00..}` | territory tiles with each feature, by the feature table index (`FEATURE_SLOTS`, the same overflow) |
//! | `centre_river`, `centre_coastal`, `centre_hills`, `centre_fresh_water` | 0/1 of the centre tile (coastal: next to a sea, the engine's `CityCoastal`) |
//! | `territory_tiles`, `unknown_tiles`, `foreign_tiles` | tiles within the radius: in the territory, unknown, owned by another player |
//! | `river_tiles`, `lake_tiles`, `natural_wonders` | territory tiles on a river, of a lake, with a natural wonder |
//! | `own_city_distance`, `foreign_city_distance` | hex distance to the nearest own city and remembered foreign city (`FAR` when none) |
//! | `own_cities`, `round`, `excess_happiness` | the empire at the moment of the choice |
//! | `heuristic_score`, `heuristic_legal` | `planner_settle::SiteContext::score` of the site (0 when it is illegal) and 1 when it is legal |
//!
//! Buildings the player "could ever build": not unbuildable, not a palace
//! or a wonder (one per world or empire, not a city's), not another
//! nation's unique, and not a regular building the player's nation replaces
//! with its unique. The developed site assumes every technology, every one
//! of those buildings and an unbounded population, so a bonus behind a
//! technology, a building or a size counts.

use crate::ai::improvements::yield_value;
use crate::ai::planner_map::KnownMap;
use crate::ai::planner_settle::SiteContext;
use crate::ai::territory::{building_tile_bonus, territory, TerritoryContext};
use crate::classic::yields::ClassicYields;
use crate::classic::Classic;
use crate::derived::tile_yields::{fresh_water, nature_yields, plot_yields, plot_yields_with, TechSet};
use crate::derived::yields::YieldTerms;
use crate::ids::{BuildingKindId, FeatureId, ImprovementId, PlayerId, ResourceId, TechId, TerrainId, TileId};
use crate::knowledge::view::PlayerView;
use crate::math::{hex_disc, MapDimensions};
use crate::rules::content::{BuildingKind, ImprovementKind, ResourceClass, Yields};
use crate::rules::effects::{CompiledEffect, Condition, Modifier};
use crate::rules::CompiledRuleset;
use crate::world::builds::{site, Builder};
use crate::world::city::CITY_WORK_RADIUS;
use crate::world::facts::TileFacts;

/// Rings of the site: the centre and every distance up to the work radius.
pub const RINGS: usize = CITY_WORK_RADIUS as usize + 1;
/// Yield channels of a feature group (`Yields`' six).
pub const CHANNELS: usize = 6;
/// Slots of the terrain and feature counts (the tables' indexes; a later
/// index counts in the last slot).
pub const TERRAIN_SLOTS: usize = 12;
pub const FEATURE_SLOTS: usize = 12;
/// The distance feature when no such city is known.
pub const FAR: i32 = 99;

const NATURE: usize = 0;
const POTENTIAL: usize = NATURE + RINGS * CHANNELS;
const BONUS_TILES: usize = POTENTIAL + RINGS * CHANNELS;
const BONUS_CITY: usize = BONUS_TILES + CHANNELS;
const RESOURCES: usize = BONUS_CITY + CHANNELS;
const RESOURCE_CLASSES: usize = 4;
const TERRAINS: usize = RESOURCES + RESOURCE_CLASSES;
const FEATURES: usize = TERRAINS + TERRAIN_SLOTS;
const CENTRE: usize = FEATURES + FEATURE_SLOTS;
const TILES: usize = CENTRE + 4;
const EMPIRE: usize = TILES + 6;
const HEURISTIC: usize = EMPIRE + 5;

/// Number of site features.
pub const SITE_FEATURES: usize = HEURISTIC + 2;

/// The features' names, in order (the module docs say what each is).
pub const SITE_NAMES: [&str; SITE_FEATURES] = [
    "nature_r0_food", "nature_r0_production", "nature_r0_gold", "nature_r0_science", "nature_r0_culture", "nature_r0_faith",
    "nature_r1_food", "nature_r1_production", "nature_r1_gold", "nature_r1_science", "nature_r1_culture", "nature_r1_faith",
    "nature_r2_food", "nature_r2_production", "nature_r2_gold", "nature_r2_science", "nature_r2_culture", "nature_r2_faith",
    "nature_r3_food", "nature_r3_production", "nature_r3_gold", "nature_r3_science", "nature_r3_culture", "nature_r3_faith",
    "potential_r0_food", "potential_r0_production", "potential_r0_gold", "potential_r0_science", "potential_r0_culture", "potential_r0_faith",
    "potential_r1_food", "potential_r1_production", "potential_r1_gold", "potential_r1_science", "potential_r1_culture", "potential_r1_faith",
    "potential_r2_food", "potential_r2_production", "potential_r2_gold", "potential_r2_science", "potential_r2_culture", "potential_r2_faith",
    "potential_r3_food", "potential_r3_production", "potential_r3_gold", "potential_r3_science", "potential_r3_culture", "potential_r3_faith",
    "bonus_tiles_food", "bonus_tiles_production", "bonus_tiles_gold", "bonus_tiles_science", "bonus_tiles_culture", "bonus_tiles_faith",
    "bonus_city_food", "bonus_city_production", "bonus_city_gold", "bonus_city_science", "bonus_city_culture", "bonus_city_faith",
    "resources_bonus", "resources_strategic", "resources_luxury", "resources_artifact",
    "terrain_00", "terrain_01", "terrain_02", "terrain_03", "terrain_04", "terrain_05",
    "terrain_06", "terrain_07", "terrain_08", "terrain_09", "terrain_10", "terrain_11",
    "feature_00", "feature_01", "feature_02", "feature_03", "feature_04", "feature_05",
    "feature_06", "feature_07", "feature_08", "feature_09", "feature_10", "feature_11",
    "centre_river", "centre_coastal", "centre_hills", "centre_fresh_water",
    "territory_tiles", "unknown_tiles", "foreign_tiles", "river_tiles", "lake_tiles", "natural_wonders",
    "own_city_distance", "foreign_city_distance", "own_cities", "round", "excess_happiness",
    "heuristic_score", "heuristic_legal",
];

/// What the features need besides the view: the player's own numbers at
/// the moment of the choice (the turn's brief and the AI's memory).
#[derive(Debug, Clone, Copy)]
pub struct SiteInputs<'c> {
    /// The player's nation (its unique buildings and improvements).
    pub nation: u8,
    /// The player's technologies.
    pub techs: &'c [TechId],
    /// Own city tiles.
    pub own_cities: &'c [TileId],
    /// Remembered foreign city tiles.
    pub foreign_cities: &'c [TileId],
    pub round: u32,
    /// Excess happiness of the empire.
    pub excess_happiness: i32,
}

/// The six channels of `yields`, in feature order.
fn channels(yields: Yields) -> [i32; CHANNELS] {
    [yields.food, yields.production, yields.gold, yields.science, yields.culture, yields.module]
}

fn add_channels(out: &mut [i32], at: usize, yields: Yields) {
    for (slot, value) in out[at..at + CHANNELS].iter_mut().zip(channels(yields)) {
        *slot += value;
    }
}

/// The features of a city at `tile` for `player` (see the module docs).
pub fn site_features(
    view: &PlayerView<'_>,
    rules: &CompiledRuleset,
    player: PlayerId,
    tile: TileId,
    inputs: &SiteInputs<'_>,
) -> [i32; SITE_FEATURES] {
    let mut out = land_features(view, rules, player, tile, inputs.nation);
    empire(view.dimensions(), tile, inputs, &mut out);
    heuristic(view, rules, player, tile, inputs, &mut out);
    out
}

/// `site_features` with the land from `land_features_memo` and the
/// heuristic given: `heuristic` is `SiteContext::score` of the site with
/// the cities of `inputs` (the caller has it), None when it is illegal.
pub fn site_features_with(
    view: &PlayerView<'_>,
    rules: &CompiledRuleset,
    player: PlayerId,
    tile: TileId,
    inputs: &SiteInputs<'_>,
    heuristic: Option<i32>,
) -> [i32; SITE_FEATURES] {
    let mut out = land_features_memo(view, rules, player, tile, inputs.nation);
    empire(view.dimensions(), tile, inputs, &mut out);
    out[HEURISTIC] = heuristic.unwrap_or(0);
    out[HEURISTIC + 1] = i32::from(heuristic.is_some());
    out
}

/// Sites the memo of a thread holds at most: beyond it, it starts over.
const MEMO_SITES: usize = 1 << 16;

thread_local! {
    /// `land_features` by (player, nation, tile): the fingerprint of what
    /// they read and the features.
    static LAND_MEMO: std::cell::RefCell<std::collections::HashMap<(PlayerId, u8, TileId), (u64, [i32; SITE_FEATURES])>> = Default::default();
}

/// `land_features`, remembered: the same answer, computed again only when
/// the fingerprint of what they read changed (`land_fingerprint`).
pub fn land_features_memo(view: &PlayerView<'_>, rules: &CompiledRuleset, player: PlayerId, tile: TileId, nation: u8) -> [i32; SITE_FEATURES] {
    let fingerprint = land_fingerprint(view, rules, player, tile);
    let key = (player, nation, tile);
    if let Some(features) = LAND_MEMO.with(|m| m.borrow().get(&key).filter(|(f, _)| *f == fingerprint).map(|(_, x)| *x)) {
        return features;
    }
    let features = land_features(view, rules, player, tile, nation);
    LAND_MEMO.with(|m| {
        let mut memo = m.borrow_mut();
        if memo.len() >= MEMO_SITES {
            memo.clear();
        }
        memo.insert(key, (fingerprint, features));
    });
    features
}

/// What the land features of a site read: the rules (by address: one
/// ruleset serves a thread's games), the map's size, and every known tile
/// within `CITY_WORK_RADIUS + 1` (a tile of the outer ring of the
/// territory reads its neighbours: fresh water, coast) as the player knows
/// it, pillage included. Not what they do not read: the route, and whose a
/// tile is beyond foreign or not (the features count the player's and
/// nobody's tiles alike, `territory` and the developed site's borders).
fn land_fingerprint(view: &PlayerView<'_>, rules: &CompiledRuleset, player: PlayerId, tile: TileId) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (rules as *const CompiledRuleset as usize).hash(&mut hasher);
    let dims = view.dimensions();
    (dims.width, dims.height, player, view.is_host(player), tile).hash(&mut hasher);
    for t in hex_disc(dims, tile, CITY_WORK_RADIUS + 1) {
        let known = view.tile(t).snapshot().map(|s| {
            let foreign = s.owner.is_some_and(|o| o != player);
            (s.terrain, s.feature, s.resource, s.improvement, s.river, s.natural_wonder, foreign, s.city)
        });
        (t, known, view.pillaged(t)).hash(&mut hasher);
    }
    hasher.finish()
}

/// The features of the land of a site (the module docs): every slot but
/// the distances, the moment and the heuristic, which stay 0.
pub fn land_features(view: &PlayerView<'_>, rules: &CompiledRuleset, player: PlayerId, tile: TileId, nation: u8) -> [i32; SITE_FEATURES] {
    let mut out = [0; SITE_FEATURES];
    let dims = view.dimensions();
    let area = territory(view, tile, CITY_WORK_RADIUS, player);
    let ring = |t: TileId| dims.distance(tile, t).unwrap_or(0).clamp(0, CITY_WORK_RADIUS) as usize;

    let all_techs: Vec<TechId> = (0..rules.technologies.len()).map(|i| TechId(i as u16)).collect();
    let builder = Builder { player, nation, techs: TechSet::of(&all_techs) };
    let mut planned = Planned { view, player, built: Vec::new() };
    for t in &area {
        let r = ring(*t);
        let nature = nature_yields(view, rules, *t).at_least(0);
        add_channels(&mut out, NATURE + r * CHANNELS, nature);
        let potential = if *t == tile {
            centre_yields(view, rules, *t)
        } else {
            let (best, build) = best_development(&planned, rules, &builder, *t);
            if let Some(build) = build {
                planned.built.push(build);
            }
            best
        };
        add_channels(&mut out, POTENTIAL + r * CHANNELS, potential);
    }

    let buildings = buildable_ever(rules, nation);
    let ctx = TerritoryContext {
        rules,
        player,
        techs: &all_techs,
        centre: tile,
        population: u32::MAX,
        buildings: &buildings,
    };
    for building in &buildings {
        let bonus = building_tile_bonus(&planned, rules, *building, &area, &ctx);
        add_channels(&mut out, BONUS_TILES, bonus.tiles);
        for count in bonus.city_effects.iter().filter(|c| c.met > 0 && conditional(&c.effect)) {
            if let Modifier::AddYield { yield_kind, amount } = count.effect.modifier {
                let mut yields = Yields::ZERO;
                yields.add(yield_kind, amount);
                add_channels(&mut out, BONUS_CITY, yields);
            }
        }
    }

    counts(view, rules, &area, &mut out);
    centre(view, rules, tile, &mut out);
    tiles(view, tile, player, &area, &mut out);
    out
}

/// The effect has a condition (either gate other than `Always`).
fn conditional(effect: &CompiledEffect) -> bool {
    effect.condition != Condition::Always || effect.second_condition != Condition::Always
}

/// The centre's yields: nature and the revealed resource, at least the
/// city minimum (the centre takes no improvement).
fn centre_yields(view: &PlayerView<'_>, rules: &CompiledRuleset, tile: TileId) -> Yields {
    let mut yields = nature_yields(view, rules, tile);
    if let Some(def) = view.resource(tile).and_then(|r| rules.resources.get(r.0 as usize)) {
        yields = yields.plus(def.yields);
    }
    yields.at_least_each(<ClassicYields as YieldTerms<Classic>>::CITY_MINIMUM).at_least(0)
}

/// The tile as it is, or with the improvement of the most valued yields
/// the rules allow there (ties: the lower id), and that build.
fn best_development(
    facts: &Planned<'_, '_>,
    rules: &CompiledRuleset,
    builder: &Builder,
    tile: TileId,
) -> (Yields, Option<Build>) {
    let mut best = plot_yields(facts, rules, tile, &builder.techs).at_least(0);
    let mut chosen = None;
    for (index, def) in rules.improvements.iter().enumerate() {
        if !worker_built(def, builder.nation) {
            continue;
        }
        let improvement = ImprovementId(index as u16);
        let Ok(plan) = site(facts, rules, builder, tile, improvement) else {
            continue;
        };
        let yields = plot_yields_with(facts, rules, tile, &builder.techs, improvement, plan.removes.is_some()).at_least(0);
        if yield_value(&yields) > yield_value(&best) {
            best = yields;
            chosen = Some(Build { tile, improvement, removes: plan.removes.is_some() });
        }
    }
    (best, chosen)
}

/// An improvement a worker (or a work boat) builds: a regular one or the
/// nation's unique, not one a great person or an event places.
fn worker_built(def: &crate::rules::compiled::ImprovementDefinition, nation: u8) -> bool {
    let kind_ok = def.kind == ImprovementKind::Regular || def.kind == ImprovementKind::Unique && def.nation == Some(nation);
    let built = (!def.unbuildable && def.work_turns > 0) || def.placement.water;
    kind_ok && built && !def.placement.prompt_when_complete
}

/// The buildings the player could ever build in a city (module docs).
pub fn buildable_ever(rules: &CompiledRuleset, nation: u8) -> Vec<BuildingKindId> {
    let replaced: Vec<BuildingKindId> = rules
        .buildings
        .iter()
        .filter(|b| b.nation == Some(nation))
        .filter_map(|b| b.replaces)
        .collect();
    rules
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.unbuildable && !b.wonder)
        .filter(|(_, b)| !matches!(b.kind, BuildingKind::Palace | BuildingKind::WorldWonder | BuildingKind::NationalWonder))
        .filter(|(_, b)| b.nation.is_none_or(|n| n == nation))
        .map(|(i, _)| BuildingKindId(i as u16))
        .filter(|b| !replaced.contains(b))
        .collect()
}

/// Resources by class, terrain kinds and features of the territory.
fn counts(view: &PlayerView<'_>, rules: &CompiledRuleset, area: &[TileId], out: &mut [i32]) {
    for t in area {
        let Some(snapshot) = view.tile(*t).snapshot().copied() else { continue };
        if let Some(class) = snapshot.resource.and_then(|r| rules.resources.get(r.0 as usize)).and_then(|d| d.class) {
            let slot = match class {
                ResourceClass::Bonus => 0,
                ResourceClass::Strategic => 1,
                ResourceClass::Luxury => 2,
                ResourceClass::Artifact => 3,
            };
            out[RESOURCES + slot] += 1;
        }
        if let Some(kind) = rules.terrain_kind_of(snapshot.terrain) {
            out[TERRAINS + (kind.0 as usize).min(TERRAIN_SLOTS - 1)] += 1;
        }
        if let Some(feature) = snapshot.feature {
            out[FEATURES + (feature.0 as usize).min(FEATURE_SLOTS - 1)] += 1;
        }
    }
}

/// River, coast, hills and fresh water of the centre.
fn centre(view: &PlayerView<'_>, rules: &CompiledRuleset, tile: TileId, out: &mut [i32]) {
    let hills = view.terrain(tile).is_some_and(|t| view.terrain_kinds().is_hills(t.0));
    let flags = [view.river(tile), view.coastal_land(tile), hills, fresh_water(view, rules, tile)];
    for (i, flag) in flags.into_iter().enumerate() {
        out[CENTRE + i] = i32::from(flag);
    }
}

/// Counts of the tiles within the radius.
fn tiles(view: &PlayerView<'_>, tile: TileId, player: PlayerId, area: &[TileId], out: &mut [i32]) {
    let mut unknown = 0;
    let mut foreign = 0;
    for t in hex_disc(view.dimensions(), tile, CITY_WORK_RADIUS) {
        match view.tile(t).snapshot() {
            None => unknown += 1,
            Some(s) if s.owner.is_some_and(|o| o != player) => foreign += 1,
            Some(_) => {}
        }
    }
    let land_river = area.iter().filter(|t| view.river(**t) && !view.is_water(**t)).count();
    let lakes = area.iter().filter(|t| view.lake(**t)).count();
    let wonders = area.iter().filter(|t| view.natural_wonder(**t).is_some()).count();
    let values = [area.len(), unknown, foreign, land_river, lakes, wonders];
    for (i, value) in values.into_iter().enumerate() {
        out[TILES + i] = value as i32;
    }
}

/// Distances to the known cities and the empire's numbers.
fn empire(dims: MapDimensions, tile: TileId, inputs: &SiteInputs<'_>, out: &mut [i32]) {
    let nearest = |cities: &[TileId]| cities.iter().filter_map(|c| dims.distance(*c, tile)).min().unwrap_or(FAR);
    out[EMPIRE] = nearest(inputs.own_cities);
    out[EMPIRE + 1] = nearest(inputs.foreign_cities);
    out[EMPIRE + 2] = inputs.own_cities.len() as i32;
    out[EMPIRE + 3] = inputs.round as i32;
    out[EMPIRE + 4] = inputs.excess_happiness;
}

/// The settling planner's own score of the site (no reservations and no
/// danger: the site as such).
fn heuristic(view: &PlayerView<'_>, rules: &CompiledRuleset, player: PlayerId, tile: TileId, inputs: &SiteInputs<'_>, out: &mut [i32]) {
    let ctx = SiteContext {
        view,
        profile: KnownMap::land(view).profile,
        rules,
        player,
        own_cities: inputs.own_cities,
        foreign_cities: inputs.foreign_cities,
        reserved: &[],
        danger: &[],
        tile_memo: Default::default(),
    };
    let score = ctx.score(tile);
    out[HEURISTIC] = score.unwrap_or(0);
    out[HEURISTIC + 1] = i32::from(score.is_some());
}

/// One planned build of the developed site.
#[derive(Debug, Clone, Copy)]
struct Build {
    tile: TileId,
    improvement: ImprovementId,
    /// The build removes the tile's feature.
    removes: bool,
}

/// The player's knowledge with the site developed: its unowned tiles the
/// player's (the city's borders), the planned builds standing (a removed
/// feature gone, nothing pillaged there). Everything else is the view's.
struct Planned<'v, 'a> {
    view: &'v PlayerView<'a>,
    player: PlayerId,
    built: Vec<Build>,
}

impl Planned<'_, '_> {
    fn build(&self, tile: TileId) -> Option<&Build> {
        self.built.iter().find(|b| b.tile == tile)
    }
}

impl TileFacts for Planned<'_, '_> {
    fn dimensions(&self) -> MapDimensions {
        self.view.dimensions()
    }
    fn terrain(&self, tile: TileId) -> Option<TerrainId> {
        self.view.terrain(tile)
    }
    fn feature(&self, tile: TileId) -> Option<FeatureId> {
        match self.build(tile) {
            Some(build) if build.removes => None,
            _ => self.view.feature(tile),
        }
    }
    fn natural_wonder(&self, tile: TileId) -> Option<crate::rules::content::NaturalWonderId> {
        self.view.natural_wonder(tile)
    }
    fn resource(&self, tile: TileId) -> Option<ResourceId> {
        self.view.resource(tile)
    }
    fn improvement(&self, tile: TileId) -> Option<ImprovementId> {
        match self.build(tile) {
            Some(build) => Some(build.improvement),
            None => self.view.improvement(tile),
        }
    }
    fn pillaged(&self, tile: TileId) -> bool {
        self.build(tile).is_none() && self.view.pillaged(tile)
    }
    fn owner(&self, tile: TileId) -> Option<PlayerId> {
        self.view.owner(tile).or(Some(self.player))
    }
    fn city(&self, tile: TileId) -> bool {
        self.view.city(tile)
    }
    fn river(&self, tile: TileId) -> bool {
        self.view.river(tile)
    }
    fn lake(&self, tile: TileId) -> bool {
        self.view.lake(tile)
    }
    fn coastal_land(&self, tile: TileId) -> bool {
        self.view.coastal_land(tile)
    }
    fn is_water(&self, tile: TileId) -> bool {
        self.view.is_water(tile)
    }
    fn is_host(&self, player: PlayerId) -> bool {
        self.view.is_host(player)
    }
}
