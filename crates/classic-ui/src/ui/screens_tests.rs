//! The classic screens in the shell's screen stack: the shell's state (shown screen, Empire
//! menu, settings) persists between frames like in the app, the module's screens draw in it.
use super::*;
use crate::ui::unit_kinds::UnitKinds;
use crate::ClassicUi;
use client::shell::module::ModuleUi;
use client::shell::screens::{self as shell_screens, ScreenState as ShellState};

struct Harness {
    shell: ShellState,
    ui: ClassicUi,
}

impl Harness {
    fn new(screen: Screen) -> Self {
        let mut shell = ShellState::default();
        shell.screen = screen.into();
        Self { shell, ui: ClassicUi::showcase(UnitKinds::default()) }
    }

    fn screen(&self) -> Screen {
        self.shell.screen.into()
    }

    fn state(&mut self) -> &mut ScreenState {
        &mut self.ui.screens
    }

    /// One frame at `size`; the shell applies what the module asked after it.
    fn frame_at(&mut self, ctx: &egui::Context, size: egui::Vec2, events: Vec<egui::Event>) -> (bool, egui::FullOutput) {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default() };
        let world = RenderWorld::default();
        let mut modal = false;
        let mut output = ctx.run_ui(raw, |ui| {
            modal = shell_screens::draw(ui.ctx(), &mut self.shell, &mut self.ui, Some(&world));
        });
        // The synthetic harness has no GPU backend to consume texture uploads.
        output.textures_delta.clear();
        self.apply();
        (modal, output)
    }

    fn frame(&mut self, ctx: &egui::Context, events: Vec<egui::Event>) -> bool {
        self.frame_at(ctx, vec2(1366.0, 768.0), events).0
    }

    fn apply(&mut self) {
        for action in self.ui.take_actions() {
            if let ShellAction::Open(screen) = action {
                self.shell.open(screen);
            }
        }
    }

    fn click(&mut self, ctx: &egui::Context, position: egui::Pos2) -> bool {
        self.frame(ctx, vec![egui::Event::PointerMoved(position), press(position, true)]);
        self.frame(ctx, vec![press(position, false)])
    }
}

fn press(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

#[test]
fn compact_navigation_opens_real_sections_at_wide_and_narrow_sizes() {
    for width in [1366., 960., 480.] {
        let ctx = egui::Context::default();
        let mut h = Harness::new(Screen::Map);
        h.shell.game_mode = true;
        h.shell.has_session = true;
        let frame = |h: &mut Harness, events| {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 640.))), events, ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| shell_screens::draw_navigation(ui.ctx(), &mut h.shell, &mut h.ui));
            output.textures_delta.clear();
            output
        };
        let click_at = |h: &mut Harness, at| {
            for pressed in [true, false] {
                frame(h, vec![egui::Event::PointerMoved(at), press(at, pressed)]);
            }
        };
        frame(&mut h, vec![]);
        frame(&mut h, vec![]);
        click_at(&mut h, pos2(width - 84., 68.));
        assert!(h.shell.navigation_is_open());
        frame(&mut h, vec![]);
        let output = frame(&mut h, vec![]);
        let at = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(t) if t.galley.text() == "Research" => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        }).expect("research is a visible navigation entry");
        assert!(at.x > 0. && at.x < width && at.y > 40. && at.y < 640.);
        click_at(&mut h, at);
        assert_eq!(h.screen(), Screen::Research);
        assert!(!h.shell.navigation_is_open());
        h.shell.screen = Screen::Map.into();
        h.shell.capture_navigation(true);
        frame(&mut h, vec![]);
        frame(&mut h, vec![]);
        click_at(&mut h, pos2(20., 300.));
        assert!(!h.shell.navigation_is_open(), "outside click dismisses the panel");
        assert_eq!(h.screen(), Screen::Map);
    }
}

#[test]
fn illustrated_menu_opens_setup_and_created_world_can_resume() {
    let ctx = egui::Context::default();
    let mut h = Harness::new(Screen::Menu);
    h.shell.dev_mode = false;
    h.frame(&ctx, vec![]);
    h.frame(&ctx, vec![]);
    h.click(&ctx, pos2(200., 307.));
    assert_eq!(h.screen(), Screen::Map);
    h.shell.open(Screen::Menu.into());
    h.frame(&ctx, vec![]);
    h.click(&ctx, pos2(200., 365.));
    assert_eq!(h.screen(), Screen::MatchSetup);
    h.frame(&ctx, vec![]);
    h.click(&ctx, pos2(1150., 690.));
    let mut request = h.state().take_match_request().expect("one creation request");
    assert!(h.state().take_match_request().is_none());
    assert!(!h.state().request_new_match());
    request.request.size = MapSize::World(WorldSize::Duel);
    let prepared = prepare_game(request).unwrap();
    h.state().match_ready();
    h.shell.match_ready(format!("{} world", prepared.civilization));
    assert!(h.shell.has_session);
    assert_eq!(h.screen(), Screen::Map);
    h.shell.open(Screen::Menu.into());
    h.frame(&ctx, vec![]);
    h.click(&ctx, pos2(200., 307.));
    assert_eq!(h.screen(), Screen::Map);
}

