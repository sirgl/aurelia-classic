//! What the end of the human's turn waits for: a technology to research, a policy to adopt.
//! UI only: the rules accept the end-turn command anyway (the AI driver and the tests call
//! `Session::end_turn` directly); the module's end-turn button and key stop at the choice.
use classic_rules::derived::culture::{branch_statuses, culture_overview, policy_statuses};
use classic_rules::derived::grants::researchable;

use super::session::Session;
use crate::ui::model::Outcome;

/// A choice the player has not made yet, in the order the end-turn button asks for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Research,
    Policy,
}

impl Session {
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
        let auth = self.game().view().auth;
        auth.players().research_queue_of(self.me()).is_empty() && !researchable(auth, self.rules(), self.me()).is_empty()
    }

    /// A policy can be adopted now (a tenet, or a branch to open): the Civics screen's own
    /// statuses say which cards are ready.
    pub fn policy_pending(&self) -> bool {
        let (auth, rules, me) = (self.game().view().auth, self.rules(), self.me());
        policy_statuses(auth, rules, me).iter().any(Result::is_ok) || branch_statuses(auth, rules, me).iter().any(Result::is_ok)
    }

    /// How many policies, branches and ideologies the player holds: a request that raises it
    /// was a choice (the gate asks for one policy a turn).
    pub fn culture_choices(&self) -> usize {
        let overview = culture_overview(self.game().view().auth, self.rules(), self.me(), 0);
        overview.policies.len() + overview.branches.len() + usize::from(overview.ideology.is_some())
    }

    /// The choices the end of the turn waits for, research first. Nothing waits for an AI seat, a player
    /// without a city, while the others play, or after the game is over. `policy_chosen`: one was made this turn.
    pub fn missing_choices(&self, policy_chosen: bool) -> Vec<Choice> {
        let mut missing = Vec::new();
        if !self.human_seat() || !self.has_city() || self.outcome() != Outcome::Playing || self.ai_busy() {
            return missing;
        }
        if self.research_pending() {
            missing.push(Choice::Research);
        }
        if !policy_chosen && self.policy_pending() {
            missing.push(Choice::Policy);
        }
        missing
    }
}
