//! Citizen and purchase marks on the cached visual surface. No authoritative map reads.
use super::*;
use i18n::{t, tf};
use render::{
    camera::CameraController,
    scene::{layout::tile_to_world, terrain::surface::SurfaceMesh},
    world::state::RenderWorld,
};

pub(super) struct Plot {
    tile: TileId,
    center: egui::Pos2,
    outline: Vec<egui::Pos2>,
}

pub(crate) fn prepare(
    state: &mut CityUiState,
    world: &RenderWorld,
    camera: &CameraController,
    surface: Option<&SurfaceMesh>,
    screen: Rect,
    city: Option<&CityView>,
) {
    state.projected.clear();
    let Some(city) = city else {
        state.cancel_buy();
        return;
    };
    state.sync(city);
    if world.width == 0 {
        return;
    }
    let Some(surface) = surface else {
        return;
    };
    let matrix = camera.view_proj(screen.aspect_ratio());
    let tiles: Vec<TileId> = if state.buy_mode {
        city.offers.iter().map(|offer| offer.tile).collect()
    } else {
        std::iter::once(TileId(city.tile))
            .chain(city.tiles.iter().map(|tile| tile.tile))
            .collect()
    };
    for id in tiles {
        let tile = id.0;
        if tile >= world.width.saturating_mul(world.height)
            || world.explored.get(tile as usize) != Some(&1)
        {
            continue;
        }
        let (x, z) = tile_to_world(tile % world.width, tile / world.width);
        let Some(center) = surface
            .sample_display(x, z)
            .and_then(|s| project(&matrix, screen, [x, s.y + 0.04, z]))
        else {
            continue;
        };
        let mut outline = Vec::with_capacity(6);
        for corner in 0..6 {
            let a = (30. + corner as f32 * 60.).to_radians();
            let (px, pz) = (x + a.cos() * 0.94, z + a.sin() * 0.94);
            if let Some(p) = surface
                .sample_display(px, pz)
                .and_then(|s| project(&matrix, screen, [px, s.y + 0.04, pz]))
            {
                outline.push(p);
            }
        }
        if outline.len() == 6 {
            state.projected.push(Plot {
                tile: id,
                center,
                outline,
            });
        }
    }
}

/// Fit observer-known citizen or purchase geometry into the unobscured map pane.
/// Camera zoom is inverse distance; the finite-difference correction accounts for
/// the perspective view and the sidebar rather than assuming viewport centering.
pub(crate) fn fit_camera(
    world: &RenderWorld,
    camera: &mut CameraController,
    surface: Option<&SurfaceMesh>,
    screen: Rect,
    city: &CityView,
    buy: bool,
) {
    let Some(surface) = surface else {
        return;
    };
    if world.width == 0 {
        return;
    }
    let body = Rect::from_min_max(screen.min + vec2(24., 80.), screen.max - vec2(24., 37.));
    let map = layout(body, buy).map;
    let input = map_input_area(map);
    let target = input.shrink2(vec2(
        (input.width() * 0.1).min(42.),
        (input.height() * 0.12).min(10.),
    ));
    if target.width() < 16. || target.height() < 16. {
        return;
    }
    let mut points = Vec::with_capacity((city.offers.len() + 1) * 6);
    let tiles: Vec<u32> = if buy {
        city.offers.iter().map(|o| o.tile.0).collect()
    } else {
        city.tiles.iter().map(|t| t.tile.0).collect()
    };
    for tile in std::iter::once(city.tile).chain(tiles) {
        if world.explored.get(tile as usize) != Some(&1) {
            continue;
        }
        let (x, z) = tile_to_world(tile % world.width, tile / world.width);
        for corner in 0..6 {
            let angle = (30. + corner as f32 * 60.).to_radians();
            let (x, z) = (x + angle.cos(), z + angle.sin());
            if let Some(sample) = surface.sample_display(x, z) {
                points.push([x, sample.y + 0.04, z]);
            }
        }
    }
    fit_points(camera, screen, target, &points);
}
fn projected_bounds(camera: &CameraController, screen: Rect, points: &[[f32; 3]]) -> Option<Rect> {
    let matrix = camera.view_proj(screen.aspect_ratio());
    let mut bounds = Rect::NOTHING;
    for point in points {
        bounds.extend_with(project(&matrix, screen, *point)?);
    }
    bounds.is_positive().then_some(bounds)
}
fn fit_points(camera: &mut CameraController, screen: Rect, target: Rect, points: &[[f32; 3]]) {
    for _ in 0..8 {
        let Some(bounds) = projected_bounds(camera, screen, points) else {
            return;
        };
        let scale = (target.width() / bounds.width()).min(target.height() / bounds.height());
        camera.zoom = (camera.zoom * scale.clamp(0.5, 1.7)).clamp(0.3, 8.);
        let Some(bounds) = projected_bounds(camera, screen, points) else {
            return;
        };
        let mut dx = camera.clone();
        dx.x += 0.1;
        let mut dz = camera.clone();
        dz.y += 0.1;
        let (Some(bx), Some(bz)) = (
            projected_bounds(&dx, screen, points),
            projected_bounds(&dz, screen, points),
        ) else {
            return;
        };
        let a = (bx.center() - bounds.center()) / 0.1;
        let b = (bz.center() - bounds.center()) / 0.1;
        let delta = target.center() - bounds.center();
        let det = a.x * b.y - a.y * b.x;
        if det.abs() < 0.001 {
            return;
        }
        camera.x += (delta.x * b.y - delta.y * b.x) / det;
        camera.y += (a.x * delta.y - a.y * delta.x) / det;
    }
}

