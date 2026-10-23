//! Showcase bench for visuals and UI: real wgpu world, deterministic demo scene and optional
//! PNG readback. It never runs the game: "Create world" only previews a generated map, and
//! city/research/policy screens show local demo data. The game is the `desktop` binary.
use render::{
    FrameFlags, camera::CameraController, gpu::context::GpuCore, renderer::Renderer,
    world::state::RenderWorld,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};
use client::shell::capture::save_capture;
use client::shell::hover::{HoverCache, HoverKey, hover_allowed};
use client::shell::module::{ModuleUi, ShellAction};
use client::shell::{hud as visual_hud, screens as visual_screens};
use classic_ui::dev::visual_scene;
use classic_ui::ui::screens::Screen;
use classic_ui::ui::unit_kinds::UnitKinds;
use classic_ui::ClassicUi as Module;
use launcher_core::audio as visual_audio;
mod visibility;
mod visual_visibility;


struct Study {
    world: RenderWorld,
    view: RenderWorld,
    shifted: bool,
    camera: CameraController,
    renderer: Renderer,
    ctx: egui::Context,
    ui_renderer: egui_wgpu::Renderer,
    hud: visual_hud::VisualHudState,
    screens: visual_screens::ScreenState,
    /// The game module's screens and panels over the hand-built scene.
    module: Module,
    map_camera: Option<CameraController>,
    hover_cache: HoverCache,
    dragging: bool,
    atlas_review: Option<String>,
    /// A generated world replaced the scene (New game previews it).
    previewing: bool,
}
impl Study {
    fn new(core: &GpuCore, format: wgpu::TextureFormat) -> Self {
        let biomes = std::env::var("AURELIA_VISUAL_SCENE").as_deref() == Ok("biomes");
        let mut world = if biomes { visual_scene::biomes() } else { visual_scene::build() };
        if std::env::var("AURELIA_VISUAL_UNITS").as_deref() == Ok("occlusion") {
            visual_scene::place_occlusion_study(&mut world);
        }
        let mode = std::env::var("AURELIA_VISUAL_VISIBILITY").unwrap_or_default();
        let shifted = mode == "shifted";
        if !biomes {
            visual_visibility::apply(&mut world, shifted, mode == "revealed");
        }
        if mode == "remembered" {
            world.visible.fill(0);
        }
        let view = visual_visibility::projection(&world);
        let mut camera = CameraController::fit_map(world.width, world.height);
        if !biomes {
            let (x, z) = visual_scene::center();
            camera.x = x + 0.4;
            camera.y = z;
            camera.zoom = 2.25;
        }
        if let Ok(s) = std::env::var("AURELIA_VISUAL_ZOOM") {
            if let Ok(z) = s.parse() {
                camera.zoom = z;
            }
        }
        if let Ok(value) = std::env::var("AURELIA_VISUAL_CENTER") {
            if let Some((x, z)) = value.split_once(',') {
                camera.x = x.parse().expect("camera X");
                camera.y = z.parse().expect("camera Z");
            }
        }
        let layers = classic_ui::terrain_layers();
        let mut renderer = Renderer::with_gpu(core, format, &layers);
        let rules = visual_scene::rules();
        renderer.set_unit_models(render::unit_models::UnitModelBook::from_rules(&rules));
        renderer.set_world_models(render::world_models::WorldBook::from_rules(&rules));
        if let Ok(value) = std::env::var("AURELIA_VISUAL_ANIMATION_TIME") {
            renderer.set_animation_time_override(value.parse().ok());
        }
        let ctx = egui::Context::default();
        visual_hud::theme(&ctx);
        let ui_renderer = egui_wgpu::Renderer::new(&core.device, format, Default::default());
        let mut hud = visual_hud::VisualHudState::default();
        hud.turn = 42;
        hud.show_grid = std::env::var("AURELIA_VISUAL_GRID").as_deref() != Ok("off");
        hud.show_yields = std::env::var("AURELIA_VISUAL_YIELDS").as_deref() != Ok("off");
        let kinds = UnitKinds::from_rules(&rules);
        // A content key, e.g. AURELIA_VISUAL_SELECT_UNIT=core:unit/settler.
        let selected = std::env::var("AURELIA_VISUAL_SELECT_UNIT").ok().and_then(|key| kinds.kind_of(&key));
        let mut module = Module::showcase(kinds);
        hud.unit_kinds = module.unit_kinds();
        if let Some(kind) = selected
        {
            hud.selected_tile = view
                .units
                .iter()
                .find(|u| u.owner == view.observer && u.kind == kind)
                .map(|u| u.tile.0);
        }
        let mut screens = visual_screens::ScreenState::default();
        let screen = match std::env::var("AURELIA_VISUAL_SCREEN")
            .unwrap_or_default()
            .as_str()
        {
            "menu" => Screen::Menu,
            "setup" => Screen::MatchSetup,
            "settings" => Screen::Settings,
            "city" => Screen::City,
            "research" => Screen::Research,
            "civics" => Screen::Civics,
            "civilizations" => Screen::Civilizations,
            "map" => Screen::Map,
            _ if std::env::args().any(|a| a == "--screenshot") => Screen::Map,
            _ => Screen::Menu,
        };
        screens.screen = screen.into();
        module.screens.sync(screens.flags());
        hud.dev_mode = screens.dev_mode;
        hud.dev_mode = true; // The authored Rivermere fixture uses sample economy and research.
        if let Ok(slug) = std::env::var("AURELIA_VISUAL_NATION") {
            module.screens.select_civilization(&slug);
        }
        if screens.dev_mode {
            if let Ok(group) = std::env::var("AURELIA_VISUAL_POLICY_ART") {
                if module.screens.select_policy_art(&group) {
                    screens.open(Screen::Civics.into());
                }
            }
        }
        Self {
            world,
            view,
            shifted,
            camera,
            renderer,
            ctx,
            ui_renderer,
            hud,
            screens,
            module,
            map_camera: None,
            hover_cache: Default::default(),
            dragging: false,
            atlas_review: None,
            previewing: false,
        }
    }
    /// What the module asked this frame: screen switches, and a generated world to
    /// preview in place of the scene.
    fn apply_module_actions(&mut self) {
        for action in self.module.take_actions() {
            match action {
                ShellAction::Open(screen) => self.screens.open(screen),
                ShellAction::MatchStarted { summary } => self.preview_match(summary),
                _ => {}
            }
        }
    }
    fn preview_match(&mut self, summary: String) {
        let Some(world) = self.module.view() else {
            return;
        };
        // A replaced world continues the revisions the renderer has seen.
        let mut world = world.clone();
        world.layers = self.view.layers;
        world.touch_all();
        let start = world.units.iter().find(|unit| unit.owner == world.observer).map(|unit| unit.tile.0);
        self.hud.selected_tile = start;
        self.hud.hovered_tile = None;
        self.hover_cache = Default::default();
        self.map_camera = None;
        self.hud.dev_mode = false;
        self.hud.turn = self.module.turn().saturating_sub(1);
        self.camera = CameraController::fit_map(world.width, world.height);
        self.camera.zoom = 2.25;
        if let Some(tile) = start {
            let (x, z) = render::scene::layout::tile_to_world(tile % world.width, tile / world.width);
            self.camera.x = x;
            self.camera.y = z;
        }
        self.screens.match_ready(summary);
        self.world = world.clone();
        self.view = world;
        self.previewing = true;
    }
    fn sync_city_camera(&mut self) {
        if self.screens.screen == Screen::City.into() {
            if self.map_camera.is_none() {
                self.map_camera = Some(self.camera.clone());
                if let Some(city) = self.view.cities.first() {
                    let (x, z) = render::scene::layout::tile_to_world(
                        city.tile.0 % self.view.width,
                        city.tile.0 / self.view.width,
                    );
                    self.camera.x = x;
                    self.camera.y = z;
                    self.camera.zoom = 3.0;
                }
            }
        } else if let Some(camera) = self.map_camera.take() {
            self.camera = camera;
        }
    }
    fn change_visibility(&mut self) {
        self.shifted = !self.shifted;
        visual_visibility::apply(&mut self.world, self.shifted, false);
        self.view = visual_visibility::projection(&self.world);
    }
    fn frame(
        &mut self,
        core: &GpuCore,
        target: &wgpu::TextureView,
        size: (u32, u32),
        raw: egui::RawInput,
    ) -> egui::PlatformOutput {
        if !self.module.creating_match()
            && raw.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        pressed: true,
                        repeat: false,
                        ..
                    }
                )
            })
        {
            let shown = self.screens.screen;
            self.screens.screen = if shown == Screen::Map.into() {
                Screen::Menu.into()
            } else if self.screens.has_session || self.screens.dev_mode {
                Screen::Map.into()
            } else {
                Screen::Menu.into()
            };
        }
        self.module.update();
        self.apply_module_actions();
        self.sync_city_camera();
        let flags = FrameFlags {
            grid: self.hud.show_grid,
            selected_city: None,
            yields: false,
        };
        let map_visible = visual_screens::wants_world(self.screens.screen, &self.module);
        if map_visible {
            self.renderer
                .draw(core, &self.view, &self.camera, target, size, flags);
        }
        let ctx = self.ctx.clone();
        let mut actions = visual_hud::VisualHudActions::default();
        let mut output = ctx.run_ui(raw, |ui| {
            if let Some(sheet) = &self.atlas_review {
                self.screens.draw_atlas_review(ui, sheet);
                return;
            }
            if !visual_screens::draw(ui.ctx(), &mut self.screens, &mut self.module, Some(&self.view)) {
                actions = visual_hud::draw(
                    ui.ctx(),
                    Some(&self.view),
                    &self.camera,
                    &mut self.hud,
                    self.renderer.visual_surface(),
                    &mut self.module,
                );
                if self.hud.show_settings {
                    self.hud.show_settings = false;
                    self.screens.open(client::shell::module::Screen::Settings);
                }
                visual_screens::draw_navigation(ui.ctx(), &mut self.screens, &mut self.module);
            }
            let key = if hover_allowed(
                ui.ctx(),
                self.screens.screen,
                self.dragging,
                self.hud.hover_over_badge,
            ) {
                ui.ctx().pointer_hover_pos().map(|pointer| {
                    HoverKey::new(
                        pointer,
                        ui.ctx().pixels_per_point(),
                        size,
                        &self.camera,
                        &self.view,
                    )
                })
            } else {
                None
            };
            self.hud.hovered_tile = self.hover_cache.resolve(key, |key| {
                self.renderer
                    .pick_tile(
                        &self.view,
                        &self.camera,
                        key.physical_pointer(),
                        (size.0 as f32, size.1 as f32),
                    )
                    .map(|tile| tile.0)
            });
            self.module.draw_tile_hover(ui.ctx(), Some(&self.view), self.hud.hovered_tile);
        });
        self.module.after_ui(&ctx, self.hud.hovered_tile, self.screens.flags());
        self.apply_module_actions();
        if actions.toggle_grid {
            self.hud.show_grid = !self.hud.show_grid;
        }
        if let Some((x, z)) = actions.camera_target {
            self.camera.x = x;
            self.camera.y = z;
        }
        // egui output does not carry closure return; draw actions are handled via state/input below.
        // Drained, not borrowed: egui checks in debug builds that no change is dropped.
        for (id, deltas) in output.textures_delta.set.drain() {
            for delta in &deltas {
                self.ui_renderer
                    .update_texture(&core.device, &core.queue, id, delta);
            }
        }
        let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.0, size.1],
            pixels_per_point: output.pixels_per_point,
        };
        let mut enc = core.device.create_command_encoder(&Default::default());
        let callbacks =
            self.ui_renderer
                .update_buffers(&core.device, &core.queue, &mut enc, &jobs, &screen);
        {
            let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("visual HUD"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if map_visible {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.ui_renderer
                .render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        core.queue
            .submit(callbacks.into_iter().chain(std::iter::once(enc.finish())));
        for id in output.textures_delta.free.drain() {
            self.ui_renderer.free_texture(&id);
        }
        output.platform_output
    }
}
fn screenshot(path: &str) {
    let core = GpuCore::new_blocking().expect("GPU");
    println!("adapter {:?}", core.adapter_info());
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (mut w, mut h) = (1664, 936);
    if let Ok(s) = std::env::var("AURELIA_VISUAL_SIZE") {
        if let Some((a, b)) = s.split_once('x') {
            w = a.parse().unwrap();
            h = b.parse().unwrap();
        }
    }
    let mut study = Study::new(&core, format);
    let tex = core.texture(render::gpu::memory::Category::RenderTargets, &wgpu::TextureDescriptor {
        label: Some("visual capture"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = tex.create_view(&Default::default());
    if std::env::var("AURELIA_VISUAL_NEW_GAME").as_deref() == Ok("1") {
        assert!(study.module.screens.request_new_match());
        let request = study.module.screens.take_match_request().expect("a new game request");
        let prepared = classic_ui::ui::screens::prepare_game(request).expect("headless world creation");
        study.module.start(prepared);
        study.apply_module_actions();
        assert!(study.previewing, "headless world creation failed");
    }

    let scale: f32 = std::env::var("AURELIA_VISUAL_SCALE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    study.ctx.set_pixels_per_point(scale);
    let frames: usize = std::env::var("AURELIA_VISUAL_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    study.module.hud.preload_action_art(&study.ctx);
    study.module.screens.preload_policy_art(&study.ctx);
    study.screens.preload_screen_art(&study.ctx, &mut study.module);
    // This review route is intentionally headless-only, outside player navigation.
    study.atlas_review = std::env::var("AURELIA_VISUAL_ATLAS_REVIEW").ok();
    if let Some(sheet) = &study.atlas_review {
        study.screens.preload_atlas_review(&study.ctx, sheet);
    }
    let sequence = std::env::var("AURELIA_VISUAL_SEQUENCE").ok();
    if let Some(dir) = &sequence {
        std::fs::create_dir_all(dir).expect("capture directory");
    }
    let hover_pixel = std::env::var("AURELIA_VISUAL_HOVER_PIXEL").ok().and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some(egui::pos2(x.parse::<f32>().ok()?, y.parse::<f32>().ok()?))
    });
    let mut timings = Vec::new();
    for i in 0..frames {
        if sequence.is_some() {
            study
                .renderer
                .set_animation_time_override(Some(i.saturating_sub(3) as f32 / 12.0));
        }
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(w as f32 / scale, h as f32 / scale),
            )),
            time: Some(i as f64),
            events: hover_pixel
                .map(|p| vec![egui::Event::PointerMoved(p / scale)])
                .unwrap_or_default(),
            ..Default::default()
        };
        let start = Instant::now();
        study.frame(&core, &target, (w, h), raw);
        core.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(30)),
            })
            .unwrap();
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        if i >= 3 {
            timings.push(elapsed);
            if let Some(dir) = &sequence {
                save_capture(&core, &tex, w, h, &format!("{dir}/frame-{:04}.png", i - 3));
            }
        }
        if i < 3 {
            println!("frame {i}: {elapsed:.2}ms");
            if i == 0 {
                println!("scene {:?}", study.renderer.stats());
            }
        }
    }
    println!("steady scene {:?}", study.renderer.stats());
    if hover_pixel.is_some() {
        println!("hovered tile {:?}", study.hud.hovered_tile);
    }
    if !timings.is_empty() {
        timings.sort_by(f64::total_cmp);
        println!(
            "{} warm frames, median {:.2}ms, p95 {:.2}ms (CPU submit + GPU wait, offscreen)",
            timings.len(),
            timings[timings.len() / 2],
            timings[(timings.len() - 1) * 95 / 100]
        );
    }
    save_capture(&core, &tex, w, h, path);
}

