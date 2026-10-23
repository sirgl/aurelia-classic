//! Headless 200-round classic game with the AI of track B (culture, policies,
//! faith, religion, tourism) on top of the greedy planner: the game must
//! run without panics, stay valid, keep its hash through save and load, and
//! the AI must actually use the mechanics. Prints a summary with
//! `--nocapture` (rounds, commands, policies, religions, wall time).

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::{war_declarations, AiState, GreedyPlanner, Planner};
use classic_rules::ids::{BuildingKindId, PlayerId, ProductionItem, UnitKindId};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const ROUNDS: usize = 200;

/// Buildings tried in this order when a city queue is empty.
const BUILD_ORDER: [&str; 12] = [
    "core:building/monument",
    "core:building/shrine",
    "core:building/granary",
    "core:building/temple",
    "core:building/library",
    "core:building/amphitheater",
    "core:building/stonehenge",
    "core:building/great_library",
    "core:building/market",
    "core:building/university",
    "core:building/opera_house",
    "core:building/museum",
];

fn scenario() -> ScenarioConfig {
    ScenarioConfig {
        seed: 0x7B_2026,
        players: 6,
        width: 56,
        height: 40,
        cities: 0,
        units: 0,
        water_ratio: 22,
    }
}

fn production_orders(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Vec<Command> {
    let mut out = Vec::new();
    for (city, core) in game.view().auth.cities().iter() {
        if core.owner != player || !core.queue.is_empty() {
            continue;
        }
        for key in BUILD_ORDER {
            if let Some(i) = rules.buildings.iter().position(|b| b.key == key) {
                out.push(Command::EnqueueProduction {
                    player,
                    city,
                    item: ProductionItem::Building(BuildingKindId(i as u16)),
                });
            }
        }
        let settler = rules
            .units
            .iter()
            .position(|u| u.key == "core:unit/settler")
            .unwrap();
        out.push(Command::EnqueueProduction {
            player,
            city,
            item: ProductionItem::Unit(UnitKindId(settler as u16)),
        });
    }
    out
}

fn player_turn(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) -> usize {
    let mut accepted = 0;
    let mut run = |game: &mut GameState, cmds: Vec<Command>| {
        for cmd in cmds {
            if game.execute(rules, cmd).is_ok() {
                accepted += 1;
            }
        }
    };
    run(game, war_declarations(&game.ai_observation(rules, player)));
    run(
        game,
        GreedyPlanner.plan(game.ai_observation(rules, player), rules),
    );
    run(game, production_orders(game, rules, player));
    for _ in 0..3 {
        let policy = classic_rules::ai::policies::policy_command(
            &game.ai_observation(rules, player),
            rules,
            &AiState::new(player, 0),
        );
        run(game, policy.into_iter().collect());
    }
    run(
        game,
        classic_rules::ai::religion::religion_commands(&game.ai_observation(rules, player), rules),
    );
    run(
        game,
        classic_rules::ai::tourism::tourism_commands(&game.ai_observation(rules, player), rules),
    );
    run(game, vec![Command::EndTurn { player }]);
    accepted
}

#[test]
fn classic_game_with_track_b_ai_runs_200_rounds() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario());
    let players = game.view().auth.players().count();
    let started = std::time::Instant::now();
    let mut accepted = 0;
    for round in 0..ROUNDS {
        for _ in 0..players {
            let player = game.view().auth.active_player();
            accepted += player_turn(&mut game, &rules, player);
        }
        if round % 25 == 24 {
            game.validate(&rules).expect("world must stay valid");
        }
    }
    let elapsed = started.elapsed();
    game.validate(&rules).unwrap();
    let loaded = decode(&encode(&game, &rules), &rules).expect("own save loads");
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    let auth = game.view().auth;
    let policies: usize = (0..players)
        .map(|p| auth.module().culture().of(PlayerId(p as u8)).policies.len())
        .sum();
    let ideologies = (0..players)
        .filter(|p| {
            auth.module()
                .culture()
                .of(PlayerId(*p as u8))
                .ideology
                .is_some()
        })
        .count();
    let religions = auth.module().religion().religions().len();
    let pantheons = auth.module().religion().pantheons();
    eprintln!(
        "track_b_game: rounds={ROUNDS} round={} commands={accepted} cities={} policies={policies} ideologies={ideologies} pantheons={pantheons} religions={religions} wall={:.1}s ({:.2} ms/round)",
        auth.turn().round,
        auth.cities().len(),
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / ROUNDS as f64,
    );
    assert!(policies >= players * 5, "policies adopted");
    assert!(pantheons >= 2, "pantheons founded");
}

/// The same world played by the M6 AI driver with the track-B advisors in
/// its registry (policies, religion, culture): 200 rounds without a panic,
/// a valid world, save and load keep the hash; prints what the AI did.
#[test]
fn classic_game_with_the_ai_driver_runs_200_rounds() {
    use classic_rules::ai::{AiDriver, AiSettings};
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario());
    let players = game.view().auth.players().count();
    let mut driver = AiDriver::new(
        AiSettings::for_rules(&rules, rules.default_difficulty()),
        Vec::new(),
    );
    let started = std::time::Instant::now();
    while game.view().auth.turn().round < ROUNDS as u32 && game.winner().is_none() {
        driver.run(&mut game, &rules, u32::MAX);
        if game.view().auth.turn().round % 50 == 0 {
            game.validate(&rules).expect("world must stay valid");
        }
    }
    let elapsed = started.elapsed();
    game.validate(&rules).unwrap();
    let loaded = decode(&encode(&game, &rules), &rules).expect("own save loads");
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    let auth = game.view().auth;
    let policies: usize = (0..players)
        .map(|p| auth.module().culture().of(PlayerId(p as u8)).policies.len())
        .sum();
    let influence: u64 = auth
        .module()
        .tourism()
        .players()
        .iter()
        .flat_map(|p| p.influence.iter())
        .sum();
    let archaeology = auth.module().tourism().archaeology();
    eprintln!(
        "driver_game: round={} cities={} policies={policies} pantheons={} religions={} works={} influence={influence} sites={} landmarks={} wall={:.1}s ({:.2} ms/round)",
        auth.turn().round,
        auth.cities().len(),
        auth.module().religion().pantheons(),
        auth.module().religion().religions().len(),
        auth.module().tourism().works().len(),
        archaeology.sites.len(),
        archaeology.landmarks.len(),
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / ROUNDS as f64,
    );
    assert!(policies >= players * 5, "policies adopted");
    assert!(
        auth.module().religion().pantheons() >= 2,
        "pantheons founded"
    );
}
