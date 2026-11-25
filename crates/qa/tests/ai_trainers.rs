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

// ---- The teachers on the true rules (24 Nov: the enemy's units heal now too) ----

/// The choke reference does not wait off the pass for the holders to be worn down: a holder that does not strike
/// heals each turn (the rules), so the wait only paid while the harness kept it hurt (the old `stand` never healed
/// the scripted enemy). On the sample (ancient and classical, instances 0-39, attempt 0, the scenes that build) the
/// reference wins at least nine tenths of what the plain fighter wins; with the wait it won half of that.
#[test]
fn the_choke_reference_does_not_wait_for_holders_that_heal() {
    let rules = ClassicBench::rules();
    let (mut choke, mut fight, mut n) = (0, 0, 0);
    for era in ["ancient", "classical"] {
        let e = lab::era_index(&rules, era).unwrap();
        for i in 0..40 {
            let Ok((game, sc)) = families::build(&rules, Fam::Choke, e, i, 0) else { continue };
            n += 1;
            choke += usize::from(play(&rules, game.clone(), sc.clone(), "ref-choke", false).unwrap().win);
            fight += usize::from(play(&rules, game, sc, "ref-fight", false).unwrap().win);
        }
    }
    assert!(n >= 40, "the sample builds ({n})");
    assert!(choke * 10 >= fight * 9, "ref-choke wins {choke} of {n}, ref-fight {fight}: the reference waits again");
}

/// The scene rule is the trainers' world: on when the switch is unset or `simple`, off only for `standard`, and a
/// misspelt value stops the run instead of playing the other world.
#[test]
fn the_scene_rule_is_the_trainers_default_and_standard_is_the_only_way_off() {
    use aurelia_qa::trainers::scene_rule::rule_of;
    assert!(rule_of(None), "unset: the rule");
    assert!(rule_of(Some("simple")));
    assert!(!rule_of(Some("standard")));
    assert!(std::panic::catch_unwind(|| rule_of(Some("simpel"))).is_err(), "an unknown value stops the run");
}

/// The scene rule's version: 2 (unset) plays the learner's civilians, 1 is the rule of before; a misspelt value stops the
/// run; and the civilians' pass of version 2 walks a convoy's settler (the world where nobody moves it is version 1).
#[test]
fn the_scene_rule_version_is_two_by_default_and_one_is_the_old_rule() {
    use aurelia_qa::trainers::scene_rule::version_of;
    assert_eq!(version_of(None), 2, "unset: version 2");
    assert_eq!(version_of(Some("1")), 1);
    assert_eq!(version_of(Some("2")), 2);
    for bad in ["3", "", "two", "0"] {
        assert!(std::panic::catch_unwind(|| version_of(Some(bad))).is_err(), "{bad:?} stops the run");
    }
    let rules = ClassicBench::rules();
    let era = (0..8u8).find(|e| families::build(&rules, Fam::Convoy, *e, 0, 0).is_ok()).expect("a convoy scene builds");
    let (mut game, sc) = families::build(&rules, Fam::Convoy, era, 0, 0).unwrap();
    let settler = sc.mine.iter().find(|p| p.role == "civilian").unwrap().clone();
    let before = act::alive(&game, &settler, sc.learner).unwrap().tile;
    families::civilians_turn(&rules, &mut game, &sc);
    let after = act::alive(&game, &settler, sc.learner).map(|c| c.tile);
    assert_ne!(after, Some(before), "the civilians' pass walks the settler (or founds the city)");
}

