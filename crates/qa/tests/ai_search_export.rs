//! The search teacher's defer choice as an imitation label
//! (classic/docs/ai-search-teacher.md, «Labels»): a game of
//! `--learner-mind search` on a scene whose units are created in a drawn
//! order exports (`ai-features --minds learner`) its deferrals as unit
//! decisions that chose the defer candidate (format 9, tag 251, last), and
//! the decisions that offered it with the candidate last, not chosen.

use aurelia_qa::bench::modules::BenchRules;
use aurelia_qa::bench::modules::ClassicBench;
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::export::{self, Selection};
use aurelia_qa::grounds::{self, BattleKind, Grounds, Rung};
use aurelia_qa::selfplay::{self, Config};
use classic_rules::ai::net_e2e::encode::tag;

#[test]
fn deferrals_export_as_the_defer_candidates_labels() {
    let rules = ClassicBench::rules();
    let rung = grounds::LADDER.iter().position(|x| *x == Rung::Disorder(&Rung::Volley(0, 0))).expect("a rung") as u32;
    let config = Config {
        suite: Suite::named("smoke").unwrap(),
        sizes: Vec::new(),
        rounds: 200,
        net: String::new(),
        net_seats: 1,
        opponents: vec!["standard".to_owned()],
        temperature: 1.0,
        group: 1,
        seed_offset: 0,
        sample_seed: 0,
        from: None,
        learner_mind: Some("search:rollouts=1".to_owned()),
        speed: String::new(),
        heads: None,
        decision_values: None,
        ladder_solvable: false,
        grounds: Some(Grounds::Battle { kind: BattleKind::Siege, rounds: (20, 40), army: (6, 10), scripts: grounds::vary_scripts(), difficulty: 1.0, stage: None, rung: Some(rung), minimal: true, vary: true }),
    };
    let (mut deferred, mut offered) = (0, 0);
    for job in selfplay::jobs(&config, 8) {
        let played = selfplay::play(&rules, &config, &job).unwrap();
        let search = played.meta.search.clone().expect("the search's record");
        let game = serde_json::to_value(&played.meta).unwrap();
        let scope = export::scope_of_game(&game, Some(&["learner".to_owned()])).unwrap();
        let shard = export::features_of(&rules, &played.meta.spec, &played.journal, Selection::Sample(1), false, &scope).unwrap();
        assert!(shard.counts.dropped.is_empty(), "{}: {:?}", job.stem(), shard.counts.dropped);
        let mut chosen = 0;
        let mut with = 0;
        for d in &shard.decisions {
            let metas = &shard.candidate_meta[d[3] as usize..(d[3] + d[4]) as usize];
            let at = metas.iter().position(|m| m[0] == tag::DEFER);
            if let Some(at) = at {
                assert_eq!(at, metas.len() - 1, "{}: the defer candidate last", job.stem());
                with += 1;
                chosen += usize::from(d[5] as usize == at);
            }
        }
        assert_eq!(chosen, search.deferrals.len(), "{}: every deferral a label", job.stem());
        assert_eq!(with, search.deferrals.len() + search.offered.len(), "{}: and every offer", job.stem());
        deferred += chosen;
        offered += with;
    }
    assert!(deferred > 0 && offered > deferred, "deferrals {deferred}, offers {offered}");
}
