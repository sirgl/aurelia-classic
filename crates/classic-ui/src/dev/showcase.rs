//! World art in the real game, offscreen: the showcase of `--screenshot` with
//! AURELIA_GAME_SHOWCASE=world and the dense late-game map of `--bench-world`.
//!
//! Both are built only through classic-rules's harness setters (terrain, improvements, routes,
//! resources, natural wonders, cities, buildings, technologies, reveal), so the picture
//! comes from the ordinary projection and renderer, not from a hand-made render world.

use classic_rules::ids::{
    BuildingKindId, ImprovementId, PlayerId, ResourceId, TechId, TerrainId, TileId, UnitKindId,
};
use classic_rules::math::{HexCoord, MapDimensions};
use classic_rules::rules::content::{BuildingKind, NaturalWonderId};
use classic_rules::world::map::Route;
use classic_rules::{CompiledRuleset, GameState};
use crate::ui::screens::{self, PreparedMatch};

const PLAINS: u16 = 0;
const GRASSLAND: u16 = 1;
const HILL: u16 = 2;
const DESERT: u16 = 3;
const COAST: u16 = 4;

/// A camera position the screenshot frames: tile at the centre and zoom.
pub struct View {
    pub name: &'static str,
    pub tile: u32,
    pub zoom: f32,
}

/// Content index of a key in a ruleset table.
fn find<'a>(keys: impl Iterator<Item = &'a String>, key: &str) -> Option<u16> {
    keys.enumerate().find(|(_, k)| *k == key).map(|(i, _)| i as u16)
}

/// The harness: a game, its rules and its map size.
struct Stage<'a> {
    game: &'a mut GameState,
    rules: &'a CompiledRuleset,
    dims: MapDimensions,
}

impl Stage<'_> {
    fn coord(&self, tile: u32) -> HexCoord {
        self.dims.to_coord(TileId(tile)).expect("tile on the map")
    }

    fn at(&self, center: u32, dq: i32, dr: i32) -> Option<u32> {
        let c = self.coord(center);
        self.dims.to_tile(HexCoord { q: c.q + dq, r: c.r + dr }).map(|t| t.0)
    }

    /// Tiles within `radius` of `center`, by distance, then by angle around it.
    fn disc(&self, center: u32, radius: i32) -> Vec<u32> {
        let c = self.coord(center);
        let mut tiles: Vec<(i32, i32, u32)> = Vec::new();
        for r in -radius..=radius {
            for q in -radius..=radius {
                let p = HexCoord { q: c.q + q, r: c.r + r };
                let d = c.distance(p);
                if d > radius {
                    continue;
                }
                if let Some(t) = self.dims.to_tile(p) {
                    let (x, z) = render::scene::layout::tile_to_world(p.q as u32, p.r as u32);
                    let (cx, cz) = render::scene::layout::tile_to_world(c.q as u32, c.r as u32);
                    let angle = ((z - cz).atan2(x - cx).to_degrees() * 10.0) as i32;
                    tiles.push((d, angle, t.0));
                }
            }
        }
        tiles.sort();
        tiles.into_iter().map(|(_, _, t)| t).collect()
    }

    fn terrain(&mut self, tiles: &[(u32, u16)]) {
        let batch: Vec<(TileId, TerrainId)> = tiles.iter().map(|(t, k)| (TileId(*t), TerrainId(*k))).collect();
        self.game.debug_set_terrain_batch(self.rules, &batch);
    }

    fn unit(&mut self, owner: u8, tile: u32, key: &str) {
        if let Some(kind) = find(self.rules.units.iter().map(|u| &u.key), key) {
            self.game.debug_spawn_unit(self.rules, PlayerId(owner), TileId(tile), UnitKindId(kind));
        }
    }

    /// A city of `owner` with population and buildings (keys that exist in the ruleset).
    fn city(&mut self, owner: u8, tile: u32, population: u32, buildings: &[&str]) {
        let city = self.game.debug_found_city(self.rules, PlayerId(owner), TileId(tile));
        self.game.debug_grow_city(self.rules, city, population.saturating_sub(1));
        for key in buildings {
            if let Some(b) = find(self.rules.buildings.iter().map(|b| &b.key), key) {
                self.game.debug_add_building(self.rules, city, BuildingKindId(b));
            }
        }
    }

    /// Every technology of an era up to `era` (classic era index) for the player.
    fn reach_era(&mut self, owner: u8, era: u8) {
        for (i, tech) in self.rules.technologies.iter().enumerate() {
            let index = tech.content_era.and_then(|e| self.rules.eras.get(e.0 as usize)).map(|e| e.index);
            if index.is_some_and(|x| x <= era) {
                self.game.debug_learn_tech(self.rules, PlayerId(owner), TechId(i as u16));
            }
        }
    }

    /// Units of `owner` so that every given tile is within sight of one (greedy cover).
    /// Returns the tiles the units stand on.
    fn cover(&mut self, owner: u8, tiles: &[u32], land: &dyn Fn(u32) -> bool) -> Vec<u32> {
        let sight = classic_rules::derived::visibility::SIGHT_RADIUS;
        let mut posts: Vec<u32> = Vec::new();
        for &t in tiles {
            let seen = posts.iter().any(|p| self.coord(*p).distance(self.coord(t)) <= sight);
            if seen {
                continue;
            }
            let post = self.disc(t, 1).into_iter().find(|c| land(*c)).unwrap_or(t);
            posts.push(post);
        }
        for post in &posts {
            let kind = if land(*post) { "core:unit/scout" } else { "core:unit/trireme" };
            self.unit(owner, *post, kind);
        }
        posts
    }
}

