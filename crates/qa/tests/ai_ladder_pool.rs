//! A pool of solvable siege ladder scenes (`ai-selfplay --ladder-pool`,
//! `selfplay::LadderPool`, tools/ladder-pool.py): the jobs of a pooled rung
//! play the pool's scenes, recorded as pooled; another rung, as drawn. Its
//! own process: the pool is the process's.

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::grounds::{self, BattleKind, GroundJob, Grounds};
use aurelia_qa::selfplay::{self, Config, LadderPool};

fn config(rules: &classic_rules::CompiledRuleset, rung: u32) -> Config {
    let suite = Suite::named("standard").unwrap();
    let speed = rules.speed_named("quick").expect("the quick speed");
    let rounds = speed.values.game_round(suite.rounds);
    Config {
        suite,
        sizes: vec!["small".to_owned()],
        rounds,
        net: String::new(),
        net_seats: 1,
        opponents: vec!["standard".to_owned()],
        temperature: 1.0,
        group: 4,
        seed_offset: 0,
        sample_seed: 0,
        from: None,
        learner_mind: Some("search:depth=0,rollouts=1".to_owned()),
        speed: speed.key.clone(),
        heads: None,
        decision_values: None,
        ladder_solvable: false,
        grounds: Some(Grounds::Battle { kind: BattleKind::Siege, rounds: (20, 40), army: (6, 10), scripts: grounds::vary_scripts(), difficulty: 1.0, stage: None, rung: Some(rung), minimal: true, vary: true }),
    }
}

#[test]
fn a_pooled_rung_plays_the_pools_scenes() {
    let rules = ClassicBench::rules();
    let speed = rules.speed_named("quick").unwrap().key.clone();
    // Rung 18's scene 7 of the run at offset 5349 (one game a group) as the only entry.
    let pool = LadderPool { suite: "standard".to_owned(), sizes: vec!["small".to_owned()], speed, rounds: (20, 40), army: (6, 10), minimal: true, vary: true, rungs: [(18, vec![(5349, 7)])].into() };
    let source = selfplay::jobs(&Config { seed_offset: 5349, group: 1, ..config(&rules, 18) }, 8).remove(7);
    selfplay::set_ladder_pool(pool);
    let on = config(&rules, 18);
    let mut jobs = selfplay::jobs(&on, 8);
    let own = jobs.clone();
    assert_eq!(selfplay::pool_jobs(&on, &mut jobs).unwrap(), 8);
    for (j, o) in jobs.iter().zip(&own) {
        assert_eq!(j.spec, source.spec, "the pool's world");
        let (Some(GroundJob::Battle { seed, start, .. }), Some(GroundJob::Battle { seed: s0, start: t0, .. })) = (&j.ground, &source.ground) else { panic!("battles") };
        assert_eq!((seed, start), (s0, t0), "the pool's scene");
        assert_eq!((j.group, j.member, j.sample_seed), (o.group, o.member, o.sample_seed), "the job's own group, member, sampling");
    }
    let played = selfplay::play(&rules, &on, &jobs[0]).unwrap();
    let d = played.meta.ladder_draw.expect("recorded");
    assert!(d.pooled && d.solved && d.draws == 0);
    // Another rung: as drawn.
    let other = config(&rules, 17);
    let mut jobs = selfplay::jobs(&other, 4);
    let own = jobs.clone();
    assert_eq!(selfplay::pool_jobs(&other, &mut jobs).unwrap(), 0);
    assert_eq!(jobs.iter().map(|j| j.stem()).collect::<Vec<_>>(), own.iter().map(|j| j.stem()).collect::<Vec<_>>());
    // Other battle settings than the pool's: refused.
    let mut wrong = config(&rules, 18);
    if let Some(Grounds::Battle { army, .. }) = &mut wrong.grounds {
        *army = (4, 6);
    }
    assert!(selfplay::pool_jobs(&wrong, &mut selfplay::jobs(&wrong, 2)).is_err());
}
