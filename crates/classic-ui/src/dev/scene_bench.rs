//! `desktop --bench-scenes`: frame time of the real game loop on three scenes — a new
//! game (the empty map), the middle of a six-power game and a late game with the whole map
//! in view — at rest in the game view, at rest zoomed out, and through a scripted session
//! of zooms and pans paced at 60 Hz. Every frame is split into its stages: CPU (module,
//! terrain step, shadows, scene recording, egui) and the GPU (the wait after the CPU is
//! done, and with `AURELIA_GPU_TIMING=1` the passes). Release, offscreen, no window: it
//! measures the frame up to the GPU being idle, not the present.
//!
//! AURELIA_BENCH_SCENES=empty,mid,late,dense (default empty,mid,late; `dense` is the 80x52 map of `--bench-world`: 170 cities, 47 wonders, every era 7), AURELIA_BENCH_FRAMES (steady frames, default
//! 120), AURELIA_BENCH_CYCLES (session cycles, default 3), AURELIA_BENCH_ROUNDS (mid game rounds,
//! default 120), AURELIA_BENCH_PACE=0 (no 60 Hz pacing), AURELIA_BENCH_CALM=1 (clock frozen, no ambient particles: the Low-quality map), AURELIA_BENCH_LOD=1 (terrain levels of detail on; off in the game), AURELIA_BENCH_SHOTS=<dir> (a PNG per scene
//! and zoom), AURELIA_VISUAL_SIZE (default 2880x1800, a MacBook Pro window in pixels).
use std::time::{Duration, Instant};

use super::headless::{env_float, env_number, request, start, turn, FrameSample, Offscreen};
use crate::ui::model::GameRequest;
use crate::ui::screens::{self, MapSize, MatchRequest, NewGame, Screen};

/// What a scene is played at: the game view and the whole map.
struct Scene {
    name: &'static str,
    play: f32,
    wide: f32,
}

pub fn bench_scenes() {
    if std::env::var_os("AURELIA_VISUAL_SIZE").is_none() {
        std::env::set_var("AURELIA_VISUAL_SIZE", "2880x1800");
    }
    let wanted = std::env::var("AURELIA_BENCH_SCENES").unwrap_or_else(|_| "empty,mid,late".into());
    for name in wanted.split(',') {
        let mut run = Offscreen::new();
        if std::env::var("AURELIA_BENCH_LOD").as_deref() == Ok("1") {
            run.app.renderer.set_terrain_lod(true);
        }
        // The map as `Quality::Low` draws it: the clock frozen, no ambient particles.
        if std::env::var("AURELIA_BENCH_CALM").as_deref() == Ok("1") {
            run.app.renderer.set_animation_time_override(Some(0.0));
            run.app.renderer.set_ambient_effects(false);
        }
        let scene = match name {
            "empty" => empty(&mut run),
            "mid" => mid(&mut run),
            "late" => late(&mut run),
            "dense" => dense(&mut run),
            other => {
                eprintln!("unknown scene {other}");
                continue;
            }
        };
        measure(&mut run, &scene);
    }
}

/// A new game: the start units on a fogged map, nothing built.
fn empty(run: &mut Offscreen) -> Scene {
    run.start_match();
    run.app.screens.open(Screen::Map.into());
    wide_scene(run, "empty")
}

/// Six powers, the observer among them, after AURELIA_BENCH_ROUNDS rounds of AI play.
fn mid(run: &mut Offscreen) -> Scene {
    let request_ = MatchRequest {
        nation: "core:nation/rome".into(),
        seed: env_number("AURELIA_GAME_SEED", 20260928),
        size: MapSize::World(crate::ui::screens::WorldSize::Standard),
        players: 6,
        water: 25,
    };
    let prepared = screens::prepare_game(NewGame { request: request_, options: screens::GameOptions::default() }).expect("world");
    start(&mut run.app, prepared);
    run.app.screens.open(Screen::Map.into());
    if let Some(session) = run.app.module.session.as_mut() {
        session.autoplay();
    }
    for _ in 0..env_number("AURELIA_BENCH_ROUNDS", 120) {
        request(&mut run.app, GameRequest::EndTurn);
        run.app.finish_ai_turns();
    }
    if let Some(session) = run.app.module.session.as_mut() {
        session.end_autoplay();
    }
    // The last round's AI work is done; the game now waits for its player.
    run.app.finish_ai_turns();
    center_on_capital(run);
    wide_scene(run, "mid")
}

