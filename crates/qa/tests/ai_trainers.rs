//! The trainers (`aurelia_qa::trainers`, classic/docs/ai-trainers.md): a scene is a function of its index,
//! every family builds in every era it claims, the win rules decide what they say, and a sample of every
//! family stays winnable (a solver wins it, the passive control does not).

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::trainers::families::{self, Fam};
use aurelia_qa::trainers::{act, lab, play, suite};
use classic_rules::ClassicGame;

fn tiles(game: &classic_rules::GameState, sc: &aurelia_qa::trainers::Scene) -> Vec<(u32, u16, u32)> {
    sc.mine.iter().chain(&sc.theirs).filter_map(|p| act::core_of(game, p.h).map(|c| (c.tile.0, p.kind.0, u32::from(c.hp)))).collect()
}

#[test]
fn a_scene_is_a_function_of_its_index() {
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "medieval").unwrap();
    for fam in [Fam::Duel, Fam::Hold, Fam::Guard] {
        let (g1, s1) = families::build(&rules, fam, era, 3, 0).unwrap();
        let (g2, s2) = families::build(&rules, fam, era, 3, 0).unwrap();
        assert_eq!(tiles(&g1, &s1), tiles(&g2, &s2), "{}: the same index, the same scene", fam.name());
        assert_eq!(s1.info, s2.info);
        let (g3, s3) = families::build(&rules, fam, era, 3, 1).unwrap();
        assert_ne!(tiles(&g1, &s1), tiles(&g3, &s3), "{}: another attempt is another scene", fam.name());
        // Played twice by the same player: the same row.
        let a = play(&rules, g1.clone(), s1.clone(), "standard", false).unwrap();
        let b = play(&rules, g2, s2, "standard", false).unwrap();
        assert_eq!((a.win, a.rounds, &a.measures), (b.win, b.rounds, &b.measures));
    }
}

#[test]
fn the_win_rules_decide_what_they_say() {
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "medieval").unwrap();
    // Duel: every foe gone with ours alive wins; ours gone loses.
    let (game, mut sc) = families::build(&rules, Fam::Duel, era, 0, 0).unwrap();
    let mut g = game.clone();
    for p in sc.theirs.clone() {
        g.debug_remove_unit(&rules, p.h);
    }
    assert_eq!(families::judge(&rules, &g, &mut sc, 1), Some(true));
    let mut g = game.clone();
    for p in sc.mine.clone() {
        g.debug_remove_unit(&rules, p.h);
    }
    assert_eq!(families::judge(&rules, &g, &mut sc.clone(), 1), Some(false));
    // The round limit with foes left loses; before it the scene goes on.
    assert_eq!(families::judge(&rules, &game, &mut sc.clone(), 1), None);
    assert_eq!(families::judge(&rules, &game, &mut sc.clone(), Fam::Duel.rounds()), Some(false));
    // Hold: an enemy unit on the pass loses at once.
    let (game, mut sc) = families::build(&rules, Fam::Hold, era, 0, 0).unwrap();
    let pass = sc.hold.unwrap();
    let mut g = game.clone();
    for p in sc.mine.clone() {
        if act::core_of(&g, p.h).is_some_and(|c| c.tile == pass) {
            g.debug_remove_unit(&rules, p.h);
        }
    }
    let foe = sc.theirs[0].clone();
    g.debug_remove_unit(&rules, foe.h);
    let h = g.debug_spawn_unit(&rules, sc.enemy, pass, foe.kind);
    let id = g.view().auth.units().get(h).unwrap().core.id;
    sc.theirs[0] = aurelia_qa::trainers::Piece { h, id, kind: foe.kind, role: foe.role };
    assert_eq!(families::judge(&rules, &g, &mut sc, 1), Some(false));
    // Hold: the limit reached with the pass ours wins.
    let (game, mut sc) = families::build(&rules, Fam::Hold, era, 0, 0).unwrap();
    assert_eq!(families::judge(&rules, &game, &mut sc, Fam::Hold.rounds()), Some(true));
    // Guard: a civilian lost loses.
    let (game, mut sc) = families::build(&rules, Fam::Guard, era, 0, 0).unwrap();
    let mut g = game.clone();
    let civ = sc.mine.iter().find(|p| p.role == "civilian").unwrap().h;
    g.debug_remove_unit(&rules, civ);
    assert_eq!(families::judge(&rules, &g, &mut sc, 1), Some(false));
}

