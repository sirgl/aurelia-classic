//! Unit order panel. In the game every button carries the core's verdict (`UnitOrders`);
//! in the showcase, without a game, the buttons only preview artwork.
use super::{GOLD, INK, Icon, LIGHT, MUTED, icon, panel, text};
use i18n::{t, tf};
use super::layout::{place, Anchor, Panel};

/// The order panel's area and its place in the HUD layout.
const ORDERS: &str = "visual_unit_actions";
use crate::ui::model::{ActionState, Availability, UnitOrders};
use classic_rules::ids::{PromotionId, UnitHandle};
use crate::ui::unit_kinds::{UnitKind, UnitKinds, UnitRole};
use render::world::state::{RenderWorld, UnitMarker};
use egui::{Align2, Color32, Rect, Sense, Stroke, TextureHandle, pos2, vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Action {
    Move,
    Attack,
    Fortify,
    Skip,
    Heal,
    Sleep,
    FoundCity,
    Improve,
    Road,
    Chop,
    Spread,
    RemoveHeresy,
    HolySite,
    GreatWork,
    Treatise,
    Concert,
    Excavate,
    /// Great person: build its own improvement (academy, manufactory, ...).
    GreatImprovement,
    /// Great scientist: science of the last turns.
    Discover,
    /// Great engineer: production in the city it stands in.
    HurryWonder,
    /// Great artist: a golden age.
    GoldenAge,
    /// Great merchant: a trade mission to a city-state.
    TradeMission,
    Promote,
    Upgrade,
    SetUp,
    Pillage,
    Intercept,
    /// Trade unit: open the trade route panel.
    TradeRoute,
    /// Combat unit: plunder the enemy trade route here.
    Plunder,
    /// Merchant of Venice: buy the city-state it stands in (art: none yet).
    BuyCityState,
    /// Nau: sell the exotic cargo next to a foreign land (art: none yet).
    SellGoods,
    /// Fighter: sweep a tile for interceptors (a targeting mode; art: none yet).
    Sweep,
}
/// Number of actions (texture slots).
const ACTION_COUNT: usize = 32;
/// Actions with no painted medallion yet: the panel draws their fallback icon.
#[cfg(test)]
const WITHOUT_ART: [Action; 3] = [Action::BuyCityState, Action::SellGoods, Action::Sweep];
#[path = "unit_action_art.rs"]
mod action_art;
/// Stable enum order keeps each atlas contiguous during one-shot decode.
const ALL: [Action; ACTION_COUNT] = [
    Action::Move, Action::Attack, Action::Fortify, Action::Skip, Action::Heal,
    Action::Sleep, Action::FoundCity, Action::Improve, Action::Road,
    Action::Chop, Action::Spread, Action::RemoveHeresy, Action::HolySite,
    Action::GreatWork, Action::Treatise, Action::Concert, Action::Excavate,
    Action::GreatImprovement, Action::Discover, Action::HurryWonder,
    Action::GoldenAge, Action::TradeMission, Action::Promote, Action::Upgrade,
    Action::SetUp, Action::Pillage, Action::Intercept, Action::TradeRoute, Action::Plunder,
    Action::BuyCityState, Action::SellGoods, Action::Sweep,
];
const MILITARY: &[Action] = &[
    Action::Move,
    Action::Attack,
    Action::Fortify,
    Action::Heal,
    Action::Skip,
    Action::Sleep,
];
const SETTLER: &[Action] = &[Action::Move, Action::FoundCity, Action::Skip, Action::Sleep];
const WORKER: &[Action] = &[
    Action::Move,
    Action::Improve,
    Action::Road,
    Action::Skip,
    Action::Sleep,
];
const UNKNOWN: &[Action] = &[Action::Move, Action::Skip, Action::Sleep];
impl Action {
    fn id(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Attack => "attack",
            Self::Fortify => "fortify",
            Self::Skip => "skip",
            Self::Heal => "heal",
            Self::Sleep => "sleep",
            Self::FoundCity => "found-city",
            Self::Improve => "build-improvement",
            Self::Road => "build-road",
            Self::Chop => "chop",
            Self::Spread => "spread-religion",
            Self::RemoveHeresy => "remove-heresy",
            Self::HolySite => "holy-site",
            Self::GreatWork => "great-work",
            Self::Treatise => "treatise",
            Self::Concert => "concert",
            Self::Excavate => "excavate",
            Self::GreatImprovement => "great-improvement",
            Self::Discover => "discover",
            Self::HurryWonder => "hurry",
            Self::GoldenAge => "golden-age",
            Self::TradeMission => "trade-mission",
            Self::Promote => "promote",
            Self::Upgrade => "upgrade",
            Self::SetUp => "set-up",
            Self::Pillage => "pillage",
            Self::Intercept => "intercept",
            Self::TradeRoute => "trade-route",
            Self::Plunder => "plunder",
            Self::BuyCityState => "buy-city-state",
            Self::SellGoods => "sell-goods",
            Self::Sweep => "air-sweep",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Move => t!("classic.ui.unit-actions.move.label"),
            Self::Attack => t!("classic.ui.unit-actions.attack.label"),
            Self::Fortify => t!("classic.ui.unit-actions.fortify.label"),
            Self::Skip => t!("classic.ui.unit-actions.skip.label"),
            Self::Heal => t!("classic.ui.unit-actions.heal.label"),
            Self::Sleep => t!("classic.ui.unit-actions.sleep.label"),
            Self::FoundCity => t!("classic.ui.unit-actions.found-city.label"),
            Self::Improve => t!("classic.ui.unit-actions.improve.label"),
            Self::Road => t!("classic.ui.unit-actions.road.label"),
            Self::Chop => t!("classic.ui.unit-actions.chop.label"),
            Self::Spread => t!("classic.ui.unit-actions.spread.label"),
            Self::RemoveHeresy => t!("classic.ui.unit-actions.remove-heresy.label"),
            Self::HolySite => t!("classic.ui.unit-actions.holy-site.label"),
            Self::GreatWork => t!("classic.ui.unit-actions.great-work.label"),
            Self::Treatise => t!("classic.ui.unit-actions.treatise.label"),
            Self::Concert => t!("classic.ui.unit-actions.concert.label"),
            Self::Excavate => t!("classic.ui.unit-actions.excavate.label"),
            Self::GreatImprovement => t!("classic.ui.unit-actions.great-improvement.label"),
            Self::Discover => t!("classic.ui.unit-actions.discover.label"),
            Self::HurryWonder => t!("classic.ui.unit-actions.hurry-wonder.label"),
            Self::GoldenAge => t!("classic.ui.unit-actions.golden-age.label"),
            Self::TradeMission => t!("classic.ui.unit-actions.trade-mission.label"),
            Self::Promote => t!("classic.ui.unit-actions.promote.label"),
            Self::Upgrade => t!("classic.ui.unit-actions.upgrade.label"),
            Self::SetUp => t!("classic.ui.unit-actions.set-up.label"),
            Self::Pillage => t!("classic.ui.unit-actions.pillage.label"),
            Self::Intercept => t!("classic.ui.unit-actions.intercept.label"),
            Self::TradeRoute => t!("classic.ui.unit-actions.trade-route.label"),
            Self::Plunder => t!("classic.ui.unit-actions.plunder.label"),
            Self::BuyCityState => t!("classic.ui.unit-actions.buy-city-state.label"),
            Self::SellGoods => t!("classic.ui.unit-actions.sell-goods.label"),
            Self::Sweep => t!("classic.ui.unit-actions.sweep.label"),
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Move => t!("classic.ui.unit-actions.move.desc"),
            Self::Attack => t!("classic.ui.unit-actions.attack.desc"),
            Self::Fortify => t!("classic.ui.unit-actions.fortify.desc"),
            Self::Skip => t!("classic.ui.unit-actions.skip.desc"),
            Self::Heal => t!("classic.ui.unit-actions.heal.desc"),
            Self::Sleep => t!("classic.ui.unit-actions.sleep.desc"),
            Self::FoundCity => t!("classic.ui.unit-actions.found-city.desc"),
            Self::Improve => t!("classic.ui.unit-actions.improve.desc"),
            Self::Road => t!("classic.ui.unit-actions.road.desc"),
            Self::Chop => t!("classic.ui.unit-actions.chop.desc"),
            Self::Spread => t!("classic.ui.unit-actions.spread.desc"),
            Self::RemoveHeresy => t!("classic.ui.unit-actions.remove-heresy.desc"),
            Self::HolySite => t!("classic.ui.unit-actions.holy-site.desc"),
            Self::GreatWork => t!("classic.ui.unit-actions.great-work.desc"),
            Self::Treatise => t!("classic.ui.unit-actions.treatise.desc"),
            Self::Concert => t!("classic.ui.unit-actions.concert.desc"),
            Self::Excavate => t!("classic.ui.unit-actions.excavate.desc"),
            Self::GreatImprovement => t!("classic.ui.unit-actions.great-improvement.desc"),
            Self::Discover => t!("classic.ui.unit-actions.discover.desc"),
            Self::HurryWonder => t!("classic.ui.unit-actions.hurry-wonder.desc"),
            Self::GoldenAge => t!("classic.ui.unit-actions.golden-age.desc"),
            Self::TradeMission => t!("classic.ui.unit-actions.trade-mission.desc"),
            Self::Promote => t!("classic.ui.unit-actions.promote.desc"),
            Self::Upgrade => t!("classic.ui.unit-actions.upgrade.desc"),
            Self::SetUp => t!("classic.ui.unit-actions.set-up.desc"),
            Self::Pillage => t!("classic.ui.unit-actions.pillage.desc"),
            Self::Intercept => t!("classic.ui.unit-actions.intercept.desc"),
            Self::TradeRoute => t!("classic.ui.unit-actions.trade-route.desc"),
            Self::Plunder => t!("classic.ui.unit-actions.plunder.desc"),
            Self::BuyCityState => t!("classic.ui.unit-actions.buy-city-state.desc"),
            Self::SellGoods => t!("classic.ui.unit-actions.sell-goods.desc"),
            Self::Sweep => t!("classic.ui.unit-actions.sweep.desc"),
        }
    }
    fn fallback(self) -> Icon {
        match self {
            Self::Move => Icon::Compass,
            Self::Attack | Self::Fortify => Icon::Shield,
            Self::Skip => Icon::Hourglass,
            Self::Heal => Icon::Flask,
            Self::Sleep => Icon::Sun,
            Self::FoundCity => Icon::City,
            Self::Improve | Self::Road => Icon::Hammer,
            Self::Chop => Icon::Leaf,
            Self::Spread | Self::HolySite => Icon::Faith,
            Self::RemoveHeresy => Icon::Shield,
            Self::GreatWork | Self::Treatise | Self::Concert => Icon::Note,
            Self::Excavate => Icon::Hammer,
            Self::GreatImprovement => Icon::City,
            Self::Discover => Icon::Flask,
            Self::HurryWonder => Icon::Hammer,
            Self::GoldenAge => Icon::Sun,
            Self::TradeMission => Icon::Compass,
            Self::Promote => Icon::Sun,
            Self::Upgrade | Self::Pillage => Icon::Hammer,
            Self::SetUp => Icon::Hourglass,
            Self::Intercept => Icon::Shield,
            Self::Sweep => Icon::Compass,
            Self::TradeRoute => Icon::Coin,
            Self::Plunder | Self::BuyCityState | Self::SellGoods => Icon::Coin,
        }
    }
}
/// Preview actions by role (rules flags and model category), never by kind number.
fn actions_for(kind: Option<&UnitKind>) -> &'static [Action] {
    match kind.map(|k| k.role) {
        Some(UnitRole::Military) => MILITARY,
        Some(UnitRole::Founder) => SETTLER,
        Some(UnitRole::Builder) => WORKER,
        Some(UnitRole::Civilian) | None => UNKNOWN,
    }
}
fn selected_owned_unit(world: &RenderWorld, tile: Option<u32>) -> Option<UnitMarker> {
    let tile = tile?;
    if tile >= world.width.saturating_mul(world.height)
        || world.visible.get(tile as usize).copied().unwrap_or(0) == 0
    {
        return None;
    }
    world
        .units
        .iter()
        .find(|u| u.tile.0 == tile && u.owner == world.observer)
        .copied()
}

