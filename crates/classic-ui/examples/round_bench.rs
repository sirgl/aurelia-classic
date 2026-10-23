//! Cost breakdown of driver.round on the rep map (diagnosing the End Turn freeze).
//! Headless: GameClient with Renderer::headless, all the logic on the CPU.
//! Prints the breakdown: observe / plan-execute(ok+err) / projection / validate.

use classic_rules::ClassicGame;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use client::simulation_driver::{DriverConfig, SimulationDriver};
use client::GameClient;
use classic_rules::ai::{GreedyPlanner, Planner};
use classic_rules::command::Command;
use classic_rules::ids::PlayerId;
use classic_rules::rules::SourceRules;
use classic_rules::scenario::ScenarioConfig;
use classic_rules::{compile_rules, GameState};
use render::renderer::Renderer;
use render::world::projection;

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let rules = Arc::new(rules);
    let rep = ScenarioConfig::representative_late_game();
    let game = classic_rules::scenario::generate(&rep, &rules);
    let mut client = GameClient::new(game, rules.clone(), Renderer::headless(), PlayerId(0));

    // 1. Observations of all AIs (full scans).
    let t0 = Instant::now();
    for p in 1..8u8 {
        black_box(client.game.ai_observation(&rules, PlayerId(p)));
    }
    println!("observe x7: {:.2}ms total", ms(t0.elapsed()));

    // 2. Planning of all AIs (without execution).
    let t0 = Instant::now();
    let mut ai = GreedyPlanner;
    let mut planned = 0;
    for p in 1..8u8 {
        let cmds = ai.plan(client.game.ai_observation(&rules, PlayerId(p)), &rules);
        planned += cmds.len();
    }
    println!(
        "plan x7: {:.2}ms total, {} commands",
        ms(t0.elapsed()),
        planned
    );

    // 3. A full round as in the game (with a projection on every submit).
    let mut driver = SimulationDriver::new(DriverConfig::default(), &rules);
    let t0 = Instant::now();
    let (done, errs) = driver.round(&mut client);
    println!(
        "driver.round: {:.1}ms total, done={} errs={}",
        ms(t0.elapsed()),
        done,
        errs
    );

    // 4. Separately: the cost of one projection (rebuild RenderWorld).
    let t0 = Instant::now();
    for _ in 0..10 {
        black_box(projection::rebuild_render_world(
            client.game.view(),
            client.observer,
            &client.rules,
        ));
    }
    println!(
        "projection rebuild x10: {:.2}ms/iter",
        ms(t0.elapsed()) / 10.0
    );

    // 5. Separately: validate (oracle) after a commit.
    let t0 = Instant::now();
    client.game.validate(&client.rules).unwrap();
    println!("validate x1: {:.2}ms", ms(t0.elapsed()));

    // 6. One player's EndTurn (the points reset).
    let t0 = Instant::now();
    for _ in 0..20 {
        black_box(
            client
                .game
                .execute(
                    &rules,
                    Command::EndTurn {
                        player: PlayerId(3),
                    },
                )
                .unwrap(),
        );
    }
    println!(
        "execute(EndTurn) x20: {:.3}ms/iter",
        ms(t0.elapsed()) / 20.0
    );

    let _ = GameState::new;
}
