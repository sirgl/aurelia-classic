use super::*;
use crate::ui::model::civics_sample::view;
use crate::ui::model::{Cost, Lock, OfferKind, PolicyState};
use classic_rules::derived::culture::PolicyBlock;
use classic_rules::ids::PolicyId;
use classic_rules::rules::content::PolicyBranchId;

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
    assert_eq!(focus.apply(&view, Move::Next), None);
    assert_eq!(focus.offer, 1);
    focus.now = 10.0;
    assert_eq!(focus.apply(&view, Move::Take), Some(GameRequest::UnlockBranch(PolicyBranchId(0))));
    focus.now = 11.0;
    assert_eq!(focus.apply(&view, Move::Next), None);
    assert_eq!(focus.offer, 0, "the list is a ring");
}

#[test]
fn enter_in_the_branches_takes_the_focused_policy_or_unlocks_the_branch() {
    let view = view();
    let mut focus = Focus::default();
    focus.apply(&view, Move::SwitchZone);
    assert_eq!(focus.zone_in(&view), Zone::Branches);
    focus.now = 5.0;
    assert_eq!(focus.apply(&view, Move::Take), Some(GameRequest::AdoptPolicy(PolicyId(11))), "Liberty is shown, its adoptable policy is focused");
    focus.now = 6.0;
    focus.apply(&view, Move::Previous);
    assert_eq!(focus.branch, Some(PolicyBranchId(0)), "Up goes to Tradition");
    assert_eq!(focus.apply(&view, Move::Take), Some(GameRequest::UnlockBranch(PolicyBranchId(0))), "an unlockable branch is unlocked");
    focus.apply(&view, Move::Next);
    focus.apply(&view, Move::Next);
    assert_eq!(focus.branch, Some(PolicyBranchId(2)), "Honor");
    focus.now = 7.0;
    assert_eq!(focus.apply(&view, Move::Take), None, "a locked branch takes nothing");
}

#[test]
fn left_and_right_walk_the_policies_of_the_branch() {
    let view = view();
    let mut focus = Focus::default();
    focus.apply(&view, Move::SwitchZone);
    focus.apply(&view, Move::NextPolicy);
    assert_eq!(focus.policy, Some(PolicyId(12)), "after Citizenship comes Meritocracy");
    focus.now = 3.0;
    assert_eq!(focus.apply(&view, Move::Take), None, "a locked policy is not taken");
    focus.apply(&view, Move::PreviousPolicy);
    focus.apply(&view, Move::PreviousPolicy);
    assert_eq!(focus.policy, Some(PolicyId(10)));
    assert_eq!(view.branches[1].policies[2].state, PolicyState::Locked(Lock::Policy(PolicyBlock::MissingPrerequisite)));
}

#[test]
fn a_second_take_right_after_the_first_is_ignored() {
    let view = view();
    let mut focus = Focus::default();
    focus.now = 1.0;
    assert!(focus.apply(&view, Move::Take).is_some());
    focus.now = 1.1;
    assert_eq!(focus.apply(&view, Move::Take), None, "a held Enter or a double click buys once");
    focus.now = 1.0 + QUIET + 0.01;
    assert!(focus.apply(&view, Move::Take).is_some());
}

#[test]
fn the_shortcut_rows_map_to_moves() {
    for id in ["civics-previous", "civics-next", "civics-previous-policy", "civics-next-policy", "civics-switch", "civics-take"] {
        assert!(Move::of(id).is_some(), "{id}");
    }
    assert_eq!(Move::of("next-city"), None);
}
