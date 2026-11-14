//! The game is built without `experimental-ai`: the AI is `standard` whatever `AURELIA_AI_VARIANT`
//! says (a net variant without weights, or a name that does not exist, panics only in the bench).
//! Since the thirteenth promotion (11 November) `standard` plays as `next-w` in this build too, and
//! since the fourteenth (14 November) as `next-x`.

#![cfg(not(feature = "experimental-ai"))]

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::replay::authoritative_hash;
use classic_rules::ai::strategy_net::trace;
use classic_rules::scenario::{generate_new_game, generate_new_game_with, NewGameOptions, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset};

/// The hash after 120 rounds of the game of `rounds`, every seat played by `next-x` built from
/// main 4d43b0e (the source of the fourteenth promotion's reference binary) with `experimental-ai`:
/// `AiDriver::with_mind(.., ClassicMind::new(Variant::NextX))` on the same scenario, release profile.
/// The same source gave the old `standard` (now `pre-next-x`, the play of `next-w`) the same hash:
/// the 120 rounds of this game have no war, no lost city and no vote, so the war, the retake, the
/// guard and the landless plan of `next-x` never act here. The city-states case below tells the two
/// apart (`CITY_STATES_OLD_STANDARD`).
/// History: the thirteenth promotion pinned `next-w` here (5667b774e7b92618 on 5079048; the
/// standard before it, now `pre-next`, gave f9dde2e4e6e3d10c), re-pinned for deals by value
/// (rules/deals, 12 November) to c13fb8a6f3dfced8.
const NEXT_X_120: u64 = 0xc13f_b8a6_f3df_ced8;

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
    for name in ["net", "strategy-net", "unit-net", "hybrid", "site-net", "choice-net", "standard-scripted", "no-such-variant", "pre-camps", "pre-next", "next-w", "pre-next-x", "next-x"] {
        std::env::set_var("AURELIA_AI_VARIANT", name);
        assert_eq!(ClassicMind::default().variant(), Variant::Standard, "{name}");
        assert_eq!(rounds(&rules, 12), plain, "{name}");
    }
    std::env::remove_var("AURELIA_AI_VARIANT");
}

#[test]
fn the_nets_have_no_name_in_this_build() {
    for name in ["net", "unit-net", "hybrid", "site-net", "choice-net", "choice-net-production", "choice-net-research", "standard-scripted", "sharp", "tall-real", "next", "next-w", "next-race", "wonder-cancel", "wonder-race", "pre-next", "war-pays", "tactician-fit", "war-muster", "siege-call", "next-war", "retake2", "next-war-retake", "vote-guard", "landless-plan", "next-x", "pre-next-x"] {
        assert_eq!(Variant::from_name(name), None, "{name}");
    }
    assert_eq!(Variant::from_name("standard"), Some(Variant::Standard));
    // The strategy net is the game's AI since the twelfth promotion: its alias is in every build.
    assert_eq!(Variant::from_name("strategy-net"), Some(Variant::StrategyNet));
    assert!(Variant::Standard.strategy_net());
    assert_eq!(Variant::from_name("danger-net"), Some(Variant::DangerNet), "the danger net's table is the game's");
}

/// The fourteenth promotion: the game built without the feature plays the game `next-x` played
/// before the promotion (`NEXT_X_120`, pinned from the old source with the feature). These 120
/// rounds do not tell it from the old standard; the city-states case below does.
#[test]
fn the_game_without_the_feature_plays_next_x() {
    let rules = compile_rules(core_rules()).unwrap();
    std::env::remove_var("AURELIA_AI_VARIANT");
    let last = *rounds(&rules, 120).last().unwrap();
    assert_eq!(last, NEXT_X_120, "{last:016x}");
}

/// The hash after `CITY_STATES_ROUNDS` rounds of a 5-seat game with 6 city-states, every seat played
/// by the game's AI with `experimental-ai` on (`Variant::NextW` until the fourteenth promotion,
/// `Variant::NextX` since; same scenario and rules, the kernel that
/// removes lost orders). The 4-seat game above has no city-states, so it never reaches the
/// gifts to city-states or the world congress's votes (the delegate price): this one does (the
/// trace below counts them).
/// Re-pinned for deals by value (rules/deals, 12 November), equal with the feature on (`NextW`).
/// Re-pinned for purchase prices as Unciv (rules/hurry-modifier, 13 November: dearer settlers and
/// +20 % units, a new city drops a queued national wonder): the congress's first vote came at round
/// 404, so the case runs 420 rounds (86 gifts, 5 votes), equal with the feature on (`NextW`).
/// Re-pinned for the citadel as Unciv (ai/great-people GP0, 13 November: the citadel on any tile next to
/// own land, the great person improvements on hills): 88 gifts, 5 votes.
/// Re-pinned for the tile price as Unciv (rules/purchase-holes, 14 November: one product of the speed and
/// the purchase factors, rounded once; no tile bought by a puppet): 37 gifts, 5 votes.
/// Re-pinned for the fourteenth promotion (14 November): every seat played by `next-x` with the
/// feature on main 4d43b0e (`Variant::NextX`): 69 gifts, 9 votes; the old `standard` (`next-w`, now
/// `pre-next-x`) gave `CITY_STATES_OLD_STANDARD` there (37 gifts, 5 votes), as pinned before.
const CITY_STATES_NEXT_X: u64 = 0x0c98_50c6_8098_e5a6;
/// The same case played by the old `standard` (now `pre-next-x`), same source.
const CITY_STATES_OLD_STANDARD: u64 = 0x60b9_0818_7ba3_9a7c;
const CITY_STATES_ROUNDS: usize = 420;

/// The longer case of the promotion's review: city-states, gifts and votes are played in the game
/// without the feature exactly as `next-x` plays them with it.
#[test]
fn the_game_without_the_feature_plays_next_x_with_city_states_gifts_and_votes() {
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
    assert_ne!(CITY_STATES_NEXT_X, CITY_STATES_OLD_STANDARD);
    assert_eq!(last, CITY_STATES_NEXT_X, "{last:016x} ({gifts} gifts, {votes} votes)");
}
