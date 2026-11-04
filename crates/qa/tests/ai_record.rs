//! The recording mind (`aurelia_qa::record`): a recorded game is the plain
//! game move for move, its file is JSON Lines of every kind with every
//! decision pointing at an earlier observation, and `every` thins the sights.

use std::collections::BTreeMap;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::record::{new_game, record_game, Recording};
use aurelia_qa::spec::{GameSpec, WorldSize};
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::scenario::MapScript;
use kernel::ai::driver::AiDriver as Driver;
use kernel::replay::hash::authoritative_hash;

const ROUNDS: u32 = 40;

fn spec() -> GameSpec {
    GameSpec::new(MapScript::Pangaea, WorldSize::Tiny, 1).max_rounds(ROUNDS)
}

fn scratch(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aurelia-ai-record-{}-{name}.jsonl", std::process::id()))
}

/// The plain mind's game, as the bench plays it.
fn plain_hash(rules: &classic_rules::CompiledRuleset, spec: &GameSpec) -> u64 {
    let mut game = new_game(rules, spec);
    let mut driver = AiDriver::new(AiSettings::for_rules(rules, &spec.difficulty), Vec::new());
    while !game.is_over() && game.view().auth.turn().round <= ROUNDS {
        assert!(driver.run(&mut game, rules, u32::MAX).round_done || game.is_over());
    }
    authoritative_hash(&game)
}

#[test]
fn the_recording_changes_nothing_and_writes_every_kind_of_line() {
    let rules = ClassicBench::rules();
    let spec = spec();
    let plain = plain_hash(&rules, &spec);

    // (a) The wrapper alone, driven as the bench drives a mind.
    let mut game = new_game(&rules, &spec);
    let recording = Recording::new(ClassicMind::new(Variant::Standard), Box::new(std::io::sink()), 1);
    let mut driver = Driver::with_mind(AiSettings::for_rules(&rules, &spec.difficulty), Vec::new(), recording);
    while !game.is_over() && game.view().auth.turn().round <= ROUNDS {
        assert!(driver.run(&mut game, &rules, u32::MAX).round_done || game.is_over());
    }
    assert_eq!(authoritative_hash(&game), plain, "the recording mind plays the plain mind's game");

    // (a) again through the recorder with its outcome and end lines.
    let path = scratch("all");
    let file = std::fs::File::create(&path).unwrap();
    let summary = record_game(&rules, &spec, Variant::Standard, 1, Box::new(file)).unwrap();
    assert_eq!(summary.hash, plain, "the recorder plays the plain mind's game");

    // (b) Every line is JSON, every tag is there, every decision points back.
    let text = std::fs::read_to_string(&path).unwrap();
    let mut by_tag: BTreeMap<String, u64> = BTreeMap::new();
    let mut observed = std::collections::BTreeSet::new();
    let mut last_seq = 0;
    // The legal list of the last near, to place the unit decision after it.
    let mut actions: Vec<serde_json::Value> = Vec::new();
    let mut placed = 0;
    for (n, line) in text.lines().enumerate() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("line {}: {e}", n + 1));
        let tag = value["t"].as_str().unwrap_or_else(|| panic!("line {} has no tag", n + 1)).to_owned();
        match tag.as_str() {
            "sight" | "near" => {
                let seq = value["seq"].as_u64().unwrap();
                assert!(seq > last_seq, "line {}: seq {seq} after {last_seq}", n + 1);
                last_seq = seq;
                observed.insert(seq);
                // A player with nothing on the map yet knows no tile; a unit
                // always sees its own.
                let tiles = value["tiles"].as_array().unwrap_or_else(|| panic!("line {}: a {tag} without tiles", n + 1));
                assert!(tag == "sight" || !tiles.is_empty(), "line {}: a near without tiles", n + 1);
                if tag == "near" {
                    actions = value["actions"].as_array().unwrap_or_else(|| panic!("line {}: a near without actions", n + 1)).clone();
                    assert!(actions.iter().all(|a| a["kind"].is_string()), "line {}: an action without a kind", n + 1);
                }
            }
            "decision" => {
                let seq = value["seq"].as_u64().unwrap();
                assert!(seq > last_seq, "line {}: seq {seq} after {last_seq}", n + 1);
                last_seq = seq;
                let observation = value["observation"].as_u64().unwrap_or_else(|| panic!("line {}: no observation", n + 1));
                assert!(observation < seq && observed.contains(&observation), "line {}: observation {observation} is no earlier sight or near", n + 1);
                let phase = value["phase"].as_str().unwrap();
                // The tree names the command's variant wherever a view named
                // its entities; a unit decision's choice points at its tree.
                if value["command"].is_string() && phase != "strategic" {
                    assert!(value["action"]["kind"].is_string(), "line {}: a command without its tree", n + 1);
                }
                if let Some(choice) = value["choice"].as_u64() {
                    assert_eq!(phase, "unit");
                    assert_eq!(actions[choice as usize], value["action"], "line {}: the choice is not the action", n + 1);
                    placed += 1;
                }
            }
            "outcome" => {
                let outcome = value["outcome"].as_str().unwrap();
                assert!(["applied", "rejected", "no_command"].contains(&outcome), "line {}: outcome {outcome}", n + 1);
            }
            _ => {}
        }
        *by_tag.entry(tag).or_default() += 1;
    }
    for tag in ["game", "sight", "near", "decision", "outcome", "end"] {
        assert!(by_tag.get(tag).is_some_and(|n| *n > 0), "no {tag} line: {by_tag:?}");
    }
    assert_eq!(by_tag["game"], 1);
    assert_eq!(by_tag["end"], 1);
    assert!(placed > 0, "no unit decision was placed in its legal list");
    assert_eq!(summary.written.missed, 0, "the legal list missed the AI's own commands");
    assert_eq!(text.lines().count() as u64, summary.written.lines.values().sum::<u64>());
    assert_eq!(text.len() as u64, summary.written.bytes);
    assert!(text.lines().next().unwrap().starts_with(r#"{"t":"game""#));
    assert!(text.lines().last().unwrap().starts_with(r#"{"t":"end""#));

    // (c) Sights on every tenth round only: fewer sight lines, the same game.
    let thinned = scratch("every-10");
    let file = std::fs::File::create(&thinned).unwrap();
    let every = record_game(&rules, &spec, Variant::Standard, 10, Box::new(file)).unwrap();
    assert_eq!(every.hash, plain);
    assert!(every.written.lines["sight"] < summary.written.lines["sight"], "{:?} against {:?}", every.written.lines, summary.written.lines);
    assert!(every.written.lines["sight"] > 0);
    assert_eq!(every.written.lines["decision"], summary.written.lines["decision"]);
    assert_eq!(every.written.lines["outcome"], summary.written.lines["outcome"]);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(thinned);
}
