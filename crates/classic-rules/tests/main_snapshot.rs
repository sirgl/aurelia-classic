//! A frozen save of current main (before the names and reasons wave): it must keep opening, validating and
//! hashing the same after every change of the wave. The ruleset may move (new city name lists), so the
//! snapshot is accepted under any rules; the world itself must not change.
use classic_rules::{
    compile_rules,
    persistence::decode::{decode_with, RulesMatch},
    replay::authoritative_hash,
};

const MAIN: &[u8] = include_bytes!("fixtures/main-b613023c.aurelia");
const MAIN_RULES: u64 = 9041359597632342489;
const MAIN_WORLD: u64 = 13915994902937364124;

fn rules() -> classic_rules::rules::CompiledRuleset {
    compile_rules(classic_content::classic::classic_rules()).unwrap()
}

#[test]
fn the_frozen_main_game_still_opens_validates_and_hashes_the_same() {
    let rules = rules();
    let save = classic_rules::save_codec::from_bytes(MAIN).unwrap();
    assert_eq!(save.header.ruleset_hash, MAIN_RULES);
    let (game, _) = decode_with(&save, &rules, RulesMatch::AnyRules).unwrap();
    game.validate(&rules).unwrap();
    assert_eq!(authoritative_hash(&game), MAIN_WORLD);
}
