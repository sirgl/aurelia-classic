//! The top bar's values are buttons: a click opens the screen the value belongs to (the Empire
//! menu's own screens), and culture and faith show how far the next policy and the pantheon are.
//! The clicks go through a stage like the app's; the numbers are compared with the core's costs.
use classic_rules::derived::culture::{culture_per_turn, next_policy_cost};
use classic_rules::derived::religion::{faith_per_turn, pantheon_cost, prophet_cost};
use classic_rules::ids::PlayerId;
use classic_rules::ClassicGame;
use classic_ui::game::Session;
use classic_ui::ui::model::{Action, GameRequest, Goal, TechStatus};
use classic_ui::ui::screens::{prepare_match, MapSize, MatchRequest, OverviewTab, Screen};
use classic_ui::ClassicUi;
use client::shell::module::{EndTurn, ModuleUi, Screen as ShellScreen, TopBarValue};
use client::shell::snapshot::Stage;

const SIZE: egui::Vec2 = egui::vec2(1366.0, 768.0);

/// The order of the classic top bar.
const GOLD: usize = 0;
const FOOD: usize = 1;
const PRODUCTION: usize = 2;
const SCIENCE: usize = 3;
const HAPPINESS: usize = 4;
const GOLDEN_AGE: usize = 5;
const CULTURE: usize = 6;
const FAITH: usize = 7;

fn request() -> MatchRequest {
    MatchRequest { nation: "core:nation/rome".into(), seed: 20260928, size: MapSize::Small, players: 3, water: 25 }
}

/// A match whose capital is founded, in which player 0 holds `culture` and `faith`.
fn stage_with(culture: u32, faith: u32) -> Stage<ClassicUi> {
    let mut prepared = prepare_match(request()).expect("the match is generated");
    if culture > 0 {
        prepared.game.debug_grant_culture(PlayerId(0), culture);
    }
    if faith > 0 {
        prepared.game.debug_grant_faith(PlayerId(0), faith);
    }
    let mut stage = Stage::new(ClassicUi::game(), SIZE);
    stage.module.start(prepared);
    stage.apply_module_actions();
    stage.module.request(GameRequest::Unit(Action::FoundCity));
    stage.frame(Vec::new());
    stage
}

fn session(stage: &mut Stage<ClassicUi>) -> &mut Session {
    stage.module.session.as_mut().expect("the match runs")
}

fn values(stage: &mut Stage<ClassicUi>) -> Vec<TopBarValue> {
    stage.frame(Vec::new());
    stage.hud.feed.as_ref().expect("a feed").top_bar.clone()
}

/// Click the centre of the value's cell on the map, then let the app open what was asked.
fn click_value(stage: &mut Stage<ClassicUi>, index: usize) {
    stage.open(ShellScreen::Map);
    stage.frame(Vec::new());
    let cell = stage.hud.value_cells[index];
    stage.click(cell.center());
    stage.frame(Vec::new());
}

#[test]
fn a_click_on_each_value_opens_its_screen() {
    let mut stage = stage_with(0, 0);
    let top = values(&mut stage);
    assert_eq!(top.len(), 8, "gold, food, production, science, happiness, golden age, culture, faith");
    let cases: [(usize, Option<Screen>, OverviewTab); 8] = [
        (GOLD, Some(Screen::Overview), OverviewTab::Cities),
        (FOOD, Some(Screen::Overview), OverviewTab::Cities),
        (PRODUCTION, Some(Screen::Overview), OverviewTab::Cities),
        (SCIENCE, Some(Screen::Research), OverviewTab::Cities),
        (HAPPINESS, None, OverviewTab::Cities),
        (GOLDEN_AGE, None, OverviewTab::Cities),
        (CULTURE, Some(Screen::Civics), OverviewTab::Cities),
        (FAITH, Some(Screen::Religion), OverviewTab::Cities),
    ];
    for (index, opens, tab) in cases {
        // A tab the player left on must not decide: the economy click selects its own.
        stage.module.screens.open_overview(OverviewTab::Notifications);
        stage.frame(Vec::new()); // the app opens that screen
        click_value(&mut stage, index);
        match opens {
            Some(screen) => {
                assert!(top[index].click.is_some(), "value {index} is a button");
                assert_eq!(stage.screens.screen, screen.into(), "value {index} opens {screen:?}");
                if screen == Screen::Overview {
                    assert_eq!(stage.module.screens.overview_tab(), tab, "value {index}");
                }
            }
            None => {
                assert!(top[index].click.is_none(), "value {index} stays a label");
                assert_ne!(stage.screens.screen, Screen::Overview.into(), "value {index}");
                assert_eq!(stage.screens.screen, ShellScreen::Map, "value {index} opens nothing");
            }
        }
    }
}

