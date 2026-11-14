//! The end of the human's turn waits for a research and a policy to be chosen when they can be
//! (UI only: the rules, `Session::end_turn` and the AI driver are not gated). The keys go through
//! a stage like the app's: Enter reaches the module's `begin_end_turn`, which opens the screen.
use classic_rules::ids::{PlayerId, ProductionItem, UnitKindId};
use classic_rules::ClassicGame;
use classic_ui::game::{PendingAction, Session};
use classic_ui::ui::model::{Action, BranchState, GameRequest, TechStatus};
use classic_ui::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest, Screen};
use classic_ui::ClassicUi;
use client::shell::module::{EndTurn, ModuleUi, Screen as ShellScreen, ShellAction};
use client::shell::shortcuts::Routed;
use client::shell::snapshot::Stage;

const SIZE: egui::Vec2 = egui::vec2(1366.0, 768.0);

fn request() -> MatchRequest {
    MatchRequest { nation: "core:nation/rome".into(), seed: 20260928, size: MapSize::Small, players: 3, water: 25 }
}

/// A match whose capital is founded (nothing is researched) in which player 0 holds `culture`.
fn stage_with(ui: ClassicUi, culture: u32) -> Stage<ClassicUi> {
    stage_where(ui, culture, false)
}

/// The same; `all_known`: every technology is known too.
fn stage_where(ui: ClassicUi, culture: u32, all_known: bool) -> Stage<ClassicUi> {
    let mut stage = stage_at_start(ui, culture, all_known);
    stage.module.request(GameRequest::Unit(Action::FoundCity));
    assert_eq!(cities(&mut stage), 1, "the capital is founded");
    settle_production_and_orders(&mut stage);
    stage
}

/// The first city builds a warrior and every unit is told to skip: only research and policy are left to ask.
fn settle_production_and_orders(stage: &mut Stage<ClassicUi>) {
    give_production(stage);
    skip_idle_units(stage);
    let session = session(stage);
    assert!(session.pending_actions(true).iter().all(|a| matches!(a, PendingAction::Research)), "{:?}", session.pending_actions(true));
}

/// Every city without production builds a warrior.
fn give_production(stage: &mut Stage<ClassicUi>) {
    let session = session(stage);
    let warrior = session.rules().units.iter().position(|u| u.key == "core:unit/warrior").expect("a warrior") as u16;
    for action in session.pending_actions(true) {
        if let PendingAction::EmptyProduction(_) = action {
            let tile = session.pending_tile(action).expect("the city stands on a tile");
            session.request(GameRequest::SelectCity(tile.0));
            session.request(GameRequest::Enqueue(ProductionItem::Unit(UnitKindId(warrior))));
        }
    }
}

/// Every unit without an order skips the turn.
fn skip_idle_units(stage: &mut Stage<ClassicUi>) {
    let session = session(stage);
    for action in session.pending_actions(true) {
        if let PendingAction::UnitOrder(unit) | PendingAction::GreatPerson(unit) = action {
            session.select_unit(unit);
            session.request(GameRequest::Unit(Action::Skip));
        }
    }
}

/// The first turn: the settler has not founded a city yet.
fn stage_at_start(ui: ClassicUi, culture: u32, all_known: bool) -> Stage<ClassicUi> {
    let mut prepared = prepare_match(request()).expect("the match is generated");
    if culture > 0 {
        prepared.game.debug_grant_culture(PlayerId(0), culture);
    }
    if all_known {
        let rules = game_rules();
        for i in 0..rules.technologies.len() {
            prepared.game.debug_learn(&rules, PlayerId(0), classic_rules::ids::TechId(i as u16));
        }
    }
    let mut stage = Stage::new(ui, SIZE);
    stage.module.start(prepared);
    stage.apply_module_actions();
    stage
}

fn session(stage: &mut Stage<ClassicUi>) -> &mut Session {
    stage.module.session.as_mut().expect("the match runs")
}

/// The end-turn button as the HUD draws it after a frame.
fn button(stage: &mut Stage<ClassicUi>) -> EndTurn {
    stage.frame(Vec::new());
    stage.hud.feed.as_ref().and_then(|feed| feed.end_turn.clone()).expect("the button")
}

fn turn(stage: &Stage<ClassicUi>) -> u32 {
    stage.module.turn()
}

fn choose_research(stage: &mut Stage<ClassicUi>) {
    let tech = session(stage).research_view().techs.iter().find(|t| t.status == TechStatus::Available).expect("a technology to research").id;
    stage.module.request(GameRequest::Research(tech));
}

