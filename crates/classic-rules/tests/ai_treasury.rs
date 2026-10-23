//! The AI keeps its treasury healthy under classic maintenance (classic/docs/QA.md):
//! in the full-game harness 38 % of the AI civilizations were broke (no
//! gold, a deficit) at round 250, their armies disbanded.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::ids::{PlayerId, TechId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};
use classic_rules::world::player::ClassicKind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
/// Map code of grassland.
const GRASSLAND: TerrainId = TerrainId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap() as u16)
}

/// Open grassland, P0's capital and a second city five tiles east, P1 far
/// away; `rounds` AI rounds of P0 (P1 only ends its turns).
fn play(game: &mut GameState, rules: &CompiledRuleset, rounds: u32) {
    let mut driver = AiDriver::new(AiSettings::for_rules(rules, rules.default_difficulty()), vec![P1]);
    for _ in 0..rounds {
        driver.run(game, rules, u32::MAX);
        if game.view().auth.active_player() == P1 {
            game.execute(rules, Command::EndTurn { player: P1 }).unwrap();
        }
    }
}

fn two_cities(wheel: bool) -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 30,
            height: 20,
            rng_seed: 11,
        },
    );
    let land: Vec<(TileId, TerrainId)> = (0..game.view().auth.map().tile_count() as u32)
        .map(|t| (TileId(t), GRASSLAND))
        .collect();
    game.debug_set_terrain_batch(&rules, &land);
    let capital = game.debug_found_city(&rules, P0, tile(&game, 5, 10));
    let second = game.debug_found_city(&rules, P0, tile(&game, 10, 10));
    for city in [capital, second] {
        game.debug_set_population(&rules, city, 5);
    }
    game.debug_found_city(&rules, P1, tile(&game, 26, 3));
    if wheel {
        game.debug_learn(&rules, P0, tech(&rules, "core:tech/the-wheel"));
    }
    (game, rules)
}

/// A worker links the second city to the capital: the connection gold
/// (−1 + 0.15 × 5 + 1.1 × 5 ≈ 5) beats four road tiles of upkeep. The AI
/// built roads only between near cities, only with nothing else to do and
/// only with a surplus, so its cities stayed unconnected.
#[test]
fn the_ai_connects_its_cities_to_the_capital() {
    let (mut game, rules) = two_cities(true);
    game.debug_spawn_unit(&rules, P0, tile(&game, 6, 10), unit(&rules, "core:unit/worker"));
    let second = game.view().auth.indexes().city_on_tile(tile(&game, 10, 10)).unwrap();
    let connected = |g: &GameState| g.player_view(&rules, P0).own_connected_cities();
    assert_eq!(connected(&game).len(), 1, "only the capital");
    play(&mut game, &rules, 40);
    // The game's AI may found a third city meanwhile and link it too.
    assert!(connected(&game).contains(&second), "the second city is connected");
    let gold = classic_rules::derived::maintenance::upkeep(game.view().auth, &rules, P0).connections;
    assert!(gold > 0, "the connection pays: {gold}");
    game.validate(&rules).unwrap();
}

/// A barbarian stands two tiles from the capital, where the settler
/// would step out: the settler does not walk out and back every turn —
/// it takes a site away from the barbarian, or waits for the garrison to
/// escort it — and founds its city. Settlers used to oscillate for the
/// rest of the game (full-game harness: settlers alive since round 45).
#[test]
fn a_settler_threatened_by_a_barbarian_still_founds_its_city() {
    let (mut game, rules) = two_cities(false);
    let barbarian = PlayerId(1);
    game.debug_set_player_kind(barbarian, classic_rules::world::player::PlayerKind::Module(ClassicKind::Barbarian));
    game.debug_set_barbarian_mode(classic_rules::world::barbarians::BarbarianMode::Normal);
    // P1 made the barbarian leaves P0 the last civilization: from round 11
    // that is a domination victory and the game ends. Barbarian camps (with
    // defenders) now appear from the first barbarian turn (Unciv:
    // BarbarianManager.kt), so the settler waits for an escort past round
    // 10; without domination the 30 rounds run to the end.
    let options = game.view().auth.module().victory().options().with(classic_rules::world::victory::VictoryKind::Domination, false);
    game.debug_set_victory_options(options);
    game.debug_spawn_unit(&rules, barbarian, tile(&game, 5, 12), unit(&rules, "core:unit/warrior"));
    let capital = tile(&game, 5, 10);
    game.debug_spawn_unit(&rules, P0, capital, unit(&rules, "core:unit/warrior"));
    game.debug_spawn_unit(&rules, P0, capital, unit(&rules, "core:unit/settler"));
    let cities = |g: &GameState| g.view().auth.indexes().cities_of(P0).len();
    play(&mut game, &rules, 30);
    assert_eq!(cities(&game), 3, "the settler founded its city");
    game.validate(&rules).unwrap();
}