/// Free showcase tiles, land and water, handed out nearest first.
struct Slots {
    land: Vec<u32>,
    water: Vec<u32>,
}

impl Slots {
    fn take(&mut self, water: bool) -> Option<u32> {
        let list = if water { &mut self.water } else { &mut self.land };
        if list.is_empty() {
            None
        } else {
            Some(list.remove(0))
        }
    }
}

/// Map terrain code of the first valid land terrain of a resource (plains when none).
fn resource_terrain(rules: &CompiledRuleset, resource: usize) -> u16 {
    let def = &rules.resources[resource];
    def.valid_terrains
        .iter()
        .filter_map(|t| rules.terrains.get(t.0 as usize).and_then(|t| t.map_code))
        .find(|code| *code != COAST)
        .unwrap_or(PLAINS)
}

fn water_resource(rules: &CompiledRuleset, resource: usize) -> bool {
    let def = &rules.resources[resource];
    let coast = def.valid_terrains.iter().any(|t| {
        rules.terrains.get(t.0 as usize).and_then(|t| t.map_code) == Some(COAST)
    });
    coast && resource_terrain(rules, resource) == PLAINS && def.valid_terrains.len() == 1
}

fn improvement_terrain(key: &str, n: usize) -> Option<u16> {
    match key {
        "core:improvement/mine" | "core:improvement/terrace-farm" => Some(HILL),
        "core:improvement/fishing-boats" | "core:improvement/offshore-platform" => None,
        "core:improvement/kasbah" => Some(DESERT),
        _ if n.is_multiple_of(2) => Some(PLAINS),
        _ => Some(GRASSLAND),
    }
}

