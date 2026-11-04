//! Clicks on a tile that holds own units: an order or a selection.
//!
//! The rules allow one land military and one land civilian unit of a player on a tile
//! (`world::stacking`, classic/docs/classic/rules.md). So with a unit selected, a left click
//! on a tile whose own units are all of the other class (a military unit on its civilian,
//! a settler on its escort) is a move order, not a selection. The unit that cannot go there
//! (same class, no moves, no way) is selected instead, as it always was, and a right click on
//! own units selects them when there is nothing to attack. Repeated clicks cycle through a
//! stack, the military unit first.
use classic_rules::command::{CommandError, Refusal};
use classic_rules::ids::{TileId, UnitHandle};
use classic_rules::rules::content::Domain;
use classic_rules::Command;

use super::session::Session;

impl Session {
    /// A left click on `tile` is a move order for the selected unit: own units stand there and
    /// the rules let the selected one stop beside them, and it can walk there (this turn, or
    /// as a march over several).
    pub(super) fn click_moves_onto_own_units(&mut self, tile: TileId) -> bool {
        let Some((unit, core)) = self.selected_unit() else {
            return false;
        };
        let (kind, from) = (core.kind, core.tile);
        if from == tile || core.moves_left == 0 || self.own_city_at(tile).is_some() {
            return false;
        }
        // An aircraft lands on cities and carriers by the Move mode; a click only selects.
        if self.rules().units.get(kind.0 as usize).is_none_or(|d| d.domain == Domain::Air) {
            return false;
        }
        let me = self.me();
        let auth = self.game().view().auth;
        let here = auth.indexes().units_on(tile);
        let all_own = !here.is_empty() && here.iter().all(|h| auth.units().get(*h).is_some_and(|u| u.core.owner == me));
        if !all_own || classic_rules::world::stacking::can_stop_at(auth, self.rules(), me, kind, Some(unit), tile).is_err() {
            return false;
        }
        self.can_walk_to(unit, tile)
    }

    fn can_walk_to(&mut self, unit: UnitHandle, tile: TileId) -> bool {
        match self.check(&Command::MoveUnit { unit, destination: tile }) {
            Ok(()) => true,
            Err(CommandError::Refused(Refusal::NoMovesLeft)) => self.check(&Command::MarchTo { unit, destination: tile }).is_ok(),
            Err(_) => false,
        }
    }

    /// A right click on `tile` selects the own unit there (the next one of a stack) when it is
    /// no attack, a nuclear launch or a paradrop: the way to pick a unit that a left click
    /// would send the selected unit to.
    pub(super) fn secondary_click_selects(&mut self, tile: TileId) -> bool {
        if let Some((unit, _)) = self.selected_unit() {
            if self.special_strike(unit, tile).is_some() || self.attack_target(unit, tile).is_some() {
                return false;
            }
        }
        let Some(unit) = self.next_own_unit_at(tile) else {
            return false;
        };
        self.presentation.play_sound(client::shell::presentation::motion::Sound::Select);
        self.select_unit(unit);
        true
    }
}
