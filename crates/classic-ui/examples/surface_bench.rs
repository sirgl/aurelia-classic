//! Continuous-surface and action benchmark on representative (25k tiles) and stress
//! (50k tiles) maps. Release only; no window.
//!
//!   cargo run --release --offline -p aurelia-render --example surface_bench -- [rep|stress|all]
//!
//! Env: AURELIA_BENCH_RUNS (samples per case, default 20), AURELIA_BENCH_SIZE (WxH, 1664x936),
//! AURELIA_BENCH_EXPLORED_RUNS (fully explored surface builds, default 5; 0 skips them).
//!
//! Cases, each with warmup and N samples, median and p95:
//! - surface.build: CPU heightfield + geometry (CDT surface) + region meshes (ground,
//!   water, grid), triangles and the bytes a full upload writes; for the observer's
//!   knowledge and for a fully explored map.
//! - frame.rebuild: a frame after every layer changed, terrain built before the frame
//!   (full rebuild and upload).
//! - frame.steady: unchanged frames.
//! - move.explored / move.reveal: the frame after MoveUnit, split by whether the move
//!   explored new tiles. The terrain of a reveal is built by the terrain worker while
//!   frames go on: reveal.publish is the frame that applies it, reveal.worst the slowest
//!   frame from the command to the publish, reveal.latency the wall time from the
//!   command to the publish, reveal.build the worker's time.
//! - found_city: the frame after FoundCity of a spawned settler.
//! - end_turn: the frame after every player's EndTurn and a projection refresh, and
//!   the frames until a terrain it changed is published.
//!
//! Frame time = renderer CPU work + submit + GPU completion (device poll wait).
//! Every measured action follows 5 untimed frames, as in the game loop.
//! Lines follow the aurelia-bench protocol: `m <case> key=value ...`.

use std::time::Instant;

use classic_rules::ids::{PlayerId, TileId, UnitHandle, UnitKindId};
use classic_rules::rules::CompiledRuleset;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{Command, GameState};
use render::camera::CameraController;
use render::gpu::context::GpuCore;
use render::renderer::{FrameFlags, Renderer};
use render::world::projection::rebuild_render_world;
use render::RenderWorld;

/// The only code that differs between renderer generations (projection API).
mod compat {
    use super::*;

    /// The projection is already the observer's masked view.
    pub fn mask(_world: &mut RenderWorld) {}

    pub fn apply(world: &mut RenderWorld, game: &GameState, journal: &classic_rules::transaction::journal::ChangeJournal, rules: &CompiledRuleset) {
        let _ = render::world::projection::apply_changes(world, game.view(), journal, rules);
    }

    pub fn refresh(world: &mut RenderWorld, game: &GameState, rules: &CompiledRuleset) {
        let observer = PlayerId(world.observer);
        render::world::projection::refresh(world, game.view(), observer, rules);
    }

    pub fn touch_all(world: &mut RenderWorld) {
        world.touch_all();
    }

