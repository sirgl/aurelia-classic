//! The search teacher (`aurelia_qa::search`, classic/docs/ai-search-teacher.md):
//! `--learner-mind search` plays a ground's learner by rollouts in copies of
//! the game. Its games are deterministic (the same job, the same log), its
//! commands replay as the log's (the scene's edits made again), it takes a
//! move-then-strike siege rung the scripted teachers miss, and its game
//! records what it did.

use aurelia_qa::bench::modules::BenchRules;
use aurelia_qa::bench::modules::ClassicBench;
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::e2;
use aurelia_qa::export::Selection;
use aurelia_qa::grounds::{self, BattleKind, GroundJob, Grounds, Rung};
use aurelia_qa::selfplay::{self, Config};

fn config(rung: Rung, mind: &str) -> Config {
    let at = grounds::LADDER.iter().position(|x| *x == rung).expect("a rung") as u32;
    Config {
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
        learner_mind: Some(mind.to_owned()),
        speed: String::new(),
        heads: None,
        decision_values: None,
        ladder_solvable: false,
        grounds: Some(Grounds::Battle { kind: BattleKind::Siege, rounds: (20, 40), army: (6, 10), scripts: grounds::vary_scripts(), difficulty: 1.0, stage: None, rung: Some(at), minimal: true, vary: true }),
    }
}

/// The capturer two tiles from a free wall (`Rung::Reach(2, 2)`): the search
/// walks it in and strikes in the first turn; the game is the same twice and
/// its log replays to the game's hash.
#[test]
fn the_search_moves_then_strikes_and_replays() {
    let rules = ClassicBench::rules();
    let config = config(Rung::Reach(2, 2), "search:rollouts=1");
    let mut taken = 0;
    for job in selfplay::jobs(&config, 3) {
        assert!(matches!(job.ground, Some(GroundJob::Battle { .. })));
        let played = selfplay::play(&rules, &config, &job).unwrap();
        let again = selfplay::play(&rules, &config, &job).unwrap();
        assert_eq!(played.journal, again.journal, "{}: the search is deterministic", job.stem());
        let battle = played.meta.ground.as_ref().and_then(|g| g.battle.clone()).unwrap();
        let search = played.meta.search.clone().expect("the search's record");
        assert!(search.decisions > 0 && search.rollouts >= search.decisions, "{search:?}");
        taken += u32::from(battle.captured_round == Some(battle.start_round));
        // The log replays to the game's hash: the scene's edits again, the search's commands as any.
        let game = serde_json::to_value(&played.meta).unwrap();
        let (_, _, counts) = e2::shard_of_game(&rules, &played.meta.spec, &job.stem(), &played.journal, &played.trace, Selection::Sample(1), &game).unwrap();
        assert_eq!(counts.hash, played.meta.hash, "{}: the log replays", job.stem());
    }
    assert!(taken >= 2, "the search takes the move-then-strike rung in its first turn: {taken} of 3");
}

/// `depth=0`: the search to the end of the learner's own turn (no round end,
/// no mind playing on, one seed): it still walks the capturer in and strikes,
/// and its games stay deterministic.
#[test]
fn the_end_of_own_turn_search_moves_then_strikes() {
    let rules = ClassicBench::rules();
    let config = config(Rung::Reach(2, 2), "search:depth=0,rollouts=1");
    let mut taken = 0;
    for job in selfplay::jobs(&config, 3) {
        let played = selfplay::play(&rules, &config, &job).unwrap();
        let again = selfplay::play(&rules, &config, &job).unwrap();
        assert_eq!(played.journal, again.journal, "{}: deterministic", job.stem());
        let battle = played.meta.ground.as_ref().and_then(|g| g.battle.clone()).unwrap();
        taken += u32::from(battle.captured_round == Some(battle.start_round));
    }
    assert!(taken >= 2, "{taken} of 3 in the first turn");
}