/// The camp rules the camp trainers stand on (`trainers::camp`, its module doc): a camp is cleared by a unit that ENTERS its
/// tile (killing the keeper does not clear it), the neutral nation at peace with the learner cannot be struck by it and cannot
/// stop on a tile of ours, the rules' record says who cleared, and the win rules read that record.
#[test]
fn the_camp_is_cleared_by_entering_and_the_neutral_is_at_peace() {
    use aurelia_qa::trainers::camp;
    use classic_rules::Command;
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "medieval").unwrap();
    let (mut g, mut sc) = families::build(&rules, Fam::CampRace, era, 5, 0).unwrap();
    let (me, n) = (sc.learner, sc.neutral.expect("a neutral"));
    assert!(!g.view().auth.diplomacy().at_war(me, n), "the neutral is at peace with the learner");
    assert!(g.view().auth.diplomacy().at_war(me, sc.enemy) && g.view().auth.diplomacy().at_war(n, sc.enemy), "both are at war with the barbarians");
    assert!(camp::standing(&g, &sc) && camp::cleared_by(&g, &sc).is_none());
    let camp_t = camp::camp_tile(&sc);
    // Only the keeper is left, at 1 hit point; our scene units and the neutral's are replaced by one melee unit each, ours beside the camp.
    let keeper = sc.theirs.iter().find(|p| act::core_of(&g, p.h).is_some_and(|c| c.tile == camp_t)).cloned().expect("a keeper on the camp");
    for p in sc.theirs.iter().chain(&sc.mine).chain(&sc.third).cloned().collect::<Vec<_>>() {
        if p.id != keeper.id {
            g.debug_remove_unit(&rules, p.h);
        }
    }
    g.debug_set_unit_hp(keeper.h, 1);
    let melee = |s: &aurelia_qa::trainers::Scene| s.mine.iter().find(|p| act::range_of(&rules, p.kind).is_none()).map(|p| p.kind).unwrap();
    let kind = melee(&sc);
    let free: Vec<_> = lab::tiles_within(&g, camp_t, 2).into_iter().filter(|t| *t != camp_t && g.view().auth.indexes().units_on(*t).is_empty() && g.view().auth.indexes().city_on_tile(*t).is_none()).collect();
    let beside = *free.iter().find(|t| lab::dist(&g, **t, camp_t) == 1).expect("a free tile beside the camp");
    let ours = g.debug_spawn_unit(&rules, me, beside, kind);
    let next = *free.iter().find(|t| **t != beside && lab::dist(&g, **t, beside) == 1 && lab::dist(&g, **t, camp_t) >= 1).expect("a tile beside ours");
    let theirs = g.debug_spawn_unit(&rules, n, next, kind);
    assert!(!act::try_cmd(&mut g, &rules, me, Command::AttackUnit { attacker: ours, defender: theirs }), "peace: a strike at the neutral is refused");
    assert!(classic_rules::world::stacking::can_stop_at(g.view().auth, &rules, n, kind, None, beside).is_err(), "the neutral's unit cannot stop on a tile of ours");
    assert!(act::try_cmd(&mut g, &rules, me, Command::AttackUnit { attacker: ours, defender: keeper.h }), "the keeper is struck");
    assert!(act::alive(&g, &keeper, sc.enemy).is_none(), "the keeper (1 hp) is dead");
    // A melee kill of the last defender advances into the tile (rules 2.3) and so clears the camp with the blow.
    assert_eq!(act::core_of(&g, ours).map(|c| c.tile), Some(camp_t), "the melee killer advanced onto the camp");
    assert_eq!(camp::cleared_by(&g, &sc).map(|c| c.0), Some(me));
    assert!(!camp::standing(&g, &sc));
    sc.mine.clear();
    sc.mine.push(aurelia_qa::trainers::Piece { h: ours, id: act::core_of(&g, ours).unwrap().id, kind, role: "melee" });
    assert_eq!(families::judge(&rules, &g, &mut sc, 1), Some(true));
    // A RANGED kill leaves the camp standing and empty: it is cleared only when a unit walks onto the tile.
    let (mut g, mut sc) = families::build(&rules, Fam::Camp, era, 5, 0).unwrap();
    let keeper = sc.theirs.iter().find(|p| act::core_of(&g, p.h).is_some_and(|c| c.tile == camp_t)).cloned().expect("a keeper on the camp");
    for p in sc.theirs.iter().chain(&sc.mine).cloned().collect::<Vec<_>>() {
        if p.id != keeper.id {
            g.debug_remove_unit(&rules, p.h);
        }
    }
    g.debug_set_unit_hp(keeper.h, 1);
    let archer = lab::EraKinds::of(&rules, era).by_arm.get(&lab::Arm::Ranged).and_then(|v| v.first().copied()).unwrap();
    let range = act::range_of(&rules, archer).unwrap() as i32;
    let at = *lab::tiles_within(&g, camp_t, range).iter().find(|t| lab::dist(&g, **t, camp_t) == range && g.view().auth.indexes().units_on(**t).is_empty()).unwrap();
    let shooter = g.debug_spawn_unit(&rules, me, at, archer);
    sc.mine = vec![aurelia_qa::trainers::Piece { h: shooter, id: act::core_of(&g, shooter).unwrap().id, kind: archer, role: "ranged" }];
    assert!(act::try_cmd(&mut g, &rules, me, Command::RangedAttack { unit: shooter, target: camp_t }), "the keeper is shot");
    assert!(act::alive(&g, &keeper, sc.enemy).is_none());
    assert!(camp::standing(&g, &sc) && camp::cleared_by(&g, &sc).is_none(), "a ranged kill leaves the camp standing");
    assert_eq!(families::judge(&rules, &g, &mut sc.clone(), 1), None, "an empty camp is not a cleared one");
    let walker = g.debug_spawn_unit(&rules, me, beside, kind);
    g.debug_set_unit_moves(walker, 2);
    assert!(act::try_cmd(&mut g, &rules, me, Command::MoveUnit { unit: walker, destination: camp_t }), "a unit walks onto the empty camp");
    assert_eq!(camp::cleared_by(&g, &sc).map(|c| c.0), Some(me));
    sc.mine.clear();
    sc.mine.push(aurelia_qa::trainers::Piece { h: walker, id: act::core_of(&g, walker).unwrap().id, kind, role: "melee" });
    assert_eq!(families::judge(&rules, &g, &mut sc, 1), Some(true));
}

