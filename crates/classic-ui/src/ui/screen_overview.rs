//! Advisor overviews: cities (sortable), units, demographics, diplomacy,
//! victory progress and the notification log. Rows navigate: a city opens
//! its screen, a unit or a notice centres the map on it.
use super::*;
use i18n::{t, tf};
use crate::ui::overview_model::{CityRow, OverviewView, ResourceRow, TradeUnitRow, UnitRow};

const ROW: f32 = 30.;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverviewTab {
    #[default]
    Cities,
    Units,
    Demographics,
    Diplomacy,
    Victory,
    Notifications,
    Resources,
    Trade,
}

impl OverviewTab {
    pub const ALL: [OverviewTab; 8] = [
        OverviewTab::Cities,
        OverviewTab::Resources,
        OverviewTab::Trade,
        OverviewTab::Units,
        OverviewTab::Demographics,
        OverviewTab::Diplomacy,
        OverviewTab::Victory,
        OverviewTab::Notifications,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OverviewTab::Cities => t!("classic.ui.screen-overview.tab-economy"),
            OverviewTab::Units => t!("classic.ui.screen-overview.tab-military"),
            OverviewTab::Demographics => t!("classic.ui.screen-overview.tab-demographics"),
            OverviewTab::Diplomacy => t!("classic.ui.screen-overview.tab-diplomacy"),
            OverviewTab::Victory => t!("classic.ui.screen-overview.tab-victory"),
            OverviewTab::Notifications => t!("classic.ui.screen-overview.tab-notifications"),
            OverviewTab::Resources => t!("classic.ui.screen-overview.tab-resources"),
            OverviewTab::Trade => t!("classic.ui.screen-overview.tab-trade"),
        }
    }

    /// The shortcut that opens the tab (an action id of `ui::shortcuts`).
    fn hotkey(self) -> Option<&'static str> {
        match self {
            OverviewTab::Cities => Some("overview-economy"),
            OverviewTab::Units => Some("overview-military"),
            OverviewTab::Diplomacy => Some("overview-diplomacy"),
            OverviewTab::Notifications => Some("notifications"),
            OverviewTab::Victory => Some("victory"),
            OverviewTab::Demographics => Some("demographics"),
            OverviewTab::Resources | OverviewTab::Trade => None,
        }
    }
}

/// Column the cities list is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CitySort {
    #[default]
    Name,
    Population,
    Food,
    Production,
    Gold,
    Science,
    Culture,
    Faith,
    Strength,
}

/// Where the map should look after an overview click (drained by the game).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Tile(u32),
    Unit(classic_rules::ids::UnitHandle),
}

#[derive(Default)]
pub(super) struct OverviewState {
    pub tab: OverviewTab,
    sort: CitySort,
    descending: bool,
}

impl ScreenState {
    pub fn open_overview(&mut self, tab: OverviewTab) {
        self.overview.tab = tab;
        self.navigate(Screen::Overview);
    }

    pub fn overview_tab(&self) -> OverviewTab {
        self.overview.tab
    }

    /// The map position an overview row asked for, once.
    pub fn take_focus(&mut self) -> Option<Focus> {
        self.map_focus.take()
    }

    #[cfg(test)]
    pub fn sort_cities_by(&mut self, sort: CitySort, descending: bool) {
        self.overview.sort = sort;
        self.overview.descending = descending;
    }
}

