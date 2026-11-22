use super::*;
use crate::ui::model::civics_sample::view;
use crate::ui::model::{BranchState, Lock};
use classic_rules::derived::culture::BranchBlock;
use classic_rules::ids::PolicyId;

static START: std::sync::LazyLock<web_time::Instant> = std::sync::LazyLock::new(web_time::Instant::now);

/// One frame of the screen at `size`.
fn frame(ctx: &egui::Context, state: &mut ScreenState, view: &CivicsView, size: egui::Vec2, events: Vec<egui::Event>) -> egui::FullOutput {
    // The input carries the real time, as the shell's frames do.
    let time = Some(START.elapsed().as_secs_f64());
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), events, time, ..Default::default() };
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
    let words: Vec<String> = output.shapes.iter().filter_map(|s| match &s.shape { egui::Shape::Text(t) => Some(t.galley.text().to_owned()), _ => None }).collect();
    assert!(words.join(" ").contains("+15% growth in every city"), "and its finisher (the line may wrap)");
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

#[test]
fn a_cut_line_ends_with_an_ellipsis_at_a_word_boundary() {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let p = ui.painter();
        let text = "+50% Production in the capital when producing a Settler; When adopted: 1 free Settler in the capital";
        let lines = wrap_lines(p, text, 11.5, 200.0, 1);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with(ELLIPSIS), "{lines:?}");
        assert!(text.starts_with(lines[0].trim_end_matches(ELLIPSIS)), "words stay whole: {lines:?}");
        assert!(text_width(p, &lines[0], 11.5) <= 200.0);
        let all = wrap_lines(p, "short text", 11.5, 200.0, 1);
        assert_eq!(all, ["short text"], "a text that fits is not cut");
        let two = wrap_lines(p, text, 11.5, 200.0, 2);
        assert_eq!(two.len(), 2);
        assert!(two[1].ends_with(ELLIPSIS));
    });
    output.textures_delta.clear();
}

/// A phone draws the screen in a layer shrunk by about 0.85, and the shell lays its texts out again
/// at the size they are drawn at (`client::shell::crisp`). The cut line and its ellipsis fit the
/// width the painter has there, and the screen lays them out in design units: laid out once at
/// `size * scale` after the shell's pass, not twice (a screen of its own scaling would end at
/// `size * scale * scale`).
#[test]
fn a_cut_line_with_its_ellipsis_fits_the_width_at_the_touch_scale() {
    const SCALE: f32 = 0.85;
    let ctx = egui::Context::default();
    let text = "+50% Production in the capital when producing a Settler; When adopted: 1 free Settler in the capital";
    let (size, width) = (10.5, 300.0);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let layer = ui.painter().layer_id();
        ui.ctx().set_transform_layer(layer, egui::emath::TSTransform::new(vec2(12.0, 58.0), SCALE));
        let lines = wrap_lines(ui.painter(), text, size, width, 1);
        assert!(lines[0].ends_with(ELLIPSIS), "{lines:?}");
        assert!(text_width(ui.painter(), &lines[0], size) <= width, "the cut line fits in the layer's points");
        let from = client::shell::crisp::mark(ui.ctx(), layer);
        clipped(ui.painter(), pos2(10.0, 10.0), text, size, Color32::WHITE, width, 1);
        client::shell::crisp::sharpen(ui.ctx(), layer, from, SCALE);
    });
    output.textures_delta.clear();
    let painted: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text().ends_with(ELLIPSIS) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(painted.len(), 1, "the cut line is painted once");
    let laid_out = painted[0].galley.job.sections[0].format.font_id.size;
    assert!((laid_out - size * SCALE).abs() < 1e-3, "laid out at {laid_out}, drawn at {}", size * SCALE);
}

fn pause_over() {
    std::thread::sleep(std::time::Duration::from_secs_f64(civics_focus::QUIET + 0.05));
}

