//! Encyclopedia screen: contents and search on the left, the page on the right.
//! Pages come from `crate::ui::pedia` (ruleset data only); this file draws them
//! and keeps the navigation history.
use super::*;
use crate::ui::pedia::{self, Art, Category, Entry, LineKind, Pedia, PediaRef};
use std::sync::Arc;

const ROW: f32 = 25.;
const ART: f32 = 230.;

pub(super) struct PediaState {
    pedia: Option<Arc<Pedia>>,
    query: String,
    /// Search results of `query` (recomputed when it changes).
    results: Vec<PediaRef>,
    searched: String,
    open: Category,
    page: Option<PediaRef>,
    back: Vec<PediaRef>,
    forward: Vec<PediaRef>,
    /// The screen Back returns to.
    pub return_to: Screen,
    scroll_list: bool,
    nation_art: nations::NationArt,
}

impl Default for PediaState {
    fn default() -> Self {
        Self {
            pedia: None,
            query: String::new(),
            results: Vec::new(),
            searched: String::new(),
            open: Category::Concepts,
            page: None,
            back: Vec::new(),
            forward: Vec::new(),
            return_to: Screen::Map,
            scroll_list: true,
            nation_art: Default::default(),
        }
    }
}

impl PediaState {
    fn pedia(&mut self) -> Arc<Pedia> {
        self.pedia.get_or_insert_with(pedia::game_pedia).clone()
    }

    /// Show a page, remembering the current one for Back.
    pub fn show(&mut self, page: PediaRef) {
        if self.page == Some(page) {
            return;
        }
        if let Some(current) = self.page {
            self.back.push(current);
        }
        self.forward.clear();
        self.set_page(page);
    }

    fn set_page(&mut self, page: PediaRef) {
        self.page = Some(page);
        if let Some(entry) = self.pedia().entry(page) {
            self.open = entry.category;
        }
        self.scroll_list = true;
    }

    fn go_back(&mut self) {
        if let Some(previous) = self.back.pop() {
            self.forward.extend(self.page);
            self.set_page(previous);
        }
    }

    fn go_forward(&mut self) {
        if let Some(next) = self.forward.pop() {
            self.back.extend(self.page);
            self.set_page(next);
        }
    }

    pub fn page(&self) -> Option<PediaRef> {
        self.page
    }

    #[cfg(test)]
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_owned();
    }
}

impl ScreenState {
    /// Open the Encyclopedia (at a page when given); Back returns to the current screen.
    pub fn open_pedia(&mut self, page: Option<PediaRef>) {
        if self.screen != Screen::Encyclopedia {
            self.pedia.return_to = self.screen;
        }
        if let Some(page) = page {
            self.pedia.show(page);
        }
        self.navigate(Screen::Encyclopedia);
    }

    /// The screen the Encyclopedia returns to.
    pub fn pedia_return(&self) -> Screen {
        self.pedia.return_to
    }

    /// The Encyclopedia page shown (tests and captures).
    pub fn pedia_page(&self) -> Option<PediaRef> {
        self.pedia.page()
    }

    /// Offscreen captures: decode the shown page's art now.
    pub fn preload_pedia_art(&mut self, ctx: &egui::Context) {
        let Some(page) = self.pedia.page else {
            return;
        };
        let pedia = self.pedia.pedia();
        let Some(entry) = pedia.entry(page) else {
            return;
        };
        match &entry.art {
            Art::Card(id) => self.art.preload(ctx, &[id.as_str()]),
            Art::Nation(slug) => {
                let key = nations::art_key(slug);
                self.pedia.nation_art.preload(ctx, &[key.as_str()], &key)
            }
            Art::Policy(id) => self.policy_art.preload_ids(ctx, std::slice::from_ref(id)),
            Art::None => {}
        }
    }
}

pub(super) fn draw(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState) {
    let pedia = state.pedia.pedia();
    if state.pedia.page.is_none() {
        let first = pedia.category(Category::Concepts).next().map(|e| e.id);
        state.pedia.page = first;
    }
    let list = Rect::from_min_size(r.min, vec2(300., r.height()));
    let page = Rect::from_min_max(pos2(list.right() + 16., r.top()), r.max);
    draw_contents(ui, list, &mut state.pedia, &pedia);
    if let Some(entry) = state.pedia.page.and_then(|p| pedia.entry(p)) {
        draw_page(ui, page, state, &pedia, entry);
    }
}

