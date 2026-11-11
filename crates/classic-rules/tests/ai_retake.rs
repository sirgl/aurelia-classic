//! The retake of a lost city by `retake2` (`ai::retake`, classic/docs/ai-retake.md): the seat finds the city
//! by its own memory (a city held at its last turn and gone now), reads the city's health and the units around
//! it only while they are in sight, refuses peace only while a case stands, and does not attack a city whose
//! captor has more at hand than the group can beat.

use classic_content::core::core_rules;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::retake::{self, Lost, Memory};
use classic_rules::ai::state::AiState;
use classic_rules::ai::strategy_net;
use classic_rules::ids::{PlayerId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameConfig, GameState};

const US: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);
const THIRD: PlayerId = PlayerId(2);
const GRASSLAND: u16 = 1;
/// The window of the lost city in rounds (`operations::march_limit` of a brief without `war.march`).
const WINDOW: u32 = 20;
const CITY: (i32, i32) = (12, 10);

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    let full = format!("core:unit/{key}");
    UnitKindId(rules.units.iter().position(|u| u.key == full).unwrap() as u16)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// What stands on the map: who holds the city at `CITY`, how hurt it is, the seat's swordsmen around it and
/// the holder's units in the open.
struct Scene {
    holder: PlayerId,
    damage: u16,
    ours: Vec<(i32, i32)>,
    guards: Vec<(i32, i32)>,
}

fn scene(rules: &CompiledRuleset, s: &Scene) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 3, width: 30, height: 20, rng_seed: 5 });
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    let tiles: Vec<TileId> = land.iter().map(|(t, _)| *t).collect();
    game.debug_clear_features(rules, &tiles);
    game.debug_set_river_edges(rules, &[]);
    game.debug_found_city(rules, US, tile(&game, 3, 10));
    game.debug_found_city(rules, THEM, tile(&game, 27, 10));
    game.debug_found_city(rules, THIRD, tile(&game, 27, 2));
    let city = game.debug_found_city(rules, s.holder, tile(&game, CITY.0, CITY.1));
    game.debug_set_city_damage(city, s.damage);
    for (a, b) in [(US, THEM), (US, THIRD), (THEM, THIRD)] {
        game.debug_meet(a, b);
        game.debug_force_war(rules, a, b);
    }
    let sword = unit_kind(rules, "swordsman");
    for (q, r) in &s.ours {
        game.debug_spawn_unit(rules, US, tile(&game, *q, *r), sword);
    }
    for (q, r) in &s.guards {
        game.debug_spawn_unit(rules, s.holder, tile(&game, *q, *r), sword);
    }
    game
}

/// Four swordsmen next to the city, on its east side.
fn group() -> Vec<(i32, i32)> {
    vec![(13, 10), (13, 9), (13, 11), (12, 8)]
}

/// The seat's memory at its last turn: it held its capital and the city.
fn memory_of_the_last_turn(game: &GameState) -> AiState {
    let mut state = AiState::new(US, 1);
    let held: Vec<TileId> = vec![tile(game, 3, 10), tile(game, CITY.0, CITY.1)];
    state.mind_memory = strategy_net::Memory { retake: Some(Memory { held, lost: Vec::new() }), ..Default::default() }.encode();
    state
}

fn remembered(state: &AiState) -> Memory {
    strategy_net::Memory::decode(&state.mind_memory).retake.expect("the section is there")
}

fn retaking(rules: &CompiledRuleset, game: &GameState, state: &mut AiState) -> (TurnBrief, Vec<retake::Case>) {
    let obs = game.ai_observation(rules, US);
    let mut brief = TurnBrief::from_observation(&obs, rules);
    brief.retake = true;
    brief.retake2 = true;
    let cases = retake::cases_remembered(&obs, &brief, state, rules);
    brief.retaking = cases.clone();
    (brief, cases)
}

/// The city falls in the seat's sight: the case opens with the health the bar shows, the loss and its taker
/// are kept, and the group is the units that stand near.
#[test]
fn a_case_opens_when_the_seats_city_falls_in_sight_with_its_real_health() {
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules, &Scene { holder: THEM, damage: 190, ours: group(), guards: vec![] });
    let mut state = memory_of_the_last_turn(&game);
    let (_, cases) = retaking(&rules, &game, &mut state);
    assert_eq!(cases.len(), 1, "one case: the city");
    let case = &cases[0];
    assert_eq!((case.tile, case.enemy), (tile(&game, CITY.0, CITY.1), THEM));
    let obs = game.ai_observation(&rules, US);
    let seen = obs.visible_foreign_cities.iter().find(|c| c.tile == case.tile).expect("the city is in sight");
    assert_eq!(case.hp, seen.hp, "the case reads the health the bar shows");
    assert!(case.hp < 100, "a city left at {} hit points", case.hp);
    assert_eq!(case.group.len(), group().len());
    let memory = remembered(&state);
    assert_eq!(memory.lost, vec![Lost { tile: case.tile, round: obs.round, captor: Some(THEM) }], "the loss, its round and its taker");
}

