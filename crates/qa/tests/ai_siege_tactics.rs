//! Siege tactics (classic/docs/ai-siege-tactics.md): the switches the
//! variants carry, the range a unit's promotions give, the artillery scene of
//! the battle bench, and the units of a `siege-tactics-fortify` side that dig
//! in within the enemy's reach.

use aurelia_qa::battle::{new_battle, outranger_kind, BattleSpec, Tally};
use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::spec::WorldSize;
use classic_rules::ai::combat::FightFix;
use classic_rules::ai::danger::Source;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::siege_tactics::{outranges, unit_range, SiegeTactics};
use classic_rules::ai::variant::Variant;
use classic_rules::combat::city::CITY_ATTACK_RANGE;
use classic_rules::ids::{PromotionId, UnitKindId};
use classic_rules::rules::effects::{Modifier, UnitStat};
use classic_rules::world::unit::PromotionSet;
use kernel::ai::seats::Seated;

/// The game's AI carries no switch (its games stay byte-identical); the
/// bundle carries all four, each single-switch variant one, and the net
/// bundle fights as `fight-fix-net` with all four.
#[test]
fn only_the_siege_tactics_variants_carry_the_switches() {
    assert_eq!(Variant::Standard.siege_tactics(), SiegeTactics::NONE);
    assert_eq!(Variant::SiegeTactics.siege_tactics(), SiegeTactics::ALL);
    assert_eq!(Variant::SiegeTacticsNet.siege_tactics(), SiegeTactics::ALL);
    assert_eq!(Variant::SiegeTacticsNet.fight_fix(), FightFix::all(Source::Net));
    assert_eq!(Variant::SiegeTactics.fight_fix(), FightFix::NONE);
    let single = |s: SiegeTactics| [s.ground, s.firing, s.outrange, s.fortify].iter().filter(|b| **b).count();
    for v in [Variant::SiegeTacticsGround, Variant::SiegeTacticsFiring, Variant::SiegeTacticsOutrange, Variant::SiegeTacticsFortify] {
        assert_eq!(single(v.siege_tactics()), 1, "{} carries one switch", v.name());
        assert_eq!(v.fight_fix(), FightFix::NONE);
    }
    // Other variants may carry them too (`war-fix` takes `SiegeTactics::ALL`); the game's AI does not.
    assert!(Variant::ALL.iter().filter(|v| v.siege_tactics().any()).count() >= 6);
    assert!(!Variant::ALL[0].siege_tactics().any());
    for name in ["siege-tactics", "siege-tactics-ground", "siege-tactics-firing", "siege-tactics-outrange", "siege-tactics-fortify", "siege-tactics-net"] {
        assert!(Variant::from_name(name).is_some_and(|v| v.siege_tactics().any()), "{name}");
    }
}

/// A unit's range counts every change of range its promotions give, read
/// from the rules' data: each ranged land kind with each promotion that
/// changes the range.
#[test]
fn the_range_counts_the_promotions() {
    let rules = ClassicBench::rules();
    let ranging: Vec<(PromotionId, i32)> = rules
        .promotions
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let amount: i32 = p
                .effects
                .passive_unit
                .iter()
                .filter_map(|e| match &e.modifier {
                    Modifier::ModifyUnitStat { stat: UnitStat::Range, amount } => Some(*amount),
                    _ => None,
                })
                .sum();
            (amount != 0).then_some((PromotionId(i as u16), amount))
        })
        .collect();
    assert!(!ranging.is_empty(), "the classic rules have a promotion of range");
    let ranged: Vec<UnitKindId> = (0..rules.units.len()).map(|i| UnitKindId(i as u16)).filter(|k| classic_rules::ai::combat::ranged_range(&rules, *k).is_some()).collect();
    assert!(!ranged.is_empty());
    for kind in ranged {
        let base = classic_rules::ai::combat::ranged_range(&rules, kind).unwrap();
        assert_eq!(unit_range(&rules, kind, &PromotionSet::default()), Some(base));
        for (promotion, amount) in &ranging {
            let mut set = PromotionSet::default();
            set.insert(*promotion);
            assert_eq!(unit_range(&rules, kind, &set), Some((base + amount).max(1)));
        }
    }
    let melee = (0..rules.units.len()).map(|i| UnitKindId(i as u16)).find(|k| classic_rules::ai::combat::ranged_range(&rules, *k).is_none() && rules.units[k.0 as usize].strength > 0).unwrap();
    assert_eq!(unit_range(&rules, melee, &PromotionSet::default()), None, "a melee unit has no range");
}

/// The artillery scene: the outranger is a land unit whose range exceeds a
/// city's strike, and it takes the ranged slots of both armies.
#[test]
fn the_artillery_scene_sets_outrangers_in_both_armies() {
    let rules = ClassicBench::rules();
    let kind = outranger_kind(&rules).expect("the classic rules have a land unit that outranges a city");
    assert!(outranges(unit_range(&rules, kind, &PromotionSet::default())));
    assert!(unit_range(&rules, kind, &PromotionSet::default()).unwrap() > CITY_ATTACK_RANGE);
    let spec = BattleSpec::new(1, WorldSize::Duel).start_round(40).army(6).outrangers(2);
    let battle = new_battle(&rules, &spec).unwrap();
    for army in &battle.armies {
        assert_eq!(army.kinds.iter().filter(|k| **k == kind).count(), 2, "two outrangers in {:?}", army.kinds);
    }
}

/// A `siege-tactics-fortify` side digs in: of its units that stood within the
/// enemy's reach a turn, nearly all end it fortified, more than the game's
/// AI on the other side of the same battles.
#[test]
fn a_fortify_side_digs_in_within_reach() {
    let rules = ClassicBench::rules();
    let (mut stood, mut fortified) = ([0u32; 2], [0u32; 2]);
    for seed in 1..=2 {
        let spec = BattleSpec::new(seed, WorldSize::Duel).start_round(40).army(6);
        let mut battle = new_battle(&rules, &spec).unwrap();
        let mind = Seated::new(ClassicMind::new(Variant::Standard), vec![(battle.sides[0], ClassicMind::new(Variant::SiegeTacticsFortify))]);
        let mut driver = battle.driver(&rules, mind).unwrap();
        let stats = battle.play(&rules, &mut driver, 10, |_| {}).unwrap();
        for side in 0..2 {
            stood[side] += stats.audit[side].get(Tally::NearFoeStood);
            fortified[side] += stats.audit[side].get(Tally::NearFoeStoodFortified);
        }
    }
    eprintln!("stood {stood:?}, fortified {fortified:?}");
    assert!(stood[0] > 0, "the candidate's units stood within reach");
    assert!(fortified[0] * 100 >= stood[0] * 85, "the candidate fortified {} of {}", fortified[0], stood[0]);
    let share = |side: usize| f64::from(fortified[side]) / f64::from(stood[side].max(1));
    assert!(share(0) >= share(1), "the candidate fortifies at least as often as the game's AI: {:.2} against {:.2}", share(0), share(1));
}