/// One row of the contents list.
enum Row {
    Heading(Category, usize),
    Page(PediaRef),
}

fn draw_contents(ui: &mut egui::Ui, r: Rect, state: &mut PediaState, pedia: &Pedia) {
    panel(ui.painter(), r);
    text(ui.painter(), r.min + vec2(15., 14.), t!("classic.ui.screens.encyclopedia").to_uppercase(), 13., GOLD);
    let search = Rect::from_min_size(r.min + vec2(12., 38.), vec2(r.width() - 24., 32.));
    ui.painter().rect_filled(search, 4., RAISED);
    let edit = ui.put(
        search.shrink2(vec2(9., 7.)),
        egui::TextEdit::singleline(&mut state.query)
            .id(egui::Id::new("pedia_search"))
            .hint_text("Search")
            .text_color(CREAM)
            .frame(egui::Frame::NONE),
    );
    let stroke = if edit.has_focus() { GOLD } else { Color32::from_rgb(57, 84, 86) };
    ui.painter().rect_stroke(search, 4., Stroke::new(1., stroke), egui::StrokeKind::Inside);
    if state.searched != state.query {
        state.results = pedia.search(&state.query);
        state.searched = state.query.clone();
    }
    let rows = contents_rows(state, pedia);
    let list = Rect::from_min_max(r.min + vec2(8., 80.), r.max - vec2(8., 10.));
    let selected = rows.iter().position(|row| matches!(row, Row::Page(p) if Some(*p) == state.page));
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(("pedia_contents", state.query.is_empty(), state.open))
            .max_height(list.height())
            .auto_shrink([false, false]);
        if state.scroll_list {
            if let Some(index) = selected {
                let offset = (index as f32 * ROW - list.height() * 0.4).max(0.);
                scroll = scroll.vertical_scroll_offset(offset);
            }
            state.scroll_list = false;
        }
        scroll.show_rows(ui, ROW, rows.len(), |ui, range| {
            for row in &rows[range] {
                contents_row(ui, row, state, pedia);
            }
        });
    });
}

fn contents_rows(state: &PediaState, pedia: &Pedia) -> Vec<Row> {
    if !state.query.trim().is_empty() {
        return state.results.iter().map(|p| Row::Page(*p)).collect();
    }
    let mut rows = Vec::new();
    for category in Category::ALL {
        let count = pedia.category(category).count();
        rows.push(Row::Heading(category, count));
        if category == state.open {
            rows.extend(pedia.category(category).map(|e| Row::Page(e.id)));
        }
    }
    rows
}

fn contents_row(ui: &mut egui::Ui, row: &Row, state: &mut PediaState, pedia: &Pedia) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW - 1.), Sense::click());
    match row {
        Row::Heading(category, count) => {
            let open = *category == state.open && state.query.trim().is_empty();
            let fill = if response.hovered() { Color32::from_rgb(39, 67, 70) } else { BG };
            ui.painter().rect_filled(rect, 3., fill);
            let c = rect.min + vec2(11., ROW * 0.5);
            let points = if open {
                vec![c + vec2(-4., -2.), c + vec2(4., -2.), c + vec2(0., 3.)]
            } else {
                vec![c + vec2(-2., -4.), c + vec2(3., 0.), c + vec2(-2., 4.)]
            };
            ui.painter().add(egui::Shape::convex_polygon(points, GOLD, Stroke::NONE));
            text(ui.painter(), rect.min + vec2(20., 5.), category.label(), 13., GOLD);
            ui.painter().text(rect.right_center() - vec2(8., 0.), Align2::RIGHT_CENTER, count, FontId::proportional(11.), MUTED);
            if response.clicked() {
                state.open = *category;
            }
        }
        Row::Page(page) => {
            let Some(entry) = pedia.entry(*page) else {
                return;
            };
            let selected = state.page == Some(*page);
            let fill = if selected {
                TEAL
            } else if response.hovered() {
                Color32::from_rgb(39, 67, 70)
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 3., fill);
            let indent = if state.query.trim().is_empty() { 20. } else { 8. };
            text(ui.painter(), rect.min + vec2(indent, 5.), &entry.name, 13., CREAM);
            if !state.query.trim().is_empty() {
                let label = entry.category.label();
                ui.painter().text(rect.right_center() - vec2(8., 0.), Align2::RIGHT_CENTER, label, FontId::proportional(10.), MUTED);
            }
            if response.clicked() {
                state.show(*page);
                state.scroll_list = false;
            }
        }
    }
}