#[test]
fn enter_opens_the_research_screen_until_a_technology_is_chosen() {
    let mut stage = stage_with(ClassicUi::game(), 0);
    let start = turn(&stage);
    match button(&mut stage) {
        EndTurn::Decide { label, tip } => {
            assert_eq!(label, "Choose research");
            assert!(tip.contains("research"), "{tip}");
        }
        other => panic!("the button must ask for the research, it is {other:?}"),
    }
    stage.open(ShellScreen::Map);
    assert_eq!(stage.press("Enter"), Routed::Shell(ShellAction::EndTurn));
    assert_eq!(turn(&stage), start, "the turn did not end");
    stage.frame(Vec::new()); // the app opens the screen the module asked for at the start of its next frame
    assert_eq!(stage.screens.screen, Screen::Research.into(), "the research screen opened");

    stage.open(ShellScreen::Map);
    assert_eq!(stage.press("Ctrl+Space"), Routed::Shell(ShellAction::EndTurn));
    assert_eq!(turn(&stage), start, "the second key does not end it either");

    choose_research(&mut stage);
    assert_eq!(button(&mut stage), EndTurn::Ready, "the button is normal once a technology is chosen");
    stage.open(ShellScreen::Map);
    assert_eq!(stage.press("Enter"), Routed::Shell(ShellAction::EndTurn));
    assert_eq!(turn(&stage), start + 1, "the turn ended");
}

#[test]
fn the_button_press_reaches_the_same_gate() {
    let mut stage = stage_with(ClassicUi::game(), 0);
    let start = turn(&stage);
    // The HUD button's press and the key are one path: the shell calls `begin_end_turn`.
    stage.module.begin_end_turn();
    assert_eq!(turn(&stage), start);
    assert!(stage.module.take_actions().contains(&ShellAction::Open(Screen::Research.into())));
    choose_research(&mut stage);
    stage.module.begin_end_turn();
    stage.module.finish_ai_turns();
    assert_eq!(turn(&stage), start + 1);
}

#[test]
fn a_policy_that_can_be_adopted_gates_the_turn_once() {
    let mut stage = stage_with(ClassicUi::game(), 100);
    choose_research(&mut stage);
    let start = turn(&stage);
    match button(&mut stage) {
        EndTurn::Decide { label, .. } => assert_eq!(label, "Choose a policy"),
        other => panic!("the button must ask for a policy, it is {other:?}"),
    }
    stage.open(ShellScreen::Map);
    assert_eq!(stage.press("Enter"), Routed::Shell(ShellAction::EndTurn));
    assert_eq!(turn(&stage), start, "the turn did not end");
    stage.frame(Vec::new());
    assert_eq!(stage.screens.screen, Screen::Civics.into(), "the policies screen opened");

    let branch = session(&mut stage).civics_view().branches.iter().find(|b| b.state == BranchState::Available).expect("a branch to unlock").id;
    stage.module.request(GameRequest::UnlockBranch(branch));
    assert!(!session(&mut stage).civics_view().branches.iter().all(|b| b.state != BranchState::Open), "the branch is open");
    assert!(session(&mut stage).policy_pending(), "culture is left for more policies");
    assert_eq!(button(&mut stage), EndTurn::Ready, "one policy a turn is asked for");
    stage.open(ShellScreen::Map);
    stage.press("Enter");
    assert_eq!(turn(&stage), start + 1);

    // The next turn asks again: culture is still there.
    match button(&mut stage) {
        EndTurn::Decide { label, .. } => assert_eq!(label, "Choose a policy"),
        other => panic!("the next turn must ask for a policy again, it is {other:?}"),
    }
}

#[test]
fn both_missing_research_comes_first_then_the_policy() {
    let mut stage = stage_with(ClassicUi::game(), 100);
    match button(&mut stage) {
        EndTurn::Decide { label, tip } => {
            assert_eq!(label, "Choose research");
            assert!(tip.contains("technology"), "the tip names the first item: {tip}");
        }
        other => panic!("{other:?}"),
    }
    stage.open(ShellScreen::Map);
    stage.press("Enter");
    stage.frame(Vec::new());
    assert_eq!(stage.screens.screen, Screen::Research.into(), "research first");
    choose_research(&mut stage);
    match button(&mut stage) {
        EndTurn::Decide { label, .. } => assert_eq!(label, "Choose a policy"),
        other => panic!("{other:?}"),
    }
    stage.open(ShellScreen::Map);
    stage.press("Enter");
    stage.frame(Vec::new());
    assert_eq!(stage.screens.screen, Screen::Civics.into(), "then the policy");
}

#[test]
fn nothing_is_asked_when_there_is_nothing_to_choose() {
    let mut stage = stage_where(ClassicUi::game(), 0, true);
    assert!(!session(&mut stage).research_pending(), "everything is researched");
    assert!(!session(&mut stage).policy_pending(), "no culture");
    assert_eq!(button(&mut stage), EndTurn::Ready);
}

