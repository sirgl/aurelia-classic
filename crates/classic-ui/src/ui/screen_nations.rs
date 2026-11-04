//! Art browser backed by the game's actual nation definitions.
use super::*;
use classic_rules::rules::source::SourceNation;
use i18n::{t, tf};
use std::collections::{HashMap, VecDeque};

#[path = "nation_catalog.rs"]
mod catalog;
use catalog::Card;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Collection { Game, Artwork }
pub(super) struct NationsState {
    roster: Vec<Card>,
    art_roster: Vec<Card>,
    collection: Collection,
    selected: usize,
    selected_art: usize,
    scroll_to_selection: bool,
    artwork: NationArt,
}
impl Default for NationsState {
    fn default() -> Self {
        Self {
            roster: classic_content::nations::nations().iter().map(|c| Card {
                key: nation_slug(c).to_owned(), name: c.name.clone(), leader: c.leader.clone(),
            }).collect(),
            art_roster: catalog::cards(), collection: Collection::Game,
            selected: 0, selected_art: 0, scroll_to_selection: false, artwork: Default::default(),
        }
    }
}
impl NationsState {
    pub fn preload(&mut self, ctx: &egui::Context) {
        let (roster, index) = match self.collection {
            Collection::Game => (&self.roster, self.selected),
            Collection::Artwork => (&self.art_roster, self.selected_art),
        };
        let slugs: Vec<_> = roster.iter().take(12).map(|c| c.key.as_str()).collect();
        if let Some(selected) = roster.get(index) {
            self.artwork.preload(ctx, &slugs, &selected.key);
        }
    }
    #[cfg(test)]
    pub fn selected_slug(&self) -> &str {
        match self.collection {
            Collection::Game => &self.roster[self.selected].key,
            Collection::Artwork => &self.art_roster[self.selected_art].key,
        }
    }
    pub fn select(&mut self, slug: &str) -> bool {
        let (roster, selected, mode) = if slug.starts_with("art:") {
            (&self.art_roster, &mut self.selected_art, Collection::Artwork)
        } else { (&self.roster, &mut self.selected, Collection::Game) };
        let Some(index) = roster.iter().position(|nation| nation.key == slug) else { return false; };
        *selected = index;
        self.collection = mode;
        self.scroll_to_selection = true;
        true
    }
}
/// Art key of a game nation by slug: its entry in the 43-nation
/// artwork catalog (`art:<slug>`), else the legacy per-slug files.
pub(super) fn art_key(slug: &str) -> String {
    static KEYS: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
    let keys = KEYS.get_or_init(|| catalog::cards().into_iter().map(|c| c.key).collect());
    let key = format!("art:{slug}");
    if keys.contains(&key) {
        key
    } else {
        slug.to_owned()
    }
}

/// The nation's name in the active language (`art:` keys name the artwork of the same nation).
fn nation_name(card: &Card) -> String {
    let id = card.key.strip_prefix("art:").unwrap_or(&card.key);
    i18n::content("classic", "nation", id, "name", &card.name)
}

fn nation_slug(nation: &SourceNation) -> &str {
    nation.id.as_ref().and_then(|id| id.0.rsplit('/').next()).unwrap_or("")
}
fn collection_tabs(ui: &mut egui::Ui, rect: Rect, state: &mut NationsState) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal(|ui| {
            let tabs = [
                (Collection::Game, tf!("classic.ui.screen-nations.tab-game", n = state.roster.len())),
                (Collection::Artwork, tf!("classic.ui.screen-nations.tab-artwork", n = state.art_roster.len())),
            ];
            for (mode, label) in tabs {
                let active = state.collection == mode;
                let button = egui::Button::new(egui::RichText::new(label).color(if active { GOLD } else { CREAM }))
                    .fill(if active { Color32::from_rgb(37,65,66) } else { RAISED })
                    .stroke(Stroke::new(1., if active { GOLD } else { MUTED }));
                if ui.add_sized(vec2(137., 28.), button).clicked() {
                    state.collection = mode;
                    state.scroll_to_selection = true;
                }
            }
        });
    });
}

