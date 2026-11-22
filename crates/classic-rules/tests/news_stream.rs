//! The rules events the news stream carries (`ClassicChange`, `PlayerChange::Eliminated`): a
//! world wonder built and a nation eliminated. (Theft, dropped proposals, first contacts and
//! quests are in `espionage_outcomes`, `congress_classic` and `city_states_classic`.)

use classic_content::core::core_rules;
use classic_rules::classic::{Audience, ClassicChange};
use classic_rules::ids::{BuildingKindId, PlayerId, ProductionItem, TechId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::BuildingKind;
use classic_rules::transaction::journal::PlayerChange;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// A world wonder that completes in a city is told to every nation, with its builder.
#[test]
fn a_completed_world_wonder_is_announced_to_every_nation() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 });
    for t in 0..rules.technologies.len() {
        game.debug_learn(&rules, P0, TechId(t as u16));
    }
    let city = game.debug_found_city(&rules, P0, tile(&game, 6, 6));
    game.debug_found_city(&rules, P1, tile(&game, 22, 12));
    game.debug_set_population(&rules, city, 20);
    let wonder = (0..rules.buildings.len())
        .filter(|&i| rules.buildings[i].kind == BuildingKind::WorldWonder)
        .map(|i| ProductionItem::Building(BuildingKindId(i as u16)))
        .find(|&item| game.execute(&rules, Command::EnqueueProduction { player: P0, city, item }).is_ok())
        .expect("a world wonder the city accepts");
    let ProductionItem::Building(building) = wonder else { unreachable!() };
    let mut built = Vec::new();
    for _ in 0..300 {
        let active = game.view().auth.active_player();
        let result = game.execute(&rules, Command::EndTurn { player: active }).unwrap();
        built.extend(result.changes().module_changes.iter().copied().filter(|c| matches!(c, ClassicChange::WonderBuilt { .. })));
        if !built.is_empty() {
            break;
        }
    }
    let first = ClassicChange::WonderBuilt { player: P0, city, building };
    assert_eq!(built.first(), Some(&first), "the wonder is told when it completes");
    assert_eq!(first.audience(), Audience::All);
    assert!(first.is_told_to(P1) && first.is_told_to(P0));
}

/// A nation that loses its last unit before it ever had a city is told as eliminated.
#[test]
fn a_nation_that_is_destroyed_is_in_the_journal() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 3, width: 30, height: 20, rng_seed: 5 });
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, PlayerId(2), tile(&game, 20, 12));
    let victim_tile = tile(&game, 10, 10);
    let warrior = classic_rules::ids::UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
    let victim = game.debug_spawn_unit(&rules, P1, victim_tile, warrior);
    let attack_from = tile(&game, 9, 10);
    let mut killer = game.debug_spawn_unit(&rules, P0, attack_from, warrior);
    game.debug_meet(P0, P1);
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    let mut eliminated = Vec::new();
    for _ in 0..40 {
        if !game.view().auth.players().is_alive(P1) {
            break;
        }
        while game.view().auth.active_player() != P0 {
            let active = game.view().auth.active_player();
            game.execute(&rules, Command::EndTurn { player: active }).unwrap();
        }
        if game.view().auth.units().get(killer).is_none() {
            killer = game.debug_spawn_unit(&rules, P0, attack_from, warrior);
        }
        if let Ok(result) = game.execute(&rules, Command::AttackUnit { attacker: killer, defender: victim }) {
            eliminated.extend(result.changes().player_changes.iter().filter_map(|c| match c {
                PlayerChange::Eliminated { player } => Some(*player),
                _ => None,
            }));
        }
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
    }
    assert!(!game.view().auth.players().is_alive(P1), "the nation is destroyed");
    assert_eq!(eliminated, vec![P1], "the journal tells it once");
}
