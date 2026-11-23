//! The trainers (`aurelia_qa::trainers`, classic/docs/ai-trainers.md): a scene is a function of its index,
//! every family builds in every era it claims, the win rules decide what they say, and a sample of every
//! family stays winnable (a solver wins it, the passive control does not).

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::trainers::families::{self, Fam};
use aurelia_qa::trainers::{act, lab, play, suite};

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