#[test]
fn the_camp_families_are_proven_and_the_neutral_clearing_loses() {
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "ancient").unwrap();
    // A sample of both families: a teacher wins at least one of six, the passive control never does.
    for fam in [Fam::Camp, Fam::CampRace] {
        let won = (0..6usize).any(|i| {
            let Ok((g, sc)) = families::build(&rules, fam, era, i, 0) else { return false };
            fam.solvers().iter().any(|s| play(&rules, g.clone(), sc.clone(), s, false).is_ok_and(|r| r.win))
        });
        assert!(won, "{}: a teacher wins one of six", fam.name());
        let mut by_neutral = 0.0;
        for i in 0..6usize {
            let (g, sc) = families::build(&rules, fam, era, i, 0).unwrap();
            let row = play(&rules, g, sc, "passive", false).unwrap();
            assert!(!row.win, "{} #{i}: the passive control does not win", fam.name());
            by_neutral += row.measures["camp_lost_to_neutral"];
        }
        // The neutral clears the camp the passive learner leaves (a clearing by the neutral is a loss).
        assert_eq!(by_neutral > 0.0, fam == Fam::CampRace, "{}: the neutral takes the camp from a passive learner", fam.name());
    }
}

/// The third-party trainers (`trainers::third_party`, its module doc): the third nation is at peace with the learner, the enemy at
/// war with it, and the scenes of the seven families build in every era sampled. A teacher wins one of six of each family and the
/// passive control never wins one (defend-friend excepted: the ally's own garrison holds some scenes alone).
#[test]
fn the_third_party_families_build_and_the_teachers_beat_the_passive_control() {
    use aurelia_qa::trainers::third_party;
    let rules = ClassicBench::rules();
    for era_name in ["ancient", "industrial"] {
        let era = lab::era_index(&rules, era_name).unwrap();
        for fam in Fam::ALL.iter().copied().filter(|f| third_party::is_third(*f)) {
            let mut built = 0;
            let mut won = 0;
            let mut passive_won = 0;
            for i in 0..6usize {
                let Ok((g, sc)) = families::build(&rules, fam, era, i, 0) else { continue };
                built += 1;
                let (me, n) = (sc.learner, sc.neutral.expect("a third nation"));
                assert!(g.view().auth.diplomacy().at_war(me, sc.enemy), "{}: the learner is at war with the enemy", fam.name());
                if !matches!(fam, Fam::NeutralJoinsWar) {
                    assert!(!g.view().auth.diplomacy().at_war(me, n), "{}: the third nation is at peace with the learner at the start", fam.name());
                }
                if fam.solvers().iter().take(1).any(|s| play(&rules, g.clone(), sc.clone(), s, false).is_ok_and(|r| r.win)) {
                    won += 1;
                }
                if play(&rules, g, sc, "passive", false).is_ok_and(|r| r.win) {
                    passive_won += 1;
                }
            }
            assert!(built >= 4, "{} {era_name}: the scene builds ({built} of 6)", fam.name());
            assert!(won >= 1, "{} {era_name}: the right teacher wins one of six", fam.name());
            let allowed = if fam == Fam::DefendFriend { 2 } else { 0 };
            assert!(passive_won <= allowed, "{} {era_name}: the passive control wins {passive_won}", fam.name());
        }
    }
}

/// The net's blindness is a fact the third-party trainers rest on: the foe set of the third families holds the enemy and,
/// where the third nation is at war with the learner, nothing of the third (`orders.foe` is the enemy's pieces only).
#[test]
fn the_third_nation_is_no_foe_token_until_it_is_at_war() {
    use aurelia_qa::trainers::scene_rule::foe_tiles;
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "medieval").unwrap();
    let (g, sc) = families::build(&rules, Fam::NeutralPath, era, 2, 0).unwrap();
    let foes = foe_tiles(&g, &sc, sc.learner);
    let third: Vec<_> = sc.third.iter().filter_map(|p| act::core_of(&g, p.h)).map(|c| c.tile).collect();
    assert!(!third.is_empty() && third.iter().all(|t| !foes.contains(t)), "the neutral's tiles are no foe tiles");
}