#[test]
fn city_widgets_select_queue_and_return_to_map() {
    let ctx = egui::Context::default();
    let mut h = Harness::new(Screen::City);
    h.shell.dev_mode = true;
    h.frame(&ctx, vec![]);
    h.frame(&ctx, vec![]);
    h.frame(&ctx, vec![]);
    h.click(&ctx, pos2(1150.0, 222.0));
    assert_eq!(h.state().production, 1, "clicking the Warrior row changes selected production");
    h.click(&ctx, pos2(1150.0, 511.0));
    assert_eq!(h.state().queue, vec![0, 1], "queue button appends the selected unit");
    assert!(h.click(&ctx, pos2(1274.0, 33.0)), "closing frame remains modal");
    assert_eq!(h.screen(), Screen::Map);
    assert!(!h.frame(&ctx, vec![]));
}

#[test]
fn scaled_city_canvas_keeps_production_visible_and_pointer_coordinates_correct() {
    let ctx = egui::Context::default();
    let mut h = Harness::new(Screen::City);
    let size = vec2(1366. / 1.5, 768. / 1.5);
    let body = Rect::from_min_max(pos2(24., 80.), pos2(size.x - 24., size.y - 37.));
    let scale = (body.width() / 1150.).min(body.height() / 640.);
    let canvas = body.size() / scale;
    let point = body.min + vec2(canvas.x - 160., 142.) * scale;
    assert!(body.contains(point));
    h.frame_at(&ctx, size, vec![]);
    h.frame_at(&ctx, size, vec![]);
    h.frame_at(&ctx, size, vec![egui::Event::PointerMoved(point), press(point, true)]);
    h.frame_at(&ctx, size, vec![press(point, false)]);
    assert_eq!(h.state().production, 1, "scaled Warrior row remains clickable");
}

#[test]
fn real_policy_button_spends_one_choice_and_cannot_repeat_adoption() {
    let ctx = egui::Context::default();
    let mut h = Harness::new(Screen::Civics);
    h.shell.dev_mode = true;
    h.frame(&ctx, vec![]);
    h.frame(&ctx, vec![]);
    h.click(&ctx, pos2(1198.0, 696.0));
    assert!(h.state().adopted.contains(&0));
    assert_eq!(h.state().policy_points, 1);
    h.click(&ctx, pos2(1198.0, 696.0));
    assert_eq!(h.state().policy_points, 1);
}

#[test]
fn city_keeps_world_visible_and_citizen_focus_interactive() {
    assert!(Screen::City.wants_world());
    assert!(Screen::Map.wants_world());
    assert!(!Screen::Menu.wants_world());
    assert!(!Screen::Research.wants_world());
    assert!(!Screen::Civics.wants_world());
    let ctx = egui::Context::default();
    let mut h = Harness::new(Screen::City);
    h.shell.dev_mode = true;
    h.frame(&ctx, vec![]);
    h.frame(&ctx, vec![]);
    h.click(&ctx, pos2(160.0, 471.0));
    assert_eq!(h.state().focus, 1, "citizen focus remains reachable beside the map");
    let (_, output) = h.frame_at(&ctx, vec2(1366.0, 768.0), vec![]);
    let blocked_center = output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Rect(rect) if rect.rect.contains(pos2(683.0, 384.0)) && rect.fill.a() > 0)
    });
    assert!(!blocked_center, "city management must leave the rendered world exposed in the center");
}

#[test]
fn real_civilization_menu_and_roster_clicks_select_source_civilization() {
    let ctx = egui::Context::default();
    let mut h = Harness::new(Screen::Menu);
    h.shell.dev_mode = true;
    h.frame(&ctx, vec![]);
    h.frame(&ctx, vec![]);
    h.click(&ctx, pos2(200.0, 536.0));
    assert_eq!(h.screen(), Screen::Civilizations);
    h.frame(&ctx, vec![]);
    h.click(&ctx, pos2(200.0, 236.0));
    assert_eq!(
        h.state().selected_civilization(),
        "arabia",
        "roster click must change the actual source selection (second BNW nation)"
    );
    assert!(!h.screen().wants_world());
}

