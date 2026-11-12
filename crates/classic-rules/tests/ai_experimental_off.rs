//! The game is built without `experimental-ai`: the AI is `standard` whatever `AURELIA_AI_VARIANT`
//! says (a net variant without weights, or a name that does not exist, panics only in the bench).
//! Since the thirteenth promotion (11 November) `standard` plays as `next-w` in this build too.

#![cfg(not(feature = "experimental-ai"))]

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::replay::authoritative_hash;
use classic_rules::ai::strategy_net::trace;
use classic_rules::scenario::{generate_new_game, generate_new_game_with, NewGameOptions, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset};

/// The hash after 120 rounds of the game of `rounds`, every seat played by `next-w` built from
/// 5079048 (`ai/next`, the source of `runs/bin-w`, the reference binary of the promotion) with
/// `experimental-ai`: `AiDriver::with_mind(.., ClassicMind::new(Variant::NextW))` on the same
/// scenario, fasttest profile. The same source gave `next` d6d1ff4775d9b115 and the old
/// `standard` f9dde2e4e6e3d10c (`OLD_STANDARD_120`) after 120 rounds: the 120 rounds tell `next-w`
/// from both.
const NEXT_W_120: u64 = 0x5667_b774_e7b9_2618;
/// The same game after 120 rounds played by the old `standard` (now `pre-next`), same source.
const OLD_STANDARD_120: u64 = 0xf9dd_e2e4_e6e3_d10c;

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
    for name in ["net", "strategy-net", "unit-net", "hybrid", "site-net", "choice-net", "standard-scripted", "no-such-variant", "pre-camps", "pre-next", "next-w"] {
        std::env::set_var("AURELIA_AI_VARIANT", name);
        assert_eq!(ClassicMind::default().variant(), Variant::Standard, "{name}");
        assert_eq!(rounds(&rules, 12), plain, "{name}");
    }
    std::env::remove_var("AURELIA_AI_VARIANT");
}

#[test]
fn the_nets_have_no_name_in_this_build() {
    for name in ["net", "unit-net", "hybrid", "site-net", "choice-net", "choice-net-production", "choice-net-research", "standard-scripted", "sharp", "tall-real", "next", "next-w", "next-race", "wonder-cancel", "wonder-race", "pre-next"] {
        assert_eq!(Variant::from_name(name), None, "{name}");
    }
    assert_eq!(Variant::from_name("standard"), Some(Variant::Standard));
    // The strategy net is the game's AI since the twelfth promotion: its alias is in every build.
    assert_eq!(Variant::from_name("strategy-net"), Some(Variant::StrategyNet));
    assert!(Variant::Standard.strategy_net());
    assert_eq!(Variant::from_name("danger-net"), Some(Variant::DangerNet), "the danger net's table is the game's");
}

/// The thirteenth promotion: the game built without the feature plays the game `next-w` played
/// before the promotion (`NEXT_W_120`, pinned from the old source with the feature), not the old
/// standard's.
#[test]
fn the_game_without_the_feature_plays_next_w() {
    let rules = compile_rules(core_rules()).unwrap();
    std::env::remove_var("AURELIA_AI_VARIANT");
    let last = *rounds(&rules, 120).last().unwrap();
    assert_ne!(NEXT_W_120, OLD_STANDARD_120);
    assert_eq!(last, NEXT_W_120, "{last:016x}");
}

/// The hash after `CITY_STATES_ROUNDS` rounds of a 5-seat game with 6 city-states, every seat played
/// by `next-w` with `experimental-ai` on (`Variant::NextW`, same scenario and rules, the kernel that
/// removes lost orders). The 4-seat game above has no city-states, so it never reaches the
/// gifts to city-states or the world congress's votes (the delegate price): this one does (the
/// trace below counts them).
const CITY_STATES_NEXT_W: u64 = 0x9ef8_30ba_d18c_e60c;
const CITY_STATES_ROUNDS: usize = 400;

/// The longer case of the promotion's review: city-states, gifts and votes are played in the game
/// without the feature exactly as `next-w` plays them with it.
#[test]
fn the_game_without_the_feature_plays_next_w_with_city_states_gifts_and_votes() {
    let rules = compile_rules(core_rules()).unwrap();
    std::env::remove_var("AURELIA_AI_VARIANT");
    let config = ScenarioConfig { seed: 0xA1_5EED, players: 5, width: 60, height: 40, cities: 0, units: 0, water_ratio: 15 };
    let mut game = generate_new_game_with(&rules, &config, None, NewGameOptions { city_states: 6, ..NewGameOptions::default() });
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new());
    trace::enable(true);
    let (mut rounds, mut stalls, mut gifts, mut votes) = (0, 0, 0, 0);
    while rounds < CITY_STATES_ROUNDS && stalls < 50 {
        if driver.run(&mut game, &rules, 100).round_done {
            rounds += 1;
            stalls = 0;
        } else {
            stalls += 1;
        }
        for line in trace::take() {
            gifts += usize::from(line.contains(" GIFT "));
            votes += usize::from(line.contains(" VOTE "));
        }
    }
    trace::enable(false);
    assert_eq!(rounds, CITY_STATES_ROUNDS, "the game stalled");
    assert!(gifts > 0 && votes > 0, "the case must exercise gifts and votes: {gifts} gifts, {votes} votes");
    let last = authoritative_hash(&game);
    assert_eq!(last, CITY_STATES_NEXT_W, "{last:016x} ({gifts} gifts, {votes} votes)");
}
