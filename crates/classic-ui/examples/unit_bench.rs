//! Unit rendering cost: frame time with 0 / 100 / 500 / 1000 visible armies, plus the
//! triangle totals of every unit formation.
//!
//!     cargo run --release --offline -p aurelia-render --example unit_bench
//!
//! Method: release build, 1920x1080 offscreen target, flat fully visible 48x40 map, one
//! unit per tile in a central block cycling through land military classes. Per case
//! 10 warm-up frames, then 30 timed frames; a frame is `Renderer::draw` plus a blocking
//! device poll (CPU submit + GPU done). Reports median and p95. Two cameras: the game
//! zoom (2.25, units mostly off-screen and culled by 8x8-tile cells) and a
//! zoom that fits the whole block (LOD chosen by projected size).

use std::time::Instant;

use classic_rules::ids::TileId;
use render::camera::CameraController;
use render::gpu::context::GpuCore;
use render::renderer::Renderer;
use render::unit_models::{self, UnitModelBook};
use render::world::state::{RenderWorld, TerrainCell, UnitMarker};
use render::FrameFlags;

const WIDTH: u32 = 48;
const HEIGHT: u32 = 40;
const SIZE: (u32, u32) = (1920, 1080);
const WARMUP: usize = 10;
const RUNS: usize = 30;
const ARMIES: [&str; 12] = [
    "core:unit-class/warrior",
    "core:unit-class/spearman",
    "core:unit-class/archer",
    "core:unit-class/swordsman",
    "core:unit-class/horseman",
    "core:unit-class/knight",
    "core:unit-class/pikeman",
    "core:unit-class/musketman",
    "core:unit-class/cavalry",
    "core:unit-class/rifleman",
    "core:unit-class/infantry",
    "core:unit-class/tank",
];

fn main() {
    formation_table();
    let core = match GpuCore::new_blocking() {
        Ok(core) => core,
        Err(e) => {
            println!("SKIP-GPU: {e}");
            return;
        }
    };
    println!("adapter {:?}", core.adapter_info().name);
    let layers: Vec<(u32, u32, Vec<u8>)> = render::ground::layers();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = Renderer::with_gpu(&core, format, &layers);
    renderer.set_unit_models(UnitModelBook::from_keys(ARMIES));
    renderer.set_animation_time_override(Some(1.0));
    let target = core.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("unit bench target"),
        size: wgpu::Extent3d {
            width: SIZE.0,
            height: SIZE.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    println!("units camera   median_ms p95_ms  unit_tris unit_draws figures");
    for units in [0usize, 100, 500, 1000] {
        let world = world_with(units);
        for (name, camera) in cameras(units) {
            let (samples, stats) = run(&core, &mut renderer, &world, &camera, &view);
            let (median, p95) = median_p95(samples);
            println!(
                "{units:>5} {name:<6} {median:>9.2} {p95:>7.2} {:>10} {:>10} {:>7}",
                stats.unit_tris, stats.unit_draws, stats.unit_instances
            );
        }
    }
}

/// Triangles per formation (whole unit) at each LOD; the marker is drawn once per unit.
fn formation_table() {
    println!("model                         figures  LOD0/fig  formation LOD0  LOD1  marker");
    for model in unit_models::models() {
        let tris = |lod: usize| model.lods[lod].1 as usize / 3;
        let figures = model.slots.len();
        println!(
            "{:<30} {figures:>6} {:>9} {:>15} {:>5} {:>7}",
            model.id,
            tris(0),
            tris(0) * figures,
            tris(1) * figures,
            tris(2)
        );
    }
}

fn world_with(units: usize) -> RenderWorld {
    let n = (WIDTH * HEIGHT) as usize;
    let mut world = RenderWorld {
        width: WIDTH,
        height: HEIGHT,
        visible: vec![1; n],
        explored: vec![1; n],
        improvements: vec![None; n],
        road: vec![0; n],
        river: vec![0; n],
        forest: vec![0; n],
        // Same ground in every case: only the unit layer changes between worlds.
        layers: render::world::state::LayerRevisions {
            units: units as u64 + 1,
            ..render::world::state::LayerRevisions::INITIAL
        },
        ..Default::default()
    };
    for tile in 0..n as u32 {
        world.terrain.push(TerrainCell {
            tile: TileId(tile),
            terrain: 0,
            owner: None,
            resource: None,
        });
    }
    let side = (units as f32).sqrt().ceil() as u32;
    let (q0, r0) = ((WIDTH - side) / 2, (HEIGHT - side) / 2);
    for i in 0..units as u32 {
        let (q, r) = (q0 + i % side.max(1), r0 + i / side.max(1));
        world.units.push(UnitMarker {
            id: 0, embarked: false, fortified: false, tile: TileId(r * WIDTH + q),
            owner: (i % 4) as u8,
            kind: (i as usize % ARMIES.len()) as u16,
            hp: 100,
        });
    }
    world
}

fn cameras(units: usize) -> Vec<(&'static str, CameraController)> {
    let mut game = CameraController::fit_map(WIDTH, HEIGHT);
    game.zoom = 2.25;
    let mut fit = CameraController::fit_map(WIDTH, HEIGHT);
    let side = (units.max(1) as f32).sqrt().ceil();
    // Distance 42 / zoom shows about 0.77 x distance hex units vertically at 45 degrees.
    fit.zoom = (42.0 * 0.77 / (side * 1.6 + 4.0)).clamp(0.15, 8.0);
    vec![("game", game), ("fit", fit)]
}

fn run(
    core: &GpuCore,
    renderer: &mut Renderer,
    world: &RenderWorld,
    camera: &CameraController,
    view: &wgpu::TextureView,
) -> (Vec<f64>, render::RenderStats) {
    let mut samples = Vec::with_capacity(RUNS);
    let mut stats = render::RenderStats::default();
    for frame in 0..WARMUP + RUNS {
        let start = Instant::now();
        stats = renderer.draw(core, world, camera, view, SIZE, FrameFlags::default());
        let _ = core.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        if frame >= WARMUP {
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    (samples, stats)
}

fn median_p95(mut samples: Vec<f64>) -> (f64, f64) {
    samples.sort_by(f64::total_cmp);
    let median = samples[samples.len() / 2];
    let p95 = samples[(samples.len() - 1) * 95 / 100];
    (median, p95)
}