#[test]
fn settings_screen_edits_the_player_settings() {
    for (width, height, scale) in [(1366., 768., 1.), (960., 640., 1.), (960., 640., 1.5), (1920., 1280., 2.), (960., 640., 2.)] {
        let ctx = egui::Context::default();
        let mut h = Harness::new(Screen::Settings);
        let frame = |h: &mut Harness, events| {
            let mut raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width / scale, height / scale))),
                events,
                ..Default::default()
            };
            raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().native_pixels_per_point = Some(scale);
            let mut output = ctx.run_ui(raw, |ui| {
                shell_screens::draw(ui.ctx(), &mut h.shell, &mut h.ui, None);
            });
            output.textures_delta.clear();
            output
        };
        let click = |h: &mut Harness, at| {
            for pressed in [true, false] {
                frame(h, vec![egui::Event::PointerMoved(at), press(at, pressed)]);
            }
        };
        let scroll = |h: &mut Harness| {
            // Wheel over the empty row inset, never a slider or combobox.
            let panel_width = (width / scale - 48.).min(1050.);
            let padding = if height / scale - 117. < 360. { 10. } else { 20. };
            let scroll_x = (width / scale - panel_width) * 0.5 + padding + 6.;
            let wheel = egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: vec2(0., -35.),
                modifiers: egui::Modifiers::NONE,
            };
            frame(h, vec![egui::Event::PointerMoved(pos2(scroll_x, height / scale - 100.)), wheel]);
            for _ in 0..20 {
                frame(h, vec![]);
            }
        };
        let find = |h: &mut Harness, stage: &str, target: &dyn Fn(&egui::FullOutput) -> Option<egui::Pos2>| {
            for _ in 0..30 {
                let output = frame(h, vec![]);
                if target(&output).is_some() {
                    // Let wheel scrolling settle before pointer-down/up: the target
                    // must not move between the two events in this scripted click.
                    for _ in 0..20 {
                        frame(h, vec![]);
                    }
                    if let Some(at) = target(&frame(h, vec![])) {
                        return at;
                    }
                }
                scroll(h);
            }
            let output = frame(h, vec![]);
            let text_shapes: Vec<_> = output.shapes.iter().filter_map(|shape| match &shape.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_owned(), t.pos, shape.clip_rect)),
                _ => None,
            }).collect();
            panic!("setting {stage} unreachable at {width} / scale {scale}: {text_shapes:?}");
        };
        let label = |output: &egui::FullOutput, name: &str| output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        });
        frame(&mut h, vec![]);
        frame(&mut h, vec![]);
        let at = find(&mut h, "Music volume", &|output| {
            let music_at = label(output, "Music volume")?;
            output.shapes.iter().filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.rect.width() > 70. && rect.rect.height() <= 10.
                    && rect.rect.center().y > music_at.y && rect.rect.center().y < music_at.y + 95.
                    && shape.clip_rect.contains(rect.rect.center()) => Some(rect.rect),
                _ => None,
            }).max_by(|a, b| a.width().total_cmp(&b.width())).map(|rail| rail.lerp_inside(vec2(0.75, 0.5)))
        });
        click(&mut h, at);
        let volume = h.shell.settings.music_volume;
        assert!((volume - 0.75).abs() < 0.08, "music slider follows pointer: {volume}");
        let at = find(&mut h, "Mute music", &|output| {
            let mute = label(output, "Mute music")?;
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.text() == "On" && t.pos.y >= mute.y - 12. && t.pos.y < mute.y + 95.
                    && shape.clip_rect.contains(t.pos + t.galley.size() * 0.5) => Some(t.pos + t.galley.size() * 0.5),
                _ => None,
            })
        });
        click(&mut h, at);
        assert!(h.shell.music_muted(), "mute click failed width={width} height={height} scale={scale} at={at:?}");
        assert_eq!(h.shell.settings.music_volume, volume);
        let at = find(&mut h, "Graphics", &|output| label(output, "Graphics"));
        click(&mut h, at);
        frame(&mut h, vec![]);
        let at = find(&mut h, "Low", &|output| output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(t) if t.galley.text() == "Low" && shape.clip_rect.contains(t.pos + t.galley.size() * 0.5) => {
                Some(t.pos + t.galley.size() * 0.5)
            }
            _ => None,
        }));
        click(&mut h, at);
        assert_eq!(h.shell.settings.quality, client::shell::settings::Quality::Low);
    }
}

