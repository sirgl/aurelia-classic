//! Defence against humans (`aurelia_qa::stress::humans`, classic/docs/ai-stress.md): every trick
//! builds or says why, the scripts play their seat through the rules, a held unit is not moved by its
//! seat's AI, the script is data that survives a saved scene, and the gates in the code are the gates
//! in the document. Positions are a tiny game of the smoke suite.

use std::path::PathBuf;
use std::sync::OnceLock;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::export::{play_log_until, suite_specs_with, write_log};
use aurelia_qa::stress::humans::{self, Cast, Order, Trick};
use aurelia_qa::stress::{self, play, position, scene, seat, Situation};
use classic_rules::ai::variant::Variant;
use classic_rules::ids::PlayerId;
use classic_rules::CompiledRuleset;
use kernel::ids::EntityId;

const ROUND: u32 = 40;

fn rules() -> &'static CompiledRuleset {
    static RULES: OnceLock<CompiledRuleset> = OnceLock::new();
    RULES.get_or_init(ClassicBench::rules)
}

/// The log of the smoke suite's game, its first 45 rounds, written once.
fn log() -> &'static PathBuf {
    static LOG: OnceLock<PathBuf> = OnceLock::new();
    LOG.get_or_init(|| {
        let spec = suite_specs_with(&Suite::named("smoke").expect("the smoke suite"), 1, 0, None, Variant::Standard).remove(0);
        let played = play_log_until(rules(), &spec, ROUND + 5).expect("the game plays");
        let dir = std::env::temp_dir().join(format!("ai-stress-humans-test-{}", std::process::id()));
        write_log(&dir, &spec, &played).expect("the log is written")
    })
}

fn position() -> position::Position {
    position::load(rules(), log(), ROUND).expect("the position loads")
}

/// The arms of `trick` at `level` on the smoke position.
fn arms(trick: Trick, level: u8) -> (scene::Plan, play::Arm, play::Arm) {
    let (rules, p) = (rules(), position());
    let plan = scene::plan(Situation::Human(trick), rules, &p, 0).unwrap_or_else(|e| panic!("{} plans on the smoke map: {e}", trick.name()));
    let edit = scene::edit_of(Situation::Human(trick), level, rules, &p, &plan).unwrap_or_else(|e| panic!("{} level {level}: {e}", trick.name()));
    let (control, edited) = scene::arms(rules, &p, &plan, &edit).expect("the arms");
    (plan, control, edited)
}

fn cast(plan: &scene::Plan) -> Cast {
    let scene::Roles::Human(cast) = plan.roles else { panic!("a human trick plans a cast") };
    cast
}

fn tile_of(game: &classic_rules::GameState, id: u64) -> Option<u32> {
    let auth = game.view().auth;
    auth.units().handle_of(EntityId(id)).and_then(|h| auth.units().get(h)).map(|u| u.core.tile.0)
}

#[test]
fn human_gates_match_the_document() {
    let doc = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/ai-stress.md")).expect("the document");
    assert!(doc.contains(&humans::gates::markdown()), "docs/ai-stress.md holds the gates table of the human tricks (`ai-stress gates`)");
    for trick in Trick::ALL {
        assert!(stress::gates::of(Situation::Human(trick)).any(|g| g.kind == stress::gates::Kind::Outcome), "{} has an outcome gate", trick.name());
    }
}

#[test]
fn every_trick_has_a_name_that_parses() {
    for trick in Trick::ALL {
        assert_eq!(Situation::parse(trick.name()), Ok(Situation::Human(trick)));
    }
    assert!(Situation::all().len() > Situation::ALL.len());
}

#[test]
fn every_trick_builds_or_says_why() {
    let (rules, p) = (rules(), position());
    for trick in Trick::ALL {
        let situation = Situation::Human(trick);
        let Ok(plan) = scene::plan(situation, rules, &p, 0) else { continue };
        for level in 0..situation.levels() {
            if let Ok(edit) = scene::edit_of(situation, level, rules, &p, &plan) {
                assert!(edit.script.is_some(), "{} level {level} has a script", trick.name());
                scene::arms(rules, &p, &plan, &edit).unwrap_or_else(|e| panic!("{} level {level}: {e}", trick.name()));
            }
        }
    }
}

