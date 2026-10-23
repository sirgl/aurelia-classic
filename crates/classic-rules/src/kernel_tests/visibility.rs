use crate::metrics::WorkCounters;
use crate::transaction::ChangeJournal;
use crate::CompiledRuleset;
use crate::derived::visibility::*;
use crate::ids::{PlayerId, UnitKindId};
use crate::rules::SourceRules;
use crate::world::state::GameConfig;
use crate::{compile_rules, Command, GameState};

fn setup() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 0,
        },
    );
    (game, rules)
}

#[test]
fn incremental_move_matches_rebuild() {
    let (mut game, rules) = setup();
    let dims = game.view().auth.map().dimensions;
    let center = dims.to_tile(crate::math::HexCoord { q: 8, r: 8 }).unwrap();
    let dest = dims.to_tile(crate::math::HexCoord { q: 10, r: 8 }).unwrap();
    let h = game.debug_spawn_unit(&rules, PlayerId(0), center, UnitKindId(0));
    game.validate(&rules).unwrap();
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: h,
            destination: dest,
        },
    )
    .unwrap();
    // Oracle: the increment after execute must match a full rebuild.
    game.validate(&rules).unwrap();
}

#[test]
fn local_move_work_bound() {
    let (mut game, rules) = setup();
    let dims = game.view().auth.map().dimensions;
    let center = dims.to_tile(crate::math::HexCoord { q: 8, r: 8 }).unwrap();
    let dest = dims.to_tile(crate::math::HexCoord { q: 9, r: 8 }).unwrap();
    let h = game.debug_spawn_unit(&rules, PlayerId(0), center, UnitKindId(0));
    // Snapshot BEFORE the move: the delta is applied to the pre-move rebuild (the test used to
    // wrongly apply it to the post-move state; saturating hid it,
    // checked per §62 caught it; the test got stricter: exact equality with rebuild).
    let game_before = game.clone();
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: h,
            destination: dest,
        },
    )
    .unwrap();
    // A direct delta measurement on the pre-move copy:
    let mut vis = VisibilityState::rebuild(game_before.view().auth, &rules);
    let journal = {
        let mut j = ChangeJournal::default();
        j.unit_changes
            .push(crate::transaction::journal::UnitChange::Moved {
                unit: h,
                from: center,
                to: dest,
            });
        j
    };
    let counters =
        VisibilityState::apply_changes_counted(&mut vis, &journal, game.view().auth, &rules);
    crate::metrics::assert_local_move_bounds(&counters, 256);
    assert!(counters.tiles_visited <= 2 * SIGHT_AREA);
    // And the result is exactly equal to the post-move rebuild.
    let fresh = VisibilityState::rebuild(game.view().auth, &rules);
    assert_eq!(vis.counts, fresh.counts);
}

/// Regression (turn spikes): every tile change — a border tile claimed
/// by culture, a road, an improvement — rebuilt the whole visibility of
/// every player, so an end turn with border growth cost a full rebuild
/// (milliseconds with thousands of units). An owner change moves only
/// that tile's own sight (one disc of radius 1); a road does no
/// visibility work at all.
#[test]
fn tile_changes_without_terrain_do_no_visibility_work() {
    let (mut game, rules) = setup();
    let dims = game.view().auth.map().dimensions;
    let tile = dims.to_tile(crate::math::HexCoord { q: 8, r: 8 }).unwrap();
    game.debug_spawn_unit(&rules, PlayerId(0), tile, UnitKindId(0));
    let mut vis = VisibilityState::rebuild(game.view().auth, &rules);
    let mut journal = ChangeJournal::default();
    journal
        .tile_changes
        .push(crate::transaction::journal::TileChange::OwnerChanged {
            tile,
            from: None,
            to: Some(PlayerId(0)),
        });
    journal
        .tile_changes
        .push(crate::transaction::journal::TileChange::RoadBuilt { tile });
    let counters =
        VisibilityState::apply_changes_counted(&mut vis, &journal, game.view().auth, &rules);
    assert_eq!(counters.tiles_visited, 7, "the owned tile's own disc, no rebuild");
    assert_eq!(counters.entities_visited, 1);
    let mut roads = ChangeJournal::default();
    roads
        .tile_changes
        .push(crate::transaction::journal::TileChange::RoadBuilt { tile });
    let counters = VisibilityState::apply_changes_counted(&mut vis, &roads, game.view().auth, &rules);
    assert_eq!(counters, WorkCounters::default(), "a road does no visibility work");
}