/// Cities in the order of a column (ties by name, so the order is stable).
pub fn sort_cities(rows: &mut [CityRow], sort: CitySort, descending: bool) {
    let key = |c: &CityRow| -> i64 {
        match sort {
            CitySort::Name => 0,
            CitySort::Population => c.population as i64,
            CitySort::Food => c.food as i64,
            CitySort::Production => c.production as i64,
            CitySort::Gold => c.gold as i64,
            CitySort::Science => c.science as i64,
            CitySort::Culture => c.culture as i64,
            CitySort::Faith => c.faith as i64,
            CitySort::Strength => c.strength as i64,
        }
    };
    rows.sort_by(|a, b| {
        let by_key = if descending { key(b).cmp(&key(a)) } else { key(a).cmp(&key(b)) };
        let by_name = if descending && sort == CitySort::Name { b.name.cmp(&a.name) } else { a.name.cmp(&b.name) };
        by_key.then(by_name)
    });
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    r: Rect,
    state: &mut ScreenState,
    view: &OverviewView,
    diplomacy: Option<&crate::ui::model::DiplomacyView>,
) {
    let tabs = Rect::from_min_size(r.min, vec2(r.width(), 40.));
    draw_tabs(ui, tabs, state);
    let body = Rect::from_min_max(pos2(r.left(), tabs.bottom() + 12.), r.max);
    panel(ui.painter(), body);
    let inner = body.shrink2(vec2(20., 16.));
    match state.overview.tab {
        OverviewTab::Cities => cities(ui, inner, state, view),
        OverviewTab::Units => units(ui, inner, state, &view.units),
        OverviewTab::Demographics => demographics(ui, inner, view),
        OverviewTab::Diplomacy => {
            if let Some(diplomacy) = diplomacy {
                super::diplomacy_screen::diplomacy_body(ui, inner, state, diplomacy);
            }
        }
        OverviewTab::Victory => victory(ui, inner, state, view),
        OverviewTab::Notifications => notices(ui, inner, state, view),
        OverviewTab::Resources => resources(ui, inner, &view.resources),
        OverviewTab::Trade => trade(ui, inner, state, view),
    }
}

fn draw_tabs(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState) {
    let tabs = OverviewTab::ALL.len() as f32;
    let width = (r.width() - (tabs - 1.) * 8.) / tabs;
    for (i, tab) in OverviewTab::ALL.into_iter().enumerate() {
        let b = Rect::from_min_size(r.min + vec2(i as f32 * (width + 8.), 0.), vec2(width, 36.));
        let key = tab.hotkey().and_then(|id| client::shell::keymap::hint(ui.ctx(), id)).unwrap_or_default();
        let label = format!("{}  {}", tab.label(), key);
        if button(ui, b, &label, state.overview.tab == tab, true) {
            state.overview.tab = tab;
        }
    }
}

/// The columns of the cities table: header, sort key, x offset (the headers are short).
fn city_columns() -> [(&'static str, CitySort, f32); 9] {
    [
        (t!("classic.ui.screen-overview.col-city"), CitySort::Name, 0.),
        (t!("classic.ui.screen-overview.col-pop"), CitySort::Population, 230.),
        (t!("classic.ui.screen-overview.col-food"), CitySort::Food, 290.),
        (t!("classic.ui.screen-overview.col-prod"), CitySort::Production, 350.),
        (t!("classic.ui.screen-overview.col-gold"), CitySort::Gold, 410.),
        (t!("classic.ui.screen-overview.col-sci"), CitySort::Science, 470.),
        (t!("classic.ui.screen-overview.col-cult"), CitySort::Culture, 530.),
        (t!("classic.ui.screen-overview.col-faith"), CitySort::Faith, 590.),
        (t!("classic.ui.screen-overview.col-str"), CitySort::Strength, 650.),
    ]
}

fn cities(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &OverviewView) {
    let economy = Rect::from_min_max(pos2(r.right() - 300., r.top()), r.max);
    let table = Rect::from_min_max(r.min, pos2(economy.left() - 20., r.bottom()));
    for (label, sort, x) in city_columns() {
        let header = Rect::from_min_size(pos2(table.left() + x, table.top()), vec2(56., 24.));
        let active = state.overview.sort == sort;
        let response = ui.interact(header, ui.id().with(("city_sort", sort as u8)), Sense::click());
        let color = if active || response.hovered() { GOLD } else { MUTED };
        let width = ui.painter().text(header.min + vec2(0., 4.), Align2::LEFT_TOP, label, FontId::proportional(12.), color).width();
        if active {
            let c = header.min + vec2(width + 8., 11.);
            let points = if state.overview.descending {
                vec![c + vec2(-4., -2.), c + vec2(4., -2.), c + vec2(0., 3.)]
            } else {
                vec![c + vec2(-4., 3.), c + vec2(4., 3.), c + vec2(0., -2.)]
            };
            ui.painter().add(egui::Shape::convex_polygon(points, GOLD, Stroke::NONE));
        }
        let response = response.on_hover_text(t!("classic.ui.screen-overview.sort-tip"));
        if response.clicked() {
            state.overview.descending = if active { !state.overview.descending } else { sort != CitySort::Name };
            state.overview.sort = sort;
        }
    }
    text(ui.painter(), pos2(table.left() + 710., table.top() + 4.), t!("classic.ui.screen-overview.col-producing"), 12., MUTED);
    let mut rows = view.cities.clone();
    sort_cities(&mut rows, state.overview.sort, state.overview.descending);
    let list = Rect::from_min_max(pos2(table.left(), table.top() + 30.), table.max);
    if rows.is_empty() {
        text(ui.painter(), list.min + vec2(0., 8.), t!("classic.ui.screen-overview.no-cities"), 14., MUTED);
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("overview_cities")
            .max_height(list.height())
            .auto_shrink([false, false])
            .show_rows(ui, ROW, rows.len(), |ui, range| {
                for row in &rows[range] {
                    if city_row(ui, row) {
                        state.requests.push(GameRequest::SelectCity(row.tile));
                    }
                }
            });
    });
    economy_panel(ui, economy, view);
}

