//! What the end of the human's turn waits for. The list is the core's (`derived::pending`); this file
//! only asks it for the local player and says where each item is answered.
//! UI only: the rules accept the end-turn command anyway (the AI driver and the tests call
//! `Session::end_turn` directly); the module's end-turn button and key stop at the first item.
use classic_rules::derived::culture::culture_overview;
use classic_rules::derived::pending::{self, HandledThisTurn};
use classic_rules::ids::TileId;
pub use classic_rules::derived::pending::PendingAction;

use super::session::Session;
use crate::ui::model::Outcome;

/// A screen whose decisions the end of the turn only asks to look at (`HandledThisTurn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Looked {
    Congress,
    Offers,
    Spies,
}

impl Session {
    /// The player opened the screen of `what` this turn.
    pub fn looked_at(&mut self, what: Looked) {
        self.looked[what as usize] = Some(self.turn());
    }

    fn looked_this_turn(&self, what: Looked) -> bool {
        self.looked[what as usize] == Some(self.turn())
    }

    /// The local player is a human seat (not the one the AI plays for: the autoplay).
    pub fn human_seat(&self) -> bool {
        self.me().0 < self.driver.config.human_players
    }

    /// The player owns a city (before the first one is founded, and after the last is lost, nothing
    /// is asked: there is no science or culture to direct).
    pub fn has_city(&self) -> bool {
        !self.game().view().auth.indexes().cities_of(self.me()).is_empty()
    }

    /// No technology is being researched although one can be.
    pub fn research_pending(&self) -> bool {
        pending::research_pending(self.game().view().auth, self.rules(), self.me())
    }

    /// A policy can be adopted now (a tenet, or a branch to open).
    pub fn policy_pending(&self) -> bool {
        pending::policy_pending(self.game().view().auth, self.rules(), self.me())
    }

    /// How many policies, branches and ideologies the player holds: a request that raises it
    /// was a choice (the gate asks for one policy a turn).
    pub fn culture_choices(&self) -> usize {
        let overview = culture_overview(self.game().view().auth, self.rules(), self.me(), 0);
        overview.policies.len() + overview.branches.len() + usize::from(overview.ideology.is_some())
    }

    /// The units the player put to rest this turn (skip, sleep).
    pub(super) fn resting_units(&self) -> Vec<classic_rules::ids::UnitHandle> {
        self.skipped.iter().chain(self.sleeping.iter()).copied().collect()
    }

    /// The actions the end of the turn waits for, the first one first. Nothing waits for an AI seat, while the others play, or after the game is over. `policy_chosen`: one
    /// was made this turn.
    pub fn pending_actions(&self, policy_chosen: bool) -> Vec<PendingAction> {
        if !self.human_seat() || self.outcome() != Outcome::Playing || self.ai_busy() {
            return Vec::new();
        }
        let resting = self.resting_units();
        pending::pending_actions(self.game().view().auth, self.rules(), self.me(), HandledThisTurn {
            policy_chosen,
            resting: &resting,
            congress_seen: self.looked_this_turn(Looked::Congress),
            offers_seen: self.looked_this_turn(Looked::Offers),
            spies_seen: self.looked_this_turn(Looked::Spies),
        })
    }

    /// The map tile of a city or a unit item (None: the item has no place on the map).
    pub fn pending_tile(&self, action: PendingAction) -> Option<TileId> {
        let auth = self.game().view().auth;
        match action {
            PendingAction::CityDecision(city) | PendingAction::EmptyProduction(city) => auth.cities().get(city).map(|c| c.tile),
            PendingAction::Promotion(unit) | PendingAction::GreatPerson(unit) | PendingAction::UnitOrder(unit) => {
                auth.units().get(unit).map(|u| u.core.tile)
            }
            PendingAction::FreeGreatPerson
            | PendingAction::FreeTechnology
            | PendingAction::Research
            | PendingAction::Policy
            | PendingAction::Pantheon
            | PendingAction::CongressVotes
            | PendingAction::OfferWaiting(_)
            | PendingAction::IdleSpy(_) => None,
        }
    }
}
