//! The game events the player is told about although they happen on another player's turn:
//! the offer that lapsed unanswered (`ClassicChange::OfferLapsed`) and the steps of a rival
//! toward a victory (`derived::standing::victory_warnings`). The war declared on a player is the
//! journal's `PlayerChange::WarDeclared`, read in the interface.

use classic_content::core::core_rules;
use classic_rules::classic::ClassicChange;
use classic_rules::derived::standing::{victory_warnings, VictoryWarning};
use classic_rules::ids::{CityHandle, PlayerId, TechId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::{CityStateId, ProjectId};
use classic_rules::transaction::journal::PlayerChange;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::world::relations::{DealItem, PeaceRefusal};
use classic_rules::world::victory::spaceship_parts;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};

const GRASSLAND: u16 = 1;
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn game_of(rules: &CompiledRuleset, players: u8) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players, width: 30, height: 20, rng_seed: 4 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, P0, tile(&game, 3, 3));
    game.debug_found_city(rules, P1, tile(&game, 24, 15));
    if players > 2 {
        game.debug_found_city(rules, P2, tile(&game, 13, 9));
    }
    game.debug_meet_all();
    game
}

/// Plays one whole round; the changes of the module and of the players, in order.
fn end_round(game: &mut GameState, rules: &CompiledRuleset) -> (Vec<ClassicChange>, Vec<PlayerChange>) {
    let round = game.view().auth.turn().round;
    let (mut module, mut players) = (Vec::new(), Vec::new());
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        let result = game.execute(rules, Command::EndTurn { player: active }).unwrap();
        module.extend(result.changes().module_changes.iter().cloned());
        players.extend(result.changes().player_changes.iter().cloned());
    }
    (module, players)
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    while game.view().auth.active_player() != player {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

fn lapsed(changes: &[ClassicChange]) -> Vec<(PlayerId, PlayerId)> {
    changes
        .iter()
        .filter_map(|c| match *c {
            ClassicChange::OfferLapsed { from, to, .. } => Some((from, to)),
            _ => None,
        })
        .collect()
}

/// An offer nobody answers lapses on the turn of the nation it was made to, and its proposer is
/// the one told; an offer that was answered never lapses.
#[test]
fn an_unanswered_offer_lapses_and_names_its_proposer() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = game_of(&rules, 2);
    let writing = TechId(rules.technologies.iter().position(|t| t.key == "core:tech/writing").unwrap() as u16);
    for player in [P0, P1] {
        game.debug_learn(&rules, player, writing);
    }
    until_active(&mut game, &rules, P0);
    game.execute(&rules, Command::ProposeDeal { from: P0, to: P1, items: vec![(P0, DealItem::Embassy)] }).unwrap();
    let mut told = Vec::new();
    for _ in 0..3 {
        told.extend(lapsed(&end_round(&mut game, &rules).0));
    }
    assert_eq!(told, vec![(P0, P1)], "told once, to the proposer");
    assert!(game.view().auth.module().relations().offers().is_empty());

    until_active(&mut game, &rules, P0);
    let id = game.view().auth.module().relations().next_offer_id();
    game.execute(&rules, Command::ProposeDeal { from: P0, to: P1, items: vec![(P0, DealItem::Embassy)] }).unwrap();
    game.execute(&rules, Command::RespondDeal { player: P1, offer: id, accept: false, why: PeaceRefusal::Unsaid }).unwrap();
    let mut told = Vec::new();
    for _ in 0..3 {
        told.extend(lapsed(&end_round(&mut game, &rules).0));
    }
    assert!(told.is_empty(), "an answered offer does not lapse: {told:?}");
}

/// A war declared on a nation is the journal's `WarDeclared` with that nation as the enemy.
#[test]
fn a_war_declared_is_in_the_journal_with_its_victim() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = game_of(&rules, 2);
    until_active(&mut game, &rules, P1);
    let result = game.execute(&rules, Command::DeclareWar { player: P1, enemy: P0 }).unwrap();
    let wars: Vec<_> = result.changes().player_changes.iter().filter(|c| matches!(c, PlayerChange::WarDeclared { .. })).collect();
    assert_eq!(wars.len(), 1);
    assert!(matches!(*wars[0], PlayerChange::WarDeclared { player: P1, enemy: P0 }));
}

