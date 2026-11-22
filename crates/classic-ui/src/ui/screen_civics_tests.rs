use super::*;
use crate::ui::model::civics_sample::view;
use crate::ui::model::{BranchState, Lock};
use classic_rules::derived::culture::BranchBlock;
use classic_rules::ids::PolicyId;

/// One frame of the screen at `size`.
fn frame(ctx: &egui::Context, state: &mut ScreenState, view: &CivicsView, size: egui::Vec2, events: Vec<egui::Event>) -> egui::FullOutput {
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default() };
    let mut output = ctx.run_ui(raw, |ui| {
        let body = Rect::from_min_max(pos2(24.0, 80.0), pos2(size.x - 24.0, size.y - 38.0));
        civics(ui, body, state, view);
    });
    output.textures_delta.clear();
    output
}

fn click(ctx: &egui::Context, state: &mut ScreenState, view: &CivicsView, size: egui::Vec2, at: egui::Pos2) {
    let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    frame(ctx, state, view, size, vec![egui::Event::PointerMoved(at), press(true)]);
    frame(ctx, state, view, size, vec![press(false)]);
}

/// The centres of every text shape reading `label`.
fn centres(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(t) if t.galley.text() == label => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        })
        .collect()
}

fn settled(ctx: &egui::Context, state: &mut ScreenState, view: &CivicsView, size: egui::Vec2) -> egui::FullOutput {
    frame(ctx, state, view, size, vec![]);
    frame(ctx, state, view, size, vec![])
}

const SIZES: [egui::Vec2; 3] = [vec2(1600.0, 900.0), vec2(1280.0, 720.0), vec2(900.0, 520.0)];

#[test]
fn one_click_on_the_adopt_button_sends_one_request() {
    let view = view();
    for size in [SIZES[0], SIZES[1]] {
        let ctx = egui::Context::default();
        let mut state = ScreenState::default();
        let output = settled(&ctx, &mut state, &view, size);
        let buttons = centres(&output, "Adopt · 45");
        assert_eq!(buttons.len(), 2, "the tree's node and the Available now card show the price at {size:?}");
        click(&ctx, &mut state, &view, size, buttons[0]);
        assert_eq!(state.requests, vec![GameRequest::AdoptPolicy(PolicyId(11))], "{size:?}");
        click(&ctx, &mut state, &view, size, buttons[0]);
        assert_eq!(state.requests.len(), 1, "a second click at once is ignored");
    }
}

#[test]
fn the_card_in_available_now_adopts_in_one_click_too() {
    let view = view();
    let ctx = egui::Context::default();
    let mut state = ScreenState::default();
    let output = settled(&ctx, &mut state, &view, SIZES[0]);
    let unlock = centres(&output, "Unlock · 45");
    assert!(!unlock.is_empty(), "the branch to unlock is offered");
    click(&ctx, &mut state, &view, SIZES[0], *unlock.last().expect("a button"));
    assert_eq!(state.requests, vec![GameRequest::UnlockBranch(view.offers[1].branch_id())]);
}

#[test]
fn a_locked_policy_shows_its_reason_and_no_button() {
    let view = view();
    let ctx = egui::Context::default();
    let mut state = ScreenState::default();
    let output = settled(&ctx, &mut state, &view, SIZES[0]);
    assert_eq!(centres(&output, "Adopt · 45").len(), 2, "only the adoptable policy has a button");
    let reasons = output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains("Needs Citizenship first"))).count();
    assert_eq!(reasons, 1, "Meritocracy says what it waits for");
}

#[test]
fn every_effect_is_on_screen_without_a_click() {
    let view = view();
    let ctx = egui::Context::default();
    let mut state = ScreenState::default();
    let output = settled(&ctx, &mut state, &view, SIZES[0]);
    let shown = |needle: &str| output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains(needle)));
    assert!(shown("+25% worker speed"), "the effect of an adoptable policy");
    assert!(shown("+1 production in every city"), "the effect of an adopted policy");
    assert!(shown("-25% culture needed for border growth"), "a branch row says what its opener does");
    assert!(shown("+15% growth in every city"), "and its finisher");
}

#[test]
fn the_phone_canvas_shows_the_offers_first_and_the_branches_on_the_other_tab() {
    let view = view();
    let ctx = egui::Context::default();
    let mut state = ScreenState::default();
    let size = SIZES[2];
    let output = settled(&ctx, &mut state, &view, size);
    assert_eq!(centres(&output, "Adopt · 45").len(), 1, "the offers tab: one card, no tree");
    let tab = centres(&output, "Branches");
    assert_eq!(tab.len(), 1);
    click(&ctx, &mut state, &view, size, tab[0]);
    let output = settled(&ctx, &mut state, &view, size);
    assert_eq!(centres(&output, "Adopt · 45").len(), 1, "the branches tab: the tree's node");
    assert!(!centres(&output, "Liberty").is_empty());
}

#[test]
fn short_effect_keeps_the_first_clause() {
    assert_eq!(short_effect("-25% culture needed for border growth; +3 culture"), "-25% culture needed for border growth");
    assert_eq!(short_effect("+1 culture in every city"), "+1 culture in every city");
    assert_eq!(short_effect("Gives a bonus. And another one"), "Gives a bonus");
}

#[test]
fn a_branch_says_what_it_is_for() {
    let view = view();
    assert_eq!(purpose(&view.branches[1]), "+1 culture in every city · When adopted: a free great person of your choice");
}

#[test]
fn locks_say_why_with_the_numbers() {
    let view = view();
    let culture = lock_text(Lock::Policy(classic_rules::derived::culture::PolicyBlock::NotEnoughCulture), &view);
    assert!(culture.contains("45") && culture.contains("120"), "{culture}");
    let honor = &view.branches[2];
    assert_eq!(honor.state, BranchState::Locked(Lock::Branch(BranchBlock::EraTooEarly)));
    assert_eq!(branch_state_line(honor, &view), "FROM THE CLASSICAL ERA");
    let meritocracy = &view.branches[1].policies[2];
    assert_eq!(policy_reason(meritocracy, &view.branches[1], &view).as_deref(), Some("Needs Citizenship first"));
    let ideology = &view.branches[3];
    assert_eq!(branch_state_line(ideology, &view), Lock::Ideology(classic_rules::derived::culture::IdeologyBlock::TooEarly).reason());
}