/// A city with every building it may build, its army and workers at
/// their caps: its queue runs empty and the AI sets a process (Wealth or
/// Research) instead of leaving the production unused (the harness saw
/// cities with empty queues for most of the late game).
#[test]
fn a_city_with_nothing_to_build_works_a_process() {
    let (mut game, rules) = two_cities(true);
    for t in 0..rules.technologies.len() {
        game.debug_learn(&rules, P0, TechId(t as u16));
    }
    let cities: Vec<_> = game.view().auth.indexes().cities_of(P0).to_vec();
    for city in &cities {
        for (index, def) in rules.buildings.iter().enumerate() {
            if !def.unbuildable {
                game.debug_add_building(&rules, *city, classic_rules::ids::BuildingKindId(index as u16));
            }
        }
    }
    // The army and the workers are at their caps already.
    let options = game.view().auth.module().victory().options().with(classic_rules::world::victory::VictoryKind::Science, false);
    game.debug_set_victory_options(options);
    for q in 0..20 {
        game.debug_spawn_unit(&rules, P0, tile(&game, 3 + q % 10, 12 + q / 10), unit(&rules, "core:unit/warrior"));
    }
    for q in 0..3 {
        game.debug_spawn_unit(&rules, P0, tile(&game, 6 + q, 8), unit(&rules, "core:unit/worker"));
    }
    play(&mut game, &rules, 3);
    let auth = game.view().auth;
    let with_process = cities
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .filter(|c| c.economy.process.is_some())
        .count();
    assert!(with_process > 0, "a city that ran out of orders works a process");
}

/// Three cities whose land is all farmed and one worker with nothing left
/// to do: the AI trains no more workers (it used to fill one per city,
/// each an upkeep without a job).
#[test]
fn the_ai_trains_no_workers_while_one_is_idle() {
    // No roads yet (The Wheel unknown): nothing but farms to build.
    let (mut game, rules) = two_cities(false);
    game.debug_found_city(&rules, P0, tile(&game, 15, 10));
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/agriculture"));
    let farm = rules.improvements.iter().position(|i| i.key == "core:improvement/farm").unwrap();
    let auth = game.view().auth;
    let farmed: Vec<(TileId, classic_rules::ids::ImprovementId)> = (0..auth.map().tile_count() as u32)
        .map(TileId)
        .filter(|t| auth.map().owner(*t).flatten() == Some(P0) && auth.indexes().city_on_tile(*t).is_none())
        .map(|t| (t, classic_rules::ids::ImprovementId(farm as u16)))
        .collect();
    game.debug_set_improvement_batch(&rules, &farmed);
    let worker = unit(&rules, "core:unit/worker");
    game.debug_spawn_unit(&rules, P0, tile(&game, 6, 10), worker);
    let workers = |g: &GameState| g.view().auth.units().iter_cores().filter(|u| u.owner == P0 && u.kind == worker).count();
    let mut most = 0;
    for _ in 0..6 {
        play(&mut game, &rules, 5);
        most = most.max(workers(&game));
    }
    assert_eq!(most, 1, "no second worker while the first has nothing to do");
}

