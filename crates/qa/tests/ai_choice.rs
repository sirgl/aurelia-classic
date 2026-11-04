//! The choice dataset (`aurelia_qa::choice`, classic/docs/ai-choice.md):
//! recording alone changes no game, exploration is a function of the seed
//! and explores, and the choice net plays the same game twice.

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::choice::probe_game;
use aurelia_qa::spec::{size_from_name, GameSpec};
use classic_rules::ai::choice_net::Kind;
use classic_rules::ai::variant::Variant;
use classic_rules::scenario::MapScript;

fn spec() -> GameSpec {
    GameSpec::new(MapScript::from_key("pangaea").expect("a map script"), size_from_name("small").expect("a size"), 3).max_rounds(60)
}

#[test]
fn recording_changes_no_game_and_exploration_is_seeded() {
    // The allocator's choices are recorded and explored on the scripted standard: since the twelfth
    // promotion `standard`'s choices are the strategy net's (`standard-scripted` is the one before).
    let rules = ClassicBench::rules();
    let plain = probe_game(&rules, &spec(), Variant::StandardScripted, 0, 4).expect("a game");
    let site = aurelia_qa::site::probe_game(&rules, &spec(), Variant::StandardScripted).expect("a game");
    assert_eq!(plain.hash, site.hash, "recording the choices changes nothing");
    assert!(plain.rows.iter().all(|r| !r.record.explored));
    assert!(plain.rows.iter().any(|r| r.record.kind == Kind::Research) && plain.rows.iter().any(|r| r.record.kind == Kind::Production));

    let explored = probe_game(&rules, &spec(), Variant::StandardScripted, 500, 4).expect("a game");
    let again = probe_game(&rules, &spec(), Variant::StandardScripted, 500, 4).expect("a game");
    assert_eq!(explored.hash, again.hash);
    assert_eq!(explored.rows, again.rows, "the same seed explores the same choices");
    assert!(explored.rows.iter().any(|r| r.record.explored));
    for row in explored.rows.iter().filter(|r| r.record.explored) {
        let chosen = row.record.chosen.expect("an explored choice is one of its set");
        assert!(chosen < row.record.rows.len() && row.record.rows.len() <= 4);
    }
    assert_ne!(explored.hash, plain.hash, "exploration changes the game");
}

#[test]
fn the_choice_net_plays_the_same_game_twice() {
    let rules = ClassicBench::rules();
    let a = probe_game(&rules, &spec(), Variant::ChoiceNet, 0, 4).expect("a game");
    let b = probe_game(&rules, &spec(), Variant::ChoiceNet, 0, 4).expect("a game");
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.rows, b.rows);
}