fn project(matrix: &[[f32; 4]; 4], screen: Rect, position: [f32; 3]) -> Option<egui::Pos2> {
    let p = [position[0], position[1], position[2], 1.];
    let clip: [f32; 4] =
        std::array::from_fn(|row| (0..4).map(|col| matrix[col][row] * p[col]).sum());
    if clip[3] <= 0. || clip[2] < 0. || clip[2] > clip[3] {
        return None;
    }
    Some(pos2(
        screen.left() + (clip[0] / clip[3] + 1.) * screen.width() * 0.5,
        screen.top() + (1. - clip[1] / clip[3]) * screen.height() * 0.5,
    ))
}
fn contains(poly: &[egui::Pos2], p: egui::Pos2) -> bool {
    let mut inside = false;
    for (a, b) in poly
        .iter()
        .zip(poly.iter().cycle().skip(1))
        .take(poly.len())
    {
        if ((a.y > p.y) != (b.y > p.y)) && (p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x) {
            inside = !inside;
        }
    }
    inside
}
fn price_badges(plots: &[Plot], area: Rect) -> Vec<Rect> {
    let mut badges: Vec<Rect> = Vec::with_capacity(plots.len());
    for plot in plots {
        let mut chosen = Rect::from_center_size(plot.center, vec2(64., 27.));
        for step in 0..24 {
            let offset = if step == 0 {
                0.
            } else {
                ((step + 1) / 2) as f32 * 31. * if step % 2 == 0 { 1. } else { -1. }
            };
            let center = pos2(
                plot.center.x.clamp(area.left() + 33., area.right() - 33.),
                (plot.center.y + offset).clamp(area.top() + 15., area.bottom() - 15.),
            );
            let candidate = Rect::from_center_size(center, vec2(64., 27.));
            chosen = candidate;
            if badges
                .iter()
                .all(|old| !old.expand(3.).intersects(candidate))
            {
                break;
            }
        }
        badges.push(chosen);
    }
    badges
}

/// Both the city title and notification strip remain above map interactions.
fn map_input_area(r: Rect) -> Rect {
    Rect::from_min_max(r.min + vec2(0., 68.), r.max - vec2(0., 66.))
}

fn citizen_primary(tile: &crate::ui::model::WorkedTile) -> Option<GameRequest> {
    if tile.worked && tile.availability.is_ready() {
        Some(GameRequest::ToggleTile(tile.tile))
    } else if !tile.worked && !tile.locked && tile.lock.is_ready() {
        // Merely toggling an unworked tile would exclude it from automatic work.
        // Lock is the real command that gives a new tile assignment priority.
        Some(GameRequest::LockTile(tile.tile))
    } else {
        None
    }
}

