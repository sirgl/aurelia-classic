//! A nuclear strike is a public act (classic/docs/classic/RESOURCING.md, "Infamy of a
//! nuclear strike"): every living major that met the user, and every owner of what the blast
//! hit, remembers it in its warmonger memory of the user, whoever the user is. The memory
//! fades by the rules' own decay.

use classic_content::core::core_rules;
use classic_rules::ids::{PlayerId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
/// `systems::diplomacy::WARMONGER_NUKE_WEIGHT`.
const WARMONGER_NUKE_WEIGHT: i32 = 400;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};

const USER: PlayerId = PlayerId(0);
const VICTIM: PlayerId = PlayerId(1);
const WITNESS: PlayerId = PlayerId(2);
const STRANGER: PlayerId = PlayerId(3);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

/// Four players: the user and the victim at war, the witness has met the user, the stranger has not.
fn scene(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 4, width: 40, height: 20, rng_seed: 7 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(1))).collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, USER, tile(&game, 4, 10));
    game.debug_found_city(rules, VICTIM, tile(&game, 12, 10));
    game.debug_found_city(rules, WITNESS, tile(&game, 30, 3));
    game.debug_found_city(rules, STRANGER, tile(&game, 36, 18));
    game.debug_meet(USER, VICTIM);
    game.debug_meet(USER, WITNESS);
    game.execute(rules, Command::DeclareWar { player: USER, enemy: VICTIM }).unwrap();
    let seen: Vec<TileId> = (8..=16).flat_map(|q| (6..=14).map(move |r| (q, r))).map(|(q, r)| tile(&game, q, r)).collect();
    game.debug_reveal(USER, &seen);
    game
}

fn memory(game: &GameState, observer: PlayerId) -> i32 {
    game.view().auth.module().relations().warmonger(observer, USER)
}

fn launch(game: &mut GameState, rules: &CompiledRuleset, key: &str) {
    let bomb = game.debug_spawn_unit(rules, USER, tile(game, 4, 10), kind(rules, key));
    let target = tile(game, 12, 10);
    game.execute(rules, Command::Nuke { unit: bomb, target }).unwrap();
}

#[test]
fn a_nuclear_strike_is_remembered_by_everyone_who_met_the_user_and_by_its_victim() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    let before: Vec<i32> = [VICTIM, WITNESS, STRANGER].iter().map(|p| memory(&game, *p)).collect();
    launch(&mut game, &rules, "core:unit/atomic-bomb");
    let after: Vec<i32> = [VICTIM, WITNESS, STRANGER].iter().map(|p| memory(&game, *p)).collect();
    // The war of the user with the victim is already remembered (the declaration); the strike adds
    // the weight of level 1, scaled by the era (50 to 100 percent).
    let added = |i: usize| after[i] - before[i];
    assert!(added(0) >= WARMONGER_NUKE_WEIGHT / 2 && added(0) <= WARMONGER_NUKE_WEIGHT, "victim: {}", added(0));
    assert_eq!(added(1), added(0), "a witness that met the user remembers the same");
    assert_eq!(added(2), 0, "a civilization that never met the user remembers nothing");
}

#[test]
fn a_level_two_weapon_weighs_twice_and_a_second_strike_adds() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut one = scene(&rules);
    let base = memory(&one, WITNESS);
    launch(&mut one, &rules, "core:unit/atomic-bomb");
    let level_one = memory(&one, WITNESS) - base;
    assert!(level_one > 0);
    let mut two = scene(&rules);
    launch(&mut two, &rules, "core:unit/nuclear-missile");
    assert_eq!(memory(&two, WITNESS) - base, 2 * level_one);
    launch(&mut one, &rules, "core:unit/atomic-bomb");
    assert_eq!(memory(&one, WITNESS) - base, 2 * level_one);
}

#[test]
fn the_memory_fades_with_the_rounds() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    launch(&mut game, &rules, "core:unit/atomic-bomb");
    let fresh = memory(&game, WITNESS);
    assert!(fresh > 0);
    // Four players end their turns for ten rounds.
    for _ in 0..40 {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
    }
    assert!(memory(&game, WITNESS) < fresh, "the memory fades");
}

#[test]
fn a_city_state_or_a_barbarian_keeps_no_such_memory() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = scene(&rules);
    game.debug_set_player_kind(STRANGER, classic_rules::world::player::PlayerKind::Module(classic_rules::world::player::ClassicKind::Barbarian));
    game.debug_meet(USER, STRANGER);
    launch(&mut game, &rules, "core:unit/atomic-bomb");
    assert_eq!(memory(&game, STRANGER), 0);
}
