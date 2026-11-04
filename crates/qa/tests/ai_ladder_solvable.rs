//! Solvable siege ladder scenes (`ai-selfplay --ladder-solvable`,
//! `selfplay::LadderDraw`, classic/docs/ai-net-data.md «Solvable scenes»): a
//! scene that allows a first-turn capture by construction but that the
//! shallow search teacher does not take in the learner's first turn is
//! drawn again; a scene it takes as drawn, and a scene that allows none, is
//! played byte for byte as without the flag.

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::suite::Suite;
use aurelia_qa::grounds::{self, BattleKind, Grounds};
use aurelia_qa::selfplay::{self, Config};

/// Rung 18 (clutter: enemy units around the city that strike, reinforcements)
/// of the scenes of siege12's last iteration (seed offset 5349, the standard
/// suite's small maps), played by the shallow search teacher.
fn config(rules: &classic_rules::CompiledRuleset, solvable: bool) -> Config {
    let suite = Suite::named("standard").unwrap();
    let speed = rules.speed_named("quick").expect("the quick speed");
    let rounds = speed.values.game_round(suite.rounds);
    Config {
        suite,
        sizes: vec!["small".to_owned()],
        rounds,
        net: String::new(),
        net_seats: 1,
        // The scenes are pinned on the scripted standard (`standard` before the twelfth promotion).
        opponents: vec!["standard-scripted".to_owned()],
        temperature: 1.0,
        group: 1,
        seed_offset: 5349,
        sample_seed: 0,
        from: None,
        learner_mind: Some("search:depth=0,rollouts=1".to_owned()),
        speed: speed.key.clone(),
        heads: None,
        decision_values: None,
        ladder_solvable: solvable,
        grounds: Some(Grounds::Battle { kind: BattleKind::Siege, rounds: (20, 40), army: (6, 10), scripts: grounds::vary_scripts(), difficulty: 1.0, stage: None, rung: Some(18), minimal: true, vary: true }),
    }
}

fn first_turn(played: &selfplay::Played) -> bool {
    let b = played.meta.ground.as_ref().and_then(|g| g.battle.clone()).expect("a battle");
    b.captured_round == Some(b.start_round)
}

#[test]
fn an_unsolvable_scene_is_drawn_again_and_a_solvable_one_is_kept() {
    let rules = ClassicBench::rules();
    let (on, off) = (config(&rules, true), config(&rules, false));
    // The scene numbers are those of the worlds with the balanced starts (4 November): before them the
    // scenes were 0 (no capture allowed), 2 (kept), 7 and 15 (drawn again, at attempts 1 and 6). On the new
    // worlds the teacher misses fewer scenes as drawn (10 of the first 200 scenes, the first of them 37), so
    // the scenes drawn again lie farther out: 40 and 37, again at attempts 1 and 6.
    let jobs = selfplay::jobs(&on, 41);
    // Scene 2 allows no first-turn capture (its ring short of the shots): not filtered, as without the flag.
    let open = selfplay::play(&rules, &on, &jobs[2]).unwrap();
    assert!(open.meta.ladder_draw.is_none());
    assert_eq!(open.journal, selfplay::play(&rules, &off, &jobs[2]).unwrap().journal);
    // Scene 1: taken as drawn: the same game with and without the flag.
    let kept = selfplay::play(&rules, &on, &jobs[1]).unwrap();
    let plain = selfplay::play(&rules, &off, &jobs[1]).unwrap();
    assert!(plain.meta.ladder_draw.is_none(), "off: no draw recorded");
    let d = kept.meta.ladder_draw.expect("the draw");
    assert_eq!((d.attempt, d.draws, d.solved), (0, 1, true));
    assert_eq!(kept.journal, plain.journal, "a scene kept is played byte for byte as without the flag");
    assert_eq!(kept.meta.hash, plain.meta.hash);
    // Scenes 40 and 37: the teacher misses them as drawn; drawn again (another scene for 40, another world for 37), it takes them.
    // Scene 37's seventh draw (attempt 6; the old scene 15's sixth world, not the fifth, since a natural wonder is found
    // inside the move that sees it (0.2): the worlds before the war play differently).
    for (i, attempt) in [(40, 1), (37, 6)] {
        let missed = selfplay::play(&rules, &off, &jobs[i]).unwrap();
        assert!(!first_turn(&missed), "scene {i} as drawn is not taken in the first turn");
        let again = selfplay::play(&rules, &on, &jobs[i]).unwrap();
        let d = again.meta.ladder_draw.expect("the draw");
        assert_eq!((d.attempt, d.solved), (attempt, true), "scene {i}");
        assert!(first_turn(&again), "scene {i} drawn again is taken by the teacher in the first turn");
        assert_eq!(again.meta.group, missed.meta.group, "the same group (GRPO)");
    }
}