/// Every family's seed id, frozen: a kept file and the gates of the integrated lanes name scenes by seed, so no id may move.
const SEED_IDS: [(&str, u64); 60] = [
    ("duel", 0),
    ("focus", 1),
    ("hold", 2),
    ("choke", 3),
    ("bait", 4),
    ("kite", 5),
    ("guard", 6),
    ("convoy", 7),
    ("raid", 8),
    ("counter", 9),
    ("siege", 10),
    ("ram", 11),
    ("rush", 12),
    ("retreat", 13),
    ("flank", 14),
    ("general", 15),
    ("citadel", 16),
    ("bombard", 17),
    ("sink", 18),
    ("airstrike", 19),
    ("prey", 20),
    ("wounded", 21),
    ("harass", 22),
    ("snipe", 23),
    ("surrounded", 24),
    ("assault-heal", 25),
    ("promotion-choice", 26),
    ("hill-archers", 27),
    ("city-fire", 28),
    ("planned-assault", 29),
    ("ambush-fog", 30),
    ("hunt-fog", 31),
    ("assault-reinforce", 32),
    ("assault-peace", 33),
    ("camp", 34),
    ("camp-race", 35),
    ("neutral-path", 36),
    ("ally-siege", 37),
    ("defend-friend", 38),
    ("closed-borders", 39),
    ("spare-friends", 40),
    ("neutral-joins-war", 41),
    ("capital-race", 42),
    ("naval-duel", 34),
    ("landing-escort", 35),
    ("air-intercept", 36),
    ("air-recon", 37),
    ("feint", 34),
    ("fog-sniper", 35),
    ("city-reinforced", 36),
    ("scout-report", 37),
    ("two-fronts", 38),
    ("second-city", 39),
    ("twin-siege", 40),
    ("air-threat", 41),
    ("surprise-attack", 42),
    ("naval-approach", 43),
    ("blind-sector", 44),
    ("second-front", 45),
    ("bait-look", 46),
];

#[test]
fn seed_ids_are_frozen() {
    assert_eq!(SEED_IDS.len(), Fam::ALL.len(), "a family was added or removed: add it to SEED_IDS with an id no family uses");
    for fam in Fam::ALL {
        let frozen = SEED_IDS.iter().find(|(n, _)| *n == fam.name()).unwrap_or_else(|| panic!("{} is not in SEED_IDS: add it with an id no family uses", fam.name()));
        assert_eq!(fam.seed_id(), frozen.1, "{}: its seed id moved; every scene of the family would be redrawn and the kept files invalid", fam.name());
    }
}

/// The nine ids that several families share (the lanes were merged after each had appended families; renumbering would
/// invalidate their kept files). The set is closed: a new family must take an id that no family uses.
#[test]
fn only_the_nine_known_groups_share_a_seed_id() {
    let known: [(u64, &[&str]); 9] = [
        (34, &["camp", "naval-duel", "feint"]),
        (35, &["camp-race", "landing-escort", "fog-sniper"]),
        (36, &["neutral-path", "air-intercept", "city-reinforced"]),
        (37, &["ally-siege", "air-recon", "scout-report"]),
        (38, &["defend-friend", "two-fronts"]),
        (39, &["closed-borders", "second-city"]),
        (40, &["spare-friends", "twin-siege"]),
        (41, &["neutral-joins-war", "air-threat"]),
        (42, &["capital-race", "surprise-attack"]),
    ];
    let mut by_id: std::collections::BTreeMap<u64, Vec<&str>> = std::collections::BTreeMap::new();
    for fam in Fam::ALL {
        by_id.entry(fam.seed_id()).or_default().push(fam.name());
    }
    let shared: Vec<(u64, Vec<&str>)> = by_id.into_iter().filter(|(_, v)| v.len() > 1).collect();
    let expected: Vec<(u64, Vec<&str>)> = known.iter().map(|(i, v)| (*i, v.to_vec())).collect();
    assert_eq!(shared, expected, "a family shares a seed id it should not: pick an id that no family uses (see Fam::seed_id)");
}