/// A city that is out of sight is still known to be lost (the seat always knows its own city is gone), but
/// nothing of it is read: no case, and the taker is not known yet.
#[test]
fn a_city_lost_out_of_sight_opens_no_case_and_names_no_taker() {
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules, &Scene { holder: THEM, damage: 190, ours: vec![(3, 11)], guards: vec![] });
    let mut state = memory_of_the_last_turn(&game);
    let (_, cases) = retaking(&rules, &game, &mut state);
    assert!(cases.is_empty());
    let memory = remembered(&state);
    assert_eq!(memory.lost.len(), 1);
    assert_eq!(memory.lost[0].captor, None);
}

/// A city taken from the seat and then by a third player from the captor is not the seat's case against the
/// third player: the memory says who took it from the seat.
#[test]
fn a_city_taken_again_by_a_third_player_opens_no_case() {
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules, &Scene { holder: THIRD, damage: 190, ours: group(), guards: vec![] });
    let mut state = memory_of_the_last_turn(&game);
    let city = tile(&game, CITY.0, CITY.1);
    let mut kept = strategy_net::Memory::decode(&state.mind_memory);
    kept.retake = Some(Memory { held: vec![tile(&game, 3, 10)], lost: vec![Lost { tile: city, round: 1, captor: Some(THEM) }] });
    state.mind_memory = kept.encode();
    let (_, cases) = retaking(&rules, &game, &mut state);
    assert!(cases.is_empty(), "the holder is not the one who took it from us");
    assert!(remembered(&state).lost.is_empty(), "the memory lets the case go");
}

/// The window and the changes of hands, round by round, on the memory alone.
#[test]
fn the_memory_keeps_a_lost_city_for_the_window_only_and_lets_it_go_when_it_changes_hands() {
    let (a, b, c) = (TileId(10), TileId(20), TileId(30));
    let mut memory = Memory { held: vec![a, b, c], lost: Vec::new() };
    // Round 40: the seat finds b gone, and sees it held by another player.
    memory.observe(40, WINDOW, &[a, c], &[(b, THEM)]);
    assert_eq!(memory.lost, vec![Lost { tile: b, round: 40, captor: Some(THEM) }]);
    assert_eq!(memory.held, vec![a, c]);
    // Later, the same player holds it: the case stays for the window.
    memory.observe(40 + WINDOW, WINDOW, &[a, c], &[(b, THEM)]);
    assert_eq!(memory.lost.len(), 1);
    // Past the window it goes: a city lost long ago is no case.
    memory.observe(41 + WINDOW, WINDOW, &[a, c], &[(b, THEM)]);
    assert!(memory.lost.is_empty());
    // Taken by a third player: gone from the memory at once.
    let mut memory = Memory { held: vec![a, b], lost: Vec::new() };
    memory.observe(10, WINDOW, &[a], &[(b, THEM)]);
    memory.observe(12, WINDOW, &[a], &[(b, THIRD)]);
    assert!(memory.lost.is_empty());
    // Taken back by the seat: gone.
    let mut memory = Memory { held: vec![a, b], lost: Vec::new() };
    memory.observe(10, WINDOW, &[a], &[]);
    assert_eq!(memory.lost, vec![Lost { tile: b, round: 10, captor: None }]);
    memory.observe(11, WINDOW, &[a, b], &[]);
    assert!(memory.lost.is_empty());
    // A city seen for the first time after the loss gets its holder then.
    let mut memory = Memory { held: vec![a, b], lost: Vec::new() };
    memory.observe(10, WINDOW, &[a], &[]);
    memory.observe(11, WINDOW, &[a], &[(b, THEM)]);
    assert_eq!(memory.lost[0].captor, Some(THEM));
    // A seat with no memory yet (a game loaded from an older save) finds nothing lost at its first turn.
    let mut fresh = Memory::default();
    fresh.observe(100, WINDOW, &[a, b], &[(c, THEM)]);
    assert!(fresh.lost.is_empty());
}

