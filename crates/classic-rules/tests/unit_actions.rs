//! The action list of one unit (`knowledge::actions`): what the rules accept
//! for a settler, a warrior and a worker, the list's invariants (every entry
//! accepted, codec order, no duplicates) and what `enumerated` covers.

use classic_content::core::core_rules;
use classic_rules::ids::{ImprovementId, PlayerId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::knowledge::actions::{enumerated, unit_actions};
use classic_rules::knowledge::observation::LocalObservation;
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::replay::codec::CommandField;

const US: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);
/// The codes of the map `GameState::new` builds (not the rules' list order).
const GRASSLAND: u16 = 1;
const RADIUS: i32 = 4;

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    let full = format!("core:unit/{key}");
    UnitKindId(rules.units.iter().position(|u| u.key == full).unwrap() as u16)
}

fn improvement(rules: &CompiledRuleset, key: &str) -> ImprovementId {
    let full = format!("core:improvement/{key}");
    ImprovementId(rules.improvements.iter().position(|i| i.key == full).unwrap() as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    let full = format!("core:tech/{key}");
    TechId(rules.technologies.iter().position(|t| t.key == full).unwrap() as u16)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// Open grassland 30 × 20, a city for each side, the two met and everybody
/// knowing the map.
fn world(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 });
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    let tiles: Vec<TileId> = land.iter().map(|(t, _)| *t).collect();
    game.debug_clear_features(rules, &tiles);
    game.debug_set_river_edges(rules, &[]);
    game.debug_found_city(rules, US, tile(&game, 2, 10));
    game.debug_found_city(rules, THEM, tile(&game, 26, 10));
    game.debug_meet(US, THEM);
    game.debug_reveal(US, &tiles);
    game
}

fn actions(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle) -> Vec<Command> {
    let (near, _) = game.ai_local_observation(rules, US, unit, RADIUS).expect("own unit");
    unit_actions(&near)
}

fn near<'a>(game: &'a GameState, rules: &'a CompiledRuleset, unit: UnitHandle) -> LocalObservation<'a> {
    game.ai_local_observation(rules, US, unit, RADIUS).expect("own unit").0
}

fn count(list: &[Command], f: impl Fn(&Command) -> bool) -> usize {
    list.iter().filter(|c| f(c)).count()
}

/// The journal's bytes of a command (its codec tag first).
fn encoded(game: &GameState, command: &Command) -> Vec<u8> {
    let mut out = Vec::new();
    command.put(game.view().auth, &mut out);
    out
}

/// Every listed command is accepted, the list follows the codec tags and
/// holds no command twice.
fn assert_invariants(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle) {
    let near = near(game, rules, unit);
    let list = unit_actions(&near);
    assert!(!list.is_empty());
    for command in &list {
        assert!(near.view.accepts(command), "{command:?}: {:?}", near.view.refusal(command));
        assert!(enumerated(command), "{command:?}");
    }
    let tags: Vec<u8> = list.iter().map(|c| encoded(game, c)[0]).collect();
    assert!(tags.windows(2).all(|w| w[0] <= w[1]), "not in codec order: {tags:?}");
    let mut bytes: Vec<Vec<u8>> = list.iter().map(|c| encoded(game, c)).collect();
    let before = bytes.len();
    bytes.sort();
    bytes.dedup();
    assert_eq!(bytes.len(), before, "duplicates in {list:?}");
}

/// A settler on open land may found a city and move; it cannot attack or
/// build.
#[test]
fn a_settler_founds_or_moves() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules);
    let settler = game.debug_spawn_unit(&rules, US, tile(&game, 8, 8), unit_kind(&rules, "settler"));
    let list = actions(&game, &rules, settler);
    assert_eq!(count(&list, |c| matches!(c, Command::FoundCity { .. })), 1, "{list:?}");
    assert!(count(&list, |c| matches!(c, Command::MoveUnit { .. })) >= 1, "{list:?}");
    let attack = |c: &Command| matches!(c, Command::AttackUnit { .. } | Command::AttackCity { .. } | Command::RangedAttack { .. });
    assert_eq!(count(&list, attack), 0, "{list:?}");
    assert_eq!(count(&list, |c| matches!(c, Command::BuildImprovement { .. })), 0, "{list:?}");
    assert_invariants(&game, &rules, settler);
}