/// The pause after a take runs on one clock, whoever took: the shell routes a key before it draws
/// and draws nothing while idle, so a clock written by the draw is stale at the next key.
#[test]
fn enter_after_the_pause_of_a_mouse_take_takes_with_no_frame_between() {
    let view = view();
    let size = SIZES[0];
    let ctx = egui::Context::default();
    let mut state = ScreenState::default();
    let output = settled(&ctx, &mut state, &view, size);
    click(&ctx, &mut state, &view, size, centres(&output, "Adopt · 45")[0]);
    assert_eq!(state.requests.len(), 1);
    pause_over();
    assert!(state.civics_key(&view, civics_focus::Move::Take).is_some(), "Enter after the pause of a mouse take is a take, not swallowed");
}

#[test]
fn a_held_enter_after_an_idle_spell_buys_once() {
    let view = view();
    let size = SIZES[0];
    let ctx = egui::Context::default();
    let mut state = ScreenState::default();
    settled(&ctx, &mut state, &view, size);
    pause_over();
    assert!(state.civics_key(&view, civics_focus::Move::Take).is_some());
    settled(&ctx, &mut state, &view, size);
    assert!(state.civics_key(&view, civics_focus::Move::Take).is_none(), "the redraw after the first take must not end the pause");
}

/// The sample with `n` more things to take, each with a long effect.
fn crowded(n: usize) -> CivicsView {
    let mut view = view();
    let template = view.offers[0].clone();
    for i in 0..n {
        let text = format!("Effect {i}: +25% worker speed; a free Worker; and every Worker you own works faster on roads and rails for the rest of the game, even far from home");
        view.offers.push(Offer { kind: OfferKind::Policy(PolicyId(100 + i as u16)), name: format!("Policy {i}"), text, ..template.clone() });
    }
    view
}

fn words(output: &egui::FullOutput) -> String {
    let rows: Vec<String> = output.shapes.iter().filter_map(|s| match &s.shape { egui::Shape::Text(t) => Some(t.galley.text().to_owned()), _ => None }).collect();
    rows.join(" ")
}

fn wheel(ctx: &egui::Context, state: &mut ScreenState, view: &CivicsView, size: egui::Vec2, at: egui::Pos2, down: f32) {
    frame(ctx, state, view, size, vec![egui::Event::PointerMoved(at)]);
    let event = egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, -down), modifiers: egui::Modifiers::NONE, phase: egui::TouchPhase::Move };
    frame(ctx, state, view, size, vec![event]);
    // egui eases a wheel notch over a few frames of real time.
    for _ in 0..12 {
        std::thread::sleep(std::time::Duration::from_millis(30));
        frame(ctx, state, view, size, vec![]);
    }
}

#[test]
fn the_available_list_scrolls_with_the_wheel() {
    let view = crowded(13);
    assert_eq!(view.offers.len(), 15);
    let ctx = egui::Context::default();
    let mut state = ScreenState::default();
    let size = SIZES[1];
    let before = settled(&ctx, &mut state, &view, size);
    assert!(!words(&before).contains("Policy 12"), "the last offer is below the list");
    let list = centres(&before, "Policy 0")[0];
    wheel(&ctx, &mut state, &view, size, list, 4000.0);
    let after = settled(&ctx, &mut state, &view, size);
    assert!(words(&after).contains("Policy 12"), "the wheel brings the last offer into view");
    assert!(!words(&after).contains("Policy 0 "), "and the first leaves it");
}

#[test]
fn the_keyboard_focus_keeps_its_card_in_view() {
    let view = crowded(13);
    let ctx = egui::Context::default();
    let mut state = ScreenState::default();
    let size = SIZES[1];
    settled(&ctx, &mut state, &view, size);
    for _ in 0..14 {
        state.civics_key(&view, civics_focus::Move::Next);
    }
    let output = settled(&ctx, &mut state, &view, size);
    let at = centres(&output, "Policy 12");
    assert_eq!(at.len(), 1, "the focused (last) card is drawn");
    assert!(at[0].y < size.y - 38.0, "and inside the screen, not below the list: {:?}", at[0]);
    for _ in 0..14 {
        state.civics_key(&view, civics_focus::Move::Previous);
    }
    let output = settled(&ctx, &mut state, &view, size);
    assert!(!centres(&output, "Citizenship").is_empty(), "back at the top the first card is in view");
}