/// The representative late game of the perf contract: 200x125, 200 cities, 1000 units.
fn late(run: &mut Offscreen) -> Scene {
    use classic_rules::ids::PlayerId;
    let rules = screens::game_rules();
    let config = classic_rules::scenario::ScenarioConfig::representative_late_game();
    let game = classic_rules::scenario::generate(&config, &rules);
    let world = render::world::projection::rebuild_render_world(game.view(), PlayerId(0), &rules);
    let options = screens::GameOptions::default();
    start(&mut run.app, screens::PreparedMatch { game, world, nation: "Bench".into(), options, seed: None });
    run.app.screens.open(Screen::Map.into());
    center_on_capital(run);
    wide_scene(run, "late")
}

/// The dense late map of `--bench-world`: the heaviest world the art can fill.
fn dense(run: &mut Offscreen) -> Scene {
    use classic_rules::ids::PlayerId;
    let rules = screens::game_rules();
    let nation = rules.nations.iter().position(|c| c.key == "core:nation/rome").unwrap_or(0) as u8;
    let config = classic_rules::scenario::ScenarioConfig {
        seed: env_number("AURELIA_GAME_SEED", 20260928),
        players: 6,
        width: 80,
        height: 52,
        cities: 0,
        units: 0,
        water_ratio: 25,
    };
    let game = classic_rules::scenario::generate_player_game(&rules, &config, nation);
    let world = render::world::projection::rebuild_render_world(game.view(), PlayerId(0), &rules);
    let options = screens::GameOptions::default();
    let mut prepared = screens::PreparedMatch { game, world, nation: "Rome".into(), options, seed: None };
    super::showcase::dense(&mut prepared);
    start(&mut run.app, prepared);
    run.app.renderer.set_animation_time_override(Some(1.0));
    run.app.screens.open(Screen::Map.into());
    run.app.center_on((52 / 2) * 80 + 80 / 2);
    wide_scene(run, "dense")
}

fn center_on_capital(run: &mut Offscreen) {
    use classic_rules::ids::PlayerId;
    let tile = run.app.module.session.as_ref().and_then(|s| {
        let auth = s.game().view().auth;
        let city = auth.indexes().cities_of(PlayerId(0)).first().copied()?;
        auth.cities().get(city).map(|c| c.tile.0)
    });
    if let Some(tile) = tile {
        run.app.center_on(tile);
    }
}

/// The scene's two zooms: the game view, and the zoom at which the whole map fits (a
/// map `w` tiles wide is about 1.74 w units across; the view at zoom z shows 72 / z).
fn wide_scene(run: &Offscreen, name: &'static str) -> Scene {
    let width = run.app.view().width as f32;
    let fit = (72.0 / (width * 1.74)).clamp(0.15, 1.0);
    Scene { name, play: env_float("AURELIA_GAME_ZOOM", 2.25), wide: env_float("AURELIA_BENCH_WIDE", fit) }
}

