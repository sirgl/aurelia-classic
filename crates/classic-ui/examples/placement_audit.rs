//! Contact audit of the world objects: how far every model of the world kit floats above, or
//! sinks into, the final terrain surface when the real placement code stands it on real maps.
//!
//!   cargo run --release --offline -p aurelia-render --example placement_audit -- [out.json] [seed ...]
//!
//! For each seed a new game map is generated and fully revealed, the continuous visual surface
//! is built, and several passes put every improvement, resource, natural wonder, city kit piece
//! and world wonder on many tiles through `WorldObjects` (the code the renderer runs). Each
//! placed LOD0 mesh is then checked against the surface:
//!
//! - gap: how far a vertex of the model's base (the lowest 3.5 cm of the model, buried
//!   foundation course included) is above the ground under it, when no support (plinth, pile) carries it;
//!   the support's own base is checked instead. Positive = floating.
//! - sink: how far the ground stands above a vertex of the model's body (model y >= 5 cm).
//!   Positive = the ground cuts into the building.
//!
//! Support models (plinths, piles) are recognised by id; a tree without them reports none.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use classic_rules::ids::{PlayerId, TileId};
use render::gpu::buffers::HexInstance;
use render::gpu::world_objects::WorldObjects;
use render::scene::heightfield::build_heightfield;
use render::scene::style::{compile, default_style};
use render::scene::terrain::evaluator::sample_surface;
use render::scene::terrain::fields::{build_terrain_geometry, TerrainGeometry};
use render::world::projection::rebuild_render_world;
use render::world::state::{CityMarker, RenderWorld};
use render::world_models::{WorldBook, WorldModelId};

/// A gap above this many hex units is reported as floating (a hex is 1.0 across its corners).
const GAP_LIMITS: [f32; 2] = [0.03, 0.06];
/// A sink of the body above this is reported as ground inside the building.
const SINK_LIMITS: [f32; 2] = [0.05, 0.10];
const BODY_Y: f32 = 0.05;
/// The models that stand on water tiles (the world kit has no flag for them).
const SEA_MODELS: [&str; 6] =
    ["resource/fish", "resource/whales", "resource/pearls", "resource/crab", "improvement/fishing-boats", "improvement/offshore-platform"];

#[derive(Default, Clone)]
struct Stats {
    n: usize,
    gaps: Vec<f32>,
    sinks: Vec<f32>,
    supports: usize,
}

impl Stats {
    fn add(&mut self, gap: f32, sink: f32, supported: bool) {
        self.n += 1;
        self.gaps.push(gap);
        self.sinks.push(sink);
        self.supports += supported as usize;
    }

    fn quantile(values: &[f32], q: f32) -> f32 {
        if values.is_empty() {
            return 0.0;
        }
        let mut v = values.to_vec();
        v.sort_by(f32::total_cmp);
        v[(((v.len() - 1) as f32) * q).round() as usize]
    }

    fn share(values: &[f32], limit: f32) -> f32 {
        if values.is_empty() {
            return 0.0;
        }
        values.iter().filter(|v| **v > limit).count() as f32 / values.len() as f32
    }

    fn line(&self) -> String {
        format!(
            "n {:>6}  gap>{:.2} {:>5.1}%  gap>{:.2} {:>5.1}%  gap p95 {:>6.3} max {:>6.3} | sink>{:.2} {:>5.1}%  sink>{:.2} {:>5.1}%  sink p95 {:>6.3} max {:>6.3} | supported {:>5.1}%",
            self.n,
            GAP_LIMITS[0],
            100.0 * Self::share(&self.gaps, GAP_LIMITS[0]),
            GAP_LIMITS[1],
            100.0 * Self::share(&self.gaps, GAP_LIMITS[1]),
            Self::quantile(&self.gaps, 0.95),
            Self::quantile(&self.gaps, 1.0),
            SINK_LIMITS[0],
            100.0 * Self::share(&self.sinks, SINK_LIMITS[0]),
            SINK_LIMITS[1],
            100.0 * Self::share(&self.sinks, SINK_LIMITS[1]),
            Self::quantile(&self.sinks, 0.95),
            Self::quantile(&self.sinks, 1.0),
            100.0 * self.supports as f32 / self.n.max(1) as f32,
        )
    }