    pub fn detail(stats: &render::RenderStats) -> String {
        format!(
            "terrain_bytes={} feature_bytes={} instance_bytes={} knowledge_bytes={} overlay_bytes={} geometry_rebuilds={} regions_rebuilt={} feature_chunks={} fog_rows={} shadow_tris={} prepare_ms={:.3}",
            stats.terrain_bytes,
            stats.feature_bytes,
            stats.instance_bytes,
            stats.knowledge_bytes,
            stats.overlay_bytes,
            stats.geometry_rebuilds,
            stats.regions_rebuilt,
            stats.feature_chunks_rebuilt,
            stats.fog_rows_baked,
            stats.shadow_tris,
            stats.prepare_ms,
        )
    }
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn env_number(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn env_size() -> (u32, u32) {
    let value = std::env::var("AURELIA_BENCH_SIZE").unwrap_or_default();
    match value.split_once('x').map(|(w, h)| (w.parse(), h.parse())) {
        Some((Ok(w), Ok(h))) => (w, h),
        _ => (1664, 936),
    }
}

/// Median and p95 of a sample set (nearest rank).
fn summary(samples: &[f64]) -> String {
    if samples.is_empty() {
        return "n=0".into();
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    let p95 = sorted[((sorted.len() as f64 * 0.95).ceil() as usize).clamp(1, sorted.len()) - 1];
    format!("n={} median_ms={median:.3} p95_ms={p95:.3}", sorted.len())
}

fn median_u64(values: &[u64]) -> u64 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted.get(sorted.len() / 2).copied().unwrap_or(0)
}

fn max_u64(values: &[u64]) -> u64 {
    values.iter().copied().max().unwrap_or(0)
}

/// Every tile known: the geography of a fully explored late game.
fn reveal_all(world: &mut RenderWorld, game: &GameState) {
    let map = game.view().auth.map();
    for i in 0..world.terrain.len() {
        let tile = TileId(i as u32);
        world.visible[i] = 1;
        world.explored[i] = 1;
        world.terrain[i].terrain = map.terrain(tile).map(|t| t.0).unwrap_or(u16::MAX);
        world.terrain[i].owner = map.owner(tile).flatten().map(|p| p.0);
        world.terrain[i].resource = map.resource(tile).flatten().map(|r| r.0);
        world.forest[i] = map.feature(tile).flatten().is_some() as u8;
        world.improvements[i] = map.improvement(tile).flatten().map(|x| x.0);
        world.road[i] = map.road(tile) as u8;
        world.river[i] = map.river(tile) as u8;
    }
    world.river_edges = map.river_edges().to_vec();
}

struct BuildSample {
    total: f64,
    heightfield: f64,
    geometry: f64,
    meshes: f64,
    land_tris: usize,
    water_tris: usize,
    vertices: usize,
    bytes: u64,
}

/// CPU work of a full surface build, stage by stage.
fn build_surface(world: &RenderWorld, style: &render::scene::style::CompiledTerrainStyle) -> BuildSample {
    let start = Instant::now();
    let hf = render::scene::heightfield::build_heightfield(world, style);
    let heightfield = ms(start);
    let t = Instant::now();
    let geom = render::scene::terrain::fields::build_terrain_geometry(world, style, &hf);
    let geometry = ms(t);
    let surface = geom.surface.as_ref().expect("visual surface enabled");
    let t = Instant::now();
    let lines = render::gpu::buffers::build_grid_lines(world, &hf);
    let all: Vec<usize> = (0..surface.regions.len()).collect();
    let meshes = render::gpu::surface_mesh::region_meshes(surface, world, &lines, &all);
    let meshes_ms = ms(t);
    let vertices: usize = meshes.iter().map(|m| m.land.vertices.len()).sum();
    let indices: usize = meshes.iter().map(|m| m.land.indices.len()).sum();
    let water: usize = meshes.iter().map(|m| m.water.len()).sum();
    let grid: usize = meshes.iter().map(|m| m.grid.len()).sum();
    let vertex_size = std::mem::size_of::<render::gpu::buffers::TerrainVertex>();
    let water_size = std::mem::size_of::<render::gpu::buffers::WaterVertex>();
    BuildSample {
        total: ms(start),
        heightfield,
        geometry,
        meshes: meshes_ms,
        land_tris: indices / 3,
        water_tris: water / 3,
        vertices,
        bytes: (vertices * vertex_size + indices * 4 + water * water_size + grid * 12) as u64,
    }
}

fn bench_surface_build(name: &str, variant: &str, world: &RenderWorld, runs: usize) {
    let style = render::scene::style::compile(render::scene::style::default_style()).unwrap();
    build_surface(world, &style);
    let samples: Vec<BuildSample> = (0..runs).map(|_| build_surface(world, &style)).collect();
    let pick = |f: fn(&BuildSample) -> f64| samples.iter().map(f).collect::<Vec<_>>();
    let last = samples.last().expect("runs > 0");
    let known = (0..world.terrain.len()).filter(|i| world.explored[*i] == 1 || world.visible[*i] == 1).count();
    println!(
        "m surface.build.{name}.{variant} c_tiles={} known_tiles={known} {} land_tris={} water_tris={} vertices={} upload_bytes={}",
        world.terrain.len(),
        summary(&pick(|s| s.total)),
        last.land_tris,
        last.water_tris,
        last.vertices,
        last.bytes,
    );
    println!(
        "m surface.build.{name}.{variant}.stages heightfield_{} | geometry_{} | region_meshes_{}",
        summary(&pick(|s| s.heightfield)),
        summary(&pick(|s| s.geometry)),
        summary(&pick(|s| s.meshes)),
    );
}

struct Scene {
    core: GpuCore,
    renderer: Renderer,
    camera: CameraController,
    view: wgpu::TextureView,
    size: (u32, u32),
}

impl Scene {
    fn new(world: &RenderWorld, game: &GameState, rules: &CompiledRuleset) -> Self {
        let core = GpuCore::new_blocking().expect("GPU adapter");
        let layers = render::ground::layers();
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut renderer = Renderer::with_gpu(&core, format, &layers);
        renderer.set_unit_models(render::unit_models::UnitModelBook::from_rules(rules));
        renderer.set_world_models(render::world_models::WorldBook::from_rules(rules));
        let size = env_size();
        let target = core.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bench target"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut camera = CameraController::fit_map(world.width, world.height);
        camera.zoom = 2.25;
        if let Some(tile) = first_unit_tile(game) {
            let (x, z) = render::scene::layout::tile_to_world(tile.0 % world.width, tile.0 / world.width);
            camera.x = x;
            camera.y = z;
        }
        Self { core, renderer, camera, view, size }
    }

    /// Untimed frames, as the game loop renders between player actions: the measured
    /// frame after a command starts from a warm, idle GPU (not one ramped down by
    /// a previous multi-second rebuild).
    fn settle(&mut self, world: &RenderWorld) {
        for _ in 0..5 {
            self.frame(world);
        }
    }

    /// Frames until the terrain of `world` is published, from an action at `since`:
    /// each frame's time and stats, and the wall time from `since` to the publish.
    fn until_published(&mut self, world: &RenderWorld, since: Instant) -> (Vec<(f64, render::RenderStats)>, f64) {
        let mut frames = Vec::new();
        for _ in 0..100_000 {
            let sample = self.frame(world);
            let done = !sample.1.terrain_pending && !sample.1.chunks_pending;
            frames.push(sample);
            if done {
                break;
            }
        }
        (frames, ms(since))
    }

    /// One frame: renderer work, submit, and GPU completion.
    fn frame(&mut self, world: &RenderWorld) -> (f64, render::RenderStats) {
        let start = Instant::now();
        let flags = FrameFlags { grid: false, selected_city: None, yields: false };
        self.renderer.draw(&self.core, world, &self.camera, &self.view, self.size, flags);
        let _ = self.core.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        (ms(start), self.renderer.stats())
    }
}

fn first_unit_tile(game: &GameState) -> Option<TileId> {
    let auth = game.view().auth;
    let unit = *auth.indexes().units_of(PlayerId(0)).first()?;
    auth.units().get(unit).map(|u| u.core.tile)
}

/// Frame samples with their stats; prints one line per case.
#[derive(Default)]
struct Series {
    times: Vec<f64>,
    bytes: Vec<u64>,
    /// Renderer CPU of each frame (prepare to submit).
    cpu: Vec<f64>,
    last: Option<render::RenderStats>,
}

impl Series {
    fn push(&mut self, (time, stats): (f64, render::RenderStats)) {
        self.times.push(time);
        self.bytes.push(stats.bytes_uploaded);
        self.cpu.push(stats.prepare_ms as f64);
        self.last = Some(stats);
    }

    fn print(&self, case: &str) {
        let detail = self.last.as_ref().map(compat::detail).unwrap_or_default();
        println!(
            "m {case} {} cpu_{} upload_median_bytes={} upload_max_bytes={} last: {detail}",
            summary(&self.times),
            summary(&self.cpu).replace("n=", "n_"),
            median_u64(&self.bytes),
            max_u64(&self.bytes),
        );
    }
}

fn explored_count(world: &RenderWorld) -> usize {
    world.explored.iter().filter(|e| **e == 1).count()
}

/// A neighbour the unit can enter now, preferring tiles the observer already knows.
fn step_target(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle, world: &RenderWorld) -> Option<TileId> {
    let auth = game.view().auth;
    let core = auth.units().get(unit)?.core;
    if core.moves_left == 0 {
        return None;
    }
    let dims = auth.map().dimensions;
    let mut scratch = classic_rules::navigation::PathScratch::default();
    classic_rules::math::tiles_in_hex_radius(&dims, core.tile, 1)
        .into_iter()
        .filter(|t| *t != core.tile && world.explored.get(t.0 as usize) == Some(&1))
        .find(|t| {
            let command = Command::MoveUnit { unit, destination: *t };
            classic_rules::command::validate(game.view(), rules, command, &mut scratch).is_ok()
        })
}

/// Frames of the actions that change the terrain: the frame after the command, the
/// publish frame, the slowest frame until the publish, the latency and the worker time.
#[derive(Default)]
struct Reveals {
    next: Series,
    publish: Series,
    worst: Vec<f64>,
    latency: Vec<f64>,
    build: Vec<f64>,
    regions: Vec<f64>,
}

impl Reveals {
    fn push(&mut self, next: (f64, render::RenderStats), (frames, latency): (Vec<(f64, render::RenderStats)>, f64)) {
        let mut worst = next.0;
        self.next.push(next);
        for (time, stats) in &frames {
            worst = worst.max(*time);
            if stats.geometry_rebuilds > 0 {
                self.publish.push((*time, *stats));
                self.build.push(stats.terrain_build_ms as f64);
                self.regions.push(stats.regions_rebuilt as f64);
            }
        }
        self.worst.push(worst);
        self.latency.push(latency);
    }

    fn print(&self, case: &str) {
        self.next.print(case);
        self.publish.print(&format!("{case}.publish"));
        println!("m {case}.worst {}", summary(&self.worst));
        println!("m {case}.latency {}", summary(&self.latency));
        println!("m {case}.build {} regions_rebuilt_{}", summary(&self.build), summary(&self.regions).replace("_ms", ""));
    }
}

#[derive(Default)]
struct Moves {
    command_ms: Vec<f64>,
    explored: Series,
    reveal: Reveals,
}

impl Moves {
    /// One MoveUnit through the incremental projection, then one measured frame.
    fn step(&mut self, scene: &mut Scene, game: &mut GameState, world: &mut RenderWorld, rules: &CompiledRuleset, unit: UnitHandle, target: TileId) -> bool {
        scene.settle(world);
        let known = explored_count(world);
        let start = Instant::now();
        let Ok(result) = game.execute(rules, Command::MoveUnit { unit, destination: target }) else {
            return false;
        };
        compat::apply(world, game, result.changes(), rules);
        self.command_ms.push(ms(start));
        let sample = scene.frame(world);
        if explored_count(world) > known {
            let rest = scene.until_published(world, start);
            self.reveal.push(sample, rest);
        } else {
            self.explored.push(sample);
        }
        true
    }
}

/// Each unit steps to an explored neighbour and back: the return step never explores.
fn bench_moves(scene: &mut Scene, game: &mut GameState, world: &mut RenderWorld, rules: &CompiledRuleset, runs: usize) {
    let mut moves = Moves::default();
    let units: Vec<UnitHandle> = game.view().auth.indexes().units_of(PlayerId(0)).to_vec();
    'outer: for _round in 0..8 {
        for &unit in &units {
            if moves.explored.times.len() >= runs && moves.reveal.next.times.len() >= runs {
                break 'outer;
            }
            let Some(home) = game.view().auth.units().get(unit).map(|u| u.core.tile) else { continue };
            let Some(target) = step_target(game, rules, unit, world) else { continue };
            if moves.step(scene, game, world, rules, unit, target) {
                moves.step(scene, game, world, rules, unit, home);
            }
        }
        end_turn(game, world, rules);
        scene.frame(world);
    }
    println!("m move.command {}", summary(&moves.command_ms));
    moves.explored.print("move.explored");
    moves.reveal.print("move.reveal");
}

fn end_turn(game: &mut GameState, world: &mut RenderWorld, rules: &CompiledRuleset) {
    let players = game.view().auth.players().count();
    for p in 0..players {
        let _ = game.execute(rules, Command::EndTurn { player: PlayerId(p as u8) });
    }
    compat::refresh(world, game, rules);
}

fn bench_end_turn(scene: &mut Scene, game: &mut GameState, world: &mut RenderWorld, rules: &CompiledRuleset, runs: usize) {
    let mut turns = Reveals::default();
    for _ in 0..runs {
        scene.settle(world);
        let start = Instant::now();
        end_turn(game, world, rules);
        let next = scene.frame(world);
        let rest = scene.until_published(world, start);
        turns.push(next, rest);
    }
    turns.print("end_turn");
}

/// Explored, empty land tiles of the observer away from cities: settler spawn spots.
fn settler_spots(game: &GameState, world: &RenderWorld, count: usize) -> Vec<TileId> {
    let auth = game.view().auth;
    let dims = auth.map().dimensions;
    let mut spots: Vec<TileId> = Vec::new();
    for i in 0..world.terrain.len() {
        let tile = TileId(i as u32);
        let land = !matches!(auth.map().terrain(tile).map(|t| t.0), Some(4) | None);
        let empty = auth.indexes().units_on(tile).is_empty();
        let free = classic_rules::math::tiles_in_hex_radius(&dims, tile, 3)
            .into_iter()
            .all(|t| auth.indexes().city_on_tile(t).is_none() && !spots.contains(&t));
        if world.visible.get(i) == Some(&1) && land && empty && free {
            spots.push(tile);
            if spots.len() == count {
                break;
            }
        }
    }
    spots
}

fn bench_found_city(scene: &mut Scene, game: &mut GameState, world: &mut RenderWorld, rules: &CompiledRuleset, runs: usize) {
    let founder = rules.units.iter().position(|u| u.founder).expect("a founder unit") as u16;
    let mut series = Series::default();
    for tile in settler_spots(game, world, runs) {
        let settler = game.debug_spawn_unit(rules, PlayerId(0), tile, UnitKindId(founder));
        compat::refresh(world, game, rules);
        scene.settle(world);
        let Ok(result) = game.execute(rules, Command::FoundCity { settler }) else { continue };
        compat::apply(world, game, result.changes(), rules);
        series.push(scene.frame(world));
    }
    series.print("found_city");
}

fn bench_frames(scene: &mut Scene, world: &mut RenderWorld, runs: usize) {
    let mut rebuild = Series::default();
    scene.renderer.set_terrain_sync(render::gpu::terrain_gpu::TerrainSync::Immediate);
    for _ in 0..runs {
        compat::touch_all(world);
        rebuild.push(scene.frame(world));
    }
    scene.renderer.set_terrain_sync(render::gpu::terrain_gpu::TerrainSync::Background);
    rebuild.print("frame.rebuild");
    for _ in 0..10 {
        scene.frame(world);
    }
    let mut steady = Series::default();
    for _ in 0..runs.max(60) {
        steady.push(scene.frame(world));
    }
    steady.print("frame.steady");
}

/// Strategic zoom over the fully explored map, terrain level of detail off and on:
/// frame time and terrain triangles drawn (platform/MESHING.md §2.5). Terrain built before each
/// frame (immediate), so every frame shows the levels its camera asks for.
fn bench_strategic(scene: &mut Scene, world: &RenderWorld, name: &str, runs: usize) {
    let saved = scene.camera.clone();
    let (cx, cz) = render::scene::layout::map_center(world.width, world.height);
    scene.renderer.set_terrain_sync(render::gpu::terrain_gpu::TerrainSync::Immediate);
    for zoom in [0.8f32, 0.5] {
        for lod in [false, true] {
            scene.renderer.set_terrain_lod(lod);
            scene.camera = CameraController { x: cx, y: cz, zoom };
            for _ in 0..10 {
                scene.frame(world);
            }
            let mut series = Series::default();
            for _ in 0..runs.max(30) {
                series.push(scene.frame(world));
            }
            let stats = series.last.expect("frames");
            println!(
                "m strategic.{name}.zoom{zoom}.lod_{} {} terrain_tris={} world_tris={} unit_tris={} feature_tris_resident={} draws={} prepare_ms={:.3}",
                if lod { "on" } else { "off" },
                summary(&series.times),
                stats.terrain_tris,
                stats.world_tris,
                stats.unit_tris,
                stats.feature_tris,
                stats.draw_calls,
                stats.prepare_ms,
            );
        }
    }
    // A zoom out and back in as the game does it: terrain in the background, levels
    // following the camera a few regions per job. Every frame until the levels and the
    // deferred chunks settle.
    scene.renderer.set_terrain_sync(render::gpu::terrain_gpu::TerrainSync::Background);
    scene.renderer.set_terrain_lod(true);
    for (label, from, to) in [("out", 2.25f32, 0.5f32), ("in", 0.5, 2.25)] {
        let start = Instant::now();
        let mut series = Series::default();
        for i in 0..=60 {
            let t = i as f32 / 60.0;
            scene.camera = CameraController { x: cx, y: cz, zoom: from * (to / from).powf(t) };
            series.push(scene.frame(world));
        }
        for _ in 0..100_000 {
            let sample = scene.frame(world);
            let settled = !sample.1.terrain_building && !sample.1.chunks_pending;
            series.push(sample);
            if settled {
                break;
            }
        }
        let worst = series.times.iter().copied().fold(0.0, f64::max);
        println!("m strategic.{name}.zoom_{label} {} worst_ms={worst:.3} settle_ms={:.1} frames={}", summary(&series.times), ms(start), series.times.len());
    }
    scene.renderer.set_terrain_lod(false);
    scene.camera = saved;
}

fn run_map(name: &str, cfg: ScenarioConfig, rules: &CompiledRuleset, runs: usize) {
    let mut game = generate(&cfg, rules);
    let mut world = rebuild_render_world(game.view(), PlayerId(0), rules);
    world.visual_surface = true;
    compat::mask(&mut world);
    bench_surface_build(name, "observer", &world, runs);
    // A fully explored map takes seconds per build: its own (smaller) sample count.
    let explored_runs = env_number("AURELIA_BENCH_EXPLORED_RUNS", 5);
    if explored_runs > 0 {
        let mut all = world.clone();
        reveal_all(&mut all, &game);
        bench_surface_build(name, "explored", &all, explored_runs);
    }
    let mut scene = Scene::new(&world, &game, rules);
    let (first, stats) = scene.frame(&world);
    println!("m frame.first.{name} wall_ms={first:.3} bytes={} {}", stats.bytes_uploaded, compat::detail(&stats));
    bench_frames(&mut scene, &mut world, runs);
    bench_moves(&mut scene, &mut game, &mut world, rules, runs);
    bench_found_city(&mut scene, &mut game, &mut world, rules, runs);
    bench_end_turn(&mut scene, &mut game, &mut world, rules, runs);
    if std::env::var("AURELIA_BENCH_STRATEGIC").as_deref() != Ok("0") {
        let mut all = world.clone();
        reveal_all(&mut all, &game);
        all.touch_all();
        bench_strategic(&mut scene, &all, name, runs);
    }
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    let runs = env_number("AURELIA_BENCH_RUNS", 20).max(1);
    let rules = classic_rules::compile_rules(classic_content::core::core_rules()).expect("core rules");
    println!("# aurelia-bench v1 surface runs={runs} size={:?}", env_size());
    if which == "rep" || which == "all" {
        run_map("rep", ScenarioConfig::representative_late_game(), &rules, runs);
    }
    if which == "stress" || which == "all" {
        run_map("stress", ScenarioConfig::stress(), &rules, runs);
    }
}