#[test]
fn a_button_value_says_what_a_click_opens() {
    let mut stage = stage_with(0, 0);
    let top = values(&mut stage);
    let hint = |i: usize| top[i].click.as_ref().map(|c| c.hint.clone()).unwrap_or_default();
    assert_eq!(hint(SCIENCE), "Click to open: Research");
    assert_eq!(hint(CULTURE), "Click to open: Policies");
    assert_eq!(hint(FAITH), "Click to open: Religion");
    assert_eq!(hint(GOLD), "Click to open: Economy");
}

#[test]
fn the_overview_resources_tab_opens_from_the_resource_value() {
    let mut stage = stage_with(0, 0);
    stage.module.screens.open_overview(OverviewTab::Cities);
    let flags = stage.screens.flags();
    stage.module.top_bar_clicked("resources", flags);
    stage.frame(Vec::new());
    assert_eq!(stage.screens.screen, Screen::Overview.into());
    assert_eq!(stage.module.screens.overview_tab(), OverviewTab::Resources);
}

#[test]
fn culture_shows_the_cores_price_and_what_is_missing() {
    let mut stage = stage_with(10, 0);
    let (cost, per_turn) = {
        let session = session(&mut stage);
        let (auth, rules, me) = (session.game().view().auth, session.rules(), session.me());
        (next_policy_cost(auth, rules, me), culture_per_turn(auth, rules, me))
    };
    assert!(cost > 10, "the first policy costs more than 10 culture");
    let saving = session(&mut stage).culture_saving().expect("the next policy is the goal");
    assert_eq!((saving.goal, saving.have, saving.cost, saving.ready), (Goal::Policy, 10, cost, false));
    assert_eq!(saving.missing(), cost - 10);
    let turns = saving.turns(per_turn);
    assert_eq!(turns, (per_turn > 0).then(|| (cost - 10).div_ceil(per_turn)));

    let top = values(&mut stage);
    assert_eq!(top[CULTURE].compact, format!("10/{cost}"), "the value is only the pool and the price");
    assert_eq!(top[CULTURE].text, format!("10/{cost} Culture"));
    assert!(!top[CULTURE].ready);
    let tip = top[CULTURE].tip.as_ref().expect("the breakdown tip").total.clone();
    assert!(tip.contains(&format!("{} more culture for the next policy", cost - 10)), "{tip}");
    match turns {
        Some(n) => assert!(tip.contains(&format!("{n} turn")) && tip.contains(&format!("+{per_turn} per turn")), "{tip}"),
        None => assert!(tip.contains('—'), "{tip}"),
    }
}

#[test]
fn a_policy_that_can_be_adopted_lights_the_value_like_the_end_turn_gate() {
    let mut stage = stage_with(200, 0);
    let research = session(&mut stage).research_view().techs.iter().find(|t| t.status == TechStatus::Available).expect("a technology").id;
    stage.module.request(GameRequest::Research(research));
    let cost = {
        let session = session(&mut stage);
        let (auth, rules, me) = (session.game().view().auth, session.rules(), session.me());
        next_policy_cost(auth, rules, me)
    };
    let saving = session(&mut stage).culture_saving().expect("the next policy");
    assert!(saving.ready && saving.missing() == 0);
    assert_eq!(saving.ready, session(&mut stage).policy_pending(), "the same test as the end-turn gate");
    let top = values(&mut stage);
    assert!(top[CULTURE].ready, "the number is lit");
    assert_eq!(top[CULTURE].compact, format!("200/{cost}"));
    assert!(top[CULTURE].tip.as_ref().unwrap().total.contains("A policy can be adopted now"));
    assert!(!top[FAITH].ready && !top[GOLD].ready);
    let end = stage.hud.feed.as_ref().and_then(|f| f.end_turn.clone());
    assert!(matches!(end, Some(EndTurn::Decide { .. })), "{end:?}: the button asks for the same policy");
}