/// Slow tier: builds every family in every era it claims (one of the first attempts of instance 0).
#[test]
fn every_family_builds_in_every_era_it_claims() {
    let rules = ClassicBench::rules();
    let eras = lab::era_names(&rules);
    let mut missing = Vec::new();
    for fam in Fam::ALL {
        for e in 0..eras.len() as u8 {
            let l = lab::lab(&rules, e).unwrap();
            if !fam.claims(&rules, &l.kinds) {
                continue;
            }
            if !(0..suite::ATTEMPTS).any(|a| families::build(&rules, fam, e, 0, a).is_ok()) {
                missing.push(format!("{} {}", fam.name(), eras[e as usize]));
            }
        }
    }
    assert!(missing.is_empty(), "families that do not build in an era they claim: {missing:?}");
    // Every family claims at least one era.
    for fam in Fam::ALL {
        assert!((0..eras.len() as u8).any(|e| fam.claims(&rules, &lab::lab(&rules, e).unwrap().kinds)), "{} claims no era", fam.name());
    }
}

/// Slow tier: a sample of every family stays solvable: of instances 0-7 in the family's middle claimed era,
/// at least two are kept by the proof (a solver wins the attempt, the passive control does not).
#[test]
fn a_sample_of_every_family_stays_solvable() {
    let rules = ClassicBench::rules();
    let eras = lab::era_names(&rules);
    let mut weak = Vec::new();
    for fam in Fam::ALL {
        let claimed: Vec<u8> = (0..eras.len() as u8).filter(|e| fam.claims(&rules, &lab::lab(&rules, *e).unwrap().kinds)).collect();
        let e = claimed[claimed.len() / 2];
        let kept = (0..8).filter(|i| suite::prove(&rules, fam, e, *i, suite::ATTEMPTS).0.attempt.is_some()).count();
        if kept < 2 {
            weak.push(format!("{} {}: {kept}/8", fam.name(), eras[e as usize]));
        }
    }
    assert!(weak.is_empty(), "families whose sample is not winnable: {weak:?}");
}

const ASSAULT: [Fam; 5] = [Fam::AssaultHeal, Fam::PromotionChoice, Fam::HillArchers, Fam::CityFire, Fam::PlannedAssault];

/// The assault families (part 2): a scene is a function of its index, and the same player plays it to the same row.
#[test]
fn the_assault_scenes_are_deterministic() {
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "medieval").unwrap();
    for fam in ASSAULT {
        let (g1, s1) = families::build(&rules, fam, era, 1, 0).unwrap();
        let (g2, s2) = families::build(&rules, fam, era, 1, 0).unwrap();
        assert_eq!(tiles(&g1, &s1), tiles(&g2, &s2), "{}: the same index, the same scene", fam.name());
        assert_eq!(s1.info, s2.info);
        let a = play(&rules, g1, s1, "ref-planned", false).unwrap();
        let b = play(&rules, g2, s2, "ref-planned", false).unwrap();
        assert_eq!((a.win, a.rounds, &a.measures), (b.win, b.rounds, &b.measures), "{}", fam.name());
    }
}

