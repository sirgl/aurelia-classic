//! Barbarians on the classic ruleset: camps appear in
//! the fog from the first barbarian turn (never with no barbarians), spawn
//! wandering units from the release round on (Unciv: BarbarianManager), are cleared for gold; barbarians capture
//! civilians; their AI attacks, pillages and marches; the state survives
//! save and load; the new-game options place city-states and barbarians.

use classic_rules::classic::ClassicChange;
use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::barbarians::barbarian_orders;
use classic_rules::ids::{PlayerId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::replay::authoritative_hash;
use classic_rules::scenario::{generate_new_game_with, NewGameOptions, ScenarioConfig};
use classic_rules::world::barbarians::{
    BarbarianMode, CAMP_GOLD, EARLIEST_RELEASE_ROUND, MIN_DISTANCE_CITY,
};
use classic_rules::world::player::PlayerKind;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};
use classic_rules::world::player::ClassicKind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const BARBARIANS: PlayerId = PlayerId(2);

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

/// Two nations with a city each in opposite corners of a 30×20 map
/// and the barbarian player (P2) in `mode`.
fn world(mode: BarbarianMode) -> (GameState, CompiledRuleset) {
    world_on(compile_rules(core_rules()).unwrap(), mode)
}

/// `world` on the ruleset `rules`.
fn world_on(rules: CompiledRuleset, mode: BarbarianMode) -> (GameState, CompiledRuleset) {
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 3,
            width: 30,
            height: 20,
            rng_seed: 5,
        },
    );
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, P1, tile(&game, 26, 16));
    game.debug_set_player_kind(BARBARIANS, PlayerKind::Module(ClassicKind::Barbarian));
    game.debug_set_barbarian_mode(mode);
    (game, rules)
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
    panic!("{player:?} never became active");
}

/// End turns until the round `round` starts with P0 to move.
fn play_until(game: &mut GameState, rules: &CompiledRuleset, round: u32) {
    while game.view().auth.turn().round < round {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active })
            .unwrap();
    }
    until_active(game, rules, P0);
}

fn camps(game: &GameState) -> Vec<TileId> {
    game.view()
        .auth
        .module()
        .barbarians()
        .camps()
        .iter()
        .map(|c| c.tile)
        .collect()
}

/// Unciv BarbarianManager: camps from the first turn, 5+ tiles from major
/// capitals, a defender each; no wandering unit before turn 10.
#[test]
fn camps_appear_in_the_fog_from_the_first_turn() {
    let (mut game, rules) = world(BarbarianMode::Normal);
    play_until(&mut game, &rules, 2);
    let placed = camps(&game);
    assert!(!placed.is_empty(), "the first wave of camps");
    let auth = game.view().auth;
    let dims = auth.map().dimensions;
    for camp in &placed {
        assert!(
            auth.map().improvement(*camp).flatten().is_some(),
            "camp improvement"
        );
        for (_, city) in auth.cities().iter() {
            // Both cities are capitals of majors.
            assert!(dims.distance(city.tile, *camp).unwrap() > MIN_DISTANCE_CITY);
        }
    }
    let barbarians = auth.indexes().units_of(BARBARIANS).len();
    assert!(barbarians >= placed.len(), "every new camp has a defender");
    play_until(&mut game, &rules, EARLIEST_RELEASE_ROUND - 1);
    let auth = game.view().auth;
    assert_eq!(
        auth.indexes().units_of(BARBARIANS).len(),
        camps(&game).len(),
        "only defenders before the release round"
    );
}

#[test]
fn raging_barbarians_come_earlier_and_none_never() {
    let (mut raging, rules) = world(BarbarianMode::Raging);
    play_until(&mut raging, &rules, 2);
    assert!(!camps(&raging).is_empty(), "raging: camps from the first turn");

    let (mut none, rules) = world(BarbarianMode::None);
    play_until(&mut none, &rules, EARLIEST_RELEASE_ROUND + 10);
    assert!(camps(&none).is_empty());
    assert!(!none.view().auth.players().is_alive(BARBARIANS));
}

#[test]
fn camps_spawn_units_over_time() {
    let (mut game, rules) = world(BarbarianMode::Raging);
    play_until(&mut game, &rules, EARLIEST_RELEASE_ROUND);
    let first = game.view().auth.indexes().units_of(BARBARIANS).len();
    play_until(&mut game, &rules, EARLIEST_RELEASE_ROUND + 6);
    let later = game.view().auth.indexes().units_of(BARBARIANS).len();
    assert!(later > first, "spawns: {first} -> {later}");
}