/// The peace refusal holds while the case stands: when the group is gone, or the city is ours again, there is
/// no case and the brief refuses nothing.
#[test]
fn the_peace_refusal_holds_only_while_the_case_stands() {
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules, &Scene { holder: THEM, damage: 190, ours: group(), guards: vec![] });
    let mut state = memory_of_the_last_turn(&game);
    let (brief, cases) = retaking(&rules, &game, &mut state);
    assert_eq!(cases.len(), 1);
    assert!(brief.retaking_from(THEM), "no peace with the holder while the city is being retaken");
    assert!(!brief.retaking_from(THIRD), "none for the others");
    assert!(brief.is_retaken(tile(&game, CITY.0, CITY.1)));
    // The group is gone: no case, no refusal.
    let game = scene(&rules, &Scene { holder: THEM, damage: 190, ours: vec![], guards: vec![] });
    let mut state = memory_of_the_last_turn(&game);
    let (brief, cases) = retaking(&rules, &game, &mut state);
    assert!(cases.is_empty());
    assert!(!brief.retaking_from(THEM));
    // The city is ours again: nothing to retake.
    let game = scene(&rules, &Scene { holder: US, damage: 0, ours: group(), guards: vec![] });
    let mut state = memory_of_the_last_turn(&game);
    let (brief, cases) = retaking(&rules, &game, &mut state);
    assert!(cases.is_empty());
    assert!(!brief.retaking_from(THEM));
    assert!(remembered(&state).lost.is_empty());
}

/// The captor's units within four tiles of the city that the seat sees weigh against the group: the same
/// four swordsmen retake the city with one of the captor's in the open and do not with eight around it.
#[test]
fn the_guard_test_refuses_the_heavily_defended_case() {
    let rules = compile_rules(core_rules()).unwrap();
    let alone = scene(&rules, &Scene { holder: THEM, damage: 190, ours: group(), guards: vec![(15, 10)] });
    let seen = alone.ai_observation(&rules, US).visible_foreign_units.iter().filter(|u| u.owner == THEM).count();
    assert_eq!(seen, 1, "the captor's unit is in sight");
    let mut state = memory_of_the_last_turn(&alone);
    let (_, cases) = retaking(&rules, &alone, &mut state);
    assert_eq!(cases.len(), 1, "one unit of the captor in the open is a fight the group takes");

    let guards = vec![(14, 10), (14, 9), (14, 11), (15, 9), (15, 10), (15, 11), (14, 8), (14, 12)];
    let crowded = scene(&rules, &Scene { holder: THEM, damage: 190, ours: group(), guards });
    let obs = crowded.ai_observation(&rules, US);
    let in_sight = obs.visible_foreign_units.iter().filter(|u| u.owner == THEM).count();
    assert!(in_sight >= 6, "the test needs the guard in sight, {in_sight} seen");
    let mut state = memory_of_the_last_turn(&crowded);
    let (brief, cases) = retaking(&rules, &crowded, &mut state);
    assert!(cases.is_empty(), "the group is outmatched by what stands around the city");
    assert!(!brief.retaking_from(THEM), "no case, no refusal of peace");
    assert_eq!(remembered(&state).lost.len(), 1, "the city stays remembered as lost");
}

/// The comparison itself: the city's strikes take from the group, the margin scales the guard.
#[test]
fn the_group_is_outmatched_by_what_the_city_and_the_guard_have() {
    // Power 400 over 400 hit points, a city that takes 30 a round for 2 rounds: 340 left.
    assert!(!retake::outmatched(400, 400, 30, 2, 340, 100));
    assert!(retake::outmatched(400, 400, 30, 2, 341, 100));
    // A margin of 200 percent needs twice the guard's power.
    assert!(retake::outmatched(400, 400, 0, 2, 201, 200));
    assert!(!retake::outmatched(400, 400, 0, 2, 200, 200));
    // No guard: never outmatched; a city that kills the whole group: outmatched by any guard.
    assert!(!retake::outmatched(0, 100, 0, 1, 0, 100));
    assert!(retake::outmatched(100, 100, 100, 1, 1, 100));
}

/// The memory is versioned: the section is after the strategy memory's, a memory without one keeps its
/// bytes, older bytes and bytes of another version load as no memory, the rest of the memory is not touched.
#[test]
fn the_memory_is_versioned_and_leaves_the_other_variants_bytes_alone() {
    let plain = strategy_net::Memory::default();
    let before = plain.encode();
    let with = strategy_net::Memory { retake: Some(Memory { held: vec![TileId(7), TileId(9)], lost: vec![Lost { tile: TileId(4), round: 12, captor: Some(THEM) }] }), ..Default::default() };
    let bytes = with.encode();
    assert!(bytes.starts_with(&before), "the section comes after the bytes the other variants write");
    assert_eq!(strategy_net::Memory::decode(&bytes), with);
    assert_eq!(strategy_net::Memory::decode(&before).retake, None, "a save from before the section loads without it");
    assert_eq!(strategy_net::Memory::decode(&before).encode(), before);
    // The body of another version is no memory; the strategy memory around it still loads.
    let mut other = bytes.clone();
    other[before.len() + 3] = 99;
    assert_eq!(strategy_net::Memory::decode(&other).retake, None);
    let one = Memory { held: vec![TileId(1)], lost: vec![] };
    assert_eq!(Memory::decode(&one.encode()), Some(one));
    assert_eq!(Memory::decode(&[]), None);
    assert_eq!(Memory::decode(&[1, 3]), None, "a truncated body");
}