fn count_at(game: &crate::GameState, player: usize, t: crate::ids::TileId) -> u16 {
    let v = &game.derived().visibility;
    v.counts[player * v.tiles + (t.0 as usize)]
}

#[test]
fn hill_shadows_tiles_behind_it() {
    use crate::ids::TerrainId;
    let (mut game, rules) = setup();
    let dims = game.view().auth.map().dimensions;
    let eye = dims.to_tile(crate::math::HexCoord { q: 4, r: 4 }).unwrap();
    let hill = dims.to_tile(crate::math::HexCoord { q: 5, r: 4 }).unwrap();
    let behind = dims.to_tile(crate::math::HexCoord { q: 6, r: 4 }).unwrap();
    game.debug_set_terrain_batch(&rules, &[(hill, TerrainId(2))]);
    game.debug_spawn_unit(&rules, PlayerId(0), eye, UnitKindId(0));
    game.validate(&rules).unwrap();
    // The hill is seen itself, the tile behind it is in shadow.
    assert!(count_at(&game, 0, eye) > 0);
    assert!(count_at(&game, 0, hill) > 0);
    assert_eq!(count_at(&game, 0, behind), 0);
    // And incremental converged with rebuild (the oracle via validate above).
}

/// Water lies below the observer: the far side of a bay two tiles away
/// is in sight (the former rule hid it, and the middle of every bay a
/// shore unit watched stayed remembered). A mountain blocks like a hill.
#[test]
fn water_does_not_block_sight_and_mountains_do() {
    let (mut game, rules) = setup();
    let dims = game.view().auth.map().dimensions;
    let at = |q, r| dims.to_tile(crate::math::HexCoord { q, r }).unwrap();
    let (eye, water, across) = (at(4, 4), at(5, 4), at(6, 4));
    let (mountain, shadow) = (at(4, 5), at(4, 6));
    game.debug_set_terrain_batch(&rules, &[(water, crate::fixtures::WATER), (mountain, crate::fixtures::MOUNTAINS)]);
    game.debug_spawn_unit(&rules, PlayerId(0), eye, UnitKindId(0));
    game.validate(&rules).unwrap();
    assert!(count_at(&game, 0, across) > 0, "the far side of the water is in sight");
    assert!(count_at(&game, 0, mountain) > 0, "the mountain itself is seen");
    assert_eq!(count_at(&game, 0, shadow), 0, "behind the mountain is hidden");
}

#[test]
fn city_grants_vision_and_explored() {
    // A city lights up its radius (counts) and marks explored (knowledge);
    // the same pair as for units; the oracle holds via validate.
    let (mut game, rules) = setup();
    let dims = game.view().auth.map().dimensions;
    let ct = dims.to_tile(crate::math::HexCoord { q: 8, r: 8 }).unwrap();
    let near = dims.to_tile(crate::math::HexCoord { q: 9, r: 8 }).unwrap();
    let far = dims
        .to_tile(crate::math::HexCoord { q: 14, r: 14 })
        .unwrap();
    assert_eq!(count_at(&game, 0, near), 0, "dark before founding");
    game.debug_found_city(&rules, PlayerId(0), ct);
    game.validate(&rules).unwrap();
    assert!(count_at(&game, 0, near) > 0, "city lights neighbors");
    assert_eq!(count_at(&game, 0, far), 0, "far stays dark");
    assert!(
        game.view().auth.knowledge()[0].is_explored(near),
        "explored follows city vision"
    );
    assert!(
        !game.view().auth.knowledge()[0].is_explored(far),
        "far stays unexplored"
    );
}