#[test]
fn entering_a_camp_clears_it_for_gold() {
    let (mut game, rules) = world(BarbarianMode::Normal);
    let camp = tile(&game, 8, 3);
    game.debug_place_camp(&rules, camp);
    let warrior = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 7, 3),
        unit(&rules, "core:unit/warrior"),
    );
    until_active(&mut game, &rules, P0);
    let gold = game.view().auth.players().treasury_of(P0);
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: warrior,
            destination: camp,
        },
    )
    .unwrap();
    let auth = game.view().auth;
    assert!(camps(&game).is_empty());
    assert!(auth.map().improvement(camp).flatten().is_none());
    assert_eq!(auth.players().treasury_of(P0), gold + CAMP_GOLD);
    let cleared = auth.module().barbarians().cleared();
    assert_eq!(cleared.len(), 1);
    assert_eq!((cleared[0].tile, cleared[0].player), (camp, P0));
}

/// The journal names the camp gold the treasury received, and nothing about a conversion
/// when no unit of the camp joined.
#[test]
fn clearing_a_camp_journals_the_gold_it_gave() {
    let (mut game, rules) = world(BarbarianMode::Normal);
    let camp = tile(&game, 8, 3);
    game.debug_place_camp(&rules, camp);
    let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 7, 3), unit(&rules, "core:unit/warrior"));
    until_active(&mut game, &rules, P0);
    let result = game
        .execute(&rules, Command::MoveUnit { unit: warrior, destination: camp })
        .unwrap();
    let changes = &result.changes().module_changes;
    assert_eq!(
        changes.iter().filter(|c| matches!(c, ClassicChange::CampCleared { .. })).copied().collect::<Vec<_>>(),
        [ClassicChange::CampCleared { player: P0, tile: camp, gold: CAMP_GOLD }]
    );
    assert!(!changes.iter().any(|c| matches!(c, ClassicChange::CampUnitConverted { .. })));
    assert!(
        !changes.iter().any(|c| matches!(c, ClassicChange::Plundered { .. } | ClassicChange::KillYield { .. })),
        "the camp's gold is told once, as the camp's"
    );
}

/// At every game speed the journal names exactly the gold the treasury got (the camp gold
/// times the speed's gold percent).
#[test]
fn the_journaled_camp_gold_is_what_the_treasury_got_at_every_speed() {
    let speeds = compile_rules(classic_content::classic::classic_rules()).unwrap().speeds;
    assert!(speeds.iter().any(|s| s.values.gold_percent != 100), "a speed that scales gold");
    let mut received_by_speed = Vec::new();
    for speed in speeds.iter().map(|s| s.key.clone()) {
        let (mut game, rules) = world_on(compile_rules(classic_content::classic::classic_rules()).unwrap(), BarbarianMode::Normal);
        game.set_speed(&rules, &speed);
        let camp = tile(&game, 8, 3);
        game.debug_place_camp(&rules, camp);
        let warrior = game.debug_spawn_unit(&rules, P0, tile(&game, 7, 3), unit(&rules, "core:unit/warrior"));
        until_active(&mut game, &rules, P0);
        let before = game.view().auth.players().treasury_of(P0);
        let first = game.execute(&rules, Command::MoveUnit { unit: warrior, destination: camp }).unwrap();
        let received = game.view().auth.players().treasury_of(P0) - before;
        let expected = kernel::rules::speeds::SpeedValues::scale(CAMP_GOLD, game.view().auth.settings().speed_values().gold_percent);
        assert_eq!(received, expected, "{speed}");
        received_by_speed.push(received);
        assert_eq!(
            first.changes().module_changes.iter().find_map(|c| match c {
                ClassicChange::CampCleared { gold, .. } => Some(*gold),
                _ => None,
            }),
            Some(received),
            "{speed}: the journal names what the treasury got"
        );
    }
    assert!(received_by_speed.iter().any(|g| *g != CAMP_GOLD), "a speed other than Standard paid another sum: {received_by_speed:?}");
}

