//! The AI turn driver (PLAN M6 stage C): slicing never changes a decision,
//! and a game saved between two players' turns continues identically.

use classic_content::core::core_rules;
use classic_rules::ai::{AiDriver, AiSettings, DriverPhase};
use classic_rules::ids::PlayerId;
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

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

fn driver(rules: &CompiledRuleset) -> AiDriver {
    let mut driver = AiDriver::new(AiSettings::for_rules(rules, rules.default_difficulty()), Vec::new());
    driver.record_commands();
    driver
}

/// Play `rounds` rounds giving the driver `budget` steps per tick; returns
/// the hash after every tick that ended a round.
fn play(
    game: &mut GameState,
    rules: &CompiledRuleset,
    driver: &mut AiDriver,
    rounds: u32,
    budget: u32,
) -> Vec<u64> {
    let mut hashes = Vec::new();
    while hashes.len() < rounds as usize {
        let report = driver.run(game, rules, budget);
        assert!(!report.waiting, "an all-AI game never waits");
        if report.round_done {
            hashes.push(authoritative_hash(game));
        }
    }
    hashes
}

#[test]
fn slicing_budgets_give_identical_commands_and_hashes() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut runs = Vec::new();
    for budget in [1, 7, 100] {
        let mut game = generate_new_game(&rules, &scenario());
        let mut driver = driver(&rules);
        let hashes = play(&mut game, &rules, &mut driver, 40, budget);
        game.validate(&rules).unwrap();
        let commands = format!("{:?}", driver.commands());
        runs.push((budget, hashes, commands, driver.states().clone()));
    }
    let (_, hashes, commands, states) = &runs[0];
    assert!(driver_did_something(commands), "the AI must act");
    for (budget, other_hashes, other_commands, other_states) in &runs[1..] {
        assert_eq!(
            other_hashes, hashes,
            "hashes differ at {budget} steps per tick"
        );
        assert_eq!(
            other_commands, commands,
            "commands differ at {budget} steps per tick"
        );
        assert_eq!(
            other_states, states,
            "AI memory differs at {budget} steps per tick"
        );
    }
}

fn driver_did_something(commands: &str) -> bool {
    commands.contains("FoundCity")
        && commands.contains("EnqueueProduction")
        && commands.contains("MoveUnit")
}

/// Run until the driver stands between two players' turns in mid-round.
fn run_to_mid_round_boundary(game: &mut GameState, rules: &CompiledRuleset, driver: &mut AiDriver) {
    loop {
        let report = driver.run(game, rules, 1);
        let between =
            report.players_done.len() == 1 && game.view().auth.active_player() != PlayerId(0);
        if between {
            assert!(driver.at_player_boundary());
            return;
        }
    }
}

#[test]
fn saved_between_players_the_game_continues_identically_for_100_rounds() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario());
    let mut live = driver(&rules);
    play(&mut game, &rules, &mut live, 20, 50);
    run_to_mid_round_boundary(&mut game, &rules, &mut live);
    assert_eq!(live.phase(), DriverPhase::NextPlayer);
    let world = encode(&game, &rules);
    let memory = live.save();
    let mut loaded = decode(&world, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    let mut restored = driver(&rules);
    restored
        .load(&memory, loaded.view().auth.map().tile_count(), &rules)
        .unwrap();
    assert_eq!(restored.states(), live.states());
    let before = live.commands().len();
    let live_hashes = play(&mut game, &rules, &mut live, 100, 13);
    let loaded_hashes = play(&mut loaded, &rules, &mut restored, 100, 13);
    assert_eq!(loaded_hashes, live_hashes, "the loaded game diverged");
    // Handles are runtime ids (a load renumbers them): compare the
    // decisions by persistent actor ids and reasons.
    assert_eq!(
        decisions(&restored),
        decisions(&live)[decisions(&live).len() - decisions(&restored).len()..]
    );
    assert_eq!(restored.commands().len(), live.commands().len() - before);
    assert_eq!(restored.states(), live.states());
    assert!(live.counters().commands_applied > 0);
}

/// (round, player, actor, reason, applied) of every traced decision.
fn decisions(driver: &AiDriver) -> Vec<String> {
    driver
        .trace()
        .entries()
        .map(|e| {
            format!(
                "{} {:?} {:?} {:?} {:?}",
                e.round, e.player, e.actor, e.reason, e.outcome
            )
        })
        .collect()
}

