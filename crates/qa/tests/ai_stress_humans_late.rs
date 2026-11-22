//! The late human tricks (`aurelia_qa::stress::humans::{sniper, lobby}`): the wonder sniper, the
//! congress lobby and the city-state poacher each build on the smoke position or say why, play their
//! script for a few rounds through the rules and report their counts. Positions are a tiny game of
//! the smoke suite (round 40: it may hold no wonder order, no congress, no city-state ally).

use std::path::PathBuf;
use std::sync::OnceLock;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::export::{play_log_until, suite_specs_with, write_log};
use aurelia_qa::stress::humans::Trick;
use aurelia_qa::stress::{play, position, scene, Situation};
use classic_rules::ai::variant::Variant;
use classic_rules::CompiledRuleset;

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
        let dir = std::env::temp_dir().join(format!("ai-stress-humans-late-test-{}", std::process::id()));
        write_log(&dir, &spec, &played).expect("the log is written")
    })
}

fn position() -> position::Position {
    position::load(rules(), log(), ROUND).expect("the position loads")
}

/// Every level of `trick` on the smoke position: the plan's error says why it is not built; else each
/// level's edited arm plays three rounds and the script's counts are present.
fn plays_or_says_why(trick: Trick, counts: &[&str]) {
    let (rules, p) = (rules(), position());
    let situation = Situation::Human(trick);
    let plan = match scene::plan(situation, rules, &p, 0) {
        Ok(plan) => plan,
        Err(why) => {
            assert!(!why.is_empty(), "{} says why it is not built", trick.name());
            return;
        }
    };
    for level in 0..situation.levels() {
        let edit = scene::edit_of(situation, level, rules, &p, &plan).unwrap_or_else(|e| panic!("{} level {level}: {e}", trick.name()));
        assert!(edit.script.is_some(), "{} level {level} has a script", trick.name());
        let (_, edited) = scene::arms(rules, &p, &plan, &edit).unwrap_or_else(|e| panic!("{} level {level}: {e}", trick.name()));
        let played = play::play_arm(rules, edited, p.variant, 3, false).unwrap_or_else(|e| panic!("{} level {level} plays: {e}", trick.name()));
        for count in counts {
            assert!(played.script.contains_key(*count), "{} level {level} counts {count}: {:?}", trick.name(), played.script);
        }
    }
}

#[test]
fn the_sniper_builds_or_says_why_and_counts() {
    plays_or_says_why(Trick::Sniper, &["script_snipes", "script_stock_at_snipe"]);
}

#[test]
fn the_lobby_builds_or_says_why_and_counts() {
    plays_or_says_why(Trick::Lobby, &["script_gifted", "script_proposed", "script_votes"]);
}

#[test]
fn the_poacher_builds_or_says_why_and_counts() {
    plays_or_says_why(Trick::Poach, &["script_gifted", "script_coups", "script_flipped"]);
}

#[test]
fn the_lobby_without_a_congress_says_so() {
    let (rules, p) = (rules(), position());
    if p.game.view().auth.module().congress().founded() {
        return;
    }
    let why = scene::plan(Situation::Human(Trick::Lobby), rules, &p, 0).expect_err("no congress, no lobby");
    assert_eq!(why, "the congress is not founded");
}