#[test]
fn barbarians_capture_civilians() {
    let (mut game, rules) = world(BarbarianMode::Normal);
    let worker = game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 10, 10),
        unit(&rules, "core:unit/worker"),
    );
    let brute = game.debug_spawn_unit(
        &rules,
        BARBARIANS,
        tile(&game, 11, 10),
        unit(&rules, "core:unit/warrior"),
    );
    until_active(&mut game, &rules, BARBARIANS);
    game.execute(
        &rules,
        Command::AttackUnit {
            attacker: brute,
            defender: worker,
        },
    )
    .unwrap();
    let auth = game.view().auth;
    let captive = auth
        .indexes()
        .units_on(tile(&game, 10, 10))
        .iter()
        .filter_map(|u| auth.units().get(*u))
        .find(|u| u.core.kind == unit(&rules, "core:unit/worker"))
        .expect("the worker is captured, not destroyed");
    assert_eq!(captive.core.owner, BARBARIANS);
}

#[test]
fn barbarian_ai_attacks_and_marches() {
    let (mut game, rules) = world(BarbarianMode::Normal);
    let warrior = unit(&rules, "core:unit/warrior");
    let near = game.debug_spawn_unit(&rules, BARBARIANS, tile(&game, 4, 5), warrior);
    let far = game.debug_spawn_unit(&rules, BARBARIANS, tile(&game, 9, 3), warrior);
    let lonely = game.debug_spawn_unit(&rules, BARBARIANS, tile(&game, 16, 10), warrior);
    game.debug_spawn_unit(
        &rules,
        P0,
        tile(&game, 4, 4),
        unit(&rules, "core:unit/worker"),
    );
    until_active(&mut game, &rules, BARBARIANS);
    let orders = barbarian_orders(&game.ai_observation(&rules, BARBARIANS), &rules);
    let attacks = |h| {
        orders
            .iter()
            .any(|c| matches!(c, Command::AttackUnit { attacker, .. } if *attacker == h))
    };
    let moves = |h| {
        orders
            .iter()
            .any(|c| matches!(c, Command::MarchTo { unit, .. } if *unit == h))
    };
    assert!(
        attacks(near),
        "the brute next to the worker attacks: {orders:?}"
    );
    assert!(
        moves(far),
        "the brute five tiles from the city marches: {orders:?}"
    );
    assert!(
        !attacks(lonely) && !moves(lonely),
        "nothing in sight: stays"
    );
    for cmd in orders {
        let _ = game.execute(&rules, cmd);
    }
}

#[test]
fn barbarian_state_survives_save_and_load() {
    let (mut game, rules) = world(BarbarianMode::Raging);
    play_until(&mut game, &rules, EARLIEST_RELEASE_ROUND + 3);
    assert!(!camps(&game).is_empty());
    let save = classic_rules::persistence::encode::encode(&game, &rules);
    let loaded = classic_rules::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    assert_eq!(camps(&loaded), camps(&game));
    assert_eq!(
        loaded.view().auth.module().barbarians().mode,
        BarbarianMode::Raging
    );
}

#[test]
fn new_game_options_place_city_states_and_barbarians() {
    let rules = compile_rules(core_rules()).unwrap();
    let cfg = ScenarioConfig {
        seed: 7,
        players: 4,
        width: 56,
        height: 36,
        cities: 0,
        units: 0,
        water_ratio: 30,
    };
    let options = NewGameOptions {
        barbarians: BarbarianMode::Normal,
        city_states: 4,
        ..Default::default()
    };
    let game = generate_new_game_with(&rules, &cfg, None, options);
    let players = game.view().auth.players();
    assert_eq!(players.count(), 9);
    for p in 0..4 {
        assert_eq!(players.kind_of(PlayerId(p)), PlayerKind::Major);
    }
    for p in 4..8 {
        let player = PlayerId(p);
        let PlayerKind::Module(ClassicKind::CityState(cs)) = players.kind_of(player) else {
            panic!("{player:?} is a city-state");
        };
        let cities = game.view().auth.indexes().cities_of(player);
        assert_eq!(cities.len(), 1, "{player:?} has its city");
        let city = game.view().auth.cities().get(cities[0]).unwrap();
        assert_eq!(city.name, rules.module.city_states[cs.0 as usize].name);
        assert!(!city.original_capital, "a city-state is no conquest target");
    }
    assert_eq!(players.kind_of(PlayerId(8)), PlayerKind::Module(ClassicKind::Barbarian));
    assert!(players.is_alive(PlayerId(8)));
    assert_eq!(
        game.view().auth.module().barbarians().mode,
        BarbarianMode::Normal
    );
}