#[test]
fn a_tap_on_a_card_shows_its_whole_text_and_adopts_nothing() {
    let mut view = view();
    let long = "+25% worker speed; a free Worker; and every Worker you own works faster on roads and rails for the rest of the game, even far from home";
    view.offers[0].text = long.into();
    let ctx = egui::Context::default();
    let mut state = ScreenState::default();
    let size = SIZES[0];
    let output = settled(&ctx, &mut state, &view, size);
    assert!(!words(&output).contains("even far from home"), "the card cuts the effect");
    let card = centres(&output, "Citizenship").into_iter().max_by(|a, b| a.x.total_cmp(&b.x)).expect("the card's name");
    click(&ctx, &mut state, &view, size, card);
    assert!(state.requests.is_empty(), "a tap on the card is not an adoption");
    let output = settled(&ctx, &mut state, &view, size);
    assert!(words(&output).contains("even far from home"), "the whole effect is shown after the tap");
    click(&ctx, &mut state, &view, size, card);
    assert!(!words(&settled(&ctx, &mut state, &view, size)).contains("even far from home"), "a second tap hides it");
}

#[test]
fn a_tap_on_a_node_shows_its_whole_text_with_the_reason_when_locked() {
    let view = view();
    let ctx = egui::Context::default();
    let mut state = ScreenState::default();
    let size = SIZES[0];
    state.civics.tap_policy(PolicyId(12));
    let output = settled(&ctx, &mut state, &view, size);
    let text = words(&output);
    assert!(text.contains("+1 happiness per city with a connection · Needs Citizenship first"), "{text}");
    state.civics.pick_branch(classic_rules::rules::content::PolicyBranchId(0));
    assert!(!words(&settled(&ctx, &mut state, &view, size)).contains("with a connection · Needs"), "another branch closes it");
}

#[test]
fn the_list_says_the_next_price_is_higher_when_the_core_says_so() {
    let mut view = view();
    let ctx = egui::Context::default();
    let mut state = ScreenState::default();
    let output = settled(&ctx, &mut state, &view, SIZES[0]);
    assert!(words(&output).contains("raises the next price: 45 now, 75 after it"));
    view.cost_after = view.next_cost;
    let output = settled(&ctx, &mut state, &view, SIZES[0]);
    assert!(!words(&output).contains("raises the next price"), "no note when the price stays");
}

/// The cut lines are measured once while the text stays on the screen and are the lines a fresh
/// measure gives; a text not drawn for a whole pass leaves the memo.
#[test]
fn the_lines_of_a_text_are_measured_once_while_it_is_shown() {
    let ctx = egui::Context::default();
    let text = "+50% Production in the capital when producing a Settler; When adopted: 1 free Settler in the capital";
    let memo = |ctx: &egui::Context| ctx.data_mut(|d| d.get_temp::<LinesMemo>(egui::Id::new("civics-lines-memo")));
    let mut pass = |draw: bool| {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            if draw {
                let p = ui.painter();
                assert_eq!(wrap_lines(p, text, 11.5, 200.0, 2), measure_lines(p, text, 11.5, 200.0, 2));
            }
        });
        output.textures_delta.clear();
    };
    pass(true);
    pass(true);
    let held = memo(&ctx).expect("the memo is kept");
    assert_eq!(held.current.len() + held.previous.len(), 1, "one text, one entry");
    pass(false);
    pass(false);
    pass(true);
    let held = memo(&ctx).expect("the memo is kept");
    assert!(held.previous.is_empty(), "the text left the memo while it was not drawn, then came back alone");
    assert_eq!(held.current.len(), 1);
}
