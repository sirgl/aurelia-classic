//! Offscreen runs of the real game loop: `--screenshot` captures a scripted match,
//! `--bench` measures command-to-feedback latency and frame time. No window is opened.
//!
//! Environment: AURELIA_GAME_SEED (default 20260928), AURELIA_GAME_TURNS (end turns after the
//! opening, default 0), AURELIA_GAME_SCREEN (map | city | research | civics | religion | culture |
//! diplomacy | city_states | congress | espionage | victory | results | menu | pedia | overview |
//! civilizations | setup | loading | settings), AURELIA_PEDIA (content key of the Encyclopedia page),
//! AURELIA_OVERVIEW (cities | resources | trade | units | demographics | diplomacy | victory | notifications),
//! AURELIA_GAME_TRADE (set: a second city, a Granary, two caravans, one food route running),
//! AURELIA_GAME_DEPOSITS (set: iron with a mine and horses with a pasture next to the capital,
//! their technologies known, and more swordsmen than iron: a deficit; `late`: uranium, oil and
//! aluminum on explored land out of sight, four to five tiles from the capital, and the
//! technologies of AURELIA_GAME_TECHS, comma-separated content keys, learned),
//! AURELIA_GAME_MARINE (set: fish, whales, pearls and crab on the coast around the capital, every third
//! one with fishing boats; `generated`: none added, the sea around the capital explored, so the
//! frame shows the generator's own deposits, with the technologies of AURELIA_GAME_TECHS learned),
//! AURELIA_GAME_SHOWCASE=placement (five sites: plain, slope, hill top, river bank, sea cliff, each a city;
//! AURELIA_GAME_DEVELOPED grows them; AURELIA_GAME_PITCH=<degrees> lowers the camera: how objects meet the ground),
//! AURELIA_GAME_NATION (content key of the observed civilization, default core:nation/rome),
//! AURELIA_GAME_CITY_RENAME (set: the city screen opens with the list of names),
//! AURELIA_GAME_SETUP / AURELIA_GAME_CITY_STATES / AURELIA_GAME_MAX_ROUNDS (see `world_setup`),
//! AURELIA_GAME_TURN_FRAMES (frames after every end turn: the presentation plays),
//! AURELIA_GAME_CAPTURE_EVERY (capture every k-th turn as <path stem>-tNNN.png),
//! AURELIA_GAME_AUTOPLAY (set: the AI plays the observed player too),
//! AURELIA_GAME_OPENING (found | none, default found), AURELIA_GAME_PARADE (set: one unit of every
//! kind around the start), AURELIA_GAME_ZOOM (camera zoom), AURELIA_VISUAL_SIZE (WxH),
//! AURELIA_VISUAL_HOVER_PIXEL (x,y: physical pointer for tooltip and route preview),
//! AURELIA_GAME_SHORTCUTS (set: the keyboard shortcuts overlay is open), AURELIA_GAME_UI_SCALE (0.75..1.5), AURELIA_GAME_SETTINGS_TAB (audio | graphics | game | controls),
//! AURELIA_GAME_SHOWCASE=world (the world art showcase: one capture per view, saved as
//! <path stem>-<view>.png) with AURELIA_GAME_ERAS (eras of players 0..=3, default 7,1,3,5).
//! `--bench-world` measures frame time and world-object rebuilds on a dense late-game map.
//! AURELIA_GAME_SEQUENCE=move|attack|ranged|missile|nuke: frames of one action while the presentation plays
//! it, saved as <path stem>-NN.png (AURELIA_GAME_SEQUENCE_FRAMES, default 14;
//! AURELIA_GAME_SEQUENCE=embark|disembark: a warrior steps from the shore onto the water, or back;
//! AURELIA_GAME_SEQUENCE_STEP seconds per frame, default 0.08).
//! `--bench-animation` measures frames while units walk (AURELIA_BENCH_MAP, AURELIA_BENCH_UNITS).
//! `--bench-ui` measures the UI slice of a frame (models, egui, tessellation; no GPU) on the
//! heaviest screens of an empire of AURELIA_BENCH_CITIES cities (default 100).
use classic_rules::ClassicGame;
use std::time::Instant;

use client::shell::capture::{capture_texture, save_capture, wait_idle};
use client::shell::module::MapButton;
use render::gpu::context::GpuCore;

use crate::ui::model::{Action, Availability, GameRequest, TechStatus};
use crate::ui::screens::{self, MapSize, MatchRequest, PreparedMatch, Screen};
use crate::ClassicUi;

use super::GameApp;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// One measured frame (`Offscreen::sample`).
pub(super) struct FrameSample {
    /// CPU + GPU wait, milliseconds.
    pub total: f64,
    pub stages: client::shell::app::FrameStages,
    pub stats: render::renderer::RenderStats,
    /// Total minus the CPU stages: the GPU still working when the CPU is done.
    pub wait: f64,
    pub gpu: Vec<(render::gpu::timing::Stage, f64)>,
}

pub(super) struct Offscreen {
    core: GpuCore,
    texture: render::gpu::memory::GpuTexture,
    target: wgpu::TextureView,
    pub(super) size: (u32, u32),
    pub(super) app: GameApp,
    frame: u64,
    pointer: Option<egui::Pos2>,
    /// Vertices the last UI frame tessellated (`--bench-ui`).
    ui_vertices: usize,
}

impl Offscreen {
    pub(super) fn new() -> Self {
        let core = match client::shell::gpu::open() {
            Ok(core) => core,
            Err(error) => {
                eprintln!("offscreen run impossible: {error}");
                std::process::exit(2);
            }
        };
        println!("adapter {:?}", core.adapter_info());
        let size = env_size();
        let texture = capture_texture(&core, size.0, size.1, FORMAT);
        let target = texture.create_view(&Default::default());
        let mut app = GameApp::new(&core, FORMAT, None, ClassicUi::game(), &crate::terrain_layers());
        // Offscreen runs never autosave into the player's saves and keep frames still.
        app.screens.settings.autosave_every = 0;
        app.fixed_animation_time = Some(1.0);
        Self {
            core,
            texture,
            target,
            size,
            app,
            frame: 0,
            pointer: env_pointer(),
            ui_vertices: 0,
        }
    }

    /// The UI part of one frame at the capture size; returns milliseconds.
    fn ui_frame(&mut self) -> f64 {
        let screen = egui::vec2(self.size.0 as f32, self.size.1 as f32) / self.app.ctx.pixels_per_point();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            time: Some(self.frame as f64 / 60.0),
            focused: true,
            ..Default::default()
        };
        self.frame += 1;
        let (time, vertices) = self.app.ui_frame(raw, self.size);
        self.ui_vertices = vertices;
        time.as_secs_f64() * 1000.0
    }

    /// One full frame (CPU + GPU wait); returns milliseconds.
    pub(super) fn frame(&mut self) -> f64 {
        // The pointer arrives once and then rests, so tooltips see it still.
        let events = self
            .pointer
            .filter(|_| self.frame < 2)
            .map(|p| vec![egui::Event::PointerMoved(p / self.app.ctx.pixels_per_point())])
            .unwrap_or_default();
        self.frame_with(events)
    }

    /// One full frame with these input events (CPU + GPU wait); returns milliseconds.
    pub(super) fn frame_with(&mut self, events: Vec<egui::Event>) -> f64 {
        let start = Instant::now();
        let screen = egui::vec2(self.size.0 as f32, self.size.1 as f32) / self.app.ctx.pixels_per_point();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            time: Some(self.frame as f64 / 60.0),
            events,
            focused: true,
            ..Default::default()
        };
        self.frame += 1;
        self.app.frame(&self.core, &self.target, self.size, raw);
        wait_idle(&self.core);
        start.elapsed().as_secs_f64() * 1000.0
    }

    /// One frame with its stages: CPU per stage, the wait for the GPU, and the GPU time of
    /// the timed passes (with `AURELIA_GPU_TIMING=1`).
    pub(super) fn sample(&mut self) -> FrameSample {
        let total = self.frame();
        let stages = self.app.stages;
        let stats = self.app.renderer.stats();
        let cpu = stages.module_ms + stages.world_ms + stages.ui_ms + stages.paint_ms;
        let gpu = self.core.timing.as_ref().map(|t| t.read(&self.core.device)).unwrap_or_default();
        FrameSample { total, stages, stats, wait: (total - cpu).max(0.0), gpu }
    }

    /// The last frame as a PNG.
    pub(super) fn save(&self, path: &str) {
        save_capture(&self.core, &self.texture, self.size.0, self.size.1, path);
    }

    /// Frames until no terrain build or chunk work is in flight for a few frames in a
    /// row (level changes publish in steps); returns the frame times and renderer CPU
    /// times on the way.
    pub(super) fn settle(&mut self) -> Vec<(f64, f64)> {
        const QUIET: usize = 3;
        const LIMIT: usize = 5000;
        let (mut frames, mut quiet) = (Vec::new(), 0);
        while quiet < QUIET && frames.len() < LIMIT {
            let frame = self.frame();
            let stats = self.app.renderer.stats();
            frames.push((frame, f64::from(stats.prepare_ms)));
            let busy = stats.terrain_building || stats.chunks_pending;
            quiet = if busy { 0 } else { quiet + 1 };
        }
        frames
    }

    /// Starts the scripted match; returns the showcase views when AURELIA_GAME_SHOWCASE=world.
    pub(super) fn start_match(&mut self) -> Vec<super::showcase::View> {
        let kind = std::env::var("AURELIA_GAME_SHOWCASE").unwrap_or_default();
        let showcase = kind == "world" || kind == "placement";
        let request = MatchRequest {
            civilization: std::env::var("AURELIA_GAME_NATION").unwrap_or_else(|_| "core:nation/rome".into()),
            seed: env_number("AURELIA_GAME_SEED", 20260928),
            size: if showcase { MapSize::Large } else { MapSize::Medium },
            players: 4,
            water: 25,
        };
        let mut prepared = match super::world_setup::match_with_city_states(&request) {
            Some(prepared) => prepared,
            None => screens::prepare_match(request).expect("world"),
        };
        if std::env::var("AURELIA_GAME_PARADE").is_ok() {
            parade(&mut prepared);
        }
        let culture = env_number("AURELIA_GAME_CULTURE", 0);
        if culture > 0 {
            prepared.game.debug_grant_culture(classic_rules::ids::PlayerId(0), culture as u32);
        }
        let faith = env_number("AURELIA_GAME_FAITH", 0);
        if faith > 0 {
            prepared.game.debug_grant_faith(classic_rules::ids::PlayerId(0), faith as u32);
        }
        let views = match kind.as_str() {
            "world" => super::showcase::world(&mut prepared, env_eras()),
            "placement" => super::showcase::placement(&mut prepared, std::env::var("AURELIA_GAME_DEVELOPED").is_ok()),
            _ => Vec::new(),
        };
        start(&mut self.app, prepared);
        self.app.renderer.set_animation_time_override(Some(1.0));
        views
    }

    /// Frames and saves one capture per showcase view.
    fn capture_views(&mut self, views: &[super::showcase::View], path: &str) {
        let stem = path.strip_suffix(".png").unwrap_or(path);
        // AURELIA_GAME_ZOOM overrides every view's zoom (strategic captures of the showcase).
        let zoom: Option<f32> = std::env::var("AURELIA_GAME_ZOOM").ok().and_then(|z| z.parse().ok());
        for view in views {
            self.app.center_on(view.tile);
            self.app.camera.zoom = zoom.unwrap_or(view.zoom);
            for _ in 0..4 {
                self.frame();
            }
            let stats = self.app.renderer.stats();
            println!(
                "{}: {} draws, terrain {} tris, world {} tris ({} props, {} city pieces), units {} tris",
                view.name, stats.draw_calls, stats.terrain_tris, stats.world_tris, stats.prop_instances,
                stats.city_instances, stats.unit_tris
            );
            save_capture(&self.core, &self.texture, self.size.0, self.size.1, &format!("{stem}-{}.png", view.name));
        }
    }
}

