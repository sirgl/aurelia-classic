//! The three fixes of the war audit (classic/docs/ai-war-audit-current.md), each behind its own variant:
//! the march limit agrees with the check that admits an operation, the standing garrisons of quiet
//! cities count as free units for a march, and the variants carry exactly the changes they name.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::operations::{march_limit, MARCH_LIMIT};
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::planner_strategy::WAR_WEARINESS_ROUNDS;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::war_probe;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{PlayerId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;

const AI: PlayerId = PlayerId(0);
const FOE: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

#[test]
fn the_variants_carry_the_changes_they_name() {
    let tuning = |v: Variant| v.war_tuning();
    // The standard before the war fix: the march limit only.
    let standard = tuning(Variant::PreWarFix);
    assert!(standard.march && !standard.unrest && !standard.group && !standard.raids);
    assert_eq!(tuning(Variant::Previous), standard, "the bench instruments built on standard play its fixes");
    assert_eq!(tuning(Variant::Unfixed), Default::default());
    assert_eq!(tuning(Variant::Direct), Default::default());
    assert!(tuning(Variant::WarfixMarch).march && !tuning(Variant::WarfixMarch).unrest && !tuning(Variant::WarfixMarch).group);
    assert!(tuning(Variant::WarfixUnrest).unrest && !tuning(Variant::WarfixUnrest).march && !tuning(Variant::WarfixUnrest).group);
    assert!(tuning(Variant::WarfixGroup).group && !tuning(Variant::WarfixGroup).march && !tuning(Variant::WarfixGroup).unrest);
    let recall = tuning(Variant::WarfixRecall);
    assert!(recall.recall && recall.march && !recall.unrest && !recall.group && !recall.raids);
    assert!(!standard.recall);
    let raids = tuning(Variant::Raids);
    assert!(raids.march && raids.unrest && !raids.group && raids.raids);
    assert_eq!(Variant::from_name("raids"), Some(Variant::Raids));
    assert_eq!(Variant::from_name("unfixed"), Some(Variant::Unfixed));
    assert_eq!(Variant::from_name("warfix-recall"), Some(Variant::WarfixRecall));
    for v in Variant::ALL.into_iter().filter(|v| v.compiled()) {
        assert_eq!(Variant::from_name(v.name()), Some(v), "every variant is listed once under its own name");
    }
}

/// The march of an operation ends when the check that admitted it said it would: with the fix,
/// at `WAR_WEARINESS_ROUNDS / 2`; without it, at the shorter `MARCH_LIMIT`.
#[test]
fn the_march_limit_agrees_with_the_check_that_admits_an_operation() {
    let rules = compile_rules(core_rules()).unwrap();
    let game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 20, rng_seed: 3 });
    let obs = game.ai_observation(&rules, AI);
    let mut brief = TurnBrief::from_observation(&obs, &rules);
    brief.war = Variant::Unfixed.war_tuning();
    assert_eq!(march_limit(&brief), MARCH_LIMIT);
    const { assert!(MARCH_LIMIT < WAR_WEARINESS_ROUNDS / 2, "the old limit ended operations the check had admitted") };
    brief.war = Variant::Standard.war_tuning();
    assert_eq!(march_limit(&brief), WAR_WEARINESS_ROUNDS / 2);
}

/// Six quiet cities, each with its garrison and a rival's city in reach: the free troops at the
/// looks for an operation count the lent garrisons (two of six) under `warfix-group`, and only the
/// units without a task under `standard`.
fn free_troops(variant: Variant) -> f64 {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 20, rng_seed: 5 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(1))).collect();
    game.debug_set_terrain_batch(&rules, &land);
    for (q, r) in [(4, 4), (4, 9), (4, 14), (9, 4), (9, 9), (9, 14)] {
        let t = tile(&game, q, r);
        let city = game.debug_found_city(&rules, AI, t);
        game.debug_set_population(&rules, city, 4);
        let warrior = kind(&rules, "core:unit/warrior");
        game.debug_spawn_unit(&rules, AI, t, warrior);
    }
    let foe_capital = tile(&game, 36, 3);
    game.debug_found_city(&rules, FOE, foe_capital);
    let foe_city = tile(&game, 18, 9);
    game.debug_found_city(&rules, FOE, foe_city);
    let everything: Vec<TileId> = (0..tiles).map(TileId).collect();
    game.debug_reveal(AI, &everything);
    game.debug_meet(AI, FOE);
    game.debug_meet(FOE, AI);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, vec![FOE], ClassicMind::new(variant));
    war_probe::enable(true);
    while game.view().auth.turn().round < 50 {
        let active = game.view().auth.active_player();
        if active == FOE {
            game.execute(&rules, Command::EndTurn { player: FOE }).unwrap();
        } else {
            let round = game.view().auth.turn().round;
            driver.state_mut(AI, round).see_city(classic_rules::ai::state::SeenCity { tile: foe_city, owner: FOE, population: 4, last_seen_round: round });
            driver.run(&mut game, &rules, u32::MAX);
        }
    }
    let probe = war_probe::take();
    war_probe::enable(false);
    let looks: Vec<_> = probe.checks.iter().filter(|c| c.round >= 35).collect();
    assert!(!looks.is_empty(), "the AI looked for an operation");
    looks.iter().map(|c| f64::from(c.free)).sum::<f64>() / looks.len() as f64
}

#[test]
fn quiet_garrisons_count_as_free_troops_under_warfix_group() {
    let plain = free_troops(Variant::Unfixed);
    let lent = free_troops(Variant::WarfixGroup);
    eprintln!("free troops at a look: standard {plain:.2}, warfix-group {lent:.2}");
    assert!(lent > plain, "{lent} against {plain}");
}
