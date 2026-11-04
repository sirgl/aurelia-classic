//! Animation filmstrips: renders a unit (or the effects) at a sequence of times and tiles
//! the frames into one PNG, for looking at walk cycles, attacks and explosions without a
//! window.
//!
//!     flock $GPU_LOCK \
//!       cargo run --release -p aurelia-render --example anim_sheet -- <out.png> <scenario> [model-key] [frames]
//!
//! Scenarios: `effects` (an explosion, a projectile, a muzzle flash), later ones below.
//! Not a test: the artefact is looked at.

use classic_rules::ids::TileId;
use render::camera::CameraController;
use render::gpu::context::GpuCore;
use render::gpu::effects::{ExplosionKind, FxEvent, MuzzleKind, ProjectileKind};
use render::renderer::Renderer;
use render::unit_models::UnitModelBook;
use render::world::state::{LayerRevisions, RenderWorld, TerrainCell, UnitMarker};
use render::FrameFlags;

const SIDE: u32 = 12;
const CELL: (u32, u32) = (448, 300);

fn flat_world(units: &[(u32, u16)]) -> RenderWorld {
    let n = (SIDE * SIDE) as usize;
    let mut world = RenderWorld {
        width: SIDE,
        height: SIDE,
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
    for (i, (tile, kind)) in units.iter().enumerate() {
        world.units.push(UnitMarker { id: i as u64 + 1, embarked: false, fortified: false, tile: TileId(*tile), owner: 1, kind: *kind, hp: 100 });
    }
    world
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or("/tmp/anim_sheet.png".into());
    let scenario = args.get(2).cloned().unwrap_or("effects".into());
    let frames: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(8);
    let core = GpuCore::new_blocking().expect("needs a GPU adapter");
    let layers = render::ground::layers();
    let mut renderer = Renderer::with_gpu(&core, wgpu::TextureFormat::Rgba8Unorm, &layers);
    renderer.set_animation_time_override(Some(1.0));
    renderer.set_render_scale(1.0);
    renderer.set_unit_models(UnitModelBook::from_keys(["core:unit/warrior"]));
    let world = flat_world(&[]);
    let (cx, cz) = render::scene::layout::tile_to_world(6, 6);
    let mut camera = CameraController::fit_map(SIDE, SIDE);
    camera.x = cx;
    camera.y = cz;
    camera.zoom = 8.0;
    let dt = 0.12;
    let mut shots: Vec<Vec<u8>> = Vec::new();
    for frame in 0..frames {
        if frame == 0 {
            match scenario.as_str() {
                "effects" => {
                    renderer.play_fx(&FxEvent::Muzzle { at: [cx - 1.2, cz], toward: [cx + 0.8, cz], kind: MuzzleKind::Cannon });
                    renderer.play_fx(&FxEvent::Projectile { from: [cx - 1.2, cz], to: [cx + 0.8, cz], kind: ProjectileKind::Shell, seconds: 0.36 });
                    renderer.play_fx(&FxEvent::Projectile { from: [cx - 1.2, cz + 0.4], to: [cx + 0.8, cz + 0.4], kind: ProjectileKind::Arrow, seconds: 0.36 });
                    renderer.play_fx(&FxEvent::Smoke { at: [cx + 2.0, cz - 1.0], size: 0.4, seconds: 1.5 });
                    renderer.play_fx(&FxEvent::Fire { at: [cx + 2.0, cz + 1.0], size: 0.4, seconds: 1.5 });
                }
                _ => {}
            }
        }
        if frame == 3 && scenario == "effects" {
            renderer.play_fx(&FxEvent::Explosion { at: [cx + 0.8, cz], size: 0.5, kind: ExplosionKind::Shell });
        }
        let pixels = renderer.render_offscreen(&core, &world, &camera, CELL.0, CELL.1, FrameFlags { grid: false, selected_city: None, yields: false });
        shots.push(pixels);
        renderer.advance_effects(dt);
    }
    let cols = 4usize.min(frames);
    let rows = frames.div_ceil(cols);
    let (w, h) = (cols as u32 * CELL.0, rows as u32 * CELL.1);
    let mut sheet = vec![0u8; (w * h * 4) as usize];
    for (i, shot) in shots.iter().enumerate() {
        let (col, row) = ((i % cols) as u32, (i / cols) as u32);
        for y in 0..CELL.1 {
            let src = (y * CELL.0 * 4) as usize;
            let dst = (((row * CELL.1 + y) * w + col * CELL.0) * 4) as usize;
            sheet[dst..dst + (CELL.0 * 4) as usize].copy_from_slice(&shot[src..src + (CELL.0 * 4) as usize]);
        }
    }
    image::save_buffer(&out, &sheet, w, h, image::ColorType::Rgba8).unwrap();
    eprintln!("wrote {out} ({w}x{h}), stats {:?}", renderer.stats().effect_particles);
}