fn measure(run: &mut Offscreen, scene: &Scene) {
    let frames = env_number("AURELIA_BENCH_FRAMES", 120) as usize;
    let cycles = env_number("AURELIA_BENCH_CYCLES", 3) as usize;
    let stats = {
        run.app.camera.zoom = scene.play;
        run.settle();
        run.sample().stats
    };
    let (width, height) = (run.app.view().width, run.app.view().height);
    println!(
        "\n== scene {}: {width}x{height} tiles, turn {}, {}x{} px, zoom {:.2} / {:.2}; city pieces {}, props {}, units {}, terrain {} tris",
        scene.name,
        run.app.module.session.as_ref().map(|_| turn(&run.app)).unwrap_or(0),
        run.size.0,
        run.size.1,
        scene.play,
        scene.wide,
        stats.city_instances,
        stats.prop_instances,
        stats.unit_instances,
        stats.terrain_tris
    );
    let shots = std::env::var("AURELIA_BENCH_SHOTS").ok();
    let middle = (scene.play * scene.wide).sqrt();
    for (label, zoom) in [("game view", scene.play), ("half way", middle), ("whole map", scene.wide)] {
        run.app.camera.zoom = zoom;
        run.settle();
        let samples: Vec<FrameSample> = (0..frames).map(|_| run.sample()).collect();
        Table::of(&format!("{} / {label}, at rest", scene.name), &samples).print();
        if let Some(dir) = &shots {
            run.save(&format!("{dir}/{}-{}.png", scene.name, label.replace(' ', "-")));
        }
    }
    let center = (run.app.camera.x, run.app.camera.y);
    let paced = std::env::var("AURELIA_BENCH_PACE").as_deref() != Ok("0");
    let mut session = Vec::new();
    for cycle in 0..cycles {
        run.app.camera.zoom = scene.play;
        run.app.camera.x = center.0;
        run.app.camera.y = center.1;
        let mut frames = interaction(run, scene, paced);
        if cycle == 0 {
            // The first pass over a zoom range builds its terrain levels: the cold cost.
            Table::of(&format!("{} / session, first cycle (cold: levels and chunks built on the way)", scene.name), &frames).print();
        }
        if cycle > 0 || cycles == 1 {
            session.append(&mut frames);
        }
    }
    Table::of(&format!("{} / session: zoom, pan, wheel notches (warm cycles)", scene.name), &session).print();
    if let Some(dir) = &shots {
        run.app.camera.zoom = scene.play;
        run.settle();
        run.sample();
        run.save(&format!("{dir}/{}-after-session.png", scene.name));
    }
}

/// One cycle of the player's camera: zoom out in 30 frames, pan across the map, zoom half
/// way in, pan, zoom in to the game view and pan, then wheel notches out and back. Paced
/// at 60 Hz like the window.
fn interaction(run: &mut Offscreen, scene: &Scene, paced: bool) -> Vec<FrameSample> {
    let mut frames = Vec::new();
    let mut step = |run: &mut Offscreen, frames: &mut Vec<FrameSample>| {
        let started = Instant::now();
        frames.push(run.sample());
        if paced {
            if let Some(rest) = Duration::from_micros(16_667).checked_sub(started.elapsed()) {
                std::thread::sleep(rest);
            }
        }
    };
    let zoom_to = |run: &mut Offscreen, frames: &mut Vec<FrameSample>, target: f32, count: usize, step: &mut dyn FnMut(&mut Offscreen, &mut Vec<FrameSample>)| {
        let factor = (target / run.app.camera.zoom).powf(1.0 / count as f32);
        for _ in 0..count {
            run.app.camera.zoom_by(factor);
            step(run, frames);
        }
    };
    let pan = |run: &mut Offscreen, frames: &mut Vec<FrameSample>, dx: f32, dy: f32, count: usize, step: &mut dyn FnMut(&mut Offscreen, &mut Vec<FrameSample>)| {
        for _ in 0..count {
            run.app.camera.pan_by(dx, dy);
            step(run, frames);
        }
    };
    let middle = (scene.play * scene.wide).sqrt();
    zoom_to(run, &mut frames, scene.wide, 30, &mut step);
    pan(run, &mut frames, -10.0, 4.0, 30, &mut step);
    pan(run, &mut frames, 10.0, -4.0, 30, &mut step);
    zoom_to(run, &mut frames, middle, 20, &mut step);
    pan(run, &mut frames, 8.0, 0.0, 30, &mut step);
    zoom_to(run, &mut frames, scene.play, 20, &mut step);
    pan(run, &mut frames, -8.0, 3.0, 30, &mut step);
    // Wheel notches: the zoom changes in one frame, six frames apart, out to the whole map
    // and back (each step is a jump of the view, not a glide).
    for (from, to) in [(scene.play, scene.wide), (scene.wide, scene.play)] {
        let notch = (to / from).powf(1.0 / 6.0);
        for _ in 0..6 {
            run.app.camera.zoom_by(notch);
            for _ in 0..6 {
                step(run, &mut frames);
            }
        }
    }
    frames
}

