//! Cost of the unit animation and the effects: frame time with 50 units resting, walking
//! (every figure posed, limbs deformed in the vertex shader), attacking, and with combat
//! effects (explosions, projectiles, smoke) alive.
//!
//!     flock $GPU_LOCK target/release/examples/anim_bench [WxH]
//!
//! Method: release build, flat 48x40 map fully visible, 50 land units in a block cycling
//! the classic military classes, game camera. Per case 10 warm-up and 40 timed frames
//! (`Renderer::draw` plus a blocking device poll); median and p95.

use std::time::Instant;

use classic_rules::ids::TileId;
use render::camera::CameraController;
use render::gpu::context::GpuCore;
use render::gpu::effects::{AirFx, BLAST_RADIUS, ExplosionKind, FxEvent, MuzzleKind, ProjectileKind};
use render::gpu::unit_poses::{Gait, UnitPose};
use render::renderer::Renderer;
use render::unit_models::UnitModelBook;
use render::world::state::{LayerRevisions, RenderWorld, TerrainCell, UnitMarker};
use render::FrameFlags;

const W: u32 = 48;
const H: u32 = 40;
const UNITS: usize = 50;
const KEYS: [&str; 8] = [
    "core:unit-class/archer",
    "core:unit-class/spearman",
    "core:unit-class/horseman",
    "core:unit-class/musketman",
    "core:unit-class/rifleman",
    "core:unit-class/tank",
    "core:unit-class/cannon",
    "core:unit-class/knight",
];

fn world() -> RenderWorld {
    let n = (W * H) as usize;
    let mut world = RenderWorld {
        width: W,
        height: H,
        visible: vec![1; n],
        explored: vec![1; n],
        improvements: vec![None; n],
        road: vec![0; n],
        river: vec![0; n],
        forest: vec![0; n],
        layers: LayerRevisions { units: 1, ..LayerRevisions::INITIAL },
        ..Default::default()
    };
    for tile in 0..n as u32 {
        world.terrain.push(TerrainCell { tile: TileId(tile), terrain: 0, owner: None, resource: None });
    }
    for i in 0..UNITS as u32 {
        let (q, r) = (20 + i % 8, 17 + i / 8);
        world.units.push(UnitMarker { id: i as u64 + 1, embarked: false, fortified: false, tile: TileId(r * W + q), owner: (i % 3) as u8, kind: (i as usize % KEYS.len()) as u16, hp: 100 - (i % 4) as u8 * 20 });
    }
    world
}

fn poses(world: &RenderWorld, gait: impl Fn(usize) -> Gait) -> Vec<UnitPose> {
    world
        .units
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let (x, z) = render::scene::layout::tile_to_world(u.tile.0 % W, u.tile.0 / W);
            UnitPose { id: u.id, kind: u.kind, owner: u.owner, tile: u.tile.0, position: [x, z], facing: 0.4, gait: gait(i), flash: 0.0, settle: 0.0, hp: [u.hp, u.hp], change: 0.0 }
        })
        .collect()
}

