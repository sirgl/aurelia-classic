//! Synthetic battles (`aurelia_qa::battle`): the same spec gives the same
//! battle, and the game's AI fights it on both sides.

use aurelia_qa::battle::{new_battle, BattleSpec, Tally};
use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::spec::WorldSize;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use kernel::ai::seats::Seated;
use kernel::replay::hash::authoritative_hash;

/// Short games on a duel map: the test stays fast in a debug build.
const START: u32 = 40;
const ROUNDS: u32 = 15;

fn spec(seed: u64) -> BattleSpec {
    BattleSpec::new(seed, WorldSize::Duel).start_round(START).army(6)
}

#[test]
fn a_battle_is_deterministic() {
    let rules = ClassicBench::rules();
    let play = || {
        let mut battle = new_battle(&rules, &spec(1)).unwrap();
        let at_war = authoritative_hash(&battle.game);
        let mut driver = battle.driver(&rules, ClassicMind::new(Variant::Standard)).unwrap();
        let stats = battle.play(&rules, &mut driver, 3, |_| {}).unwrap();
        (at_war, stats)
    };
    let (first_war, first) = play();
    let (second_war, second) = play();
    assert_eq!(first_war, second_war, "the same spec sets up the same war");
    assert_eq!(first, second, "and the AI plays it the same way");
}

#[test]
fn both_sides_attack() {
    let rules = ClassicBench::rules();
    let mut attacks = [0, 0];
    for seed in 1..=3 {
        let mut battle = new_battle(&rules, &spec(seed)).unwrap();
        assert_eq!(battle.armies[0].units.len(), 6);
        let mut driver = battle.driver(&rules, ClassicMind::new(Variant::Standard)).unwrap();
        let stats = battle.play(&rules, &mut driver, ROUNDS, |_| {}).unwrap();
        eprintln!("seed {seed}: {stats:?}, damaged share {:.3}", stats.damaged_share());
        assert!(stats.attacks_on_enemy.iter().sum::<u32>() > 0, "seed {seed}: the armies fought");
        for side in 0..2 {
            attacks[side] += stats.attacks_on_enemy[side];
        }
    }
    assert!(attacks.iter().all(|a| *a > 0), "attacks on the enemy by side: {attacks:?}");
}

/// The bench's calibration controls (`ai::weak_fighter`): a `fight-passive`
/// side never strikes with its units (its cities still do), the standard side
/// does, and the kills between the sides are credited by the strikes
/// (`BattleStats::killed`): every kill a side is credited with is a kill of
/// one of its tallies.
#[test]
fn a_passive_control_never_strikes() {
    let rules = ClassicBench::rules();
    let mut battle = new_battle(&rules, &spec(1)).unwrap();
    let passive = vec![(battle.sides[0], ClassicMind::new(Variant::FightPassive))];
    let mut driver = battle.driver(&rules, Seated::new(ClassicMind::new(Variant::Standard), passive)).unwrap();
    let stats = battle.play(&rules, &mut driver, ROUNDS, |_| {}).unwrap();
    let strikes = |side: usize| [Tally::MeleeOnUnits, Tally::MeleeOnCities, Tally::RangedOnUnits, Tally::RangedOnCities].iter().map(|t| stats.audit[side].get(*t)).sum::<u32>();
    assert_eq!(strikes(0), 0, "the passive side's units never strike: {:?}", stats.audit[0]);
    assert!(strikes(1) > 0, "the standard side's units do: {:?}", stats.audit[1]);
    for side in 0..2 {
        let tallied: u32 = [Tally::KillsMelee, Tally::KillsRanged, Tally::KillsCity, Tally::KillsDefending].iter().map(|t| stats.audit[side].get(*t)).sum();
        assert_eq!(stats.killed[side], tallied, "side {side}: the kills credited are the kills tallied");
    }
}
