//! Defects the full-game harness (integration/crates/aurelia-qa, classic/docs/QA.md) found in
//! AI-only classic games, each reduced to the smallest world that shows it.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ids::{PlayerId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::city_states::Personality;
use classic_rules::world::player::PlayerKind;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};
use classic_rules::world::player::ClassicKind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const CS: PlayerId = PlayerId(2);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn new_game(players: u8) -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let game = GameState::new(
        &rules,
        GameConfig {
            players,
            width: 30,
            height: 20,
            rng_seed: 9,
        },
    );
    (game, rules)
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..8 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    panic!("{player:?} never became active");
}

/// A missionary enters rival territory without open borders. Its march
/// used to take the path through a closed border (the path search knows
/// the ability) and then fail at the border step after moving: the command
/// was rejected with the unit already moved, and sight and knowledge
/// never learned of the move (round 193 of fractal/small/seed 1).
#[test]
fn a_missionary_marches_through_closed_borders() {
    let (mut game, rules) = new_game(2);
    game.debug_found_city(&rules, P1, tile(&game, 10, 8));
    let missionary = game.debug_spawn_unit(&rules, P0, tile(&game, 6, 8), unit(&rules, "core:unit/missionary"));
    let foreign = |g: &GameState, t: TileId| g.view().auth.map().owner(t).flatten() == Some(P1);
    let destination = tile(&game, 10, 9);
    assert!(foreign(&game, destination), "the destination lies behind P1's closed border");
    let mut entered = false;
    for _ in 0..4 {
        until_active(&mut game, &rules, P0);
        let at = game.view().auth.units().get(missionary).unwrap().core.tile;
        if at == destination {
            break;
        }
        game.execute(&rules, Command::MarchTo { unit: missionary, destination })
            .expect("the march goes on into rival territory");
        game.validate(&rules).expect("sight and knowledge follow every step");
        entered |= foreign(&game, game.view().auth.units().get(missionary).unwrap().core.tile);
        game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
    }
    assert!(entered, "the missionary walked into P1's land");
}

/// A city-state that loses its last city is eliminated with its units
/// and its friend loses the maritime food. It used
/// to live on with its units, and when those died its friend's yields kept
/// the bonus (yields mismatch, round 197 of continents/small/seed 1).
#[test]
fn a_city_state_without_cities_is_eliminated_with_its_units() {
    let (mut game, rules) = new_game(3);
    let byblos = rules.module.city_states.iter().position(|c| c.key == "core:city-state/byblos").unwrap();
    game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(CityStateId(byblos as u16))));
    game.debug_set_personality(CS, Personality::Neutral);
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    let city = game.debug_found_city(&rules, CS, tile(&game, 13, 9));
    let straggler = game.debug_spawn_unit(&rules, CS, tile(&game, 18, 4), unit(&rules, "core:unit/warrior"));
    game.debug_meet_all();
    let food = |g: &GameState| g.ai_observation(&rules, P0).own_cities[0].income.food_surplus;
    let without_friend = food(&game);
    game.debug_set_influence(&rules, CS, P0, 60);
    assert!(food(&game) > without_friend, "the maritime ally feeds P0's capital");

    until_active(&mut game, &rules, P1);
    game.execute(&rules, Command::DeclareWar { player: P1, enemy: CS }).unwrap();
    let attacker = game.debug_spawn_unit(&rules, P1, tile(&game, 12, 9), unit(&rules, "core:unit/warrior"));
    let core = game.view().auth.cities().get(city).unwrap();
    let max = classic_rules::combat::city::max_hit_points(game.view().auth, &rules, core);
    game.debug_set_city_damage(city, max - 1);
    game.execute(&rules, Command::AttackCity { attacker, city }).unwrap();

    let auth = game.view().auth;
    assert!(auth.indexes().cities_of(CS).is_empty(), "the city fell");
    assert!(!auth.players().is_alive(CS), "a city-state without a city is gone");
    assert!(auth.units().get(straggler).is_none(), "its units went with it");
    assert_eq!(food(&game), without_friend, "no friend, no food");
    game.validate(&rules).expect("yields follow the elimination");
}

