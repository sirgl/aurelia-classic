//! Boundaries of the notices: a nation the player has not met is never named.
use classic_content::core::core_rules;
use classic_rules::derived::standing::{victory_warnings, VictoryWarning};
use classic_rules::derived::tourism;
use classic_rules::ids::{PlayerId, TerrainId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::{CityStateId, ProjectId};
use classic_rules::transaction::journal::PlayerChange;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::world::victory::spaceship_parts;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// Three players on grassland, NOBODY has met anybody.
fn strangers(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 3, width: 30, height: 20, rng_seed: 4 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(1))).collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, P0, tile(&game, 3, 3));
    game.debug_found_city(rules, P1, tile(&game, 24, 15));
    game.debug_found_city(rules, P2, tile(&game, 13, 9));
    game
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    while game.view().auth.active_player() != player {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

fn project_named(rules: &CompiledRuleset, key: &str) -> ProjectId {
    ProjectId(rules.module.projects.iter().position(|p| p.key == key).unwrap() as u16)
}

/// The steps of a rival the player has never met are not warnings: they would name a stranger.
#[test]
fn a_rival_the_player_has_not_met_is_not_a_victory_warning() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = strangers(&rules);
    assert!(!tourism::met(game.view().auth, P0, P1), "P0 has not met P1");
    game.debug_add_project(P1, project_named(&rules, "core:project/apollo_program"));
    let parts: Vec<ProjectId> = spaceship_parts(&rules).iter().flat_map(|(p, n)| std::iter::repeat(*p).take(usize::from(*n))).collect();
    for part in &parts[..3] {
        game.debug_add_project(P1, *part);
    }
    let w = victory_warnings(game.view().auth, &rules, P0);

    assert!(w.is_empty(), "a nation the player has not met must not be named: {w:?}");
}

/// A city-state allied to the victim joins the war on the aggressor (the player) although it never met it: the journal holds the war, so the notice must check the acquaintance itself.
#[test]
fn an_allied_city_state_joins_a_war_without_meeting_the_aggressor() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = strangers(&rules);
    game.debug_set_player_kind(P2, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    game.debug_meet(P0, P1);
    game.debug_meet(P1, P2);
    game.debug_set_influence(&rules, P2, P1, 300);
    for _ in 0..2 {
        let round = game.view().auth.turn().round;
        while game.view().auth.turn().round == round {
            let a = game.view().auth.active_player();
            game.execute(&rules, Command::EndTurn { player: a }).unwrap();
        }
    }
    until_active(&mut game, &rules, P0);
    assert!(!tourism::met(game.view().auth, P0, P2), "P0 has not met the city-state");
    let result = game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    let wars: Vec<_> = result.changes().player_changes.iter().filter_map(|c| match *c {
        PlayerChange::WarDeclared { player, enemy } => Some((player, enemy)),
        _ => None,
    }).collect();

    assert!(wars.contains(&(P2, P0)), "the allied city-state declares war on the player: {wars:?}");
}