/// AURELIA_OBSERVER=1: a game of the setup screen's defaults (AURELIA_GAME_SEED, the `Small` world:
/// six civilizations and the city-states; AURELIA_OBSERVER_POWERS=8 the `Standard` one) played by the AI alone, shot as the observer
/// screen (`client::shell::observer::capture`).
fn observe(run: &mut Offscreen, path: &str) {
    use client::shell::observer::capture::{self, Options};
    let size = if env_number("AURELIA_OBSERVER_POWERS", 6) > 6 { screens::WorldSize::Standard } else { screens::WorldSize::Small };
    let request = MatchRequest {
        civilization: "core:nation/rome".into(),
        seed: env_number("AURELIA_GAME_SEED", 20260928),
        size: MapSize::World(size),
        players: size.civilizations(),
        water: 25,
    };
    let options = screens::GameOptions { city_states: size.city_states(), ..Default::default() };
    let prepared = screens::prepare_game(screens::NewGame { request, options }).expect("world");
    run.app.module.start_observer(prepared, client::shell::observer::Watch::All);
    let shots = Options::from_env(client::shell::observer::frame::Size { width: run.size.0, height: run.size.1 });
    capture::run(&mut run.app, &run.core, &run.texture, &run.target, &shots, path);
}

/// AURELIA_GAME_PARADE: one unit of every kind of the ruleset on land around the first own
/// unit, plus a rival warrior, so a screenshot shows each unit model in the game view.
fn parade(prepared: &mut PreparedMatch) {
    use classic_rules::ids::{PlayerId, TileId, UnitKindId};
    let rules = screens::game_rules();
    let game = &mut prepared.game;
    let width = prepared.world.width as i32;
    let Some(start) = game
        .view()
        .auth
        .units()
        .iter_cores()
        .find(|u| u.owner == PlayerId(0))
        .map(|u| u.tile)
    else {
        return;
    };
    let (q0, r0) = (start.0 as i32 % width, start.0 as i32 / width);
    let map = game.view().auth.map();
    let mut free: Vec<(i32, u32)> = (0..map.tile_count() as u32)
        .filter(|&t| t != start.0)
        .filter(|&t| classic_rules::navigation::cost::enter_cost_on_map(map, TileId(t)).is_some())
        .map(|t| {
            let (dq, dr) = (t as i32 % width - q0, t as i32 / width - r0);
            ((dq.abs() + dr.abs() + (dq + dr).abs()) / 2, t)
        })
        .filter(|&(distance, _)| (1..=3).contains(&distance))
        .collect();
    free.sort();
    let mut tiles = free.into_iter().map(|(_, t)| TileId(t));
    for kind in 0..rules.units.len() {
        if let Some(tile) = tiles.next() {
            game.debug_spawn_unit(&rules, PlayerId(0), tile, UnitKindId(kind as u16));
        }
    }
    let warrior = rules
        .units
        .iter()
        .position(|u| u.key == "core:unit/warrior");
    if let (Some(tile), Some(kind)) = (tiles.next(), warrior) {
        game.debug_spawn_unit(&rules, PlayerId(1), tile, UnitKindId(kind as u16));
    }
}

pub fn screenshot(path: &str) {
    let mut run = Offscreen::new();
    if client::shell::observer::capture::requested() {
        return observe(&mut run, path);
    }
    // Every captured frame shows the terrain of the world it draws.
    run.app.renderer.set_terrain_sync(render::gpu::terrain_gpu::TerrainSync::Immediate);
    let views = run.start_match();
    if !views.is_empty() {
        run.app.screens.open(Screen::Map.into());
        run.capture_views(&views, path);
        return;
    }
    let capital = match std::env::var("AURELIA_GAME_OPENING").as_deref() {
        Ok("none") => None,
        _ => opening(&mut run.app),
    };
    match std::env::var("AURELIA_GAME_DEPOSITS").as_deref() {
        Ok("late") => late_deposits(&mut run.app),
        Ok(_) => deposits(&mut run.app),
        Err(_) => {}
    }
    if std::env::var("AURELIA_GAME_MARINE").is_ok() {
        marine(&mut run.app);
    }
    if std::env::var("AURELIA_GAME_TRADE").is_ok() {
        trade_demo(&mut run.app);
    }
    super::world_setup::before_turns(&mut run.app);
    play_turns(&mut run, path);
    adopt_policies(&mut run.app);
    found_religion(&mut run.app);
    great_works(&mut run.app);
    let target = super::world_setup::after_turns(&mut run.app);
    let staged = super::world_setup::embark_scene(&mut run.app);
    // AURELIA_GAME_MOVES=n: n single-tile moves of the first idle unit, camera kept where the
    // unit started, so MOVES=0 and MOVES=1 frame the same ground (fog before/after).
    let start = if std::env::var("AURELIA_GAME_SELECT").as_deref() == Ok("unit") {
        run.app.module.session.as_mut().and_then(|session| {
            session.select_next_idle_unit();
            session.selected_tile()
        })
    } else {
        None
    };
    for _ in 0..env_number("AURELIA_GAME_MOVES", 0) {
        run.frame();
        match reachable_neighbour(&mut run.app) {
            Some(target) => run.app.click(Some(target), MapButton::Primary),
            None => break,
        }
    }
    // Screenshot-only research goal setup uses the exact same typed UI request.
    if let Ok(key) = std::env::var("AURELIA_GAME_RESEARCH") {
        if let Some(session) = run.app.module.session.as_mut() {
            if let Some(index) = session.rules().technologies.iter().position(|tech| tech.key == key) {
                session.request(GameRequest::Research(classic_rules::ids::TechId(index as u16)));
                println!("Research goal: {key}");
            } else { eprintln!("Unknown AURELIA_GAME_RESEARCH technology: {key}"); }
        }
    }
    show_screen(&mut run.app, capital);
    if let Ok(kind) = std::env::var("AURELIA_GAME_SEQUENCE") {
        return sequence(&mut run, &kind, path);
    }
    if let Some(tile) = start.or(staged.map(|t| t.0)) {
        run.app.center_on(tile);
    }
    if let Some(tile) = target {
        run.pointer = tile_pointer(&run, tile.0);
    }
    run.frame();
    if run.app.screens.screen == Screen::Research.into() { prepare_research_capture(&mut run.app); }
    if super::world_setup::wants_promotions() {
        run.app.module.hud.open_promotions();
    }
    // With a pointer, wait out the tooltip delay so the capture shows the tooltip.
    let frames = if run.pointer.is_some() { 45 } else { 4 };
    for _ in 0..frames {
        run.frame();
    }
    println!("turn {}  hash {:x}", turn(&run.app), hash(&run.app));
    save_capture(&run.core, &run.texture, run.size.0, run.size.1, path);
}

/// AURELIA_GAME_DEPOSITS: strategic resources next to the capital (harness, like
/// AURELIA_GAME_FAITH), and swordsmen past the iron so the deficit shows.
fn deposits(app: &mut GameApp) {
    let Some(session) = app.module.session.as_mut() else {
        return;
    };
    let me = session.me();
    let (game, rules) = session.harness_game();
    let near = own_tiles_around_capital(game, me);
    let pairs = [("core:resource/iron", "core:improvement/mine"), ("core:resource/horses", "core:improvement/pasture")];
    for ((resource, improvement), tile) in pairs.iter().zip(near.iter()) {
        let (Some(r), Some(i)) = (
            rules.resources.iter().position(|x| x.key == *resource),
            rules.improvements.iter().position(|x| x.key == *improvement),
        ) else {
            continue;
        };
        game.debug_set_resource_batch(&rules, &[(*tile, classic_rules::ids::ResourceId(r as u16))]);
        game.debug_set_improvement_batch(&rules, &[(*tile, classic_rules::ids::ImprovementId(i as u16))]);
    }
    for key in ["core:tech/bronze-working", "core:tech/animal-husbandry", "core:tech/iron-working"] {
        if let Some(t) = rules.technologies.iter().position(|t| t.key == key) {
            game.debug_learn(&rules, me, classic_rules::ids::TechId(t as u16));
        }
    }
    if let Some(tile) = near.first() {
        overdraw_iron(game, &rules, me, *tile);
    }
    session.refresh_after_harness();
}