#[test]
fn roster_mouse_wheel_reaches_and_selects_last_civilization() {
    let ctx = egui::Context::default();
    let mut h = Harness::new(Screen::Civilizations);
    h.shell.dev_mode = true;
    h.frame(&ctx, vec![]);
    h.frame(&ctx, vec![egui::Event::PointerMoved(pos2(170.0, 400.0))]);
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        phase: egui::TouchPhase::Move,
        delta: vec2(0.0, -9000.0),
        modifiers: egui::Modifiers::NONE,
    };
    h.frame(&ctx, vec![wheel]);
    for _ in 0..16 {
        h.frame(&ctx, vec![]);
    }
    h.click(&ctx, pos2(160.0, 661.0));
    assert_eq!(
        h.state().selected_civilization(),
        "zulu",
        "wheel scrolling must expose the final roster row for real pointer selection"
    );
}

#[test]
fn navigation_retains_preview_choices() {
    let mut s = ScreenState::default();
    s.queue.push(1);
    s.screen = Screen::Settings;
    s.navigate(Screen::Research);
    assert_eq!(s.screen, Screen::Research);
    s.navigate(Screen::Map);
    assert_eq!(s.queue, vec![0, 1]);
}

#[test]
fn research_requires_prerequisites_and_sets_current_choice() {
    let mut s = ScreenState::default();
    s.research = 5;
    assert!(!s.start_research());
    assert_eq!(s.current_research, Some(2));
    s.research = 4;
    assert!(s.start_research());
    assert_eq!(s.current_research, Some(4));
    s.research = 0;
    assert!(!s.start_research());
}

#[test]
fn policy_adoption_spends_once_and_unlocks_children() {
    let mut s = ScreenState::default();
    s.policy = 1;
    assert!(!s.adopt_policy());
    s.policy = 0;
    assert!(s.adopt_policy());
    assert!(!s.adopt_policy());
    s.policy = 1;
    assert!(s.adopt_policy());
    assert_eq!(s.policy_points, 0);
    s.policy = 3;
    assert!(!s.adopt_policy());
}

/// The HUD over the map with the classic order panel: hover passes only where no panel,
/// badge, menu or drag takes the pointer.
#[test]
fn current_frame_hud_navigation_drag_and_leave_suppress_hover() {
    use classic_rules::ids::TileId;
    use client::shell::hover::hover_allowed;
    use client::shell::hud::{self, VisualHudState};
    use render::camera::CameraController;
    use render::world::state::{TerrainCell, UnitMarker};
    let ctx = egui::Context::default();
    let world = RenderWorld {
        width: 1,
        height: 1,
        observer: 1,
        visible: vec![1],
        explored: vec![1],
        terrain: vec![TerrainCell { tile: TileId(0), terrain: 1, owner: None, resource: None }],
        units: vec![UnitMarker { id: 0, embarked: false, fortified: false, tile: TileId(0), kind: 4, owner: 1, hp: 100 }],
        ..Default::default()
    };
    let camera = CameraController::default();
    let mut hud = VisualHudState::default();
    hud.selected_tile = Some(0);
    let mut h = Harness::new(Screen::Map);
    h.shell.dev_mode = true;
    hud.dev_mode = true;
    for (point, allowed) in [
        (egui::pos2(1200., 65.), false),
        (egui::pos2(30., 20.), false),
        (egui::pos2(100., 100.), false),
        (egui::pos2(1250., 700.), false),
        (egui::pos2(200., 610.), false),
        (egui::pos2(200., 500.), true),
        (egui::pos2(700., 350.), true),
    ] {
        let mut result = false;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1366., 768.))),
            events: vec![egui::Event::PointerMoved(point)],
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            hud::draw(ui.ctx(), Some(&world), &camera, &mut hud, None, &mut h.ui);
            shell_screens::draw_navigation(ui.ctx(), &mut h.shell, &mut h.ui);
            result = hover_allowed(ui.ctx(), h.shell.screen, false, hud.hover_over_badge);
            assert!(!hover_allowed(ui.ctx(), h.shell.screen, true, false));
            assert!(!hover_allowed(ui.ctx(), Screen::City.into(), false, false));
        });
        output.textures_delta.clear();
        assert_eq!(result, allowed, "{point:?}");
    }
    let raw = egui::RawInput { events: vec![egui::Event::PointerGone], ..Default::default() };
    let mut output = ctx.run_ui(raw, |ui| {
        assert!(!hover_allowed(ui.ctx(), Screen::Map.into(), false, false));
    });
    output.textures_delta.clear();
}
