//! Route preview for the selected unit with the core pathfinder.
//!
//! The pathfinder reads the authoritative map, so a route is shown only when every tile of
//! it is explored by the player: the preview never draws a way through unknown land.
use classic_rules::ids::{TileId, UnitHandle};
use classic_rules::navigation::{find_path, find_path_cost, PathRequest};

use super::session::Session;
use crate::ui::model::{Action, PathPreview};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathCache {
    unit: UnitHandle,
    target: u32,
    revision: u64,
    preview: Option<PathPreview>,
}

impl Session {
    pub fn path_preview(&mut self, hover: Option<u32>) -> Option<PathPreview> {
        let target = hover?;
        if self.targeting == Some(Action::Attack) {
            return None;
        }
        let (unit, _) = self.selected_unit()?;
        let revision = self.view().revision();
        if let Some(cache) = &self.path_cache {
            if (cache.unit, cache.target, cache.revision) == (unit, target, revision) {
                return cache.preview.clone();
            }
        }
        let preview = self.compute_path(unit, TileId(target));
        self.path_cache = Some(PathCache {
            unit,
            target,
            revision,
            preview: preview.clone(),
        });
        preview
    }

    fn compute_path(&mut self, unit: UnitHandle, target: TileId) -> Option<PathPreview> {
        let core = *self.game().view().auth.units().get(unit)?.core;
        if core.tile == target || !self.explored(target) {
            return None;
        }
        let view = self.client.game.view();
        let request = PathRequest::for_unit(view.auth, &self.client.rules, unit, target)?;
        let path = find_path(view, &self.client.rules, request, &mut self.scratch)?;
        if !path.tiles.iter().all(|t| self.explored(*t)) {
            return None;
        }
        let this_turn = self.reachable_prefix(unit, &path.tiles);
        Some(PathPreview {
            tiles: path.tiles.iter().map(|t| t.0).collect(),
            this_turn,
            turns: path.cost.turns + 1,
        })
    }

    /// Leading tiles the unit reaches this turn (one cost query per tile,
    /// the core's turn-aware path cost).
    fn reachable_prefix(&mut self, unit: UnitHandle, tiles: &[TileId]) -> usize {
        let mut count = 1;
        for &tile in &tiles[1..] {
            let view = self.client.game.view();
            let Some(request) = PathRequest::for_unit(view.auth, &self.client.rules, unit, tile) else {
                break;
            };
            match find_path_cost(view, &self.client.rules, &request, &mut self.scratch) {
                Some(cost) if cost.this_turn() => count += 1,
                _ => break,
            }
        }
        count
    }

    fn explored(&self, tile: TileId) -> bool {
        self.view().explored.get(tile.0 as usize) == Some(&1)
    }
}
