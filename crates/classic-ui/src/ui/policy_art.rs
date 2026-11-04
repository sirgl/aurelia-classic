//! Artwork inventory, independent of policy rules and simulation state.
use super::*;
use i18n::{t, tf};
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;
use web_time::Instant;
const CAPACITY: usize = 80;
const RETRY: Duration = Duration::from_secs(2);
#[derive(Clone, Copy)]
struct Item {
    id: &'static str,
    name: &'static str,
    parent: &'static str,
    kind: &'static str,
}
fn catalog() -> Vec<Item> {
    include_str!("../../../../assets/visual/generated/policies/catalog.tsv")
        .lines()
        .skip(1)
        .map(|line| {
            let mut c = line.split('\t');
            Item {
                id: c.next().unwrap(),
                name: c.next().unwrap(),
                parent: c.next().unwrap(),
                kind: c.next().unwrap(),
            }
        })
        .collect()
}
enum ImageState {
    Loading,
    Missing(Instant),
    Ready(egui::TextureHandle),
}
struct Entry {
    image: ImageState,
    used: u64,
}
type Pixels = Option<egui::ColorImage>;
struct Worker {
    send: SyncSender<&'static str>,
    receive: Receiver<(&'static str, Pixels)>,
}
#[derive(Default)]
pub(super) struct PolicyArt {
    pub collection: bool,
    group: &'static str,
    selected: Option<&'static str>,
    cache: BTreeMap<&'static str, Entry>,
    clock: u64,
    worker: Option<Worker>,
}
struct DecodedAtlas {
    path: std::path::PathBuf,
    image: image::DynamicImage,
}
#[derive(Default)]
struct ArtDecoder {
    atlas: Option<DecodedAtlas>,
}
impl ArtDecoder {
    fn decode(&mut self, id: &str) -> Pixels {
        let root = client::shell::assets::path("visual/generated/policies");
        let path = root.join(id).join("emblem-v1.png");
        let source = if client::shell::assets::image_exists(&path) {
            client::shell::assets::open_image(&path)?
        } else {
            self.atlas_region(&root, id)?
        };
        let image = source.resize(192, 192, image::imageops::FilterType::Triangle).to_rgba8();
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize], image.as_raw(),
        ))
    }
    fn atlas_region(&mut self, root: &std::path::Path, id: &str) -> Option<image::DynamicImage> {
        let index = client::shell::assets::read_to_string(&root.join("atlas-index.tsv"))?;
        let columns: Vec<_> = index.lines().skip(1)
            .find(|line| line.split('\t').next() == Some(id))?.split('\t').collect();
        if columns.len() != 6 { return None; }
        let path = root.join(columns[1]);
        if self.atlas.as_ref().is_none_or(|atlas| atlas.path != path) {
            self.atlas = Some(DecodedAtlas { image: client::shell::assets::open_image(&path)?, path });
        }
        let image = &self.atlas.as_ref()?.image;
        let x: u32 = columns[2].parse().ok()?;
        let y: u32 = columns[3].parse().ok()?;
        let width: u32 = columns[4].parse().ok()?;
        let height: u32 = columns[5].parse().ok()?;
        if width == 0 || height == 0 || x.checked_add(width)? > image.width()
            || y.checked_add(height)? > image.height() { return None; }
        Some(image.crop_imm(x, y, width, height))
    }
}

/// The catalog's static id of an art id (the game model holds owned strings).
pub fn catalog_id(id: &str) -> Option<&'static str> {
    static CATALOG: std::sync::OnceLock<Vec<Item>> = std::sync::OnceLock::new();
    let items = CATALOG.get_or_init(catalog);
    items.iter().find(|i| i.id == id).map(|i| i.id)
}

impl PolicyArt {
    /// Texture of an art id that may be unknown to the catalog (None then).
    pub fn texture_of(&mut self, ctx: &egui::Context, id: &str) -> Option<egui::TextureId> {
        let id = catalog_id(id)?;
        self.texture(ctx, id)
    }

    /// Synchronous decoding of the given ids, for offscreen captures only.
    pub fn preload_ids(&mut self, ctx: &egui::Context, ids: &[String]) {
        let mut decoder = ArtDecoder::default();
        for id in ids {
            if let Some(id) = catalog_id(id) {
                self.install(ctx, id, decoder.decode(id));
            }
        }
    }

