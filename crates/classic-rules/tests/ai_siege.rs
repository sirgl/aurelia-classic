//! The fixes of the defects that kept the AI from ever taking a city (`operations::SiegeFixes`,
//! the `siege` variants): a unit on an assault is worth its share of its target, so the defence
//! takes it only for a greater saving; a recovery after a lost city ends. The siege that closes in
//! when its damage beats the city's healing is tested beside the siege model (`ai::front`). The
//! truth is read only to set the scene and judge it.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::operations::SiegeFixes;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::planner_strategy::review_strategy;
use classic_rules::ai::state::{AiState, ObjectiveKind, SeenCity, Strategy, WarObjective};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::UnitTask;
use classic_rules::ids::{EntityId, PlayerId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
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

#[test]
fn the_siege_variants_carry_the_fixes_they_name() {
    let siege = |v: Variant| v.war_tuning().siege;
    assert_eq!(siege(Variant::PreWarFix), SiegeFixes::NONE, "the standard before the war fix plays none of them");
    assert_eq!(siege(Variant::Siege), SiegeFixes::ALL);
    assert_eq!(siege(Variant::SiegeAssault), SiegeFixes { assault: true, ..SiegeFixes::NONE });
    assert_eq!(siege(Variant::SiegeGarrison), SiegeFixes { garrison: true, ..SiegeFixes::NONE });
    assert_eq!(siege(Variant::SiegeClose), SiegeFixes { close: true, ..SiegeFixes::NONE });
    assert_eq!(siege(Variant::SiegeChance), SiegeFixes { chance: true, ..SiegeFixes::NONE });
    assert_eq!(siege(Variant::SiegeRecover), SiegeFixes { recover: true, ..SiegeFixes::NONE });
    let mut standard = Variant::Siege.war_tuning();
    standard.siege = SiegeFixes::NONE;
    assert_eq!(standard, Variant::PreWarFix.war_tuning(), "`siege` is `pre-war-fix` but for its fixes");
    for v in [Variant::Siege, Variant::SiegeAssault, Variant::SiegeGarrison, Variant::SiegeClose, Variant::SiegeChance, Variant::SiegeRecover] {
        assert_eq!(Variant::from_name(v.name()), Some(v));
    }
}

/// Open grassland. The AI: its capital (6, 10) with four warriors on an assault of the enemy's
/// city of `population` at (19, 10), and a border town (13, 10) without a garrison. The enemy:
/// that city, and a band of three warriors two tiles from the border town, at war with the AI.
fn scene(rules: &CompiledRuleset, population: u32) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 11 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, AI, tile(&game, 6, 10));
    game.debug_found_city(rules, AI, tile(&game, 13, 10));
    let target = game.debug_found_city(rules, ENEMY, tile(&game, 19, 10));
    game.debug_set_population(rules, target, population);
    let warrior = unit_kind(rules, "core:unit/warrior");
    for (q, r) in [(7, 10), (7, 9), (8, 9), (7, 11)] {
        game.debug_spawn_unit(rules, AI, tile(&game, q, r), warrior);
    }
    for (q, r) in [(13, 12), (12, 12), (14, 12)] {
        game.debug_spawn_unit(rules, ENEMY, tile(&game, q, r), warrior);
    }
    let everything: Vec<TileId> = (0..tiles).map(TileId).collect();
    game.debug_reveal(AI, &everything);
    game.debug_meet(AI, ENEMY);
    game.execute(rules, Command::DeclareWar { player: AI, enemy: ENEMY }).unwrap();
    game
}

/// The AI's units, by id.
fn army(game: &GameState) -> Vec<EntityId> {
    let auth = game.view().auth;
    let mut units: Vec<EntityId> = auth.indexes().units_of(AI).iter().filter_map(|h| auth.units().get(*h)).map(|u| u.core.id).collect();
    units.sort();
    units
}

