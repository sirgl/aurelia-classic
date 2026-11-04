//! What the map shows of the player's air orders: the tiles a sweep can reach while the
//! sweep mode waits for its target, and a mark on every own unit that waits to intercept.
//! The shell places the paint on the ground (`ModuleUi::map_paint`).
use std::collections::BTreeSet;

use classic_rules::math::tiles_in_hex_radius;
use classic_rules::world::unit::Stance;
use client::shell::hud::Icon;
use client::shell::module::{MapPaint, TileIcon, TileTint};
use egui::{Color32, Stroke};
use i18n::t;

use super::session::Session;
use crate::ui::model::Action;

/// The fill and the outline of the tiles a sweep reaches.
const SWEEP_FILL: Color32 = Color32::from_rgba_premultiplied(52, 30, 8, 60);
const SWEEP_EDGE: Color32 = Color32::from_rgb(236, 168, 72);
/// The mark of a unit that waits to intercept.
const WATCH: Color32 = Color32::from_rgb(126, 196, 236);

impl Session {
    /// The air orders' paint for this frame (empty when there is nothing to show).
    pub fn air_paint(&self) -> MapPaint {
        let mut paint = MapPaint {
            known_land: true,
            ..MapPaint::default()
        };
        self.paint_sweep_range(&mut paint);
        self.paint_interceptors(&mut paint);
        paint
    }

    /// Tiles within the sweeping unit's range that the player has explored (the core
    /// accepts any tile in range; the unexplored ones cannot be drawn).
    fn paint_sweep_range(&self, paint: &mut MapPaint) {
        if self.targeting != Some(Action::Sweep) {
            return;
        }
        let Some((unit, core)) = self.selected_unit() else {
            return;
        };
        let (tile, range) = (core.tile, self.sweep_range(unit));
        let dims = self.game().view().auth.map().dimensions;
        let explored = &self.view().explored;
        for target in tiles_in_hex_radius(&dims, tile, range) {
            if target == tile || explored.get(target.0 as usize) != Some(&1) {
                continue;
            }
            paint.tints.push(TileTint {
                tile: target.0,
                fill: SWEEP_FILL,
                edge: Some(Stroke::new(1.0, SWEEP_EDGE)),
            });
        }
    }

    /// A mark on the tile of each own unit in the interception stance.
    fn paint_interceptors(&self, paint: &mut MapPaint) {
        let auth = self.game().view().auth;
        let mut marked = BTreeSet::new();
        for &handle in auth.indexes().units_of(self.me()) {
            let Some(unit) = auth.units().get(handle) else {
                continue;
            };
            if unit.core.stance == Stance::Intercept && marked.insert(unit.core.tile.0) {
                paint.icons.push(TileIcon {
                    tile: unit.core.tile.0,
                    icon: Icon::Shield,
                    tint: WATCH,
                    label: Some(t!("classic.game.orders.mark-intercepting").into()),
                });
            }
        }
    }
}