fn citizen_tooltip(tile: &crate::ui::model::WorkedTile) -> String {
    let status = match (tile.worked, tile.locked) {
        (true, true) => t!("classic.ui.screen-city-map.working-locked"),
        (true, false) => t!("classic.ui.screen-city-map.working-auto"),
        (false, true) => t!("classic.ui.screen-city-map.locked-waiting"),
        (false, false) => t!("classic.ui.screen-city-map.unassigned"),
    };
    let primary = if tile.worked {
        &tile.availability
    } else {
        &tile.lock
    };
    let click = if !tile.worked && tile.locked {
        t!("classic.ui.screen-city-map.unlock-priority").to_owned()
    } else {
        match primary {
            Availability::Ready => if tile.worked {
                t!("classic.ui.screen-city-map.click-stop")
            } else {
                t!("classic.ui.screen-city-map.click-assign")
            }
            .to_owned(),
            Availability::Blocked(reason) => reason.clone(),
        }
    };
    let lock = match &tile.lock {
        Availability::Ready => if tile.locked {
            t!("classic.ui.screen-city-map.unlock")
        } else {
            t!("classic.ui.screen-city-map.lock")
        }
        .to_owned(),
        Availability::Blocked(reason) => tf!("classic.ui.screen-city-map.lock-unavailable", reason = reason.as_str()),
    };
    format!("{}\n{status}\n{click}\n{lock}", tile.label)
}

fn citizen_glyph(p: &egui::Painter, center: egui::Pos2, color: Color32, filled: bool) {
    p.circle_filled(center - vec2(0., 4.5), 3.3, color);
    let shoulders = Rect::from_center_size(center + vec2(0., 4.), vec2(12., 7.));
    if filled {
        p.rect_filled(shoulders, 3., color);
    } else {
        p.rect_stroke(
            shoulders,
            3.,
            Stroke::new(1.4, color),
            egui::StrokeKind::Inside,
        );
    }
}
fn lock_glyph(p: &egui::Painter, center: egui::Pos2) {
    p.circle_filled(center, 7., BG);
    p.rect_stroke(
        Rect::from_center_size(center - vec2(0., 2.), vec2(5., 6.)),
        3.,
        Stroke::new(1.4, GOLD),
        egui::StrokeKind::Inside,
    );
    p.rect_filled(
        Rect::from_center_size(center + vec2(0., 1.5), vec2(8., 6.)),
        1.,
        GOLD,
    );
}
fn draw_citizens(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, city: &CityView) {
    let area = map_input_area(r).intersect(ui.clip_rect());
    if !area.is_positive() {
        return;
    }
    // A single map response consumes blank-space clicks as well as tile clicks.
    // Nothing here can select a unit or another city behind the overlay.
    let response = ui.interact(area, ui.id().with("city_citizen_map"), Sense::click());
    let hovered = response.hover_pos().and_then(|pointer| {
        state
            .city_ui
            .projected
            .iter()
            .find(|plot| plot.center.distance(pointer) <= 18.)
            .or_else(|| {
                state
                    .city_ui
                    .projected
                    .iter()
                    .find(|plot| contains(&plot.outline, pointer))
            })
            .map(|plot| plot.tile)
    });
    let row = hovered
        .filter(|id| id.0 != city.tile)
        .and_then(|id| city.tiles.iter().find(|tile| tile.tile == id));
    if let Some(tile) = row {
        if response.clicked() {
            if let Some(request) = citizen_primary(tile) {
                state.requests.push(request);
            }
        } else if response.secondary_clicked() && tile.lock.is_ready() {
            state.requests.push(GameRequest::LockTile(tile.tile));
        }
    }
    let painter = ui.painter().with_clip_rect(area);
    for plot in &state.city_ui.projected {
        let center = plot.tile.0 == city.tile;
        let row = city.tiles.iter().find(|tile| tile.tile == plot.tile);
        if !center && row.is_none() {
            continue;
        }
        let worked = center || row.is_some_and(|tile| tile.worked);
        let locked = row.is_some_and(|tile| tile.locked);
        let enabled =
            row.is_some_and(|tile| citizen_primary(tile).is_some() || tile.lock.is_ready());
        let color = if center || locked {
            GOLD
        } else if worked {
            Color32::from_rgb(124, 226, 202)
        } else if enabled {
            Color32::from_rgb(207, 221, 210)
        } else {
            MUTED
        };
        let selected = hovered == Some(plot.tile);
        painter.add(egui::Shape::closed_line(
            plot.outline.clone(),
            Stroke::new(
                if selected { 2.2 } else { 0.9 },
                color.gamma_multiply(if selected { 0.95 } else { 0.45 }),
            ),
        ));
        if !area.contains(plot.center) {
            continue;
        }
        painter.circle_filled(plot.center, 16., Color32::from_rgb(12, 31, 35));
        painter.circle_stroke(
            plot.center,
            16.,
            Stroke::new(if selected { 2.2 } else { 1.3 }, color),
        );
        if center {
            let c = plot.center;
            painter.rect_filled(
                Rect::from_center_size(c + vec2(0., 3.), vec2(12., 10.)),
                1.,
                GOLD,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![c + vec2(-9., -2.), c + vec2(0., -10.), c + vec2(9., -2.)],
                GOLD,
                Stroke::NONE,
            ));
            painter.rect_filled(
                Rect::from_center_size(c + vec2(0., 5.), vec2(3., 6.)),
                0.,
                BG,
            );
        } else {
            citizen_glyph(&painter, plot.center, color, worked);
            if locked {
                lock_glyph(&painter, plot.center + vec2(12., 11.));
            }
        }
    }
    if let Some(tile) = row {
        if citizen_primary(tile).is_some() || tile.lock.is_ready() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        response.on_hover_text(citizen_tooltip(tile));
    } else if hovered == Some(TileId(city.tile)) {
        response.on_hover_text(t!("classic.ui.screen-city-map.center-tip"));
    }
}

