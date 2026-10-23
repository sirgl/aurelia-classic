//! Filmstrips of the air war through the real presentation: take-off, cruise, turn, strike,
//! landing, interception, a nuclear launch and blast, a paradrop, an airlift. The cues go
//! through `Presentation` (the same code the game runs), the poses and effects to the
//! renderer, the frames are tiled into one PNG. Not a test: the artefact is looked at.
//!
//!     flock $GPU_LOCK \
//!       cargo run --release -p classic-ui --example air_sheet -- <out.png> <scenario> [model-key] [frames] [reduced]
//!
//! `AIR_ZOOM=<z>` overrides the camera zoom (1 = the whole map), `AIR_FOLLOW=<z>` follows the first pose.
//!
//! Scenarios: parked, rebase, strike, intercept, missile, nuke, paradrop, airlift, heli,
//! bench (30 aircraft in flight with effects: prints the frame time).

use client::shell::presentation::cue::{Attack, AttackStyle, Cue, Derived, Figure, Origin};
use client::shell::presentation::{Presentation, Settings, Speed};
use kernel::ids::{EntityId, TileId};
use render::camera::CameraController;
use render::gpu::context::GpuCore;
use render::gpu::effects::EffectsMode;
use render::gpu::flight::FlightClass;
use render::renderer::Renderer;
use render::unit_models::{UnitModelBook, Weapon};
use render::world::state::{CityMarker, LayerRevisions, RenderWorld, TerrainCell, UnitMarker};
use render::FrameFlags;

const W: u32 = 16;
const H: u32 = 10;
const CELL: (u32, u32) = (640, 380);

/// Kind indices of the book.
const KEYS: [&str; 14] = [
    "core:unit/fighter",
    "core:unit/bomber",
    "core:unit/jet-fighter",
    "core:unit/stealth-bomber",
    "core:unit/great-war-bomber",
    "core:unit/triplane",
    "core:unit/helicopter-gunship",
    "core:unit/guided-missile",
    "core:unit/atomic-bomb",
    "core:unit/nuclear-missile",
    "core:unit/paratrooper",
    "core:unit/marine",
    "core:unit/legion",
    "core:unit/carrier",
];

fn kind_of(key: &str) -> u16 {
    KEYS.iter().position(|k| *k == key || k.ends_with(key)).unwrap_or_else(|| panic!("unknown model {key}")) as u16
}

fn tile(q: u32, r: u32) -> TileId {
    TileId(r * W + q)
}

fn world(units: &[(u64, TileId, u16, u8)], cities: &[TileId]) -> RenderWorld {
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
    for t in 0..n as u32 {
        world.terrain.push(TerrainCell { tile: TileId(t), terrain: 0, owner: None, resource: None });
    }
    for (id, at, kind, owner) in units {
        world.units.push(UnitMarker { id: *id, embarked: false, fortified: false, tile: *at, owner: *owner, kind: *kind, hp: 100 });
    }
    for at in cities {
        world.cities.push(CityMarker {
            tile: *at,
            owner: 1,
            name: "Base".into(),
            population: Some(6),
            buildings: Vec::new(),
            era: 5,
            showpieces: Vec::new(),
            unworked: Vec::new(),
        });
    }
    world
}

fn figure(id: u64, kind: u16, owner: u8) -> Figure {
    Figure { id: EntityId(id), kind, owner }
}

fn derived(cue: Cue) -> Derived {
    Derived { cue, origin: Origin::Own, concerns_observer: true }
}

fn attack(kind: u16, weapon: Weapon, from: TileId, target: TileId, taken: Option<u16>, kill: bool) -> Attack {
    Attack {
        attacker: figure(1, kind, 1),
        from,
        target,
        defender: Some(figure(2, kind_of("legion"), 2)),
        style: AttackStyle::Air,
        weapon,
        figures: 1,
        reach: 7,
        defender_hp: None,
        attacker_hp: None,
        dealt: Some(24),
        taken,
        kill,
        attacker_seen: true,
        target_owner: 2,
    }
}

fn weapon_of(kind: u16) -> Weapon {
    let id = render::unit_models::resolve(KEYS[kind as usize]).expect("model");
    id.model().weapon
}

struct Scene {
    cues: Vec<Cue>,
    /// The world after the commands (units where the journal left them).
    world: RenderWorld,
    /// Camera centre tile and zoom.
    look: (TileId, f32),
}