/// A nation that never had a city lives on its units (its first
/// settlers are walking); one that had a city does not.
#[test]
fn units_keep_alive_only_a_nation_before_its_first_city() {
    let (mut game, rules) = new_game(2);
    let settler = game.debug_spawn_unit(&rules, P1, tile(&game, 20, 10), unit(&rules, "core:unit/settler"));
    game.debug_spawn_unit(&rules, P1, tile(&game, 21, 10), unit(&rules, "core:unit/warrior"));
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    until_active(&mut game, &rules, P0);
    game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
    assert!(game.view().auth.players().is_alive(P1), "settlers on the way");
    game.execute(&rules, Command::FoundCity { settler }).unwrap();
    assert!(game.view().auth.players().is_alive(P1));
}

/// A city-state that becomes someone's ally joins its ally's wars; the
/// enemy it now fights is no longer its friend and loses the maritime
/// food. The food stayed in the enemy's yields until something else
/// recomputed them (yields mismatch, round 79 of fractal/small/seed 2).
#[test]
fn a_city_state_joining_its_allys_war_ends_the_enemys_friendship_bonus() {
    let (mut game, rules) = new_game(3);
    let byblos = rules.module.city_states.iter().position(|c| c.key == "core:city-state/byblos").unwrap();
    game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(CityStateId(byblos as u16))));
    game.debug_set_personality(CS, Personality::Neutral);
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    game.debug_found_city(&rules, CS, tile(&game, 13, 9));
    game.debug_meet_all();
    let food = |g: &GameState| g.ai_observation(&rules, P1).own_cities[0].income.food_surplus;
    let without_friend = food(&game);
    game.debug_set_influence(&rules, CS, P1, 30);
    assert!(food(&game) > without_friend, "P1 is a maritime friend");
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();

    game.debug_set_influence(&rules, CS, P0, 60);
    assert_eq!(game.view().auth.module().city_states().ally(CS), Some(P0));
    assert!(game.view().auth.diplomacy().at_war(CS, P1), "the ally's war is the city-state's");
    assert_eq!(food(&game), without_friend, "no friendship at war");
    game.validate(&rules).expect("yields follow the war");
}

/// Scientists, engineers, merchants and generals build their improvement
/// on an own tile. The catalog gave one only to the prophet, so the
/// command refused them and they stood idle for the rest of the game.
#[test]
fn great_people_build_academy_manufactory_customs_house_and_citadel() {
    let (mut game, rules) = new_game(2);
    game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let people = [
        ("great-scientist", "academy"),
        ("great-engineer", "manufactory"),
        ("great-merchant", "customs-house"),
        ("great-general", "citadel"),
    ];
    let sites = [(4, 5), (6, 5), (5, 4), (5, 6)];
    for ((person, improvement), (q, r)) in people.into_iter().zip(sites) {
        let at = tile(&game, q, r);
        let unit = game.debug_spawn_unit(&rules, P0, at, unit(&rules, &format!("core:unit/{person}")));
        game.execute(&rules, Command::CreateGreatImprovement { unit })
            .unwrap_or_else(|e| panic!("{person}: {e:?}"));
        let built = game.view().auth.map().improvement(at).flatten().map(|i| rules.improvements[i.0 as usize].key.clone());
        assert_eq!(built.as_deref(), Some(format!("core:improvement/{improvement}").as_str()));
    }
    game.validate(&rules).unwrap();
}

/// A Great Merchant the AI gets in its capital walks out of the city and
/// builds a Customs House on an own tile (scientists bulb, engineers hurry
/// when they can: `ai::great_people`). Builders used to stand idle in the
/// capital for the rest of the game.
#[test]
fn the_ai_builds_a_customs_house_with_its_great_merchant() {
    use classic_rules::ai::{AiDriver, AiSettings};
    let (mut game, rules) = new_game(2);
    let capital = tile(&game, 5, 5);
    game.debug_found_city(&rules, P0, capital);
    game.debug_found_city(&rules, P1, tile(&game, 25, 15));
    let merchant = game.debug_spawn_unit(&rules, P0, capital, unit(&rules, "core:unit/great-merchant"));
    let customs = rules.improvements.iter().position(|i| i.key == "core:improvement/customs-house").unwrap();
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), vec![P1]);
    for _ in 0..4 {
        driver.run(&mut game, &rules, u32::MAX);
        if game.view().auth.active_player() == P1 {
            game.execute(&rules, Command::EndTurn { player: P1 }).unwrap();
        }
    }
    assert!(game.view().auth.units().get(merchant).is_none(), "the merchant was spent");
    let map = game.view().auth.map();
    let built = (0..map.tile_count() as u32)
        .any(|t| map.improvement(TileId(t)).flatten().is_some_and(|i| i.0 as usize == customs));
    assert!(built, "a Customs House stands on P0's land");
    game.validate(&rules).unwrap();
}
