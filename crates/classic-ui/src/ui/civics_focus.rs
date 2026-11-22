//! Where the Civics screen's focus is, and what the keys do with it. The mouse writes the same
//! fields: a click on a branch row, a node or an offer card moves the focus there.
//!
//! Two lists share the screen: the branches with their policies, and the offers ("Available now").
//! `Zone::Auto` follows the offers: when something may be taken, the focus starts there (the
//! end-turn button waits for exactly that choice); otherwise it starts in the branches.
use crate::ui::model::{BranchCard, BranchState, CivicsView, GameRequest, OfferKind, PolicyCard};
use classic_rules::ids::PolicyId;
use classic_rules::rules::content::PolicyBranchId;
use std::time::Duration;
use web_time::Instant;

/// A second adoption right after the first is ignored for this long: the next card slides under
/// the pointer, and a double click or a held Enter must not buy it too.
pub const QUIET: f64 = 0.35;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Zone {
    #[default]
    Auto,
    Branches,
    Offers,
}

/// What a key asks of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Previous,
    Next,
    PreviousPolicy,
    NextPolicy,
    SwitchZone,
    Take,
}

impl Move {
    /// The move a shortcut row of the screen gives.
    pub fn of(id: &str) -> Option<Move> {
        Some(match id {
            "civics-previous" => Move::Previous,
            "civics-next" => Move::Next,
            "civics-previous-policy" => Move::PreviousPolicy,
            "civics-next-policy" => Move::NextPolicy,
            "civics-switch" => Move::SwitchZone,
            "civics-take" => Move::Take,
            _ => return None,
        })
    }
}

#[derive(Debug, Default)]
pub struct Focus {
    /// The zone the player chose; `Auto` until then.
    pub zone: Zone,
    pub branch: Option<PolicyBranchId>,
    pub policy: Option<PolicyId>,
    pub offer: usize,
    /// Before this moment a take is ignored. The moment is read where the input arrives (the click
    /// in the draw, the key in the shell's key routing), on one clock: the shell routes keys before
    /// it draws and draws nothing while idle, so a clock written by the draw is stale at a key.
    pub quiet_until: Option<Instant>,
    /// How far the "Available now" list is scrolled (points), and the offer the scroll last
    /// followed: the keyboard focus moves the list only when it moves to another offer.
    pub scroll: f32,
    pub followed: Option<usize>,
    /// The policy or offer a tap opened to read in full.
    pub reading: Option<Reading>,
}

/// What the player tapped to read the whole text of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    Policy(PolicyId),
    Offer(OfferKind),
}

impl Focus {
    /// The zone in force: the player's, else the offers when there are any.
    pub fn zone_in(&self, view: &CivicsView) -> Zone {
        match self.zone {
            Zone::Offers if view.offers.is_empty() => Zone::Branches,
            Zone::Auto if view.offers.is_empty() => Zone::Branches,
            Zone::Auto => Zone::Offers,
            zone => zone,
        }
    }

    /// The branch looked at: the remembered one, else the one with something to adopt, else the
    /// first open one, else the first available one, else the first.
    pub fn branch_index(&self, view: &CivicsView) -> usize {
        if let Some(id) = self.branch {
            if let Some(i) = view.branches.iter().position(|b| b.id == id) {
                return i;
            }
        }
        let first = |test: &dyn Fn(&BranchCard) -> bool| view.branches.iter().position(test);
        first(&|b| b.adoptable().next().is_some())
            .or_else(|| first(&|b| b.state == BranchState::Open))
            .or_else(|| first(&|b| matches!(b.state, BranchState::Available(_))))
            .unwrap_or(0)
    }