#[derive(Default)]
pub struct ActionPanelState {
    unit: Option<UnitMarker>,
    /// Preview actions of `unit`, fixed when it is selected.
    actions: &'static [Action],
    preview: Option<Action>,
    textures: [Option<TextureHandle>; ACTION_COUNT],
    loaded: bool,
    pending: Option<std::sync::mpsc::Receiver<(Action, egui::ColorImage)>>,
    /// The unit whose promotion list is open (game only).
    promotions_for: Option<UnitHandle>,
}
impl std::fmt::Debug for ActionPanelState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionPanelState")
            .field("unit", &self.unit)
            .field("preview", &self.preview)
            .finish_non_exhaustive()
    }
}
impl ActionPanelState {
    /// The Promote button opens the unit's promotion list, or closes it.
    pub(super) fn toggle_promotions(&mut self, unit: UnitHandle) {
        self.promotions_for = if self.promotions_for == Some(unit) {
            None
        } else {
            Some(unit)
        };
    }
    fn select(&mut self, unit: Option<UnitMarker>, actions: &'static [Action]) {
        if self.unit != unit {
            self.unit = unit;
            self.preview = None;
        }
        self.actions = actions;
    }
    fn preview(&mut self, action: Action) {
        if self.unit.is_some() && self.actions.contains(&action) {
            self.preview = Some(action);
        }
    }
    fn load_art(&mut self, ctx: &egui::Context) {
        if client::shell::art::skip_offline(ctx, "unit actions") {
            return;
        }
        if !self.loaded {
            self.loaded = true;
            let (send, receive) = std::sync::mpsc::sync_channel(8);
            self.pending = Some(receive);
            let repaint = ctx.clone();
            client::shell::task::spawn(move || {
                let mut decoder = action_art::Decoder::default();
                for action in ALL {
                    let Some(pixels) = decoder.decode(action) else { continue; };
                    if send.send((action, pixels)).is_err() {
                        break;
                    }
                    repaint.request_repaint();
                }
            });
        }
        let ready: Vec<_> = self
            .pending
            .as_ref()
            .map(|r| r.try_iter().collect())
            .unwrap_or_default();
        for (action, pixels) in ready {
            self.upload(ctx, action, pixels);
        }
    }
    fn upload(&mut self, ctx: &egui::Context, action: Action, pixels: egui::ColorImage) {
        self.textures[action as usize] = Some(ctx.load_texture(
            format!("unit-action-{}", action.id()),
            pixels,
            egui::TextureOptions::LINEAR,
        ));
    }
    /// Blocking warmup for explicit offscreen captures, never called by the live HUD.
    pub fn preload(&mut self, ctx: &egui::Context) {
        self.load_art(ctx);
        if let Some(receive) = self.pending.take() {
            for (action, pixels) in receive {
                self.upload(ctx, action, pixels);
            }
        }
    }
}

