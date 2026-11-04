//! Own-city overlay. Presentation reads projected state; every order stays typed.
use super::*;
use i18n::{t, tf};
use crate::ui::model::{Availability, CityView, GameRequest, ProductionOption, ScreenModel};
use classic_rules::ids::TileId;

#[path = "screen_city_map.rs"]
mod city_map;
pub(crate) use city_map::{fit_camera, prepare};

/// The colour of a reason a control is blocked.
const REASON: Color32 = Color32::from_rgb(225, 151, 128);

pub(crate) struct CityUiState {
    city: Option<u32>,
    tab: usize,
    production_tab: bool,
    query: String,
    category: usize,
    show_locked: bool,
    pub(crate) buy_mode: bool,
    /// The list of names to rename the city with is open.
    renaming: bool,
    selected: Option<TileId>,
    projected: Vec<city_map::Plot>,
}
impl Default for CityUiState {
    fn default() -> Self {
        Self {
            city: None,
            tab: 1,
            production_tab: false,
            query: String::new(),
            category: 0,
            show_locked: false,
            buy_mode: false,
            renaming: false,
            selected: None,
            projected: Vec::new(),
        }
    }
}
impl CityUiState {
    fn sync(&mut self, city: &CityView) {
        if self.city != Some(city.tile) {
            *self = Self {
                city: Some(city.tile),
                ..Default::default()
            };
        }
        if self
            .selected
            .is_some_and(|id| !city.offers.iter().any(|o| o.tile == id))
        {
            self.selected = None;
        }
    }
    /// Every production option listed, locked ones too (`--bench-art`).
    pub(crate) fn list_all_production(&mut self, city: &CityView) {
        self.sync(city);
        self.show_locked = true;
    }
    pub(crate) fn begin_buy(&mut self, city: &CityView) {
        self.sync(city);
        self.buy_mode = true;
        self.selected = None;
    }
    /// Offscreen capture setup: the list of names open.
    pub(crate) fn begin_rename(&mut self, city: &CityView) {
        self.sync(city);
        self.renaming = true;
    }
    pub(crate) fn cancel_buy(&mut self) -> bool {
        let was = self.buy_mode;
        self.buy_mode = false;
        self.selected = None;
        was
    }
}

#[derive(Clone, Copy)]
struct Layout {
    left: Rect,
    right: Option<Rect>,
    map: Rect,
    compact: bool,
}
fn layout(r: Rect, buy: bool) -> Layout {
    let compact = r.width() < 1050.;
    if r.width() < 520. && r.height() > r.width() * 1.1 {
        // A phone held upright: the map above, the panel below at the full width (a side panel
        // would leave the map a sliver and squeeze the name and the buttons of the panel).
        let map_h = (r.height() * 0.36).max(180.).min(r.height() - 240.);
        let map = Rect::from_min_size(r.min, vec2(r.width(), map_h));
        let left = Rect::from_min_max(pos2(r.left(), map.bottom() + 8.), r.max);
        return Layout {
            left,
            right: None,
            map,
            compact: true,
        };
    }
    let left_w = if compact {
        (r.width() * 0.37).clamp(240., 310.)
    } else {
        300.
    };
    let left = Rect::from_min_size(r.min, vec2(left_w.min(r.width() * 0.6), r.height()));
    let right =
        (!compact && !buy).then(|| Rect::from_min_max(pos2(r.right() - 332., r.top()), r.max));
    let map = Rect::from_min_max(
        pos2(left.right() + 14., r.top()),
        pos2(right.map_or(r.right(), |x| x.left() - 14.), r.bottom()),
    );
    Layout {
        left,
        right,
        map,
        compact,
    }
}

