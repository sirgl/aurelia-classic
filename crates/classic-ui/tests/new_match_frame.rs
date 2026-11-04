//! A new classic match started over an old one in the same window shows nothing of the old
//! one (decision D330, the same rule as Lands & Laws': `lal-ui/tests/new_match_frame.rs`).
//! The first frame of a match started after another — a fresh one, a played one — is the
//! first frame of a clean run: the same ground, minimap, units and panels.

use client::shell::app::App;
use client::shell::capture::{capture_pixels, capture_texture, wait_idle};
use client::shell::module::{AiProgress, ModuleUi};
use classic_ui::ui::screens::{prepare_match, MapSize, MatchRequest};
use classic_ui::ClassicUi;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: (u32, u32) = (1280, 720);
/// Pixels of a frame that may differ between two runs of the same frame: 0.1 % of the window.
const JITTER: usize = (SIZE.0 * SIZE.1 / 1000) as usize;

struct Window {
    core: render::gpu::context::GpuCore,
    texture: render::gpu::memory::GpuTexture,
    target: wgpu::TextureView,
    clock: std::cell::Cell<f64>,
}

impl Window {
    fn open() -> Option<Window> {
        let core = match client::shell::gpu::open() {
            Ok(core) => core,
            Err(error) => {
                eprintln!("skipped: no GPU ({error})");
                return None;
            }
        };
        let texture = capture_texture(&core, SIZE.0, SIZE.1, FORMAT);
        let target = texture.create_view(&Default::default());
        Some(Window { core, texture, target, clock: Default::default() })
    }

    fn app(&self) -> App<ClassicUi> {
        let mut app = App::new(&self.core, FORMAT, None, ClassicUi::game().without_end_turn_gate(), &classic_ui::terrain_layers());
        app.screens.settings.autosave_every = 0;
        app.fixed_animation_time = Some(1.0);
        app.presentation_step = Some(1.0 / 60.0);
        app.renderer.set_terrain_sync(render::gpu::terrain_gpu::TerrainSync::Immediate);
        // The menu is shown first, as in a window: the interface scale settles.
        self.frames(&mut app, None);
        app
    }

    fn frames(&self, app: &mut App<ClassicUi>, pointer: Option<egui::Pos2>) -> Vec<u8> {
        for i in 0..8 {
            self.clock.set(self.clock.get() + 1.0 / 60.0);
            let screen = egui::vec2(SIZE.0 as f32, SIZE.1 as f32) / app.ctx.pixels_per_point();
            let events = match (i, pointer) {
                (0, Some(at)) => vec![egui::Event::PointerMoved(at)],
                (0, None) => vec![egui::Event::PointerGone],
                _ => Vec::new(),
            };
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
                time: Some(self.clock.get()),
                focused: true,
                events,
                ..Default::default()
            };
            app.frame(&self.core, &self.target, SIZE, raw);
            wait_idle(&self.core);
        }
        capture_pixels(&self.core, &self.texture, SIZE.0, SIZE.1)
    }

    fn start(&self, app: &mut App<ClassicUi>, seed: u64) {
        let request = MatchRequest { nation: "core:nation/rome".into(), seed, size: MapSize::Small, players: 3, water: 25 };
        app.module.start(prepare_match(request).expect("a world"));
        app.apply_module_actions();
    }

    fn first_frame(&self, app: &mut App<ClassicUi>, seed: u64) -> Vec<u8> {
        self.start(app, seed);
        self.frames(app, None)
    }

    fn play(&self, app: &mut App<ClassicUi>, seed: u64, turns: u32) {
        self.start(app, seed);
        for _ in 0..turns {
            self.frames(app, Some(egui::pos2(640., 360.)));
            app.module.begin_end_turn();
            while app.module.tick_ai(std::time::Duration::from_secs(60)) == AiProgress::Busy {}
            app.apply_module_actions();
        }
        self.frames(app, Some(egui::pos2(640., 360.)));
    }
}

/// The order panel's corner, whose action icons are decoded in the background and drawn when
/// ready (a loaded asset, not a match's state): the compared frames leave it out.
fn in_order_panel(pixel: usize) -> bool {
    let (x, y) = (pixel % SIZE.0 as usize, pixel / SIZE.0 as usize);
    x < 360 && y > 470
}

fn differing(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(a.len(), b.len(), "frames of one size");
    let pairs = a.chunks(4).zip(b.chunks(4)).enumerate().filter(|(i, _)| !in_order_panel(*i));
    pairs.filter(|(_, (x, y))| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > 8)).count()
}

#[test]
fn a_new_match_over_another_starts_clean() {
    let Some(window) = Window::open() else { return };
    let clean = window.first_frame(&mut window.app(), 11);
    let mut app = window.app();
    window.first_frame(&mut app, 12);
    let after_fresh = window.first_frame(&mut app, 11);
    window.play(&mut app, 12, 4);
    let after_played = window.first_frame(&mut app, 11);
    for (name, after) in [("fresh match of another seed", after_fresh), ("played match of another seed", after_played)] {
        let n = differing(&clean, &after);
        assert!(n <= JITTER, "{name}: the first frame of the new match differs from a clean run's in {n} pixels (allowed {JITTER})");
    }
}