/// Where the panel's buttons come from.
#[derive(Clone, Copy)]
pub enum PanelSource<'a> {
    /// The game: orders of the selected own unit, or no panel.
    Game(Option<&'a UnitOrders>),
    /// The showcase: preview buttons for the own unit marker on the selected tile,
    /// its kind resolved through the ruleset's content keys.
    Showcase(&'a UnitKinds),
}

/// Shared paint, input and yield-exclusion bounds for the actual selected unit.
pub(super) fn panel_rect(
    screen: Rect,
    world: &RenderWorld,
    selected: Option<u32>,
    source: PanelSource<'_>,
) -> Option<Rect> {
    let (count, pitch) = match source {
        PanelSource::Game(orders) => {
            let orders = orders?;
            (orders.actions.len(), row_pitch(&orders.actions))
        }
        PanelSource::Showcase(kinds) => {
            (actions_for(kinds.get(selected_owned_unit(world, selected)?.kind)).len(), ROW)
        }
    };
    Some(panel_bounds(screen, count, pitch))
}

/// The order panel as it asks the HUD layout for room (it scrolls its orders when short).
pub(super) fn panel_spec(screen: Rect, world: &RenderWorld, selected: Option<u32>, source: PanelSource<'_>) -> Option<Panel> {
    let home = panel_rect(screen, world, selected, source)?;
    let min = vec2(home.width().min(304.), home.height().min(112.));
    Some(Panel::at(ORDERS, Anchor::BottomLeft, home).min(min).priority(100))
}

/// Height of one row of order buttons.
const ROW: f32 = 76.;
/// A row whose blocked order says why under its label is this much taller.
const REASON_ROW: f32 = 98.;

fn row_pitch(buttons: &[ActionState]) -> f32 {
    if buttons.iter().any(|b| !b.availability.is_ready()) { REASON_ROW } else { ROW }
}

fn panel_bounds(screen: Rect, count: usize, pitch: f32) -> Rect {
    let available = if screen.width() < 800. {
        (screen.width() - 200.).max(144.)
    } else {
        screen.width() - 40.
    };
    let columns = count.min(((available - 32.) / 64.).floor().max(1.) as usize);
    let rows = count.div_ceil(columns);
    let width = (columns as f32 * 64. + 32.).max(304.).min(available);
    let height = (56. + rows as f32 * pitch).min((screen.height() - 180.).max(112.));
    // Keep the two compact HUD panels apart at high display scaling.
    let bottom_gap = 89.;
    Rect::from_min_size(
        pos2(
            screen.left() + 20.,
            (screen.bottom() - bottom_gap - height).max(screen.top() + 90.),
        ),
        vec2(width, height),
    )
}

/// What the panel shows this frame: game orders, or the showcase preview.
struct PanelContent {
    title: String,
    status: String,
    buttons: Vec<ActionState>,
    active: Option<Action>,
    live: bool,
}

fn live_content(orders: &UnitOrders) -> PanelContent {
    let status = match orders.targeting {
        Some(Action::Move) => t!("classic.ui.unit-actions.target-move").to_owned(),
        Some(Action::Attack) => t!("classic.ui.unit-actions.target-attack").to_owned(),
        Some(Action::Sweep) => t!("classic.ui.unit-actions.target-sweep").to_owned(),
        Some(action) => tf!("classic.ui.unit-actions.target-other", action = action.label().to_lowercase()),
        None => orders.status.clone(),
    };
    PanelContent {
        title: orders.name.clone(),
        status,
        buttons: orders.actions.clone(),
        active: orders.targeting,
        live: true,
    }
}

fn preview_content(
    unit: UnitMarker,
    kinds: &UnitKinds,
    state: &ActionPanelState,
    width: f32,
) -> PanelContent {
    let status = if width < 250. {
        t!("classic.ui.unit-actions.preview-short").into()
    } else {
        state
            .preview
            .map(|action| tf!("classic.ui.unit-actions.preview-action", action = action.label()))
            .unwrap_or_else(|| t!("classic.ui.unit-actions.preview-only").into())
    };
    let kind = kinds.get(unit.kind);
    let buttons = actions_for(kind)
        .iter()
        .map(|&action| ActionState {
            action,
            availability: Availability::Ready,
            detail: None,
        })
        .collect();
    PanelContent {
        title: kind.map_or_else(|| t!("classic.ui.unit-actions.unit").to_owned(), |k| k.display_name()),
        status,
        buttons,
        active: state.preview,
        live: false,
    }
}

/// Draws the panel; returns the order clicked in the game (never in the showcase preview).
pub fn draw(
    ctx: &egui::Context,
    screen: Rect,
    world: &RenderWorld,
    selected_tile: Option<u32>,
    state: &mut ActionPanelState,
    source: PanelSource<'_>,
) -> Option<Action> {
    let bounds = place(ctx, panel_spec(screen, world, selected_tile, source)?)?;
    let content = match source {
        PanelSource::Game(orders) => live_content(orders?),
        PanelSource::Showcase(kinds) => {
            let unit = selected_owned_unit(world, selected_tile);
            let actions = unit.map_or(UNKNOWN, |u| actions_for(kinds.get(u.kind)));
            state.select(unit, actions);
            preview_content(state.unit?, kinds, state, bounds.width())
        }
    };
    state.load_art(ctx);
    let mut clicked = None;
    egui::Area::new(ORDERS.into())
        .order(egui::Order::Foreground)
        .fixed_pos(bounds.min)
        .constrain(false)
        .show(ctx, |ui| {
            clicked = draw_panel(ui, bounds.size(), &content, state);
        });
    if content.live {
        return clicked;
    }
    if let Some(action) = clicked {
        state.preview(action);
    }
    None
}

/// The open promotion list above the order panel; returns the promotion picked.
pub(super) fn promotion_picker(
    ctx: &egui::Context,
    screen: Rect,
    world: &RenderWorld,
    selected_tile: Option<u32>,
    state: &mut ActionPanelState,
    orders: &UnitOrders,
) -> Option<PromotionId> {
    if state.promotions_for != Some(orders.unit) || orders.promotions.is_empty() {
        return None;
    }
    let laid_out = crate::ui::hud::layout::current(ctx).rect(ORDERS);
    let bounds = laid_out.or_else(|| panel_rect(screen, world, selected_tile, PanelSource::Game(Some(orders))))?;
    let height = (44. + orders.promotions.len() as f32 * 34.).min((bounds.top()-screen.top()-98.).max(180.));
    let home = Rect::from_min_size(pos2(bounds.left(), bounds.top() - height - 8.), vec2(260., height));
    let r = place(ctx, Panel::at("promotion_picker", Anchor::BottomLeft, home).min(vec2(260., 112.)).priority(90))?;
    let mut picked = None;
    egui::Area::new("promotion_picker".into())
        .order(egui::Order::Foreground)
        .fixed_pos(r.min)
        .constrain(false)
        .show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(r.size(), Sense::hover());
            panel(ui.painter(), r, 7.);
            let title_at = r.min + vec2(14., 12.);
            text(ui.painter(), title_at, Align2::LEFT_TOP, t!("classic.ui.unit-actions.choose-promotion"), 10., GOLD);
            let list=Rect::from_min_max(r.min+vec2(10.,34.),r.max-vec2(10.,10.));
            let mut list_ui=ui.new_child(egui::UiBuilder::new().max_rect(list));
            list_ui.set_clip_rect(list.intersect(ui.clip_rect()));
            egui::ScrollArea::vertical().id_salt("promotion_choices").max_height(list.height()).auto_shrink([false,false]).show(&mut list_ui,|ui| {
                for choice in &orders.promotions {
                    let (row,_)=ui.allocate_exact_size(vec2(ui.available_width(),30.),Sense::hover());
                    let id=egui::Id::new(("promotion",choice.id.0));
                    if list_button(ui,row,"",id) {picked=Some(choice.id);}
                    crate::ui::screens::world_symbols::promotion(ui.painter(),Rect::from_min_size(row.min+vec2(4.,2.),vec2(26.,26.)),&choice.key,GOLD,INK);
                    let painter=ui.painter().with_clip_rect(row.shrink(4.).intersect(ui.clip_rect()));
                    text(&painter,row.min+vec2(38.,7.),Align2::LEFT_TOP,&choice.name,13.,LIGHT);
                    ui.interact(row,id.with("description"),Sense::hover()).on_hover_text(&choice.name);
                    ui.add_space(4.);
                }
            });
        });
    if picked.is_some() {
        state.promotions_for = None;
    }
    picked
}

