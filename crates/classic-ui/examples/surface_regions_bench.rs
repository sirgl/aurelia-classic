//! Surface build bench (platform/MESHING.md §3, M1): whole map and regions, on the
//! sandbox island and on the 25k-tile representative late game.
//!
//! Method: release build, 2 warm-up builds per case, then `RUNS` timed
//! builds; median and p95 of wall time. `build` runs regions on worker
//! threads; `profile` repeats the build on one thread and times each step
//! and each region. Output uses the aurelia-bench line protocol.
//!
//! Run: `cargo run --release --offline -p aurelia-render --example surface_regions_bench [runs] [case] [size]`
//! (`case`: island, rep or rep_revealed; `size`: one region size, 0 for the whole map).
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{compile_rules, ids::PlayerId};
use render::scene::style::{compile, default_style, CompiledTerrainStyle};
use render::scene::terrain::surface::{self, fixtures, BuildProfile};
use render::world::{projection::rebuild_render_world, state::RenderWorld};
use std::time::Instant;

fn stats(mut times: Vec<f64>) -> (f64, f64) {
    times.sort_by(f64::total_cmp);
    (times[times.len() / 2], times[(times.len() - 1) * 95 / 100])
}

/// AURELIA_LEVEL=n builds every region at detail level n (platform/MESHING.md §2.5, LOD).
fn levels(world: &RenderWorld, size: u32) -> surface::Levels {
    let level: u8 = std::env::var("AURELIA_LEVEL").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut levels = surface::Levels::default();
    for y in 0..world.height.div_ceil(size) {
        for x in 0..world.width.div_ceil(size) {
            levels.set(surface::RegionId { x, y }, level);
        }
    }
    levels
}

fn time_builds(world: &RenderWorld, style: &CompiledTerrainStyle, size: u32, runs: usize) -> (f64, f64, surface::SurfaceMesh) {
    let levels = levels(world, size);
    let build = || surface::SurfaceBuilder::default().build(world, style, size, &levels, None);
    // Two warm-up builds.
    build();
    let mut mesh = build();
    let mut times = vec![];
    for _ in 0..runs {
        let start = Instant::now();
        mesh = build();
        times.push(start.elapsed().as_secs_f64() * 1e3);
    }
    let (median, p95) = stats(times);
    (median, p95, mesh)
}

fn profiles(world: &RenderWorld, style: &CompiledTerrainStyle, size: u32, runs: usize) -> Vec<BuildProfile> {
    surface::profile(world, style, size);
    surface::profile(world, style, size);
    (0..runs).map(|_| surface::profile(world, style, size)).collect()
}

fn report(name: &str, world: &RenderWorld, style: &CompiledTerrainStyle, runs: usize) {
    // An optional second argument selects cases by name.
    if std::env::args().nth(2).is_some_and(|only| only != name) {
        return;
    }
    let known = (0..world.terrain.len()).filter(|&i| world.explored[i] == 1).count();
    let sizes: Vec<u32> = match std::env::args().nth(3).and_then(|v| v.parse::<u32>().ok()) {
        Some(0) => vec![1 << 16],
        Some(size) => vec![size],
        None => vec![1 << 16, 32, 16, 8],
    };
    let profile_size = match sizes.as_slice() {
        [size] if *size != 1 << 16 => *size,
        _ => surface::REGION,
    };
    for size in sizes {
        let (median, p95, mesh) = time_builds(world, style, size, runs);
        let label = if size == 1 << 16 { "whole".to_string() } else { format!("r{size}") };
        println!(
            "m surface.build.{name}.{label} tiles={} known={known} regions={} land={} water={} vertices={} runs={runs} median_ms={median:.2} p95_ms={p95:.2}",
            world.terrain.len(),
            mesh.regions.len(),
            mesh.land_triangles.len(),
            mesh.water_triangles.len(),
            mesh.vertices.len(),
        );
    }
    let all = profiles(world, style, profile_size, runs);
    let step = |f: &dyn Fn(&BuildProfile) -> f64| stats(all.iter().map(f).collect());
    let (observation, observation95) = step(&|p| p.observation_ms);
    let (plan, plan95) = step(&|p| p.seam_plan_ms);
    let (assembly, assembly95) = step(&|p| p.assembly_ms);
    let (regions_sum, regions_sum95) = step(&|p| p.regions.iter().map(|r| r.ms).sum());
    println!(
        "m surface.profile.{name}.r{profile_size} threads=1 runs={runs} observation_ms={observation:.2}/{observation95:.2} seam_plan_ms={plan:.2}/{plan95:.2} regions_sum_ms={regions_sum:.2}/{regions_sum95:.2} assembly_ms={assembly:.2}/{assembly95:.2}"
    );
    // Per region: median over runs of each region, then min / median / max over regions.
    let count = all[0].regions.len();
    let mut per_region: Vec<(f64, usize)> = (0..count)
        .map(|i| (stats(all.iter().map(|p| p.regions[i].ms).collect()).0, all[0].regions[i].land_triangles))
        .collect();
    per_region.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (fast, mid, slow) = (per_region[0], per_region[count / 2], per_region[count - 1]);
    let mut triangles: Vec<usize> = per_region.iter().map(|r| r.1).collect();
    triangles.sort();
    println!(
        "m surface.region.{name}.r{profile_size} regions={count} region_ms_min={:.2} region_ms_median={:.2} region_ms_max={:.2} land_min={} land_median={} land_max={}",
        fast.0,
        mid.0,
        slow.0,
        triangles[0],
        triangles[count / 2],
        triangles[count - 1],
    );
}

fn main() {
    let runs: usize = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(15).max(15);
    println!("# aurelia-bench v1 surface ({} threads available)", std::thread::available_parallelism().map_or(1, |n| n.get()));
    let style = compile(default_style()).unwrap();
    report("island", &fixtures::island(), &style, runs);

    let rules = compile_rules(classic_content::core::core_rules()).unwrap();
    let game = generate(&ScenarioConfig::representative_late_game(), &rules);
    let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    world.visual_surface = true;
    report("rep", &world, &style, runs);
    world.visible.fill(1);
    world.explored.fill(1);
    // The projection carries true terrain; knowing every cell is the worst case.
    report("rep_revealed", &world, &style, runs);
}
