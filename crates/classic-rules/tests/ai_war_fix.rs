//! `war-fix` (classic/docs/ai-war-fix.md): the tactics of `fight-fix-net` with the war fixes of
//! `siege`, the capture chance learned from outcomes, a captured city priced by the rules and a
//! siege's shots worth their city. The truth is read only to set the scene.

use classic_content::core::core_rules;
use classic_rules::ai::combat::FightFix;
use classic_rules::ai::danger::Source;
use classic_rules::ai::operations::{captured_parts, city_worth, learned_chance, logistic_percent, SiegeFixes, CHANCE_FEATURES};
use classic_rules::ai::value::{PriceModel, Prices, SciencePricing};
use classic_rules::ai::variant::Variant;
use classic_rules::ids::{PlayerId, TerrainId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, GameConfig, GameState};

#[test]
fn the_war_fix_variants_carry_the_fixes_they_name() {
    let siege = |v: Variant| v.war_tuning().siege;
    assert_eq!(siege(Variant::WarFix), SiegeFixes::WAR_FIX);
    assert_eq!(siege(Variant::WarFixPlain), SiegeFixes::ALL, "the combination alone: the `siege` five");
    assert_eq!(siege(Variant::WarFixModel), SiegeFixes { chance: true, learned: false, ..SiegeFixes::WAR_FIX });
    assert_eq!(siege(Variant::WarFixUnpriced), SiegeFixes { priced: false, ..SiegeFixes::WAR_FIX });
    assert_eq!(siege(Variant::WarFixShots), SiegeFixes { worth: false, ..SiegeFixes::WAR_FIX });
    let siege_five = SiegeFixes { learned: false, priced: false, worth: false, asset: false, chance: true, ..SiegeFixes::WAR_FIX };
    assert_eq!(siege_five, SiegeFixes::ALL, "`war-fix` keeps four of the `siege` five and learns the fifth");
    for v in [Variant::WarFix, Variant::WarFixPlain, Variant::WarFixModel, Variant::WarFixUnpriced, Variant::WarFixShots] {
        assert_eq!(v.fight_fix(), FightFix::all(Source::Net), "{} fights as `fight-fix-net`", v.name());
        assert_eq!(Variant::from_name(v.name()), Some(v));
        let mut standard = v.war_tuning();
        assert_eq!(standard.recall, v != Variant::WarFixPlain, "{}: the recall of `warfix-recall` but in the plain combination", v.name());
        standard.siege = SiegeFixes::NONE;
        standard.recall = false;
        assert_eq!(standard, Variant::PreWarFix.war_tuning(), "{} is `pre-war-fix` but for its war fixes", v.name());
    }
    assert_eq!(Variant::WarFixOps.war_tuning(), Variant::WarFix.war_tuning(), "`war-fix-ops` wars as `war-fix`");
    assert_eq!(Variant::WarFixOps.fight_fix(), FightFix::NONE, "and fights as `standard`");
    assert_eq!(Variant::WarFixTactics.war_tuning(), Variant::WarFix.war_tuning());
    assert_eq!(Variant::WarFixTactics.fight_fix(), FightFix::all(Source::Net));
    assert!(Variant::WarFixTactics.siege_tactics().any() && !Variant::WarFix.siege_tactics().any());
    assert_eq!(siege(Variant::PreWarFix), SiegeFixes::NONE, "the standard before the promotion plays none of them");
    assert_eq!(Variant::PreWarFix.fight_fix(), FightFix::NONE);
    // Promoted (1 November): the game's AI wars and fights as `war-fix`.
    assert_eq!(Variant::Standard.war_tuning(), Variant::WarFix.war_tuning());
    assert_eq!(Variant::Standard.fight_fix(), Variant::WarFix.fight_fix());
    assert_eq!(Variant::from_name("pre-war-fix"), Some(Variant::PreWarFix));
}

/// The integer logistic: one half at zero, nothing and everything far out, never falling.
#[test]
fn the_learned_chance_is_an_integer_logistic() {
    assert_eq!(logistic_percent(0), 50);
    assert_eq!(logistic_percent(-9000), 0);
    assert_eq!(logistic_percent(9000), 100);
    let mut last = 0;
    for logit in (-9000..=9000).step_by(37) {
        let p = logistic_percent(logit);
        assert!(p >= last, "monotone at {logit}");
        last = p;
    }
    assert_eq!(CHANCE_FEATURES.len(), 12);
}

