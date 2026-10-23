//! The memo of effect results (`kernel::effects::memo`): `player_modifiers`,
//! the connected-city list and the follower counts are answered from a memo
//! while the world does not change. These tests prove (1) that a memoised
//! game is the same game as an unmemoised one (world hash after every
//! round), (2) that the oracle mode (`Verify`: recompute every hit and
//! compare) finds no stale answer over whole AI games, and (3) that every
//! kind of change a game makes (a city founded, a unit moved, territory
//! gained, a technology or a policy learned, a deal accepted, a war
//! declared, a road built, a turn ended) is seen by the next answer.

use classic_content::core::core_rules;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::derived::connections::{compute_connected_cities, connected_cities};
use classic_rules::effects::memo::MemoMode;
use classic_rules::effects::player::{compute_player_modifiers, player_modifiers};
use classic_rules::ids::{PlayerId, TechId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::world::relations::DealItem;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

/// A whole AI game in `mode`: the world hash after every round.
fn play(
    rules: &CompiledRuleset,
    seed: u64,
    players: u8,
    rounds: u32,
    mode: MemoMode,
) -> (Vec<u64>, classic_rules::effects::memo::MemoStats) {
    let config = ScenarioConfig {
        seed,
        players,
        width: 48,
        height: 36,
        cities: 0,
        units: 0,
        water_ratio: 20,
    };
    let mut game = generate_new_game(rules, &config);
    game.set_memo_mode(mode);
    let mut driver = AiDriver::new(
        AiSettings::for_rules(rules, rules.default_difficulty()),
        Vec::new(),
    );
    let mut hashes = Vec::new();
    while game.view().auth.turn().round < rounds && game.winner().is_none() {
        let report = driver.run(&mut game, rules, u32::MAX);
        if report.waiting {
            break;
        }
        hashes.push(authoritative_hash(&game));
    }
    let stats = game.view().auth.memo().stats();
    (hashes, stats)
}

#[test]
fn a_memoised_game_is_the_same_game() {
    let rules = compile_rules(core_rules()).unwrap();
    for seed in [1u64, 2, 3] {
        let (off, off_stats) = play(&rules, seed, 4, 70, MemoMode::Off);
        let (on, on_stats) = play(&rules, seed, 4, 70, MemoMode::On);
        assert!(
            off.len() >= 60,
            "seed {seed}: the game ran {} rounds",
            off.len()
        );
        assert_eq!(off, on, "seed {seed}: the world hash differs in some round");
        assert_eq!(
            off_stats.hits + off_stats.misses,
            0,
            "Off never touches the memo"
        );
        assert!(
            on_stats.hits > on_stats.misses,
            "seed {seed}: the memo does not answer ({on_stats:?})"
        );
    }
}

#[test]
fn the_oracle_finds_no_stale_answer_in_ai_games() {
    // `Verify` recomputes every memo hit and asserts equality inside the memo.
    let rules = compile_rules(core_rules()).unwrap();
    for (seed, players) in [(11u64, 4u8), (12, 6), (13, 3)] {
        let (verified, stats) = play(&rules, seed, players, 90, MemoMode::Verify);
        let (plain, _) = play(&rules, seed, players, 90, MemoMode::Off);
        assert_eq!(verified, plain, "seed {seed}");
        assert!(stats.hits > 0, "seed {seed}: nothing was verified");
    }
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(
        rules
            .technologies
            .iter()
            .position(|t| t.key == key)
            .unwrap() as u16,
    )
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..8 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, Command::EndTurn { player: active })
            .unwrap();
    }
    panic!("never active");
}

/// Every memoised answer equals the computed one, for every player.
fn agrees(game: &GameState, rules: &CompiledRuleset, what: &str) {
    let auth = game.view().auth;
    for p in 0..auth.players().count() {
        let p = PlayerId(p as u8);
        // Twice: the second answer comes from the memo.
        for _ in 0..2 {
            assert_eq!(
                *player_modifiers(auth, rules, p),
                compute_player_modifiers(auth, rules, p),
                "{what}: modifiers of {p:?}"
            );
        }
        if let Some(capital) = classic_rules::world::city::capital_of(auth, rules, p) {
            for _ in 0..2 {
                assert_eq!(
                    *connected_cities(auth, rules, p, capital),
                    compute_connected_cities(auth, rules, p, capital),
                    "{what}: connected cities of {p:?}"
                );
            }
        }
    }
}

/// Three civilizations that met, one city each, far apart.
fn three_civs() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 3,
            width: 24,
            height: 16,
            rng_seed: 3,
        },
    );
    game.set_memo_mode(MemoMode::Verify);
    let scout = unit(&rules, "core:unit/scout");
    for (i, p) in [P0, P1, P2].into_iter().enumerate() {
        game.debug_spawn_unit(&rules, p, tile(&game, 10 + i as i32, 8), scout);
        game.debug_found_city(&rules, p, tile(&game, 2 + 9 * i as i32, 2));
        for key in [
            "core:tech/writing",
            "core:tech/civil-service",
            "core:tech/chivalry",
            "core:tech/education",
        ] {
            game.debug_learn(&rules, p, tech(&rules, key));
        }
    }
    (game, rules)
}