fn scene(name: &str, key: &str) -> Scene {
    let a = tile(2, 5);
    let b = tile(11, 5);
    let kind = kind_of(key);
    let mid = tile(7, 5);
    match name {
        "rebase" => Scene {
            cues: vec![Cue::Flight { unit: figure(1, kind, 1), from: a, to: b, reach: 9, class: FlightClass::Plane }],
            world: world(&[(1, b, kind, 1)], &[a, b]),
            look: (mid, 6.5),
        },
        "strike" | "intercept" => {
            let taken = (name == "intercept").then_some(31);
            let at = attack(kind, weapon_of(kind), a, tile(9, 5), taken, false);
            Scene {
                cues: vec![Cue::Attack(at)],
                world: world(&[(1, a, kind, 1), (2, tile(9, 5), kind_of("legion"), 2)], &[a]),
                look: (tile(6, 5), 7.0),
            }
        }
        "missile" => {
            let at = attack(kind, Weapon::Missile, a, tile(9, 5), None, true);
            Scene {
                cues: vec![Cue::Attack(at)],
                world: world(&[(2, tile(9, 5), kind_of("legion"), 2)], &[a]),
                look: (tile(6, 5), 7.0),
            }
        }
        "nuke" => {
            let target = tile(11, 5);
            let blast = render_blast(target);
            Scene {
                cues: vec![Cue::Nuke { unit: figure(1, kind, 1), from: a, target, reach: 9, blast }],
                world: world(&[], &[a]),
                look: (tile(10, 5), 4.6),
            }
        }
        "paradrop" => Scene {
            cues: vec![Cue::Paradrop { unit: figure(1, kind_of("paratrooper"), 1), from: a, to: tile(9, 5), reach: 7, plane: kind_of("bomber") }],
            world: world(&[(1, tile(9, 5), kind_of("paratrooper"), 1)], &[a]),
            look: (tile(6, 5), 7.0),
        },
        "airlift" => Scene {
            cues: vec![Cue::Airlift { unit: figure(1, kind_of("marine"), 1), from: a, to: b, reach: 9, plane: kind_of("bomber") }],
            world: world(&[(1, b, kind_of("marine"), 1)], &[a, b]),
            look: (mid, 6.5),
        },
        "heli" => {
            let path: Vec<TileId> = (2..8u32).map(|q| tile(q, 5)).collect();
            Scene {
                cues: vec![Cue::Move { unit: figure(1, kind_of("helicopter-gunship"), 1), path }],
                world: world(&[(1, tile(7, 5), kind_of("helicopter-gunship"), 1), (2, tile(3, 6), kind_of("legion"), 1)], &[]),
                look: (tile(5, 5), 6.0),
            }
        }
        "carrier" => {
            let deck = tile(7, 5);
            let mut w = world(
                &[(1, tile(3, 5), kind, 1), (3, deck, kind_of("bomber"), 1), (4, deck, kind_of("carrier"), 1), (5, deck, kind_of("fighter"), 1)],
                &[tile(3, 5)],
            );
            let water = w.water_code();
            for r in 3..8u32 {
                for q in 5..10u32 {
                    w.terrain[(r * W + q) as usize].terrain = water;
                }
            }
            Scene {
                cues: vec![Cue::Flight { unit: figure(1, kind, 1), from: deck, to: tile(3, 5), reach: 4, class: FlightClass::Plane }],
                world: w,
                look: (tile(5, 5), 7.0),
            }
        }
        "parked" => {
            let mut units = Vec::new();
            let city = tile(8, 5);
            for (i, k) in KEYS.iter().enumerate().take(10) {
                units.push((i as u64 + 1, tile(4 + (i as u32 % 5) * 2, 3 + (i as u32 / 5) * 3), kind_of(k), 1));
            }
            units.push((20, tile(3, 7), kind_of("legion"), 1));
            units.push((21, tile(5, 7), kind_of("marine"), 1));
            Scene { cues: vec![], world: world(&units, &[city]), look: (tile(8, 5), 5.5) }
        }
        other => panic!("unknown scenario {other}"),
    }
}

