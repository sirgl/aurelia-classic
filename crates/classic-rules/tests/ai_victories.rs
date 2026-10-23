//! The AI pursues the victories it can reach (classic/docs/QA.md): the full-game
//! harness found science never won; these are the reduced cases.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::ids::{CityHandle, PlayerId, ProductionItem, TechId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::ProjectId;
use classic_rules::world::state::GameConfig;
use classic_rules::world::victory::VictoryKind;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn project(rules: &CompiledRuleset, key: &str) -> ProjectId {
    ProjectId(rules.module.projects.iter().position(|p| p.key == key).unwrap() as u16)
}

/// P0 knows every technology and has a capital and a second city; P1 has
/// one city far away.
fn space_age() -> (GameState, CompiledRuleset, CityHandle, CityHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 30,
            height: 20,
            rng_seed: 5,
        },
    );
    let capital = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let second = game.debug_found_city(&rules, P0, tile(&game, 10, 5));
    game.debug_found_city(&rules, P1, tile(&game, 25, 15));
    for t in 0..rules.technologies.len() {
        game.debug_learn(&rules, P0, TechId(t as u16));
    }
    // Spaceship parts need Aluminum (classic strategic resources): a deposit
    let aluminum = rules.resources.iter().position(|r| r.key == "core:resource/aluminum").unwrap();
    let site = tile(&game, 5, 5); // under the capital: the city works it
    game.debug_set_deposit(&rules, site, classic_rules::ids::ResourceId(aluminum as u16), 12);
    (game, rules, capital, second)
}

fn next_item(game: &GameState, rules: &CompiledRuleset, city: CityHandle, planned: &[ProductionItem]) -> Option<ProductionItem> {
    let obs = game.ai_observation(rules, P0);
    let own = obs.own_cities.iter().find(|c| c.handle == city).unwrap();
    classic_rules::ai::victory::next_space_item(&obs, rules, own, planned)
}

/// Apollo first, then exactly the parts the ship lacks: installed parts,
/// parts on the map and parts queued or planned count. It used to queue
/// one part per idle city by kind order, without counting, and only in
/// cities with nothing else to build.
#[test]
fn the_ai_orders_apollo_then_only_the_missing_parts() {
    let (mut game, rules, capital, second) = space_age();
    let apollo = project(&rules, "core:project/apollo_program");
    assert_eq!(next_item(&game, &rules, capital, &[]), Some(ProductionItem::Project(apollo)));
    assert_eq!(next_item(&game, &rules, second, &[ProductionItem::Project(apollo)]), None, "one Apollo");

    game.debug_add_project(P0, apollo);
    let booster = unit(&rules, "core:unit/ss-booster");
    let cockpit = unit(&rules, "core:unit/ss-cockpit");
    let engine = unit(&rules, "core:unit/ss-engine");
    let stasis = unit(&rules, "core:unit/ss-stasis-chamber");
    game.debug_add_project(P0, project(&rules, "core:project/ss_booster"));
    game.debug_spawn_unit(&rules, P0, tile(&game, 6, 6), booster);
    game.execute(&rules, Command::EnqueueProduction { player: P0, city: second, item: ProductionItem::Unit(cockpit) })
        .unwrap();
    let first = next_item(&game, &rules, capital, &[]);
    assert_eq!(first, Some(ProductionItem::Unit(booster)), "the third booster");
    let planned = [ProductionItem::Unit(booster)];
    assert_eq!(next_item(&game, &rules, capital, &planned), Some(ProductionItem::Unit(engine)), "the cockpit is queued");
    let planned = [ProductionItem::Unit(booster), ProductionItem::Unit(engine)];
    assert_eq!(next_item(&game, &rules, capital, &planned), Some(ProductionItem::Unit(stasis)));
    let planned = [ProductionItem::Unit(booster), ProductionItem::Unit(engine), ProductionItem::Unit(stasis)];
    assert_eq!(next_item(&game, &rules, capital, &planned), None, "the ship is complete");
}

/// Science off: no Apollo, no parts.
#[test]
fn the_ai_builds_no_spaceship_when_science_victory_is_off() {
    let (mut game, rules, capital, _) = space_age();
    let options = game.view().auth.module().victory().options().with(VictoryKind::Science, false);
    game.debug_set_victory_options(options);
    assert_eq!(next_item(&game, &rules, capital, &[]), None);
}

/// Six parts next to a capital crowded with civilians (a worker and a
/// great scientist stacked there by training): the AI moves the
/// civilians out, marches the parts in and installs them, and wins. The
/// parts used to wait at the gate for the rest of the game.
#[test]
fn the_ai_clears_its_capital_installs_every_part_and_wins() {
    let (mut game, rules, _, _) = space_age();
    game.debug_add_project(P0, project(&rules, "core:project/apollo_program"));
    let capital = tile(&game, 5, 5);
    game.debug_spawn_unit(&rules, P0, capital, unit(&rules, "core:unit/worker"));
    game.debug_spawn_unit(&rules, P0, capital, unit(&rules, "core:unit/great-scientist"));
    let parts = ["ss-booster", "ss-booster", "ss-booster", "ss-cockpit", "ss-engine", "ss-stasis-chamber"];
    let gates = [(6, 4), (6, 5), (4, 6), (5, 6), (4, 5), (5, 4)];
    for (key, (q, r)) in parts.iter().zip(gates) {
        game.debug_spawn_unit(&rules, P0, tile(&game, q, r), unit(&rules, &format!("core:unit/{key}")));
    }
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), vec![P1]);
    for _ in 0..12 {
        if game.outcome().is_some() {
            break;
        }
        driver.run(&mut game, &rules, u32::MAX);
        if game.view().auth.active_player() == P1 {
            game.execute(&rules, Command::EndTurn { player: P1 }).unwrap();
        }
    }
    let outcome = game.outcome().expect("the spaceship launches");
    assert_eq!((outcome.winner, outcome.kind), (P0, VictoryKind::Science));
    game.validate(&rules).unwrap();
}