fn city_row(ui: &mut egui::Ui, row: &CityRow) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW - 2.), Sense::click());
    let fill = if response.hovered() { Color32::from_rgb(39, 67, 70) } else { RAISED };
    let p = ui.painter();
    p.rect_filled(rect, 3., fill);
    let name = if row.capital { format!("★ {}", row.name) } else { row.name.clone() };
    text(p, rect.min + vec2(8., 6.), name, 14., CREAM);
    let values = [
        row.population as i64,
        row.food as i64,
        row.production as i64,
        row.gold as i64,
        row.science as i64,
        row.culture as i64,
        row.faith as i64,
        row.strength as i64,
    ];
    for (value, (_, _, x)) in values.iter().zip(city_columns().iter().skip(1)) {
        text(p, rect.min + vec2(*x, 6.), value, 14., CREAM);
    }
    text(p, rect.min + vec2(710., 6.), &row.producing, 13., MUTED);
    let tip = match row.grows_in {
        Some(turns) => tf!("classic.ui.screen-overview.city-tip-grows", n = turns),
        None => t!("classic.ui.screen-overview.city-tip-static").to_string(),
    };
    response.on_hover_text(tip).clicked()
}

fn economy_panel(ui: &egui::Ui, r: Rect, view: &OverviewView) {
    let p = ui.painter();
    p.rect_filled(r, 4., BG);
    text(p, r.min + vec2(14., 12.), t!("classic.ui.screen-overview.treasury"), 11., GOLD);
    let Some(gold) = &view.gold else {
        return;
    };
    text(p, r.min + vec2(14., 32.), &gold.total, 16., CREAM);
    let mut y = r.top() + 64.;
    for (label, value) in &gold.lines {
        text(p, pos2(r.left() + 14., y), label, 13., MUTED);
        p.text(pos2(r.right() - 14., y), Align2::RIGHT_TOP, value, FontId::proportional(13.), CREAM);
        y += 22.;
    }
    let total = |f: fn(&CityRow) -> i64| -> i64 { view.cities.iter().map(f).sum() };
    y += 12.;
    text(p, pos2(r.left() + 14., y), t!("classic.ui.screen-overview.empire-per-turn"), 11., GOLD);
    y += 22.;
    for (label, value) in [
        (t!("classic.ui.screen-overview.food-surplus"), total(|c| c.food as i64)),
        (t!("classic.ui.screen-overview.production"), total(|c| c.production as i64)),
        (t!("classic.ui.screen-overview.science"), total(|c| c.science as i64)),
        (t!("classic.ui.screen-overview.culture"), total(|c| c.culture as i64)),
        (t!("classic.ui.screen-overview.faith"), total(|c| c.faith as i64)),
    ] {
        text(p, pos2(r.left() + 14., y), label, 13., MUTED);
        p.text(pos2(r.right() - 14., y), Align2::RIGHT_TOP, value, FontId::proportional(13.), CREAM);
        y += 22.;
    }
}

