//! The defer choice in E2 (format 9, classic/docs/ai-net-data.md, «E2: the
//! net's own games»): a net that defers samples the defer candidate like any
//! other, the game replays from its log, and the export rebuilds every
//! decision — the defer candidate where the net offered it, a deferral as
//! the choice — with nothing dropped.

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::e2;
use aurelia_qa::export::Selection;
use aurelia_qa::selfplay::{self, Config};
use classic_rules::ai::net_e2e::encode::{head, tag};

fn config() -> Config {
    let weights = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../classic-rules/tests/fixtures/net_mind/random-small.bin");
    Config {
        suite: Suite::named("smoke").unwrap(),
        sizes: Vec::new(),
        rounds: 20,
        net: weights.to_str().unwrap().to_owned(),
        net_seats: 1,
        opponents: vec!["standard".to_owned()],
        temperature: 1.0,
        group: 1,
        seed_offset: 0,
        sample_seed: 7,
        from: None,
        learner_mind: None,
        speed: String::new(),
        heads: None,
        decision_values: None,
        ladder_solvable: false,
        grounds: None,
    }
}

#[test]
fn a_deferring_net_samples_replays_and_exports_aligned() {
    // The pinned random net with the choice on (its own file is version 4): bias 1 logit.
    // Each test runs in its own process (nextest), so the variable is this test's.
    std::env::set_var(classic_rules::ai::net_e2e::mind::DEFER_ENV, "1");
    let rules = ClassicBench::rules();
    let config = config();
    let job = &selfplay::jobs(&config, 1)[0];
    let played = selfplay::play(&rules, &config, job).unwrap();
    let again = selfplay::play(&rules, &config, job).unwrap();
    assert_eq!(played.journal, again.journal, "the same job, the same log");
    assert_eq!(played.trace, again.trace);
    assert!(played.trace.defer, "the trace says the net defers");
    let meta = &played.meta;
    assert!(meta.defers > 0 && meta.defers < meta.defer_offered, "{} of {} offers taken", meta.defers, meta.defer_offered);
    let (shard, arrays, counts) = e2::shard(&rules, &job.spec, &job.stem(), &played.journal, &played.trace, Selection::Sample(1)).unwrap();
    assert_eq!(counts.hash, meta.hash, "the log replays to the game's world");
    assert!(counts.dropped.is_empty(), "{:?}", counts.dropped);
    assert_eq!(counts.unit_commands_not_sent, 0);
    assert_eq!(counts.aligned + counts.empty, counts.decisions);
    // Every deferral is a decision of the shard whose choice is the defer candidate, the last one.
    let mut deferrals = 0;
    let mut offered = 0;
    for row in shard.decisions.iter().filter(|d| d[1] == head::UNIT) {
        let (first, k, choice) = (row[3] as usize, row[4] as usize, row[5] as usize);
        let tags: Vec<i32> = shard.candidate_meta[first..first + k].iter().map(|m| m[0]).collect();
        assert!(tags[..k - 1].iter().all(|t| *t != tag::DEFER), "the defer candidate is last");
        offered += usize::from(tags[k - 1] == tag::DEFER);
        deferrals += usize::from(tags[choice] == tag::DEFER);
    }
    assert_eq!(deferrals as u64, meta.defers);
    assert_eq!(offered as u64, meta.defer_offered);
    assert_eq!(arrays.logp.len(), shard.decisions.len());
}