    pub fn select_group(&mut self, group: &str) -> bool {
        let Some(item) = catalog()
            .into_iter()
            .find(|i| i.parent.is_empty() && i.id == group)
        else {
            return false;
        };
        self.collection = true;
        self.group = item.id;
        self.selected = None;
        true
    }
    fn start_worker(&mut self, ctx: &egui::Context) {
        if self.worker.is_some() {
            return;
        }
        let (send, requests) = mpsc::sync_channel::<&'static str>(CAPACITY);
        let (results, receive) = mpsc::sync_channel(CAPACITY);
        let repaint = ctx.clone();
        client::shell::task::spawn(move || {
            let mut decoder = ArtDecoder::default();
            for id in requests {
                if results.send((id, decoder.decode(id))).is_err() {
                    break;
                }
                repaint.request_repaint();
            }
        });
        self.worker = Some(Worker { send, receive });
    }
    fn install(&mut self, ctx: &egui::Context, id: &'static str, pixels: Pixels) {
        let image = match pixels {
            Some(p) => ImageState::Ready(ctx.load_texture(
                format!("policy-art-{id}"),
                p,
                egui::TextureOptions::LINEAR,
            )),
            None => ImageState::Missing(Instant::now()),
        };
        self.clock += 1;
        if !self.cache.contains_key(id) && self.cache.len() >= CAPACITY {
            let oldest = *self.cache.iter().min_by_key(|(_, e)| e.used).unwrap().0;
            self.cache.remove(oldest);
        }
        self.cache.insert(
            id,
            Entry {
                image,
                used: self.clock,
            },
        );
    }
    pub fn texture(&mut self, ctx: &egui::Context, id: &'static str) -> Option<egui::TextureId> {
        if client::shell::art::skip_offline(ctx, id) {
            return None;
        }
        self.start_worker(ctx);
        let ready: Vec<_> = self.worker.as_ref().unwrap().receive.try_iter().collect();
        for (key, pixels) in ready {
            if self.cache.contains_key(key) {
                self.install(ctx, key, pixels);
            }
        }
        self.clock += 1;
        let mut request = true;
        if let Some(entry) = self.cache.get_mut(id) {
            entry.used = self.clock;
            match &entry.image {
                ImageState::Ready(t) => return Some(t.id()),
                ImageState::Loading => request = false,
                ImageState::Missing(at) => request = at.elapsed() >= RETRY,
            }
        }
        if request && self.worker.as_ref().unwrap().send.try_send(id).is_ok() {
            if !self.cache.contains_key(id) && self.cache.len() >= CAPACITY {
                let oldest = *self.cache.iter().min_by_key(|(_, e)| e.used).unwrap().0;
                self.cache.remove(oldest);
            }
            self.cache.insert(
                id,
                Entry {
                    image: ImageState::Loading,
                    used: self.clock,
                },
            );
        }
        ctx.request_repaint_after(RETRY);
        None
    }
    /// Explicit synchronous preparation for offscreen captures only.
    pub fn preload(&mut self, ctx: &egui::Context) {
        let group = if self.group.is_empty() {
            "tradition"
        } else {
            self.group
        };
        let mut decoder = ArtDecoder::default();
        for item in catalog()
            .into_iter()
            .filter(|i| i.id == group || i.parent == group)
        {
            self.install(ctx, item.id, decoder.decode(item.id));
        }
    }
}
fn emblem(ui: &mut egui::Ui, r: Rect, id: &'static str, state: &mut PolicyArt) {
    if let Some(texture) = state.texture(ui.ctx(), id) {
        ui.painter().image(
            texture,
            r,
            Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)),
            Color32::WHITE,
        );
    } else {
        ui.painter().circle_stroke(
            r.center(),
            r.width() * 0.40,
            Stroke::new(1., GOLD.gamma_multiply(0.45)),
        );
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            t!("classic.ui.policy-art.generating"),
            FontId::proportional(10.),
            MUTED,
        );
    }
}
pub(super) fn draw(ui: &mut egui::Ui, r: Rect, state: &mut PolicyArt) {
    if state.group.is_empty() {
        state.group = "tradition";
    }
    let items = catalog();
    let groups: Vec<_> = items.iter().filter(|i| i.parent.is_empty()).collect();
    let columns = ((r.width() / 140.).floor() as usize).clamp(3, 6);
    let width = (r.width() - 8. * (columns - 1) as f32) / columns as f32;
    for (index, item) in groups.iter().enumerate() {
        let rect = Rect::from_min_size(
            r.min
                + vec2(
                    (index % columns) as f32 * (width + 8.),
                    (index / columns) as f32 * 37.,
                ),
            vec2(width, 30.),
        );
        if button(ui, rect, item.name, state.group == item.id, true) {
            state.select_group(item.id);
        }
    }
    let heading_y = r.top() + groups.len().div_ceil(columns) as f32 * 37. + 8.;
    let header = groups.iter().find(|i| i.id == state.group).unwrap();
    emblem(
        ui,
        Rect::from_min_size(pos2(r.left() + 4., heading_y), vec2(72., 72.)),
        header.id,
        state,
    );
    text(
        ui.painter(),
        pos2(r.left() + 91., heading_y + 4.),
        header.name,
        26.,
        CREAM,
    );
    text(
        ui.painter(),
        pos2(r.left() + 92., heading_y + 41.),
        tf!("classic.ui.policy-art.header-line", kind = header.kind),
        12.,
        MUTED,
    );
    let body = Rect::from_min_max(pos2(r.left(), heading_y + 85.), r.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("policy_art_grid")
            .max_height(body.height())
            .show(ui, |ui| {
                let columns = ((body.width() / 230.).floor() as usize).max(1);
                let card_width = (body.width() - 12. * columns as f32) / columns as f32;
                let children: Vec<_> = items.iter().filter(|i| i.parent == state.group).collect();
                for row in children.chunks(columns) {
                    ui.horizontal(|ui| {
                        for item in row {
                            let (card, response) =
                                ui.allocate_exact_size(vec2(card_width, 160.), Sense::click());
                            panel(ui.painter(), card);
                            if state.selected == Some(item.id) {
                                ui.painter().rect_stroke(
                                    card,
                                    7.,
                                    Stroke::new(1.5, GOLD),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            emblem(
                                ui,
                                Rect::from_min_size(card.min + vec2(15., 12.), vec2(72., 72.)),
                                item.id,
                                state,
                            );
                            emblem(
                                ui,
                                Rect::from_min_size(card.min + vec2(110., 32.), vec2(48., 48.)),
                                item.id,
                                state,
                            );
                            text(ui.painter(), card.min + vec2(27., 90.), "72 px", 10., MUTED);
                            text(
                                ui.painter(),
                                card.min + vec2(119., 90.),
                                "48 px",
                                10.,
                                MUTED,
                            );
                            paragraph(
                                ui.painter(),
                                card.min + vec2(14., 114.),
                                item.name,
                                15.,
                                CREAM,
                                card_width - 28.,
                            );
                            response
                                .clone()
                                .on_hover_text(tf!("classic.ui.policy-art.item-tip", name = item.name));
                            if response.clicked() {
                                state.selected = Some(item.id);
                            }
                        }
                    });
                    ui.add_space(8.);
                }
            });
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_clicks_toggle_collection_select_group_and_preview_only() {
        let ctx = egui::Context::default();
        let mut state = ScreenState::default();
        state.dev_mode = true;
        let screen = Rect::from_min_size(pos2(0., 0.), vec2(1100., 700.));
        fn frame(
            ctx: &egui::Context,
            state: &mut ScreenState,
            screen: Rect,
            events: Vec<egui::Event>,
        ) {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    progress::civics(ui, screen, state);
                },
            );
            output.textures_delta.clear();
        }
        fn click(ctx: &egui::Context, state: &mut ScreenState, screen: Rect, pos: egui::Pos2) {
            frame(
                ctx,
                state,
                screen,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            frame(
                ctx,
                state,
                screen,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
        frame(&ctx, &mut state, screen, vec![]);
        frame(&ctx, &mut state, screen, vec![]);
        click(&ctx, &mut state, screen, pos2(990., 45.));
        assert!(state.policy_art.collection);
        frame(&ctx, &mut state, screen, vec![]);
        click(&ctx, &mut state, screen, pos2(275., 123.));
        assert_eq!(state.policy_art.group, "liberty");
        click(&ctx, &mut state, screen, pos2(60., 320.));
        assert!(state.policy_art.selected.unwrap().starts_with("liberty/"));
        assert!(state.adopted.is_empty());
        assert_eq!(state.policy_points, 2);
        click(&ctx, &mut state, screen, pos2(990., 45.));
        assert!(!state.policy_art.collection);
        assert!(state.select_policy_art("autocracy"));
        assert_eq!(state.screen, Screen::Civics);
    }
    #[test]
    fn inventory_has_all_twelve_groups_and_105_symbols() {
        let items = catalog();
        assert_eq!(items.len(), 105);
        assert_eq!(
            items.iter().map(|i| i.id).collect::<BTreeSet<_>>().len(),
            105
        );
        let groups: Vec<_> = items.iter().filter(|i| i.parent.is_empty()).collect();
        assert_eq!(groups.len(), 12);
        for group in groups {
            assert_eq!(
                items.iter().filter(|i| i.parent == group.id).count(),
                if group.kind == "branch" { 5 } else { 16 }
            );
        }
    }
    #[test]
    fn cache_remains_bounded_and_evicts_least_recent() {
        let ctx = egui::Context::default();
        let mut state = PolicyArt::default();
        for item in catalog() {
            state.install(
                &ctx,
                item.id,
                Some(egui::ColorImage::new([2, 2], vec![Color32::WHITE; 4])),
            );
            assert!(state.cache.len() <= CAPACITY);
        }
        assert_eq!(state.cache.len(), CAPACITY);
        assert!(!state.cache.contains_key("tradition"));
        assert!(state.select_group("freedom"));
        assert_eq!(state.group, "freedom");
        assert!(!state.select_group("unknown"));
    }
}