fn draw_page(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, pedia: &Pedia, entry: &Entry) {
    panel(ui.painter(), r);
    page_header(ui, r, state, entry);
    let art_rect = Rect::from_min_size(r.min + vec2(20., 86.), vec2(ART, ART));
    page_art(ui, art_rect, state, entry);
    let stats = Rect::from_min_max(pos2(art_rect.right() + 24., art_rect.top()), pos2(r.right() - 20., art_rect.bottom()));
    page_stats(ui, stats, entry);
    let body = Rect::from_min_max(pos2(r.left() + 20., art_rect.bottom() + 18.), r.max - vec2(20., 16.));
    let mut target = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::vertical()
            .id_salt(("pedia_page", entry.id))
            .max_height(body.height())
            .auto_shrink([false, false])
            .show(ui, |ui| {
                page_text(ui, entry);
                target = page_links(ui, pedia, entry);
            });
    });
    if let Some(page) = target {
        state.pedia.show(page);
    }
}

fn page_header(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, entry: &Entry) {
    let back = Rect::from_min_size(r.min + vec2(20., 18.), vec2(34., 32.));
    let forward = back.translate(vec2(40., 0.));
    if button(ui, back, "◀", false, !state.pedia.back.is_empty()) {
        state.pedia.go_back();
    }
    if button(ui, forward, "▶", false, !state.pedia.forward.is_empty()) {
        state.pedia.go_forward();
    }
    text(ui.painter(), pos2(forward.right() + 18., r.top() + 14.), &entry.name, 26., CREAM);
    text(ui.painter(), pos2(forward.right() + 19., r.top() + 50.), &entry.subtitle, 13., GOLD);
    ui.painter().line_segment(
        [pos2(r.left() + 20., r.top() + 74.), pos2(r.right() - 20., r.top() + 74.)],
        Stroke::new(1., GOLD.gamma_multiply(0.4)),
    );
}

fn page_art(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, entry: &Entry) {
    ui.painter().rect_filled(r.expand(3.), 3., BG);
    ui.painter().rect_stroke(r.expand(3.), 3., Stroke::new(1., GOLD.gamma_multiply(0.7)), egui::StrokeKind::Inside);
    match &entry.art {
        Art::Card(id) => art(state, ui, r, id),
        Art::Nation(slug) => {
            let brightness = state.brightness;
            let key = nations::art_key(slug);
            state.pedia.nation_art.paint(ui, r, &key, nations::Asset::Portrait, brightness);
            let emblem = Rect::from_min_size(r.left_bottom() + vec2(6., -62.), vec2(56., 56.));
            state.pedia.nation_art.paint(ui, emblem, &key, nations::Asset::Emblem, brightness);
        }
        Art::Policy(id) => match state.policy_art.texture_of(ui.ctx(), id) {
            Some(texture) => {
                let uv = Rect::from_min_max(pos2(0., 0.), pos2(1., 1.));
                ui.painter().image(texture, r.shrink(18.), uv, Color32::WHITE);
            }
            None => placeholder(ui.painter(), r, entry),
        },
        Art::None => placeholder(ui.painter(), r, entry),
    }
}