/// The world showcase west of the map centre: a grown capital on a bay with landmarks and
/// wonders, every improvement, every resource (every other one improved), every natural
/// wonder, a railroad and roads, and three foreign cities in other eras.
/// `eras`: the era of player 0 (the capital) and of players 1..=3.
pub fn world(prepared: &mut PreparedMatch, eras: [u8; 4]) -> Vec<View> {
    let rules = screens::game_rules();
    let dims = prepared.game.view().auth.map().dimensions;
    let mut stage = Stage { game: &mut prepared.game, rules: &rules, dims };
    let center = dims.to_tile(HexCoord { q: dims.width as i32 / 2 - 3, r: dims.height as i32 / 2 - 2 }).unwrap().0;
    let region = stage.disc(center, 7);
    // A bay south of the capital (toward the camera); open ground elsewhere, no forest.
    let bay = |stage: &Stage<'_>, t: u32| {
        let (a, b) = (stage.coord(center), stage.coord(t));
        let (dq, dr) = (b.q - a.q, b.r - a.r);
        dr >= 1 && (dq + dr).abs() <= 1 + dr / 2 && dq <= 0 || (dr >= 1 && dq == 0)
    };
    let ground: Vec<(u32, u16)> = region
        .iter()
        .map(|t| (*t, if bay(&stage, *t) { COAST } else if t.is_multiple_of(3) { GRASSLAND } else { PLAINS }))
        .collect();
    stage.terrain(&ground);
    let cleared: Vec<TileId> = region.iter().map(|t| TileId(*t)).collect();
    stage.game.debug_clear_features(&rules, &cleared);
    let water: Vec<u32> = region.iter().copied().filter(|t| bay(&stage, *t)).collect();
    let others = foreign_cities(&mut stage, center, &eras);
    let mut seen: Vec<u32> = region.clone();
    for (tile, _) in &others {
        seen.extend(stage.disc(*tile, 2));
    }
    let land = |t: u32| !water.contains(&t);
    let posts = stage.cover(0, &seen, &land);
    let far = |t: &u32| stage.coord(*t).distance(stage.coord(center)) >= 3 && !posts.contains(t);
    let mut slots = Slots {
        land: region.iter().copied().filter(|t| far(t) && land(*t)).collect(),
        water: region.iter().copied().filter(|t| far(t) && !land(*t)).collect(),
    };
    place_improvements(&mut stage, &mut slots);
    place_resources(&mut stage, &mut slots);
    place_natural_wonders(&mut stage, &mut slots);
    place_routes(&mut stage, center, &others);
    stage.reach_era(0, eras[0]);
    stage.city(0, center, 20, CAPITAL);
    let explored: Vec<TileId> = seen.iter().map(|t| TileId(*t)).collect();
    stage.game.debug_reveal(PlayerId(0), &explored);
    // Camera zoom: 2.25 is the default game view, larger is closer.
    let mut views = vec![
        View { name: "city", tile: center, zoom: 3.4 },
        View { name: "city-close", tile: center, zoom: 7.0 },
        View { name: "improvements", tile: stage.at(center, 3, -2).unwrap_or(center), zoom: 3.4 },
        View { name: "resources", tile: stage.at(center, -4, 0).unwrap_or(center), zoom: 3.4 },
        View { name: "natural-wonders", tile: stage.at(center, 2, -6).unwrap_or(center), zoom: 2.6 },
        View { name: "overview", tile: center, zoom: 1.2 },
    ];
    for (i, (tile, _)) in others.iter().enumerate() {
        let name = ["era-a", "era-b", "era-c"][i.min(2)];
        views.push(View { name, tile: *tile, zoom: 5.0 });
    }
    views
}