/// The assault win rules: the city taken wins, too many of ours lost loses, the round limit loses.
#[test]
fn the_assault_win_rules_decide_what_they_say() {
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "medieval").unwrap();
    for fam in ASSAULT {
        let (game, mut sc) = families::build(&rules, fam, era, 0, 0).unwrap();
        assert_eq!(families::judge(&rules, &game, &mut sc.clone(), 1), None, "{}: the scene goes on", fam.name());
        assert_eq!(families::judge(&rules, &game, &mut sc.clone(), fam.rounds()), Some(false), "{}: the limit loses", fam.name());
        let mut g = game.clone();
        for p in sc.mine.clone() {
            g.debug_remove_unit(&rules, p.h);
        }
        assert_eq!(families::judge(&rules, &g, &mut sc.clone(), 1), Some(false), "{}: all lost", fam.name());
        sc.taken = Some(1);
        assert_eq!(families::judge(&rules, &game, &mut sc, 1), Some(true), "{}: the city taken wins", fam.name());
    }
}

/// The "at any cost" assault win rules: the city taken by the deadline wins whatever was lost, a capture after it
/// (the reinforcements landed, the peace) loses, the scene ends at N; N is drawn from the instance alone.
#[test]
fn the_any_cost_win_rules_decide_what_they_say() {
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "medieval").unwrap();
    for (fam, lag) in [(Fam::AssaultReinforce, 1), (Fam::AssaultPeace, 0)] {
        let (game, sc) = families::build(&rules, fam, era, 0, 0).unwrap();
        let (_, again) = families::build(&rules, fam, era, 0, 1).unwrap();
        assert_eq!(sc.rounds, again.rounds, "{}: N does not change with the attempt", fam.name());
        let n = sc.rounds;
        assert!(n <= fam.rounds() && n >= fam.rounds() - 2, "{}: N {n}", fam.name());
        assert_eq!(sc.deadline, Some(n - lag), "{}", fam.name());
        assert_eq!(sc.reinforce.is_empty(), fam == Fam::AssaultPeace, "{}", fam.name());
        assert_eq!(families::judge(&rules, &game, &mut sc.clone(), n), Some(false), "{}: the limit loses", fam.name());
        // All but one of ours lost: no loss cap.
        let mut g = game.clone();
        for p in sc.mine.iter().skip(1) {
            g.debug_remove_unit(&rules, p.h);
        }
        let mut won = sc.clone();
        won.taken = Some(n - lag);
        assert_eq!(families::judge(&rules, &g, &mut won, n - lag), Some(true), "{}: taken by the deadline wins", fam.name());
        let mut late = sc.clone();
        late.taken = Some(n - lag + 1);
        if lag > 0 {
            assert_eq!(families::judge(&rules, &g, &mut late, n), Some(false), "{}: taken after the deadline loses", fam.name());
        }
    }
}

/// The citadel stands on the holders' land (its damage zone hurts only units next to a citadel an enemy at war
/// owns); a citadel on neutral land damaged nobody.
#[test]
fn the_citadel_stands_on_the_holders_land() {
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "medieval").unwrap();
    for i in 0..4 {
        let (game, sc) = families::build(&rules, Fam::Citadel, era, i, 0).unwrap();
        let pass = sc.hold.unwrap();
        let map = game.view().auth.map();
        assert_eq!(map.owner(pass).flatten(), Some(sc.enemy), "instance {i}: the pass tile is the holders'");
        assert!(map.improvement(pass).flatten().is_some_and(|imp| rules.improvements[imp.0 as usize].nearby_enemy_damage > 0), "instance {i}: a citadel on the pass");
    }
}

// ---- The end of a unit's turn in a trainer (24 Nov: the army net's units never healed) ----

/// The hit points of `h` after its owner ends the turn, when the unit was wounded to 40 first and `act` was applied.
fn hp_after_turn_end(rules: &classic_rules::CompiledRuleset, game: &classic_rules::GameState, h: classic_rules::ids::UnitHandle, act: impl Fn(&mut classic_rules::GameState)) -> u16 {
    let mut g = game.clone();
    g.debug_set_unit_hp(h, 40);
    act(&mut g);
    let owner = g.view().auth.units().get(h).unwrap().core.owner;
    g.execute(rules, classic_rules::Command::EndTurn { player: owner }).expect("the owner ends its turn");
    g.view().auth.units().get(h).expect("the unit lives").core.hp
}