    /// The policy looked at in `card`: the remembered one, else the first that may be adopted.
    pub fn policy_in<'a>(&self, card: &'a BranchCard) -> Option<&'a PolicyCard> {
        let every = || card.opener.iter().chain(card.policies.iter()).chain(card.finisher.iter());
        if let Some(id) = self.policy {
            if let Some(found) = every().find(|p| p.id == id) {
                return Some(found);
            }
        }
        card.adoptable().next().or(card.opener.as_ref()).or(card.policies.first())
    }

    /// Focus on a branch (a click, or the arrows).
    pub fn pick_branch(&mut self, id: PolicyBranchId) {
        self.zone = Zone::Branches;
        self.branch = Some(id);
        self.policy = None;
        self.reading = None;
    }

    pub fn pick_policy(&mut self, id: PolicyId) {
        self.zone = Zone::Branches;
        self.policy = Some(id);
    }

    pub fn pick_offer(&mut self, index: usize) {
        self.zone = Zone::Offers;
        self.offer = index;
    }

    /// A tap on a node: it gets the focus and its whole text is shown; a second tap hides it.
    pub fn tap_policy(&mut self, id: PolicyId) {
        self.pick_policy(id);
        self.toggle_reading(Reading::Policy(id));
    }

    /// A tap on an offer card.
    pub fn tap_offer(&mut self, index: usize, view: &CivicsView) {
        self.pick_offer(index);
        if let Some(offer) = view.offers.get(index) {
            self.toggle_reading(Reading::Offer(offer.kind));
        }
    }

    fn toggle_reading(&mut self, what: Reading) {
        self.reading = if self.reading == Some(what) { None } else { Some(what) };
    }

    /// Whether a take may go through at `now` (the quiet time after the last one is over).
    pub fn may_take(&self, now: Instant) -> bool {
        self.quiet_until.is_none_or(|until| now >= until)
    }

    /// A take went through at `now`: nothing is taken for a moment.
    pub fn taken(&mut self, now: Instant) {
        self.quiet_until = Some(now + Duration::from_secs_f64(QUIET));
        self.reading = None;
    }

    /// One key, pressed at `now`. The request to send when the key took something; the focus moves
    /// otherwise.
    pub fn apply(&mut self, view: &CivicsView, key: Move, now: Instant) -> Option<GameRequest> {
        let zone = self.zone_in(view);
        match key {
            Move::SwitchZone => {
                self.zone = if zone == Zone::Offers || view.offers.is_empty() { Zone::Branches } else { Zone::Offers };
                None
            }
            Move::Previous | Move::Next => {
                let step = if key == Move::Next { 1 } else { -1 };
                match zone {
                    Zone::Offers => self.offer = wrap(self.offer.min(view.offers.len() - 1), step, view.offers.len()),
                    _ => self.step_branch(view, step),
                }
                None
            }
            Move::PreviousPolicy | Move::NextPolicy => {
                if zone != Zone::Offers {
                    self.step_policy(view, if key == Move::NextPolicy { 1 } else { -1 });
                }
                None
            }
            Move::Take if !self.may_take(now) => None,
            Move::Take => {
                let request = self.take(view, zone);
                if request.is_some() {
                    self.taken(now);
                    self.move_on(view, zone);
                }
                request
            }
        }
    }

    fn step_branch(&mut self, view: &CivicsView, step: i32) {
        if view.branches.is_empty() {
            return;
        }
        let to = wrap(self.branch_index(view), step, view.branches.len());
        self.pick_branch(view.branches[to].id);
    }

    fn step_policy(&mut self, view: &CivicsView, step: i32) {
        let Some(card) = view.branches.get(self.branch_index(view)) else { return };
        if card.policies.is_empty() {
            return;
        }
        let at = self.policy_in(card).and_then(|p| card.policies.iter().position(|q| q.id == p.id));
        let to = match at {
            Some(i) => wrap(i, step, card.policies.len()),
            None => 0,
        };
        self.pick_policy(card.policies[to].id);
    }

    /// After a take the focus leaves what was taken, so the next Enter acts: in the branches it goes
    /// to the next policy that may be adopted, else to the list; the offers list keeps its place
    /// (the next card slides into it).
    fn move_on(&mut self, view: &CivicsView, zone: Zone) {
        if zone != Zone::Branches {
            return;
        }
        let Some(card) = view.branches.get(self.branch_index(view)) else { return };
        let Some(taken) = self.policy_in(card).filter(|p| p.price().is_some()).map(|p| p.id) else { return };
        let next = card.adoptable().find(|p| p.id != taken).map(|p| p.id);
        self.policy = next;
        if next.is_none() && view.offers.len() > 1 {
            self.zone = Zone::Offers;
            self.offer = 0;
        }
    }

    fn take(&self, view: &CivicsView, zone: Zone) -> Option<GameRequest> {
        if zone == Zone::Offers {
            let offer = view.offers.get(self.offer.min(view.offers.len().saturating_sub(1)))?;
            return Some(offer.kind.request());
        }
        let card = view.branches.get(self.branch_index(view))?;
        if let Some(policy) = self.policy_in(card).filter(|p| p.price().is_some()) {
            return Some(OfferKind::Policy(policy.id).request());
        }
        match card.state {
            BranchState::Available(_) if card.ideology => Some(OfferKind::Ideology(card.id).request()),
            BranchState::Available(_) => Some(OfferKind::Unlock(card.id).request()),
            _ => None,
        }
    }
}

/// `at` moved by `step` in a ring of `len` places.
fn wrap(at: usize, step: i32, len: usize) -> usize {
    let len = len as i32;
    ((at as i32 + step).rem_euclid(len)) as usize
}

#[cfg(test)]
#[path = "civics_focus_tests.rs"]
mod tests;
