use super::*;
use crate::ui::model::civics_sample::view;
use crate::ui::model::{Cost, Lock, OfferKind, PolicyState};
use classic_rules::derived::culture::PolicyBlock;
use classic_rules::ids::PolicyId;
use classic_rules::rules::content::PolicyBranchId;
use std::time::Duration;
use web_time::Instant;

/// The moments keys come at: each call of `later` is a second after the last, `soon` a tenth.
struct Clock(Instant);

impl Clock {
    fn new() -> Clock {
        Clock(Instant::now())
    }

    fn later(&mut self) -> Instant {
        self.0 += Duration::from_secs(1);
        self.0
    }

    fn soon(&mut self) -> Instant {
        self.0 += Duration::from_millis(100);
        self.0
    }
}

#[test]
fn offers_hold_everything_that_can_be_taken_across_branches() {
    let view = view();
    let kinds: Vec<OfferKind> = view.offers.iter().map(|o| o.kind).collect();
    assert_eq!(kinds, [OfferKind::Policy(PolicyId(11)), OfferKind::Unlock(PolicyBranchId(0))], "an open branch's policy, then a branch to unlock");
    let citizenship = &view.offers[0];
    assert_eq!(citizenship.name, "Citizenship");
    assert_eq!(citizenship.branch, "Liberty");
    assert_eq!(citizenship.text, "+25% worker speed; a free Worker", "the whole effect travels with the offer");
    assert_eq!(citizenship.cost, Cost::Culture(45));
    let tradition = &view.offers[1];
    assert!(tradition.text.starts_with("-25% culture needed"), "a branch offers its opener's effect");
    assert!(view.offers.iter().all(|o| o.name != "Republic" && o.name != "Meritocracy" && o.name != "Freedom"), "adopted, locked and not-yet-due things are not offered");
}

#[test]
fn an_offer_asks_for_the_request_of_its_kind() {
    let view = view();
    assert_eq!(view.offers[0].kind.request(), GameRequest::AdoptPolicy(PolicyId(11)));
    assert_eq!(view.offers[1].kind.request(), GameRequest::UnlockBranch(PolicyBranchId(0)));
    assert_eq!(OfferKind::Ideology(PolicyBranchId(3)).request(), GameRequest::ChooseIdeology(PolicyBranchId(3)));
}

#[test]
fn the_screen_opens_on_the_offers_when_there_are_any() {
    let mut view = view();
    let focus = Focus::default();
    assert_eq!(focus.zone_in(&view), Zone::Offers);
    assert_eq!(focus.branch_index(&view), 1, "the branch with something to adopt is shown");
    view.offers.clear();
    assert_eq!(focus.zone_in(&view), Zone::Branches, "nothing to take: the branches");
}

#[test]
fn enter_takes_the_focused_offer_and_the_arrows_move_the_focus() {
    let view = view();
    let mut focus = Focus::default();
    let mut clock = Clock::new();
    assert_eq!(focus.apply(&view, Move::Next, clock.later()), None);
    assert_eq!(focus.offer, 1);
    assert_eq!(focus.apply(&view, Move::Take, clock.later()), Some(GameRequest::UnlockBranch(PolicyBranchId(0))));
    assert_eq!(focus.apply(&view, Move::Next, clock.later()), None);
    assert_eq!(focus.offer, 0, "the list is a ring");
}

#[test]
fn enter_in_the_branches_takes_the_focused_policy_or_unlocks_the_branch() {
    let view = view();
    let mut focus = Focus::default();
    let mut clock = Clock::new();
    focus.apply(&view, Move::SwitchZone, clock.later());
    assert_eq!(focus.zone_in(&view), Zone::Branches);
    assert_eq!(focus.apply(&view, Move::Take, clock.later()), Some(GameRequest::AdoptPolicy(PolicyId(11))), "Liberty is shown, its adoptable policy is focused");
    assert_eq!(focus.zone, Zone::Offers, "nothing else to adopt in Liberty: the focus went to the list");
    focus.apply(&view, Move::SwitchZone, clock.later());
    focus.apply(&view, Move::Previous, clock.later());
    assert_eq!(focus.branch, Some(PolicyBranchId(0)), "Up goes to Tradition");
    assert_eq!(focus.apply(&view, Move::Take, clock.later()), Some(GameRequest::UnlockBranch(PolicyBranchId(0))), "an unlockable branch is unlocked");
    focus.apply(&view, Move::Next, clock.later());
    focus.apply(&view, Move::Next, clock.later());
    assert_eq!(focus.branch, Some(PolicyBranchId(2)), "Honor");
    assert_eq!(focus.apply(&view, Move::Take, clock.later()), None, "a locked branch takes nothing");
}

#[test]
fn left_and_right_walk_the_policies_of_the_branch() {
    let view = view();
    let mut focus = Focus::default();
    let mut clock = Clock::new();
    focus.apply(&view, Move::SwitchZone, clock.later());
    focus.apply(&view, Move::NextPolicy, clock.later());
    assert_eq!(focus.policy, Some(PolicyId(12)), "after Citizenship comes Meritocracy");
    assert_eq!(focus.apply(&view, Move::Take, clock.later()), None, "a locked policy is not taken");
    focus.apply(&view, Move::PreviousPolicy, clock.later());
    focus.apply(&view, Move::PreviousPolicy, clock.later());
    assert_eq!(focus.policy, Some(PolicyId(10)));
    assert_eq!(view.branches[1].policies[2].state, PolicyState::Locked(Lock::Policy(PolicyBlock::MissingPrerequisite)));
}

#[test]
fn a_second_take_right_after_the_first_is_ignored() {
    let view = view();
    let mut focus = Focus::default();
    let mut clock = Clock::new();
    let first = clock.later();
    assert!(focus.apply(&view, Move::Take, first).is_some());
    assert_eq!(focus.apply(&view, Move::Take, clock.soon()), None, "a held Enter or a double click buys once");
    assert!(focus.apply(&view, Move::Take, first + Duration::from_secs_f64(QUIET + 0.01)).is_some());
}

#[test]
fn after_enter_in_the_branches_the_focus_is_on_the_next_policy_to_adopt() {
    let mut view = view();
    let mut focus = Focus::default();
    let mut clock = Clock::new();
    focus.apply(&view, Move::SwitchZone, clock.later());
    // Liberty gets a second policy to adopt after Citizenship.
    view.branches[1].policies[2].state = PolicyState::Adoptable(Cost::Culture(45));
    assert!(focus.apply(&view, Move::Take, clock.later()).is_some());
    assert_eq!(focus.policy, Some(PolicyId(12)), "the next Enter takes Meritocracy, not Citizenship again");
    view.branches[1].policies[1].state = PolicyState::Adopted;
    assert!(focus.apply(&view, Move::Take, clock.later()).is_some());
    assert_eq!(focus.policy, None, "nothing else to adopt in the branch");
    assert_eq!(focus.zone, Zone::Offers, "the focus went to the list, which has more");
}

#[test]
fn the_shortcut_rows_map_to_moves() {
    for id in ["civics-previous", "civics-next", "civics-previous-policy", "civics-next-policy", "civics-switch", "civics-take"] {
        assert!(Move::of(id).is_some(), "{id}");
    }
    assert_eq!(Move::of("next-city"), None);
}