/// AURELIA_GAME_DEPOSITS=late: uranium, oil and aluminum (with their improvements) on explored
/// land the observer does not see, and the technologies listed in AURELIA_GAME_TECHS: the picture
/// of late deposits on remembered tiles.
fn late_deposits(app: &mut GameApp) {
    let Some(session) = app.module.session.as_mut() else {
        return;
    };
    let me = session.me();
    let (game, rules) = session.harness_game();
    let auth = game.view().auth;
    let Some(capital) = auth.cities().iter().find(|(_, c)| c.owner == me).map(|(_, c)| c.tile) else {
        return;
    };
    let dims = auth.map().dimensions;
    let view = game.player_view(&rules, me);
    let ring: Vec<classic_rules::ids::TileId> = classic_rules::math::tiles_in_hex_radius(&dims, capital, 5)
        .into_iter()
        .filter(|t| matches!(dims.distance(capital, *t), Some(4 | 5)))
        .filter(|t| classic_rules::navigation::cost::enter_cost_on_map(auth.map(), *t).is_some())
        .filter(|t| !view.is_visible(*t) && auth.map().resource(*t).flatten().is_none())
        .collect();
    let picks = [ring.len() / 6, ring.len() / 2, ring.len() * 5 / 6];
    let pairs = [
        ("core:resource/uranium", "core:improvement/mine"),
        ("core:resource/oil", "core:improvement/oil-well"),
        ("core:resource/aluminum", "core:improvement/mine"),
    ];
    let mut shown = Vec::new();
    for ((resource, improvement), at) in pairs.iter().zip(picks) {
        let (Some(tile), Some(r), Some(i)) = (
            ring.get(at).copied(),
            rules.resources.iter().position(|x| x.key == *resource),
            rules.improvements.iter().position(|x| x.key == *improvement),
        ) else {
            continue;
        };
        game.debug_set_resource_batch(&rules, &[(tile, classic_rules::ids::ResourceId(r as u16))]);
        game.debug_set_improvement_batch(&rules, &[(tile, classic_rules::ids::ImprovementId(i as u16))]);
        shown.push(tile);
    }
    game.debug_reveal(me, &shown);
    println!("late deposits on tiles {:?} around capital {:?}", shown.iter().map(|t| t.0).collect::<Vec<_>>(), capital.0);
    for key in std::env::var("AURELIA_GAME_TECHS").unwrap_or_default().split(',').filter(|k| !k.is_empty()) {
        match rules.technologies.iter().position(|t| t.key == key) {
            Some(t) => game.debug_learn(&rules, me, classic_rules::ids::TechId(t as u16)),
            None => eprintln!("Unknown AURELIA_GAME_TECHS technology: {key}"),
        }
    }
    session.refresh_after_harness();
}

/// AURELIA_GAME_MARINE: the coast around the capital gets fish, whales, pearls and crab (every third
/// one with fishing boats), so a capture shows what a coast with fish looks like in the real game:
/// visible, then remembered after the sight moves on. `AURELIA_GAME_MARINE=generated` places nothing:
/// the sea around the capital (six tiles) is explored and the technologies of AURELIA_GAME_TECHS are
/// learned, so the capture shows the deposits the map generator laid there.
fn marine(app: &mut GameApp) {
    let generated = std::env::var("AURELIA_GAME_MARINE").as_deref() == Ok("generated");
    let Some(session) = app.module.session.as_mut() else {
        return;
    };
    let me = session.me();
    let (game, rules) = session.harness_game();
    let auth = game.view().auth;
    let Some(capital) = auth.cities().iter().find(|(_, c)| c.owner == me).map(|(_, c)| c.tile) else {
        return;
    };
    let map = auth.map();
    if generated {
        let around = classic_rules::math::tiles_in_hex_radius(&map.dimensions, capital, 6);
        let sea: Vec<_> = around.into_iter().filter(|t| map.is_water(*t)).collect();
        let deposits: Vec<_> = sea.iter().filter(|t| matches!(map.resource(**t), Some(Some(_)))).map(|t| {
            let kind = map.resource(*t).flatten().map(|r| rules.resources[r.0 as usize].key.trim_start_matches("core:resource/").to_owned());
            format!("{} {}", t.0, kind.unwrap_or_default())
        }).collect();
        println!("generated sea deposits within 6 of capital {:?}: {deposits:?}", capital.0);
        game.debug_reveal(me, &sea);
        for key in std::env::var("AURELIA_GAME_TECHS").unwrap_or_default().split(',').filter(|k| !k.is_empty()) {
            match rules.technologies.iter().position(|t| t.key == key) {
                Some(t) => game.debug_learn(&rules, me, classic_rules::ids::TechId(t as u16)),
                None => eprintln!("Unknown AURELIA_GAME_TECHS technology: {key}"),
            }
        }
        session.refresh_after_harness();
        return;
    }
    let water = rules.terrain_kinds.palette().code(classic_rules::rules::roles::Ground::Water);
    let is_water = |t: classic_rules::ids::TileId| map.terrain(t).map(|x| x.0) == Some(water);
    let kinds: Vec<u16> = ["fish", "fish", "whales", "pearls", "crab"]
        .iter()
        .filter_map(|name| rules.resources.iter().position(|r| r.key == format!("core:resource/{name}")))
        .map(|i| i as u16)
        .collect();
    let boats = rules.improvements.iter().position(|i| i.key == "core:improvement/fishing-boats");
    let mut coast: Vec<classic_rules::ids::TileId> =
        classic_rules::math::tiles_in_hex_radius(&map.dimensions, capital, 4)
            .into_iter()
            .filter(|t| is_water(*t) && map.dimensions.neighbors(*t).into_iter().any(|n| !is_water(n)))
            .collect();
    coast.sort_by_key(|t| (map.dimensions.distance(capital, *t).unwrap_or(99), t.0));
    let (mut resources, mut improvements) = (Vec::new(), Vec::new());
    for (n, tile) in coast.into_iter().take(12).enumerate() {
        if kinds.is_empty() {
            break;
        }
        resources.push((tile, classic_rules::ids::ResourceId(kinds[n % kinds.len()])));
        if let Some(boats) = boats.filter(|_| n % 3 == 0) {
            improvements.push((tile, classic_rules::ids::ImprovementId(boats as u16)));
        }
    }
    game.debug_set_resource_batch(&rules, &resources);
    game.debug_set_improvement_batch(&rules, &improvements);
    session.refresh_after_harness();
}

/// AURELIA_GAME_TRADE: a second own city five to seven tiles from the capital, a
/// Granary in the capital, Animal Husbandry and Sailing (two routes) and two
/// caravans; the first starts a food route through the trade panel's request.
fn trade_demo(app: &mut GameApp) {
    use classic_rules::ids::{BuildingKindId, TechId, UnitKindId};
    let Some(session) = app.module.session.as_mut() else {
        return;
    };
    let me = session.me();
    let (game, rules) = session.harness_game();
    let auth = game.view().auth;
    let Some((capital, tile)) = auth.cities().iter().find(|(_, c)| c.owner == me).map(|(h, c)| (h, c.tile)) else {
        return;
    };
    let dims = auth.map().dimensions;
    let east = classic_rules::math::tiles_in_hex_radius(&dims, tile, 7).into_iter().find(|t| {
        let map = auth.map();
        dims.distance(tile, *t).unwrap_or(0) >= 5
            && classic_rules::navigation::cost::enter_cost_on_map(map, *t).is_some()
            && map.owner(*t).flatten().is_none()
    });
    let index = |found: Option<usize>| found.map(|i| i as u16);
    let granary = index(rules.buildings.iter().position(|b| b.key == "core:building/granary"));
    let caravan = index(rules.units.iter().position(|u| u.key == "core:unit/caravan"));
    let tech = |key: &str| index(rules.technologies.iter().position(|t| t.key == key));
    let (husbandry, sailing) = (tech("core:tech/animal-husbandry"), tech("core:tech/sailing"));
    let (Some(east), Some(granary), Some(caravan), Some(husbandry), Some(sailing)) =
        (east, granary, caravan, husbandry, sailing)
    else {
        return;
    };
    let other = game.debug_found_city(&rules, me, east);
    game.debug_add_building(&rules, capital, BuildingKindId(granary));
    game.debug_learn(&rules, me, TechId(husbandry));
    game.debug_learn(&rules, me, TechId(sailing));
    let first = game.debug_spawn_unit(&rules, me, tile, UnitKindId(caravan));
    game.debug_spawn_unit(&rules, me, tile, UnitKindId(caravan));
    session.refresh_after_harness();
    session.request(GameRequest::EstablishTradeRoute {
        unit: first,
        destination: other,
        kind: classic_rules::world::trade::RouteKind::Food,
    });
}

/// The player's own tiles next to its first city.
fn own_tiles_around_capital(game: &classic_rules::GameState, me: classic_rules::ids::PlayerId) -> Vec<classic_rules::ids::TileId> {
    let auth = game.view().auth;
    let Some(capital) = auth.cities().iter().find(|(_, c)| c.owner == me).map(|(_, c)| c.tile) else {
        return Vec::new();
    };
    let neighbours = auth.map().dimensions.neighbors(capital);
    neighbours.into_iter().filter(|t| auth.map().owner(*t).flatten() == Some(me)).collect()
}

/// One swordsman more than the iron: a deficit.
fn overdraw_iron(
    game: &mut classic_rules::GameState,
    rules: &classic_rules::CompiledRuleset,
    me: classic_rules::ids::PlayerId,
    tile: classic_rules::ids::TileId,
) {
    let iron = rules.resources.iter().position(|r| r.key == "core:resource/iron");
    let sword = rules.units.iter().position(|u| u.key == "core:unit/swordsman");
    let (Some(iron), Some(sword)) = (iron, sword) else {
        return;
    };
    let ledger = classic_rules::derived::resources::resource_ledger(game.view().auth, rules, me);
    for _ in 0..=ledger.total(classic_rules::ids::ResourceId(iron as u16)) {
        game.debug_spawn_unit(rules, me, tile, classic_rules::ids::UnitKindId(sword as u16));
    }
}

