//! Save → load → continue: the loaded game is the same game (PLAN M3 P1).
//!
//! A real-content game is played by the greedy AI with production orders;
//! mid-game it is saved and loaded, and both copies keep playing the same
//! way. Every round of the continuation must have the same world hash, and
//! the save of the continued copy must equal the save of the original.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::{war_declarations, GreedyPlanner, Planner};
use classic_rules::ids::{CityHandle, PlayerId, ProductionItem};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

fn scenario() -> ScenarioConfig {
    ScenarioConfig {
        seed: 0x5A7E_0001,
        players: 3,
        width: 24,
        height: 24,
        cities: 6,
        units: 30,
        water_ratio: 10,
    }
}

fn own_cities(game: &GameState, player: PlayerId) -> Vec<CityHandle> {
    let auth = game.view().auth;
    auth.cities()
        .iter()
        .filter(|(_, core)| core.owner == player)
        .map(|(handle, _)| handle)
        .collect()
}

/// Warriors forever: units are trained, fight and die, so unit slots are
/// freed and reused before the save.
fn production_orders(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Vec<Command> {
    let warrior = rules
        .units
        .iter()
        .position(|u| u.key == "core:unit/warrior")
        .expect("warrior in core content");
    let mut out = Vec::new();
    for city in own_cities(game, player) {
        if game
            .view()
            .auth
            .cities()
            .get(city)
            .unwrap()
            .queue
            .is_empty()
        {
            let item = ProductionItem::Unit(classic_rules::ids::UnitKindId(warrior as u16));
            out.push(Command::EnqueueProduction { player, city, item });
        }
    }
    out
}

/// One full round: every player in turn order plans on its own state.
fn play_round(game: &mut GameState, rules: &CompiledRuleset) {
    let players = game.view().auth.players().count();
    for _ in 0..players {
        let player = game.view().auth.active_player();
        let mut commands = war_declarations(&game.ai_observation(rules, player));
        commands.extend(GreedyPlanner.plan(game.ai_observation(rules, player), rules));
        commands.extend(production_orders(game, rules, player));
        for command in commands {
            let _ = game.execute(rules, command);
        }
        let _ = game.execute(rules, Command::EndTurn { player });
    }
}

#[test]
fn loaded_game_continues_exactly_like_the_original() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut original = generate(&scenario(), &rules);
    let mut died = 0usize;
    for _ in 0..40 {
        let before = original.view().auth.units().len();
        play_round(&mut original, &rules);
        died += before.saturating_sub(original.view().auth.units().len());
    }
    assert!(
        died > 0,
        "the scenario must free unit slots before the save"
    );
    let mut loaded = decode(&encode(&original, &rules), &rules).expect("own save loads");
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&original));
    for round in 0..30 {
        play_round(&mut original, &rules);
        play_round(&mut loaded, &rules);
        assert_eq!(
            authoritative_hash(&loaded),
            authoritative_hash(&original),
            "continuation diverged in round {round}"
        );
        loaded.validate(&rules).unwrap();
    }
    let a = encode(&original, &rules).world;
    let b = encode(&loaded, &rules).world;
    assert!(a == b, "saves of the two continuations differ");
}