/// The placement showcase (AURELIA_GAME_SHOWCASE=placement): five sites in a row, each a city of
/// player 0 on a kind of ground that stresses seating (open plain, slope beside hills, a
/// hill top, a river bank, a sea cliff), built fresh (`developed` false: population 3, no
/// buildings, the ancient era) or grown (population 20, walls, castle, harbour, temple,
/// factory, the industrial era). One view per site; the camera angle is AURELIA_GAME_PITCH.
pub fn placement(prepared: &mut PreparedMatch, developed: bool) -> Vec<View> {
    let rules = screens::game_rules();
    let dims = prepared.game.view().auth.map().dimensions;
    let mut stage = Stage { game: &mut prepared.game, rules: &rules, dims };
    let middle = dims.to_tile(HexCoord { q: dims.width as i32 / 2, r: dims.height as i32 / 2 }).unwrap().0;
    let names = ["site-plain", "site-slope", "site-hilltop", "site-river", "site-cliff"];
    let mut views = Vec::new();
    let mut seen: Vec<u32> = Vec::new();
    let mut river: Vec<(TileId, TileId)> = Vec::new();
    let mut towns: Vec<u32> = Vec::new();
    let farm = find(rules.improvements.iter().map(|i| &i.key), "core:improvement/farm");
    let mine = find(rules.improvements.iter().map(|i| &i.key), "core:improvement/mine");
    let mut improvements = Vec::new();
    for (k, name) in names.iter().enumerate() {
        let Some(center) = stage.at(middle, (k as i32 - 2) * 7, 0) else { continue };
        let c = stage.coord(center);
        let region = stage.disc(center, 3);
        let ground: Vec<(u32, u16)> = region
            .iter()
            .map(|t| {
                let p = stage.coord(*t);
                let (dq, dr, d) = (p.q - c.q, p.r - c.r, c.distance(p));
                let kind = match k {
                    1 if dq + dr / 2 < 0 && d >= 1 => HILL,
                    2 if d == 0 => HILL,
                    4 if dr >= 1 => COAST,
                    _ => PLAINS,
                };
                (*t, kind)
            })
            .collect();
        stage.terrain(&ground);
        let cleared: Vec<TileId> = region.iter().map(|t| TileId(*t)).collect();
        stage.game.debug_clear_features(&rules, &cleared);
        if k == 3 {
            for dr in -3..=2 {
                if let (Some(a), Some(b)) = (stage.at(center, 0, dr), stage.at(center, 1, dr)) {
                    river.push((TileId(a), TileId(b)));
                    if let Some(next) = stage.at(center, 0, dr + 1) {
                        river.push((TileId(b), TileId(next)));
                    }
                }
            }
        }
        if let (Some(farm), Some(t)) = (farm, stage.at(center, 2, -1)) {
            improvements.push((TileId(t), ImprovementId(farm)));
        }
        if let (Some(mine), Some(t)) = (mine, stage.at(center, -1, 0).filter(|_| k == 1)) {
            improvements.push((TileId(t), ImprovementId(mine)));
        }
        seen.extend(region.iter().copied());
        towns.push(center);
        views.push(View { name, tile: center, zoom: 6.5 });
    }
    stage.game.debug_set_river_edges(&rules, &river);
    stage.game.debug_set_improvement_batch(&rules, &improvements);
    stage.reach_era(0, if developed { 5 } else { 0 });
    let grown: &[&str] = &[
        "core:building/walls",
        "core:building/castle",
        "core:building/harbor",
        "core:building/temple",
        "core:building/university",
        "core:building/factory",
    ];
    for center in towns {
        stage.city(0, center, if developed { 20 } else { 3 }, if developed { grown } else { &[] });
    }
    let explored: Vec<TileId> = seen.iter().map(|t| TileId(*t)).collect();
    stage.game.debug_reveal(PlayerId(0), &explored);
    views
}

const CAPITAL: &[&str] = &[
    "core:building/palace",
    "core:building/walls",
    "core:building/castle",
    "core:building/harbor",
    "core:building/factory",
    "core:building/university",
    "core:building/stadium",
    "core:building/temple",
    "core:building/pyramids",
    "core:building/colossus",
    "core:building/great_library",
    "core:building/eiffel_tower",
    "core:building/taj_mahal",
    "core:building/statue_of_liberty",
    "core:building/sydney_opera_house",
];