#[test]
fn a_human_turn_stops_the_driver_and_input_resumes_it() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario());
    let mut driver = AiDriver::new(
        AiSettings::for_rules(&rules, rules.default_difficulty()),
        vec![PlayerId(0)],
    );
    let report = driver.run(&mut game, &rules, 1000);
    assert!(report.waiting, "P0 is human and active: nothing to do");
    assert_eq!(report.steps, 1);
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    let report = driver.run(&mut game, &rules, u32::MAX);
    assert!(report.round_done);
    assert_eq!(
        report.players_done,
        vec![PlayerId(1), PlayerId(2), PlayerId(3)]
    );
    assert_eq!(game.view().auth.active_player(), PlayerId(0));
    assert_eq!(game.view().auth.turn().round, 1);
    assert!(driver.at_player_boundary());
}

/// A system module plugs in through the registry; its commands run at
/// their moment like any decision (traced, validated by the core).
#[test]
fn a_system_advisor_plugs_in_and_its_commands_run() {
    use classic_rules::ai::system::{AdvisorInput, Proposal, SystemAdvisor};
    use classic_rules::classic::ClassicSystem;
    use classic_rules::ai::trace::{Outcome, Reason};
    // classic open borders need Civil Service (M4 diplomacy): the probe
    // advisor denounces every civilization it met instead.
    struct DenounceAll;
    impl SystemAdvisor for DenounceAll {
        fn start_of_turn(&self, input: &AdvisorInput<'_, '_>) -> Vec<Proposal> {
            let player = input.observation.player;
            let mut out = Vec::new();
            for relation in &input.observation.relations {
                if !relation.we_denounced {
                    let command = Command::Denounce {
                        player,
                        target: relation.other,
                    };
                    out.push(Proposal {
                        priority: 1,
                        command,
                    });
                }
            }
            out
        }
    }
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &scenario());
    let mut driver = driver(&rules);
    driver.set_trace_capacity(100_000);
    let systems = driver.mind_mut().advisors_mut().systems();
    driver.mind_mut().advisors_mut().register(ClassicSystem::Diplomacy, Box::new(DenounceAll));
    assert_eq!(driver.mind_mut().advisors_mut().systems(), systems, "replaced, not added");
    play(&mut game, &rules, &mut driver, 60, 100);
    let advised = driver
        .trace()
        .entries()
        .filter(|e| e.reason == Reason::SystemAdvisor && e.outcome == Outcome::Applied)
        .count();
    let auth = game.view().auth;
    let met = (1..4).any(|p| auth.knowledge()[0].contact(PlayerId(p)).met_round.is_some());
    assert!(met, "the fixture players meet");
    assert!(advised > 0, "the advisor's commands were executed");
    let denounced = driver.trace().entries().any(|e| {
        e.outcome == Outcome::Applied && matches!(e.command, Some(Command::Denounce { .. }))
    });
    assert!(denounced, "met players were denounced by the advisor");
}

/// Difficulty is data (the handicap rows), not code: the AI settings of a
/// level are read from its row and the AI_DEFAULT row.
#[test]
fn difficulty_levels_are_handicap_rows() {
    let rules = compile_rules(core_rules()).unwrap();
    let keys: Vec<&str> = rules.handicaps.iter().map(|h| h.key.as_str()).collect();
    assert_eq!(keys.len(), 9);
    assert_eq!(keys[0], "core:handicap/settler");
    assert_eq!(keys[7], "core:handicap/deity");
    assert_eq!(keys[8], rules.ai_handicap().unwrap().key);
    let supply = |level: &str| AiSettings::for_rules(&rules, level).supply;
    let deity = supply("core:handicap/deity");
    let settler = supply("core:handicap/settler");
    assert_eq!(
        (deity.free_units, deity.per_city, deity.ai_percent),
        (7, 3, 50)
    );
    assert_eq!(settler.ai_percent, 0);
    assert!(deity.supplied(4, 20) > settler.supplied(4, 20));
    let ai_default = rules.ai_handicap().unwrap();
    assert_eq!(ai_default.values.module.happiness_default, 15);
    let deity_row = rules.handicap("core:handicap/deity").unwrap();
    assert_eq!(deity_row.values.ai_train_percent, 50);
}
