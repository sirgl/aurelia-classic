//! The classic panels of the map HUD, drawn as the module's HUD layers in the shell's HUD
//! (`client::shell::hud`): the research box, the unit's orders and promotions, notices,
//! the rivals, the captured city's fate, the end of the game, and the tile tooltip. All of
//! them read `HudModel` and return typed requests.
use crate::ui::model::{GameRequest, HudModel};
use crate::ui::overview_model::HudExtras;
use crate::ui::unit_kinds::UnitKinds;
pub(crate) use client::shell::hud::{icon, panel, text, Icon, GOLD, INK, LIGHT, MUTED};
pub(crate) use client::shell::layout;
use client::shell::module::{HudLayer, TopBarValue};
use egui::Rect;
use render::world::state::RenderWorld;

#[path = "tile_tooltip.rs"]
pub(crate) mod tile_tooltip;
#[path = "unit_actions.rs"]
mod unit_actions;
#[path = "hud_game.rs"]
mod game_widgets;
pub use unit_actions::Action;
use unit_actions::PanelSource;

/// The classic HUD of the running match (`game: None` in the showcase, whose order panel
/// previews the selected unit's orders).
#[derive(Debug, Default)]
pub struct ClassicHud {
    pub game: Option<HudModel>,
    /// Banners, breakdowns and unit details of the running game.
    pub extras: HudExtras,
    /// Unit kinds of the running ruleset (names, roles) resolved by content key.
    pub unit_kinds: UnitKinds,
    action_panel: unit_actions::ActionPanelState,
}

/// What the panels asked for this frame.
#[derive(Debug, Default)]
pub struct PanelClicks {
    pub requests: Vec<GameRequest>,
    pub open_research: bool,
    /// The end-of-game panel asks for the results screen.
    pub open_results: bool,
    /// A notice was clicked: its tile.
    pub center_on: Option<u32>,
}

impl ClassicHud {
    fn panel_source(&self) -> PanelSource<'_> {
        match &self.game {
            Some(game) => PanelSource::Game(game.orders.as_ref()),
            None => PanelSource::Showcase(&self.unit_kinds),
        }
    }

    /// The screen area of the order panel (labels and yields hide under it).
    pub fn reserved(&self, screen: Rect, world: &RenderWorld, selected: Option<u32>) -> Option<Rect> {
        unit_actions::panel_rect(screen, world, selected, self.panel_source())
    }

    /// The panels laid out before the frame draws: the order panel.
    pub fn declared_panels(&self, screen: Rect, world: &RenderWorld, selected: Option<u32>) -> Vec<layout::Panel> {
        unit_actions::panel_spec(screen, world, selected, self.panel_source()).into_iter().collect()
    }

    /// Explicit headless warmup; live selection decodes action art off the UI thread.
    pub fn preload_action_art(&mut self, ctx: &egui::Context) {
        self.action_panel.preload(ctx);
    }

    /// Offscreen captures: open the promotion list of the selected unit.
    pub fn open_promotions(&mut self) {
        let unit = self.game.as_ref().and_then(|g| g.orders.as_ref()).map(|o| o.unit);
        if let Some(unit) = unit {
            self.action_panel.toggle_promotions(unit);
        }
    }

    /// The top bar's values: the economy with its breakdowns.
    pub fn top_bar(&self) -> Vec<TopBarValue> {
        let Some(game) = &self.game else {
            return Vec::new();
        };
        let tips = &self.extras.top_bar.yields;
        game_widgets::economy_values(&game.economy)
            .into_iter()
            .map(|value| TopBarValue {
                icon: value.icon,
                tint: value.tint,
                compact: value.compact,
                tip: value.tip.and_then(|i| tips.get(i)).cloned(),
                text: value.text,
            })
            .collect()
    }

    /// One HUD layer of the frame.
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        layer: HudLayer,
        screen: Rect,
        world: &RenderWorld,
        selected: Option<u32>,
    ) -> PanelClicks {
        let mut clicks = PanelClicks::default();
        match layer {
            HudLayer::Status => {
                if let Some(game) = self.game.as_ref().filter(|_| screen.width() >= 800.) {
                    clicks.open_research = game_widgets::research(ctx, game);
                }
            }
            HudLayer::Panels => self.panels(ctx, screen, world, selected, &mut clicks),
        }
        clicks
    }

    fn panels(&mut self, ctx: &egui::Context, screen: Rect, world: &RenderWorld, selected: Option<u32>, clicks: &mut PanelClicks) {
        let source = match &self.game {
            Some(game) => PanelSource::Game(game.orders.as_ref()),
            None => PanelSource::Showcase(&self.unit_kinds),
        };
        let order = unit_actions::draw(ctx, screen, world, selected, &mut self.action_panel, source);
        let orders = self.game.as_ref().and_then(|g| g.orders.clone());
        match (order, &orders) {
            (Some(Action::Promote), Some(orders)) => self.action_panel.toggle_promotions(orders.unit),
            (Some(action), _) => clicks.requests.push(GameRequest::Unit(action)),
            (None, _) => {}
        }
        if let Some(orders) = &orders {
            let picker = &mut self.action_panel;
            if let Some(id) = unit_actions::promotion_picker(ctx, screen, world, selected, picker, orders) {
                clicks.requests.push(GameRequest::Promote(id));
            }
        }
        if let Some(game) = &self.game {
            clicks.center_on = game_widgets::notices(ctx, screen, game);
            clicks.requests.extend(game_widgets::diplomacy(ctx, screen, game));
            clicks.requests.extend(game_widgets::capture(ctx, screen, game));
            clicks.requests.extend(game_widgets::ruins(ctx, screen, game));
            clicks.open_results = game_widgets::outcome(ctx, screen, game);
        }
    }

    /// With a unit selected over an enemy at war, the attack odds replace the terrain
    /// details of the hovered tile.
    pub fn draw_tile_hover(&self, ctx: &egui::Context, world: &RenderWorld, tile: Option<u32>) {
        let combat = self.game.as_ref().and_then(|g| g.combat.as_ref());
        match combat {
            Some(odds) if tile.is_some() => tile_tooltip::draw_odds(ctx, odds),
            _ => tile_tooltip::draw(ctx, world, tile, &self.extras.player_names),
        }
    }
}