/// Cities of players 1..=3 east, north-west and north-east of the showcase, each in its era.
fn foreign_cities(stage: &mut Stage<'_>, center: u32, eras: &[u8; 4]) -> Vec<(u32, u8)> {
    let spots = [(10, -1), (-4, -7), (6, -10)];
    let wonders = ["core:building/hagia_sophia", "core:building/big_ben", "core:building/kremlin"];
    let mut out = Vec::new();
    for (i, (dq, dr)) in spots.into_iter().enumerate() {
        let owner = i as u8 + 1;
        if owner as usize >= stage.game.view().auth.players().count() {
            break;
        }
        let Some(tile) = stage.at(center, dq, dr) else { continue };
        let ring: Vec<(u32, u16)> = stage.disc(tile, 2).into_iter().map(|t| (t, GRASSLAND)).collect();
        stage.terrain(&ring);
        let cleared: Vec<TileId> = ring.iter().map(|(t, _)| TileId(*t)).collect();
        stage.game.debug_clear_features(stage.rules, &cleared);
        stage.reach_era(owner, eras[owner as usize]);
        let buildings = ["core:building/walls", "core:building/temple", "core:building/market", wonders[i]];
        stage.city(owner, tile, 8 + 4 * i as u32, &buildings);
        out.push((tile, owner));
    }
    out
}

/// Every improvement, and the farm on two more terrains.
fn place_improvements(stage: &mut Stage<'_>, slots: &mut Slots) {
    let rules = stage.rules;
    let mut items: Vec<(&str, Option<u16>)> = rules
        .improvements
        .iter()
        .enumerate()
        .map(|(n, i)| (i.key.as_str(), improvement_terrain(&i.key, n)))
        .collect();
    items.extend([("core:improvement/farm", Some(GRASSLAND)), ("core:improvement/farm", Some(DESERT))]);
    let mut terrain = Vec::new();
    let mut built = Vec::new();
    for (key, kind) in items {
        let Some(tile) = slots.take(kind.is_none()) else { continue };
        let id = find(rules.improvements.iter().map(|i| &i.key), key).expect("improvement");
        if let Some(kind) = kind {
            terrain.push((tile, kind));
        }
        built.push((TileId(tile), ImprovementId(id)));
    }
    stage.terrain(&terrain);
    // Lumber mill and brazilwood camp stand in their forest / jungle.
    for (key, feature) in [
        ("core:improvement/lumber-mill", "core:feature/forest"),
        ("core:improvement/brazilwood-camp", "core:feature/jungle"),
    ] {
        let id = find(rules.improvements.iter().map(|i| &i.key), key);
        let f = find(rules.features.iter().map(|x| &x.key), feature);
        if let (Some(id), Some(f)) = (id, f) {
            let tiles: Vec<(TileId, classic_rules::ids::FeatureId)> = built
                .iter()
                .filter(|(_, i)| i.0 == id)
                .map(|(t, _)| (*t, classic_rules::ids::FeatureId(f)))
                .collect();
            stage.game.debug_set_feature_batch(rules, &tiles);
        }
    }
    stage.game.debug_set_improvement_batch(rules, &built);
}

/// Every resource with map presence; every other one also gets its improvement.
fn place_resources(stage: &mut Stage<'_>, slots: &mut Slots) {
    let rules = stage.rules;
    let book = render::world_models::WorldBook::from_rules(rules);
    let mut terrain = Vec::new();
    let mut resources = Vec::new();
    let mut improved = Vec::new();
    let shown = (0..rules.resources.len()).filter(|r| book.resource(*r as u16).is_some());
    for (n, r) in shown.enumerate() {
        let wet = water_resource(rules, r);
        let Some(tile) = slots.take(wet) else { continue };
        if !wet {
            terrain.push((tile, resource_terrain(rules, r)));
        }
        resources.push((TileId(tile), ResourceId(r as u16)));
        if let Some(imp) = rules.resources[r].improvements.first().filter(|_| n.is_multiple_of(2)) {
            improved.push((TileId(tile), *imp));
        }
    }
    stage.terrain(&terrain);
    stage.game.debug_set_resource_batch(rules, &resources);
    stage.game.debug_set_improvement_batch(rules, &improved);
}