fn main() {
    let size = std::env::args()
        .nth(1)
        .and_then(|s| s.split_once('x').map(|(w, h)| (w.parse().unwrap_or(1920), h.parse().unwrap_or(1080))))
        .unwrap_or((1920u32, 1080u32));
    let core = GpuCore::new_blocking().expect("needs a GPU adapter");
    println!("adapter {:?} size {size:?}", core.adapter_info().name);
    let layers = render::ground::layers();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = Renderer::with_gpu(&core, format, &layers);
    renderer.set_unit_models(UnitModelBook::from_keys(KEYS));
    renderer.set_animation_time_override(Some(1.0));
    let target = core.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("anim bench target"),
        size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let world = world();
    let mut camera = CameraController::fit_map(W, H);
    let (cx, cz) = render::scene::layout::tile_to_world(24, 20);
    camera.x = cx;
    camera.y = cz;
    camera.zoom = 2.6;
    let flags = FrameFlags { grid: false, selected_city: None, yields: false };
    println!("case                         median_ms  p95_ms  posed_figs particles unit_tris");
    for case in ["resting", "posed-standing", "walking-4", "walking", "attacking", "attacking+effects", "resting+effects", "nuke-flash", "nuke-cloud", "nuke-fallout", "launches"] {
        let mut samples = Vec::new();
        let mut last = Default::default();
        let mut t = 0.0f32;
        if case.starts_with("nuke") {
            // One nuclear strike on the block; the cases look at the flash and fireball
            // (the first second), the cloud and rings (1.2 s on) and the lingering fallout.
            renderer.effects().clear();
            let (x, z) = render::scene::layout::tile_to_world(24, 20);
            renderer.play_fx(&FxEvent::Air(AirFx::Launch { at: [x - 6.0, z], heavy: true }));
            renderer.play_fx(&FxEvent::Air(AirFx::Blast { at: [x, z], radius: BLAST_RADIUS }));
            let skip = match case {
                "nuke-cloud" => 1.2,
                "nuke-fallout" => 3.5,
                _ => 0.0,
            };
            if skip > 0.0 {
                renderer.advance_effects(skip);
            }
        }
        for frame in 0..50 {
            t += 1.0 / 60.0;
            match case {
                "posed-standing" => renderer.set_unit_poses(&poses(&world, |_| Gait::Stand)),
                "walking-4" => renderer.set_unit_poses(&poses(&world, |i| Gait::Walk { phase: t * 4.0 + i as f32 * 0.3 })[..4]),
                "walking" => renderer.set_unit_poses(&poses(&world, |i| Gait::Walk { phase: t * 4.0 + i as f32 * 0.3 })),
                "attacking" | "attacking+effects" => renderer.set_unit_poses(&poses(&world, |i| Gait::Attack { aim: (t * 1.5).fract().min(1.0), strike: ((t * 1.5 + i as f32 * 0.07).fract() - 0.5).max(0.0) * 2.0 })),
                _ => renderer.set_unit_poses(&[]),
            }
            if case.starts_with("nuke") {
                renderer.advance_effects(1.0 / 60.0);
            }
            if case == "launches" {
                renderer.advance_effects(1.0 / 60.0);
                if frame % 10 == 0 {
                    let (x, z) = render::scene::layout::tile_to_world(21 + (frame as u32 * 7) % 6, 18 + (frame as u32 * 3) % 5);
                    renderer.play_fx(&FxEvent::Air(AirFx::Launch { at: [x, z], heavy: false }));
                }
            }
            if case.ends_with("effects") {
                renderer.advance_effects(1.0 / 60.0);
                // Twenty events a second: shells, shots and blasts around the block.
                if frame % 3 == 0 {
                    let (x, z) = render::scene::layout::tile_to_world(21 + (frame as u32 * 7) % 6, 18 + (frame as u32 * 3) % 5);
                    renderer.play_fx(&FxEvent::Muzzle { at: [x - 1.5, z], toward: [x, z], kind: MuzzleKind::Cannon });
                    renderer.play_fx(&FxEvent::Projectile { from: [x - 1.5, z], to: [x, z], kind: ProjectileKind::Shell, seconds: 0.3 });
                    renderer.play_fx(&FxEvent::Explosion { at: [x, z], size: 0.6, kind: ExplosionKind::Shell });
                }
            }
            let start = Instant::now();
            last = renderer.draw(&core, &world, &camera, &view, size, flags);
            let _ = core.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
            if frame >= 10 {
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "{case:<28} {:>9.2} {:>7.2} {:>11} {:>9} {:>9}",
            samples[samples.len() / 2],
            samples[(samples.len() - 1) * 95 / 100],
            last.posed_figures,
            last.effect_particles,
            last.unit_tris
        );
    }
}