fn unit_columns() -> [(&'static str, f32); 6] {
    [
        (t!("classic.ui.screen-overview.col-unit"), 0.),
        (t!("classic.ui.screen-overview.col-status"), 260.),
        (t!("classic.ui.screen-overview.col-hp"), 470.),
        (t!("classic.ui.screen-overview.col-strength"), 540.),
        (t!("classic.ui.screen-overview.col-ranged"), 630.),
        (t!("classic.ui.screen-overview.col-moves"), 720.),
    ]
}

fn units(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, rows: &[UnitRow]) {
    for (label, x) in unit_columns() {
        text(ui.painter(), pos2(r.left() + x, r.top() + 4.), label, 12., MUTED);
    }
    let military = rows.iter().filter(|u| u.military).count();
    let summary = tf!("classic.ui.screen-overview.unit-summary", military = military, civilian = rows.len() - military);
    ui.painter().text(pos2(r.right(), r.top() + 4.), Align2::RIGHT_TOP, summary, FontId::proportional(12.), GOLD);
    let list = Rect::from_min_max(pos2(r.left(), r.top() + 30.), r.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("overview_units")
            .max_height(list.height())
            .auto_shrink([false, false])
            .show_rows(ui, ROW, rows.len(), |ui, range| {
                for row in &rows[range] {
                    if unit_row(ui, row) {
                        state.map_focus = Some(Focus::Unit(row.unit));
                        state.navigate(Screen::Map);
                    }
                }
            });
    });
}

fn unit_row(ui: &mut egui::Ui, row: &UnitRow) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW - 2.), Sense::click());
    crate::ui::pedia_link::mark(ui, rect, crate::ui::pedia::PediaRef::unit(row.kind));
    let fill = if response.hovered() { Color32::from_rgb(39, 67, 70) } else { RAISED };
    let p = ui.painter();
    p.rect_filled(rect, 3., fill);
    let ink = if row.military { CREAM } else { Color32::from_rgb(196, 214, 204) };
    text(p, rect.min + vec2(8., 6.), &row.name, 14., ink);
    text(p, rect.min + vec2(260., 6.), &row.status, 13., MUTED);
    text(p, rect.min + vec2(470., 6.), row.hp, 14., CREAM);
    text(p, rect.min + vec2(540., 6.), row.strength, 14., CREAM);
    let ranged = if row.ranged > 0 { row.ranged.to_string() } else { "—".into() };
    text(p, rect.min + vec2(630., 6.), ranged, 14., CREAM);
    text(p, rect.min + vec2(720., 6.), &row.moves, 14., CREAM);
    response.on_hover_text(t!("classic.ui.screen-overview.unit-tip")).clicked()
}

fn demographics(ui: &egui::Ui, r: Rect, view: &OverviewView) {
    let p = ui.painter();
    let columns = [
        (t!("classic.ui.screen-overview.col-measure"), 0.),
        (t!("classic.ui.screen-overview.col-value"), 300.),
        (t!("classic.ui.screen-overview.col-rank"), 440.),
        (t!("classic.ui.screen-overview.col-best"), 540.),
        (t!("classic.ui.screen-overview.col-average"), 660.),
        (t!("classic.ui.screen-overview.col-worst"), 780.),
    ];
    for (label, x) in columns {
        text(p, pos2(r.left() + x, r.top() + 4.), label, 12., MUTED);
    }
    let mut y = r.top() + 34.;
    for row in &view.demographics {
        let line = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), ROW + 6.));
        p.rect_filled(line, 3., RAISED);
        text(p, line.min + vec2(8., 4.), row.label, 15., CREAM);
        text(p, line.min + vec2(8., 21.), row.unit, 10., MUTED);
        text(p, line.min + vec2(300., 8.), row.value, 15., CREAM);
        let rank_color = if row.rank == 1 { GOLD } else { CREAM };
        text(p, line.min + vec2(440., 8.), format!("#{}", row.rank), 15., rank_color);
        text(p, line.min + vec2(540., 8.), row.best, 14., MUTED);
        text(p, line.min + vec2(660., 8.), row.average, 14., MUTED);
        text(p, line.min + vec2(780., 8.), row.worst, 14., MUTED);
        y += ROW + 12.;
    }
    paragraph(
        p,
        pos2(r.left(), y + 10.),
        t!("classic.ui.screen-overview.demographics-note"),
        12.,
        MUTED,
        r.width(),
    );
}

