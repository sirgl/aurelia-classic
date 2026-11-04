//! The AI of Unciv 4.22.10 ported as a mind (`ai::unciv`, classic/docs/ai-unciv.md):
//! the variants that play it, its unit automation in small scenes, and its
//! determinism. The truth is read only to set the scenes and judge them.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::state::SeenCity;
use classic_rules::ai::trace::Outcome;
use classic_rules::ai::unciv::UncivPlay;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::classic::Classic;
use classic_rules::ids::{PlayerId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::seats::Seated;

const AI: PlayerId = PlayerId(0);
const ENEMY: PlayerId = PlayerId(1);
const GRASSLAND: u16 = 1;

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

/// The Unciv variants play `standard` in every switch but Unciv's part.
#[test]
fn the_unciv_variants_are_named_and_play_standard_elsewhere() {
    for v in [Variant::Unciv, Variant::UncivMilitary] {
        assert_eq!(Variant::from_name(v.name()), Some(v));
        assert!(v.unciv().on(), "{}", v.name());
        assert_eq!(v.price_model(), Variant::Standard.price_model());
        assert_eq!(v.war_tuning(), Variant::Standard.war_tuning());
        assert_eq!(v.conquest(), Variant::Standard.conquest());
        assert!(!v.siege_walls(), "Unciv's units besiege its way, not `siege_walls`'");
    }
    assert_eq!(Variant::UncivMilitary.unciv(), UncivPlay::Military);
    assert_eq!(Variant::Standard.unciv(), UncivPlay::Off);
    assert!(Variant::ALL.iter().filter(|v| v.unciv().on()).count() == 2);
}

/// Open grassland, at war: the AI's city far west, the enemy's capital of
/// three at (16, 8) with an archer inside; the AI's army five tiles out:
/// two archers, three swordsmen and a catapult.
fn scene(rules: &CompiledRuleset) -> (GameState, TileId) {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 28, height: 18, rng_seed: 11 });
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = (0..dims.tile_count() as u32).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, AI, tile(&game, 3, 8));
    let iron = rules.resources.iter().position(|r| r.key == "core:resource/iron").unwrap();
    game.debug_set_deposit(rules, tile(&game, 3, 8), classic_rules::ids::ResourceId(iron as u16), 6);
    let bronze = rules.technologies.iter().position(|t| t.key == "core:tech/bronze-working").unwrap();
    game.debug_learn(rules, AI, classic_rules::ids::TechId(bronze as u16));
    let target = tile(&game, 16, 8);
    let city = game.debug_found_city(rules, ENEMY, target);
    game.debug_set_population(rules, city, 3);
    game.debug_spawn_unit(rules, ENEMY, target, unit_kind(rules, "core:unit/archer"));
    for (q, r) in [(10, 7), (10, 8)] {
        game.debug_spawn_unit(rules, AI, tile(&game, q, r), unit_kind(rules, "core:unit/archer"));
    }
    for (q, r) in [(11, 7), (11, 8), (11, 9)] {
        game.debug_spawn_unit(rules, AI, tile(&game, q, r), unit_kind(rules, "core:unit/swordsman"));
    }
    game.debug_spawn_unit(rules, AI, tile(&game, 10, 9), unit_kind(rules, "core:unit/catapult"));
    let everything: Vec<TileId> = (0..dims.tile_count() as u32).map(TileId).collect();
    game.debug_reveal(AI, &everything);
    game.debug_meet(AI, ENEMY);
    game.debug_earn(AI, 1_000);
    game.execute(rules, Command::DeclareWar { player: AI, enemy: ENEMY }).unwrap();
    (game, target)
}

/// The commands the AI applied over `rounds` rounds of the scene and the round
/// it took the capital.
fn play(variant: Variant, rounds: u32) -> (Vec<String>, Option<u32>) {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, target) = scene(&rules);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mind = Seated::new(ClassicMind::new(Variant::Standard), vec![(AI, ClassicMind::new(variant))]);
    let mut driver = AiDriver::<Classic, Seated<ClassicMind>>::with_mind(settings, Vec::new(), mind);
    driver.set_trace_capacity(1_000_000);
    let start = game.view().auth.turn().round;
    // The enemy's capital as the AI saw it once (the scene reveals the map, not the city).
    driver.state_mut(AI, start).see_city(SeenCity { tile: target, owner: ENEMY, population: 3, last_seen_round: start });
    let mut taken = None;
    while game.view().auth.turn().round < start + rounds && taken.is_none() {
        let round = game.view().auth.turn().round;
        driver.run(&mut game, &rules, u32::MAX);
        let auth = game.view().auth;
        let owner = auth.indexes().city_on_tile(target).and_then(|c| auth.cities().get(c)).map(|c| c.owner);
        if owner == Some(AI) {
            taken = Some(round);
        }
    }
    let applied = driver
        .trace()
        .entries()
        .filter(|e| e.player == AI && e.outcome == Outcome::Applied)
        .filter_map(|e| e.command.as_ref().map(|c| format!("{c:?}")))
        .collect();
    (applied, taken)
}