/// Every natural wonder; land ones a tile apart so the large models do not touch.
fn place_natural_wonders(stage: &mut Stage<'_>, slots: &mut Slots) {
    let rules = stage.rules;
    let sea = ["core:natural-wonder/the-great-barrier-reef", "core:natural-wonder/krakatoa"];
    let mut terrain = Vec::new();
    let mut wonders = Vec::new();
    for (i, def) in rules.natural_wonders.iter().enumerate() {
        let wet = sea.contains(&def.key.as_str());
        let tile = if wet { slots.water.pop() } else { slots.take(false) };
        let Some(tile) = tile else { continue };
        if !wet {
            terrain.push((tile, PLAINS));
            slots.take(false);
        }
        wonders.push((TileId(tile), NaturalWonderId(i as u16)));
    }
    stage.terrain(&terrain);
    stage.game.debug_set_natural_wonder_batch(rules, &wonders);
}

/// A railroad from the capital to the eastern city and a road to the others.
fn place_routes(stage: &mut Stage<'_>, center: u32, others: &[(u32, u8)]) {
    let mut routes = Vec::new();
    for (i, (tile, _)) in others.iter().enumerate() {
        let kind = if i == 0 { Route::Railroad } else { Route::Road };
        let (a, b) = (stage.coord(center), stage.coord(*tile));
        let steps = a.distance(b);
        for s in 1..steps {
            // Straight line in cube coordinates, rounded to hexes.
            let t = s as f32 / steps as f32;
            let (q, r) = (a.q as f32 + (b.q - a.q) as f32 * t, a.r as f32 + (b.r - a.r) as f32 * t);
            let (x, z, y) = (q, r, -q - r);
            let (mut rx, mut rz, ry) = (x.round(), z.round(), y.round());
            let (dx, dz, dy) = ((rx - x).abs(), (rz - z).abs(), (ry - y).abs());
            if dx > dz && dx > dy {
                rx = -ry - rz;
            } else if dz > dy {
                rz = -rx - ry;
            }
            if let Some(t) = stage.dims.to_tile(HexCoord { q: rx as i32, r: rz as i32 }) {
                routes.push((t, kind));
            }
        }
    }
    stage.game.debug_set_route_batch(stage.rules, &routes);
}