pub(super) fn show(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, model: &ScreenModel) {
    let Some(city) = &model.city else {
        let hint = Rect::from_center_size(r.center(), vec2(r.width().min(460.), 94.));
        panel(ui.painter(), hint);
        ui.painter().text(
            hint.center(),
            Align2::CENTER_CENTER,
            t!("classic.ui.screen-city-game.select-city"),
            FontId::proportional(16.),
            CREAM,
        );
        return;
    };
    state.city_ui.sync(city);
    let frame = layout(r, state.city_ui.buy_mode);
    // The live map handles citizen assignment or purchases in its visible area.
    city_map::draw(ui, frame.map, state, city);
    panel(ui.painter(), frame.left);
    let mut left = frame.left;
    if frame.compact && !state.city_ui.buy_mode {
        for (i, label) in [t!("classic.ui.screen-city-game.tab-city"), t!("classic.ui.screen-city-game.tab-production")].into_iter().enumerate() {
            let b = Rect::from_min_size(
                left.min + vec2(10. + i as f32 * (left.width() - 20.) * 0.5, 10.),
                vec2((left.width() - 24.) * 0.5, 32.),
            );
            if button(ui, b, label, state.city_ui.production_tab == (i == 1), true) {
                state.city_ui.production_tab = i == 1;
            }
        }
        left.min.y += 50.;
    }
    if frame.compact && state.city_ui.production_tab && !state.city_ui.buy_mode {
        production(ui, left, state, city);
    } else {
        overview(ui, left, state, city);
    }
    if let Some(right) = frame.right {
        panel(ui.painter(), right);
        production(ui, right, state, city);
    }
    if !state.city_ui.buy_mode && frame.map.width() > 140. {
        let title = Rect::from_min_size(frame.map.min, vec2(frame.map.width(), 68.));
        panel(ui.painter(), title);
        ui.interact(title, ui.id().with("city_title_shield"), Sense::click());
        fitted(
            ui,
            title.shrink2(vec2(12., 7.)),
            &city.name,
            26.,
            CREAM,
            Align2::CENTER_TOP,
        );
        ui.painter().text(
            title.center_bottom() - vec2(0., 12.),
            Align2::CENTER_BOTTOM,
            &city.subtitle,
            FontId::proportional(10.),
            GOLD,
        );
    }
    if let Some(message) = &model.message {
        let toast = Rect::from_min_size(
            pos2(frame.map.left(), frame.map.bottom() - 66.),
            vec2(frame.map.width(), 66.),
        );
        panel(ui.painter(), toast);
        ui.interact(toast, ui.id().with("city_message_shield"), Sense::click());
        wrapped(ui, toast.shrink(10.), message, 12., CREAM);
    }
}

