use crate::ids::*;
use crate::derived::movement::*;
use crate::ids::UnitKindId;
use crate::math::HexCoord;
use crate::rules::SourceRules;
use crate::world::state::GameConfig;
use crate::{compile_rules, Command, GameState};

#[test]
fn occupancy_tracks_spawn_move_kill() {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 0,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 2, r: 2 }).unwrap();
    let t1 = dims.to_tile(HexCoord { q: 3, r: 2 }).unwrap();
    let h = game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
    let mv = &game.derived().movement;
    assert_eq!(mv.units_of(PlayerId(0), t0), 1);
    assert_eq!(mv.military_of(PlayerId(0), t0), 1);
    assert_eq!(mv.units_of(PlayerId(1), t0), 0);
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: h,
            destination: t1,
        },
    )
    .unwrap();
    let mv = &game.derived().movement;
    assert_eq!(mv.units_of(PlayerId(0), t0), 0);
    assert_eq!(mv.units_of(PlayerId(0), t1), 1);
    // Oracle after every operation: via validate (includes movement).
    game.validate(&rules).unwrap();
}

#[test]
fn incremental_matches_rebuild_under_churn() {
    // Spawns, moves, deaths (via FoundCity-consume): the oracle holds.
    // Content with a founder kind: FoundCity gating needs a real kind.
    // A fixture from RON (no hardcoded tables).
    let rules = crate::fixtures::arena_content();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 3,
        },
    );
    let dims = game.view().auth.map().dimensions;
    for i in 0..10 {
        let t = dims.to_tile(HexCoord { q: i, r: 0 }).unwrap();
        game.debug_spawn_unit(&rules, PlayerId((i % 2) as u8), t, UnitKindId(0));
    }
    game.validate(&rules).unwrap();
    let units = game.view().auth.indexes().units_of(PlayerId(0)).to_vec();
    for u in units.iter().take(3) {
        let cur = game.view().auth.units().get(*u).unwrap().core.tile.0;
        game.execute(
            &rules,
            Command::MoveUnit {
                unit: *u,
                destination: TileId(cur + 16),
            },
        )
        .unwrap();
    }
    game.validate(&rules).unwrap();
    // Absorption of a settler by a city: the Destroyed path of the cache.
    crate::fixtures::play_until_active(&mut game, &rules, PlayerId(1));
    let settler = game.view().auth.indexes().units_of(PlayerId(1))[0];
    game.execute(&rules, Command::FoundCity { settler })
        .unwrap();
    game.validate(&rules).unwrap();
    let (fresh, _) = MovementState::rebuild_counted(game.view().auth, &rules);
    assert_eq!(game.derived().movement, fresh);
}
