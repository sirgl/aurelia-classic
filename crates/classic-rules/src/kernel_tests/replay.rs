use crate::Command;
use crate::replay::log::*;
use crate::ids::{PlayerId, TileId, UnitKindId};
use crate::replay::authoritative_hash;
use crate::rules::SourceRules;
use crate::scenario::ScenarioConfig;
use crate::{compile_rules, GameState};

/// A small deterministic world + a command script (all must be Ok).
fn scripted_game(seed: u64) -> (GameState, crate::rules::CompiledRuleset, Vec<Command>) {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let cfg = ScenarioConfig {
        seed,
        players: 2,
        width: 16,
        height: 16,
        cities: 3,
        units: 6,
        water_ratio: 0,
    };
    let mut game = crate::scenario::generate(&cfg, &rules);
    let mut script = Vec::new();
    // War P0-P1: borders are open everywhere (otherwise closed borders break
    // the script; the test is about replay determinism, not diplomacy).
    script.push(Command::DeclareWar {
        player: PlayerId(0),
        enemy: PlayerId(1),
    });
    // Move the first two P0 units onto neighbouring tiles (cost 1 <= margin 2).
    let units = game.view().auth.indexes().units_of(PlayerId(0)).to_vec();
    let dims = game.view().auth.map().dimensions;
    for u in units.iter().take(2) {
        let cur = game.view().auth.units().get(*u).unwrap().core.tile.0;
        let x = cur % dims.width;
        let dest = if x + 1 < dims.width { cur + 1 } else { cur - 1 };
        script.push(Command::MoveUnit {
            unit: *u,
            destination: TileId(dest),
        });
    }
    script.push(Command::EndTurn {
        player: PlayerId(0),
    });
    script.push(Command::EndTurn {
        player: PlayerId(1),
    });
    for c in &script {
        game.execute(&rules, c.clone()).unwrap();
    }
    (game, rules, script)
}

#[test]
fn same_snapshot_same_commands_same_hash() {
    // DoD §54/§55 item 7: deterministic replay.
    let (g1, _, script) = scripted_game(7);
    let h1 = authoritative_hash(&g1);
    // The second copy: the same seed + the same script.
    let rules = compile_rules(SourceRules::default()).unwrap();
    let cfg = ScenarioConfig {
        seed: 7,
        players: 2,
        width: 16,
        height: 16,
        cities: 3,
        units: 6,
        water_ratio: 0,
    };
    let mut g2 = crate::scenario::generate(&cfg, &rules);
    for c in &script {
        g2.execute(&rules, c.clone()).unwrap();
    }
    assert_eq!(h1, authoritative_hash(&g2));
    g1.validate(&rules).unwrap();
    g2.validate(&rules).unwrap();
}

#[test]
fn invalid_command_changes_nothing() {
    // DoD §54: a rejected command leaves the world unchanged.
    let (mut game, rules, _) = scripted_game(7);
    let before = authoritative_hash(&game);
    let bad_unit = crate::ids::UnitHandle {
        index: 9999,
        generation: 0,
    };
    let r1 = game.execute(
        &rules,
        Command::MoveUnit {
            unit: bad_unit,
            destination: TileId(5),
        },
    );
    assert!(r1.is_err());
    let r2 = game.execute(
        &rules,
        Command::MoveUnit {
            unit: game.view().auth.indexes().units_of(PlayerId(0))[0],
            destination: TileId(u32::MAX),
        },
    );
    assert!(r2.is_err());
    assert_eq!(before, authoritative_hash(&game));
    game.validate(&rules).unwrap();
}

#[test]
fn replay_log_carries_hash_checkpoints() {
    // Replay structure: entries + periodic hashes reproduce the same final.
    let rules = compile_rules(SourceRules::default()).unwrap();
    let cfg = ScenarioConfig {
        seed: 11,
        players: 2,
        width: 16,
        height: 16,
        cities: 2,
        units: 4,
        water_ratio: 0,
    };
    let game = crate::scenario::generate(&cfg, &rules);
    let initial = crate::persistence::encode::encode(&game, &rules);
    let units = game.view().auth.indexes().units_of(PlayerId(0)).to_vec();
    let mut replay = Replay {
        initial,
        entries: vec![],
    };
    let mut live = game;
    for u in units.iter().take(2) {
        // A neighbouring tile: guaranteed within the turn's margin.
        let cur = live.view().auth.units().get(*u).unwrap().core.tile.0;
        let cmd = Command::MoveUnit {
            unit: *u,
            destination: TileId(if cur % 16 < 15 { cur + 1 } else { cur - 1 }),
        };
        // Only Ok goes into the log.
        if live.execute(&rules, cmd.clone()).is_ok() {
            let h = authoritative_hash(&live);
            replay.entries.push(ReplayEntry {
                command: cmd,
                hash_after: Some(h),
            });
        }
    }
    // At least one turn must reach the log (otherwise the test is vacuous).
    assert!(!replay.entries.is_empty());
    assert!(replay.entries.iter().all(|e| e.hash_after.is_some()));
    live.validate(&rules).unwrap();
    let _ = UnitKindId(0);
}

#[test]
fn replay_from_save_reproduces_final_hash() {
    // Full cycle §27: initial snapshot -> decode -> command log -> same hash.
    // It used to run into the decode stub; now decode is real.
    let rules = compile_rules(SourceRules::default()).unwrap();
    let cfg = ScenarioConfig {
        seed: 13,
        players: 2,
        width: 16,
        height: 16,
        cities: 2,
        units: 4,
        water_ratio: 0,
    };
    let game = crate::scenario::generate(&cfg, &rules);
    let initial = crate::persistence::encode::encode(&game, &rules);
    let units = game.view().auth.indexes().units_of(PlayerId(0)).to_vec();
    let mut live = game;
    let mut entries = vec![];
    for u in units.iter().take(2) {
        let cur = live.view().auth.units().get(*u).unwrap().core.tile.0;
        let cmd = Command::MoveUnit {
            unit: *u,
            destination: TileId(if cur % 16 < 15 { cur + 1 } else { cur - 1 }),
        };
        if live.execute(&rules, cmd.clone()).is_ok() {
            entries.push(ReplayEntry {
                command: cmd,
                hash_after: Some(authoritative_hash(&live)),
            });
        }
    }
    assert!(!entries.is_empty(), "replay without entries proves nothing");
    let expected = authoritative_hash(&live);
    // The second branch: load(initial) + the same log.
    let mut fork = crate::persistence::decode::decode(&initial, &rules).unwrap();
    for e in &entries {
        fork.execute(&rules, e.command.clone()).unwrap();
        assert_eq!(Some(authoritative_hash(&fork)), e.hash_after);
    }
    assert_eq!(expected, authoritative_hash(&fork));
}