    fn json(&self) -> String {
        format!(
            "{{\"n\":{},\"gap_over_{}\":{:.4},\"gap_over_{}\":{:.4},\"gap_p95\":{:.4},\"gap_max\":{:.4},\"sink_over_{}\":{:.4},\"sink_over_{}\":{:.4},\"sink_p95\":{:.4},\"sink_max\":{:.4},\"supported\":{:.4}}}",
            self.n,
            GAP_LIMITS[0],
            Self::share(&self.gaps, GAP_LIMITS[0]),
            GAP_LIMITS[1],
            Self::share(&self.gaps, GAP_LIMITS[1]),
            Self::quantile(&self.gaps, 0.95),
            Self::quantile(&self.gaps, 1.0),
            SINK_LIMITS[0],
            Self::share(&self.sinks, SINK_LIMITS[0]),
            SINK_LIMITS[1],
            Self::share(&self.sinks, SINK_LIMITS[1]),
            Self::quantile(&self.sinks, 0.95),
            Self::quantile(&self.sinks, 1.0),
            self.supports as f32 / self.n.max(1) as f32,
        )
    }
}

/// The level a model contacts at (x, z): the displayed surface: the water where it stands above the ground (the sea bed is not what a
/// boat, a fish or a pile meets), the ground elsewhere.
fn ground(geom: &TerrainGeometry, p: [f32; 3]) -> Option<f32> {
    if let Some(d) = geom.surface.as_ref().and_then(|s| s.sample_display(p[0], p[2])) {
        return Some(d.y);
    }
    sample_surface(geom, p[0], p[2]).map(|s| s.y)
}

fn is_support(id: &str) -> bool {
    id.starts_with("support/")
}

/// Cells of 0.5 units holding the support instances: (centre x, z, radius, top, kind).
struct Supports {
    cells: BTreeMap<(i32, i32), Vec<([f32; 3], f32, f32)>>,
}

impl Supports {
    fn new() -> Self {
        Self { cells: BTreeMap::new() }
    }

    fn add(&mut self, i: &HexInstance) {
        // A plinth is unit height scaled by scale_height[1]; its footprint radius is scale_height[0].
        let key = ((i.offset[0] * 2.0).floor() as i32, (i.offset[2] * 2.0).floor() as i32);
        self.cells.entry(key).or_default().push((i.offset, i.scale_height[0], i.offset[1] + i.scale_height[1]));
    }