/// fog-sniper claims exactly the eras its scene builds in (the builder refuses where the kinds see over the ridge), so the
/// claim cannot drift from the builder in either direction.
#[test]
fn fog_sniper_claims_exactly_the_eras_it_builds() {
    let rules = ClassicBench::rules();
    let eras = lab::era_names(&rules);
    for e in 0..eras.len() as u8 {
        let l = lab::lab(&rules, e).unwrap();
        let builds = (0..suite::ATTEMPTS).any(|a| families::build(&rules, Fam::FogSniper, e, 0, a).is_ok());
        let claims = Fam::FogSniper.claims(&rules, &l.kinds);
        // An era without the land arms is not claimed and not built either.
        assert_eq!(claims, builds, "fog-sniper in {}: claims {claims}, builds {builds}", eras[e as usize]);
    }
}

/// The class guard of the rule version. A row, a record or a kept set of a family whose scene depends on the rule
/// version is comparable only inside one version, so the list of those families (`rule_dependent.json`, read by the
/// Rust tools and by the Python gate tools) must be exactly the families that differ: every family is rebuilt under both
/// versions (the era claims, the scene, its pieces and info) and a family with a learner's civilian is listed too (the
/// rule's civilians pass plays it). A new dependence of another family on the version fails here until the list names it.
#[test]
fn the_families_that_depend_on_the_rule_version_are_the_listed_ones() {
    use aurelia_qa::trainers::scene_rule::{dependent_families, with_version};
    let rules = ClassicBench::rules();
    let eras = lab::era_names(&rules).len() as u8;
    let digest = |v: u8, fam: Fam, e: u8, i: usize, a: u32| {
        with_version(v, || {
            let claims = lab::lab(&rules, e).map(|l| fam.claims(&rules, &l.kinds)).unwrap_or(false);
            match families::build(&rules, fam, e, i, a) {
                Ok((g, s)) => (format!("{claims} {:?} {:?} {:?} {:?} {:?}", tiles(&g, &s), s.refuge, s.rounds, s.info, s.aim_learner), s.mine.iter().any(|p| p.role == "civilian")),
                Err(err) => (format!("{claims} err {err}"), false),
            }
        })
    };
    let mut found: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for fam in Fam::ALL {
        for e in 0..eras {
            for i in 0..2 {
                for a in 0..2 {
                    let (d1, civ1) = digest(1, fam, e, i, a);
                    let (d2, civ2) = digest(2, fam, e, i, a);
                    if d1 != d2 || civ1 || civ2 {
                        found.insert(fam.name().to_owned());
                    }
                }
            }
        }
    }
    let mut listed = dependent_families();
    listed.sort();
    assert_eq!(found.into_iter().collect::<Vec<_>>(), listed, "rule_dependent.json is the families whose scene differs between rule versions 1 and 2 (or has a learner's civilian)");
}

/// A kept set is played only under the version it was proven under, for the dependent families; an old file (no
/// `rule_version`) needs the version named by `--kept-version`; the other families do not care.
#[test]
fn a_kept_set_of_another_rule_version_is_refused() {
    let k = |family: &str, v: Option<u8>| suite::Kept { family: family.to_owned(), era: "medieval".into(), instance: 0, attempt: Some(0), solver: None, tried: 1, passive_won: 0, unbuilt: 0, rule_version: v };
    let playing = ["guard", "convoy", "duel"];
    assert!(suite::check_kept(&[k("guard", Some(2)), k("convoy", Some(2))], &playing, 2, None).is_ok());
    let wrong = suite::check_kept(&[k("guard", Some(1))], &playing, 2, None).unwrap_err();
    assert!(wrong.contains("version 1") && wrong.contains("version 2"), "{wrong}");
    let none = suite::check_kept(&[k("convoy", None)], &playing, 2, None).unwrap_err();
    assert!(none.contains("--kept-version"), "{none}");
    assert!(suite::check_kept(&[k("convoy", None)], &playing, 2, Some(2)).is_ok(), "kept-r2 with the version named");
    assert!(suite::check_kept(&[k("convoy", None)], &playing, 2, Some(1)).is_err(), "kept-r named as 1, played under 2");
    assert!(suite::check_kept(&[k("convoy", None)], &playing, 1, Some(1)).is_ok(), "kept-r under ARMY_SCENE_RULE_VERSION=1");
    assert!(suite::check_kept(&[k("duel", None), k("guard", Some(2))], &playing, 2, None).is_ok(), "a family that does not depend on the version needs no marker");
    assert!(suite::check_kept(&[k("guard", Some(1))], &["duel"], 2, None).is_ok(), "a family not played is not checked");
    // The kept set's identity is a function of its bytes.
    assert_eq!(suite::file_id("a\n"), suite::file_id("a\n"));
    assert_ne!(suite::file_id("a\n"), suite::file_id("b\n"));
}