fn music_mode_for_screen(
    screens: &visual_screens::ScreenState,
    module: &Module,
    previous: visual_audio::MusicMode,
) -> visual_audio::MusicMode {
    screens.music(previous, module)
}

#[cfg(test)]
mod music_routing_tests {
    use super::*;
    #[test]
    fn game_overlays_keep_game_music_and_art_preview_preserves_context() {
        use visual_audio::MusicMode;
        let module = Module::showcase(UnitKinds::default());
        let mut screens = visual_screens::ScreenState::default();
        let mut music = |screen: Screen, previous| {
            screens.screen = screen.into();
            music_mode_for_screen(&screens, &module, previous)
        };
        for screen in [Screen::Map, Screen::City, Screen::Research, Screen::Civics] {
            assert_eq!(music(screen, MusicMode::Game), MusicMode::Game);
        }
        for mode in [MusicMode::Menu, MusicMode::Game] {
            assert_eq!(music(Screen::Civilizations, mode), mode);
        }
        assert_eq!(music(Screen::Menu, MusicMode::Game), MusicMode::Menu);
    }
}


struct Desktop {
    window: Arc<Window>,
    core: GpuCore,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    study: Study,
    input: ui_input::State,
    cursor: (f64, f64),
    drag: bool,
    down: (f64, f64),
    music: visual_audio::MusicPlayer,
    music_mode: visual_audio::MusicMode,
}
impl Desktop {
    fn new(window: Arc<Window>) -> Self {
        let core = GpuCore::new_blocking().unwrap();
        let surface = core.instance.create_surface(window.clone()).unwrap();
        let caps = surface.get_capabilities(&core.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&core.device, &config);
        let study = Study::new(&core, format);
        let input = ui_input::State::new(study.ctx.clone(), window.scale_factor() as f32);
        Self {
            window,
            core,
            surface,
            config,
            study,
            input,
            cursor: (0., 0.),
            drag: false,
            down: (0., 0.),
            music: visual_audio::MusicPlayer::new(),
            music_mode: visual_audio::MusicMode::Menu,
        }
    }
    fn resize(&mut self, w: u32, h: u32) {
        if w > 0 && h > 0 {
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(&self.core.device, &self.config);
        }
    }
    fn draw(&mut self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => return,
        };
        let view = frame.texture.create_view(&Default::default());
        let raw = self.input.take_egui_input(&self.window);
        self.study.dragging = self.drag;
        let out = self.study.frame(
            &self.core,
            &view,
            (self.config.width, self.config.height),
            raw,
        );
        self.input.handle_platform_output(&self.window, out);
        self.sync_music();
        self.core.queue.present(frame);
    }
    fn sync_music(&mut self) {
        self.music_mode = music_mode_for_screen(&self.study.screens, &self.study.module, self.music_mode);
        self.music.set_volume(self.study.screens.music_volume());
        self.music.set_muted(self.study.screens.music_muted());
        self.music.set_mode(self.music_mode);
        self.music.update();
        self.study
            .screens
            .set_music_error(self.music.unavailable_reason().map(str::to_owned));
    }
    fn pick(&self) -> Option<u32> {
        self.study
            .renderer
            .pick_tile(
                &self.study.view,
                &self.study.camera,
                (self.cursor.0 as f32, self.cursor.1 as f32),
                (self.config.width as f32, self.config.height as f32),
            )
            .map(|t| t.0)
    }
}
#[derive(Default)]
struct App {
    desktop: Option<Desktop>,
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.desktop.is_none() {
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_title("Aurelia · Low-poly visual sandbox")
                            .with_inner_size(winit::dpi::LogicalSize::new(1440., 900.)),
                    )
                    .unwrap(),
            );
            self.desktop = Some(Desktop::new(window));
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(d) = self.desktop.as_mut() else {
            return;
        };
        let response = d.input.on_window_event(&d.window, &event);
        let map_input = d.study.screens.screen == Screen::Map.into();
        if !map_input {
            d.drag = false;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(s) => d.resize(s.width, s.height),
            WindowEvent::RedrawRequested => {
                d.draw();
                if d.study.screens.quit_requested {
                    event_loop.exit();
                    return;
                }
                if visual_screens::wants_world(d.study.screens.screen, &d.study.module) || d.study.ctx.has_requested_repaint() {
                    d.window.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } | WindowEvent::Focused(false) => {
                d.drag = false;
                d.study.dragging = false;
                d.study.hud.hovered_tile = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if map_input && d.drag && !response.consumed {
                    d.study.camera.pan_by(
                        (position.x - d.cursor.0) as f32,
                        (position.y - d.cursor.1) as f32,
                    );
                }
                d.cursor = (position.x, position.y);
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if state == ElementState::Pressed && map_input && !response.consumed {
                    d.drag = true;
                    d.down = d.cursor;
                } else if state == ElementState::Released {
                    let moved = (d.cursor.0 - d.down.0).abs() + (d.cursor.1 - d.down.1).abs();
                    if d.drag && moved < 5.0 {
                        d.study.hud.selected_tile = d.pick();
                    }
                    d.drag = false;
                }
            }
            WindowEvent::MouseWheel { delta, .. } if map_input && !response.consumed => {
                let n = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 80.,
                };
                d.study.camera.zoom_by(1.0 + n * 0.1);
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::F1) if !d.study.module.creating_match() => {
                        d.study.screens.screen = Screen::Menu.into()
                    }
                    PhysicalKey::Code(KeyCode::F2)
                        if d.study.screens.dev_mode && !d.study.module.creating_match() =>
                    {
                        d.study.screens.screen = Screen::City.into()
                    }
                    PhysicalKey::Code(KeyCode::F3)
                        if d.study.screens.dev_mode && !d.study.module.creating_match() =>
                    {
                        d.study.screens.screen = Screen::Research.into()
                    }
                    PhysicalKey::Code(KeyCode::F4)
                        if d.study.screens.dev_mode && !d.study.module.creating_match() =>
                    {
                        d.study.screens.screen = Screen::Civics.into()
                    }
                    PhysicalKey::Code(KeyCode::F5)
                        if d.study.screens.dev_mode && !d.study.module.creating_match() =>
                    {
                        d.study.screens.screen = Screen::Civilizations.into()
                    }
                    PhysicalKey::Code(KeyCode::KeyG) if map_input && !response.consumed => {
                        d.study.hud.show_grid = !d.study.hud.show_grid
                    }
                    PhysicalKey::Code(KeyCode::KeyV)
                        if map_input
                            && d.study.screens.dev_mode
                            && !d.study.previewing
                            && !response.consumed =>
                    {
                        d.study.change_visibility()
                    }
                    PhysicalKey::Code(KeyCode::Home) if map_input && !response.consumed => {
                        let (x, z) = if d.study.previewing {
                            d.study
                                .view
                                .units
                                .iter()
                                .find(|unit| unit.owner == d.study.view.observer)
                                .map(|unit| {
                                    render::scene::layout::tile_to_world(
                                        unit.tile.0 % d.study.view.width,
                                        unit.tile.0 / d.study.view.width,
                                    )
                                })
                                .unwrap_or((d.study.camera.x, d.study.camera.y))
                        } else {
                            visual_scene::center()
                        };
                        d.study.camera.x = x;
                        d.study.camera.y = z;
                        d.study.camera.zoom = 2.25;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        if response.repaint {
            d.window.request_redraw();
        }
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--screenshot") {
        screenshot(
            args.get(2)
                .map(String::as_str)
                .unwrap_or("output/visual-sandbox.png"),
        );
        return;
    }
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut App::default()).unwrap();
}