    /// Whether a support stands under the point (x, y, z): inside its radius, its top at y or higher.
    fn carries(&self, p: [f32; 3]) -> bool {
        let (cx, cz) = ((p[0] * 2.0).floor() as i32, (p[2] * 2.0).floor() as i32);
        for dx in -2..=2 {
            for dz in -2..=2 {
                for (o, r, top) in self.cells.get(&(cx + dx, cz + dz)).into_iter().flatten() {
                    if (o[0] - p[0]).hypot(o[2] - p[2]) <= r * 1.1 && *top >= p[1] - 0.03 && o[1] <= p[1] {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// (gap, sink, supported) of one placed non-support instance.
fn measure(geom: &TerrainGeometry, model: WorldModelId, inst: &HexInstance, supports: &Supports) -> Option<(f32, f32, bool)> {
    let m = model.model();
    let mesh = model.mesh(0);
    let base_limit = m.low.min(0.0) + 0.035;
    let (mut gap, mut sink, mut touched, mut carried) = (f32::NEG_INFINITY, f32::NEG_INFINITY, false, false);
    for (k, v) in mesh.iter().enumerate() {
        let body = v.pos[1] >= BODY_Y;
        let base = v.pos[1] <= base_limit;
        if !body && !base {
            continue;
        }
        if body && k % 3 != 0 {
            continue;
        }
        let p = inst.transform(v.pos);
        let Some(g) = ground(geom, p) else { continue };
        if base {
            if supports.carries(p) {
                carried = true;
            } else {
                gap = gap.max(p[1] - g);
                touched = true;
            }
        }
        if body {
            sink = sink.max(g - p[1]);
        }
    }
    if !touched && !carried && sink == f32::NEG_INFINITY {
        return None;
    }
    Some((if touched { gap } else { 0.0 }, sink.max(0.0), carried))
}

/// (gap) of a support instance: its base ring against the ground.
fn measure_support(geom: &TerrainGeometry, model: WorldModelId, inst: &HexInstance) -> f32 {
    let mut gap = f32::NEG_INFINITY;
    for v in model.mesh(0).iter().filter(|v| v.pos[1] <= 0.001) {
        let p = inst.transform(v.pos);
        if let Some(g) = ground(geom, p) {
            gap = gap.max(p[1] - g);
        }
    }
    gap.max(0.0)
}

struct Audit {
    by_model: BTreeMap<String, Stats>,
    by_kind: BTreeMap<String, Stats>,
    by_site: BTreeMap<String, Stats>,
    all: Stats,
    support_gaps: Stats,
    support_instances: usize,
    objects: usize,
}

fn classify(geom: &TerrainGeometry, world: &RenderWorld, inst: &HexInstance) -> String {
    let radius = 0.3;
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for k in 0..9 {
        let (x, z) = if k == 8 {
            (inst.offset[0], inst.offset[2])
        } else {
            let a = std::f32::consts::TAU * k as f32 / 8.0;
            (inst.offset[0] + radius * a.cos(), inst.offset[2] + radius * a.sin())
        };
        if let Some(s) = sample_surface(geom, x, z) {
            lo = lo.min(s.y);
            hi = hi.max(s.y);
        }
    }
    let relief = hi - lo;
    let slope = if relief < 0.03 { "plain" } else if relief < 0.15 { "slope" } else { "steep" };
    let tile = render::scene::picking::world_to_tile(inst.offset[0], inst.offset[2], (world.width, world.height));
    let mut tags = String::new();
    if let Some(TileId(t)) = tile {
        let t = t as usize;
        if world.terrain_kinds.is_hills(world.terrain[t].terrain) {
            tags.push_str("+hill");
        }
        if world.river.get(t).copied().unwrap_or(0) != 0 {
            tags.push_str("+river");
        }
        let (w, h) = (world.width as i32, world.height as i32);
        let (q, r) = (t as i32 % w, t as i32 / w);
        let coastal = render::scene::layout::EDGE_NEIGHBORS.iter().any(|(dq, dr)| {
            let (nq, nr) = (q + dq, r + dr);
            nq >= 0 && nr >= 0 && nq < w && nr < h && world.terrain_kinds.is_water(world.terrain[(nr * w + nq) as usize].terrain)
        });
        if coastal && !world.terrain_kinds.is_water(world.terrain[t].terrain) {
            tags.push_str("+coast");
        }
    }
    format!("{slope}{tags}")
}

/// Prints where the worst floating vertex of an instance is and what carries it (AUDIT_MODEL=id).
fn explain(geom: &TerrainGeometry, model: WorldModelId, inst: &HexInstance, supports: &Supports) {
    let m = model.model();
    let base_limit = m.low.min(0.0) + 0.035;
    let mut worst = (f32::NEG_INFINITY, [0.0f32; 3], [0.0f32; 3], 0.0f32);
    for v in model.mesh(0).iter().filter(|v| v.pos[1] <= base_limit) {
        let p = inst.transform(v.pos);
        if supports.carries(p) {
            continue;
        }
        if let Some(g) = ground(geom, p) {
            if p[1] - g > worst.0 {
                worst = (p[1] - g, v.pos, p, g);
            }
        }
    }
    eprintln!(
        "{} at ({:.2},{:.2}) y {:.3} yaw {:.2} scale {:?} radius {:.2} foot {:?} low {:.3} seat {:?}: worst gap {:.3} vertex {:?} -> world {:?} ground {:.3}",
        m.id, inst.offset[0], inst.offset[2], inst.offset[1], inst.yaw, inst.scale_height, m.radius, m.foot, m.low, m.seat, worst.0, worst.1, worst.2, worst.3
    );
    let (cx, cz) = ((inst.offset[0] * 2.0).floor() as i32, (inst.offset[2] * 2.0).floor() as i32);
    for dx in -2..=2 {
        for dz in -2..=2 {
            for (o, r, top) in supports.cells.get(&(cx + dx, cz + dz)).into_iter().flatten() {
                if (o[0] - inst.offset[0]).hypot(o[2] - inst.offset[2]) < 0.9 {
                    eprintln!("    support at ({:.2},{:.2}) bottom {:.3} top {:.3} radius {:.3}", o[0], o[2], o[1], top, r);
                }
            }
        }
    }
}

fn record(audit: &mut Audit, geom: &TerrainGeometry, world: &RenderWorld, batches: &render::gpu::world_objects::Batches, instances: &[HexInstance]) {
    let mut supports = Supports::new();
    for (model, range) in batches.placed() {
        if is_support(model.model().id) {
            for inst in &instances[range] {
                supports.add(inst);
            }
        }
    }
    for (model, range) in batches.placed() {
        let id = model.model().id;
        for inst in &instances[range] {
            if is_support(id) {
                audit.support_instances += 1;
                audit.support_gaps.add(measure_support(geom, model, inst), 0.0, false);
                continue;
            }
            let Some((gap, sink, carried)) = measure(geom, model, inst, &supports) else { continue };
            audit.objects += 1;
            if gap > 0.1 && std::env::var("AUDIT_MODEL").as_deref() == Ok(id) {
                explain(geom, model, inst, &supports);
            }
            let kind = id.split('/').next().unwrap_or("?").to_owned();
            let site = classify(geom, world, inst);
            let broad = site.split('+').next().unwrap_or("plain").to_owned();
            audit.by_model.entry(id.to_owned()).or_default().add(gap, sink, carried);
            audit.by_kind.entry(kind.clone()).or_default().add(gap, sink, carried);
            audit.by_kind.entry(format!("{kind}/{broad}")).or_default().add(gap, sink, carried);
            audit.by_site.entry(site).or_default().add(gap, sink, carried);
            audit.all.add(gap, sink, carried);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().cloned().unwrap_or_else(|| "/tmp/placement-audit.json".into());
    let seeds: Vec<u64> = args.iter().skip(1).filter_map(|a| a.parse().ok()).collect();
    let seeds = if seeds.is_empty() { vec![7, 20260928, 3] } else { seeds };
    let rules = classic_rules::compile_rules(classic_content::core::core_rules()).expect("BNW rules");
    let style = compile(default_style()).expect("style");
    let book = WorldBook::from_rules(&rules);
    let mut audit = Audit {
        by_model: BTreeMap::new(),
        by_kind: BTreeMap::new(),
        by_site: BTreeMap::new(),
        all: Stats::default(),
        support_gaps: Stats::default(),
        support_instances: 0,
        objects: 0,
    };
    let (mut build_ms, mut builds) = (0.0f64, 0usize);
    // Models made for water tiles (fish, boats, platforms) go on water, all the others on land.
    let split = |ids: Vec<(u16, Option<WorldModelId>)>| -> (Vec<u16>, Vec<u16>) {
        let mut land = Vec::new();
        let mut sea = Vec::new();
        for (id, model) in ids {
            match model {
                Some(m) if SEA_MODELS.contains(&m.model().id) => sea.push(id),
                Some(_) => land.push(id),
                None => {}
            }
        }
        (land, sea)
    };
    let (improvements, sea_improvements) =
        split((0..rules.improvements.len() as u16).map(|i| (i, book.improvement(i, 0))).collect());
    let (resources, sea_resources) = split((0..rules.resources.len() as u16).map(|r| (r, book.resource(r))).collect());
    let wonders: Vec<u16> = (0..rules.buildings.len() as u16).filter(|b| book.wonder(*b).is_some()).collect();
    let landmarks: Vec<u16> = (0..rules.buildings.len() as u16).filter(|b| book.landmark(*b).is_some()).collect();
    let walls = rules.buildings.iter().position(|b| b.key == "core:building/walls").map(|i| i as u16);
    let natural: Vec<u16> = (0..rules.natural_wonders.len() as u16).collect();
    for seed in &seeds {
        let cfg = classic_rules::scenario::ScenarioConfig {
            seed: *seed,
            players: 4,
            width: 48,
            height: 34,
            cities: 0,
            units: 0,
            water_ratio: 25,
        };
        let game = classic_rules::scenario::generate_new_game(&rules, &cfg);
        let base = rebuild_render_world(game.view(), PlayerId(0), &rules);
        let map = game.view().auth.map();
        for pass in 0..6usize {
            let mut world = base.clone();
            world.visual_surface = true;
            world.unlocked_resources = (0..rules.resources.len() as u16).collect();
            for i in 0..world.terrain.len() {
                let tile = TileId(i as u32);
                world.visible[i] = 1;
                world.explored[i] = 1;
                world.terrain[i].terrain = map.terrain(tile).map(|t| t.0).unwrap_or(u16::MAX);
                world.forest[i] = 0;
                world.road[i] = 0;
                world.river[i] = map.river(tile) as u8;
            }
            world.river_edges = map.river_edges().to_vec();
            let n = world.terrain.len();
            let wet = |w: &RenderWorld, i: usize| w.terrain_kinds.is_water(w.terrain[i].terrain);
            let mountain = |w: &RenderWorld, i: usize| w.terrain_kinds.is_mountains(w.terrain[i].terrain);
            let hash = |i: usize, salt: usize| render::scene::terrain::fields::hash_u32((i * 31 + salt * 977 + pass * 7919) as u32) as usize;
            // Cities on a thin subset of land tiles (no spacing rules: a stress case); the wonders
            // and landmarks rotate so that every model is placed over the passes.
            world.cities.clear();
            let mut city_tiles = Vec::new();
            for i in 0..n {
                if !wet(&world, i) && !mountain(&world, i) && hash(i, 1) % 9 == 0 {
                    city_tiles.push(i);
                }
            }
            for (k, &i) in city_tiles.iter().enumerate() {
                let mut buildings: Vec<u16> = walls.into_iter().collect();
                for j in 0..3 {
                    buildings.push(landmarks[(k * 5 + j * 7 + pass * 3) % landmarks.len()]);
                }
                for j in 0..2 {
                    buildings.push(wonders[(k * 3 + j * 11 + pass * 5) % wonders.len()]);
                }
                buildings.sort_unstable();
                buildings.dedup();
                world.cities.push(CityMarker {
                    tile: TileId(i as u32),
                    owner: (k % 4) as u8,
                    name: format!("C{k}"),
                    population: Some(6 + (k * 5 + pass) as u32 % 22),
                    buildings,
                    era: ((k + pass) % 8) as u8,
                    showpieces: Vec::new(),
                    unworked: Vec::new(),
                });
            }
            let is_city = |i: usize| city_tiles.contains(&i);
            for i in 0..n {
                world.improvements[i] = None;
                world.terrain[i].resource = None;
                world.natural_wonders[i] = None;
                if is_city(i) {
                    continue;
                }
                let land = !wet(&world, i);
                if land && !mountain(&world, i) {
                    if hash(i, 2) % 3 != 0 {
                        world.improvements[i] = Some(improvements[hash(i, 3) % improvements.len()]);
                    }
                    if hash(i, 4) % 3 == 0 {
                        world.terrain[i].resource = Some(resources[hash(i, 5) % resources.len()]);
                    }
                    if hash(i, 6) % 53 == 0 {
                        world.natural_wonders[i] = Some(natural[hash(i, 7) % natural.len()]);
                    }
                } else if wet(&world, i) && hash(i, 8) % 4 == 0 {
                    // Water: the sea resources, the water improvements.
                    world.terrain[i].resource = Some(sea_resources[hash(i, 9) % sea_resources.len()]);
                    world.improvements[i] = Some(sea_improvements[hash(i, 10) % sea_improvements.len()]);
                }
            }
            world.touch_all();
            let started = std::time::Instant::now();
            let hf = build_heightfield(&world, &style);
            let geom = build_terrain_geometry(&world, &style, &hf);
            let mut objects = WorldObjects::default();
            objects.set_book(book.clone());
            let t = std::time::Instant::now();
            let cities = objects.build_cities(&world, &geom);
            let props = objects.build_props(&world, &geom);
            build_ms += t.elapsed().as_secs_f64() * 1000.0;
            builds += 1;
            let _ = started;
            record(&mut audit, &geom, &world, &objects.props, &props);
            record(&mut audit, &geom, &world, &objects.cities, &cities);
            eprintln!("seed {seed} pass {pass}: {} objects so far", audit.objects);
        }
    }
    let mut text = String::new();
    let _ = writeln!(text, "ALL      {}", audit.all.line());
    let _ = writeln!(
        text,
        "supports instances {} (base gap > {:.2}: {:.1}%, max {:.3}); placement build {:.1} ms per scene ({} scenes)",
        audit.support_instances,
        GAP_LIMITS[0],
        100.0 * Stats::share(&audit.support_gaps.gaps, GAP_LIMITS[0]),
        Stats::quantile(&audit.support_gaps.gaps, 1.0),
        build_ms / builds.max(1) as f64,
        builds
    );
    let _ = writeln!(text, "\nBY KIND");
    for (k, s) in &audit.by_kind {
        let _ = writeln!(text, "{k:<24} {}", s.line());
    }
    let _ = writeln!(text, "\nBY SITE");
    for (k, s) in &audit.by_site {
        let _ = writeln!(text, "{k:<24} {}", s.line());
    }
    let mut worst: Vec<(&String, &Stats)> = audit.by_model.iter().collect();
    worst.sort_by(|a, b| {
        let key = |s: &Stats| Stats::quantile(&s.gaps, 0.95).max(Stats::quantile(&s.sinks, 0.95));
        key(b.1).total_cmp(&key(a.1))
    });
    let _ = writeln!(text, "\nWORST 25 MODELS BY max(p95 gap, p95 sink) ({} models placed)", audit.by_model.len());
    for (k, s) in worst.iter().take(25) {
        let _ = writeln!(text, "{k:<40} {}", s.line());
    }
    println!("{text}");
    let mut json = String::from("{\n");
    let _ = writeln!(json, "  \"all\": {},", audit.all.json());
    let _ = writeln!(json, "  \"models_placed\": {},", audit.by_model.len());
    let _ = writeln!(json, "  \"support_instances\": {},", audit.support_instances);
    let _ = writeln!(json, "  \"placement_build_ms_per_scene\": {:.2},", build_ms / builds.max(1) as f64);
    json.push_str("  \"by_kind\": {\n");
    let kinds: Vec<String> = audit.by_kind.iter().map(|(k, s)| format!("    \"{k}\": {}", s.json())).collect();
    json.push_str(&kinds.join(",\n"));
    json.push_str("\n  },\n  \"by_site\": {\n");
    let sites: Vec<String> = audit.by_site.iter().map(|(k, s)| format!("    \"{k}\": {}", s.json())).collect();
    json.push_str(&sites.join(",\n"));
    json.push_str("\n  },\n  \"by_model\": {\n");
    let models: Vec<String> = audit.by_model.iter().map(|(k, s)| format!("    \"{k}\": {}", s.json())).collect();
    json.push_str(&models.join(",\n"));
    json.push_str("\n  }\n}\n");
    std::fs::write(&out, json).expect("write json");
    eprintln!("wrote {out}");
}