#[test]
fn the_ai_playing_for_the_player_is_not_asked_and_the_rules_are_not_gated() {
    let mut stage = stage_with(ClassicUi::game(), 100);
    session(&mut stage).autoplay();
    assert!(!session(&mut stage).human_seat());
    assert!(session(&mut stage).pending_actions(false).is_empty());
    assert!(!matches!(button(&mut stage), EndTurn::Decide { .. }), "the AI plays: the button does not ask");

    // The gate is the module's: the session of a human with nothing chosen ends the turn.
    let prepared = prepare_match(request()).expect("the match is generated");
    let mut session = Session::new(prepared.game, game_rules());
    assert!(session.human_seat());
    session.request(GameRequest::Unit(Action::FoundCity));
    assert!(session.pending_actions(false).contains(&PendingAction::Research));
    let start = session.turn();
    session.end_turn();
    assert_eq!(session.turn(), start + 1, "Session::end_turn is not gated");
}

#[test]
fn scripts_can_leave_the_gate_off() {
    let mut stage = stage_with(ClassicUi::game().without_end_turn_gate(), 100);
    let start = turn(&stage);
    assert_eq!(button(&mut stage), EndTurn::Ready);
    stage.open(ShellScreen::Map);
    stage.press("Enter");
    assert_eq!(turn(&stage), start + 1);
}

#[test]
fn before_the_first_city_only_the_settler_is_asked_for() {
    let mut stage = stage_at_start(ClassicUi::game(), 100, false);
    let start = turn(&stage);
    assert_eq!(cities(&mut stage), 0);
    let waiting = session(&mut stage).pending_actions(false);
    assert!(matches!(waiting.as_slice(), [PendingAction::UnitOrder(_), ..]), "no city: no research or policy, but the units: {waiting:?}");
    match button(&mut stage) {
        EndTurn::Decide { label, .. } => assert_eq!(label, "Unit needs an order"),
        other => panic!("{other:?}"),
    }
    stage.open(ShellScreen::Map);
    assert_eq!(stage.press("Enter"), Routed::Shell(ShellAction::EndTurn));
    assert_eq!(turn(&stage), start, "the turn did not end");

    // The capital is founded: its production comes first, then the research.
    let mut stage = stage_at_start(ClassicUi::game(), 0, false);
    stage.module.request(GameRequest::Unit(Action::FoundCity));
    assert_eq!(cities(&mut stage), 1);
    match button(&mut stage) {
        EndTurn::Decide { label, .. } => assert_eq!(label, "Choose production"),
        other => panic!("after the founding the button must ask, it is {other:?}"),
    }
}

#[test]
fn a_city_without_production_stops_the_turn_and_the_button_leads_to_it() {
    let mut stage = stage_at_start(ClassicUi::game(), 0, false);
    stage.module.request(GameRequest::Unit(Action::FoundCity));
    skip_idle_units(&mut stage);
    let start = turn(&stage);
    stage.open(ShellScreen::Map);
    assert_eq!(stage.press("Enter"), Routed::Shell(ShellAction::EndTurn));
    assert_eq!(turn(&stage), start, "the turn did not end");
    stage.frame(Vec::new());
    assert_eq!(stage.screens.screen, Screen::City.into(), "the city screen opened");
    give_production(&mut stage);
    match button(&mut stage) {
        EndTurn::Decide { label, .. } => assert_eq!(label, "Choose research", "research is next"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_unit_without_an_order_stops_the_turn_until_it_gets_one() {
    let mut stage = stage_at_start(ClassicUi::game(), 0, false); // Rome: a settler and a warrior
    stage.module.request(GameRequest::Unit(Action::FoundCity));
    give_production(&mut stage);
    choose_research(&mut stage);
    let start = turn(&stage);
    match button(&mut stage) {
        EndTurn::Decide { label, tip } => {
            assert_eq!(label, "Unit needs an order");
            assert!(tip.contains("order"), "{tip}");
        }
        other => panic!("the warrior has no order, the button is {other:?}"),
    }
    stage.open(ShellScreen::Map);
    assert_eq!(stage.press("Enter"), Routed::Shell(ShellAction::EndTurn));
    assert_eq!(turn(&stage), start, "the turn did not end");
    let waiting = session(&mut stage).pending_actions(false);
    let [PendingAction::UnitOrder(unit)] = waiting.as_slice() else { panic!("one unit waits: {waiting:?}") };
    assert_eq!(session(&mut stage).selected_unit_id(), Some(unit_id(&mut stage, *unit)), "the button selected the unit");

    stage.module.request(GameRequest::Unit(Action::Fortify));
    assert_eq!(button(&mut stage), EndTurn::Ready, "the order was given");
    stage.open(ShellScreen::Map);
    stage.press("Enter");
    assert_eq!(turn(&stage), start + 1);
}

fn unit_id(stage: &mut Stage<ClassicUi>, unit: classic_rules::ids::UnitHandle) -> u64 {
    session(stage).game().view().auth.units().get(unit).expect("the unit").core.id.0
}
