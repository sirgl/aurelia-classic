//! The game is built without `experimental-ai`: the AI is `standard` whatever `AURELIA_AI_VARIANT`
//! says (a net variant without weights, or a name that does not exist, panics only in the bench).

#![cfg(not(feature = "experimental-ai"))]

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset};

fn rounds(rules: &CompiledRuleset, count: usize) -> Vec<u64> {
    let config = ScenarioConfig { seed: 0xA1_5EED, players: 4, width: 40, height: 32, cities: 0, units: 0, water_ratio: 15 };
    let mut game = generate_new_game(rules, &config);
    let mut driver = AiDriver::new(AiSettings::for_rules(rules, rules.default_difficulty()), Vec::new());
    let mut hashes = Vec::new();
    while hashes.len() < count {
        if driver.run(&mut game, rules, 100).round_done {
            hashes.push(authoritative_hash(&game));
        }
    }
    hashes
}

#[test]
fn the_variable_is_ignored_and_the_game_plays_standard() {
    let rules = compile_rules(core_rules()).unwrap();
    std::env::remove_var("AURELIA_AI_VARIANT");
    let plain = rounds(&rules, 12);
    assert!(plain.windows(2).any(|w| w[0] != w[1]), "the AI moves: the hashes change");
    for name in ["net", "strategy-net", "unit-net", "hybrid", "site-net", "choice-net", "standard-scripted", "no-such-variant", "pre-camps"] {
        std::env::set_var("AURELIA_AI_VARIANT", name);
        assert_eq!(ClassicMind::default().variant(), Variant::Standard, "{name}");
        assert_eq!(rounds(&rules, 12), plain, "{name}");
    }
    std::env::remove_var("AURELIA_AI_VARIANT");
}

#[test]
fn the_nets_have_no_name_in_this_build() {
    for name in ["net", "unit-net", "hybrid", "site-net", "choice-net", "choice-net-production", "choice-net-research", "standard-scripted"] {
        assert_eq!(Variant::from_name(name), None, "{name}");
    }
    assert_eq!(Variant::from_name("standard"), Some(Variant::Standard));
    // The strategy net is the game's AI since the twelfth promotion: its alias is in every build.
    assert_eq!(Variant::from_name("strategy-net"), Some(Variant::StrategyNet));
    assert!(Variant::Standard.strategy_net());
    assert_eq!(Variant::from_name("danger-net"), Some(Variant::DangerNet), "the danger net's table is the game's");
}