fn render_blast(target: TileId) -> Vec<TileId> {
    let (q, r) = ((target.0 % W) as i32, (target.0 / W) as i32);
    let mut out = Vec::new();
    for dr in -2i32..=2 {
        for dq in -2i32..=2 {
            if (dq + dr).abs() <= 2 {
                let (nq, nr) = (q + dq, r + dr);
                if (0..W as i32).contains(&nq) && (0..H as i32).contains(&nr) {
                    out.push(tile(nq as u32, nr as u32));
                }
            }
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or("/tmp/air_sheet.png".into());
    let name = args.get(2).cloned().unwrap_or("rebase".into());
    let key = args.get(3).cloned().unwrap_or("fighter".into());
    let frames: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(8);
    let reduced = args.get(5).is_some_and(|s| s == "reduced");
    let core = GpuCore::new_blocking().expect("needs a GPU adapter");
    let layers = render::ground::layers();
    let mut renderer = Renderer::with_gpu(&core, wgpu::TextureFormat::Rgba8Unorm, &layers);
    renderer.set_animation_time_override(Some(1.0));
    renderer.set_render_scale(1.0);
    renderer.set_unit_models(UnitModelBook::from_keys(KEYS));
    if reduced {
        renderer.effects().set_mode(EffectsMode::Reduced);
    }
    if name == "bench" {
        bench(&core, &mut renderer);
        return;
    }
    let scene = scene(&name, &key);
    let mut presentation = Presentation::default();
    presentation.reset(W);
    presentation.set_settings(Settings { speed: Speed::Normal, reduced_motion: reduced, camera_glides: false });
    let duration: f32 = scene.cues.iter().map(|c| c.duration()).fold(0.0, f32::max);
    presentation.push(scene.cues.iter().cloned().map(derived).collect());
    let (cx, cz) = {
        let t = scene.look.0;
        render::scene::layout::tile_to_world(t.0 % W, t.0 / W)
    };
    let mut camera = CameraController::fit_map(W, H);
    camera.x = cx;
    camera.y = cz;
    camera.zoom = std::env::var("AIR_ZOOM").ok().and_then(|z| z.parse::<f32>().ok()).unwrap_or(scene.look.1);
    let dt = 1.0 / 30.0;
    let span = if name == "parked" { 0.0 } else { duration + 0.9 };
    let steps = (span / dt) as usize + 1;
    let mut shots: Vec<Vec<u8>> = Vec::new();
    let mut next_shot = 0usize;
    for step in 0..steps.max(frames) {
        let frame = presentation.advance(dt, &scene.world);
        renderer.set_unit_poses(&frame.poses);
        for fx in &frame.fx {
            renderer.play_fx(fx);
        }
        renderer.advance_effects(dt);
        // `AIR_FOLLOW=zoom`: the camera follows the first pose (a close-up of the aircraft).
        if let Some(zoom) = std::env::var("AIR_FOLLOW").ok().and_then(|z| z.parse::<f32>().ok()) {
            if let Some(pose) = frame.poses.iter().find(|p| p.id == 1).or(frame.poses.first()) {
                camera.x = pose.position[0];
                camera.y = pose.position[1];
                camera.zoom = zoom;
            }
        }
        // Capture `frames` evenly spaced frames of the whole span.
        let due = if frames <= 1 { 0 } else { next_shot * steps.saturating_sub(1) / (frames - 1) };
        if next_shot < frames && step >= due {
            let pixels = renderer.render_offscreen(&core, &scene.world, &camera, CELL.0, CELL.1, FrameFlags { grid: false, selected_city: None, yields: false });
            shots.push(pixels);
            next_shot += 1;
        }
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
    eprintln!("wrote {out} ({w}x{h}); cue {duration:.2}s over {steps} steps; live particles {}", renderer.effects().live());
}

/// 30 aircraft in flight with trails, bombs and one blast: the frame time at the target size.
fn bench(core: &GpuCore, renderer: &mut Renderer) {
    let mut units = Vec::new();
    let mut cues = Vec::new();
    let a = tile(1, 1);
    for i in 0..30u64 {
        let kind = (i % 7) as u16;
        let from = tile(1 + (i as u32 % 6), 1 + (i as u32 / 6) * 2);
        let to = tile(9 + (i as u32 % 6), 1 + (i as u32 / 6) * 2);
        if i % 3 == 0 {
            let mut at = attack(kind, weapon_of(kind), from, to, None, false);
            at.attacker = figure(100 + i, kind, 1);
            at.defender = Some(figure(200 + i, kind_of("legion"), 2));
            at.reach = 7;
            units.push((200 + i, to, kind_of("legion"), 2));
            cues.push(Cue::Attack(at));
        } else {
            cues.push(Cue::Flight { unit: figure(100 + i, kind, 1), from, to, reach: 8, class: FlightClass::Plane });
            units.push((100 + i, to, kind, 1));
        }
    }
    cues.push(Cue::Nuke { unit: figure(999, kind_of("nuclear-missile"), 1), from: a, target: tile(12, 8), reach: 11, blast: render_blast(tile(12, 8)) });
    let world = world(&units, &[a]);
    let mut presentation = Presentation::default();
    presentation.reset(W);
    presentation.set_settings(Settings { speed: Speed::Normal, reduced_motion: false, camera_glides: false });
    presentation.push(cues.into_iter().map(derived).collect());
    let mut camera = CameraController::fit_map(W, H);
    camera.zoom = 6.0;
    let dt = 1.0 / 30.0;
    let (w, h) = std::env::var("AURELIA_VISUAL_SIZE").ok().and_then(|s| s.split_once('x').map(|(a, b)| (a.parse().unwrap_or(1440), b.parse().unwrap_or(900)))).unwrap_or((1440u32, 900u32));
    let mut times = Vec::new();
    let mut peak = 0usize;
    for step in 0..150 {
        let frame = presentation.advance(dt, &world);
        renderer.set_unit_poses(&frame.poses);
        for fx in &frame.fx {
            renderer.play_fx(fx);
        }
        renderer.advance_effects(dt);
        let start = std::time::Instant::now();
        let _ = renderer.render_offscreen(core, &world, &camera, w, h, FrameFlags { grid: false, selected_city: None, yields: false });
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if step >= 5 {
            times.push(ms);
        }
        peak = peak.max(renderer.effects().live());
        if step % 30 == 0 {
            eprintln!("step {step}: poses {} live particles {} frame {ms:.1} ms", frame.poses.len(), renderer.effects().live());
        }
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    let p95 = times[times.len() * 95 / 100];
    eprintln!("bench {w}x{h}: median {median:.1} ms, p95 {p95:.1} ms, peak particles {peak}");
}
