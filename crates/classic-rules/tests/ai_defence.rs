//! Home defence of the game's AI, `standard` (named `defence` until it
//! became the game's AI; classic/docs/ai-defence.md): an enemy
//! band next to a city of the AI makes it a city in danger; the AI's free
//! units come to guard it, an operation worth less than the city comes
//! home, the cities arm and the town puts its defence first in its queue.
//! The `leaders` AI it is built on does none of it. The truth is read only to set the scene and judge it.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::state::{AiState, Composition, Operation, OperationPhase, SeenCity, Strategy};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::planner::{role_of, AiRole};
use classic_rules::ai::{AiSettings, UnitTask};
use classic_rules::ids::{EntityId, PlayerId, ProductionItem, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::mind::AiMind;

const AI: PlayerId = PlayerId(0);
const ENEMY: PlayerId = PlayerId(1);
const GRASSLAND: u16 = 1;

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

/// Open grassland. The AI: its capital (6, 10) with four warriors, and a
/// border town (13, 10) without a garrison. The enemy: its capital far
/// away, a town (24, 4), and a band of six warriors two tiles from the
/// border town, at war with the AI.
fn scene(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 11 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, AI, tile(&game, 6, 10));
    game.debug_found_city(rules, AI, tile(&game, 13, 10));
    game.debug_found_city(rules, ENEMY, tile(&game, 28, 18));
    game.debug_found_city(rules, ENEMY, tile(&game, 24, 4));
    let warrior = unit_kind(rules, "core:unit/warrior");
    for (q, r) in [(5, 10), (6, 9), (7, 9), (5, 11)] {
        game.debug_spawn_unit(rules, AI, tile(&game, q, r), warrior);
    }
    for (q, r) in [(15, 10), (15, 9), (16, 9), (15, 11), (14, 12), (16, 10)] {
        game.debug_spawn_unit(rules, ENEMY, tile(&game, q, r), warrior);
    }
    game.debug_meet(AI, ENEMY);
    game.execute(rules, Command::DeclareWar { player: AI, enemy: ENEMY }).unwrap();
    game
}

/// The AI's units and their tasks after the start of its turn.
fn start(variant: Variant, game: &GameState, rules: &CompiledRuleset, state: &mut AiState) -> Vec<(EntityId, Option<UnitTask>)> {
    let mind = ClassicMind::new(variant);
    let started = mind.start_turn(&mind.sense(game, rules, AI), rules, AI, state);
    started.brief.units.iter().map(|u| (u.id, state.task_of(u.id).map(|t| t.task))).collect()
}

#[test]
fn a_city_in_danger_draws_its_guards_and_arms() {
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules);
    let border = tile(&game, 13, 10);
    let round = game.view().auth.turn().round;

    let capital = tile(&game, 6, 10);
    let garrisoned = |state: &mut AiState| {
        for unit in army(&game) {
            state.set_task(unit, UnitTask::Defend { city: capital }, round);
        }
    };
    let mut state = AiState::new(AI, round);
    garrisoned(&mut state);
    let mind = ClassicMind::new(Variant::Standard);
    let started = mind.start_turn(&mind.sense(&game, &rules, AI), &rules, AI, &mut state);
    let watch = started.brief.defence.as_ref().expect("a defending AI watches its cities");
    let danger = watch.danger(border).copied().expect("the border town is in danger");
    assert!(danger.threat > danger.guard, "{danger:?}");
    assert_eq!(watch.dangers().len(), 1, "the capital is out of the band's reach: {watch:?}");
    let guards = state.tasks().iter().filter(|t| t.task == UnitTask::Defend { city: border }).count();
    assert!(guards >= 2, "the capital's warriors come: {:?}", state.tasks());
    let kept = state.tasks().iter().filter(|t| t.task == UnitTask::Defend { city: capital }).count();
    eprintln!("defence: {danger:?}, guards sent {guards}, kept at the capital {kept}");
    assert!(kept >= 1, "the capital keeps its first garrison: {:?}", state.tasks());
    assert_eq!(state.strategy.current, Strategy::Defend, "the cities arm");

    let mut leaders = AiState::new(AI, round);
    garrisoned(&mut leaders);
    let tasks = start(Variant::Leaders, &game, &rules, &mut leaders);
    assert!(tasks.iter().all(|(_, task)| *task == Some(UnitTask::Defend { city: capital })), "{tasks:?}");
    assert_ne!(leaders.strategy.current, Strategy::Defend);
}