/// A text row button of the promotion list.
fn list_button(ui: &mut egui::Ui, r: Rect, label: &str, id: egui::Id) -> bool {
    let response = ui.interact(r, id, Sense::click());
    let fill = if response.hovered() {
        Color32::from_rgb(40, 72, 83)
    } else {
        Color32::from_rgb(18, 34, 44)
    };
    ui.painter().rect_filled(r, 4., fill);
    let stroke = Stroke::new(1., GOLD.gamma_multiply(if response.hovered() { 1. } else { 0.5 }));
    ui.painter().rect_stroke(r, 4., stroke, egui::StrokeKind::Inside);
    text(ui.painter(), r.left_center() + vec2(12., 0.), Align2::LEFT_CENTER, label, 13., LIGHT);
    response.clicked()
}

fn draw_panel(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    content: &PanelContent,
    state: &ActionPanelState,
) -> Option<Action> {
    let (r, _) = ui.allocate_exact_size(size, Sense::hover());
    panel(ui.painter(), r, 7.);
    let title_at = r.min + vec2(16., 13.);
    text(ui.painter(), title_at, Align2::LEFT_TOP, &content.title, 17., LIGHT);
    let status_at = r.right_top() + vec2(-14., 16.);
    text(ui.painter(), status_at, Align2::RIGHT_TOP, &content.status, 10., GOLD);
    let count = content.buttons.len();
    let columns = count.min(((r.width() - 32.) / 64.).floor().max(1.) as usize);
    let rows = count.div_ceil(columns);
    let pitch = row_pitch(&content.buttons);
    if 56. + rows as f32 * pitch <= r.height() {
        let origin = r.min + vec2(12., 44.);
        return draw_action_rows(ui, origin, r.width() - 24., columns, content, state);
    }
    let body = Rect::from_min_max(r.min + vec2(8., 42.), r.max - vec2(8., 10.));
    let mut clicked = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("compact_actions_scroll")
            .max_height(body.height())
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
                let size = vec2(body.width() - 12., rows as f32 * pitch);
                let (canvas, _) = ui.allocate_exact_size(size, Sense::hover());
                let width = canvas.width();
                clicked = draw_action_rows(ui, canvas.min, width, columns, content, state);
            });
    });
    clicked
}