#[test]
fn faith_counts_toward_the_pantheon_then_the_great_prophet() {
    let mut stage = stage_with(0, 4);
    let (pantheon, per_turn) = {
        let session = session(&mut stage);
        let (auth, rules, me) = (session.game().view().auth, session.rules(), session.me());
        (pantheon_cost(auth, rules), faith_per_turn(auth, rules, me))
    };
    assert!(pantheon > 4);
    let saving = session(&mut stage).faith_saving().expect("the pantheon is the goal");
    assert_eq!((saving.goal, saving.have, saving.cost, saving.ready), (Goal::Pantheon, 4, pantheon, false));
    let top = values(&mut stage);
    assert_eq!(top[FAITH].compact, format!("4/{pantheon}"));
    assert_eq!(top[FAITH].text, format!("4/{pantheon} Faith"));
    let tip = top[FAITH].tip.as_ref().expect("tip").total.clone();
    assert!(tip.contains(&format!("{} more faith for the pantheon", pantheon - 4)), "{tip}");
    if per_turn == 0 {
        assert!(tip.contains('—'), "no faith income shows a dash: {tip}");
    }

    // With enough faith the pantheon can be founded: the number is lit.
    let mut stage = stage_with(0, 500);
    let saving = session(&mut stage).faith_saving().expect("the pantheon");
    assert!(saving.ready);
    let top = values(&mut stage);
    assert!(top[FAITH].ready);
    assert!(top[FAITH].tip.as_ref().unwrap().total.contains("A pantheon can be founded now"));

    // Once the pantheon is founded the goal is the next Great Prophet.
    let belief = session(&mut stage).religion_view().pantheons[0].id;
    stage.module.request(GameRequest::FoundPantheon(belief));
    let prophet = {
        let session = session(&mut stage);
        prophet_cost(session.game().view().auth, session.rules(), session.me())
    };
    let saving = session(&mut stage).faith_saving().expect("the prophet");
    assert_eq!((saving.goal, saving.cost, saving.ready), (Goal::Prophet, prophet, false));
    let top = values(&mut stage);
    assert!(top[FAITH].compact.ends_with(&format!("/{prophet}")), "{}", top[FAITH].compact);
    assert!(!top[FAITH].ready);
}

#[test]
fn turns_are_rounded_up_and_without_income_there_is_no_number() {
    use classic_ui::ui::model::{turns_to, Saving};
    let saving = |have| Saving { goal: Goal::Policy, have, cost: 45, ready: false };
    assert_eq!(saving(24).missing(), 21);
    assert_eq!(saving(24).turns(3), Some(7));
    assert_eq!(saving(23).turns(3), Some(8), "22 missing at +3 is 8 turns");
    assert_eq!(saving(24).turns(0), None, "no income: a dash, no division by zero");
    assert_eq!(saving(45).turns(0), Some(0), "nothing missing needs no income");
    assert_eq!(saving(60).missing(), 0, "a pool above the price never goes negative");
    assert_eq!(turns_to(1, 1), Some(1));
}

#[test]
fn science_is_lit_while_the_research_gate_waits_for_a_technology() {
    let mut stage = stage_with(0, 0);
    let top = values(&mut stage);
    assert!(top[SCIENCE].ready, "nothing is researched: the same test as the end-turn gate's «Choose research»");
    assert!(top[SCIENCE].tip.as_ref().unwrap().total.contains("nothing is being researched"));
    let end = stage.hud.feed.as_ref().and_then(|f| f.end_turn.clone());
    assert!(matches!(end, Some(EndTurn::Decide { .. })), "{end:?}");
    let tech = session(&mut stage).research_view().techs.iter().find(|t| t.status == TechStatus::Available).expect("a technology").id;
    stage.module.request(GameRequest::Research(tech));
    let top = values(&mut stage);
    assert!(!top[SCIENCE].ready, "a technology is chosen: the number is plain again");
}