/// Unciv's units close on the city and strike it (the swordsmen charge, the
/// catapult sets up and shoots); the same scene played twice gives the same
/// commands. (`standard`'s diplomacy, which `unciv-military` keeps, makes
/// peace within a few rounds, so the capture is not asked for.)
#[test]
fn unciv_units_take_a_weak_city_and_play_deterministically() {
    let (commands, taken) = play(Variant::UncivMilitary, 20);
    let strikes = commands.iter().filter(|c| c.starts_with("RangedAttack") || c.starts_with("AttackCity") || c.starts_with("AttackUnit")).count();
    assert!(strikes > 0, "Unciv's units strike: {commands:#?}");
    let charges = commands.iter().filter(|c| c.starts_with("AttackCity")).count();
    let siege = commands.iter().filter(|c| c.starts_with("SetUpRangedAttack")).count();
    assert!(charges > 0 && siege > 0, "the melee units charge the city, the catapult sets up: {commands:#?}");
    let (again, taken_again) = play(Variant::UncivMilitary, 20);
    assert_eq!(commands, again, "deterministic");
    assert_eq!(taken, taken_again);
}

/// A whole game of four nations, every seat `unciv`: the nations found cities,
/// research, adopt policies and build; the rules refuse none of the AI's
/// orders; the same seed plays the same game.
#[test]
fn unciv_plays_whole_games_on_every_seat() {
    use classic_rules::scenario::{generate_new_game, ScenarioConfig};
    let rules = compile_rules(core_rules()).unwrap();
    let play = || {
        let config = ScenarioConfig { seed: 7, players: 4, width: 40, height: 28, cities: 0, units: 0, water_ratio: 25 };
        let mut game = generate_new_game(&rules, &config);
        let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
        let mut driver = AiDriver::<Classic, ClassicMind>::with_mind(settings, Vec::new(), ClassicMind::new(Variant::Unciv));
        driver.set_trace_capacity(1_000_000);
        while game.view().auth.turn().round < 90 && game.winner().is_none() {
            driver.run(&mut game, &rules, u32::MAX);
        }
        game.validate(&rules).unwrap();
        let auth = game.view().auth;
        let cities: Vec<usize> = (0..4).map(|p| auth.indexes().cities_of(PlayerId(p)).len()).collect();
        let techs: Vec<usize> = (0..4).map(|p| auth.players().techs_of(PlayerId(p)).len()).collect();
        let policies: Vec<usize> = (0..4).map(|p| auth.module().culture().of(PlayerId(p)).policies.len()).collect();
        let rejected = driver.trace().entries().filter(|e| matches!(e.outcome, Outcome::Rejected(_))).count();
        let applied = driver.trace().entries().filter(|e| e.outcome == Outcome::Applied).count();
        (cities, techs, policies, rejected, applied)
    };
    let (cities, techs, policies, rejected, applied) = play();
    eprintln!("unciv, 90 rounds: cities {cities:?}, techs {techs:?}, policies {policies:?}, {applied} applied, {rejected} rejected");
    assert!(cities.iter().all(|c| *c >= 2), "every nation founds a second city: {cities:?}");
    assert!(techs.iter().all(|t| *t >= 8), "every nation researches: {techs:?}");
    assert!(policies.iter().all(|p| *p >= 1), "every nation adopts policies: {policies:?}");
    assert_eq!(rejected, 0, "the rules refuse none of the orders");
    assert_eq!(play(), (cities, techs, policies, rejected, applied), "deterministic");
}

/// Unciv's modules inside `standard` (`std-unciv-*`, `ai::unciv::parts`), each on seat 0
/// of a 4-nation game against `standard`: its family deciding but not played (dry) leaves
/// the seat's game `standard`'s, command for command; played, the family changes the game
/// and the rules refuse none of its orders.
#[test]
fn unciv_modules_inside_standard_change_only_their_family() {
    use classic_rules::scenario::{generate_new_game, ScenarioConfig};
    let rules = compile_rules(core_rules()).unwrap();
    let play = |mind: ClassicMind| {
        let config = ScenarioConfig { seed: 5, players: 4, width: 40, height: 28, cities: 0, units: 0, water_ratio: 25 };
        let mut game = generate_new_game(&rules, &config);
        let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
        let seated = Seated::new(ClassicMind::new(Variant::Standard), vec![(AI, mind)]);
        let mut driver = AiDriver::<Classic, Seated<ClassicMind>>::with_mind(settings, Vec::new(), seated);
        driver.set_trace_capacity(1_000_000);
        while game.view().auth.turn().round < 90 && game.winner().is_none() {
            driver.run(&mut game, &rules, u32::MAX);
        }
        let applied: Vec<String> = driver.trace().entries().filter(|e| e.outcome == Outcome::Applied).filter_map(|e| e.command.as_ref().map(|c| format!("{:?} {c:?}", e.player))).collect();
        let rejected = driver.trace().entries().filter(|e| e.player == AI && matches!(e.outcome, Outcome::Rejected(_))).count();
        (applied, rejected)
    };
    let (standard, _) = play(ClassicMind::new(Variant::Standard));
    for v in Variant::ALL.into_iter().filter(|v| v.unciv().parts().any()) {
        let (dry, _) = play(ClassicMind::new(v).dry_unciv());
        assert!(dry == standard, "{}: deciding but not played, the game is `standard`'s", v.name());
        let (played, rejected) = play(ClassicMind::new(v));
        assert_eq!(rejected, 0, "{}: the rules refuse none of the orders", v.name());
        assert!(played != standard, "{}: its family changes the game", v.name());
    }
}
