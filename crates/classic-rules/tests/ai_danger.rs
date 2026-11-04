//! The danger features (`ai::danger`): a unit next to an enemy at war has a
//! threat, the blow the battlefield model gives and the search's exposure;
//! a unit out of every enemy's reach has none; the row is the documented
//! width and the same for the same sight. A unit decision's row (its local
//! observation and the turn's sight beyond) at the unit's own tile is the
//! sight's row; at a tile it may move to, the row of the unit standing there.

use classic_content::core::core_rules;
use classic_rules::ai::danger::{self, heuristic_exposure, unit_features, Place, Sight, Threats, FEATURES, NAMES, THREATS};
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ids::{PlayerId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::knowledge::observation::OwnUnit;
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameConfig, GameState};

const US: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);
/// The codes of the map `GameState::new` builds (not the rules' list order).
const GRASSLAND: u16 = 1;

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    let full = format!("core:unit/{key}");
    UnitKindId(rules.units.iter().position(|u| u.key == full).unwrap() as u16)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// Open grassland 30 × 20, a city for each side far apart, the sides at war.
fn world(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 });
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    let tiles: Vec<TileId> = land.iter().map(|(t, _)| *t).collect();
    game.debug_clear_features(rules, &tiles);
    game.debug_set_river_edges(rules, &[]);
    game.debug_found_city(rules, US, tile(&game, 2, 10));
    game.debug_found_city(rules, THEM, tile(&game, 27, 10));
    game.debug_meet(US, THEM);
    game.debug_force_war(rules, US, THEM);
    game
}

fn column(name: &str) -> usize {
    NAMES.iter().position(|n| *n == name).unwrap()
}

#[test]
fn a_unit_next_to_an_enemy_is_exposed_and_a_far_one_is_not() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules);
    let sword = unit_kind(&rules, "swordsman");
    game.debug_spawn_unit(&rules, US, tile(&game, 12, 10), sword);
    game.debug_spawn_unit(&rules, US, tile(&game, 4, 4), sword);
    game.debug_spawn_unit(&rules, THEM, tile(&game, 13, 10), sword);

    let obs = game.ai_observation(&rules, US);
    let brief = TurnBrief::from_observation(&obs, &rules);
    let threats = Threats::build(&obs, &rules, &brief, &[]);
    let own: Vec<OwnUnit> = threats.own.clone();
    assert_eq!(own.len(), 2, "both swordsmen are subjects");
    assert_eq!(threats.foes.len(), 1, "the enemy swordsman is in sight");
    let near = own.iter().find(|u| u.tile == tile(&game, 12, 10)).unwrap();
    let far = own.iter().find(|u| u.tile == tile(&game, 4, 4)).unwrap();

    let row = unit_features(&threats, &rules, near);
    assert_eq!(row.len(), FEATURES);
    assert_eq!(row[column("hp")], 100);
    assert_eq!(row[column("threats")], 1, "the adjacent swordsman can strike");
    assert_eq!(row[column("foes_1")], 1);
    assert!(row[column("blow_sum")] > 0);
    assert_eq!(row[column("blow_sum")], row[column("blow_max")], "one blow");
    assert_eq!(row[column("wars")], 1);
    let exposure = heuristic_exposure(&threats, &rules, near);
    assert_eq!(exposure, row[column("blow_sum")] + row[column("fog_blow")], "the search's exposure is the blows and the fog");
    // The blow is the battlefield model's own.
    let field = &threats.fields[0];
    let foe = field.targets.iter().find(|t| t.unit.is_some()).unwrap();
    assert_eq!(row[column("blow_max")], i32::from(field.blow(foe, sword, 100, near.tile, false)));

    let row = unit_features(&threats, &rules, far);
    assert_eq!(row[column("threats")], 0);
    assert_eq!(row[column("blow_sum")], 0);
    assert_eq!(heuristic_exposure(&threats, &rules, far), 0, "out of reach, nothing comes");
    assert!(row[column("foe_distance")] > 3, "the enemy is far");
    assert_eq!(row[column("foes_3")], 0);

    // The same sight gives the same rows.
    let again = Threats::build(&obs, &rules, &brief, &[]);
    for unit in &own {
        assert_eq!(unit_features(&threats, &rules, unit), unit_features(&again, &rules, unit));
    }
}