/// The tasks of the four warriors on the assault of a city of `population` after the start of the
/// AI's turn under `variant`.
fn assault_after_start(variant: Variant, population: u32) -> Vec<Option<UnitTask>> {
    let rules = compile_rules(core_rules()).unwrap();
    let game = scene(&rules, population);
    let target = tile(&game, 19, 10);
    let round = game.view().auth.turn().round;
    let mut state = AiState::new(AI, round);
    state.see_city(SeenCity { tile: target, owner: ENEMY, population, last_seen_round: round });
    state.relation_mut(ENEMY).objective = Some(WarObjective { tile: target, kind: ObjectiveKind::City, chosen_round: round });
    state.strategy.current = Strategy::Attack;
    let units = army(&game);
    for unit in &units {
        state.set_task(*unit, UnitTask::Assault { target }, round);
    }
    let mind = ClassicMind::new(variant);
    let started = mind.start_turn(&mind.sense(&game, &rules, AI), &rules, AI, &mut state);
    let border = tile(&game, 13, 10);
    assert!(started.brief.defence.as_ref().and_then(|w| w.danger(border)).is_some(), "the border town is in danger");
    units.iter().map(|u| state.task_of(*u).map(|t| t.task)).collect()
}

/// The standard before the war fix counts the units on an assault as free: the border town's danger takes them.
/// With the fix each is worth its share of what the city of five is worth to the army that takes
/// it, more than the small band's threat to a town of one saves: the assault goes on. A city of
/// eight no army of the warriors' kind takes (`operations::MAX_ARMY`): its assault is worth
/// nothing and its units go home.
#[test]
fn an_assault_worth_more_than_the_saving_is_not_taken_by_the_defence() {
    let on_assault = |tasks: &[Option<UnitTask>]| tasks.iter().filter(|t| matches!(t, Some(UnitTask::Assault { .. }))).count();
    let standard = assault_after_start(Variant::PreWarFix, 5);
    assert!(on_assault(&standard) < standard.len(), "the standard before the war fix sends assault units home: {standard:?}");
    let fixed = assault_after_start(Variant::SiegeAssault, 5);
    assert_eq!(on_assault(&fixed), fixed.len(), "the assault keeps its units: {fixed:?}");
    let hopeless = assault_after_start(Variant::SiegeAssault, 8);
    assert!(on_assault(&hopeless) < hopeless.len(), "a hopeless assault gives its units up: {hopeless:?}");
}

/// An empire that lost a city recovers until its loss is older than the time an operation may
/// march to take a city back; then its peak is the cities it holds and Recover ends. The standard
/// before the war fix recovers for ever.
#[test]
fn a_recovery_after_a_lost_city_ends() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 11 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(&rules, &land);
    game.debug_found_city(&rules, AI, tile(&game, 6, 10));
    game.debug_found_city(&rules, AI, tile(&game, 12, 10));
    let warrior = unit_kind(&rules, "core:unit/warrior");
    game.debug_spawn_unit(&rules, AI, tile(&game, 6, 10), warrior);
    let obs = game.ai_observation(&rules, AI);
    let review = |variant: Variant, rounds_recovering: u32| {
        let mut brief = TurnBrief::from_observation(&obs, &rules);
        brief.war = variant.war_tuning();
        brief.round = 1000;
        let mut state = AiState::new(AI, 0);
        state.strategy.current = Strategy::Recover;
        state.strategy.since_round = brief.round - rounds_recovering;
        state.strategy.peak_cities = brief.cities.len() as u32 + 1;
        review_strategy(&brief, &mut state);
        (state.strategy.current, state.strategy.peak_cities, brief.cities.len() as u32)
    };
    let window = classic_rules::ai::planner_strategy::WAR_WEARINESS_ROUNDS / 2;
    let (strategy, peak, cities) = review(Variant::SiegeRecover, window - 1);
    assert_eq!(strategy, Strategy::Recover, "a recent loss keeps it recovering");
    assert_eq!(peak, cities + 1);
    let (strategy, peak, cities) = review(Variant::SiegeRecover, window);
    assert_ne!(strategy, Strategy::Recover, "an old loss no longer does");
    assert_eq!(peak, cities);
    let (strategy, _, _) = review(Variant::PreWarFix, 10 * window);
    assert_eq!(strategy, Strategy::Recover, "the standard before the war fix recovers for ever");
}
