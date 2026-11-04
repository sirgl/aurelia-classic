//! The danger probe (`aurelia_qa::danger`): a probed game is the plain game
//! move for move, its rows are the same every time, and every row is
//! well-formed (labels within the unit's health, a dead unit loses all).

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::danger::{header, line, probe_game};
use aurelia_qa::record::new_game;
use aurelia_qa::spec::{GameSpec, WorldSize};
use classic_rules::ai::danger::FEATURES;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::scenario::MapScript;
use kernel::replay::hash::authoritative_hash;

const ROUNDS: u32 = 40;

fn spec() -> GameSpec {
    GameSpec::new(MapScript::Pangaea, WorldSize::Tiny, 1).max_rounds(ROUNDS)
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
fn the_probe_changes_nothing_and_its_rows_are_sound() {
    let rules = ClassicBench::rules();
    let spec = spec();
    let plain = plain_hash(&rules, &spec);
    let start = new_game(&rules, &spec).view().auth.turn().round;
    let rounds = ROUNDS + 1 - start;
    let settings = || AiSettings::for_rules(&rules, &spec.difficulty);

    let probed = probe_game(&rules, new_game(&rules, &spec), rounds, settings(), Variant::Standard, spec.seed).unwrap();
    assert_eq!(probed.hash, plain, "the probed game is the plain mind's game");
    assert!(!probed.rows.is_empty(), "military units at the end of turns");
    assert!(probed.cost.units as usize >= probed.rows.len(), "at most a row per unit seen");

    for row in &probed.rows {
        assert_eq!(row.features.len(), FEATURES);
        assert!((1..=100).contains(&row.hp()), "{row:?}");
        assert!((0..=100).contains(&row.damage), "{row:?}");
        assert!(row.healed >= 0 && row.hp() + row.healed <= 100, "{row:?}");
        assert!(row.exposure >= 0, "{row:?}");
        if row.dead == 1 {
            assert_eq!(row.damage, row.hp() + row.healed, "a dead unit loses all it had: {row:?}");
        }
        assert!(row.round >= start && row.round <= ROUNDS);
    }
    let columns = header().split(',').count();
    assert!(probed.rows.iter().all(|r| line(r).split(',').count() == columns), "every line has the header's columns");

    let again = probe_game(&rules, new_game(&rules, &spec), rounds, settings(), Variant::Standard, spec.seed).unwrap();
    assert_eq!(again.rows, probed.rows, "the same game gives the same rows");
}