fn overview(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, city: &CityView) {
    let title = Rect::from_min_size(r.min + vec2(16., 12.), vec2(r.width() - 128., 30.));
    fitted(ui, title, &city.name, 23., CREAM, Align2::LEFT_TOP);
    let rename = Rect::from_min_size(pos2(r.right() - 106., r.top() + 12.), vec2(92., 28.));
    let can_rename = !city.names.is_empty();
    if button(ui, rename, t!("classic.ui.screen-city-game.rename"), state.city_ui.renaming, can_rename) {
        state.city_ui.renaming = !state.city_ui.renaming;
    }
    if !can_rename {
        ui.interact(rename, ui.id().with("rename_reason"), Sense::hover())
            .on_hover_text(crate::game::reasons::no_free_names());
    }
    if state.city_ui.renaming && can_rename {
        rename_list(ui, Rect::from_min_max(r.min + vec2(14., 50.), r.max - vec2(14., 14.)), state, city);
        return;
    }
    let buy = Rect::from_min_size(r.min + vec2(14., 50.), vec2(r.width() - 28., 36.));
    if button(
        ui,
        buy,
        if state.city_ui.buy_mode {
            t!("classic.ui.screen-city-game.finish-buying")
        } else {
            t!("classic.ui.screen-city-game.buy-tile-mode")
        },
        state.city_ui.buy_mode,
        !city.offers.is_empty() || state.city_ui.buy_mode,
    ) {
        state.city_ui.buy_mode = !state.city_ui.buy_mode;
        state.city_ui.selected = None;
    }
    if state.city_ui.buy_mode {
        purchase_details(
            ui,
            Rect::from_min_max(r.min + vec2(16., 104.), r.max - vec2(16., 16.)),
            state,
            city,
        );
        return;
    }
    for (i, label) in [t!("classic.ui.screen-city-game.tab-overview"), t!("classic.ui.screen-city-game.tab-citizens"), t!("classic.ui.screen-city-game.tab-buildings")]
        .into_iter()
        .enumerate()
    {
        let width = (r.width() - 28.) / 3.;
        let b = Rect::from_min_size(
            r.min + vec2(14. + i as f32 * width, 100.),
            vec2(width - 3., 31.),
        );
        if button(ui, b, label, state.city_ui.tab == i, true) {
            state.city_ui.tab = i;
        }
    }
    let body = Rect::from_min_max(r.min + vec2(14., 145.), r.max - vec2(14., 14.));
    super::scroll::scroll_body(ui, body, "city_overview_scroll", |ui| {
        match state.city_ui.tab {
            1 => citizens(ui, state, city),
            2 => buildings(ui, state, city),
            _ => city_stats(ui, city),
        }
    });
}
/// The unused names of the nation: one click gives the city the name.
fn rename_list(ui: &mut egui::Ui, body: Rect, state: &mut ScreenState, city: &CityView) {
    super::scroll::scroll_body(ui, body, "city_rename_scroll", |ui| {
        heading(ui, t!("classic.ui.screen-city-game.choose-name"));
        for (at, name) in &city.names {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.), Sense::hover());
            if button(ui, row, name, false, true) {
                state.requests.push(GameRequest::RenameCity(*at));
                state.city_ui.renaming = false;
            }
            ui.add_space(3.);
        }
    });
}
fn city_stats(ui: &mut egui::Ui, city: &CityView) {
    heading(ui, t!("classic.ui.screen-city-game.population-growth"));
    ui.label(
        egui::RichText::new(tf!("classic.ui.screen-city-game.citizens", n = city.population))
            .size(24.)
            .color(CREAM),
    );
    ui.label(egui::RichText::new(&city.growth).color(CREAM));
    ui.add(
        egui::ProgressBar::new(
            (city.food_stored as f32 / city.food_needed.max(1) as f32).clamp(0., 1.),
        )
        .text(tf!("classic.ui.screen-city-game.food-progress", stored = city.food_stored, needed = city.food_needed))
        .fill(Color32::from_rgb(119, 165, 88)),
    );
    ui.add_space(16.);
    heading(ui, t!("classic.ui.screen-city-game.output-per-turn"));
    for (label, value) in &city.yields {
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.), Sense::hover());
        let p = ui.painter();
        p.line_segment(
            [row.left_bottom(), row.right_bottom()],
            Stroke::new(0.5, Color32::from_rgb(42, 64, 65)),
        );
        p.text(
            row.left_center(),
            Align2::LEFT_CENTER,
            *label,
            FontId::proportional(13.),
            MUTED,
        );
        p.text(
            row.right_center(),
            Align2::RIGHT_CENTER,
            value,
            FontId::proportional(16.),
            CREAM,
        );
    }
    ui.add_space(14.);
    heading(ui, t!("classic.ui.screen-city-game.citizen-assignment"));
    ui.label(egui::RichText::new(&city.citizens).color(CREAM));
    ui.add_space(14.);
    great_people(ui, city);
}
fn citizens(ui: &mut egui::Ui, state: &mut ScreenState, city: &CityView) {
    ui.label(
        egui::RichText::new(tf!("classic.ui.screen-city-game.citizens", n = city.population)).color(CREAM).size(22.),
    );
    ui.label(egui::RichText::new(&city.citizens).color(MUTED).size(12.));
    ui.add_space(8.);
    ui.label(egui::RichText::new(t!("classic.ui.screen-city-game.output-label")).color(MUTED).size(11.));
    let width = (ui.available_width() - ui.spacing().item_spacing.x).max(0.) * 0.5;
    // `CityView::yields` lists food, production, science, gold, culture, faith, ... in this order.
    for positions in [[0usize, 1], [3, 2]] {
        ui.horizontal(|ui| {
            for position in positions {
                if let Some((label, value)) = city.yields.get(position) {
                    let (r, _) = ui.allocate_exact_size(vec2(width, 25.), Sense::hover());
                    ui.painter().text(r.left_center(), Align2::LEFT_CENTER,
                        label, FontId::proportional(11.), MUTED);
                    ui.painter().text(r.right_center(), Align2::RIGHT_CENTER,
                        value, FontId::proportional(14.), CREAM);
                }
            }
        });
    }
    ui.label(egui::RichText::new(&city.growth).color(CREAM).size(12.));
    ui.add_space(12.);
    heading(ui, t!("classic.ui.screen-city-game.work-the-map"));
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(Color32::from_rgb(104, 197, 176), t!("classic.ui.screen-city-game.legend-working"));
        ui.colored_label(GOLD, t!("classic.ui.screen-city-game.legend-locked"));
        ui.colored_label(CREAM, t!("classic.ui.screen-city-game.legend-available"));
    });
    ui.label(egui::RichText::new(
        t!("classic.ui.screen-city-game.work-help-click")
    ).color(CREAM).size(12.));
    ui.label(egui::RichText::new(t!("classic.ui.screen-city-game.work-help-lock")).color(CREAM).size(12.));
    ui.label(egui::RichText::new(t!("classic.ui.screen-city-game.work-help-center")).color(MUTED).size(11.));
    ui.add_space(14.);
    heading(ui, t!("classic.ui.screen-city-game.governor-focus"));
    for chunk in city.focus.chunks(2) {
        ui.horizontal(|ui| {
            for (focus, current, availability) in chunk {
                if choice(ui, focus_label(*focus), availability, *current, 112.) {
                    state.requests.push(GameRequest::SetFocus(*focus));
                }
            }
        });
    }
    let (avoiding, availability) = &city.avoid_growth;
    if choice(ui, t!("classic.ui.screen-city-game.avoid-growth"), availability, *avoiding, 230.) {
        state.requests.push(GameRequest::SetAvoidGrowth(!*avoiding));
    }
    ui.add_space(14.);
    heading(ui, t!("classic.ui.screen-city-game.specialists"));
    for row in &city.specialists {
        let title = if row.slots > 0 {
            tf!("classic.ui.screen-city-game.specialist-slots", name = row.name.as_str(), n = row.slots)
        } else {
            row.name.clone()
        };
        ui.label(egui::RichText::new(title).color(CREAM));
        ui.label(egui::RichText::new(tf!("classic.ui.screen-city-game.specialist-working", working = row.working, assigned = row.forced
        )).color(CREAM).size(12.));
        if row.slots > 0 && row.forced == 0 {
            ui.label(egui::RichText::new(t!("classic.ui.screen-city-game.governor-manages")).color(MUTED).size(11.));
        }
        ui.label(egui::RichText::new(&row.yields).color(MUTED).size(11.));
        if row.slots > 0 {
            ui.horizontal(|ui| {
                if choice(ui, t!("classic.ui.screen-city-game.remove"), &row.fewer, false, 104.) {
                    state.requests.push(GameRequest::SetSpecialists(
                        row.kind,
                        row.forced.saturating_sub(1),
                    ));
                }
                if choice(ui, t!("classic.ui.screen-city-game.assign"), &row.more, false, 104.) {
                    state.requests.push(GameRequest::SetSpecialists(
                        row.kind,
                        row.forced.saturating_add(1),
                    ));
                }
            });
        }
        ui.add_space(8.);
    }

}
fn great_people(ui: &mut egui::Ui, city: &CityView) {
    if city.great_people.is_empty() {
        return;
    }
    heading(ui, t!("classic.ui.screen-city-game.great-people"));
    for gp in &city.great_people {
        ui.label(egui::RichText::new(&gp.name).color(CREAM));
        ui.add(
            egui::ProgressBar::new((gp.points as f32 / gp.threshold.max(1) as f32).min(1.))
                .text(tf!("classic.ui.screen-city-game.great-person-progress", points = gp.points, threshold = gp.threshold, per_turn = gp.per_turn
                ))
                .fill(Color32::from_rgb(113, 88, 145)),
        );
    }
}
fn buildings(ui: &mut egui::Ui, state: &mut ScreenState, city: &CityView) {
    heading(
        ui,
        &tf!("classic.ui.screen-city-game.buildings-heading", n = city.buildings.len()),
    );
    if city.buildings.is_empty() {
        ui.label(egui::RichText::new(t!("classic.ui.screen-city-game.no-buildings")).color(MUTED));
    }
    for b in &city.buildings {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 92.), Sense::hover());
        if !ui.is_rect_visible(r) {
            continue;
        }
        art(
            state,
            ui,
            Rect::from_min_size(r.min, vec2(54., 54.)),
            &b.art,
        );
        fitted(
            ui,
            Rect::from_min_max(r.min + vec2(64., 2.), pos2(r.right(), r.top() + 28.)),
            &b.name,
            16.,
            CREAM,
            Align2::LEFT_TOP,
        );
        wrapped(
            ui,
            Rect::from_min_max(r.min + vec2(64., 31.), r.max),
            &b.effect,
            11.,
            MUTED,
        );
        ui.interact(r, ui.id().with(&b.name), Sense::hover())
            .on_hover_text(&b.effect);
    }
}
fn purchase_details(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, city: &CityView) {
    let confirm = Rect::from_min_size(pos2(r.left(), r.bottom() - 40.), vec2(r.width(), 40.));
    super::scroll::scroll_body(
        ui,
        Rect::from_min_max(r.min, pos2(r.right(), confirm.top() - 14.)),
        "city_purchase_info",
        |ui| {
            heading(ui, t!("classic.ui.screen-city-game.expand"));
            ui.label(
                egui::RichText::new(tf!("classic.ui.screen-city-game.gold-available", gold = city.treasury))
                    .color(GOLD)
                    .size(19.),
            );
            ui.add_space(12.);
            ui.label(egui::RichText::new(t!("classic.ui.screen-city-game.buy-help")).color(CREAM));
            ui.add_space(16.);
            if let Some(offer) = city
                .offers
                .iter()
                .find(|o| Some(o.tile) == state.city_ui.selected)
            {
                heading(ui, t!("classic.ui.screen-city-game.selected-tile"));
                ui.label(egui::RichText::new(&offer.label).color(CREAM).size(15.));
                ui.add_space(12.);
                ui.label(
                    egui::RichText::new(tf!("classic.ui.screen-city-game.price", gold = offer.price))
                        .color(GOLD)
                        .size(28.),
                );
                if let Availability::Blocked(reason) = &offer.availability {
                    ui.label(egui::RichText::new(reason).color(Color32::from_rgb(225, 151, 128)));
                }
            } else {
                ui.label(egui::RichText::new(t!("classic.ui.screen-city-game.no-tile")).color(MUTED));
            }
            ui.add_space(16.);
            ui.label(egui::RichText::new(t!("classic.ui.screen-city-game.outline-legend")).color(MUTED).size(12.));
        },
    );
    let selected = city
        .offers
        .iter()
        .find(|o| Some(o.tile) == state.city_ui.selected);
    let label = selected.map_or(t!("classic.ui.screen-city-game.choose-tile").into(), |o| {
        tf!("classic.ui.screen-city-game.buy-tile", price = o.price)
    });
    if button(
        ui,
        confirm,
        &label,
        true,
        selected.is_some_and(|o| o.availability.is_ready()),
    ) {
        if let Some(offer) = selected {
            state.requests.push(GameRequest::BuyTile(offer.tile));
        }
    }
}

