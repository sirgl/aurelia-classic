//! Shared fixtures of the system tests (public API only: GameState,
//! commands, content compiled from RON).

pub(crate) use crate::command::Command;
pub(crate) use crate::fixtures::{play_round, play_until_active};
pub(crate) use crate::ids::{PlayerId, TechId, TileId, UnitKindId};
pub(crate) use crate::math::HexCoord;
pub(crate) use crate::replay::authoritative_hash;
pub(crate) use crate::rules::source::SourceRules;
pub(crate) use crate::rules::CompiledRuleset;
pub(crate) use crate::world::state::GameConfig;
pub(crate) use crate::{compile_rules, GameState};

pub(crate) fn arena() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 99,
        },
    );
    // The arena's players know each other (war needs contact).
    game.debug_meet_all();
    (game, rules)
}

pub(crate) fn writing_id(rules: &CompiledRuleset) -> crate::ids::TechId {
    rules
        .technologies
        .iter()
        .position(|t| t.key == "core:tech/writing")
        .map(|i| crate::ids::TechId(i as u16))
        .expect("writing in test content")
}

pub(crate) fn arena_content() -> (GameState, CompiledRuleset) {
    let rules = crate::fixtures::arena_content();
    assert_eq!(rules.units[0].key, "core:unit/settler");
    assert_eq!(rules.units[1].key, "core:unit/warrior");
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 99,
        },
    );
    // The arena's players know each other (war needs contact).
    game.debug_meet_all();
    (game, rules)
}

/// The arena plus one building `core:building/<slug>` (BuildingKindId(0))
/// whose City-scope effect is `modifier` (`AddYield(yield_kind: Culture,
/// amount: 1)`, ...): tests of one yield channel build it into a city.
pub(crate) fn arena_with_building(slug: &str, modifier: &str) -> (GameState, CompiledRuleset) {
    let building = format!(
        "buildings: [(id: \"core:building/{slug}\", cost: 40, effects: [(scope: City, selector: SourceCity, modifier: {modifier})])],\n    units: ["
    );
    let text = crate::fixtures::ARENA_RON.replacen("units: [", &building, 1);
    let rules = crate::fixtures::compile_ron(&text);
    let game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 99,
        },
    );
    (game, rules)
}

pub(crate) fn adjacent_pair(
    game: &mut GameState,
    rules: &CompiledRuleset,
) -> (crate::ids::UnitHandle, crate::ids::UnitHandle) {
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 4, r: 4 }).unwrap();
    let t1 = dims.to_tile(HexCoord { q: 5, r: 4 }).unwrap();
    let a = game.debug_spawn_unit(rules, PlayerId(0), t0, UnitKindId(0));
    let d = game.debug_spawn_unit(rules, PlayerId(1), t1, UnitKindId(0));
    (a, d)
}

pub(crate) fn pottery_id(rules: &CompiledRuleset) -> crate::ids::TechId {
    rules
        .technologies
        .iter()
        .position(|t| t.key == "core:tech/pottery")
        .map(|i| crate::ids::TechId(i as u16))
        .expect("pottery in test content")
}

/// A chain as in the real content: agriculture -> pottery -> writing.
pub(crate) fn agriculture_id(rules: &CompiledRuleset) -> crate::ids::TechId {
    rules
        .technologies
        .iter()
        .position(|t| t.key == "core:tech/agriculture")
        .map(|i| crate::ids::TechId(i as u16))
        .expect("agriculture in test content")
}

/// Run turns until the player learns the tech (A2: research goes
/// by beakers, not instantly). Returns the number of turns (deterministic).
pub(crate) fn research_until(
    game: &mut GameState,
    rules: &CompiledRuleset,
    player: PlayerId,
    tech: TechId,
) -> u32 {
    let mut turns = 0u32;
    for _ in 0..200 {
        if game.view().auth.players().techs_of(player).contains(&tech) {
            break;
        }
        game.execute(&rules, Command::EndTurn { player }).unwrap();
        turns += 1;
    }
    assert!(
        game.view().auth.players().techs_of(player).contains(&tech),
        "tech {:?} must complete",
        tech
    );
    turns
}

pub(crate) fn worked_set_tiles(
    game: &GameState,
    rules: &CompiledRuleset,
    city: crate::ids::CityHandle,
) -> Vec<TileId> {
    let core = game.view().auth.cities().get(city).unwrap();
    crate::derived::yields::city_work(game.view().auth, rules, core)
        .tiles()
        .collect()
}

/// Ring n around the centre (a deterministic traversal order).
pub(crate) fn ring_tile(
    game: &GameState,
    center: crate::ids::TileId,
    ring: i32,
) -> crate::ids::TileId {
    let dims = game.view().auth.map().dimensions;
    crate::math::tiles_in_hex_radius(&dims, center, ring)
        .into_iter()
        .find(|t| {
            dims.to_coord(*t)
                .zip(dims.to_coord(center))
                .map(|(a, b)| a.distance(b) == ring)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("ring {ring} must have in-map tiles"))
}