fn draw_action_rows(
    ui: &mut egui::Ui,
    origin: egui::Pos2,
    width: f32,
    columns: usize,
    content: &PanelContent,
    state: &ActionPanelState,
) -> Option<Action> {
    let step = width / columns as f32;
    let pitch = row_pitch(&content.buttons);
    let mut clicked = None;
    for (index, button) in content.buttons.iter().enumerate() {
        let center = origin
            + vec2(
                step * ((index % columns) as f32 + 0.5),
                26. + (index / columns) as f32 * pitch,
            );
        let active = content.active == Some(button.action);
        if draw_action(ui, center, (button, step), active, state) {
            clicked = Some(button.action);
        }
    }
    clicked
}

fn draw_action(
    ui: &mut egui::Ui,
    center: egui::Pos2,
    (button, step): (&ActionState, f32),
    active: bool,
    state: &ActionPanelState,
) -> bool {
    let action = button.action;
    let ready = button.availability.is_ready();
    let target = Rect::from_center_size(center + vec2(0., 9.), vec2(60., 74.));
    let id = egui::Id::new(("unit_action", action.id()));
    let response = ui.interact(target, id, Sense::click());
    if ready && (active || response.hovered()) {
        ui.painter()
            .circle_filled(center, 27., Color32::from_rgb(40, 72, 83));
        ui.painter()
            .circle_stroke(center, 27., Stroke::new(if active { 2. } else { 1. }, GOLD));
    }
    paint_action_icon(ui, center, action, ready, state);
    let ink = if active { LIGHT } else if ready { MUTED } else { MUTED.gamma_multiply(0.5) };
    let label_at = center + vec2(0., 32.);
    text(ui.painter(), label_at, Align2::CENTER_TOP, action.label(), 11., ink);
    if let Availability::Blocked(reason) = &button.availability {
        reason_lines(ui.painter(), label_at + vec2(0., 15.), reason, step - 4.);
    }
    let clicked = response.clicked() && ready;
    response.on_hover_text(action_hint(ui.ctx(), button));
    clicked
}