fn production(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, city: &CityView) {
    let header = Rect::from_min_size(r.min + vec2(14., 12.), vec2(r.width() - 28., 28.));
    fitted(ui, header, t!("classic.ui.screen-city-game.production-heading"), 12., GOLD, Align2::LEFT_TOP);
    let search = Rect::from_min_size(r.min + vec2(12., 39.), vec2(r.width() - 24., 32.));
    ui.put(
        search,
        egui::TextEdit::singleline(&mut state.city_ui.query)
            .hint_text(t!("classic.ui.screen-city-game.search-hint"))
            .text_color(CREAM),
    );
    let filter = Rect::from_min_size(r.min + vec2(12., 80.), vec2(r.width() - 24., 30.));
    for (i, label) in [t!("classic.ui.screen-city-game.filter-available"), t!("classic.ui.screen-city-game.filter-all")].into_iter().enumerate() {
        let b = Rect::from_min_size(
            filter.min + vec2(i as f32 * filter.width() * 0.5, 0.),
            vec2(filter.width() * 0.5 - 3., 30.),
        );
        if button(ui, b, label, state.city_ui.show_locked == (i == 1), true) {
            state.city_ui.show_locked = i == 1;
        }
    }
    let kinds = [
        t!("classic.ui.screen-city-game.kind-all"),
        t!("classic.ui.screen-city-game.kind-units"),
        t!("classic.ui.screen-city-game.kind-buildings"),
        t!("classic.ui.screen-city-game.kind-wonders"),
        t!("classic.ui.screen-city-game.kind-projects"),
    ];
    let category = Rect::from_min_size(r.min + vec2(12., 118.), vec2(r.width() - 24., 30.));
    ui.scope_builder(egui::UiBuilder::new().max_rect(category), |ui| {
        egui::ComboBox::from_id_salt("city_production_category")
            .selected_text(kinds[state.city_ui.category])
            .width(category.width() - 12.)
            .show_ui(ui, |ui| {
                for (i, label) in kinds.into_iter().enumerate() {
                    ui.selectable_value(&mut state.city_ui.category, i, label);
                }
            });
    });
    let bottom_height = (r.height() * 0.43).clamp(204., 298.);
    let list = Rect::from_min_max(
        r.min + vec2(12., 156.),
        pos2(r.right() - 12., r.bottom() - bottom_height),
    );
    let visible: Vec<usize> = city
        .options
        .iter()
        .enumerate()
        .filter_map(|(i, o)| option_visible(o, &state.city_ui).then_some(i))
        .collect();
    if !visible.contains(&state.production) {
        state.production = visible.first().copied().unwrap_or(usize::MAX);
    }
    super::scroll::scroll_body(ui, list, "city_production_options", |ui| {
        if visible.is_empty() {
            ui.label(
                egui::RichText::new(t!("classic.ui.screen-city-game.no-match"))
                    .color(MUTED),
            );
        }
        for i in &visible {
            production_row(ui, state, *i, &city.options[*i]);
        }
    });
    let add = Rect::from_min_size(
        pos2(r.left() + 12., list.bottom() + 8.),
        vec2(r.width() - 24., 36.),
    );
    let selected = city.options.get(state.production);
    let label = selected
        .filter(|o| o.availability.is_ready())
        .map_or(t!("classic.ui.screen-city-game.choose-available").into(), |o| {
            tf!("classic.ui.screen-city-game.enqueue", name = o.name.as_str())
        });
    if button(
        ui,
        add,
        &label,
        true,
        selected.is_some_and(|o| o.availability.is_ready()),
    ) {
        if let Some(o) = selected {
            state.requests.push(GameRequest::Enqueue(o.item));
        }
    }
    let queue = Rect::from_min_max(
        pos2(r.left() + 12., add.bottom() + 12.),
        r.max - vec2(12., 12.),
    );
    production_queue(ui, queue, state, city);
}
fn option_visible(option: &ProductionOption, state: &CityUiState) -> bool {
    (state.show_locked || option.availability.is_ready())
        && (state.query.trim().is_empty()
            || option
                .name
                .to_lowercase()
                .contains(&state.query.trim().to_lowercase()))
        && match state.category {
            1 => option.kind == "UNIT", // i18n-ok: model identifier
            2 => option.kind == "BUILDING", // i18n-ok: model identifier
            3 => option.kind == "WONDER", // i18n-ok: model identifier
            4 => option.kind == "PROJECT", // i18n-ok: model identifier
            _ => true,
        }
}
fn production_row(ui: &mut egui::Ui, state: &mut ScreenState, index: usize, o: &ProductionOption) {
    let (r, response) = ui.allocate_exact_size(vec2(ui.available_width(), 70.), Sense::click());
    if !ui.is_rect_visible(r) {
        return;
    }
    if response.clicked() {
        state.production = index;
    }
    let selected = state.production == index;
    ui.painter().rect_filled(
        r,
        4.,
        if selected {
            Color32::from_rgb(38, 68, 75)
        } else {
            RAISED
        },
    );
    if selected {
        ui.painter()
            .rect_stroke(r, 4., Stroke::new(1., GOLD), egui::StrokeKind::Inside);
    }
    if let Some(page) = crate::ui::pedia::PediaRef::item(o.item) {
        crate::ui::pedia_link::mark(ui, r, page);
    }
    art(
        state,
        ui,
        Rect::from_min_size(r.min + vec2(5., 7.), vec2(54., 54.)),
        &o.art,
    );
    let text_rect = Rect::from_min_max(r.min + vec2(68., 8.), r.max - vec2(8., 5.));
    fitted(
        ui,
        text_rect,
        &o.name,
        15.,
        if o.availability.is_ready() {
            CREAM
        } else {
            MUTED
        },
        Align2::LEFT_TOP,
    );
    let detail = match &o.availability {
        Availability::Ready => match o.turns {
            Some(turns) => tf!("classic.ui.screen-city-game.cost-turns", cost = o.cost, n = turns),
            None => tf!("classic.ui.screen-city-game.cost-no-output", cost = o.cost),
        },
        Availability::Blocked(reason) => reason.clone(),
    };
    wrapped(
        ui,
        Rect::from_min_max(text_rect.min + vec2(0., 24.), text_rect.max),
        &detail,
        11.,
        if o.availability.is_ready() { MUTED } else { REASON },
    );
    response.on_hover_text(format!("{}\n{}", o.name, detail));
    ui.add_space(5.);
}
fn production_queue(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, city: &CityView) {
    let clear = Rect::from_min_size(pos2(r.left(), r.bottom() - 32.), vec2(90., 32.));
    let hurry = Rect::from_min_max(pos2(clear.right() + 7., clear.top()), r.max);
    if ready_button(ui, clear, t!("classic.ui.screen-city-game.clear"), &city.clear) {
        state.requests.push(GameRequest::ClearQueue);
    }
    if ready_button(ui, hurry, &city.hurry.0, &city.hurry.1) {
        state.requests.push(GameRequest::Hurry);
    }
    let body = Rect::from_min_max(r.min, pos2(r.right(), clear.top() - 8.));
    super::scroll::scroll_body(ui, body, "city_queue_scroll", |ui| {
        heading(ui, &tf!("classic.ui.screen-city-game.queue-heading", n = city.queue.len()));
        if let Some(head) = city.queue.first() {
            ui.label(egui::RichText::new(&head.name).color(CREAM).size(15.));
            let remaining = head.cost.saturating_sub(city.production_stored);
            let progress = if city.production_per_turn > 0 {
                tf!("classic.ui.screen-city-game.queue-progress",
                    stored = city.production_stored,
                    cost = head.cost,
                    n = remaining.div_ceil(city.production_per_turn as u32)
                )
            } else {
                tf!("classic.ui.screen-city-game.queue-progress-idle", stored = city.production_stored, cost = head.cost)
            };
            ui.add(
                egui::ProgressBar::new(
                    (city.production_stored as f32 / head.cost.max(1) as f32).min(1.),
                )
                .text(progress)
                .fill(Color32::from_rgb(164, 126, 62)),
            );
            for (i, entry) in city.queue.iter().enumerate().skip(1) {
                ui.label(
                    egui::RichText::new(tf!("classic.ui.screen-city-game.queue-entry",
                        n = i + 1,
                        name = entry.name.as_str(),
                        cost = entry.cost
                    ))
                    .color(MUTED)
                    .size(12.),
                );
            }
        } else {
            ui.label(egui::RichText::new(&city.idle_production).color(CREAM));
            ui.label(
                egui::RichText::new(tf!("classic.ui.screen-city-game.production-stored", n = city.production_stored))
                    .color(MUTED)
                    .size(11.),
            );
            heading(ui, t!("classic.ui.screen-city-game.convert-production"));
            for (process, label, current, availability) in &city.processes {
                if choice(ui, label, availability, *current, ui.available_width()) {
                    state.requests.push(GameRequest::SetProcess(*process));
                }
            }
        }
    });
}
/// The city includes long production and process names; keep every caption in its button.
fn button(ui: &mut egui::Ui, r: Rect, label: &str, selected: bool, enabled: bool) -> bool {
    let font = FontId::proportional(14.);
    let available = (r.width() - 14.).max(0.);
    let fits = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.into(), font.clone(), CREAM)
            .size()
            .x
            <= available
    };
    let mut short = label.to_owned();
    if !fits(&short) {
        while !short.is_empty() && !fits(&format!("{short}…")) {
            short.pop();
        }
        short.push('…');
    }
    let clicked = super::button(ui, r, &short, selected, enabled);
    if short != label {
        ui.interact(
            r,
            ui.id()
                .with(("city_full_caption", r.min.x.to_bits(), r.min.y.to_bits())),
            Sense::hover(),
        )
        .on_hover_text(label);
    }
    clicked
}