/// AURELIA_GAME_TURNS end turns. With AURELIA_GAME_TURN_FRAMES=n every turn is
/// followed by n frames (the presentation plays what the AI did), and with
/// AURELIA_GAME_CAPTURE_EVERY=k every k-th turn is captured as
/// `<path stem>-tNNN.png` with its turn time and resident memory printed
/// (the QA soak of the game with presentation, classic/docs/QA.md).
fn play_turns(run: &mut Offscreen, path: &str) {
    if std::env::var("AURELIA_GAME_AUTOPLAY").is_ok() {
        if let Some(session) = run.app.module.session.as_mut() {
            session.autoplay();
        }
    }
    let frames = env_number("AURELIA_GAME_TURN_FRAMES", 0);
    let every = env_number("AURELIA_GAME_CAPTURE_EVERY", 0);
    let stem = path.strip_suffix(".png").unwrap_or(path).to_owned();
    for index in 1..=env_number("AURELIA_GAME_TURNS", 0) {
        let started = Instant::now();
        request(&mut run.app, GameRequest::EndTurn);
        run.app.finish_ai_turns();
        for _ in 0..frames {
            run.frame();
        }
        let turn_ms = started.elapsed().as_secs_f64() * 1000.0;
        if every > 0 && index % every == 0 {
            let gpu = run.app.renderer.gpu_memory();
            println!(
                "turn {} ({turn_ms:.0} ms with {frames} frames), rss {} KiB, gpu {} KiB, hash {:x}",
                turn(&run.app),
                resident_kib(),
                gpu.total() / 1024,
                hash(&run.app)
            );
            print_gpu_memory(run);
            save_capture(&run.core, &run.texture, run.size.0, run.size.1, &format!("{stem}-t{index:03}.png"));
        }
    }
}

/// The process memory after a bench phase; the heap peak then restarts.
fn print_memory(phase: &str) {
    println!("memory after {phase}: {}", client::perf::MemoryLine);
    client::perf::reset_heap_peak();
}

/// The renderer's GPU memory ledger by category, and what the driver reports in all.
fn print_gpu_memory(run: &Offscreen) {
    println!("{}", run.app.renderer.gpu_memory());
    if let Some(bytes) = run.core.driver_allocated_bytes() {
        println!("  driver allocated  {:>10.2}", bytes as f64 / (1024.0 * 1024.0));
    }
}

/// Resident memory of this process (`ps`), 0 when unavailable.
fn resident_kib() -> u64 {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Where a tile's centre appears on the capture (the pointer of a hover).
fn tile_pointer(run: &Offscreen, tile: u32) -> Option<egui::Pos2> {
    let width = run.app.view().width.max(1);
    let (x, z) = render::scene::layout::tile_to_world(tile % width, tile / width);
    let (w, h) = (run.size.0 as f32, run.size.1 as f32);
    let (sx, sy) = run.app.camera.project_to_screen([x, 0.3, z], w / h, w, h)?;
    Some(egui::pos2(sx, sy))
}

/// What a sequence shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SequenceKind {
    Move,
    Melee,
    Ranged,
    /// From the shore onto the water (Optics known): soldiers become a boat.
    Embark,
    /// From the water onto the shore: the boat becomes soldiers.
    Disembark,
    /// A guided missile flies three tiles and blows up on a rival warrior.
    Missile,
    /// A nuclear missile launched at a rival warrior three tiles away.
    Nuke,
}

/// AURELIA_GAME_SEQUENCE: a fresh unit of the player near the capital acts (a two-tile walk, a
/// melee strike or a ranged shot at a rival warrior next to it); every frame of the
/// presentation is saved. Offscreen, fixed presentation steps.
fn sequence(run: &mut Offscreen, kind: &str, path: &str) {
    let kind = match kind {
        "attack" | "melee" => SequenceKind::Melee,
        "ranged" => SequenceKind::Ranged,
        "missile" => SequenceKind::Missile,
        "nuke" => SequenceKind::Nuke,
        "embark" => SequenceKind::Embark,
        "disembark" => SequenceKind::Disembark,
        _ => SequenceKind::Move,
    };
    let Some((target, focus)) = stage_sequence(&mut run.app, kind) else {
        println!("AURELIA_GAME_SEQUENCE: no room for the scene");
        return;
    };
    run.app.presentation_step = Some(env_float("AURELIA_GAME_SEQUENCE_STEP", 0.08));
    // The camera looks at the middle of the scene (the actor and its target).
    run.app.center_on(focus);
    let (x, z) = (run.app.camera.x, run.app.camera.y);
    run.app.center_on(target);
    run.app.camera.x = (run.app.camera.x + x) * 0.5;
    run.app.camera.y = (run.app.camera.y + z) * 0.5;
    run.app.camera.zoom = std::env::var("AURELIA_GAME_ZOOM").ok().and_then(|z| z.parse().ok()).unwrap_or(5.0);
    for _ in 0..4 {
        run.frame();
    }
    let button = if matches!(kind, SequenceKind::Move | SequenceKind::Embark | SequenceKind::Disembark) {
        MapButton::Primary
    } else {
        MapButton::Secondary
    };
    run.app.click(Some(target), button);
    let stem = path.strip_suffix(".png").unwrap_or(path);
    for index in 0..env_number("AURELIA_GAME_SEQUENCE_FRAMES", 14) {
        run.frame();
        let stats = run.app.renderer.stats();
        println!("frame {index:02}: posed figures {}, instance bytes {}", stats.posed_figures, stats.instance_bytes);
        save_capture(&run.core, &run.texture, run.size.0, run.size.1, &format!("{stem}-{index:02}.png"));
    }
    println!("turn {}  hash {:x}", turn(&run.app), hash(&run.app));
}

/// Spawns the acting unit (and its rival) on free land near the capital and selects it.
/// Returns the tile to click and the tile to centre the camera on.
fn stage_sequence(app: &mut GameApp, kind: SequenceKind) -> Option<(u32, u32)> {
    use classic_rules::ids::{PlayerId, UnitKindId};
    let session = app.module.session.as_mut()?;
    let rules = screens::game_rules();
    let kind_of = |key: &str| rules.units.iter().position(|u| u.key == key).map(|i| UnitKindId(i as u16));
    let visible = session.view().visible.clone();
    let (game, _) = session.harness_game();
    let home = game.view().auth.units().iter_cores().find(|u| u.owner == PlayerId(0)).map(|u| u.tile)?;
    if matches!(kind, SequenceKind::Embark | SequenceKind::Disembark) {
        let (shore, water) = shore_and_water(game, home)?;
        let optics = rules.technologies.iter().position(|t| t.key == "core:tech/optics")?;
        game.debug_learn(&rules, PlayerId(0), classic_rules::ids::TechId(optics as u16));
        let (from, to) = if kind == SequenceKind::Embark { (shore, water) } else { (water, shore) };
        let unit = game.debug_spawn_unit(&rules, PlayerId(0), from, kind_of("core:unit/warrior")?);
        session.refresh_after_harness();
        session.select_unit(unit);
        return Some((to.0, from.0));
    }
    let mut free = free_land(game, home, if matches!(kind, SequenceKind::Missile | SequenceKind::Nuke) { 4 } else { 3 });
    // Inside the visible land: the fog's soft edge would hide the figures.
    let dims = game.view().auth.map().dimensions;
    let seen = |t: classic_rules::ids::TileId| classic_rules::math::tiles_in_hex_radius(&dims, t, 1).iter().all(|n| visible.get(n.0 as usize) == Some(&1));
    if kind != SequenceKind::Missile {
        free.retain(|t| seen(*t));
    }
    let gap = match kind {
        SequenceKind::Move => 2,
        SequenceKind::Missile => 3,
        SequenceKind::Nuke => 2,
        _ => 1,
    };
    let (first, second) = free.iter().find_map(|a| {
        let b = free.iter().find(|b| dims.distance(*a, **b) == Some(gap))?;
        Some((*a, *b))
    })?;
    let actor = match kind {
        SequenceKind::Ranged => kind_of("core:unit/archer")?,
        // Four moves: a two-tile walk over any ground this turn.
        SequenceKind::Move => kind_of("core:unit/horseman").or_else(|| kind_of("core:unit/warrior"))?,
        SequenceKind::Melee => kind_of("core:unit/warrior")?,
        SequenceKind::Missile => kind_of("core:unit/guided-missile")?,
        SequenceKind::Nuke => kind_of("core:unit/nuclear-missile")?,
        SequenceKind::Embark | SequenceKind::Disembark => unreachable!("staged above"),
    };
    let unit = game.debug_spawn_unit(&rules, PlayerId(0), first, actor);
    if kind != SequenceKind::Move {
        game.debug_spawn_unit(&rules, PlayerId(1), second, kind_of("core:unit/warrior")?);
    }
    session.refresh_after_harness();
    session.select_unit(unit);
    if kind == SequenceKind::Move {
        // A two-tile walk the unit finishes this turn.
        let target = free.iter().copied().filter(|t| dims.distance(first, *t) == Some(2)).find(|t| {
            let route = session.path_preview(Some(t.0));
            matches!(route, Some(r) if r.this_turn == r.tiles.len())
        });
        return Some((target.unwrap_or(second).0, first.0));
    }
    Some((second.0, first.0))
}

/// The shore tile nearest to `home` that has a free water tile next to it, with that water
/// tile (both free of units and cities).
fn shore_and_water(game: &classic_rules::GameState, home: classic_rules::ids::TileId) -> Option<(classic_rules::ids::TileId, classic_rules::ids::TileId)> {
    let auth = game.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    let free = |t: &classic_rules::ids::TileId| auth.indexes().units_on(*t).is_empty() && auth.indexes().city_on_tile(*t).is_none();
    let water = |t: &classic_rules::ids::TileId| classic_rules::navigation::terrain::is_water(map, *t);
    let mut tiles: Vec<_> = (0..map.tile_count() as u32).map(classic_rules::ids::TileId).filter(|t| !water(t) && free(t)).collect();
    tiles.sort_by_key(|t| dims.distance(home, *t).unwrap_or(i32::MAX));
    tiles.into_iter().find_map(|shore| {
        let sea = dims.neighbors(shore).into_iter().find(|n| water(n) && free(n))?;
        classic_rules::navigation::cost::enter_cost_on_map(map, shore)?;
        Some((shore, sea))
    })
}

