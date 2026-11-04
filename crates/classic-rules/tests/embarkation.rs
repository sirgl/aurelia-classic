//! Embarkation on the classic ruleset (classic/docs/classic/rules.md): land units take to the water after
//! Optics (coast) and Astronomy (ocean), embarking and landing end the move,
//! the embarked allowance is 2 + the embarked extra moves, embarked units
//! defend with the era's value, stack one per tile, exert no zone of
//! control and land with an amphibious attack. Denmark and Polynesia change
//! the rules through their traits.

use classic_content::core::core_rules;
use classic_rules::combat::exchange::AttackKind;
use classic_rules::combat::plan_attack;
use classic_rules::ids::{PlayerId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::world::state::GameConfig;
use classic_rules::world::unit::UnitCore;
use classic_rules::ids::PromotionId;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const WATER: TerrainId = TerrainId(4);
const MOUNTAIN: TerrainId = TerrainId(6);

fn setup() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 20,
            height: 12,
            rng_seed: 7,
        },
    );
    (game, rules)
}

fn tile(q: i32, r: i32) -> TileId {
    classic_rules::math::MapDimensions { width: 20, height: 12 }.to_tile(HexCoord { q, r }).unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn promotion(rules: &CompiledRuleset, key: &str) -> PromotionId {
    PromotionId(rules.promotions.iter().position(|p| p.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn nation(rules: &CompiledRuleset, key: &str) -> u8 {
    rules.nations.iter().position(|c| c.key == key).unwrap_or_else(|| panic!("{key}")) as u8
}

fn core(game: &GameState, unit: UnitHandle) -> UnitCore {
    *game.view().auth.units().get(unit).unwrap().core
}

/// Columns `from..to` (axial q) are water on every row: a strait between
/// two shores.
fn strait(game: &mut GameState, rules: &CompiledRuleset, from: i32, to: i32) {
    let (w, h) = (20, 12);
    let mut batch = Vec::new();
    for r in 0..h {
        for q in from..to {
            if let Some(t) = game.view().auth.map().dimensions.to_tile(HexCoord { q, r }) {
                batch.push((t, WATER));
            }
        }
    }
    let _ = w;
    game.debug_set_terrain_batch(rules, &batch);
}

fn move_to(game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle, to: TileId) -> Result<(), String> {
    game.execute(rules, Command::MoveUnit { unit, destination: to })
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

/// End turns until `player` is active again (a new round for it).
fn next_turn_of(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..8 {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
        if game.view().auth.active_player() == player {
            return;
        }
    }
    panic!("{player:?} never became active");
}

fn war(game: &mut GameState, rules: &CompiledRuleset) {
    game.debug_meet(P0, P1);
    game.execute(rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
}

/// Embarking takes the move; next turn the unit has 2 moves at sea, +1 with
/// Astronomy, +1 with Steam Power; landing takes the move and gives the
/// land allowance back (§7.1, §7.4).
#[test]
fn the_embarked_allowance_grows_with_astronomy_and_steam_power() {
    let (mut game, rules) = setup();
    strait(&mut game, &rules, 6, 12);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    let scout = game.debug_spawn_unit(&rules, P0, tile(5, 5), unit(&rules, "core:unit/scout"));
    move_to(&mut game, &rules, scout, tile(6, 5)).unwrap();
    let embarked = core(&game, scout);
    assert_eq!((embarked.moves_left, embarked.moves_max), (0, 120), "embarking ends the move; 2 moves at sea");
    next_turn_of(&mut game, &rules, P0);
    assert_eq!(core(&game, scout).moves_left, 120);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/astronomy"));
    next_turn_of(&mut game, &rules, P0);
    assert_eq!(core(&game, scout).moves_left, 180, "Astronomy: +1 embarked move");
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/steam-power"));
    next_turn_of(&mut game, &rules, P0);
    assert_eq!(core(&game, scout).moves_left, 240, "Steam Power: +1 embarked move");
    // Three steps along the coast, then landing.
    move_to(&mut game, &rules, scout, tile(6, 8)).unwrap();
    assert_eq!(core(&game, scout).moves_left, 60);
    move_to(&mut game, &rules, scout, tile(5, 8)).unwrap();
    let landed = core(&game, scout);
    assert_eq!((landed.moves_left, landed.moves_max), (0, 120), "landing ends the move; the scout's 2 land moves");
    game.validate(&rules).unwrap();
}

/// Denmark (Viking Fury): +1 embarked move and a one-point landing, after
/// which the unit walks on with what is left (at most its land moves).
#[test]
fn denmark_lands_for_one_point_and_walks_on() {
    let (mut game, rules) = setup();
    strait(&mut game, &rules, 6, 12);
    game.debug_set_player_nation(P0, nation(&rules, "core:nation/denmark"));
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    let warrior = game.debug_spawn_unit(&rules, P0, tile(6, 5), unit(&rules, "core:unit/warrior"));
    next_turn_of(&mut game, &rules, P0);
    assert_eq!(core(&game, warrior).moves_left, 180, "Denmark: 3 moves at sea");
    move_to(&mut game, &rules, warrior, tile(5, 5)).unwrap();
    assert_eq!(core(&game, warrior).moves_left, 120, "one point to land, capped by the 2 land moves");
    move_to(&mut game, &rules, warrior, tile(3, 5)).unwrap();
    // Not Denmark: the landing takes every move.
    let (mut game, rules) = setup();
    strait(&mut game, &rules, 6, 12);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    let warrior = game.debug_spawn_unit(&rules, P0, tile(6, 5), unit(&rules, "core:unit/warrior"));
    next_turn_of(&mut game, &rules, P0);
    move_to(&mut game, &rules, warrior, tile(5, 5)).unwrap();
    assert_eq!(core(&game, warrior).moves_left, 0);
}

/// Polynesia (Wayfinding): embarks without Optics and enters the ocean at
/// once; everyone else needs Optics for the coast and Astronomy for the
/// ocean (§7.5).
#[test]
fn polynesia_sails_the_ocean_from_the_start() {
    let (mut game, rules) = setup();
    strait(&mut game, &rules, 6, 12);
    let ocean = tile(8, 5);
    let coast = tile(6, 5);
    let other = game.debug_spawn_unit(&rules, P0, tile(5, 5), unit(&rules, "core:unit/warrior"));
    assert!(move_to(&mut game, &rules, other, coast).is_err(), "no Optics, no embarking");
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    move_to(&mut game, &rules, other, coast).unwrap();
    next_turn_of(&mut game, &rules, P0);
    assert!(move_to(&mut game, &rules, other, ocean).is_err(), "the ocean needs Astronomy");
    let (mut game, rules) = setup();
    strait(&mut game, &rules, 6, 12);
    game.debug_set_player_nation(P0, nation(&rules, "core:nation/polynesia"));
    let warrior = game.debug_spawn_unit(&rules, P0, tile(5, 5), unit(&rules, "core:unit/warrior"));
    move_to(&mut game, &rules, warrior, coast).unwrap();
    next_turn_of(&mut game, &rules, P0);
    move_to(&mut game, &rules, warrior, ocean).unwrap();
    assert_eq!(core(&game, warrior).tile, ocean);
    game.validate(&rules).unwrap();
}

/// A settler marches across a strait over several turns and founds a city
/// on the other shore (settling by sea).
#[test]
fn a_settler_crosses_a_strait_and_founds_a_city() {
    let (mut game, rules) = setup();
    strait(&mut game, &rules, 6, 8);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    let settler = game.debug_spawn_unit(&rules, P0, tile(4, 5), unit(&rules, "core:unit/settler"));
    let site = tile(11, 5);
    game.execute(&rules, Command::MarchTo { unit: settler, destination: site }).unwrap();
    for _ in 0..10 {
        if core(&game, settler).tile == site {
            break;
        }
        next_turn_of(&mut game, &rules, P0);
    }
    assert_eq!(core(&game, settler).tile, site, "the settler arrives");
    let founded = game.execute(&rules, Command::FoundCity { settler });
    assert!(founded.is_ok(), "{founded:?}");
    assert_eq!(game.view().auth.indexes().cities_of(P0).len(), 1);
    game.validate(&rules).unwrap();
}

/// No city at sea, no landing on a mountain.
#[test]
fn embarked_units_found_no_city_and_climb_no_mountain() {
    let (mut game, rules) = setup();
    strait(&mut game, &rules, 6, 12);
    game.debug_set_terrain_batch(&rules, &[(tile(5, 5), MOUNTAIN)]);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    let settler = game.debug_spawn_unit(&rules, P0, tile(6, 5), unit(&rules, "core:unit/settler"));
    assert!(game.execute(&rules, Command::FoundCity { settler }).is_err());
    next_turn_of(&mut game, &rules, P0);
    assert!(move_to(&mut game, &rules, settler, tile(5, 5)).is_err(), "a mountain");
    move_to(&mut game, &rules, settler, tile(5, 6)).unwrap();
}

/// One embarked unit per water tile, whatever its kind; an own ship may
/// share the tile (§8).
#[test]
fn embarked_units_stack_one_per_tile_beside_a_ship() {
    let (mut game, rules) = setup();
    strait(&mut game, &rules, 6, 12);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    let sea = tile(6, 5);
    let warrior = game.debug_spawn_unit(&rules, P0, tile(5, 5), unit(&rules, "core:unit/warrior"));
    let worker = game.debug_spawn_unit(&rules, P0, tile(5, 6), unit(&rules, "core:unit/worker"));
    let _ship = game.debug_spawn_unit(&rules, P0, sea, unit(&rules, "core:unit/trireme"));
    move_to(&mut game, &rules, warrior, sea).unwrap();
    assert!(move_to(&mut game, &rules, worker, sea).is_err(), "a second embarked unit");
}

/// Zones of control at sea (§7.6): an enemy ship slows embarked units; an
/// embarked enemy slows nobody.
#[test]
fn ships_exert_zones_of_control_over_embarked_units_and_embarked_units_none() {
    // A scout embarked at (7,3) next to an enemy trireme at (7,4) steps to
    // (6,4), also next to it; (6,3) is a mountain (no way around).
    let step_next_to_ship = |at_war: bool| {
        let (mut game, rules) = setup();
        strait(&mut game, &rules, 6, 12);
        game.debug_set_terrain_batch(&rules, &[(tile(6, 3), MOUNTAIN)]);
        game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
        game.debug_learn(&rules, P0, tech(&rules, "core:tech/astronomy"));
        let scout = game.debug_spawn_unit(&rules, P0, tile(7, 3), unit(&rules, "core:unit/scout"));
        game.debug_spawn_unit(&rules, P1, tile(7, 4), unit(&rules, "core:unit/trireme"));
        if at_war {
            war(&mut game, &rules);
        }
        next_turn_of(&mut game, &rules, P0);
        move_to(&mut game, &rules, scout, tile(6, 4)).unwrap();
        core(&game, scout).moves_left
    };
    assert_eq!(step_next_to_ship(false), 120, "3 moves at sea, one step");
    assert_eq!(step_next_to_ship(true), 0, "the ship's zone of control takes every move");
    // An embarked enemy at (6,5) exerts none on a land unit walking from
    // (5,5) to (5,6), both next to it.
    let (mut game, rules) = setup();
    strait(&mut game, &rules, 6, 12);
    game.debug_learn(&rules, P1, tech(&rules, "core:tech/optics"));
    game.debug_spawn_unit(&rules, P1, tile(6, 5), unit(&rules, "core:unit/warrior"));
    let walker = game.debug_spawn_unit(&rules, P0, tile(5, 5), unit(&rules, "core:unit/warrior"));
    war(&mut game, &rules);
    next_turn_of(&mut game, &rules, P0);
    move_to(&mut game, &rules, walker, tile(5, 6)).unwrap();
    assert_eq!(core(&game, walker).moves_left, 60, "no zone of control from an embarked unit");
}

/// A ship sinks an embarked unit (era defense, §1.8) and sails into its
/// tile; an embarked civilian is captured by the ship (§2.3).
#[test]
fn ships_sink_embarked_units_and_capture_embarked_civilians() {
    let (mut game, rules) = setup();
    strait(&mut game, &rules, 6, 12);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    let warrior = game.debug_spawn_unit(&rules, P0, tile(7, 5), unit(&rules, "core:unit/warrior"));
    game.debug_set_unit_hp(warrior, 10);
    let settler = game.debug_spawn_unit(&rules, P0, tile(7, 7), unit(&rules, "core:unit/settler"));
    let ship = game.debug_spawn_unit(&rules, P1, tile(8, 5), unit(&rules, "core:unit/trireme"));
    let raider = game.debug_spawn_unit(&rules, P1, tile(8, 7), unit(&rules, "core:unit/trireme"));
    war(&mut game, &rules);
    next_turn_of(&mut game, &rules, P1);
    let plan = plan_attack(game.view(), &rules, ship, tile(7, 5), AttackKind::Melee).unwrap();
    assert_eq!(plan.defender_strength, 400, "embarked defense: Optics is a classical technology");
    game.execute(&rules, Command::AttackUnit { attacker: ship, defender: warrior }).unwrap();
    assert!(game.view().auth.units().get(warrior).is_none(), "the embarked warrior sinks");
    assert_eq!(core(&game, ship).tile, tile(7, 5), "the ship sails in");
    game.execute(&rules, Command::AttackUnit { attacker: raider, defender: settler }).unwrap();
    assert!(game.view().auth.units().get(settler).is_none());
    let auth = game.view().auth;
    let worker = unit(&rules, "core:unit/worker");
    let captured = auth.indexes().units_of(P1).iter().any(|u| auth.units().get(*u).unwrap().core.kind == worker);
    assert!(captured, "the settler becomes the raider's worker");
    game.validate(&rules).unwrap();
}

/// An embarked unit attacks the shore at −50 %
/// unless it is amphibious, and lands on a win (§1.3, §7.9).
#[test]
fn an_amphibious_landing_attacks_at_half_strength_unless_amphibious() {
    let (mut game, rules) = setup();
    strait(&mut game, &rules, 6, 12);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    let warrior = game.debug_spawn_unit(&rules, P0, tile(6, 5), unit(&rules, "core:unit/warrior"));
    let target = tile(5, 5);
    let defender = game.debug_spawn_unit(&rules, P1, target, unit(&rules, "core:unit/warrior"));
    war(&mut game, &rules);
    next_turn_of(&mut game, &rules, P0);
    let plan = plan_attack(game.view(), &rules, warrior, target, AttackKind::Melee).unwrap();
    assert_eq!(plan.attacker_strength, 8 * 50, "a warrior landing: 8 × (100 − 50)");
    game.debug_set_experience(warrior, 30);
    for key in ["core:promotion/drill-1", "core:promotion/amphibious"] {
        game.execute(&rules, Command::Promote { unit: warrior, promotion: promotion(&rules, key) }).unwrap();
    }
    let plan = plan_attack(game.view(), &rules, warrior, target, AttackKind::Melee).unwrap();
    assert_eq!(plan.attacker_strength, 8 * 100, "amphibious: no landing penalty");
    game.debug_set_unit_hp(defender, 1);
    game.execute(&rules, Command::AttackUnit { attacker: warrior, defender }).unwrap();
    assert_eq!(core(&game, warrior).tile, target, "the winner lands");
    assert_eq!(core(&game, warrior).moves_max, 120, "on land again");
    game.validate(&rules).unwrap();
}

/// The AI prices steps from its knowledge (`planner_map::step_points`, a
/// step that takes every move as two points) with the same rule the core
/// moves by (`step_cost_for`): on random
/// coasts, oceans, hills, mountains, forests, roads and foreign land, for
/// land units without and with Optics or Astronomy, Polynesian and Danish
/// units, a scout, a trireme and a caravel, the two prices are equal for
/// every step between two tiles the player knows (it sees coast and ocean).
#[test]
fn the_ai_view_prices_known_steps_as_the_core_moves() {
    use classic_rules::ai::planner_map::{step_points, KnownMap};
    use classic_rules::navigation::cost::{step_cost_for, StepCost};
    use classic_rules::navigation::MoveProfile;
    let mut seed = 0x5EA5_0A1Du64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut compared = 0;
    for round in 0..12 {
        let (mut game, rules) = setup();
        let forest = classic_rules::ids::FeatureId(rules.features.iter().position(|f| f.key == "core:feature/forest").unwrap() as u16);
        let count = 20 * 12;
        let terrain: Vec<(TileId, TerrainId)> = (0..count)
            .map(|i| {
                let pick = rnd() % 10;
                let t = match pick {
                    0..=3 => WATER,
                    4 => MOUNTAIN,
                    5 => TerrainId(2),
                    _ => TerrainId((rnd() % 2) as u16),
                };
                (TileId(i), t)
            })
            .collect();
        game.debug_set_terrain_batch(&rules, &terrain);
        let woods: Vec<_> = terrain
            .iter()
            .filter(|(_, t)| *t != WATER && *t != MOUNTAIN && rnd() % 4 == 0)
            .map(|(tile, _)| (*tile, forest))
            .collect();
        game.debug_set_feature_batch(&rules, &woods);
        let roads: Vec<TileId> = (0..count).filter(|_| rnd() % 5 == 0).map(TileId).collect();
        game.debug_set_road_batch(&rules, &roads);
        let land: Vec<TileId> = terrain.iter().filter(|(_, t)| *t != WATER && *t != MOUNTAIN).map(|(t, _)| *t).collect();
        game.debug_found_city(&rules, P1, land[land.len() / 2]);
        game.debug_found_city(&rules, P0, land[land.len() / 5]);
        match round % 4 {
            0 => {}
            1 => game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics")),
            2 => {
                game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
                game.debug_learn(&rules, P0, tech(&rules, "core:tech/astronomy"));
            }
            _ => game.debug_set_player_nation(P0, nation(&rules, if round % 8 == 3 { "core:nation/polynesia" } else { "core:nation/denmark" })),
        }
        if round % 3 == 0 {
            game.debug_meet(P0, P1);
            game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
        }
        let known: Vec<TileId> = (0..count).filter(|_| rnd() % 3 != 0).map(TileId).collect();
        game.debug_reveal(P0, &known);
        let kinds = ["core:unit/warrior", "core:unit/settler", "core:unit/scout", "core:unit/trireme", "core:unit/caravel"];
        let auth = game.view().auth;
        let view = game.player_view(&rules, P0);
        let dims = auth.map().dimensions;
        let is_known = |t: TileId| view.is_known(t);
        for key in kinds {
            let profile = MoveProfile::of_kind(auth, &rules, P0, unit(&rules, key));
            for from in 0..count {
                let from = TileId(from);
                for to in dims.neighbors(from) {
                    let all_known = is_known(from) && is_known(to);
                    if !all_known {
                        continue;
                    }
                    let core = step_cost_for(auth, &rules, P0, &profile, from, to).map(|cost| match cost {
                        StepCost::Points(points) => points,
                        StepCost::AllMoves => 120,
                    });
                    let ai = step_points(&KnownMap { view: &view, profile }, from, to);
                    assert_eq!(ai, core, "round {round}, {key}: {from:?} -> {to:?}");
                    compared += 1;
                }
            }
        }
    }
    assert!(compared > 5000, "enough known steps compared: {compared}");
}
