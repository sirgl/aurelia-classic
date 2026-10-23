//! A damaged save is a DecodeError::Corrupt, never a panic (PLAN M3 P2).
//!
//! Each case damages one field of a real mid-game save so that it no longer
//! describes a legal game state (content ids outside the rules, impossible
//! values, broken invariants). Decode must reject every case as Corrupt:
//! accepting it lets the garbage reach the turn code and the UI, which
//! index content tables by those ids (in debug builds a population of
//! u32::MAX already overflowed in the turn code). A panic fails the test.

use classic_rules::ClassicGame;
use std::panic::{catch_unwind, AssertUnwindSafe};

use classic_content::core::core_rules;
use classic_rules::ai::{GreedyPlanner, Planner};
use classic_rules::ids::ProductionItem;
use classic_rules::persistence::decode::{decode, DecodeError};
use classic_rules::persistence::encode::encode;
use classic_rules::persistence::SavedWorldV1;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

fn played_game(rules: &CompiledRuleset) -> GameState {
    let config = ScenarioConfig {
        seed: 0xC0_44_07,
        players: 2,
        width: 16,
        height: 16,
        cities: 3,
        units: 8,
        water_ratio: 10,
    };
    let mut game = generate(&config, rules);
    let monument = rules
        .buildings
        .iter()
        .position(|b| b.key == "core:building/monument")
        .unwrap();
    for _ in 0..6 {
        play_round(&mut game, rules, Some(monument));
    }
    game
}

fn play_round(game: &mut GameState, rules: &CompiledRuleset, build: Option<usize>) {
    for _ in 0..game.view().auth.players().count() {
        let player = game.view().auth.active_player();
        let mut commands = GreedyPlanner.plan(game.ai_observation(rules, player), rules);
        if let Some(building) = build {
            for (city, core) in game.view().auth.cities().iter() {
                if core.owner == player {
                    let item =
                        ProductionItem::Building(classic_rules::ids::BuildingKindId(building as u16));
                    commands.push(Command::EnqueueProduction { player, city, item });
                }
            }
        }
        for command in commands {
            let _ = game.execute(rules, command);
        }
        let _ = game.execute(rules, Command::EndTurn { player });
    }
}

type Damage = fn(&mut SavedWorldV1);

fn damages() -> Vec<(&'static str, Damage)> {
    vec![
        ("unit hp", |w| w.units[0].hp = 0),
        ("unit hp above max", |w| w.units[0].hp = 999),
        ("unit moves", |w| w.units[0].moves_left = 9_999),
        ("unit work tile", |w| {
            w.units[0].work = Some((999_999, 0, 0, 3))
        }),
        ("unit work improvement", |w| {
            w.units[0].work = Some((0, 0, 999, 3))
        }),
        ("unit work tag", |w| w.units[0].work = Some((0, 7, 0, 3))),
        ("unit work chop feature", |w| {
            w.units[0].work = Some((0, 2, 999, 3))
        }),
        ("unit goal", |w| w.units[0].goal = Some(999_999)),
        ("unit carrier missing", |w| w.units[0].module = Some(u64::MAX)),
        ("unit carried by itself", |w| w.units[0].module = Some(w.units[0].id)),
        ("city building", |w| w.cities[0].buildings.push(999)),
        ("city queue unit", |w| w.cities[0].queue.push((0, 999))),
        ("city queue building", |w| w.cities[0].queue.push((1, 999))),
        ("city population", |w| w.cities[0].population = u32::MAX),
        ("city population zero", |w| w.cities[0].population = 0),
        ("city unworked tile", |w| w.cities[0].unworked.push(999_999)),
        ("city radius", |w| w.cities[0].radius = 255),
        ("city damage", |w| w.cities[0].damage = u16::MAX),
        ("second city on a tile", |w| {
            w.cities[1].tile = w.cities[0].tile
        }),
        ("terrain", |w| w.terrain[0] = 999),
        ("feature", |w| w.feature[0] = Some(999)),
        ("resource", |w| w.resource[0] = Some(999)),
        ("improvement", |w| w.improvement[0] = Some(999)),
        ("player civ", |w| w.player_civs[0] = 250),
        ("player gold missing", |w| w.player_gold.clear()),
        ("player science missing", |w| w.player_science.clear()),
        ("player research missing", |w| w.player_research.clear()),
        ("player civs missing", |w| w.player_civs.clear()),
        ("knowledge value", |w| w.knowledge_explored[0][0] = 7),
        ("knowledge memory missing", |w| {
            w.knowledge_memory[0].pop();
        }),
        ("knowledge memory owner", |w| {
            w.knowledge_memory[0][0].3 = 77
        }),
        ("contact from the future", |w| {
            w.knowledge_contacts[0][1] = (Some(9_999), None)
        }),
        ("diplomacy value", |w| w.diplomacy[1] = 7),
        ("tile city", |w| {
            let owned = w.owner.iter().position(|o| o.is_some()).unwrap();
            w.tile_city[owned] = Some(999);
        }),
        ("next entity", |w| w.next_entity = 0),
        ("dead player with units", |w| w.players_alive[0] = false),
    ]
}

/// Decode and, when accepted, validate and play two rounds.
fn load_and_play(
    save: &classic_rules::persistence::SaveGameV1,
    rules: &CompiledRuleset,
) -> Result<(), DecodeError> {
    let mut game = decode(save, rules)?;
    game.validate(rules).map_err(DecodeError::Corrupt)?;
    for _ in 0..2 {
        play_round(&mut game, rules, None);
    }
    game.validate(rules)
        .expect("an accepted save keeps the world valid");
    Ok(())
}

#[test]
fn damaged_saves_are_errors_not_panics() {
    let rules = compile_rules(core_rules()).unwrap();
    let game = played_game(&rules);
    let clean = encode(&game, &rules);
    assert!(!clean.world.cities.is_empty() && !clean.world.units.is_empty());
    let mut panicked = Vec::new();
    let mut accepted = Vec::new();
    for (name, damage) in damages() {
        let mut save = clean.clone();
        damage(&mut save.world);
        let outcome = catch_unwind(AssertUnwindSafe(|| load_and_play(&save, &rules)));
        match outcome {
            Ok(Err(DecodeError::Corrupt(_))) => {}
            Ok(other) => accepted.push((name, other)),
            Err(_) => panicked.push(name),
        }
    }
    assert!(panicked.is_empty(), "damaged saves panicked: {panicked:?}");
    assert!(accepted.is_empty(), "damaged saves accepted: {accepted:?}");
}
