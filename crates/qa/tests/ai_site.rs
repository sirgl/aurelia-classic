//! The site probe (`aurelia_qa::site`): a probed game is the plain game
//! move for move, every city the AI founded has a row whose features are
//! the site's, and the CSV has a column per header name.

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::record::new_game;
use aurelia_qa::site::{header, line, probe_game, spearman, Agreement, HORIZONS};
use aurelia_qa::spec::{GameSpec, WorldSize};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::scenario::MapScript;
use kernel::replay::hash::authoritative_hash;

const ROUNDS: u32 = 40;

#[test]
fn the_probe_changes_nothing_and_writes_a_row_per_founded_city() {
    let rules = ClassicBench::rules();
    let spec = GameSpec::new(MapScript::Pangaea, WorldSize::Tiny, 1).max_rounds(ROUNDS);
    let mut game = new_game(&rules, &spec);
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, &spec.difficulty), Vec::new());
    while !game.is_over() && game.view().auth.turn().round <= ROUNDS {
        assert!(driver.run(&mut game, &rules, u32::MAX).round_done || game.is_over());
    }
    let plain = authoritative_hash(&game);

    let probed = probe_game(&rules, &spec, Variant::Standard).unwrap();
    assert_eq!(probed.hash, plain, "the probed game is the plain mind's game");
    assert_eq!(probed.cost.sites as usize, probed.rows.len() + probed.failed as usize, "every founding order is a row or a failure");

    // Every nation founded its capital; every row's tile holds a city of its
    // player at the end unless it was lost.
    let auth = game.view().auth;
    let majors = spec.nations as usize;
    let capitals = probed.rows.iter().filter(|r| r.feature("own_cities") == Some(0)).count();
    assert_eq!(capitals, majors, "one capital a nation");
    for row in &probed.rows {
        let tile = kernel::ids::TileId(row.tile);
        let owner = auth.cities().iter().find(|(_, c)| c.tile == tile).map(|(_, c)| c.owner.0);
        if row.lost_round.is_none() {
            assert_eq!(owner, Some(row.player), "row {row:?}");
        }
        assert_eq!(row.feature("round"), Some(row.round as i32));
        assert!(row.feature("territory_tiles").unwrap() > 0);
        // A state at a horizon only once the game got there.
        for (horizon, state) in HORIZONS.iter().zip(row.states) {
            if row.round + horizon > probed.rounds {
                assert!(state.is_none(), "{row:?}");
            }
            if let Some(state) = state {
                assert!(state.population >= 1 && state.worked >= 1);
            }
        }
    }
    assert!(probed.rows.iter().any(|r| r.states[0].is_some()), "some city reached +{}", HORIZONS[0]);

    let columns = header().split(',').count();
    for row in &probed.rows {
        assert_eq!(line(row).split(',').count(), columns);
    }
}

#[test]
fn the_agreement_measures_order_overall_and_within_groups() {
    let rising = [(1, 1.0, 10.0), (1, 2.0, 20.0), (2, 5.0, 1.0), (2, 6.0, 3.0)];
    let agreement = Agreement::of(&rising);
    assert_eq!(agreement.groups, 2);
    assert_eq!(agreement.pairs, 2);
    assert_eq!(agreement.concordant, 1.0);
    assert!((agreement.within_spearman - 1.0).abs() < 1e-9);
    assert!(agreement.spearman < 1.0, "across groups the order is not kept");
    assert!((spearman(&[1.0, 2.0, 2.0, 3.0], &[1.0, 2.0, 2.0, 3.0]) - 1.0).abs() < 1e-9);
}