fn project_named(rules: &CompiledRuleset, key: &str) -> ProjectId {
    ProjectId(rules.module.projects.iter().position(|p| p.key == key).unwrap() as u16)
}

fn warnings(game: &GameState, rules: &CompiledRuleset, me: PlayerId) -> Vec<VictoryWarning> {
    victory_warnings(game.view().auth, rules, me)
}

/// The ship is a step at half of it and at its last part, and only of a rival.
#[test]
fn a_rival_with_half_the_ship_and_then_its_last_part_is_a_step_of_that_rival_only() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = game_of(&rules, 2);
    assert!(warnings(&game, &rules, P0).is_empty());
    game.debug_add_project(P1, project_named(&rules, "core:project/apollo_program"));
    let parts: Vec<ProjectId> = spaceship_parts(&rules).iter().flat_map(|(p, n)| std::iter::repeat(*p).take(usize::from(*n))).collect();
    assert_eq!(parts.len(), 6, "the ship of the rules");
    let mut seen = Vec::new();
    for part in &parts[..5] {
        game.debug_add_project(P1, *part);
        seen.push(warnings(&game, &rules, P0));
    }
    // Apollo and 1, 2 parts: nothing; 3 and 4 parts (half of 7 is 4): half; 5 parts: the last one is missing.
    assert!(seen[0].is_empty() && seen[1].is_empty(), "{seen:?}");
    assert_eq!(seen[2], vec![VictoryWarning::ShipHalf(P1)]);
    assert_eq!(seen[3], vec![VictoryWarning::ShipHalf(P1)]);
    assert_eq!(seen[4], vec![VictoryWarning::ShipLastPart(P1)]);
    assert!(warnings(&game, &rules, P1).is_empty(), "nobody is warned of its own ship");
}

/// A rival that holds every original capital but one is a step; the capture that makes it is
/// the one that is told.
#[test]
fn a_rival_that_holds_every_capital_but_one_is_a_step() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = game_of(&rules, 3);
    assert!(warnings(&game, &rules, P0).is_empty());
    until_active(&mut game, &rules, P2);
    game.execute(&rules, Command::DeclareWar { player: P2, enemy: P1 }).unwrap();
    let city: CityHandle = game.view().auth.indexes().cities_of(P1)[0];
    let at = game.view().auth.cities().get(city).unwrap().tile;
    let hex = game.view().auth.map().dimensions.to_coord(at).unwrap();
    let beside = tile(&game, hex.q - 1, hex.r);
    let warrior = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
    let attacker = game.debug_spawn_unit(&rules, P2, beside, warrior);
    let max = classic_rules::combat::city::max_hit_points(game.view().auth, &rules, game.view().auth.cities().get(city).unwrap());
    game.debug_set_city_damage(city, max - 1);
    game.execute(&rules, Command::AttackCity { attacker, city }).unwrap();
    assert_eq!(warnings(&game, &rules, P0), vec![VictoryWarning::LastCapital(P2)]);
    assert!(!warnings(&game, &rules, P2).contains(&VictoryWarning::LastCapital(P2)), "not its own step");
}

/// The vote for the World Leader is a step in the last turns before its session, for everyone.
#[test]
fn the_world_leader_vote_is_a_step_before_its_session() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = game_of(&rules, 3);
    game.debug_set_player_kind(P2, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    for i in 0..rules.technologies.len() {
        game.debug_learn(&rules, P0, TechId(i as u16));
        game.debug_learn(&rules, P1, TechId(i as u16));
    }
    game.debug_set_influence(&rules, P2, P0, 200);
    end_round(&mut game, &rules);
    assert!(game.view().auth.module().congress().united_nations());
    let mut told = 0;
    for _ in 0..30 {
        let congress = game.view().auth.module().congress();
        let near = congress.leader_vote_due() && congress.turns_until_session() <= 3;
        for me in [P0, P1] {
            assert_eq!(warnings(&game, &rules, me).contains(&VictoryWarning::LeaderVote), near, "{me:?}, {} turns", congress.turns_until_session());
        }
        told += usize::from(near);
        if game.outcome().is_some() {
            break;
        }
        end_round(&mut game, &rules);
    }
    assert!(told > 0, "a vote came near");
}