/// Strategic and luxury resources: connected, held by units and
/// buildings, free (a deficit in red weakens the units that need it).
fn resources(ui: &egui::Ui, r: Rect, rows: &[ResourceRow]) {
    let p = ui.painter();
    let columns = [
        (t!("classic.ui.screen-overview.col-resource"), 0.),
        (t!("classic.ui.screen-overview.col-kind"), 260.),
        (t!("classic.ui.screen-overview.col-connected"), 400.),
        (t!("classic.ui.screen-overview.col-in-use"), 540.),
        (t!("classic.ui.screen-overview.col-available"), 680.),
    ];
    for (label, x) in columns {
        text(p, pos2(r.left() + x, r.top() + 4.), label, 12., MUTED);
    }
    let mut y = r.top() + 34.;
    for row in rows {
        let line = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), ROW));
        p.rect_filled(line, 3., RAISED);
        text(p, line.min + vec2(8., 7.), row.name.as_str(), 15., CREAM);
        text(p, line.min + vec2(260., 8.), row.class, 13., MUTED);
        text(p, line.min + vec2(400., 7.), row.total.to_string(), 15., CREAM);
        text(p, line.min + vec2(540., 7.), row.used.to_string(), 15., CREAM);
        let color = if row.available < 0 { Color32::from_rgb(214, 96, 78) } else { GOLD };
        text(p, line.min + vec2(680., 7.), row.available.to_string(), 15., color);
        y += ROW + 6.;
    }
    let note = if rows.is_empty() {
        t!("classic.ui.screen-overview.resources-none")
    } else {
        t!("classic.ui.screen-overview.resources-note")
    };
    paragraph(p, pos2(r.left(), y + 10.), note, 12., MUTED, r.width());
}

/// Trade routes (the trade route overview): what runs, what the
/// trade units in our cities may start (a click starts the route).
/// The route kind is worded by the game layer ("Food", "Production", "Gold and science");
/// food and production routes feed the destination city, the rest pay the player.
fn feeds_city(kind: &str) -> bool {
    let is = |name: &str, english: &str| kind == english || trade_text(name).is_some_and(|text| text == kind);
    is("kind-food", "Food") || is("kind-production", "Production") // i18n-ok: model identifiers
}

/// A word of the game layer's trade messages, when the catalogue has it.
fn trade_text(name: &str) -> Option<String> {
    // i18n-keys: classic.game.trade.*
    i18n::tr_dynamic(&format!("classic.game.trade.{name}"), &[])
}

/// The other end of a route is empty or the game layer's word for "nothing".
fn no_partner(other_end: &str) -> bool {
    other_end.is_empty()
        || other_end == "nothing" // i18n-ok: model identifier
        || trade_text("nothing").is_some_and(|text| text == other_end)
}

fn trade(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &OverviewView) {
    paragraph(
        ui.painter(),
        r.min,
        &view.trade_summary,
        14.,
        GOLD,
        r.width(),
    );
    let body = Rect::from_min_max(r.min + vec2(0., 48.), r.max);
    super::scroll::scroll_body(ui, body, "trade_routes_overview", |ui| {
        ui.label(
            egui::RichText::new(t!("classic.ui.screen-overview.active-connections"))
                .color(GOLD)
                .size(11.),
        );
        if view.trade_routes.is_empty() {
            ui.label(egui::RichText::new(t!("classic.ui.screen-overview.no-routes")).color(MUTED));
        }
        for row in &view.trade_routes {
            let (card, _) = ui.allocate_exact_size(vec2(ui.available_width(), 68.), Sense::hover());
            ui.painter().rect_filled(card, 5., RAISED);
            let title = if row.own {
                tf!("classic.ui.screen-overview.route-title", from = row.from.as_str(), to = row.to.as_str())
            } else {
                tf!("classic.ui.screen-overview.route-title-incoming", from = row.from.as_str(), to = row.to.as_str())
            };
            text(ui.painter(), card.min + vec2(12., 9.), title, 14., CREAM);
            let you = t!("classic.ui.screen-overview.you");
            let recipient = if feeds_city(row.kind) { row.to.as_str() } else { you };
            let detail = if no_partner(&row.other_end) {
                tf!("classic.ui.screen-overview.route-line",
                    kind = row.kind,
                    recipient = recipient,
                    yields = row.yields.as_str(),
                    n = row.turns_left
                )
            } else {
                tf!("classic.ui.screen-overview.route-line-other",
                    kind = row.kind,
                    recipient = recipient,
                    yields = row.yields.as_str(),
                    other = row.other_end.as_str(),
                    n = row.turns_left
                )
            };
            paragraph(
                ui.painter(),
                card.min + vec2(12., 34.),
                &detail,
                12.,
                GOLD,
                card.width() - 24.,
            );
            ui.add_space(4.);
        }
        ui.add_space(14.);
        ui.label(egui::RichText::new(t!("classic.ui.screen-overview.start-route")).color(GOLD).size(11.));
        if view.trade_units.is_empty() {
            ui.label(egui::RichText::new(t!("classic.ui.screen-overview.no-trade-units")).color(MUTED));
        }
        for unit in &view.trade_units {
            trade_unit(ui, unit, state);
        }
    });
}