/// The reason an order is blocked, under its label: small, centred, at most two lines.
fn reason_lines(p: &egui::Painter, top: egui::Pos2, reason: &str, width: f32) {
    let color = Color32::from_rgb(222, 128, 112);
    let galley = p.layout(reason.to_owned(), egui::FontId::proportional(9.), color, width);
    let at = pos2(top.x - galley.size().x * 0.5, top.y);
    let clip = Rect::from_min_size(pos2(top.x - width * 0.5, top.y), vec2(width, 24.));
    p.with_clip_rect(clip).galley(at, galley, color);
}

fn paint_action_icon(
    ui: &egui::Ui,
    center: egui::Pos2,
    action: Action,
    ready: bool,
    state: &ActionPanelState,
) {
    let button = Rect::from_center_size(center, vec2(52., 52.));
    let tint = if ready { Color32::WHITE } else { Color32::from_gray(90) };
    if let Some(texture) = &state.textures[action as usize] {
        let uv = Rect::from_min_max(pos2(0., 0.), pos2(1., 1.));
        ui.painter().image(texture.id(), button, uv, tint);
        return;
    }
    ui.painter().circle_filled(center, 25., INK);
    ui.painter()
        .circle_stroke(center, 25., Stroke::new(1., GOLD.gamma_multiply(if ready { 1. } else { 0.4 })));
    let ink = if ready { LIGHT } else { MUTED.gamma_multiply(0.6) };
    icon(ui.painter(), center, 15., action.fallback(), ink);
}