pub(super) fn draw(ui: &mut egui::Ui, r: Rect, state: &mut NationsState, brightness: f32) {
    let roster = Rect::from_min_size(r.min, vec2(310.0, r.height()));
    panel(ui.painter(), roster);
    collection_tabs(ui, Rect::from_min_size(roster.min + vec2(10., 39.), vec2(290., 28.)), state);
    let (cards, selected) = match state.collection {
        Collection::Game => (&state.roster, &mut state.selected),
        Collection::Artwork => (&state.art_roster, &mut state.selected_art),
    };
    text(
        ui.painter(),
        roster.min + vec2(18.0, 17.0),
        t!("classic.ui.screen-nations.heading"),
        13.0,
        GOLD,
    );
    ui.painter().text(
        roster.right_top() + vec2(-18.0, 17.0),
        Align2::RIGHT_TOP,
        cards.len(),
        FontId::proportional(13.0),
        MUTED,
    );
    let list = Rect::from_min_max(roster.min + vec2(10.0, 78.0), roster.max - vec2(10.0, 34.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("nation_roster")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .max_height(list.height())
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for (index, nation) in cards.iter().enumerate() {
                    let (row, response) =
                        ui.allocate_exact_size(vec2(list.width() - 12.0, 68.0), Sense::click());
                    if response.clicked() {
                        *selected = index;
                    }
                    let name = nation_name(nation);
                    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, *selected == index, &name));
                    let selected = *selected == index;
                    if selected && state.scroll_to_selection {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    if !ui.is_rect_visible(row) { continue; }
                    ui.painter().rect_filled(
                        row,
                        5.0,
                        if selected {
                            Color32::from_rgb(37, 65, 66)
                        } else {
                            RAISED
                        },
                    );
                    if selected {
                        ui.painter().rect_stroke(
                            row,
                            5.0,
                            Stroke::new(1.3, GOLD),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let emblem = Rect::from_min_size(row.min + vec2(3.0, 2.0), vec2(64.0, 64.0));
                    state
                        .artwork
                        .paint(ui, emblem, &nation.key, Asset::Emblem, brightness);
                    text(
                        ui.painter(),
                        row.min + vec2(77.0, 13.0),
                        &name,
                        18.0,
                        CREAM,
                    );
                    paragraph(
                        ui.painter(),
                        row.min + vec2(78.0, 39.0),
                        &nation.leader,
                        12.0,
                        MUTED,
                        row.width() - 86.0,
                    );
                }
            });
    });
    state.scroll_to_selection = false;
    let cue = if cards.len() as f32 * 74.0 - 6.0 > list.height() {
        t!("classic.ui.screen-nations.scroll-more")
    } else {
        t!("classic.ui.screen-nations.all-shown")
    };
    text(
        ui.painter(),
        pos2(roster.left() + 18.0, roster.bottom() - 23.0),
        cue,
        11.0,
        GOLD,
    );
    if cards.is_empty() {
        return;
    }
    let nation = &cards[(*selected).min(cards.len() - 1)];
    let scene = Rect::from_min_max(pos2(roster.right() + 18.0, r.top()), r.max);
    panel(ui.painter(), scene);
    state.artwork.paint(
        ui,
        scene.shrink(1.0),
        &nation.key,
        Asset::Background,
        brightness,
    );
    darken(ui.painter(), scene, 15, 155);
    let portrait_h = (scene.height() - 202.0).min(600.0);
    let portrait_w = (scene.width() * 0.40).min(portrait_h * 0.70);
    let frame = Rect::from_min_size(
        pos2(scene.right() - portrait_w - 26.0, scene.top() + 26.0),
        vec2(portrait_w, portrait_w),
    );
    ui.painter().rect_filled(frame.expand(5.0), 3.0, BG);
    ui.painter().rect_stroke(
        frame.expand(5.0),
        3.0,
        Stroke::new(1.5, GOLD),
        egui::StrokeKind::Inside,
    );
    state
        .artwork
        .paint(ui, frame, &nation.key, Asset::Portrait, brightness);
    let plaque = Rect::from_min_size(
        pos2(scene.left() + 24.0, scene.bottom() - 156.0),
        vec2(scene.width() - 48.0, 132.0),
    );
    ui.painter()
        .rect_filled(plaque, 4.0, Color32::from_black_alpha(185));
    let emblem = Rect::from_min_size(plaque.min + vec2(10.0, 19.0), vec2(94.0, 94.0));
    state
        .artwork
        .paint(ui, emblem, &nation.key, Asset::Emblem, brightness);
    text(
        ui.painter(),
        plaque.min + vec2(122.0, 12.0),
        &nation_name(nation),
        38.0,
        CREAM,
    );
    text(
        ui.painter(),
        plaque.min + vec2(124.0, 62.0),
        &nation.leader,
        21.0,
        GOLD,
    );
    text(
        ui.painter(),
        plaque.min + vec2(125.0, 101.0),
        t!("classic.ui.screen-nations.asset-list"),
        10.0,
        MUTED,
    );
    let label = Rect::from_min_size(scene.min + vec2(24.0, 26.0), vec2(209.0, 31.0));
    ui.painter()
        .rect_filled(label, 3.0, Color32::from_black_alpha(180));
    text(
        ui.painter(),
        label.min + vec2(12.0, 9.0),
        if state.collection == Collection::Artwork { t!("classic.ui.screen-nations.label-artwork") } else { t!("classic.ui.screen-nations.label-game") },
        11.0,
        GOLD,
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Asset {
    Emblem,
    Portrait,
    Background,
}
impl Asset {
    fn name(self) -> &'static str {
        match self {
            Self::Emblem => "emblem",
            Self::Portrait => "portrait",
            Self::Background => "background",
        }
    }
    fn max_dimension(self) -> u32 {
        match self {
            Self::Emblem => 192,
            Self::Portrait => 768,
            Self::Background => 1664,
        }
    }
}
#[derive(Default)]
pub(super) struct NationArt {
    // A bounded LRU covers visible roster emblems; large textures retain two recent pairs.
    emblems: HashMap<String, egui::TextureHandle>,
    emblem_order: VecDeque<String>,
    scenes: VecDeque<(String, egui::TextureHandle)>,
    attempted: HashMap<String, f64>,
    pending: std::collections::HashSet<String>,
    worker: Option<NationArtWorker>,
    /// A browser page has no thread to decode on: the frame that asks decodes (one sheet,
    /// retried after `attempted` when its file has not arrived yet).
    #[cfg(target_arch = "wasm32")]
    inline: catalog::Decoder,
}
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct NationArtWorker {
    send: std::sync::mpsc::SyncSender<(String, Asset)>,
    receive: std::sync::mpsc::Receiver<(String, Asset, Option<egui::ColorImage>)>,
}
impl NationArt {
    pub(super) fn paint(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        slug: &str,
        asset: Asset,
        brightness: f32,
    ) {
        self.paint_captioned(ui, rect, slug, asset, brightness, true);
    }
    /// `paint`; `caption` says whether a picture still on its way is announced in words (a small
    /// card has no room for them: it shows the plain panel until the picture is there).
    pub(super) fn paint_captioned(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        slug: &str,
        asset: Asset,
        brightness: f32,
        caption: bool,
    ) {
        let Some(texture) = self.texture(ui.ctx(), slug, asset) else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(2));
            ui.painter().rect_filled(rect, 0.0, PANEL);
            if asset != Asset::Emblem && caption {
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    t!("classic.ui.screen-nations.art-arriving"),
                    FontId::proportional(16.0),
                    MUTED,
                );
            }
            return;
        };
        let size = texture.size_vec2();
        let (destination, uv) = if asset == Asset::Background {
            let scale = (rect.width() / size.x).max(rect.height() / size.y);
            (
                rect,
                Rect::from_center_size(pos2(0.5, 0.5), rect.size() / (size * scale)),
            )
        } else {
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            (
                Rect::from_center_size(rect.center(), size * scale),
                Rect::from_min_max(egui::Pos2::ZERO, pos2(1.0, 1.0)),
            )
        };
        ui.painter().image(
            texture.id(),
            destination,
            uv,
            Color32::from_gray((brightness * 255.0).clamp(0.0, 255.0) as u8),
        );
    }
    fn texture(
        &mut self,
        ctx: &egui::Context,
        slug: &str,
        asset: Asset,
    ) -> Option<egui::TextureHandle> {
        if client::shell::art::skip_offline(ctx, slug) {
            return None;
        }
        let ready: Vec<_> = self
            .worker
            .as_ref()
            .map(|w| w.receive.try_iter().collect())
            .unwrap_or_default();
        for (key, asset, pixels) in ready {
            self.pending.remove(&key);
            if let Some(pixels) = pixels {
                self.attempted.remove(&key);
                self.install(ctx, key, asset, pixels);
            }
        }
        let key = format!("{slug}/{}", asset.name());
        if asset == Asset::Emblem {
            if let Some(texture) = self.emblems.get(&key) {
                self.emblem_order.retain(|id| id != &key);
                self.emblem_order.push_back(key.clone());
                return Some(texture.clone());
            }
        } else if let Some(index) = self.scenes.iter().position(|(id, _)| id == &key) {
            let cached = self.scenes.remove(index).unwrap();
            let texture = cached.1.clone();
            self.scenes.push_back(cached);
            return Some(texture);
        }
        #[cfg(target_arch = "wasm32")]
        return self.page_picture(ctx, slug, asset, key);
        #[cfg(not(target_arch = "wasm32"))]
        self.worker_picture(ctx, slug, asset, key)
    }
    /// A browser page: the page's art service fetches and decodes (off this thread) what the frame
    /// asks for: the portrait and the background first, then the roster's emblems.
    #[cfg(target_arch = "wasm32")]
    fn page_picture(&mut self, ctx: &egui::Context, slug: &str, asset: Asset, key: String) -> Option<egui::TextureHandle> {
        use client::shell::assets::web;
        let request = self.inline.request(slug, asset)?;
        web::trace_once("ask", &key);
        web::begin_picture_pass(ctx.cumulative_pass_nr());
        web::want_picture(&request);
        if web::has_picture(&request.key) {
            if let Some(Some(pixels)) = web::take_picture(&request.key) {
                self.install(ctx, key.clone(), asset, pixels);
                web::trace_once("shown", &key);
                return if asset == Asset::Emblem {
                    self.emblems.get(&key).cloned()
                } else {
                    self.scenes.iter().find(|(id, _)| id == &key).map(|(_, texture)| texture.clone())
                };
            }
        }
        None
    }
    /// A desktop: a worker thread decodes, at most once in two seconds per picture.
    #[cfg(not(target_arch = "wasm32"))]
    fn worker_picture(&mut self, ctx: &egui::Context, slug: &str, asset: Asset, key: String) -> Option<egui::TextureHandle> {
        let now = ctx.input(|input| input.time);
        if self
            .attempted
            .get(&key)
            .is_some_and(|last| now - last < 2.0)
        {
            return None;
        }
        self.attempted.insert(key.clone(), now);
        if self.pending.contains(&key) {
            return None;
        }
        if self.worker.is_none() {
            let (send, requests) = std::sync::mpsc::sync_channel::<(String, Asset)>(32);
            let (results, receive) = std::sync::mpsc::sync_channel(32);
            let repaint = ctx.clone();
            client::shell::task::spawn(move || {
                let mut decoder = catalog::Decoder::default();
                for (slug, asset) in requests {
                    let pixels = decoder.decode(&slug, asset);
                    let key = format!("{slug}/{}", asset.name());
                    if results.send((key, asset, pixels)).is_err() {
                        break;
                    }
                    repaint.request_repaint();
                }
            });
            self.worker = Some(NationArtWorker { send, receive });
        }
        if self
            .worker
            .as_ref()?
            .send
            .try_send((slug.to_owned(), asset))
            .is_ok()
        {
            self.pending.insert(key);
        }
        None
    }
    fn install(
        &mut self,
        ctx: &egui::Context,
        key: String,
        asset: Asset,
        pixels: egui::ColorImage,
    ) {
        let texture = ctx.load_texture(
            format!("nation/{key}"),
            pixels,
            egui::TextureOptions::LINEAR,
        );
        if asset == Asset::Emblem {
            self.emblem_order.retain(|id| id != &key);
            self.emblem_order.push_back(key.clone());
            self.emblems.insert(key, texture);
            while self.emblem_order.len() > 24 {
                if let Some(old) = self.emblem_order.pop_front() { self.emblems.remove(&old); }
            }
        } else {
            self.cache_scene(key, texture);
        }
    }
    pub(super) fn preload(&mut self, ctx: &egui::Context, slugs: &[&str], selected: &str) {
        // A page asks its art service for what the frame paints; decoding every sheet here would stall it.
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (ctx, slugs, selected);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut decoder = catalog::Decoder::default();
            for slug in slugs.iter().copied().chain(std::iter::once(selected)) {
                if let Some(pixels) = decoder.decode(slug, Asset::Emblem) {
                    self.install(ctx, format!("{slug}/emblem"), Asset::Emblem, pixels);
                }
            }
            for asset in [Asset::Portrait, Asset::Background] {
                if let Some(pixels) = decoder.decode(selected, asset) {
                    self.install(ctx, format!("{selected}/{}", asset.name()), asset, pixels);
                }
            }
        }
    }
    fn cache_scene(&mut self, key: String, texture: egui::TextureHandle) {
        self.scenes.push_back((key, texture));
        while self.scenes.len() > 4 {
            self.scenes.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roster_and_selection_use_every_actual_content_id() {
        let mut state = NationsState::default();
        let source = classic_content::nations::nations();
        assert_eq!(source.len(), 43, "BNW nations");
        assert_eq!(
            state.roster.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(),
            source.iter().map(nation_slug).collect::<Vec<_>>()
        );
        for nation in source {
            assert!(state.select(nation_slug(&nation)));
            assert_eq!(state.roster[state.selected].key, nation_slug(&nation));
        }
        let before = state.selected;
        assert!(!state.select("unknown"));
        assert_eq!(state.selected, before);
    }
    #[test]
    fn large_texture_cache_evicts_old_pairs_and_retains_four_only() {
        let ctx = egui::Context::default();
        let mut art = NationArt::default();
        for index in 0..20 {
            let key = format!("test{index}/portrait");
            let texture = ctx.load_texture(
                &key,
                egui::ColorImage::filled([1, 1], Color32::WHITE),
                egui::TextureOptions::LINEAR,
            );
            art.cache_scene(key, texture);
            assert!(art.scenes.len() <= 4);
        }
        assert_eq!(art.scenes.front().unwrap().0, "test16/portrait");
        assert_eq!(art.scenes.back().unwrap().0, "test19/portrait");
        ctx.tex_manager().write().take_delta().clear();
    }
}