/// A warrior next to a foreign unit attacks it at war only.
#[test]
fn a_warrior_attacks_an_adjacent_enemy_at_war_only() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules);
    let warrior = unit_kind(&rules, "warrior");
    let ours = game.debug_spawn_unit(&rules, US, tile(&game, 10, 10), warrior);
    let theirs = game.debug_spawn_unit(&rules, THEM, tile(&game, 11, 10), warrior);
    let attacks = |list: &[Command]| count(list, |c| matches!(c, Command::AttackUnit { defender, .. } if *defender == theirs));
    let peace = actions(&game, &rules, ours);
    assert_eq!(attacks(&peace), 0, "{peace:?}");
    game.debug_force_war(&rules, US, THEM);
    let war = actions(&game, &rules, ours);
    assert_eq!(attacks(&war), 1, "{war:?}");
    assert_invariants(&game, &rules, ours);
}

/// A worker on an own tile builds a farm with its technology only, and a
/// road where there is none.
#[test]
fn a_worker_builds_what_its_technologies_allow() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules);
    let farm = improvement(&rules, "farm");
    let farming = rules.improvements[farm.0 as usize].required_tech.expect("the farm needs a technology");
    assert!(!game.view().auth.players().techs_of(US).contains(&farming), "the player starts without it");
    let worker = game.debug_spawn_unit(&rules, US, tile(&game, 3, 10), unit_kind(&rules, "worker"));
    let farms = |list: &[Command]| count(list, |c| matches!(c, Command::BuildImprovement { improvement, .. } if *improvement == farm));
    let before = actions(&game, &rules, worker);
    assert_eq!(farms(&before), 0, "{before:?}");
    game.debug_learn(&rules, US, farming);
    game.debug_learn(&rules, US, tech(&rules, "the-wheel"));
    let after = actions(&game, &rules, worker);
    assert_eq!(farms(&after), 1, "{after:?}");
    assert_eq!(count(&after, |c| matches!(c, Command::BuildRoad { .. })), 1, "{after:?}");
    assert_invariants(&game, &rules, worker);
}

/// `enumerated` names the kinds the list can hold: not a march, not a
/// player's command.
#[test]
fn enumerated_names_the_listed_kinds() {
    let unit = UnitHandle { index: 0, generation: 0 };
    assert!(enumerated(&Command::MoveUnit { unit, destination: TileId(0) }));
    assert!(enumerated(&Command::Fortify { unit }));
    assert!(!enumerated(&Command::MarchTo { unit, destination: TileId(0) }));
    assert!(!enumerated(&Command::EndTurn { player: US }));
}

/// How long one list takes (read with `--nocapture`; not an assertion).
#[test]
fn timing_of_one_list() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules);
    game.debug_learn(&rules, US, tech(&rules, "agriculture"));
    game.debug_learn(&rules, US, tech(&rules, "the-wheel"));
    let warrior = game.debug_spawn_unit(&rules, US, tile(&game, 10, 10), unit_kind(&rules, "warrior"));
    game.debug_spawn_unit(&rules, THEM, tile(&game, 11, 10), unit_kind(&rules, "warrior"));
    game.debug_force_war(&rules, US, THEM);
    let worker = game.debug_spawn_unit(&rules, US, tile(&game, 3, 10), unit_kind(&rules, "worker"));
    for (name, unit) in [("warrior", warrior), ("worker", worker)] {
        let near = near(&game, &rules, unit);
        let _warm = unit_actions(&near);
        let runs = 20;
        let start = std::time::Instant::now();
        let mut listed = 0;
        for _ in 0..runs {
            listed = unit_actions(&near).len();
        }
        let micros = start.elapsed().as_micros() / runs;
        eprintln!("unit_actions({name}): {listed} commands, {micros} us");
    }
}