fn heading(ui: &mut egui::Ui, label: &str) {
    ui.label(egui::RichText::new(label).color(GOLD).size(10.));
}
fn focus_label(focus: classic_rules::world::citizens::CityFocus) -> &'static str {
    use classic_rules::world::citizens::CityFocus::*;
    match focus {
        Default => t!("classic.ui.screen-city-game.focus-balanced"),
        Food => t!("classic.ui.screen-city-game.focus-food"),
        Production => t!("classic.ui.screen-city-game.focus-production"),
        Gold => t!("classic.ui.screen-city-game.focus-gold"),
        Science => t!("classic.ui.screen-city-game.focus-science"),
        Culture => t!("classic.ui.screen-city-game.focus-culture"),
        SpecialistPoints => t!("classic.ui.screen-city-game.focus-great-people"),
        ModuleYield => t!("classic.ui.screen-city-game.focus-faith"),
    }
}
fn choice(
    ui: &mut egui::Ui,
    label: &str,
    availability: &Availability,
    selected: bool,
    width: f32,
) -> bool {
    let (r, _) = ui.allocate_exact_size(
        vec2(width.min(ui.available_width()).max(30.), 30.),
        Sense::hover(),
    );
    // The chosen option is shown by its highlight; "already chosen" is no reason to print.
    let reason = match availability {
        Availability::Blocked(reason) if !selected => Some(reason.as_str()),
        _ => None,
    };
    let enabled = availability.is_ready() || selected;
    let clicked = client::shell::kit::note_button(ui, r, label, selected, enabled, reason) && !selected;
    if let Some(reason) = reason {
        ui.interact(
            r,
            ui.id().with(("city_reason", label, r.min.y.to_bits())),
            Sense::hover(),
        )
        .on_hover_text(reason);
    }
    clicked
}
fn fitted(ui: &egui::Ui, r: Rect, label: &str, size: f32, color: Color32, align: Align2) {
    let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let galley = p.layout_no_wrap(label.into(), FontId::proportional(size), color);
    let origin = if align == Align2::CENTER_TOP {
        pos2(r.center().x - galley.size().x * 0.5, r.top())
    } else {
        r.min
    };
    p.galley(origin, galley, color);
}
fn wrapped(ui: &egui::Ui, r: Rect, label: &str, size: f32, color: Color32) {
    let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let galley = p.layout(
        label.into(),
        FontId::proportional(size),
        color,
        r.width().max(1.),
    );
    p.galley(r.min, galley, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn city_opens_on_citizens_without_changing_production_or_purchase_defaults() {
        let state = CityUiState::default();
        assert_eq!(state.tab, 1);
        assert!(!state.production_tab);
        assert!(!state.buy_mode);
        assert!(state.query.is_empty());
        assert_eq!(state.category, 0);
        assert!(!state.show_locked);
        assert!(state.selected.is_none());
    }
    #[test]
    fn city_overlay_keeps_map_and_panels_separate_at_narrow_and_wide_sizes() {
        for (width, height) in [(960., 640.), (1366., 768.), (1920., 1080.), (640., 480.)] {
            let r = Rect::from_min_max(pos2(24., 80.), pos2(width - 24., height - 37.));
            for buy in [false, true] {
                let l = layout(r, buy);
                assert!(r.contains_rect(l.left));
                assert!(r.contains_rect(l.map));
                assert!(!l.left.intersects(l.map));
                assert!(l.map.width() > 100.);
                if let Some(right) = l.right {
                    assert!(r.contains_rect(right));
                    assert!(!right.intersects(l.map));
                }
                if buy {
                    assert!(
                        l.right.is_none(),
                        "buy mode gives land the full remaining viewport"
                    );
                }
            }
        }
    }
    #[test]
    fn city_overlay_stacks_the_panel_under_the_map_on_an_upright_phone() {
        for (width, height) in [(360., 740.), (390., 844.), (412., 915.)] {
            let r = Rect::from_min_max(pos2(0., 56.), pos2(width, height - 20.));
            for buy in [false, true] {
                let l = layout(r, buy);
                assert!(r.contains_rect(l.left) && r.contains_rect(l.map));
                assert!(!l.left.intersects(l.map));
                assert_eq!(l.left.width(), r.width());
                assert_eq!(l.map.width(), r.width());
                assert!(l.map.height() >= 180. && l.left.height() >= 240.);
            }
        }
    }
    #[test]
    fn production_filters_search_availability_and_category_independently() {
        let mut state = CityUiState::default();
        let mut option = ProductionOption {
            item: classic_rules::ids::ProductionItem::Unit(classic_rules::ids::UnitKindId(0)),
            name: "Warrior".into(),
            art: "warrior".into(),
            kind: "UNIT",
            cost: 40,
            turns: Some(5),
            availability: Availability::Ready,
        };
        assert!(option_visible(&option, &state));
        option.availability = Availability::Blocked("Requires technology".into());
        assert!(!option_visible(&option, &state));
        state.show_locked = true;
        assert!(option_visible(&option, &state));
        state.query = "wArR".into();
        assert!(option_visible(&option, &state));
        state.category = 2;
        assert!(!option_visible(&option, &state));
        state.category = 1;
        assert!(option_visible(&option, &state));
        state.query = "Granary".into();
        assert!(!option_visible(&option, &state));
    }
}
