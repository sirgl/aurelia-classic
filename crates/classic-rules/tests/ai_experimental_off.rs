//! The game is built without `experimental-ai`: the AI is `standard` whatever `AURELIA_AI_VARIANT`
//! says (a net variant without weights, or a name that does not exist, panics only in the bench).
//! Since the thirteenth promotion (11 November) `standard` plays as `next-w` in this build too, since
//! the fourteenth (14 November) as `next-x`, since the fifteenth (22 November) as `nets-x-site`
//! (the site net's table is compiled in this build since), and since the sixteenth (23 November) as `net2`
//! (`ai::net2` is compiled in every build).

#![cfg(not(feature = "experimental-ai"))]

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::replay::authoritative_hash;
use classic_rules::ai::strategy_net::trace;
use classic_rules::scenario::{generate_new_game, generate_new_game_with, NewGameOptions, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset};

/// The hash after 120 rounds of the game of `rounds`, every seat played by `net2` with `experimental-ai` on the
/// source of the sixteenth promotion (`ai/promote-net2`, the merge of `ai/net2` into main 64df245):
/// `AiDriver::with_mind(.., ClassicMind::new(Variant::Net2))` on the same scenario, fasttest profile. The same source
/// gave the old `standard` (now `pre-net2`, the play of `nets-x-site`) `PRE_NET2_120`, the pin before: `net2`
/// plays these 120 rounds otherwise, so they tell the two apart.
/// History: the thirteenth promotion pinned `next-w` here (5667b774e7b92618 on 5079048; the
/// standard before it, now `pre-next`, gave f9dde2e4e6e3d10c), re-pinned for deals by value
/// (rules/deals, 12 November) to c13fb8a6f3dfced8; the fourteenth pinned `next-x` there, with the
/// same hash (no war, no lost city and no vote in these rounds); the fifteenth pinned `nets-x-site`
/// (0d2924db39996453; the old standard, now `pre-nets-x`, c13fb8a6f3dfced8).
const NET2_120: u64 = 0x27a8_480f_a63f_cfd8;
/// The same game after 120 rounds played by the old `standard` (now `pre-net2`), same source.
const PRE_NET2_120: u64 = 0x0d29_24db_3999_6453;

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
    for name in ["net", "strategy-net", "unit-net", "hybrid", "site-net", "choice-net", "standard-scripted", "no-such-variant", "pre-camps", "pre-next", "next-w", "pre-next-x", "next-x", "pre-nets-x", "nets-x-site", "econ-site", "net2", "net2-defence", "tac2", "net2-tac", "pre-net2"] {
        std::env::set_var("AURELIA_AI_VARIANT", name);
        assert_eq!(ClassicMind::default().variant(), Variant::Standard, "{name}");
        assert_eq!(rounds(&rules, 12), plain, "{name}");
    }
    std::env::remove_var("AURELIA_AI_VARIANT");
}

#[test]
fn the_nets_have_no_name_in_this_build() {
    for name in ["net", "unit-net", "hybrid", "site-net", "choice-net", "choice-net-production", "choice-net-research", "standard-scripted", "sharp", "tall-real", "next", "next-w", "next-race", "wonder-cancel", "wonder-race", "pre-next", "war-pays", "tactician-fit", "war-muster", "siege-call", "next-war", "retake2", "next-war-retake", "vote-guard", "landless-plan", "next-x", "pre-next-x", "strategist-x", "strategist-x-threat", "econ-site", "econ-site-tall", "econ-site-own-land", "nets-x", "nets-x-site", "pre-nets-x", "net2", "net2-defence", "tac2", "net2-tac", "net2-no-war", "net2-off", "pre-net2"] {
        assert_eq!(Variant::from_name(name), None, "{name}");
    }
    assert_eq!(Variant::from_name("standard"), Some(Variant::Standard));
    // The strategy net is the game's AI since the twelfth promotion: its alias is in every build.
    assert_eq!(Variant::from_name("strategy-net"), Some(Variant::StrategyNet));
    assert!(Variant::Standard.strategy_net());
    assert_eq!(Variant::from_name("danger-net"), Some(Variant::DangerNet), "the danger net's table is the game's");
    // The game's AI reads the site net since the fifteenth promotion: its table is compiled in this build.
    assert!(!classic_rules::ai::site_net::references().is_empty(), "the site net's table is the game's");
    // The game's AI plays `net2` since the sixteenth promotion, in this build too.
    assert!(ClassicMind::default().plays_net2() && Variant::Standard.net2(), "the game's AI plays net2");
}