#[test]
fn the_control_has_no_script_and_the_edit_raises_the_army() {
    let (plan, control, edited) = arms(Trick::Rush, 1);
    assert!(control.script.is_none() && control.tracked.is_empty());
    let Some(seat::Script::Human(Order::Campaign(order))) = edited.script.clone() else { panic!("the rush plays a campaign") };
    assert_eq!(order.units.len(), 5, "the medium rush is three ranged and two melee units");
    assert_eq!(order.units, edited.tracked, "the watch follows the army");
    let human = PlayerId(cast(&plan).human);
    for id in &order.units {
        let auth = edited.game.view().auth;
        let unit = auth.units().handle_of(EntityId(*id)).and_then(|h| auth.units().get(h)).expect("the unit exists");
        assert_eq!(unit.core.owner, human);
    }
}

#[test]
fn the_rush_declares_war_and_walks_on_the_capital() {
    let (plan, _, edited) = arms(Trick::Rush, 2);
    let c = cast(&plan);
    let Some(seat::Script::Human(Order::Campaign(order))) = edited.script.clone() else { panic!("a campaign") };
    let start: Vec<Option<u32>> = order.units.iter().map(|id| tile_of(&edited.game, *id)).collect();
    let played = play::play_arm(rules(), edited, position().variant, 3, false).expect("plays");
    assert_eq!(played.script.get("script_war"), Some(&1.0), "the rusher is at war: {:?}", played.script);
    assert!(played.window.view().auth.diplomacy().at_war(PlayerId(c.human), PlayerId(c.seat)) || played.window.view().auth.module().relations().peace_treaty_until(PlayerId(c.human), PlayerId(c.seat)) > 0);
    let moved = order.units.iter().zip(&start).any(|(id, at)| tile_of(&played.window, *id) != *at);
    assert!(moved, "the army walked");
}

#[test]
fn a_waiting_army_is_held_where_it_stands() {
    let (_, _, edited) = arms(Trick::Vulture, 0);
    let Some(seat::Script::Human(Order::Campaign(order))) = edited.script.clone() else { panic!("a campaign") };
    let start: Vec<Option<u32>> = order.units.iter().map(|id| tile_of(&edited.game, *id)).collect();
    let played = play::play_arm(rules(), edited, position().variant, 3, false).expect("plays");
    if played.script.get("script_war") == Some(&0.0) {
        let now: Vec<Option<u32>> = order.units.iter().map(|id| tile_of(&played.window, *id)).collect();
        assert_eq!(now, start, "the seat's AI moved none of the waiting units");
    }
}

#[test]
fn the_script_is_data_in_a_saved_scene() {
    let (rules, p) = (rules(), position());
    let dir = std::env::temp_dir().join(format!("ai-stress-humans-scenes-{}", std::process::id()));
    let done = stress::run_instance(rules, &p, 0, &[Situation::Human(Trick::Squat)], Some(&dir), false);
    if done.rows.is_empty() {
        return;
    }
    let saved: stress::Saved = serde_json::from_slice(&std::fs::read(dir.join("hmo5-000-L0.json")).expect("the scene is saved")).expect("it parses");
    assert!(matches!(saved.script, Some(seat::Script::Human(Order::Squat(_)))), "the squat script is saved: {:?}", saved.script);
    stress::rebuild(rules, &saved).expect("it rebuilds to its hash");
}

#[test]
fn a_trick_plays_the_same_twice() {
    let (rules, p) = (rules(), position());
    let run = || stress::run_instance(rules, &p, 0, &[Situation::Human(Trick::Bait)], None, false);
    let (a, b) = (run(), run());
    let json = |d: &stress::Done| serde_json::to_string(&d.rows).expect("rows serialise");
    assert_eq!(json(&a), json(&b));
}

#[test]
fn the_swindler_offers_its_ware() {
    let (_, _, edited) = arms(Trick::Swindle, 2);
    let played = play::play_arm(rules(), edited, position().variant, 2, false).expect("plays");
    assert!(played.script.contains_key("script_offers"), "the swindle counts its offers: {:?}", played.script);
}