/// No artwork: the category and the initial of the page, never an empty hole.
fn placeholder(p: &egui::Painter, r: Rect, entry: &Entry) {
    p.rect_filled(r, 0., PANEL);
    p.circle_stroke(r.center(), r.width() * 0.3, Stroke::new(1.5, GOLD.gamma_multiply(0.6)));
    if entry.category == crate::ui::pedia::Category::Promotions {
        super::world_symbols::promotion(p,r.shrink(36.),&entry.key,GOLD,PANEL);
        p.text(r.center_bottom()-vec2(0.,18.),Align2::CENTER_CENTER,"Promotion",FontId::proportional(12.),MUTED);
        return;
    }
    let initial: String = entry.name.chars().take(1).collect();
    p.text(r.center(), Align2::CENTER_CENTER, initial, FontId::proportional(64.), GOLD);
    p.text(r.center_bottom() - vec2(0., 18.), Align2::CENTER_CENTER, entry.category.label(), FontId::proportional(12.), MUTED);
}

fn page_stats(ui: &egui::Ui, r: Rect, entry: &Entry) {
    let p = ui.painter();
    let mut y = r.top();
    for stat in &entry.stats {
        if y + 24. > r.bottom() {
            break;
        }
        text(p, pos2(r.left(), y), stat.label, 13., MUTED);
        let value = p.layout(stat.value.clone(), FontId::proportional(14.), CREAM, r.width() - 190.);
        p.galley(pos2(r.left() + 190., y - 1.), value, CREAM);
        y += 25.;
    }
}

fn page_text(ui: &mut egui::Ui, entry: &Entry) {
    for line in &entry.text {
        let (text, color, font) = match line.kind {
            LineKind::Text => (line.text.clone(), CREAM, FontId::proportional(14.)),
            LineKind::Effect => (format!("•  {}", line.text), CREAM, FontId::proportional(14.)),
            LineKind::Inactive => (format!("–  {}", line.text), MUTED, FontId::proportional(13.)),
            LineKind::Formula => (line.text.clone(), Color32::from_rgb(196, 214, 204), FontId::monospace(12.)),
        };
        let label = ui.label(egui::RichText::new(text).font(font).color(color));
        if line.kind == LineKind::Inactive {
            label.on_hover_text("From the ruleset catalog; this build does not apply it yet.");
        }
    }
    ui.add_space(12.);
}

/// Link groups as clickable names; returns the page clicked.
fn page_links(ui: &mut egui::Ui, pedia: &Pedia, entry: &Entry) -> Option<PediaRef> {
    let mut clicked = None;
    for group in &entry.links {
        ui.add_space(6.);
        ui.label(egui::RichText::new(group.label.to_uppercase()).size(11.).color(GOLD));
        ui.horizontal_wrapped(|ui| {
            for target in &group.targets {
                let Some(page) = pedia.entry(*target) else {
                    continue;
                };
                let chip = egui::Button::new(egui::RichText::new(&page.name).size(13.).color(CREAM))
                    .fill(RAISED)
                    .stroke(Stroke::new(1., Color32::from_rgb(57, 84, 86)));
                let response = ui.add(chip).on_hover_text(&page.subtitle);
                if response.clicked() {
                    clicked = Some(*target);
                }
            }
        });
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, state: &mut ScreenState, events: Vec<egui::Event>) {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1600., 900.))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            super::super::draw(ui.ctx(), state, &RenderWorld::default(), None);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn search_narrows_the_contents_and_links_navigate_with_history() {
        let ctx = egui::Context::default();
        let mut state = ScreenState::default();
        state.game_mode = true;
        state.screen = Screen::Map;
        let pedia = pedia::game_pedia();
        let warrior = pedia.by_key("core:unit/warrior").unwrap();
        state.open_pedia(Some(warrior));
        assert_eq!(state.screen, Screen::Encyclopedia);
        frame(&ctx, &mut state, vec![]);
        state.pedia.set_query("swords");
        frame(&ctx, &mut state, vec![]);
        let swordsman = pedia.by_key("core:unit/swordsman").unwrap();
        assert!(state.pedia.results.contains(&swordsman));
        state.pedia.show(swordsman);
        assert_eq!(state.pedia_page(), Some(swordsman));
        state.pedia.go_back();
        assert_eq!(state.pedia_page(), Some(warrior));
        state.pedia.go_forward();
        assert_eq!(state.pedia_page(), Some(swordsman));
        frame(&ctx, &mut state, vec![]);
    }
}
