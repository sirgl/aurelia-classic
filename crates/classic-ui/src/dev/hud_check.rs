//! The HUD's overlap check over a match: the states a player meets (the opening, a unit
//! with its orders, the promotion list, notices and rivals after some turns, the autoplay)
//! at each window size, through the app's real UI frames. What overlaps is read from what
//! egui drew (`layout::shown_overlaps`), not from the layout's own claims.
use client::shell::layout;

use super::headless::{opening, request, start};
use super::GameApp;
use crate::ui::model::GameRequest;
use crate::ui::screens::{self, MapSize, MatchRequest};
use crate::ClassicUi;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// The overlaps of every state at every size, one line each ("<state> <W>x<H>: ...");
/// None without a GPU (the app's renderer needs one).
pub fn hud_overlaps(sizes: &[(u32, u32)]) -> Option<Vec<String>> {
    let core = client::shell::gpu::open().ok()?;
    let mut app = GameApp::new(&core, FORMAT, None, ClassicUi::game(), &crate::terrain_layers());
    app.screens.settings.autosave_every = 0;
    let request_match = MatchRequest { civilization: "core:nation/rome".into(), seed: 20260928, size: MapSize::Medium, players: 4, water: 25 };
    start(&mut app, screens::prepare_match(request_match).expect("world"));
    let capital = opening(&mut app);
    let mut found = Vec::new();
    let mut check = |app: &mut GameApp, state: &str, expected: &[&str]| {
        if let Some(tile) = capital {
            app.center_on(tile);
        }
        for &size in sizes {
            found.extend(problems_at(app, size, expected).into_iter().map(|line| format!("{state} {}x{}: {line}", size.0, size.1)));
        }
    };
    check(&mut app, "opening", &["visual_top", "visual_minimap", "visual_map_controls", "game_research", "banner:"]);
    select_unit(&mut app);
    check(&mut app, "unit", &["visual_unit_actions"]);
    app.module.hud.open_promotions();
    check(&mut app, "promotions", &["visual_unit_actions"]);
    // An aircraft has the longest row of orders (the air ones included), in the sweep mode.
    if let Some(tile) = capital {
        select_fighter(&mut app, tile);
        check(&mut app, "fighter sweeping", &["visual_unit_actions"]);
    }
    for _ in 0..12 {
        request(&mut app, GameRequest::EndTurn);
        app.finish_ai_turns();
        app.apply_module_actions();
    }
    check(&mut app, "turn 13", &["game_notices", "badge:"]);
    select_unit(&mut app);
    check(&mut app, "turn 13 unit", &["visual_unit_actions"]);
    if let Some(session) = app.module.session.as_mut() {
        session.autoplay();
    }
    for _ in 0..3 {
        request(&mut app, GameRequest::EndTurn);
        app.finish_ai_turns();
        app.apply_module_actions();
    }
    check(&mut app, "autoplay", &["visual_top"]);
    Some(found)
}

fn select_unit(app: &mut GameApp) {
    if let Some(session) = app.module.session.as_mut() {
        session.select_next_idle_unit();
    }
    app.apply_module_actions();
}

/// Three UI frames at `size` (the layout settles its plan), then what overlaps, and the
/// `expected` things (names, or a mark's kind) that were not shown.
fn problems_at(app: &mut GameApp, size: (u32, u32), expected: &[&str]) -> Vec<String> {
    for frame in 0..3 {
        let screen = egui::vec2(size.0 as f32, size.1 as f32) / app.ctx.pixels_per_point();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            time: Some(f64::from(frame) / 60.0),
            focused: true,
            ..Default::default()
        };
        app.ui_frame(raw, size);
    }
    let shown = layout::shown(&app.ctx, &app.hud.marks());
    let missing = expected.iter().filter(|name| !shown.iter().any(|(n, _)| n.starts_with(**name))).map(|name| format!("not shown: {name}"));
    layout::shown_overlaps(&app.ctx, &shown).into_iter().chain(missing).collect()
}

/// A fighter in the capital, selected, with its sweep mode waiting for a target.
fn select_fighter(app: &mut GameApp, tile: u32) {
    let Some(session) = app.module.session.as_mut() else { return };
    let (game, rules) = session.harness_game();
    let Some(kind) = rules.units.iter().position(|u| u.key == "core:unit/fighter") else { return };
    let fighter = game.debug_spawn_unit(&rules, classic_rules::ids::PlayerId(0), classic_rules::ids::TileId(tile), classic_rules::ids::UnitKindId(kind as u16));
    session.refresh_after_harness();
    session.select_unit(fighter);
    session.request(GameRequest::Unit(crate::ui::model::Action::Sweep));
    app.apply_module_actions();
}