pub(super) fn draw(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, city: &CityView) {
    if !state.city_ui.buy_mode {
        draw_citizens(ui, r, state, city);
        return;
    }
    let hit_area = map_input_area(r);
    if !hit_area.is_positive() {
        return;
    }
    let response = ui.interact(hit_area, ui.id().with("city_purchase_map"), Sense::click());
    let pointer = response.hover_pos();
    let badges = price_badges(&state.city_ui.projected, hit_area);
    let hovered = pointer.and_then(|p| {
        state
            .city_ui
            .projected
            .iter()
            .zip(&badges)
            .find(|(_, badge)| badge.contains(p))
            .map(|(plot, _)| plot.tile)
            .or_else(|| {
                state
                    .city_ui
                    .projected
                    .iter()
                    .find(|plot| contains(&plot.outline, p))
                    .map(|plot| plot.tile)
            })
    });
    if response.clicked() {
        state.city_ui.selected = hovered;
    }
    let p = ui
        .painter()
        .with_clip_rect(hit_area.intersect(ui.clip_rect()));
    for (plot, badge) in state.city_ui.projected.iter().zip(&badges) {
        let Some(offer) = city.offers.iter().find(|o| o.tile == plot.tile) else {
            continue;
        };
        let selected = state.city_ui.selected == Some(plot.tile);
        let active = offer.availability.is_ready();
        let color = if selected {
            Color32::from_rgb(124, 226, 202)
        } else if active {
            GOLD
        } else {
            Color32::from_rgb(140, 141, 133)
        };
        // Triangulate a fan: the projected surface edge need not be convex.
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(
            plot.center,
            color.gamma_multiply(if selected { 0.18 } else { 0.08 }),
        );
        for point in &plot.outline {
            mesh.colored_vertex(
                *point,
                color.gamma_multiply(if selected { 0.18 } else { 0.08 }),
            );
        }
        for i in 0..6u32 {
            mesh.add_triangle(0, i + 1, (i + 1) % 6 + 1);
        }
        p.add(egui::Shape::mesh(mesh));
        p.add(egui::Shape::closed_line(
            plot.outline.clone(),
            Stroke::new(if selected { 2.7 } else { 1.6 }, color),
        ));
        if badge.center().distance(plot.center) > 5. {
            p.line_segment([plot.center, badge.center()], Stroke::new(1., color));
        }
        p.rect_filled(*badge, 12., Color32::from_rgb(12, 31, 35));
        p.rect_stroke(
            *badge,
            12.,
            Stroke::new(if hovered == Some(plot.tile) { 2. } else { 1. }, color),
            egui::StrokeKind::Inside,
        );
        p.text(
            badge.center(),
            Align2::CENTER_CENTER,
            tf!("classic.ui.screen-city-map.price", gold = offer.price),
            FontId::proportional(12.),
            color,
        );
    }
    let header = Rect::from_min_size(r.min, vec2(r.width(), 50.));
    let header_painter = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    header_painter.rect_filled(header, 6., BG.gamma_multiply(0.94));
    header_painter.text(
        header.center(),
        Align2::CENTER_CENTER,
        t!("classic.ui.screen-city-map.choose-land"),
        FontId::proportional(14.),
        CREAM,
    );
    if let Some(id) = hovered {
        if let Some(offer) = city.offers.iter().find(|o| o.tile == id) {
            response.on_hover_text(match &offer.availability {
                Availability::Ready => tf!("classic.ui.screen-city-map.offer-tip", label = offer.label.as_str(), gold = offer.price),
                Availability::Blocked(reason) => {
                    tf!("classic.ui.screen-city-map.offer-tip-blocked", label = offer.label.as_str(), gold = offer.price, reason = reason.as_str())
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Session;
    use crate::ui::model::Action;
    use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};
    fn session(gold: u32) -> Session {
        let rules = game_rules();
        let prepared = prepare_match(MatchRequest {
            civilization: "core:nation/rome".into(),
            seed: 20260928,
            size: MapSize::Small,
            players: 3,
            water: 25,
        })
        .unwrap();
        let mut save = classic_rules::persistence::encode::encode(&prepared.game, &rules);
        save.world.player_gold[0] = gold;
        let mut s = Session::new(
            classic_rules::persistence::decode::decode(&save, &rules).unwrap(),
            rules,
        );
        s.request(GameRequest::Unit(Action::FoundCity));
        assert!(s.city_view().is_some());
        s
    }
    fn frame(
        ctx: &egui::Context,
        state: &mut ScreenState,
        city: &CityView,
        events: Vec<egui::Event>,
    ) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(960., 640.))),
                events,
                ..Default::default()
            },
            |ui| {
                draw(
                    ui,
                    Rect::from_min_max(pos2(330., 20.), pos2(940., 620.)),
                    state,
                    city,
                );
                super::super::purchase_details(
                    ui,
                    Rect::from_min_max(pos2(20., 20.), pos2(300., 620.)),
                    state,
                    city,
                );
            },
        );
        output.textures_delta.clear();
    }
    fn click(ctx: &egui::Context, state: &mut ScreenState, city: &CityView, at: egui::Pos2) {
        for pressed in [true, false] {
            frame(
                ctx,
                state,
                city,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }
    fn prepare_fake_projection(state: &mut ScreenState, city: &CityView, tile: TileId) {
        state.city_ui.sync(city);
        state.city_ui.buy_mode = true;
        state.city_ui.projected = vec![Plot {
            tile,
            center: pos2(520., 280.),
            outline: vec![
                pos2(470., 230.),
                pos2(570., 230.),
                pos2(590., 280.),
                pos2(570., 330.),
                pos2(470., 330.),
                pos2(450., 280.),
            ],
        }];
    }
    #[test]
    fn city_purchase_pointer_selects_then_confirms_real_command_and_price() {
        let mut s = session(5000);
        let city = s.city_view().unwrap();
        assert!(city
            .offers
            .iter()
            .all(|o| s.view().explored[o.tile.0 as usize] == 1));
        let offer = city
            .offers
            .iter()
            .find(|o| o.availability.is_ready())
            .unwrap()
            .clone();
        let selected = s.selection();
        let before = s.world_hash();
        let ctx = egui::Context::default();
        let mut state = ScreenState::default();
        state.screen = super::super::Screen::City;
        prepare_fake_projection(&mut state, &city, offer.tile);
        frame(&ctx, &mut state, &city, vec![]);
        click(&ctx, &mut state, &city, pos2(520., 280.));
        assert_eq!(state.city_ui.selected, Some(offer.tile));
        assert!(
            state.requests.is_empty(),
            "map click only selects, never buys"
        );
        assert_eq!(s.world_hash(), before);
        click(&ctx, &mut state, &city, pos2(160., 600.));
        assert_eq!(state.requests, vec![GameRequest::BuyTile(offer.tile)]);
        s.request(state.requests.pop().unwrap());
        assert_eq!(
            s.selection(),
            selected,
            "purchase preserves the selected city"
        );
        assert_eq!(
            s.game().view().auth.players().treasury_of(s.me()),
            5000 - offer.price
        );
        assert_eq!(
            s.game().view().auth.map().owner(offer.tile).flatten(),
            Some(s.me())
        );
        let refreshed = s.city_view().unwrap();
        assert!(!refreshed.offers.iter().any(|o| o.tile == offer.tile));
        state.city_ui.sync(&refreshed);
        assert_eq!(state.city_ui.selected, None);
        assert!(state.cancel_city_buy());
        assert!(!state.city_buy_mode());
        assert_eq!(s.selection(), selected);
    }
    #[test]
    fn city_purchase_unaffordable_confirm_never_dispatches() {
        let mut s = session(0);
        let city = s.city_view().unwrap();
        let offer = city.offers.first().unwrap();
        assert!(!offer.availability.is_ready());
        let ctx = egui::Context::default();
        let mut state = ScreenState::default();
        prepare_fake_projection(&mut state, &city, offer.tile);
        frame(&ctx, &mut state, &city, vec![]);
        click(&ctx, &mut state, &city, pos2(520., 280.));
        assert_eq!(state.city_ui.selected, Some(offer.tile));
        click(&ctx, &mut state, &city, pos2(160., 600.));
        assert!(state.requests.is_empty());
        assert_eq!(s.game().view().auth.players().treasury_of(s.me()), 0);
    }
    #[test]
    fn city_purchase_camera_fits_free_pane_at_compact_large_and_hidpi_sizes() {
        let mut session = session(5000);
        let city = session.city_view().unwrap();
        let world = session.view();
        let style =
            render::scene::style::compile(render::scene::style::default_style()).unwrap();
        let surface = render::scene::terrain::surface::build(world, &style);
        for size in [vec2(960., 640.), vec2(1440., 900.), vec2(2880., 1800.)] {
            let screen = Rect::from_min_size(egui::Pos2::ZERO, size);
            let (x, z) = tile_to_world(city.tile % world.width, city.tile / world.width);
            let mut camera = CameraController { x, y: z, zoom: 3. };
            fit_camera(world, &mut camera, Some(&surface), screen, &city, true);
            let mut state = CityUiState::default();
            state.begin_buy(&city);
            prepare(
                &mut state,
                world,
                &camera,
                Some(&surface),
                screen,
                Some(&city),
            );
            let map = layout(
                Rect::from_min_max(screen.min + vec2(24., 80.), screen.max - vec2(24., 37.)),
                true,
            )
            .map;
            let mut bounds = Rect::NOTHING;
            for plot in &state.projected {
                for p in &plot.outline {
                    bounds.extend_with(*p);
                }
            }
            assert!(
                map.contains_rect(bounds),
                "{size:?}: {bounds:?} outside {map:?}"
            );
            let input = map_input_area(map);
            let target = input.shrink2(vec2(
                (input.width() * 0.1).min(42.),
                (input.height() * 0.12).min(10.),
            ));
            assert!(
                (bounds.center() - target.center()).length() < 20.,
                "must center on free pane: {bounds:?}"
            );
            assert!(
                bounds.width() > target.width() * 0.65 || bounds.height() > target.height() * 0.65,
                "must fill useful space"
            );
            let badges = price_badges(&state.projected, map);
            for (i, a) in badges.iter().enumerate() {
                for b in badges.iter().skip(i + 1) {
                    assert!(!a.intersects(*b));
                }
            }
        }
    }
    #[test]
    fn city_purchase_projection_tracks_surface_and_excludes_unknown_land() {
        let mut s = session(5000);
        let city = s.city_view().unwrap();
        let mut world = s.view().clone();
        let style =
            render::scene::style::compile(render::scene::style::default_style()).unwrap();
        let surface = render::scene::terrain::surface::build(&world, &style);
        let (x, z) = tile_to_world(city.tile % world.width, city.tile / world.width);
        let mut camera = CameraController::default();
        camera.x = x;
        camera.y = z;
        camera.zoom = 2.15;
        let mut state = CityUiState::default();
        state.begin_buy(&city);
        prepare(
            &mut state,
            &world,
            &camera,
            Some(&surface),
            Rect::from_min_size(egui::Pos2::ZERO, vec2(960., 640.)),
            Some(&city),
        );
        let plot = state
            .projected
            .first()
            .expect("an offered tile projects onto the actual cached surface");
        let tile = plot.tile;
        let first = plot.center;
        prepare(
            &mut state,
            &world,
            &camera,
            Some(&surface),
            Rect::from_min_size(egui::Pos2::ZERO, vec2(1920., 1280.)),
            Some(&city),
        );
        let second = state
            .projected
            .iter()
            .find(|p| p.tile == tile)
            .unwrap()
            .center;
        assert!(
            (second.x - first.x * 2.).abs() < 0.01 && (second.y - first.y * 2.).abs() < 0.01,
            "DPI/viewport projection must stay aligned"
        );
        world.explored[tile.0 as usize] = 0;
        prepare(
            &mut state,
            &world,
            &camera,
            Some(&surface),
            Rect::from_min_size(egui::Pos2::ZERO, vec2(960., 640.)),
            Some(&city),
        );
        assert!(
            !state.projected.iter().any(|p| p.tile == tile),
            "never mark an unknown tile, even if a stale offered list includes it"
        );
    }
    fn citizen_click(
        ctx: &egui::Context,
        state: &mut ScreenState,
        city: &CityView,
        at: egui::Pos2,
        button: egui::PointerButton,
    ) {
        for pressed in [true, false] {
            frame(
                ctx,
                state,
                city,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }
    fn worker_projection(state: &mut ScreenState, city: &CityView, tile: TileId) {
        prepare_fake_projection(state, city, tile);
        state.city_ui.buy_mode = false;
    }
    #[test]
    fn city_citizen_pointer_changes_real_assignment_and_lock_without_changing_selection() {
        let mut session = session(5000);
        let mut city = session.city_view().unwrap();
        let tile = city
            .tiles
            .iter()
            .find(|tile| tile.worked && !tile.locked && tile.availability.is_ready())
            .unwrap()
            .tile;
        let selection = session.selection();
        let ctx = egui::Context::default();
        let mut state = ScreenState::default();
        worker_projection(&mut state, &city, tile);
        frame(&ctx, &mut state, &city, vec![]);
        citizen_click(
            &ctx,
            &mut state,
            &city,
            pos2(520., 280.),
            egui::PointerButton::Primary,
        );
        assert_eq!(state.requests, vec![GameRequest::ToggleTile(tile)]);
        session.request(state.requests.pop().unwrap());
        city = session.city_view().unwrap();
        assert!(
            !city
                .tiles
                .iter()
                .find(|row| row.tile == tile)
                .unwrap()
                .worked
        );
        // Clicking the now-empty tile sends the real priority command, not a
        // second exclusion toggle that might leave it without any citizen.
        frame(&ctx, &mut state, &city, vec![]);
        citizen_click(
            &ctx,
            &mut state,
            &city,
            pos2(520., 280.),
            egui::PointerButton::Primary,
        );
        assert_eq!(state.requests, vec![GameRequest::LockTile(tile)]);
        session.request(state.requests.pop().unwrap());
        city = session.city_view().unwrap();
        let row = city.tiles.iter().find(|row| row.tile == tile).unwrap();
        assert!(row.worked && row.locked);
        citizen_click(
            &ctx,
            &mut state,
            &city,
            pos2(520., 280.),
            egui::PointerButton::Secondary,
        );
        assert_eq!(state.requests, vec![GameRequest::LockTile(tile)]);
        session.request(state.requests.pop().unwrap());
        assert!(
            !session
                .city_view()
                .unwrap()
                .tiles
                .iter()
                .find(|row| row.tile == tile)
                .unwrap()
                .locked
        );
        assert_eq!(session.selection(), selection);
    }
    #[test]
    fn city_citizen_center_locked_capacity_and_overlay_strips_never_dispatch() {
        let mut session = session(5000);
        let city = session.city_view().unwrap();
        let worked = city
            .tiles
            .iter()
            .find(|tile| tile.worked && tile.lock.is_ready())
            .unwrap()
            .tile;
        session.request(GameRequest::LockTile(worked));
        let city = session.city_view().unwrap();
        let blocked = city
            .tiles
            .iter()
            .find(|tile| !tile.worked && !tile.lock.is_ready())
            .expect("population reserved by a lock blocks another assignment")
            .tile;
        let ctx = egui::Context::default();
        let mut state = ScreenState::default();
        for tile in [TileId(city.tile), blocked] {
            worker_projection(&mut state, &city, tile);
            frame(&ctx, &mut state, &city, vec![]);
            for button in [egui::PointerButton::Primary, egui::PointerButton::Secondary] {
                citizen_click(&ctx, &mut state, &city, pos2(520., 280.), button);
            }
            assert!(
                state.requests.is_empty(),
                "center or capacity-blocked tile must not dispatch"
            );
        }
        for buy in [false, true] {
            worker_projection(&mut state, &city, worked);
            state.city_ui.buy_mode = buy;
            for y in [50., 590.] {
                let delta = vec2(0., y - state.city_ui.projected[0].center.y);
                state.city_ui.projected[0].center += delta;
                for point in &mut state.city_ui.projected[0].outline {
                    *point += delta;
                }
                frame(&ctx, &mut state, &city, vec![]);
                citizen_click(
                    &ctx,
                    &mut state,
                    &city,
                    pos2(520., y),
                    egui::PointerButton::Primary,
                );
                assert!(state.requests.is_empty());
                assert!(
                    state.city_ui.selected.is_none(),
                    "title/toast cannot select purchase land"
                );
            }
        }
    }
    #[test]
    fn city_citizen_projection_and_camera_fit_owned_tiles_and_center_in_free_map() {
        let mut session = session(5000);
        let city = session.city_view().unwrap();
        let mut world = session.view().clone();
        let style =
            render::scene::style::compile(render::scene::style::default_style()).unwrap();
        let surface = render::scene::terrain::surface::build(&world, &style);
        for size in [vec2(960., 640.), vec2(1440., 900.), vec2(1920., 1280.)] {
            let screen = Rect::from_min_size(egui::Pos2::ZERO, size);
            let (x, z) = tile_to_world(city.tile % world.width, city.tile / world.width);
            let mut camera = CameraController { x, y: z, zoom: 3. };
            fit_camera(&world, &mut camera, Some(&surface), screen, &city, false);
            let mut state = CityUiState::default();
            prepare(
                &mut state,
                &world,
                &camera,
                Some(&surface),
                screen,
                Some(&city),
            );
            assert!(state.projected.iter().any(|plot| plot.tile.0 == city.tile));
            assert_eq!(state.projected.len(), city.tiles.len() + 1);
            let pane = layout(
                Rect::from_min_max(screen.min + vec2(24., 80.), screen.max - vec2(24., 37.)),
                false,
            )
            .map;
            let input = map_input_area(pane);
            for plot in &state.projected {
                assert!(
                    input.contains(plot.center),
                    "{size:?}: {:?} outside {input:?}",
                    plot.center
                );
                assert!(plot.outline.iter().all(|point| input.contains(*point)));
            }
            let hidden = city.tiles[0].tile;
            world.explored[hidden.0 as usize] = 0;
            prepare(
                &mut state,
                &world,
                &camera,
                Some(&surface),
                screen,
                Some(&city),
            );
            assert!(!state.projected.iter().any(|plot| plot.tile == hidden));
            world.explored[hidden.0 as usize] = 1;
        }
    }
    #[test]
    fn city_bought_land_appears_as_citizen_plot_after_canceling_purchase_mode() {
        let mut session = session(5000);
        let before = session.city_view().unwrap();
        let offer = before
            .offers
            .iter()
            .find(|offer| offer.availability.is_ready())
            .unwrap()
            .clone();
        session.request(GameRequest::BuyTile(offer.tile));
        let city = session.city_view().unwrap();
        assert!(city.tiles.iter().any(|tile| tile.tile == offer.tile));
        let world = session.view();
        let style =
            render::scene::style::compile(render::scene::style::default_style()).unwrap();
        let surface = render::scene::terrain::surface::build(world, &style);
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(960., 640.));
        let (x, z) = tile_to_world(city.tile % world.width, city.tile / world.width);
        let mut camera = CameraController { x, y: z, zoom: 3. };
        let mut state = CityUiState::default();
        state.begin_buy(&before);
        assert!(state.cancel_buy());
        fit_camera(world, &mut camera, Some(&surface), screen, &city, false);
        prepare(
            &mut state,
            world,
            &camera,
            Some(&surface),
            screen,
            Some(&city),
        );
        assert!(state.projected.iter().any(|plot| plot.tile == offer.tile));
        assert!(state.selected.is_none());
    }
    #[test]
    fn city_both_camera_modes_fit_an_extreme_compact_logical_viewport() {
        let mut session = session(5000);
        let city = session.city_view().unwrap();
        let world = session.view();
        let style =
            render::scene::style::compile(render::scene::style::default_style()).unwrap();
        let surface = render::scene::terrain::surface::build(world, &style);
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(480., 320.));
        for buy in [false, true] {
            let (x, z) = tile_to_world(city.tile % world.width, city.tile / world.width);
            let mut camera = CameraController { x, y: z, zoom: 3. };
            fit_camera(world, &mut camera, Some(&surface), screen, &city, buy);
            let mut state = CityUiState::default();
            state.sync(&city);
            state.buy_mode = buy;
            prepare(
                &mut state,
                world,
                &camera,
                Some(&surface),
                screen,
                Some(&city),
            );
            let pane = layout(
                Rect::from_min_max(screen.min + vec2(24., 80.), screen.max - vec2(24., 37.)),
                buy,
            )
            .map;
            let input = map_input_area(pane);
            assert!(!state.projected.is_empty());
            for plot in &state.projected {
                assert!(
                    input.contains(plot.center),
                    "buy={buy}: {:?} outside {input:?}",
                    plot.center
                );
            }
        }
    }
}
