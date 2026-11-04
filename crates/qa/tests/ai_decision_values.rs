//! The net's decisions valued by the search teacher's rollouts
//! (`ai-selfplay --decision-values`, `aurelia_qa::search::Values`,
//! classic/docs/ai-net-data.md «Per-decision values»): valuing reads the
//! game and never changes it, every decision of the scene's units is valued
//! or counted out, the values are consistent, and the export puts each on
//! its own decision row (`e2_dvalues.npy`).

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::e2;
use aurelia_qa::export::Selection;
use aurelia_qa::grounds::{self, BattleKind, Grounds, Rung};
use aurelia_qa::search::ValuesConfig;
use aurelia_qa::selfplay::{self, Config};
use classic_rules::ai::net_e2e::encode::head;

fn config(values: Option<ValuesConfig>) -> Config {
    let weights = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../classic-rules/tests/fixtures/net_mind/random-small.bin");
    let at = grounds::LADDER.iter().position(|x| *x == Rung::Reach(2, 2)).expect("a rung") as u32;
    Config {
        suite: Suite::named("smoke").unwrap(),
        sizes: Vec::new(),
        rounds: 200,
        net: weights.to_str().unwrap().to_owned(),
        net_seats: 1,
        // The scenes are pinned on the scripted standard (`standard` before the twelfth promotion).
        opponents: vec!["standard-scripted".to_owned()],
        temperature: 1.0,
        group: 1,
        seed_offset: 0,
        sample_seed: 3,
        from: None,
        learner_mind: None,
        speed: String::new(),
        heads: None,
        decision_values: values,
        ladder_solvable: false,
        grounds: Some(Grounds::Battle { kind: BattleKind::Siege, rounds: (20, 40), army: (6, 10), scripts: grounds::vary_scripts(), difficulty: 1.0, stage: None, rung: Some(at), minimal: true, vary: true }),
    }
}

#[test]
fn decisions_are_valued_without_changing_the_game_and_exported_on_their_rows() {
    // The pinned random net with the defer choice on: deferrals are valued too.
    std::env::set_var(classic_rules::ai::net_e2e::mind::DEFER_ENV, "1");
    let rules = ClassicBench::rules();
    let valued_config = config(Some(ValuesConfig { k: 4, depth: 1, rollouts: 2, share: 1.0, later: 1.0 }));
    let plain = config(None);
    let mut valued_total = 0;
    for job in selfplay::jobs(&valued_config, 2) {
        let played = selfplay::play(&rules, &valued_config, &job).unwrap();
        let control = selfplay::play(&rules, &plain, &job).unwrap();
        assert_eq!(played.journal, control.journal, "{}: valuing never changes the game", job.stem());
        assert_eq!(played.trace, control.trace);
        assert!(control.meta.decision_values.is_none());
        let record = played.meta.decision_values.clone().expect("the values' record");
        let s = &record.stats;
        assert!(s.decisions > 0, "{s:?}");
        assert_eq!(s.mismatched, 0, "the rebuilt candidates fit every decision: {s:?}");
        assert_eq!(s.sampled_out, 0);
        assert_eq!(s.valued + s.skipped, s.decisions, "{s:?}");
        assert_eq!(record.values.len() as u64, s.valued);
        let unit_decisions = played.trace.turns.iter().flat_map(|t| &t.decisions).filter(|d| i32::from(d.head) == head::UNIT).count() as u64;
        assert!(s.decisions <= unit_decisions);
        for v in &record.values {
            let at = v.candidates.iter().position(|c| *c == v.chosen).expect("the choice is valued");
            assert_eq!(v.q_chosen, v.q[at]);
            let (lo, hi) = v.q.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), q| (lo.min(*q), hi.max(*q)));
            assert_eq!(v.q_best, hi);
            assert!(v.q_mean >= lo - 1e-5 && v.q_mean <= hi + 1e-5, "{v:?}");
            assert!(v.candidates.windows(2).all(|w| w[0] < w[1]), "in the net's order, once each: {v:?}");
            // The key finds the decision in the trace: a unit decision of the learner with that choice.
            let turn = played.trace.turns.iter().find(|t| t.player == v.player && t.round == v.round).expect("its turn");
            let d = &turn.decisions[v.ordinal as usize];
            assert_eq!((i32::from(d.head), d.chosen), (head::UNIT, Some(v.chosen)));
        }
        valued_total += record.values.len();
        // The export: one row per decision, the values on the valued ones, NaN elsewhere.
        let game = serde_json::to_value(&played.meta).unwrap();
        let (_, arrays, counts) = e2::shard_of_game(&rules, &played.meta.spec, &job.stem(), &played.journal, &played.trace, Selection::Sample(1), &game).unwrap();
        assert!(counts.dropped.is_empty(), "{:?}", counts.dropped);
        assert_eq!(arrays.dvalues.len(), arrays.logp.len());
        let rows: Vec<&[f32; 4]> = arrays.dvalues.iter().filter(|r| !r[0].is_nan()).collect();
        assert_eq!(rows.len(), record.values.len(), "every valued decision on a row of its own");
        let mut want: Vec<[f32; 4]> = record.values.iter().map(|v| [v.q_chosen, v.q_mean, v.q_best, v.q.len() as f32]).collect();
        let mut got: Vec<[f32; 4]> = rows.into_iter().copied().collect();
        let key = |r: &[f32; 4]| r.map(f32::to_bits);
        want.sort_by_key(key);
        got.sort_by_key(key);
        assert_eq!(got, want);
    }
    assert!(valued_total > 0);
}

#[test]
fn a_share_values_part_of_the_decisions() {
    let rules = ClassicBench::rules();
    let config = config(Some(ValuesConfig { k: 2, depth: 1, rollouts: 1, share: 0.3, later: 0.0 }));
    let job = &selfplay::jobs(&config, 1)[0];
    let played = selfplay::play(&rules, &config, job).unwrap();
    let s = played.meta.decision_values.expect("the record").stats;
    assert!(s.sampled_out > 0 && s.valued > 0, "{s:?}");
    assert_eq!(s.valued + s.sampled_out + s.skipped + s.mismatched, s.decisions);
}

/// `depth` 0: each candidate to the end of the learner's own turn only (the
/// fast policy finishes the turn, `search::turn_score`), one seed.
#[test]
fn the_end_of_the_own_turn_values_without_changing_the_game() {
    let rules = ClassicBench::rules();
    let shallow = config(Some(ValuesConfig { k: 6, depth: 0, rollouts: 1, share: 1.0, later: 0.0 }));
    let job = &selfplay::jobs(&shallow, 1)[0];
    let played = selfplay::play(&rules, &shallow, job).unwrap();
    let control = selfplay::play(&rules, &config(None), job).unwrap();
    assert_eq!(played.journal, control.journal);
    let record = played.meta.decision_values.expect("the record");
    let s = &record.stats;
    assert!(s.valued > 0 && s.mismatched == 0, "{s:?}");
    assert_eq!(s.rollouts, record.values.iter().map(|v| v.q.len() as u64).sum::<u64>(), "one seed a candidate");
    // A first turn's values: a capture this turn is worth 1, nothing above it (and the tie-break's twentieth).
    assert!(record.values.iter().all(|v| v.q.iter().all(|q| (0.0..=1.06).contains(q))), "{:?}", record.values);
}
