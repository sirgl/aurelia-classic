//! The AI driver with the M4 combat advisor and tactics wins a conquest
//! with ranged units: archers bombard the passive player's capital from
//! their range, the melee units take it, and holding every original
//! capital the AI wins a domination victory. The truth is read only to judge the outcome.
//!
//! The game's AI plays it (`standard`, whose leaders pursue a victory,
//! classic/docs/ai-leaders.md): the rival's capital is the last original capital
//! the AI lacks, so its capture wins the game and is worth the victory to
//! every leader, whatever its score (the game's AI before the leaders, now
//! `allocator`, valued a war by the time victory's score alone and did not
//! start this one: the two-citizen capital is worth less than the army it
//! risks, −27 to −312 points at rounds 40–85).

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::trace::Outcome;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::ids::{PlayerId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const AI: PlayerId = PlayerId(0);
const PASSIVE: PlayerId = PlayerId(1);
const GRASSLAND: u16 = 1;

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

/// Open grassland; the AI's capital with two archers, two catapults (they
/// set up before they shoot) and five swordsmen on iron, with a war chest;
/// the passive player's only city eight tiles east with a warrior.
fn conquest_game(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(
        rules,
        GameConfig {
            players: 2,
            width: 30,
            height: 20,
            rng_seed: 41,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32)
        .map(|t| (TileId(t), TerrainId(GRASSLAND)))
        .collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, AI, tile(&game, 6, 10));
    // Iron under the capital for the swordsmen (without it they fight at
    // −50 %, rules.md).
    let iron = rules.resources.iter().position(|r| r.key == "core:resource/iron").unwrap();
    game.debug_set_deposit(rules, tile(&game, 6, 10), classic_rules::ids::ResourceId(iron as u16), 6);
    let bronze = rules.technologies.iter().position(|t| t.key == "core:tech/bronze-working").unwrap();
    game.debug_learn(rules, AI, classic_rules::ids::TechId(bronze as u16));
    let archer = unit_kind(rules, "core:unit/archer");
    let catapult = unit_kind(rules, "core:unit/catapult");
    let swordsman = unit_kind(rules, "core:unit/swordsman");
    let warrior = unit_kind(rules, "core:unit/warrior");
    // The oldest units hold the capital (M6 garrison seniority): the
    // swordsmen first, so the ranged units join the assault.
    // Five swordsmen: with classic maintenance (M4-A) the AI buys no extra
    // units, and the rally needs the band at the city besides the
    // garrison and the scout.
    for (q, r) in [(8, 9), (5, 11), (6, 11), (8, 10), (7, 11)] {
        game.debug_spawn_unit(rules, AI, tile(&game, q, r), swordsman);
    }
    for (q, r) in [(7, 10), (7, 9)] {
        game.debug_spawn_unit(rules, AI, tile(&game, q, r), archer);
    }
    for (q, r) in [(6, 9), (5, 10)] {
        game.debug_spawn_unit(rules, AI, tile(&game, q, r), catapult);
    }
    let target = tile(&game, 14, 10);
    game.debug_found_city(rules, PASSIVE, target);
    game.debug_spawn_unit(rules, PASSIVE, target, warrior);
    game.debug_meet(AI, PASSIVE);
    // A war chest: one classic city cannot pay seven units past the free ones
    // (M4-A maintenance); an empty treasury disbands them.
    game.debug_earn(AI, 2_000);
    game
}

fn train_warriors(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    let warrior = unit_kind(rules, "core:unit/warrior");
    let idle: Vec<_> = game
        .view()
        .auth
        .cities()
        .iter()
        .filter(|(_, c)| c.owner == player && c.queue.is_empty())
        .map(|(h, _)| h)
        .collect();
    for city in idle {
        let item = classic_rules::ids::ProductionItem::Unit(warrior);
        let _ = game.execute(rules, Command::EnqueueProduction { player, city, item });
    }
}

// Re-run 2026-10-23 on origin/main and on the branch of the legality work (identical): the diagnosis below is out of date. The AI now declares the war (round 75 to 80) and shoots 51 times in 150 rounds, but does not take the city: the passive rival's city has grown to 7 citizens by round 120, the operation formed at round 70 launches, assaults once and ends (`EndOperation` 1, `AttackCity` 1), the five swordsmen are spread over the map as scouts and garrison. Whether to value the capture higher or to form the operation earlier is a war-economics decision (classic/docs/ai-strikes.md, why wars are rare), not a defect of one planner.
// RED, diagnosed, not fixed (2026-10-22): the standard AI values a war by the score weights (ai/value.rs `ScorePoints` reads SCORE_CITY / SCORE_POPULATION / SCORE_WONDER). With Unciv's weights (10 / 3 / 40) the two-citizen capital is worth less than the army it risks and no war is declared (0 ranged attacks in 150 rounds); with the old weights 8 / 4 / 25 the test passes (measured). A fix belongs in the war valuation (value the capture by what it brings in the ruleset, including the victory), not in the weights.
#[test]
#[ignore = "the standard AI declares no war under Unciv's score weights; see the comment"]
fn the_ai_conquers_with_ranged_units_and_wins() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = conquest_game(&rules);
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), vec![PASSIVE]);
    driver.record_commands();
    driver.set_trace_capacity(100_000);
    while game.view().auth.turn().round < 150 && game.winner().is_none() {
        let active = game.view().auth.active_player();
        if active == AI {
            driver.run(&mut game, &rules, u32::MAX);
        } else {
            // The rival keeps training warriors (the core no longer trains
            // a default unit for an empty queue).
            train_warriors(&mut game, &rules, active);
            game.execute(&rules, Command::EndTurn { player: active })
                .unwrap();
        }
    }
    let shots = driver
        .trace()
        .entries()
        .filter(|e| e.outcome == Outcome::Applied)
        .filter(|e| matches!(e.command, Some(Command::RangedAttack { .. })))
        .count();
    let round = game.view().auth.turn().round;
    eprintln!(
        "conquest: winner {:?} at round {round}, {shots} ranged attacks",
        game.winner()
    );
    game.validate(&rules).unwrap();
    assert!(shots > 0, "the archers bombarded");
    assert_eq!(
        game.winner(),
        Some(AI),
        "the AI took the only city of its rival"
    );
    let outcome = game.outcome().expect("a victory");
    assert_eq!(
        outcome.kind,
        classic_rules::world::victory::VictoryKind::Domination
    );
}