fn trade_unit(ui: &mut egui::Ui, unit: &TradeUnitRow, state: &mut ScreenState) {
    ui.add_space(8.);
    ui.label(
        egui::RichText::new(if unit.at.is_empty() {
            t!("classic.ui.screen-overview.idle-trader").to_string()
        } else {
            tf!("classic.ui.screen-overview.trader-in", city = unit.at.as_str())
        })
        .color(CREAM)
        .size(15.),
    );
    if unit.offers.is_empty() {
        ui.add(egui::Label::new(egui::RichText::new(t!("classic.ui.screen-overview.no-legal-route")).color(MUTED)).wrap());
    }
    for offer in &unit.offers {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 76.), Sense::hover());
        ui.painter().rect_filled(r, 4., RAISED);
        let start = Rect::from_min_size(pos2(r.right() - 130., r.top() + 20.), vec2(118., 34.));
        paragraph(
            ui.painter(),
            r.min + vec2(12., 8.),
            &offer.label,
            14.,
            CREAM,
            r.width() - 158.,
        );
        let benefit = if no_partner(&offer.other_end) {
            tf!("classic.ui.screen-overview.benefit", yields = offer.yields.as_str())
        } else {
            tf!("classic.ui.screen-overview.benefit-partner", yields = offer.yields.as_str(), partner = offer.other_end.as_str())
        };
        paragraph(
            ui.painter(),
            r.min + vec2(12., 40.),
            &benefit,
            12.,
            GOLD,
            r.width() - 158.,
        );
        if button(ui, start, t!("classic.ui.screen-overview.start-route-button"), false, true) {
            state.requests.push(GameRequest::EstablishTradeRoute {
                unit: unit.unit,
                destination: offer.destination,
                kind: offer.kind,
            });
        }
    }
}

/// A compact summary of the same victory projection as the full screen.
fn victory(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &OverviewView) {
    let Some(victory) = &view.victory else {
        text(ui.painter(), r.min, t!("classic.ui.screen-overview.victory-unavailable"), 14., MUTED);
        return;
    };
    let open = Rect::from_min_size(pos2(r.right() - 168., r.top()), vec2(168., 34.));
    if button(ui, open, t!("classic.ui.screen-overview.open-victory"), false, true) { state.navigate(Screen::Victory); }
    text(ui.painter(), r.min + vec2(0., 8.), tf!("classic.ui.screen-overview.victory-heading", turn = victory.turn), 17., GOLD);
    let rows: Vec<_> = victory.rows.iter().filter(|row| row.enabled).collect();
    let body = Rect::from_min_max(r.min + vec2(0., 48.), r.max);
    if rows.is_empty() {
        text(ui.painter(), body.min, t!("classic.ui.screen-overview.victory-none"), 14., MUTED);
        return;
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        ui.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt("overview_victory_progress")
            .max_height(body.height()).auto_shrink([false, false]).show(ui, |ui| {
                for row in rows {
                    let width = ui.available_width();
                    let goal = ui.painter().layout(row.goal.clone(), FontId::proportional(12.), MUTED, width - 32.);
                    let progress = ui.painter().layout(row.progress.clone(), FontId::proportional(14.), CREAM, width - 32.);
                    let height = 54. + goal.size().y + progress.size().y;
                    let (card, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                    ui.painter().rect_filled(card, 4., RAISED);
                    text(ui.painter(), card.min + vec2(14., 10.), row.kind, 17., GOLD);
                    let at = card.min + vec2(14., 34.);
                    let progress_height = progress.size().y;
                    ui.painter().galley(at, progress, CREAM);
                    ui.painter().galley(at + vec2(0., progress_height + 3.), goal, MUTED);
                    let bar = Rect::from_min_size(card.left_bottom() + vec2(14., -7.), vec2(width - 28., 3.));
                    ui.painter().rect_filled(bar, 1., BG);
                    let share = if !row.show_progress || row.need == 0 { 0. } else { (row.have as f32 / row.need as f32).clamp(0., 1.) };
                    ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * share, bar.height())), 1., GOLD);
                    ui.add_space(8.);
                }
            });
    });
}

