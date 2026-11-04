//! The AI orders only what the rules allow (`ai::legal`, classic/docs/ai-legal-orders.md): a
//! national wonder that needs a building in every city, a unit that needs a coast, a technology
//! the rules do not offer. The full-game measurement is `aurelia-qa/tests/ai_legal_orders.rs`.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{BuildingKindId, PlayerId, ProductionItem, TechId, TileId};
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::mind::AiMind;
use kernel::ai::trace::Outcome;

const AI: PlayerId = PlayerId(0);
const OTHER: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

/// Two cities of the AI and one of the other player; the AI knows the technology of the
/// national college (a national wonder that needs a library in every city) and has a
/// library in the first `libraries` cities. A treasury and room to build.
fn scene(rules: &CompiledRuleset, libraries: usize) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 40, height: 30, rng_seed: 5 });
    let first = game.debug_found_city(rules, AI, tile(&game, 5, 5));
    let second = game.debug_found_city(rules, AI, tile(&game, 10, 8));
    game.debug_found_city(rules, OTHER, tile(&game, 30, 20));
    for key in ["core:tech/writing", "core:tech/philosophy"] {
        game.debug_learn(rules, AI, tech(rules, key));
    }
    let library = building(rules, "core:building/library");
    for city in [first, second].iter().take(libraries) {
        game.debug_complete_building(rules, *city, library);
    }
    game
}

/// The production orders the end of an AI turn plans, and whether the rules accept each now.
fn planned_orders(rules: &CompiledRuleset, game: &GameState, variant: Variant) -> Vec<(ProductionItem, bool)> {
    let mind = ClassicMind::new(variant);
    let sight = mind.sense(game, rules, AI);
    let mut state = classic_rules::ai::AiState::new(AI, game.view().auth.turn().round);
    let started = mind.start_turn(&sight, rules, AI, &mut state);
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut scratch = Default::default();
    let mut counters = Default::default();
    let mut tools = kernel::ai::plan::PlanTools { rules, settings: &settings, scratch: &mut scratch, counters: &mut counters };
    mind.plan_end_of_turn(&sight, AI, &started.brief, &state, &mut tools)
        .into_iter()
        .filter_map(|d| d.command)
        .filter_map(|c| match &c {
            Command::EnqueueProduction { item, .. } => Some((*item, sight.view.accepts(&c))),
            _ => None,
        })
        .collect()
}

/// A national wonder that needs a library in every city is never ordered while one city has
/// none, and what is ordered is accepted by the rules; with a library in each it may be.
#[test]
fn a_national_wonder_that_needs_a_building_in_every_city_is_ordered_only_when_every_city_has_it() {
    let rules = compile_rules(core_rules()).unwrap();
    let college = ProductionItem::Building(building(&rules, "core:building/national_college"));
    for variant in [Variant::Standard, Variant::Previous] {
        let orders = planned_orders(&rules, &scene(&rules, 1), variant);
        assert!(!orders.iter().any(|(item, _)| *item == college), "{variant:?}: {orders:?}");
        assert!(orders.iter().all(|(_, accepted)| *accepted), "{variant:?}: {orders:?}");
        let orders = planned_orders(&rules, &scene(&rules, 2), variant);
        assert!(orders.iter().all(|(_, accepted)| *accepted), "{variant:?}: {orders:?}");
    }
}

/// Over some turns of a small game every order the standard AI sent was accepted by the
/// core (the full-game measurement is in `aurelia-qa`).
#[test]
fn the_core_refuses_no_order_of_a_small_game() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules, 1);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, vec![OTHER], ClassicMind::new(Variant::Standard));
    driver.set_trace_capacity(100_000);
    for _ in 0..40 {
        let round = game.view().auth.turn().round;
        while game.view().auth.turn().round == round {
            driver.run(&mut game, &rules, u32::MAX);
            if game.view().auth.active_player() == OTHER {
                game.execute(&rules, Command::EndTurn { player: OTHER }).unwrap();
            }
        }
    }
    let refused: Vec<_> = driver.trace().entries().filter(|e| matches!(e.outcome, Outcome::Rejected(_))).map(|e| format!("{e}")).collect();
    assert!(refused.is_empty(), "{refused:?}");
}