#[test]
fn every_kind_of_change_reaches_the_next_answer() {
    let (mut game, rules) = three_civs();
    agrees(&game, &rules, "start");
    let prime = |game: &GameState| player_modifiers(game.view().auth, &rules, P0);
    let _ = prime(&game);

    // A technology learned (harness).
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/philosophy"));
    agrees(&game, &rules, "technology learned");

    // A policy adopted (harness).
    let policy = classic_rules::ids::PolicyId(
        rules
            .module
            .policies
            .iter()
            .position(|p| p.key == "core:policy/tradition/aristocracy")
            .unwrap() as u16,
    );
    game.debug_adopt_policy(&rules, P0, policy);
    agrees(&game, &rules, "policy adopted");

    // A city founded by a command, the territory it claims.
    let settler = unit(&rules, "core:unit/settler");
    until_active(&mut game, &rules, P0);
    let s = game.debug_spawn_unit(&rules, P0, tile(&game, 4, 8), settler);
    agrees(&game, &rules, "unit spawned");
    game.execute(&rules, Command::FoundCity { settler: s })
        .unwrap();
    agrees(&game, &rules, "city founded by a command");

    // A unit moved.
    until_active(&mut game, &rules, P1);
    let scout = game
        .view()
        .auth
        .units()
        .iter_handles()
        .find(|(_, u)| u.owner == P1)
        .map(|(h, _)| h)
        .unwrap();
    let _ = game.execute(
        &rules,
        Command::MoveUnit {
            unit: scout,
            destination: tile(&game, 15, 10),
        },
    );
    agrees(&game, &rules, "unit moved");

    // A road between two cities: the connected list changes.
    let auth = game.view().auth;
    let capital = classic_rules::world::city::capital_of(auth, &rules, P0).unwrap();
    let list_before = connected_cities(auth, &rules, P0, capital);
    let second = auth
        .indexes()
        .cities_of(P0)
        .iter()
        .copied()
        .find(|c| *c != capital)
        .unwrap();
    let (a, b) = (
        auth.cities().get(capital).unwrap().tile,
        auth.cities().get(second).unwrap().tile,
    );
    let (ca, cb) = (
        auth.map().dimensions.to_coord(a).unwrap(),
        auth.map().dimensions.to_coord(b).unwrap(),
    );
    let mut road = Vec::new();
    for q in ca.q.min(cb.q)..=ca.q.max(cb.q) {
        for r in ca.r.min(cb.r)..=ca.r.max(cb.r) {
            road.extend(game.view().auth.map().dimensions.to_tile(HexCoord { q, r }));
        }
    }
    game.debug_set_road_batch(&rules, &road);
    agrees(&game, &rules, "road built");
    let auth = game.view().auth;
    assert!(
        connected_cities(auth, &rules, P0, capital).len() >= list_before.len(),
        "a road never disconnects"
    );

    // Contact (harness), a deal accepted, a war declared.
    until_active(&mut game, &rules, P0);
    let id = game.view().auth.module().relations().next_offer_id();
    game.execute(
        &rules,
        Command::ProposeDeal {
            from: P0,
            to: P1,
            items: vec![(P1, DealItem::Embassy)],
        },
    )
    .unwrap();
    game.execute(
        &rules,
        Command::RespondDeal {
            player: P1,
            offer: id,
            accept: true,
        },
    )
    .unwrap();
    agrees(&game, &rules, "deal accepted");
    game.execute(
        &rules,
        Command::DeclareWar {
            player: P0,
            enemy: P2,
        },
    )
    .unwrap();
    agrees(&game, &rules, "war declared");

    // Turns end: growth, production, research, the city-state and religion steps.
    for _ in 0..40 {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active })
            .unwrap();
        agrees(&game, &rules, "turn ended");
    }
    game.validate(&rules).unwrap();
    let stats = game.view().auth.memo().stats();
    assert!(stats.hits > 0 && stats.misses > 0, "{stats:?}");
}

#[test]
fn the_memo_is_closed_while_the_world_may_change() {
    let (mut game, rules) = three_civs();
    game.set_memo_mode(MemoMode::On);
    let memo = |game: &GameState| game.view().auth.memo().stats();
    let first = player_modifiers(game.view().auth, &rules, P0);
    let again = player_modifiers(game.view().auth, &rules, P0);
    assert!(
        std::sync::Arc::ptr_eq(&first, &again),
        "the second answer is the stored one"
    );
    let stats = memo(&game);
    assert_eq!((stats.hits, stats.misses), (1, 1));

    // `world_mut` hands out a mutable world: the stored answers are dropped.
    let _ = game.world_mut();
    let after = player_modifiers(game.view().auth, &rules, P0);
    assert!(
        !std::sync::Arc::ptr_eq(&first, &after),
        "recomputed after world_mut"
    );

    // A command closes it too.
    until_active(&mut game, &rules, P0);
    let before = player_modifiers(game.view().auth, &rules, P0);
    game.execute(&rules, Command::EndTurn { player: P0 })
        .unwrap();
    let later = player_modifiers(game.view().auth, &rules, P0);
    assert!(
        !std::sync::Arc::ptr_eq(&before, &later),
        "recomputed after a command"
    );

    // A clone starts with an empty memo.
    let copy = game.clone();
    let hits = copy.view().auth.memo().stats().hits;
    let _ = player_modifiers(copy.view().auth, &rules, P0);
    assert_eq!(
        copy.view().auth.memo().stats().hits,
        hits,
        "the clone's first answer is computed"
    );
}