/// A unit that does not act heals what the rules give it, once, whoever ends its turn: the engine's own end of turn
/// (the control: nobody touches the unit), the seat's stand (the army net, the scripted references), the teacher's
/// rest. A unit that moved heals nothing, in every way of ending its turn. The old `stand` set `moves_left = 0`, so a
/// unit that did not act counted as moved and never healed.
#[test]
fn a_unit_that_does_not_act_heals_by_the_rules_once_and_a_unit_that_moved_does_not() {
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "medieval").unwrap();
    let (game, sc) = families::build(&rules, Fam::Duel, era, 0, 0).unwrap();
    let side = game.view().auth.active_player();
    let p = sc.mine.iter().chain(&sc.theirs).find(|p| act::core_of(&game, p.h).is_some_and(|c| c.owner == side)).expect("a unit of the active side").clone();
    let h = p.h;
    // The control: the engine's end of turn over an unmoved unit.
    let engine = hp_after_turn_end(&rules, &game, h, |_| {});
    assert!(engine > 40, "the rules heal a resting unit in this scene ({engine})");
    // (a) the seat's stand (the net's `Done`, the end of the army) gives exactly that, once.
    let stood = hp_after_turn_end(&rules, &game, h, |g| act::stand(g, &rules, h));
    assert_eq!(stood, engine, "stand: an unmoved unit heals by the rules");
    // Standing twice does not heal twice (the seat's end repeats the unit's own `Done`).
    let twice = hp_after_turn_end(&rules, &game, h, |g| {
        act::stand(g, &rules, h);
        act::stand(g, &rules, h);
    });
    assert_eq!(twice, engine, "stand twice: once");
    // (b) the teacher's rest.
    let rested = hp_after_turn_end(&rules, &game, h, |g| aurelia_qa::trainers::assault::rest(g, &rules, side, h));
    assert_eq!(rested, engine, "rest: an unmoved unit heals by the rules");
    // (c) a unit that moved is not healed by the end of its turn, and a stand after the move does not give it either.
    let start = act::core_of(&game, h).unwrap().tile;
    let step = lab::tiles_within(&game, start, 1).into_iter().find(|t| *t != start && game.clone().accepts_order(&rules, side, &classic_rules::Command::MoveUnit { unit: h, destination: *t })).expect("a free neighbour");
    let go = |g: &mut classic_rules::GameState| {
        g.execute(&rules, classic_rules::Command::MoveUnit { unit: h, destination: step }).expect("the step");
    };
    let moved = hp_after_turn_end(&rules, &game, h, go);
    assert_eq!(moved, 40, "a unit that moved does not heal");
    let moved_stood = hp_after_turn_end(&rules, &game, h, |g| {
        go(g);
        act::stand(g, &rules, h);
    });
    assert_eq!(moved_stood, 40, "a unit that moved and stands does not heal");
}

/// The class: a trainer ends a unit's turn only through `act::hold_unit` / `act::stand`. Setting the moves to 0 by
/// hand, or calling the harness hold directly, is how the army net's units lost their heal.
#[test]
fn no_trainer_ends_a_units_turn_outside_the_one_rule() {
    fn walk(dir: &std::path::Path, hits: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(&path, hits);
            } else if path.extension().is_some_and(|x| x == "rs") && !path.ends_with("trainers/act.rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for (i, line) in text.lines().enumerate() {
                    let code = line.split("//").next().unwrap_or("");
                    if code.contains("moves_left = 0") || code.contains("debug_hold_unit(") || code.contains("debug_set_unit_moves(") {
                        hits.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                    }
                }
            }
        }
    }
    let mut hits = Vec::new();
    walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut hits);
    assert!(hits.is_empty(), "a unit's turn ends through act::hold_unit only:\n{}", hits.join("\n"));
}