/// Land tiles without units or cities within `radius` of `center`.
fn free_land(game: &classic_rules::GameState, center: classic_rules::ids::TileId, radius: i32) -> Vec<classic_rules::ids::TileId> {
    let auth = game.view().auth;
    let map = auth.map();
    classic_rules::math::tiles_in_hex_radius(&map.dimensions, center, radius)
        .into_iter()
        .filter(|t| classic_rules::navigation::cost::enter_cost_on_map(map, *t).is_some())
        .filter(|t| auth.indexes().units_on(*t).is_empty() && auth.indexes().city_on_tile(*t).is_none())
        .collect()
}

pub(super) fn env_float(name: &str, default: f32) -> f32 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// AURELIA_GAME_POLICIES=key,key,...: unlock these branches and adopt these policies in order
/// (content keys, e.g. `core:policy/tradition,core:policy/tradition/aristocracy`).
fn adopt_policies(app: &mut GameApp) {
    let Ok(list) = std::env::var("AURELIA_GAME_POLICIES") else {
        return;
    };
    let rules = screens::game_rules();
    for key in list.split(',').map(str::trim).filter(|k| !k.is_empty()) {
        let branch = rules.module.policy_branches.iter().position(|b| b.key == key);
        let policy = rules.module.policies.iter().position(|p| p.key == key);
        let wanted = match (branch, policy) {
            (Some(b), _) => GameRequest::UnlockBranch(classic_rules::rules::content::PolicyBranchId(b as u16)),
            (None, Some(p)) => GameRequest::AdoptPolicy(classic_rules::ids::PolicyId(p as u16)),
            (None, None) => {
                println!("AURELIA_GAME_POLICIES: unknown key {key}");
                continue;
            }
        };
        request(app, wanted);
    }
}

/// AURELIA_GAME_RELIGION=religion,founder,follower[,pantheon]: the player's Great Prophet founds
/// this religion (content keys), e.g. after AURELIA_GAME_FAITH=350 AURELIA_GAME_TURNS=1.
fn found_religion(app: &mut GameApp) {
    let Ok(list) = std::env::var("AURELIA_GAME_RELIGION") else {
        return;
    };
    let rules = screens::game_rules();
    let keys: Vec<&str> = list.split(',').map(str::trim).collect();
    let religion = rules.module.religions.iter().position(|r| r.key == keys[0]);
    let belief = |k: Option<&&str>| {
        k.and_then(|k| rules.module.beliefs.iter().position(|b| b.key == *k))
            .map(|i| classic_rules::ids::BeliefId(i as u16))
    };
    let (Some(religion), Some(founder), Some(follower)) = (religion, belief(keys.get(1)), belief(keys.get(2))) else {
        println!("AURELIA_GAME_RELIGION: unknown key in {list}");
        return;
    };
    request(app, GameRequest::FoundReligion {
        religion: classic_rules::rules::content::ReligionId(religion as u16),
        founder,
        follower,
        pantheon: belief(keys.get(3)),
    });
}

/// AURELIA_GAME_WORKS=1: the capital gets an Amphitheater, a Museum and an Opera House and
/// great people fill them (a writer, two artists, a musician), for the Culture screen.
fn great_works(app: &mut GameApp) {
    use classic_rules::ids::{BuildingKindId, PlayerId, UnitKindId};
    if std::env::var("AURELIA_GAME_WORKS").is_err() {
        return;
    }
    let Some(session) = app.module.session.as_mut() else {
        return;
    };
    let (game, rules) = session.harness_game();
    let Some(city) = game.view().auth.indexes().cities_of(PlayerId(0)).first().copied() else {
        return;
    };
    let tile = game.view().auth.cities().get(city).unwrap().tile;
    for key in ["core:building/amphitheater", "core:building/museum", "core:building/opera_house"] {
        if let Some(i) = rules.buildings.iter().position(|b| b.key == key) {
            game.debug_add_building(&rules, city, BuildingKindId(i as u16));
        }
    }
    let people = [
        "core:unit/great-writer",
        "core:unit/great-artist",
        "core:unit/great-artist",
        "core:unit/great-musician",
    ];
    for key in people {
        let Some(i) = rules.units.iter().position(|u| u.key == key) else {
            continue;
        };
        let unit = game.debug_spawn_unit(&rules, PlayerId(0), tile, UnitKindId(i as u16));
        let _ = game.execute(&rules, classic_rules::Command::CreateGreatWork { unit });
    }
    session.refresh_after_harness();
}

/// Found the capital, queue a warrior, start the first available technology.
pub(super) fn opening(app: &mut GameApp) -> Option<u32> {
    request(app, GameRequest::Unit(Action::FoundCity));
    let session = app.module.session.as_mut()?;
    let capital = session.selected_city_tile();
    let city = session.city_view()?;
    let warrior = city.options.iter().find(|o| o.name == "Warrior")?.item;
    session.request(GameRequest::Enqueue(warrior));
    let research = session.research_view();
    let first = research
        .techs
        .iter()
        .find(|t| t.status == TechStatus::Available)?
        .id;
    session.request(GameRequest::Research(first));
    capital
}

/// Layout and zoom can discard the first frame. Reapply explicit QA focus once
/// the actual viewport and scrollable content size have been established.
fn prepare_research_capture(app: &mut GameApp) {
    if let Some(session) = app.module.session.as_mut() {
        let focus = std::env::var("AURELIA_GAME_RESEARCH_FOCUS").ok().and_then(|key|
            session.rules().technologies.iter().position(|tech| tech.key == key));
        let (width, height) = env_size();
        app.module.screens.capture_research(&app.ctx, &session.research_view(), focus,
            egui::vec2(width as f32, height as f32) / app.ctx.pixels_per_point());
    }
}

fn show_screen(app: &mut GameApp, capital: Option<u32>) {
    // Offscreen App has no config path: this never changes the player's settings file.
    if let Some(scale) = std::env::var("AURELIA_GAME_UI_SCALE").ok().and_then(|s| s.parse::<f32>().ok())
        .filter(|scale| scale.is_finite() && (0.75..=1.5).contains(scale)) {
        app.screens.settings.ui_scale = scale;
        app.ctx.set_zoom_factor(scale);
    }
    let screen = match std::env::var("AURELIA_GAME_SCREEN").as_deref() {
        Ok("city") => Screen::City,
        Ok("research") => Screen::Research,
        Ok("civics") => Screen::Civics,
        Ok("religion") => Screen::Religion,
        Ok("culture") => Screen::Culture,
        Ok("diplomacy") => Screen::Diplomacy,
        Ok("city_states") => Screen::CityStates,
        Ok("congress") => Screen::Congress,
        Ok("espionage") => Screen::Espionage,
        Ok("victory") => Screen::Victory,
        Ok("results") => Screen::GameEnd,
        Ok("menu") => Screen::Menu,
        Ok("civilizations") => Screen::Civilizations,
        Ok("pedia") => Screen::Encyclopedia,
        Ok("overview") => Screen::Overview,
        Ok("setup") | Ok("loading") => Screen::MatchSetup,
        Ok("settings") => Screen::Settings,
        _ => Screen::Map,
    };
    if screen == Screen::City {
        if let Some(tile) = capital {
            request(app, GameRequest::SelectCity(tile));
        }
    }
    app.screens.open(screen.into());
    if screen == Screen::City && std::env::var_os("AURELIA_GAME_CITY_BUY").is_some() {
        if let Some(city) = app.module.session.as_mut().and_then(|session| session.city_view()) {
            app.module.screens.capture_city_buy(&city);
        }
    }
    if screen == Screen::City && std::env::var_os("AURELIA_GAME_CITY_RENAME").is_some() {
        if let Some(city) = app.module.session.as_mut().and_then(|session| session.city_view()) {
            app.module.screens.capture_city_rename(&city);
        }
    }
    if screen == Screen::Research { prepare_research_capture(app); }
    if screen == Screen::Encyclopedia {
        let key = std::env::var("AURELIA_PEDIA").unwrap_or_default();
        let page = crate::ui::pedia::game_pedia().by_key(&key);
        app.screens.open(Screen::Map.into());
        app.module.screens.screen = Screen::Map;
        app.module.screens.open_pedia(page);
        app.apply_module_actions();
        app.module.screens.preload_pedia_art(&app.ctx);
    }
    if screen == Screen::Overview {
        app.module.screens.open_overview(overview_tab());
        app.apply_module_actions();
    }
    if std::env::var("AURELIA_GAME_SCREEN").as_deref() == Ok("loading") {
        // The loading screen of a new game; the world itself is not made.
        app.module.screens.request_new_match();
        let _ = app.module.screens.take_match_request();
    }
    if screen == Screen::Map {
        if std::env::var("AURELIA_GAME_SELECT").as_deref() == Ok("unit") {
            if let Some(session) = app.module.session.as_mut() {
                session.select_next_idle_unit();
            }
        }
        app.center_on_selection();
        if let Some(zoom) = std::env::var("AURELIA_GAME_ZOOM")
            .ok()
            .and_then(|z| z.parse().ok())
        {
            app.camera.zoom = zoom;
        }
    }
    if let Ok(tab) = std::env::var("AURELIA_GAME_SETTINGS_TAB") { app.screens.capture_settings_tab(&app.ctx, &tab); }
    app.screens.capture_navigation(std::env::var("AURELIA_GAME_NAVIGATION").as_deref() == Ok("1"));
    app.screens.shortcuts.open = std::env::var_os("AURELIA_GAME_SHORTCUTS").is_some();
    if std::env::var("AURELIA_GAME_SETUP_TAB").as_deref() == Ok("rules") {
        app.module.screens.capture_setup_rules();
    }
    app.screens.preload_screen_art(&app.ctx, &mut app.module);
    app.module.hud.preload_action_art(&app.ctx);
    if screen == Screen::Civics {
        if let Some(session) = app.module.session.as_mut() {
            let mut model = session.screen_model();
            model.civics = Some(session.civics_view());
            app.module.screens.preload_civics_art(&app.ctx, &model);
        }
    }
    if screen == Screen::Religion {
        if let Some(session) = app.module.session.as_mut() {
            let mut model = session.screen_model();
            model.religion = Some(session.religion_view());
            app.module.screens.preload_religion_art(&app.ctx, &model);
        }
    }
    if screen == Screen::Culture {
        if let Some(session) = app.module.session.as_mut() {
            let mut model = session.screen_model();
            model.culture = Some(session.culture_view());
            app.module.screens.preload_culture_art(&app.ctx, &model);
        }
    }
    if screen == Screen::Diplomacy {
        if let Some(session) = app.module.session.as_mut() {
            let model = crate::module::screen_model_of(session, screen, &[]);
            app.module.screens.preload_leader_art(&app.ctx, &model);
        }
    }
    let names = visible_art(app, screen);
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    app.module.screens.preload_art(&app.ctx, &names);
    if let Ok(sheet) = std::env::var("AURELIA_GAME_ATLAS_REVIEW") {
        app.screens.preload_atlas_review(&app.ctx, &sheet);
    }
}