/// The sixteenth promotion: the game built without the feature plays the game `net2` plays with it
/// (`NET2_120`, pinned with the feature), not the old standard's.
#[test]
fn the_game_without_the_feature_plays_net2() {
    let rules = compile_rules(core_rules()).unwrap();
    std::env::remove_var("AURELIA_AI_VARIANT");
    let last = *rounds(&rules, 120).last().unwrap();
    assert_ne!(NET2_120, PRE_NET2_120);
    assert_eq!(last, NET2_120, "{last:016x}");
}

/// The hash after `CITY_STATES_ROUNDS` rounds of a 5-seat game with 6 city-states, every seat played
/// by the game's AI with `experimental-ai` on (`Variant::NextW` until the fourteenth promotion,
/// `Variant::NextX` until the fifteenth, `Variant::NetsXSite` since; same scenario and rules, the kernel that
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
/// `pre-next-x`) gave 60b908187ba39a7c there (37 gifts, 5 votes), as pinned before.
/// Re-pinned for the fifteenth promotion (22 November): every seat played by `nets-x-site` with the
/// feature on `ai/nets-x` 266bd4d (`Variant::NetsXSite`): 84 gifts, 14 votes; the old `standard`
/// (`next-x`, now `pre-nets-x`) gave `CITY_STATES_OLD_STANDARD` there (69 gifts, 9 votes), as pinned before.
/// Re-pinned for the grievances and the record of thefts (release/fixes, 22 November): `nets-x-site`
/// with the feature gives the same d7a7a2695f354aa9 (124 gifts, 14 votes), `pre-nets-x` 45ce4b825a460c90
/// (85 gifts, 9 votes). With both switched off the game gives the pins before again (3b75e0a918810fcb,
/// 84 gifts; 0c9850c68098e5a6, 69 gifts); the grievances alone change the play (84 -> 124 gifts), the
/// thefts only the hash.
/// Re-pinned for the sixteenth promotion (23 November): every seat played by `net2` with the feature on
/// `ai/promote-net2` (`Variant::Net2`): 157 gifts, 14 votes; the old `standard` (`nets-x-site`, now `pre-net2`)
/// gives `CITY_STATES_OLD_STANDARD` there (d7a7a2695f354aa9, 124 gifts, 14 votes), the pin before.
/// Re-pinned for R11 at war (`ai/capitals`, 23 November: a rival's stack at our cities stays a menace once it
/// declares war): 169 gifts, 14 votes, equal with the feature on; main's 5770513fcf624657 before.
const CITY_STATES_NET2: u64 = 0x495b_1121_a057_0cd5;
/// The same case played by the old `standard` (now `pre-net2`), same source.
const CITY_STATES_OLD_STANDARD: u64 = 0xd7a7_a269_5f35_4aa9;
const CITY_STATES_ROUNDS: usize = 420;

/// The longer case of the promotion's review: city-states, gifts and votes are played in the game
/// without the feature exactly as `net2` plays them with it.
#[test]
fn the_game_without_the_feature_plays_net2_with_city_states_gifts_and_votes() {
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
    assert_ne!(CITY_STATES_NET2, CITY_STATES_OLD_STANDARD);
    assert_eq!(last, CITY_STATES_NET2, "{last:016x} ({gifts} gifts, {votes} votes)");
}