/// The dense late-game map of `--bench-world`: a generated map where every land tile is
/// improved and on a route, a city of player 0 every few tiles (so the observer sees all;
/// population 8-20, landmarks and
/// walls), all 47 world wonders spread over the cities, the natural wonders on open
/// land, every player in the information era and the whole map explored by player 0.
pub fn dense(prepared: &mut PreparedMatch) {
    let rules = screens::game_rules();
    let dims = prepared.game.view().auth.map().dimensions;
    let players = prepared.game.view().auth.players().count() as u8;
    let mut stage = Stage { game: &mut prepared.game, rules: &rules, dims };
    for p in 0..players {
        stage.reach_era(p, 7);
    }
    let land: Vec<u32> = (0..dims.tile_count() as u32)
        .filter(|t| {
            let terrain = stage.game.view().auth.map().terrain(TileId(*t)).map(|x| x.0);
            matches!(terrain, Some(PLAINS | GRASSLAND | HILL | DESERT | 5))
        })
        .collect();
    let mut cities = Vec::new();
    for &t in &land {
        let c = stage.coord(t);
        if c.q % 5 == (c.r * 2) % 5 && c.r % 3 == 1 {
            cities.push(t);
        }
    }
    let wonders: Vec<&str> = rules
        .buildings
        .iter()
        .filter(|b| b.kind == BuildingKind::WorldWonder)
        .map(|b| b.key.as_str())
        .collect();
    let landmarks = [
        "core:building/walls", "core:building/castle", "core:building/factory", "core:building/university",
        "core:building/stadium", "core:building/temple", "core:building/harbor", "core:building/military_base",
        "core:building/broadcast_tower", "core:building/airport",
    ];
    let farm = find(rules.improvements.iter().map(|i| &i.key), "core:improvement/farm").unwrap();
    let mine = find(rules.improvements.iter().map(|i| &i.key), "core:improvement/mine").unwrap();
    let mut improvements = Vec::new();
    let mut routes = Vec::new();
    for &t in &land {
        if cities.contains(&t) {
            continue;
        }
        let hill = stage.game.view().auth.map().terrain(TileId(t)).map(|x| x.0) == Some(HILL);
        let resource = stage.game.view().auth.map().resource(TileId(t)).flatten();
        let fitting = resource.and_then(|r| rules.resources[r.0 as usize].improvements.first().copied());
        let id = fitting.unwrap_or(ImprovementId(if hill { mine } else { farm }));
        improvements.push((TileId(t), id));
        routes.push((TileId(t), if t % 4 == 0 { Route::Road } else { Route::Railroad }));
    }
    let forested: Vec<TileId> = land.iter().map(|t| TileId(*t)).collect();
    stage.game.debug_clear_features(&rules, &forested);
    stage.game.debug_set_improvement_batch(&rules, &improvements);
    stage.game.debug_set_route_batch(&rules, &routes);
    for (i, &tile) in cities.iter().enumerate() {
        let mut buildings: Vec<&str> = landmarks.iter().copied().take(3 + i % 6).collect();
        buildings.extend(wonders.iter().skip(i * 2).take(2));
        stage.city(0, tile, 8 + (i as u32 * 5) % 13, &buildings);
    }
    let wonder_tiles: Vec<(TileId, NaturalWonderId)> = land
        .iter()
        .filter(|t| !cities.contains(t))
        .step_by(land.len() / rules.natural_wonders.len().max(1) + 1)
        .zip(0..rules.natural_wonders.len() as u16)
        .map(|(t, w)| (TileId(*t), NaturalWonderId(w)))
        .collect();
    stage.game.debug_set_natural_wonder_batch(&rules, &wonder_tiles);
    let all: Vec<TileId> = (0..dims.tile_count() as u32).map(TileId).collect();
    stage.game.debug_reveal(PlayerId(0), &all);
    println!(
        "dense map {}x{}: {} cities, {} improved tiles, {} wonders placed in cities",
        dims.width,
        dims.height,
        cities.len(),
        improvements.len(),
        (cities.len() * 2).min(wonders.len())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use render::scene::terrain::surface;

    /// The bay of the world showcase is known water: the surface must hold
    /// water there, and the projection must call it visible.
    #[test]
    fn showcase_bay_is_known_water_in_the_surface() {
        let request = screens::MatchRequest {
            civilization: "core:nation/rome".into(),
            seed: 20260928,
            size: screens::MapSize::Large,
            players: 4,
            water: 25,
        };
        let mut prepared = screens::prepare_match(request).expect("world");
        world(&mut prepared, [0, 1, 2, 3]);
        let rules = screens::game_rules();
        let mut render = render::world::projection::rebuild_render_world(prepared.game.view(), PlayerId(0), &rules);
        render.visual_surface = true;
        let style = render::scene::style::compile(render::scene::style::default_style()).unwrap();
        let mesh = surface::build(&render, &style);
        let dims = prepared.game.view().auth.map().dimensions;
        let center = dims.to_tile(HexCoord { q: dims.width as i32 / 2 - 3, r: dims.height as i32 / 2 - 2 }).unwrap();
        let mut missing = vec![];
        for t in classic_rules::math::tiles_in_hex_radius(&dims, center, 7) {
            if prepared.game.view().auth.map().terrain(t) != Some(TerrainId(COAST)) {
                continue;
            }
            let c = dims.to_coord(t).unwrap();
            let (x, z) = render::scene::layout::tile_to_world(c.q as u32, c.r as u32);
            let fog = render::scene::fog::fog_at(&render, t);
            let water = mesh.sample_water(x, z).is_some();
            let ground = mesh.sample_ground(x, z).is_some();
            eprintln!("tile {} ({},{}) fog {fog} water {water} ground {ground} region {:?}", t.0, c.q, c.r, mesh.region_of_point(x, z));
            if !water || fog < 1.0 {
                missing.push(t.0);
            }
        }
        assert!(missing.is_empty(), "bay tiles without visible water: {missing:?}");
    }
}