fn notices(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &OverviewView) {
    if view.notices.is_empty() {
        text(ui.painter(), r.min, t!("classic.ui.screen-overview.no-notices"), 14., MUTED);
        return;
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("overview_notices")
            .max_height(r.height())
            .auto_shrink([false, false])
            .show_rows(ui, ROW, view.notices.len(), |ui, range| {
                for notice in &view.notices[range] {
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW - 2.), Sense::click());
                    let clickable = notice.tile.is_some();
                    let fill = if clickable && response.hovered() { Color32::from_rgb(39, 67, 70) } else { RAISED };
                    ui.painter().rect_filled(rect, 3., fill);
                    text(ui.painter(), rect.min + vec2(8., 7.), tf!("classic.ui.screen-overview.notice-turn", turn = notice.turn), 12., MUTED);
                    text(ui.painter(), rect.min + vec2(90., 6.), &notice.text, 14., CREAM);
                    if let (true, Some(tile)) = (response.clicked(), notice.tile) {
                        state.map_focus = Some(Focus::Tile(tile));
                        state.navigate(Screen::Map);
                    }
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, population: u32, gold: i32) -> CityRow {
        CityRow {
            tile: 0,
            name: name.into(),
            capital: false,
            population,
            food: 0,
            production: 0,
            gold,
            science: 0,
            culture: 0,
            faith: 0,
            strength: 0,
            producing: String::new(),
            grows_in: None,
        }
    }

    #[test]
    fn overview_victory_filters_disabled_conditions_and_opens_full_screen() {
        let ctx = egui::Context::default();
        let mut state = ScreenState::default(); state.screen = Screen::Overview;
        let mut view = OverviewView::default();
        view.victory = Some(crate::ui::model::VictoryView { turn: 42, max_turns: 100,
            rows: vec![
                crate::ui::model::VictoryRow { kind:"Science", enabled:true, goal:"Build Apollo and spaceship parts".into(), progress:"Apollo Program built · 1 of 6 spaceship parts".into(), show_progress:true, have:2, need:7 },
                crate::ui::model::VictoryRow { kind:"Disabled condition", enabled:false, goal:"Never shown".into(), progress:"Hidden".into(), show_progress:false, have:0, need:1 },
            ], scores:vec![] });
        for width in [1366., 960.] {
            let mut frame = |events| {
                let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 640.));
                let mut output = ctx.run_ui(egui::RawInput { screen_rect:Some(screen), events, ..Default::default() }, |ui| {
                    victory(ui, screen.shrink(24.), &mut state, &view);
                }); output.textures_delta.clear(); output
            };
            frame(vec![]); let output = frame(vec![]);
            assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.text().contains("Disabled condition"))));
            let at = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.text()=="Open victory" => Some(t.pos + t.galley.size()*0.5), _=>None,
            }).unwrap();
            for pressed in [true,false] { frame(vec![egui::Event::PointerMoved(at), egui::Event::PointerButton { pos:at,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE }]); }
            assert_eq!(state.screen, Screen::Victory); state.screen=Screen::Overview;
        }
    }

    #[test]
    fn cities_sort_by_any_column_both_ways_with_stable_ties() {
        let mut rows = vec![row("Rome", 5, 3), row("Antium", 9, 3), row("Cumae", 5, 7)];
        sort_cities(&mut rows, CitySort::Population, true);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Antium", "Cumae", "Rome"]);
        sort_cities(&mut rows, CitySort::Gold, false);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Antium", "Rome", "Cumae"]);
        sort_cities(&mut rows, CitySort::Name, true);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Rome", "Cumae", "Antium"]);
    }
}