/// The AI's units, by id.
fn army(game: &GameState) -> Vec<EntityId> {
    let auth = game.view().auth;
    let mut units: Vec<EntityId> = auth.indexes().units_of(AI).iter().filter_map(|h| auth.units().get(*h)).map(|u| u.core.id).collect();
    units.sort();
    units
}

/// An operation against the enemy's small town is worth less than the
/// border town it leaves open: it ends and its army guards the town.
#[test]
fn an_operation_worth_less_than_the_city_comes_home() {
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules);
    let border = tile(&game, 13, 10);
    let target = tile(&game, 24, 4);
    let round = game.view().auth.turn().round;
    let operation = Operation {
        target,
        enemy: ENEMY,
        muster: tile(&game, 6, 10),
        need: Composition { melee: 4, ranged: 0, siege: 0 },
        phase: OperationPhase::Forming,
        since_round: round,
        phase_round: round,
    };
    let enlisted = |state: &mut AiState| {
        state.see_city(SeenCity { tile: target, owner: ENEMY, population: 1, last_seen_round: round });
        state.start_operation(operation);
        for unit in army(&game) {
            state.set_task(unit, UnitTask::Operation { target }, round);
        }
    };

    let mut state = AiState::new(AI, round);
    enlisted(&mut state);
    let tasks = start(Variant::Standard, &game, &rules, &mut state);
    assert!(state.operations().is_empty(), "the operation ends");
    let home = tasks.iter().filter(|(_, task)| *task == Some(UnitTask::Defend { city: border })).count();
    assert!(home >= 2, "its army guards the border town: {tasks:?}");

    let mut leaders = AiState::new(AI, round);
    enlisted(&mut leaders);
    start(Variant::Leaders, &game, &rules, &mut leaders);
    assert_eq!(leaders.operations().len(), 1, "the leaders' operation goes on");
}

/// The border town of ten citizens, with a garrison, a peaceful order in
/// its queue and a band twice the size against it, puts what saves it first — a defender or its walls — and keeps the old
/// head after it; `leaders` keeps its queue (its emergency order is only
/// for a city without a garrison).
#[test]
fn a_city_in_danger_puts_its_defence_first() {
    let rules = compile_rules(core_rules()).unwrap();
    let head_after_turn = |variant: Variant| {
        let mut game = scene(&rules);
        let border = tile(&game, 13, 10);
        let auth = game.view().auth;
        let town = auth.indexes().city_on_tile(border).unwrap();
        let warrior = unit_kind(&rules, "core:unit/warrior");
        game.debug_spawn_unit(&rules, AI, border, warrior);
        game.debug_set_population(&rules, town, 10);
        // The band doubles against a town of ten: the guards on their way
        // no longer hold it, and new defenders pay their upkeep.
        for (q, r) in [(13, 12), (12, 12), (15, 8), (16, 8), (12, 13), (13, 13)] {
            game.debug_spawn_unit(&rules, ENEMY, tile(&game, q, r), warrior);
        }
        let worker = ProductionItem::Unit(unit_kind(&rules, "core:unit/worker"));
        game.execute(&rules, Command::EnqueueProduction { player: AI, city: town, item: worker }).unwrap();
        let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
        let mut driver = kernel::ai::driver::AiDriver::with_mind(settings, vec![ENEMY], ClassicMind::new(variant));
        driver.run(&mut game, &rules, u32::MAX);
        let queue = game.view().auth.cities().get(town).unwrap().queue.clone();
        (queue, worker)
    };
    let (queue, worker) = head_after_turn(Variant::Standard);
    let defence = |item: &ProductionItem| match item {
        ProductionItem::Unit(kind) => role_of(&rules, *kind) == AiRole::Military,
        ProductionItem::Building(kind) => rules.buildings[kind.0 as usize].key == "core:building/walls",
        ProductionItem::Project(_) => false,
    };
    let keys = |queue: &[ProductionItem]| -> Vec<String> {
        queue.iter().map(|item| match item {
            ProductionItem::Unit(k) => rules.units[k.0 as usize].key.clone(),
            ProductionItem::Building(b) => rules.buildings[b.0 as usize].key.clone(),
            ProductionItem::Project(_) => "project".to_owned(),
        }).collect()
    };
    assert!(queue.first().is_some_and(defence), "{:?}", keys(&queue));
    assert_eq!(queue.get(1), Some(&worker), "the old head comes after it");
    let (queue, _) = head_after_turn(Variant::Leaders);
    assert_eq!(queue.first(), Some(&worker), "{queue:?}");
}