/// Our bodies are the enemy's obstacles: a unit whose every neighbour is
/// held by our units cannot be struck by a melee foe that is not adjacent.
#[test]
fn a_surrounded_unit_is_out_of_a_melee_reach() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules);
    let sword = unit_kind(&rules, "swordsman");
    let centre = tile(&game, 12, 10);
    game.debug_spawn_unit(&rules, US, centre, sword);
    let ring: Vec<TileId> = game.view().auth.map().dimensions.neighbors(centre).into_iter().collect();
    for t in &ring {
        game.debug_spawn_unit(&rules, US, *t, sword);
    }
    game.debug_spawn_unit(&rules, THEM, tile(&game, 15, 10), sword);
    let obs = game.ai_observation(&rules, US);
    let brief = TurnBrief::from_observation(&obs, &rules);
    let threats = Threats::build(&obs, &rules, &brief, &[]);
    let inner = threats.own.iter().find(|u| u.tile == centre).unwrap();
    let row = unit_features(&threats, &rules, inner);
    assert_eq!(row[column("threats")], 0, "no free tile next to it");
    assert_eq!(row[column("adjacent_own")], 6);
}

/// The columns where two rows differ: (name, first, second).
fn differences(a: &[i32; FEATURES], b: &[i32; FEATURES]) -> Vec<(&'static str, i32, i32)> {
    (0..FEATURES).filter(|i| a[*i] != b[*i]).map(|i| (NAMES[i], a[i], b[i])).collect()
}

/// The local radius of a unit decision (`AiSettings::budget.local_radius`).
const RADIUS: i32 = 5;

/// The row a decision of `unit` reads at its own tile: its local
/// observation, the sight beyond.
fn decision_row(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle) -> (TileId, [i32; FEATURES]) {
    let obs = game.ai_observation(rules, US);
    let brief = TurnBrief::from_observation(&obs, rules);
    let sight = Sight::take(&obs, rules, &[]);
    let (local, _) = game.ai_local_observation(rules, US, unit, RADIUS).unwrap();
    let threats = Threats::around(&local, rules, &brief, &sight, &[(local.unit.tile, 2)]);
    (local.unit.tile, threats.row(rules, &local.unit, &threats.place_of(&local.unit)))
}

#[test]
fn the_threats_column_is_named() {
    assert_eq!(NAMES[THREATS], "threats");
}

/// Every own unit, deciding, reads at its own tile the row the whole sight
/// gives it: near the enemy and far from it, with own units and enemies
/// beyond its local radius.
#[test]
fn a_decision_reads_the_sights_row_at_its_own_tile() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules);
    let sword = unit_kind(&rules, "swordsman");
    let archer = unit_kind(&rules, "archer");
    let horse = unit_kind(&rules, "horseman");
    let ours = [
        game.debug_spawn_unit(&rules, US, tile(&game, 12, 10), sword),
        game.debug_spawn_unit(&rules, US, tile(&game, 11, 11), archer),
        game.debug_spawn_unit(&rules, US, tile(&game, 12, 12), horse),
        game.debug_spawn_unit(&rules, US, tile(&game, 6, 10), sword),
        game.debug_spawn_unit(&rules, US, tile(&game, 4, 4), sword),
        game.debug_spawn_unit(&rules, US, tile(&game, 19, 4), archer),
    ];
    game.debug_set_unit_hp(ours[0], 60);
    game.debug_spawn_unit(&rules, THEM, tile(&game, 14, 10), sword);
    game.debug_spawn_unit(&rules, THEM, tile(&game, 14, 9), archer);
    game.debug_spawn_unit(&rules, THEM, tile(&game, 21, 4), horse);
    game.debug_spawn_unit(&rules, THEM, tile(&game, 22, 12), sword);

    let obs = game.ai_observation(&rules, US);
    let brief = TurnBrief::from_observation(&obs, &rules);
    let whole = Threats::build(&obs, &rules, &brief, &[]);
    let mut threatened = 0;
    for unit in ours {
        let (at, row) = decision_row(&game, &rules, unit);
        let own = whole.own.iter().find(|u| u.handle == unit).unwrap();
        assert_eq!(own.tile, at);
        assert_eq!(differences(&row, &unit_features(&whole, &rules, own)), [], "the unit at {at:?}");
        threatened += usize::from(row[THREATS] > 0);
    }
    assert!(threatened >= 4, "the units near the enemy are threatened ({threatened})");
}