fn action_hint(ctx: &egui::Context, button: &ActionState) -> String {
    let action = button.action;
    let what = button.detail.clone().unwrap_or_else(|| action.description().into());
    let key = crate::ui::shortcuts::row_of(action).and_then(|id| client::shell::keymap::hint(ctx, id));
    let label = match key {
        Some(key) => tf!("classic.ui.unit-actions.label-key", label = action.label(), key = key),
        None => action.label().to_owned(),
    };
    match &button.availability {
        Availability::Ready => format!("{label}\n{what}"),
        Availability::Blocked(reason) => tf!("classic.ui.unit-actions.hint-blocked", label = label.as_str(), what = what.as_str(), reason = reason.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use classic_rules::ids::TileId;
    fn kinds() -> UnitKinds {
        UnitKinds::from_rules(&crate::ui::screens::game_rules())
    }
    /// Own unit on tile 0, its kind looked up by content key ("warrior").
    fn unit(name: &str) -> UnitMarker {
        let kind = kinds().kind_of(&format!("core:unit/{name}")).expect(name);
        UnitMarker {
            id: 0, embarked: false, fortified: false, tile: TileId(0),
            owner: 1,
            kind,
            hp: 100,
        }
    }
    #[test]
    fn final_promotion_is_reachable_and_returns_its_id_after_scroll() {
        let ctx=egui::Context::default();
        let unit=UnitHandle {index:0,generation:0};
        let orders=UnitOrders {unit,tile:0,name:"Warrior".into(),status:String::new(),actions:vec![ActionState{action:Action::Promote,availability:Availability::Ready,detail:None}],targeting:None,promotions:(0..40).map(|i|crate::ui::model::PromotionChoice{id:PromotionId(i),key:"core:promotion/accuracy-3".into(),name:format!("Promotion {i}")}).collect()};
        let world=RenderWorld::default();
        let mut state=ActionPanelState::default();state.toggle_promotions(unit);
        let screen=Rect::from_min_size(egui::Pos2::ZERO,vec2(960.,640.));
        let mut picked=None;
        let mut frame=|events| {
            let mut output=ctx.run_ui(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ui| {if let Some(id)=promotion_picker(ui.ctx(),screen,&world,Some(0),&mut state,&orders){picked=Some(id);}});
            output.textures_delta.clear();output.shapes
        };
        frame(vec![]);frame(vec![]);
        frame(vec![egui::Event::PointerMoved(pos2(130.,220.))]);
        frame(vec![egui::Event::MouseWheel{unit:egui::MouseWheelUnit::Point,phase:egui::TouchPhase::Move,delta:vec2(0.,-9000.),modifiers:Default::default()}]);
        let mut shapes=Vec::new();for _ in 0..24 {shapes=frame(vec![]);}
        let point=shapes.iter().find_map(|shape|match &shape.shape {egui::Shape::Text(t) if t.galley.text()=="Promotion 39" && shape.clip_rect.contains(t.pos)=>Some(t.pos+vec2(5.,5.)),_=>None}).expect("last promotion is visible inside scroll clip");
        frame(vec![egui::Event::PointerMoved(point)]);
        for pressed in [true,false] {frame(vec![egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}]);}
        assert_eq!(picked,Some(PromotionId(39)));
        assert!(state.promotions_for.is_none());
    }

    #[test]
    fn all_generated_actions_have_alpha_and_bounded_cached_textures() {
        let mut decoder = action_art::Decoder::default();
        for action in ALL.into_iter().filter(|a| !WITHOUT_ART.contains(a)) {
            let image = decoder.decode(action).expect(action.id());
            assert_eq!(image.pixels[0].a(), 0, "{} needs transparent outside", action.id());
            assert!(image.pixels.iter().any(|pixel| pixel.a() >= 250), "{} must retain an opaque medallion", action.id());
        }
        assert_eq!(decoder.atlas_decodes, 3, "each new sheet is decoded only once");
        let ctx = egui::Context::default();
        let mut state = ActionPanelState::default();
        state.preload(&ctx);
        let ids: Vec<_> = ALL
            .iter()
            .filter(|a| !WITHOUT_ART.contains(a))
            .map(|action| {
                let t = state.textures[*action as usize]
                    .as_ref()
                    .expect("all painted action textures");
                assert!(t.size()[0] <= 192 && t.size()[1] <= 192);
                t.id()
            })
            .collect();
        state.load_art(&ctx);
        assert_eq!(
            ids,
            ALL.iter()
                .filter(|a| !WITHOUT_ART.contains(a))
                .map(|action| state.textures[*action as usize].as_ref().unwrap().id())
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn native_button_click_opens_an_action_preview_without_mutating_world() {
        let world = RenderWorld {
            width: 1,
            height: 1,
            observer: 1,
            visible: vec![1],
            units: vec![unit("warrior")],
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut state = ActionPanelState::default();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(1366., 768.));
        let button = pos2(130., 617.);
        let mut frame = |events| {
            let raw = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(raw, |ui| {
                let kinds = kinds();
                draw(ui.ctx(), screen, &world, Some(0), &mut state, PanelSource::Showcase(&kinds));
            });
            output.textures_delta.clear();
        };
        frame(vec![]);
        frame(vec![]);
        frame(vec![
            egui::Event::PointerMoved(button),
            egui::Event::PointerButton {
                pos: button,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        frame(vec![egui::Event::PointerButton {
            pos: button,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(state.preview, Some(Action::Attack));
        assert_eq!(world.units, vec![unit("warrior")]);
    }
    #[test]
    fn compact_bounds_follow_action_count_and_avoid_minimap_at_high_scaling() {
        let kinds = kinds();
        for (kind, width) in [("settler", 304.), ("worker", 352.), ("warrior", 416.)] {
            let world = RenderWorld {
                width: 1,
                height: 1,
                observer: 1,
                visible: vec![1],
                units: vec![unit(kind)],
                ..Default::default()
            };
            let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(1366., 768.));
            let r = panel_rect(screen, &world, Some(0), PanelSource::Showcase(&kinds)).unwrap();
            assert_eq!(r.width(), width);
            assert_eq!(r.height(), 132.);
            for scale in [1., 1.5, 2.] {
                let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(1366., 768.) / scale);
                let r = panel_rect(screen, &world, Some(0), PanelSource::Showcase(&kinds)).unwrap();
                let reserve = if screen.width() < 800. {
                    vec2(161., 150.)
                } else {
                    vec2(321., 273.)
                };
                let minimap = Rect::from_min_max(screen.max - reserve, screen.max);
                assert!(screen.contains_rect(r));
                assert!(!r.intersects(minimap), "kind {kind}, scale {scale}");
            }
        }
    }
    #[test]
    fn small_viewports_scroll_actions_without_covering_map_controls() {
        let kinds = kinds();
        for size in [vec2(512., 384.), vec2(640., 360.)] {
            for kind in ["settler", "warrior", "worker"] {
                let world = RenderWorld {
                    width: 1,
                    height: 1,
                    observer: 1,
                    visible: vec![1],
                    units: vec![unit(kind)],
                    ..Default::default()
                };
                let screen = Rect::from_min_size(egui::Pos2::ZERO, size);
                let panel = panel_rect(screen, &world, Some(0), PanelSource::Showcase(&kinds)).unwrap();
                assert!(screen.contains_rect(panel));
                assert!(panel.bottom() <= screen.bottom() - 82.);
            }
        }
    }
    #[test]
    fn action_context_follows_the_unit_role_not_its_kind_number() {
        let kinds = kinds();
        let actions = |name: &str| actions_for(kinds.get(unit(name).kind));
        for name in ["archer", "horseman", "swordsman", "warrior"] {
            assert_eq!(actions(name), MILITARY, "{name}");
        }
        assert!(actions("settler").contains(&Action::FoundCity));
        assert!(!actions("settler").contains(&Action::Attack));
        assert!(actions("worker").contains(&Action::Improve) && actions("worker").contains(&Action::Road));
        assert!(!actions("worker").contains(&Action::FoundCity));
        assert_eq!(actions_for(kinds.get(u16::MAX)), UNKNOWN);
    }
    #[test]
    fn hidden_enemy_and_invalid_units_cannot_open_the_panel() {
        let mut world = RenderWorld {
            width: 1,
            height: 1,
            observer: 1,
            visible: vec![1],
            explored: vec![1],
            units: vec![unit("warrior")],
            ..Default::default()
        };
        assert_eq!(selected_owned_unit(&world, Some(0)), Some(unit("warrior")));
        world.visible[0] = 0;
        assert!(selected_owned_unit(&world, Some(0)).is_none());
        world.visible[0] = 1;
        world.units[0].owner = 2;
        assert!(selected_owned_unit(&world, Some(0)).is_none());
        assert!(selected_owned_unit(&world, Some(4)).is_none());
    }
    #[test]
    fn clicking_only_sets_preview_and_context_switch_clears_it() {
        let mut state = ActionPanelState::default();
        state.select(Some(unit("warrior")), MILITARY);
        state.preview(Action::Attack);
        assert_eq!(state.preview, Some(Action::Attack));
        assert_eq!(state.unit, Some(unit("warrior")));
        state.select(Some(unit("worker")), WORKER);
        assert_eq!(state.preview, None);
        state.preview(Action::Attack);
        assert_eq!(state.preview, None);
        state.preview(Action::Road);
        assert_eq!(state.preview, Some(Action::Road));
        state.select(None, UNKNOWN);
        assert_eq!(state.preview, None);
    }
}
