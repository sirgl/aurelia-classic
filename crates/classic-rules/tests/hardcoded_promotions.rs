//! Promotions whose behaviour the catalog leaves to the game's code
//! (`hardcoded_promotion_*`, `great_general_receives_movement`): the
//! Hakkapeliitta lends its rate to a Great General stacked with it, and the
//! Bonus vs Helicopters promotion doubles the strength against helicopters.
//! Each test checks the oracle (`GameState::validate`).

use classic_content::core::core_rules;
use classic_rules::combat::exchange::AttackKind;
use classic_rules::combat::plan_attack;
use classic_rules::ids::{PlayerId, PromotionId, TileId, UnitHandle, UnitKindId};
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn setup() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let game = GameState::new(&rules, GameConfig { players: 2, width: 24, height: 16, rng_seed: 5 });
    (game, rules)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn promotion(rules: &CompiledRuleset, key: &str) -> PromotionId {
    PromotionId(rules.promotions.iter().position(|p| p.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn moves(game: &GameState, unit: UnitHandle) -> (u16, u16) {
    let core = game.view().auth.units().get(unit).unwrap().core;
    (core.moves_left, core.moves_max)
}

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

/// The Hakkapeliitta's Great General moves at its rate (four moves, not the
/// general's two) if they start the turn stacked; apart, or stacked with
/// another horseman, it keeps its own two.
#[test]
fn a_great_general_moves_at_the_rate_of_the_hakkapeliitta() {
    let rate_of_general = |lender: &str, together: bool| {
        let (mut game, rules) = setup();
        let general = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), unit(&rules, "core:unit/great-general"));
        let at = if together { tile(&game, 5, 5) } else { tile(&game, 7, 5) };
        game.debug_spawn_unit(&rules, P0, at, unit(&rules, lender));
        next_turn_of(&mut game, &rules, P0);
        game.validate(&rules).unwrap();
        moves(&game, general)
    };
    let two = 2 * classic_rules::world::unit::MOVE_DENOMINATOR;
    assert_eq!(rate_of_general("core:unit/hakkapeliitta", true), (4 * two / 2, 4 * two / 2), "four moves when stacked");
    assert_eq!(rate_of_general("core:unit/hakkapeliitta", false), (two, two), "apart: its own two");
    assert_eq!(rate_of_general("core:unit/horseman", true), (two, two), "an ordinary horseman lends nothing");
}

/// Bonus vs Helicopters: +100 % strength against a helicopter, nothing
/// against other units.
#[test]
fn the_anti_helicopter_promotion_doubles_the_strength_against_helicopters() {
    let strength = |defender: &str, promoted: bool| {
        let (mut game, rules) = setup();
        let attacker = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 5), unit(&rules, "core:unit/warrior"));
        game.debug_spawn_unit(&rules, P1, tile(&game, 6, 5), unit(&rules, defender));
        game.debug_meet(P0, P1);
        game.debug_force_war(&rules, P0, P1);
        if promoted {
            game.debug_grant_promotion(&rules, attacker, promotion(&rules, "core:promotion/anti-helicopter"));
        }
        game.validate(&rules).unwrap();
        plan_attack(game.view(), &rules, attacker, tile(&game, 6, 5), AttackKind::Melee).unwrap().attacker_strength
    };
    let helicopter = "core:unit/helicopter-gunship";
    assert_eq!(strength(helicopter, true) - strength(helicopter, false), 8 * 100);
    assert_eq!(strength("core:unit/warrior", true), strength("core:unit/warrior", false));
}

// ---- Ancient Ruins and Native Tongue ----

/// The rank of a reward (0 best) from what a player gained: the citizens,
/// culture, faith, gold and experience, and whether the map was shown
/// (the numbers of `systems::ruins`: 60 gold, 40 culture, 30 faith, 10
/// experience, a map beyond sight); 6 when none fits.
fn ruins_rank(citizens: u32, culture: u32, faith: u32, gold: u32, experience: u32, map: bool) -> usize {
    match (citizens, culture, faith, gold, experience, map) {
        (1, 0, 0, 0, 0, _) => 0,
        (0, c, 0, 0, 0, _) if c == u64::from(classic_rules::ruins::CULTURE) as _ => 1,
        (0, 0, f, 0, 0, _) if f == u64::from(classic_rules::ruins::FAITH) as _ => 2,
        (0, 0, 0, g, 0, _) if g == u64::from(classic_rules::ruins::GOLD) as _ => 3,
        (0, 0, 0, 0, x, _) if x == u64::from(classic_rules::ruins::EXPERIENCE) as _ => 4,
        (0, 0, 0, 0, 0, true) => 5,
        _ => 6,
    }
}

/// What the unit that entered the ruins got: the rank of the reward in
/// `REWARDS` (0 best), and whether the ruins are gone.
fn ruins_reward(seed: u64, chooser: bool) -> (usize, bool) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 24, height: 16, rng_seed: seed });
    // An AI player is paid when its unit enters; a human claims (tests/ruins_choice.rs).
    game.configure_players(rules.default_difficulty(), &[P0]);
    let city = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let ruins = rules.improvements.iter().position(|i| i.key == "core:improvement/ancient-ruins").unwrap();
    let site = tile(&game, 12, 5);
    game.debug_set_improvement_batch(&rules, &[(site, classic_rules::ids::ImprovementId(ruins as u16))]);
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 11, 5), unit(&rules, "core:unit/warrior"));
    if chooser {
        game.debug_grant_promotion(&rules, warrior, promotion(&rules, "core:promotion/goody-hut-picker"));
    }
    let snapshot = |game: &GameState| {
        let auth = game.view().auth;
        let view = game.player_view(&rules, P0);
        let known = (0..auth.map().tile_count() as u32).filter(|t| view.is_known(TileId(*t))).count();
        (
            auth.cities().get(city).unwrap().population,
            auth.module().culture().player(P0).map_or(0, |c| c.culture),
            auth.module().religion().player(P0).faith,
            auth.players().treasury_of(P0),
            auth.units().get(warrior).unwrap().core.experience,
            known,
        )
    };
    let before = snapshot(&game);
    game.execute(&rules, Command::MoveUnit { unit: warrior, destination: site }).unwrap();
    let after = snapshot(&game);
    game.validate(&rules).unwrap();
    let gone = game.view().auth.map().improvement(site).flatten().is_none();
    let rank = ruins_rank(
        (after.0 - before.0) as u32,
        after.1 - before.1,
        after.2 - before.2,
        after.3 - before.3,
        after.4 - before.4,
        after.5 > before.5 + 12,
    );
    (rank, gone)
}

/// A unit of an AI player that enters Ancient Ruins takes one reward and the ruins are
/// gone; the unit with Native Tongue takes the best of the same three
/// offers, so it never does worse and, over many ruins, does better.
#[test]
fn native_tongue_takes_the_best_reward_of_the_ruins() {
    let mut better = 0;
    let mut seen = [false; 6];
    for seed in 0..20 {
        let (plain, plain_gone) = ruins_reward(seed, false);
        let (picked, picked_gone) = ruins_reward(seed, true);
        assert!(plain_gone && picked_gone, "the ruins are taken");
        assert!(picked <= plain, "seed {seed}: the chooser took rank {picked}, the plain unit {plain}");
        better += usize::from(picked < plain);
        assert!(plain < 6, "seed {seed}: a known reward");
        seen[plain] = true;
    }
    assert!(better >= 3, "the chooser does better in many ruins: {better}");
    assert!(seen.iter().filter(|s| **s).count() >= 4, "the plain unit gets various rewards");
}
