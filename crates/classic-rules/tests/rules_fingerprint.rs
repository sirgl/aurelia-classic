//! The classic ruleset fingerprint is pinned in every edition a save may carry:
//! moving rules between the kernel and the rules module (platform/PLATFORM.md
//! stage 4) must hash the same bytes in the same order, or every save and
//! replay of the current content stops loading.

use classic_content::core::core_rules;
use classic_rules::persistence::decode::{decode, decode_snapshot};
use classic_rules::persistence::encode::encode;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, CompiledRuleset, GameState};

/// `CompiledRuleset::hash` of the classic content. Re-pinned for content
/// changes (rules that changed what a row does): the gameplay wave
/// (nation traits that were inert, city name lists, spy names), then
/// the resources at sea (`sea_density`, the modern strategics' land counts),
/// then the game speeds table (`speeds.ron`), then the Shoshone start unit and the Pathfinder's sight.
const CURRENT: u64 = classic_rules::classic::CLASSIC_FINGERPRINT;
/// The fingerprints of the earlier editions the snapshot loader migrates
/// (`Classic::EARLIER_EDITIONS`), in the order of the list: before role
/// bindings, before the module id, before city names, before the naval
/// rules, before the city strike, before the unit abilities; then two
/// editions of main: the one after the unit effects (free maintenance,
/// great-work experience, general aura, golden age points; 13783899014204876011),
/// the one before the gameplay wave (9041359597632342489), the one before the AI role list
/// (2151245367700835936: `ai_branches` replaced two ordered lists in `roles.ron`, which the
/// fingerprint covers) and the last one of main before the resources at sea
/// (16018243681386759787: the rows differ in the map generator's counts only, a saved map
/// keeps its deposits) and the one before the rename (11011276917743099964: the module id
/// and the content keys, which the hash covers, carried an earlier module name then).
const EARLIER: [u64; 16] = [
    1312007536065307279,
    8423526412409906946,
    2736398163960058391,
    13258587229036033521,
    12001440589935184063,
    8950411716607108964,
    13783899014204876011,
    9041359597632342489,
    2151245367700835936,
    16018243681386759787,
    11011276917743099964,
    // Before the data was re-sourced from Unciv (docs/classic/RESOURCING.md).
    10671736804218944378,
    // Before the formula pass and the Encyclopedia/Unciv checks of the catalogs.
    5756561721247808815,
    // Before the own trade route system (docs/classic/BNW-DESIGN.md)
    18180021184894311762,
    // Before the game speeds: the same rows, the speeds table added (`tests/game_speeds.rs`).
    14820141147641077048,
    // Before the Shoshone start unit and the Pathfinder's Extra Sight (`tests/nation_start_units.rs`).
    3662418266917708599,
];

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

#[test]
fn the_classic_fingerprint_is_pinned() {
    assert_eq!(rules().hash(), CURRENT);
    assert_eq!(CURRENT, 835780351697724273);
}

#[test]
fn saves_of_every_earlier_edition_still_load() {
    let rules = rules();
    let config = GameConfig { players: 2, width: 8, height: 8, rng_seed: 1 };
    let mut save = encode(&GameState::new(&rules, config), &rules);
    for fingerprint in EARLIER {
        save.header.ruleset_hash = fingerprint;
        assert!(decode_snapshot(&save, &rules).is_ok(), "edition {fingerprint} loads");
        assert!(decode(&save, &rules).is_err(), "replay stays strict for {fingerprint}");
    }
}