fn overview_tab() -> crate::ui::screens::OverviewTab {
    use crate::ui::screens::OverviewTab;
    match std::env::var("AURELIA_OVERVIEW").as_deref() {
        Ok("units") => OverviewTab::Units,
        Ok("demographics") => OverviewTab::Demographics,
        Ok("diplomacy") => OverviewTab::Diplomacy,
        Ok("victory") => OverviewTab::Victory,
        Ok("notifications") => OverviewTab::Notifications,
        Ok("resources") => OverviewTab::Resources,
        Ok("trade") => OverviewTab::Trade,
        _ => OverviewTab::Cities,
    }
}

/// Art the first screenful of a game screen shows (live frames load it off-thread).
fn visible_art(app: &mut GameApp, screen: Screen) -> Vec<String> {
    let Some(session) = app.module.session.as_mut() else {
        return Vec::new();
    };
    match screen {
        Screen::Research => {
            let view = session.research_view();
            let early = view.techs.into_iter().filter(|t| t.column < 4);
            early.map(|t| t.art).take(28).collect()
        }
        Screen::City => match session.city_view() {
            Some(city) => city.options.into_iter().map(|o| o.art).take(8).collect(),
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Latency of a unit move and of end turn (with AI) until the next frame is on the GPU,
/// and steady frame time. Release build, offscreen, CPU submit + GPU wait per frame.
pub fn bench() {
    let mut run = Offscreen::new();
    print_memory("shell and GPU open");
    run.start_match();
    print_memory("match started");
    for _ in 0..5 {
        run.frame();
    }
    print_memory("first frames");
    let steady: Vec<f64> = (0..60).map(|_| run.frame()).collect();
    report(
        "steady frame",
        &steady,
        run.app.renderer.stats().bytes_uploaded,
    );
    // With AURELIA_GPU_TIMING=1: the GPU time of the scene pass over 30 more steady frames.
    if run.core.timing.is_some() {
        let mut scene: Vec<f64> = (0..30)
            .filter_map(|_| {
                run.sample().gpu.iter().find(|(stage, _)| *stage == render::gpu::timing::Stage::Scene).map(|(_, ms)| *ms)
            })
            .collect();
        scene.sort_by(f64::total_cmp);
        if let Some(median) = scene.get(scene.len() / 2) {
            println!("steady GPU scene pass: n={} median {median:.2} ms", scene.len());
        }
    }
    // Shader work only needs the steady frame: skip the move and turn latency runs.
    if std::env::var_os("AURELIA_BENCH_STEADY_ONLY").is_some() {
        print_gpu_memory(&run);
        return;
    }
    let moves = bench_moves(&mut run, 24);
    report("unit move: command", &moves.commands, 0);
    report("unit move, explored land: next frame", &moves.explored, moves.explored_bytes);
    report("unit move, first reveal: next frame", &moves.reveal, moves.reveal_bytes);
    report("unit move, first reveal: terrain publish frame", &moves.publish, moves.publish_bytes);
    report("unit move, first reveal: worst frame until published", &moves.worst, 0);
    report("unit move, first reveal: command to terrain on screen", &moves.latency, 0);
    request(&mut run.app, GameRequest::Unit(Action::FoundCity));
    let found = run.frame();
    report("found city: next frame", &[found], run.app.renderer.stats().bytes_uploaded);
    let (turns, turn_frames, turn_bytes) = bench_turns(&mut run, 30);
    report("end turn + AI: command", &turns, 0);
    report("end turn + AI: next frame", &turn_frames, turn_bytes);
    println!("final turn {}  hash {:x}", turn(&run.app), hash(&run.app));
    print_memory("30 turns played");
    print_gpu_memory(&run);
}

/// `--bench-world`: frame time, triangles and instances on a dense late-game map, and the
/// cost of the two world-object rebuilds a game triggers (a city grows: city tiles; an
/// improvement is built: tile props). Release, offscreen, CPU submit + GPU wait per frame.
/// AURELIA_BENCH_MAP=WxH (default 80x52), AURELIA_BENCH_FRAMES (default 40).
pub fn bench_world() {
    use classic_rules::ids::PlayerId;
    let mut run = Offscreen::new();
    print_memory("shell and GPU open");
    let rules = screens::game_rules();
    let (width, height) = env_pair("AURELIA_BENCH_MAP", (80, 52));
    let civ = rules.civs.iter().position(|c| c.key == "core:nation/rome").unwrap_or(0) as u8;
    let config = classic_rules::scenario::ScenarioConfig {
        seed: env_number("AURELIA_GAME_SEED", 20260928),
        players: 6,
        width,
        height,
        cities: 0,
        units: 0,
        water_ratio: 25,
    };
    let started = Instant::now();
    let game = classic_rules::scenario::generate_player_game(&rules, &config, civ);
    let world = render::world::projection::rebuild_render_world(game.view(), PlayerId(0), &rules);
    let options = screens::GameOptions::default();
    let mut prepared = PreparedMatch { game, world, civilization: "Rome".into(), options };
    super::showcase::dense(&mut prepared);
    println!("map built in {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
    print_memory("dense map built");
    start(&mut run.app, prepared);
    print_memory("match started");
    run.app.renderer.set_animation_time_override(Some(1.0));
    run.app.screens.open(Screen::Map.into());
    let center = (height / 2) * width + width / 2;
    run.app.center_on(center);
    let first = run.frame();
    let stats = run.app.renderer.stats();
    report("first frame (all products built)", &[first], stats.bytes_uploaded);
    print_memory("first frame");
    let frames = env_number("AURELIA_BENCH_FRAMES", 40) as usize;
    for (label, zoom) in [("game view (zoom 2.25)", 2.25), ("strategic view (zoom 0.8)", 0.8)] {
        run.app.camera.zoom = zoom;
        report_settle(&format!("{label}: zoom change until settled"), &run.settle());
        let samples: Vec<f64> = (0..frames).map(|_| run.frame()).collect();
        report(label, &samples, 0);
        print_stats(&run.app.renderer.stats());
        print_memory(label);
    }
    run.app.camera.zoom = 2.25;
    report_settle("back to game view: zoom change until settled", &run.settle());
    let (grow, grow_frames, grow_bytes) = bench_rebuilds(&mut run, frames, Rebuild::CityGrows);
    report("city grows: projection refresh", &grow, 0);
    report("city grows: next frame", &grow_frames, grow_bytes);
    let (build, build_frames, build_bytes) = bench_rebuilds(&mut run, frames, Rebuild::ImprovementBuilt);
    report("improvement built: projection refresh", &build, 0);
    report("improvement built: next frame", &build_frames, build_bytes);
    print_memory("rebuilds");
    print_gpu_memory(&run);
}

/// `--bench-animation`: frames while AURELIA_BENCH_UNITS (default 50) units walk at once on
/// the representative 25k map (200x125, 200 cities, 1000 units), against the same view at
/// rest. Release, offscreen, CPU submit + GPU wait per frame; the presentation advances
/// 1/60 s per frame. Reports frame median/p95, bytes uploaded, terrain rebuilds and the
/// posed figures, so a regression to per-frame terrain or full unit rebuilds shows.
pub fn bench_animation() {
    use classic_rules::ids::PlayerId;
    let mut run = Offscreen::new();
    let rules = screens::game_rules();
    let config = classic_rules::scenario::ScenarioConfig::representative_late_game();
    print_memory("shell and GPU open");
    let game = classic_rules::scenario::generate(&config, &rules);
    let world = render::world::projection::rebuild_render_world(game.view(), PlayerId(0), &rules);
    print_memory("late game generated");
    let options = screens::GameOptions::default();
    start(&mut run.app, PreparedMatch { game, world, civilization: "Bench".into(), options });
    print_memory("match started (AI, knowledge, projection)");
    run.app.screens.open(Screen::Map.into());
    let count = env_number("AURELIA_BENCH_UNITS", 50) as usize;
    let Some((center, walks)) = stage_walkers(&mut run.app, count) else {
        println!("no room for {count} walkers");
        return;
    };
    run.app.center_on(center);
    run.app.camera.zoom = env_float("AURELIA_GAME_ZOOM", 1.6);
    run.app.presentation_step = Some(0.0);
    report_settle("warmup until settled", &run.settle());
    let frames = env_number("AURELIA_BENCH_FRAMES", 40) as usize;
    let rest = measure(&mut run, frames);
    rest.print("at rest (same view, nothing animates)");
    for (unit, target) in &walks {
        if let Some(session) = run.app.module.session.as_mut() {
            session.select_unit(*unit);
        }
        run.app.click(Some(*target), MapButton::Primary);
    }
    // The commands may reveal land: let that terrain settle with the animation frozen.
    report_settle("moves issued, terrain settled (animation frozen)", &run.settle());
    run.app.presentation_step = Some(1.0 / 60.0);
    let walking = measure(&mut run, frames);
    walking.print(&format!("{} walks", walks.len()));
    // The walks end and the units come to rest again (their resting figures return).
    let landing = measure(&mut run, 30);
    landing.print("walks ending and settling");
    print_memory("walks");
    print_gpu_memory(&run);
}

/// Frames of one bench phase and what they did.
#[derive(Default)]
struct Phase {
    frames: Vec<f64>,
    cpu: Vec<f64>,
    bytes: Vec<f64>,
    instance_bytes: Vec<f64>,
    posed: Vec<usize>,
    terrain_rebuilds: u32,
    feature_chunks: u32,
    unit_instances: usize,
}

impl Phase {
    fn print(&self, name: &str) {
        report(&format!("{name}: frame"), &self.frames, 0);
        report(&format!("{name}: renderer CPU"), &self.cpu, 0);
        report_bytes(&format!("{name}: bytes uploaded per frame"), &self.bytes);
        report_bytes(&format!("{name}: unit instance bytes per frame"), &self.instance_bytes);
        let posed_min = self.posed.iter().min().copied().unwrap_or(0);
        let posed_max = self.posed.iter().max().copied().unwrap_or(0);
        println!(
            "{name}: posed figures {posed_min}..{posed_max} of {} resting unit instances; terrain rebuilds {}, feature chunk rebuilds {}",
            self.unit_instances, self.terrain_rebuilds, self.feature_chunks
        );
    }
}

fn measure(run: &mut Offscreen, frames: usize) -> Phase {
    let mut phase = Phase::default();
    for _ in 0..frames {
        phase.frames.push(run.frame());
        let stats = run.app.renderer.stats();
        phase.cpu.push(f64::from(stats.prepare_ms));
        phase.bytes.push(stats.bytes_uploaded as f64);
        phase.instance_bytes.push(stats.instance_bytes as f64);
        phase.posed.push(stats.posed_figures);
        phase.terrain_rebuilds += stats.geometry_rebuilds;
        phase.feature_chunks += stats.feature_chunks_rebuilt;
        phase.unit_instances = stats.unit_instances;
    }
    phase
}

/// `count` horsemen of the player on free land around the map centre, each with a
/// two-tile walk it finishes this turn. Returns the centre and (unit, target) pairs.
fn stage_walkers(app: &mut GameApp, count: usize) -> Option<(u32, Vec<(classic_rules::ids::UnitHandle, u32)>)> {
    use classic_rules::ids::{PlayerId, TileId, UnitKindId};
    let session = app.module.session.as_mut()?;
    let rules = screens::game_rules();
    let horseman = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/horseman")? as u16);
    let (game, _) = session.harness_game();
    let dims = game.view().auth.map().dimensions;
    let center = TileId((dims.height / 2) * dims.width + dims.width / 2);
    let mut free = free_land(game, center, 12);
    free.sort_by_key(|t| dims.distance(center, *t));
    let mut units = Vec::new();
    for tile in free.iter().step_by(3).take(count) {
        units.push(game.debug_spawn_unit(&rules, PlayerId(0), *tile, horseman));
    }
    session.refresh_after_harness();
    let mut walks = Vec::new();
    // Targets are free now and stay free: no walker stands or ends on another's tile.
    let taken: Vec<u32> = free.iter().step_by(3).take(count).map(|t| t.0).collect();
    let mut used = Vec::new();
    for unit in units {
        session.select_unit(unit);
        let from = session.selected_tile()?;
        let open = |t: &u32| !taken.contains(t) && !used.contains(t);
        let target = free.iter().map(|t| t.0).filter(open).filter(|t| dims.distance(TileId(from), TileId(*t)) == Some(2)).find(|t| {
            let route = session.path_preview(Some(*t));
            matches!(route, Some(r) if r.this_turn == r.tiles.len())
        });
        if let Some(target) = target {
            used.push(target);
            walks.push((unit, target));
        }
    }
    Some((center.0, walks))
}

/// `--bench-ui`: the UI part of a frame on the heaviest screens — the Encyclopedia
/// contents and a search, the overviews of an empire of 100 cities, the map HUD with
/// their banners. Release, CPU only (the UI slice of the perf contract: <= 4 ms).
pub fn bench_ui() {
    use crate::ui::screens::{GameOptions, MapSize, MatchRequest, NewGame, OverviewTab, WorldSize};
    let mut run = Offscreen::new();
    let cities = env_number("AURELIA_BENCH_CITIES", 100) as usize;
    let frames = env_number("AURELIA_BENCH_FRAMES", 40) as usize;
    let new_game = NewGame {
        request: MatchRequest {
            civilization: "core:nation/rome".into(),
            seed: env_number("AURELIA_GAME_SEED", 20260928),
            size: MapSize::World(WorldSize::Standard),
            players: 8,
            water: 25,
        },
        options: GameOptions { map: classic_rules::scenario::MapScript::Pangaea, ..GameOptions::default() },
    };
    let mut prepared = screens::prepare_game(new_game).expect("world");
    let founded = found_cities(&mut prepared, cities);
    spawn_garrisons(&mut prepared, env_number("AURELIA_BENCH_UNITS", 300) as usize);
    start(&mut run.app, prepared);
    for _ in 0..3 {
        request(&mut run.app, GameRequest::EndTurn);
        run.app.finish_ai_turns();
    }
    let units = run.app.module.session.as_ref().map(|s| s.game().view().auth.indexes().units_of(classic_rules::ids::PlayerId(0)).len());
    println!("empire: {founded} cities, {} units, turn {}", units.unwrap_or(0), turn(&run.app));
    let pedia = crate::ui::pedia::game_pedia();
    let cases: [(&str, Box<dyn Fn(&mut GameApp)>); 8] = [
        ("map HUD (banners, top bar, notices)", Box::new(|app: &mut GameApp| app.screens.open(Screen::Map.into()))),
        ("Encyclopedia: contents, Units open", Box::new(move |app: &mut GameApp| {
            open_pedia(app, pedia.by_key("core:unit/swordsman"));
        })),
        ("Encyclopedia: Promotions open (214 pages)", Box::new(|app: &mut GameApp| {
            open_pedia(app, crate::ui::pedia::game_pedia().by_key("core:promotion/drill-1"));
        })),
        ("Encyclopedia: nation page", Box::new(|app: &mut GameApp| {
            open_pedia(app, crate::ui::pedia::game_pedia().by_key("core:nation/rome"));
        })),
        ("overview: cities", Box::new(|app: &mut GameApp| open_overview(app, OverviewTab::Cities))),
        ("overview: units", Box::new(|app: &mut GameApp| open_overview(app, OverviewTab::Units))),
        ("overview: demographics", Box::new(|app: &mut GameApp| open_overview(app, OverviewTab::Demographics))),
        ("overview: notifications", Box::new(|app: &mut GameApp| open_overview(app, OverviewTab::Notifications))),
    ];
    bench_view_rebuilds(&mut run, frames);
    bench_pedia(frames);
    for (name, open) in cases {
        open(&mut run.app);
        for _ in 0..10 {
            run.ui_frame();
        }
        let samples: Vec<f64> = (0..frames).map(|_| run.ui_frame()).collect();
        report(&format!("UI {name} ({} vertices, screen {:?})", run.ui_vertices, Screen::from(run.app.screens.screen)), &samples, 0);
    }
}

/// Time of the empire-wide views after a game change (built once per change, then cached).
fn bench_view_rebuilds(run: &mut Offscreen, count: usize) {
    let Some(session) = run.app.module.session.as_mut() else {
        return;
    };
    let mut overview = Vec::new();
    let mut extras = Vec::new();
    for _ in 0..count {
        session.invalidate_views();
        let start = Instant::now();
        std::hint::black_box(session.overview_view());
        overview.push(start.elapsed().as_secs_f64() * 1000.0);
        session.invalidate_views();
        let start = Instant::now();
        std::hint::black_box(session.hud_extras(None));
        extras.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    report("model: overview view rebuild (once per game change)", &overview, 0);
    report("model: HUD banners and breakdowns rebuild (once per game change)", &extras, 0);
}

/// The Encyclopedia's one-time build and a search per keystroke.
fn bench_pedia(count: usize) {
    let rules = screens::game_rules();
    let builds: Vec<f64> = (0..count.min(10))
        .map(|_| {
            let start = Instant::now();
            std::hint::black_box(crate::ui::pedia::Pedia::build(&rules));
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    report("model: Encyclopedia build (once per run)", &builds, 0);
    let pedia = crate::ui::pedia::game_pedia();
    let searches: Vec<f64> = (0..count)
        .map(|i| {
            let query = ["a", "ar", "gre", "sword", "e"][i % 5];
            let start = Instant::now();
            std::hint::black_box(pedia.search(query));
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    report("model: Encyclopedia search (per keystroke)", &searches, 0);
}

/// `count` warriors of player 0 spread over its cities (a harness army).
fn spawn_garrisons(prepared: &mut PreparedMatch, count: usize) {
    use classic_rules::ids::{PlayerId, UnitKindId};
    let rules = screens::game_rules();
    let Some(warrior) = rules.units.iter().position(|u| u.key == "core:unit/warrior") else {
        return;
    };
    let game = &mut prepared.game;
    let cities: Vec<_> = game.view().auth.indexes().cities_of(PlayerId(0)).to_vec();
    let tiles: Vec<_> = cities.iter().filter_map(|c| game.view().auth.cities().get(*c).map(|c| c.tile)).collect();
    if tiles.is_empty() {
        return;
    }
    for i in 0..count {
        game.debug_spawn_unit(&rules, PlayerId(0), tiles[i % tiles.len()], UnitKindId(warrior as u16));
    }
}

/// `count` cities of player 0 on land tiles three apart (a harness empire).
fn found_cities(prepared: &mut PreparedMatch, count: usize) -> usize {
    use classic_rules::ids::{PlayerId, TileId};
    let rules = screens::game_rules();
    let game = &mut prepared.game;
    let (width, height) = (prepared.world.width, prepared.world.height);
    let mut founded = 0;
    for r in (2..height - 2).step_by(3) {
        for q in (2..width - 2).step_by(3) {
            if founded >= count {
                return founded;
            }
            let tile = TileId(r * width + q);
            let land = classic_rules::navigation::cost::enter_cost_on_map(game.view().auth.map(), tile).is_some();
            let free = game.view().auth.indexes().city_on_tile(tile).is_none();
            if land && free {
                game.debug_found_city(&rules, PlayerId(0), tile);
                founded += 1;
            }
        }
    }
    founded
}

#[derive(Clone, Copy)]
enum Rebuild {
    CityGrows,
    ImprovementBuilt,
}

/// Applies `count` harness changes of one kind; returns refresh times, next-frame times
/// and the largest upload.
fn bench_rebuilds(run: &mut Offscreen, count: usize, kind: Rebuild) -> (Vec<f64>, Vec<f64>, u64) {
    use classic_rules::ids::{ImprovementId, TileId};
    let (mut refresh, mut frames, mut bytes) = (Vec::new(), Vec::new(), 0);
    let mut prepare = Vec::new();
    for i in 0..count {
        let Some(session) = run.app.module.session.as_mut() else {
            break;
        };
        let (game, rules) = session.harness_game();
        match kind {
            Rebuild::CityGrows => {
                let city = game.view().auth.cities().iter().map(|(h, _)| h).nth(i % 7);
                if let Some(city) = city {
                    game.debug_grow_city(&rules, city, 1);
                }
            }
            Rebuild::ImprovementBuilt => {
                let tiles = game.view().auth.map().tile_count() as u32;
                let tile = TileId((i as u32 * 7919) % tiles);
                let improvement = ImprovementId((i % 4) as u16 + 3);
                game.debug_set_improvement_batch(&rules, &[(tile, improvement)]);
            }
        }
        let start = Instant::now();
        session.refresh_after_harness();
        refresh.push(start.elapsed().as_secs_f64() * 1000.0);
        frames.push(run.frame());
        bytes = bytes.max(run.app.renderer.stats().bytes_uploaded);
        prepare.push(run.app.renderer.stats().prepare_ms as f64);
    }
    report("  of which renderer CPU (prepare to submit)", &prepare, 0);
    (refresh, frames, bytes)
}

fn print_stats(s: &render::RenderStats) {
    println!(
        "  draws {} (world {}, units {}), triangles: terrain {}, world {}, units {}, features {}; \
         instances: props {}, city pieces {}, unit figures {}; prepare {:.2} ms",
        s.draw_calls, s.world_draws, s.unit_draws, s.terrain_tris, s.world_tris, s.unit_tris, s.feature_tris,
        s.prop_instances, s.city_instances, s.unit_instances, s.prepare_ms
    );
}

fn env_pair(name: &str, default: (u32, u32)) -> (u32, u32) {
    let value = std::env::var(name).unwrap_or_default();
    match value.split_once('x').map(|(w, h)| (w.parse(), h.parse())) {
        Some((Ok(w), Ok(h))) => (w, h),
        _ => default,
    }
}

#[derive(Default)]
struct MoveSamples {
    commands: Vec<f64>,
    /// Frames after moves that explored no new tile.
    explored: Vec<f64>,
    explored_bytes: u64,
    /// Frames after moves that revealed new tiles; the terrain they reveal is built off
    /// the frame and published later.
    reveal: Vec<f64>,
    reveal_bytes: u64,
    /// The frames that published the revealed terrain, the slowest frame from the move
    /// to the publish, and the wall time from the move to the publish.
    publish: Vec<f64>,
    publish_bytes: u64,
    worst: Vec<f64>,
    latency: Vec<f64>,
}

fn explored_tiles(app: &GameApp) -> usize {
    app.view().explored.iter().filter(|e| **e == 1).count()
}

/// The settler walks back and forth between two tiles; turns end when it is out of moves.
/// After the first round trip every move stays in explored land.
fn bench_moves(run: &mut Offscreen, count: usize) -> MoveSamples {
    let mut samples = MoveSamples::default();
    let Some(unit) = run.app.module.session.as_ref().and_then(|s| s.selection().selected_unit()) else {
        return samples;
    };
    let home = run.app.module.session.as_ref().and_then(|s| s.selected_tile());
    let mut away = None;
    for _ in 0..count * 3 {
        if samples.commands.len() >= count {
            break;
        }
        let Some(session) = run.app.module.session.as_mut() else {
            break;
        };
        session.select_unit(unit);
        let here = session.selected_tile();
        let target = match (here == home, away) {
            (false, _) => home.filter(|_| can_move(&mut run.app)),
            (true, Some(tile)) => Some(tile).filter(|_| can_move(&mut run.app)),
            (true, None) => reachable_neighbour(&mut run.app),
        };
        let Some(target) = target else {
            request(&mut run.app, GameRequest::EndTurn);
            run.app.finish_ai_turns();
            run.frame();
            continue;
        };
        away = away.or(Some(target).filter(|_| here == home));
        let known = explored_tiles(&run.app);
        let start = Instant::now();
        run.app.click(Some(target), MapButton::Primary);
        samples.commands.push(start.elapsed().as_secs_f64() * 1000.0);
        let frame = run.frame();
        let bytes = run.app.renderer.stats().bytes_uploaded;
        if explored_tiles(&run.app) > known {
            samples.reveal.push(frame);
            samples.reveal_bytes = samples.reveal_bytes.max(bytes);
            let mut worst = frame;
            while run.app.renderer.stats().terrain_pending || run.app.renderer.stats().chunks_pending {
                let next = run.frame();
                worst = worst.max(next);
                let stats = run.app.renderer.stats();
                if stats.geometry_rebuilds > 0 && !stats.terrain_pending {
                    samples.publish.push(next);
                    samples.publish_bytes = samples.publish_bytes.max(stats.bytes_uploaded);
                }
            }
            samples.worst.push(worst);
            samples.latency.push(start.elapsed().as_secs_f64() * 1000.0);
        } else {
            samples.explored.push(frame);
            samples.explored_bytes = samples.explored_bytes.max(bytes);
        }
    }
    if let Some(session) = run.app.module.session.as_mut() {
        session.select_unit(unit);
    }
    samples
}

fn can_move(app: &mut GameApp) -> bool {
    let Some(orders) = app.module.session.as_mut().and_then(|s| s.unit_orders()) else {
        return false;
    };
    orders
        .actions
        .iter()
        .any(|a| a.action == Action::Move && a.availability == Availability::Ready)
}

fn reachable_neighbour(app: &mut GameApp) -> Option<u32> {
    let session = app.module.session.as_mut()?;
    let orders = session.unit_orders()?;
    let moves = orders.actions.iter().find(|a| a.action == Action::Move)?;
    if moves.availability != Availability::Ready {
        return None;
    }
    let width = session.view().width;
    let from = orders.tile;
    let candidates = [
        from + 1,
        from.wrapping_sub(1),
        from + width,
        from.wrapping_sub(width),
    ];
    candidates.into_iter().find(|tile| {
        let route = session.path_preview(Some(*tile));
        matches!(route, Some(r) if r.this_turn == r.tiles.len())
    })
}

fn bench_turns(run: &mut Offscreen, count: usize) -> (Vec<f64>, Vec<f64>, u64) {
    let (mut commands, mut frames, mut bytes) = (Vec::new(), Vec::new(), 0);
    for _ in 0..count {
        let start = Instant::now();
        request(&mut run.app, GameRequest::EndTurn);
        run.app.finish_ai_turns();
        commands.push(start.elapsed().as_secs_f64() * 1000.0);
        frames.push(run.frame());
        bytes = bytes.max(run.app.renderer.stats().bytes_uploaded);
    }
    (commands, frames, bytes)
}

/// Frames spent settling after a camera change: count, worst frame, worst renderer
/// CPU and total time.
fn report_settle(name: &str, frames: &[(f64, f64)]) {
    let worst = frames.iter().map(|f| f.0).fold(0.0, f64::max);
    let cpu = frames.iter().map(|f| f.1).fold(0.0, f64::max);
    let total: f64 = frames.iter().map(|f| f.0).sum();
    println!("{name}: {} frames, worst {worst:.2} ms (renderer CPU worst {cpu:.2} ms), total {total:.0} ms", frames.len());
}

fn report_bytes(name: &str, samples: &[f64]) {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let Some(max) = sorted.last() else {
        return;
    };
    let median = sorted[sorted.len() / 2];
    let p95 = sorted[(sorted.len() - 1) * 95 / 100];
    println!("{name}: n={} median {median:.0} B, p95 {p95:.0} B, max {max:.0} B", sorted.len());
}

fn report(name: &str, samples: &[f64], bytes: u64) {
    if samples.is_empty() {
        println!("{name}: no samples");
        return;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    let p95 = sorted[(sorted.len() - 1) * 95 / 100];
    let upload = if bytes > 0 {
        format!(", max upload {bytes} B ({:.2} MB)", bytes as f64 / 1e6)
    } else {
        String::new()
    };
    println!(
        "{name}: n={} median {median:.2} ms, p95 {p95:.2} ms{upload}",
        sorted.len()
    );
}

pub(super) fn turn(app: &GameApp) -> u32 {
    app.module.session.as_ref().map(|s| s.turn()).unwrap_or(0)
}

pub(super) fn hash(app: &GameApp) -> u64 {
    app.module.session.as_ref().map(|s| s.world_hash()).unwrap_or(0)
}

pub(super) fn env_number(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// AURELIA_GAME_ERAS: classic era index of players 0..=3 (default 7,1,3,5).
fn env_eras() -> [u8; 4] {
    let mut eras = [7, 1, 3, 5];
    if let Ok(value) = std::env::var("AURELIA_GAME_ERAS") {
        for (slot, part) in eras.iter_mut().zip(value.split(',')) {
            if let Ok(era) = part.trim().parse() {
                *slot = era;
            }
        }
    }
    eras
}

fn env_size() -> (u32, u32) {
    let value = std::env::var("AURELIA_VISUAL_SIZE").unwrap_or_default();
    match value.split_once('x').map(|(w, h)| (w.parse(), h.parse())) {
        Some((Ok(w), Ok(h))) => (w, h),
        _ => (1664, 936),
    }
}

fn env_pointer() -> Option<egui::Pos2> {
    let value = std::env::var("AURELIA_VISUAL_HOVER_PIXEL").ok()?;
    let (x, y) = value.split_once(',')?;
    Some(egui::pos2(x.parse().ok()?, y.parse().ok()?))
}

/// A widget's request, as the UI would send it, then what it asks of the shell.
pub(super) fn request(app: &mut GameApp, request: GameRequest) {
    app.module.request(request);
    app.apply_module_actions();
}

/// The match starts as when the setup screen made it.
pub(super) fn start(app: &mut GameApp, prepared: PreparedMatch) {
    app.module.start(prepared);
    app.apply_module_actions();
}

/// The Encyclopedia at a page, as its links open it.
pub(super) fn open_pedia(app: &mut GameApp, page: Option<crate::ui::pedia::PediaRef>) {
    app.module.screens.screen = app.screens.screen.into();
    app.module.screens.open_pedia(page);
    app.apply_module_actions();
}

fn open_overview(app: &mut GameApp, tab: crate::ui::screens::OverviewTab) {
    app.module.screens.open_overview(tab);
    app.apply_module_actions();
}
