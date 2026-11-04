//! PlayerAgent P2 (classic/docs/player-agent.md): the classic AI played through a
//! `SeatTable` by an `AiAgent` is the direct driver: the same commands, the
//! same hashes after every round, the same AI memory, at any slicing; a seat
//! without an agent is left to its human.

use classic_content::core::core_rules;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::classic::Classic;
use classic_rules::ids::PlayerId;
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, Command, CompiledRuleset};
use kernel::agent::{AiAgent, SeatTable};
use kernel::ai::StepOutcome;

fn scenario() -> ScenarioConfig {
    ScenarioConfig {
        seed: 0xA1_5EED,
        players: 4,
        width: 40,
        height: 32,
        cities: 0,
        units: 0,
        water_ratio: 15,
    }
}

fn driver(rules: &CompiledRuleset, humans: Vec<PlayerId>) -> AiDriver {
    let mut driver = AiDriver::new(
        AiSettings::for_rules(rules, rules.default_difficulty()),
        humans,
    );
    driver.record_commands();
    driver
}

const ROUNDS: usize = 40;

/// The hash after every round, the commands, the AI memory: the direct driver.
fn direct(rules: &CompiledRuleset) -> (Vec<u64>, String, String) {
    let mut game = generate_new_game(rules, &scenario());
    let mut driver = driver(rules, Vec::new());
    let mut hashes = Vec::new();
    while hashes.len() < ROUNDS {
        let report = driver.run(&mut game, rules, 100);
        assert!(!report.waiting);
        if report.round_done {
            hashes.push(authoritative_hash(&game));
        }
    }
    (
        hashes,
        format!("{:?}", driver.commands()),
        format!("{:?}", driver.states()),
    )
}

fn through_the_table(rules: &CompiledRuleset, slice: u32) -> (Vec<u64>, String, String) {
    let mut game = generate_new_game(rules, &scenario());
    let seats: Vec<PlayerId> = (0..4).map(PlayerId).collect();
    let mut table: SeatTable<Classic> = SeatTable::new();
    let slot = table.assign(
        &seats,
        Box::new(AiAgent::<Classic>::new(driver(rules, Vec::new()))),
    );
    let mut hashes = Vec::new();
    while hashes.len() < ROUNDS {
        let report = table.run(&mut game, rules, slice);
        assert!(!report.waiting);
        if report.round_done {
            hashes.push(authoritative_hash(&game));
        }
    }
    let ai = table
        .agent_ref::<AiAgent<Classic>>(slot)
        .expect("the AI agent")
        .ai();
    (
        hashes,
        format!("{:?}", ai.commands()),
        format!("{:?}", ai.states()),
    )
}

#[test]
fn the_ai_agent_plays_the_direct_drivers_game_at_any_slicing() {
    let rules = compile_rules(core_rules()).unwrap();
    let direct = direct(&rules);
    assert!(
        direct.1.contains("FoundCity") && direct.1.contains("MoveUnit"),
        "the AI must act"
    );
    for slice in [1, 7, 100] {
        let table = through_the_table(&rules, slice);
        assert_eq!(table.0, direct.0, "hashes differ at {slice} steps per run");
        assert_eq!(
            table.1, direct.1,
            "commands differ at {slice} steps per run"
        );
        assert_eq!(
            table.2, direct.2,
            "AI memory differs at {slice} steps per run"
        );
    }
}

#[test]
fn a_seat_without_an_agent_waits_for_its_human() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario());
    let humans = vec![PlayerId(0)];
    let mut table: SeatTable<Classic> = SeatTable::new();
    table.assign(
        &[PlayerId(1), PlayerId(2), PlayerId(3)],
        Box::new(AiAgent::<Classic>::new(driver(&rules, humans))),
    );
    assert_eq!(
        table.step(&mut game, &rules),
        StepOutcome::Waiting,
        "P0 is a human with no agent"
    );
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    let report = table.run(&mut game, &rules, u32::MAX);
    assert!(report.round_done);
    assert_eq!(
        report.players_done,
        vec![PlayerId(1), PlayerId(2), PlayerId(3)]
    );
    assert_eq!(game.view().auth.active_player(), PlayerId(0));
    assert_eq!(game.view().auth.turn().round, 1);
}