/// Stage times of a series of frames.
struct Table<'a> {
    name: &'a str,
    rows: Vec<(String, Vec<f64>)>,
    frames: usize,
}

impl<'a> Table<'a> {
    fn of(name: &'a str, samples: &[FrameSample]) -> Self {
        use render::gpu::timing::Stage;
        let column = |f: &dyn Fn(&FrameSample) -> f64| samples.iter().map(f).collect::<Vec<f64>>();
        let mut rows = vec![
            ("FRAME (CPU + GPU wait)".to_string(), column(&|s| s.total)),
            ("  module, input, HUD models".into(), column(&|s| s.stages.module_ms)),
            ("  renderer CPU".into(), column(&|s| s.stages.world_ms)),
            ("    terrain step".into(), column(&|s| f64::from(s.stats.terrain_step_ms))),
            ("    shadow fit and maps".into(), column(&|s| f64::from(s.stats.shadow_ms))),
            ("    scene recording".into(), column(&|s| f64::from(s.stats.encode_ms))),
            ("    other (products, uploads, submit)".into(), column(&|s| {
                s.stages.world_ms - f64::from(s.stats.terrain_step_ms + s.stats.shadow_ms + s.stats.encode_ms)
            })),
            ("  egui run".into(), column(&|s| s.stages.ui_ms)),
            ("  egui tessellate, upload, submit".into(), column(&|s| s.stages.paint_ms)),
            ("  wait for the GPU".into(), column(&|s| s.wait)),
        ];
        for stage in Stage::ALL {
            let seen = samples.iter().any(|s| s.gpu.iter().any(|(st, _)| *st == stage));
            if seen {
                rows.push((format!("    {}", stage.name()), column(&|s| s.gpu.iter().find(|(st, _)| *st == stage).map_or(0.0, |(_, ms)| *ms))));
            }
        }
        rows.push(("shadow triangles drawn".into(), column(&|s| s.stats.shadow_tris as f64)));
        rows.push(("terrain triangles".into(), column(&|s| s.stats.terrain_tris as f64)));
        rows.push(("water triangles (resident)".into(), column(&|s| s.stats.water_tris as f64)));
        rows.push(("feature triangles (resident)".into(), column(&|s| s.stats.feature_tris as f64)));
        rows.push(("draw calls".into(), column(&|s| f64::from(s.stats.draw_calls))));
        Self { name, rows, frames: samples.len() }
    }

    fn print(&self) {
        println!("-- {} (n={})", self.name, self.frames);
        println!("{:<40} {:>9} {:>9} {:>9} {:>9} {:>9}", "", "mean", "p50", "p95", "p99", "max");
        for (label, values) in &self.rows {
            let mut sorted = values.clone();
            sorted.sort_by(f64::total_cmp);
            if sorted.is_empty() {
                continue;
            }
            let at = |q: usize| sorted[((sorted.len() - 1) * q / 100).min(sorted.len() - 1)];
            let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
            let number = |v: f64| if label.contains("triangles") || label.contains("draw calls") { format!("{v:.0}") } else { format!("{v:.2}") };
            println!(
                "{label:<40} {:>9} {:>9} {:>9} {:>9} {:>9}",
                number(mean),
                number(at(50)),
                number(at(95)),
                number(at(99)),
                number(sorted[sorted.len() - 1])
            );
        }
    }
}