/// A decision's row at a tile the unit may move to is the row of the unit
/// standing there (but for the moves it spent), and a step next to the
/// enemy raises the threats; the net reads only threatened rows.
#[test]
fn a_decision_reads_the_row_of_the_tile_it_moves_to() {
    let rules = compile_rules(core_rules()).unwrap();
    let sword = unit_kind(&rules, "swordsman");
    let mut game = world(&rules);
    let unit = game.debug_spawn_unit(&rules, US, tile(&game, 10, 10), sword);
    // A scout of ours sees the enemy, three tiles from the unit.
    game.debug_spawn_unit(&rules, US, tile(&game, 13, 8), sword);
    game.debug_spawn_unit(&rules, THEM, tile(&game, 13, 10), sword);
    let (forward, back) = (tile(&game, 11, 10), tile(&game, 9, 10));

    let obs = game.ai_observation(&rules, US);
    let brief = TurnBrief::from_observation(&obs, &rules);
    let sight = Sight::take(&obs, &rules, &[]);
    let (local, _) = game.ai_local_observation(&rules, US, unit, RADIUS).unwrap();
    let threats = Threats::around(&local, &rules, &brief, &sight, &[(local.unit.tile, 2)]);
    let row_at = |to: TileId| {
        let posture = threats.posture_with(&local.unit, to);
        threats.row(&rules, &danger::moved(&local.unit, to), &Place { tile: to, posture: &posture, hurt: &[] })
    };
    let here = threats.row(&rules, &local.unit, &threats.place_of(&local.unit));
    let ahead = row_at(forward);
    assert_eq!(here[THREATS], 0, "out of the enemy's reach");
    assert_eq!(ahead[THREATS], 1, "a step ahead the swordsman reaches it");
    assert!(ahead[column("blow_sum")] > 0);
    assert_eq!(row_at(back)[THREATS], 0);
    assert_eq!(ahead[column("moved")], 1);
    let mine = obs.own_units.iter().find(|u| u.handle == unit).unwrap();
    assert!(threats.net_danger(&rules, mine, &threats.place_of(mine)).is_none(), "no threat, no answer");
    let posture = threats.posture_with(mine, forward);
    let answer = threats.net_danger(&rules, &danger::moved(mine, forward), &Place { tile: forward, posture: &posture, hurt: &[] }).unwrap();
    assert!(answer.damage > 0 && answer.death_permille <= 1000);
    // The enemy gone, nothing threatens the tile.
    let enemy = tile(&game, 13, 10);
    assert_eq!(threats.row(&rules, &danger::moved(mine, forward), &Place { tile: forward, posture: &posture, hurt: &[(enemy, 0)] })[THREATS], 0);

    // The same unit standing on the tile ahead.
    let mut there = world(&rules);
    there.debug_spawn_unit(&rules, US, forward, sword);
    there.debug_spawn_unit(&rules, US, tile(&there, 13, 8), sword);
    there.debug_spawn_unit(&rules, THEM, enemy, sword);
    let obs = there.ai_observation(&rules, US);
    let brief = TurnBrief::from_observation(&obs, &rules);
    let whole = Threats::build(&obs, &rules, &brief, &[]);
    let standing = whole.own.iter().find(|u| u.tile == forward).unwrap();
    let mut expected = unit_features(&whole, &rules, standing);
    expected[column("moved")] = 1;
    assert_eq!(ahead, expected);
}