/// The fitted chance is what the games deliver, not what the siege model promises: about one
/// operation in nine sees its enemy lose a city to the player in its war (the model gave 86 %),
/// more with a larger army, less against a larger or farther city, an enemy that threatens our
/// cities, or when the war runs already.
#[test]
fn the_learned_chance_is_what_the_games_deliver() {
    let base: [i64; 12] = [60, 86, 8, 4, 10, 0, 18, 4, 0, 10, 6, 7];
    let chance = learned_chance(&base);
    assert!((5..=30).contains(&chance), "{chance} %");
    let changed = |at: usize, to: i64| {
        let mut f = base;
        f[at] = to;
        learned_chance(&f)
    };
    assert!(changed(10, 10) > chance, "a larger army");
    assert!(changed(3, 8) < chance, "a larger city");
    assert!(changed(2, 14) < chance, "a farther city");
    assert!(changed(9, 100) < chance, "an enemy that threatens our cities");
    assert!(changed(5, 1) < chance, "a war that runs already");
}

/// The prices of a player with one city of three on open land.
fn prices() -> Prices {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 24, height: 24, rng_seed: 5 });
    let land = rules
        .terrains
        .iter()
        .filter(|t| !t.water && t.can_found_city && !t.impassable)
        .filter_map(|t| t.map_code)
        .find(|code| !rules.terrain_kinds.is_hills(*code))
        .expect("a flat land terrain");
    let all: Vec<_> = (0..24 * 24).map(|i| (TileId(i), TerrainId(land))).collect();
    game.debug_set_terrain_batch(&rules, &all);
    let centre = game.view().auth.map().dimensions.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), centre);
    game.debug_set_population(&rules, city, 3);
    let obs = game.ai_observation(&rules, PlayerId(0));
    Prices::of_effects(&obs, &rules, PriceModel::Uses, SciencePricing::Reach, true)
}

/// A captured city by the rules: a small one becomes a puppet with no resistance and yields at
/// once; a large one is annexed and yields only after its resistance (as many rounds as the
/// citizens the capture leaves it). In a deficit the empire lives in, its unrest costs the drift
/// of its points, not every point at the price of the first step out of the deficit
/// (`Prices::happiness`), which made it 2.23 times the city.
#[test]
fn a_captured_city_is_priced_by_what_it_changes() {
    let mut prices = prices();
    prices.excess = -4;
    prices.drift = prices.score.citizen / 4;
    prices.happiness = prices.score.citizen * 8;
    let rounds = 10;
    let (puppet, puppet_unrest) = captured_parts(&prices, 2, rounds, false);
    assert_eq!(puppet, city_worth(&prices, 1, rounds), "a city of two keeps one citizen and yields at once");
    let (annexed, annexed_unrest) = captured_parts(&prices, 8, rounds, false);
    assert_eq!(annexed, city_worth(&prices, 5, rounds + 5), "a city of eight keeps five, annexed after five rounds of resistance");
    assert!(annexed < city_worth(&prices, 5, rounds));
    assert!(puppet_unrest < prices.captured_city_unrest_flat(1), "{puppet_unrest} against the flat {}", prices.captured_city_unrest_flat(1));
    // Priced as a puppet (`SiegeFixes::puppet`, `puppet-war`): the same city yields at once, with a
    // normal city's unrest; a small city is a puppet either way.
    let (as_puppet, as_puppet_unrest) = captured_parts(&prices, 8, rounds, true);
    assert_eq!(as_puppet, city_worth(&prices, 5, rounds), "a puppet of five yields at once");
    assert!(as_puppet_unrest < annexed_unrest, "{as_puppet_unrest} against the annexed {annexed_unrest}");
    assert_eq!(captured_parts(&prices, 2, rounds, true), (puppet, puppet_unrest));
    // Crossing the very-unhappy line costs its step.
    prices.excess = -9;
    let (_, crossing) = captured_parts(&prices, 2, rounds, false);
    assert!(crossing > puppet_unrest, "the line's step is priced");
}
