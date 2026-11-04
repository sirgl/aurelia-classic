//! The presentation over real games: cues from the journal, and the guarantee that
//! playing them never changes the simulation.
use classic_rules::ids::{PlayerId, TileId, UnitKindId};
use classic_rules::transaction::journal::AppliedCommand;
use classic_rules::Command;
use render::gpu::unit_poses::Gait;

use super::{MapButton, Session};
use client::shell::presentation::cue::{Cue, Origin};
use crate::game::cues::{derive, Observer};
use client::shell::presentation::{Settings, Speed};
use crate::ui::model::{Action, GameRequest};
use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

fn new_match(seed: u64) -> Session {
    let request = MatchRequest {
        nation: "core:nation/rome".into(),
        seed,
        size: MapSize::Small,
        players: 3,
        water: 25,
    };
    let prepared = prepare_match(request).expect("world");
    Session::new(prepared.game, game_rules())
}

/// A neighbour of the selected unit it can step on this turn.
fn step_target(session: &mut Session) -> Option<TileId> {
    let orders = session.unit_orders()?;
    let width = session.view().width;
    let from = orders.tile;
    let candidates = [from + 1, from.wrapping_sub(1), from + width, from.wrapping_sub(width)];
    candidates.into_iter().map(TileId).find(|tile| {
        let route = session.path_preview(Some(tile.0));
        matches!(route, Some(r) if r.this_turn == r.tiles.len())
    })
}

#[test]
fn a_move_of_the_player_walks_its_unit_along_the_path() {
    let mut session = new_match(20260928);
    let unit = session.selection().selected_unit().expect("a unit at the start");
    let id = session.game().view().auth.units().get(unit).unwrap().core.id;
    let target = step_target(&mut session).expect("a free neighbour");
    session.click(Some(target), MapButton::Primary);
    let frame = session.advance_presentation(0.1);
    let pose = frame.poses.iter().find(|p| p.id == id.0).expect("the unit is posed");
    assert!(matches!(pose.gait, Gait::Walk { .. }), "{:?}", pose.gait);
    for _ in 0..30 {
        session.advance_presentation(0.1);
    }
    assert!(session.presentation().is_idle(), "the walk ends and the unit rests again");
}

/// P1 strikes a warrior of the player until it dies: the journal yields attacks with
/// damage, then a death; all of them concern the player. The warrior stands next to the
/// starting units, which keep the fight in sight.
#[test]
fn attacks_and_deaths_come_from_the_combat_journal() {
    let session = new_match(7);
    let rules = game_rules();
    let mut game = session.game().clone();
    let start = game.view().auth.indexes().units_of(PlayerId(0))[0];
    let start = game.view().auth.units().get(start).unwrap().core.tile;
    while game.view().auth.active_player() != PlayerId(1) {
        let player = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player }).unwrap();
    }
    // War needs contact (track C diplomacy).
    game.debug_meet(PlayerId(1), PlayerId(0));
    game.execute(&rules, Command::DeclareWar { player: PlayerId(1), enemy: PlayerId(0) }).unwrap();
    let dims = game.view().auth.map().dimensions;
    let free = |game: &classic_rules::GameState, t: TileId| {
        let auth = game.view().auth;
        classic_rules::navigation::cost::enter_cost_on_map(auth.map(), t).is_some()
            && auth.indexes().units_on(t).is_empty()
            && auth.indexes().city_on_tile(t).is_none()
    };
    let (tile, beside) = classic_rules::math::tiles_in_hex_radius(&dims, start, 1)
        .into_iter()
        .filter(|t| *t != start && free(&game, *t))
        .find_map(|t| {
            let beside = classic_rules::math::tiles_in_hex_radius(&dims, t, 1).into_iter().find(|b| {
                *b != t && *b != start && dims.distance(*b, start).is_some_and(|d| d <= 2) && free(&game, *b)
            })?;
            Some((t, beside))
        })
        .expect("free land next to the starting units");
    let warrior = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
    let target = game.debug_spawn_unit(&rules, PlayerId(0), tile, warrior);
    let mut records = Vec::new();
    for _ in 0..40 {
        if game.view().auth.units().get(target).is_none() {
            break;
        }
        let attacker = game.debug_spawn_unit(&rules, PlayerId(1), beside, warrior);
        let command = Command::AttackUnit { attacker, defender: target };
        let result = game.execute(&rules, command.clone()).unwrap();
        records.push(AppliedCommand { command, changes: result.into_journal() });
    }
    let observer = Observer::new(game.view(), &rules, PlayerId(0));
    let cues = derive(&observer, &records);
    let attacks: Vec<_> = cues.iter().filter_map(|d| match &d.cue { Cue::Attack(a) => Some(a), _ => None }).collect();
    assert_eq!(attacks.len(), records.len(), "one attack cue per strike");
    assert!(attacks.iter().all(|a| a.target == tile && a.from == beside && a.attacker_seen));
    assert!(attacks[..attacks.len() - 1].iter().all(|a| a.dealt.is_some() && !a.kill));
    assert!(attacks.last().unwrap().kill, "the last strike kills");
    let deaths = cues.iter().filter(|d| matches!(d.cue, Cue::Death { tile: t, .. } if t == tile)).count();
    assert_eq!(deaths, 1);
    // The winner then steps into the cleared tile: a move of another player.
    let fight = cues.iter().filter(|d| !matches!(d.cue, Cue::Move { .. }));
    assert!(fight.into_iter().all(|d| d.origin == Origin::Other && d.concerns_observer));
}

/// Plays a scripted game: found the capital, then every round step the selected unit
/// if it can and end the turn. `frames` presentation frames run after every action.
fn scripted_hashes(settings: Settings, frames: usize, rounds: usize) -> Vec<u64> {
    let mut session = new_match(20261013);
    session.set_presentation_settings(settings);
    session.request(GameRequest::Unit(Action::FoundCity));
    let mut hashes = Vec::new();
    for _ in 0..rounds {
        session.select_next_idle_unit();
        if let Some(target) = step_target(&mut session) {
            session.click(Some(target), MapButton::Primary);
        }
        for _ in 0..frames {
            session.advance_presentation(0.1);
        }
        session.end_turn();
        for _ in 0..frames {
            session.advance_presentation(0.1);
        }
        hashes.push(session.world_hash());
    }
    hashes
}

#[test]
fn the_world_hash_is_the_same_with_animations_and_with_skip() {
    let animated = scripted_hashes(Settings::default(), 12, 25);
    let skipped = scripted_hashes(Settings { speed: Speed::Skip, ..Settings::default() }, 1, 25);
    let never_played = scripted_hashes(Settings::default(), 0, 25);
    assert_eq!(animated, skipped);
    assert_eq!(animated, never_played);
}

#[test]
fn a_hundred_ai_rounds_with_the_presentation_on_never_panic() {
    let mut session = new_match(42);
    session.request(GameRequest::Unit(Action::FoundCity));
    let mut posed = 0;
    for _ in 0..100 {
        session.end_turn();
        for _ in 0..4 {
            let frame = session.advance_presentation(0.1);
            posed += frame.poses.len();
            assert!(frame.poses.iter().all(|p| p.position.iter().all(|v| v.is_finite())));
        }
        assert!(session.presentation().waiting() <= 160);
    }
    assert!(session.turn() > 100, "turn {}", session.turn());
    assert!(posed > 0, "the other players' visible actions were shown");
}

/// A load replaces the world: no cue of the old one survives.
#[test]
fn loading_a_save_drops_the_presentation_of_the_old_world() {
    let mut session = new_match(20260928);
    let save = session.save_bytes();
    let target = step_target(&mut session).expect("a free neighbour");
    session.click(Some(target), MapButton::Primary);
    assert!(!session.advance_presentation(0.05).poses.is_empty());
    session.load_bytes(&save).expect("load");
    let frame = session.advance_presentation(0.05);
    assert!(frame.poses.is_empty() && frame.marks.is_empty());
    assert!(session.presentation().is_idle());
}

/// The player's city strikes from its banner: the bow aims, a map click on
/// an enemy unit two tiles away fires; the strike becomes a city strike cue
/// with the damage dealt, and the hovered target shows the odds first.
#[test]
fn a_city_strike_from_the_banner_hits_and_shows_its_damage() {
    let mut session = new_match(7);
    session.request(GameRequest::Unit(Action::FoundCity));
    let rules = game_rules();
    let me = session.me();
    let (city, city_tile) = {
        let auth = session.game().view().auth;
        let city = auth.indexes().cities_of(me)[0];
        (city, auth.cities().get(city).unwrap().tile)
    };
    let target = {
        let (game, _) = session.harness_game();
        let dims = game.view().auth.map().dimensions;
        let warrior = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap() as u16);
        let spot = classic_rules::math::tiles_in_hex_radius(&dims, city_tile, 2)
            .into_iter()
            .filter(|t| dims.distance(*t, city_tile) == Some(2))
            .find(|t| {
                let auth = game.view().auth;
                classic_rules::navigation::cost::enter_cost_on_map(auth.map(), *t).is_some() && auth.indexes().units_on(*t).is_empty()
            })
            .expect("land two tiles from the capital");
        game.debug_spawn_unit(&rules, PlayerId(1), spot, warrior);
        game.debug_meet(me, PlayerId(1));
        game.execute(&rules, Command::DeclareWar { player: me, enemy: PlayerId(1) }).unwrap();
        spot
    };
    session.refresh_after_harness();
    let _ = session.advance_presentation(5.0);
    session.request(GameRequest::CityStrike(city_tile.0));
    assert_eq!(session.aiming_city(), Some(city));
    let odds = session.hud_model(Some(target.0)).combat.expect("the strike's odds on the hovered target");
    assert!(odds.ranged && odds.to_defender.1 > 0);
    session.click(Some(target), MapButton::Primary);
    assert_eq!(session.aiming_city(), None);
    let auth = session.game().view().auth;
    assert!(auth.cities().get(city).unwrap().strike_made, "the city struck");
    let queued = session.presentation().queued_cues();
    let strike = queued.iter().find_map(|cue| match cue {
        Cue::CityStrike(strike) => Some(strike.clone()),
        _ => None,
    });
    let strike = strike.expect("a city strike cue");
    assert_eq!((strike.city, strike.target), (city_tile, target));
    assert!(strike.dealt.is_some_and(|d| d > 0));
}

/// Air commands of the core become flights, nuclear launches and parachute
/// jumps for the presentation (own units, so always seen).
#[test]
fn air_commands_become_their_own_cues() {
    let session = new_match(7);
    let rules = game_rules();
    let mut game = session.game().clone();
    let dims = game.view().auth.map().dimensions;
    let start = {
        let u = game.view().auth.indexes().units_of(PlayerId(0))[0];
        game.view().auth.units().get(u).unwrap().core.tile
    };
    let land: Vec<TileId> = classic_rules::math::tiles_in_hex_radius(&dims, start, 6)
        .into_iter()
        .filter(|t| classic_rules::navigation::cost::enter_cost_on_map(game.view().auth.map(), *t).is_some())
        .collect();
    let at = |d: i32| land.iter().copied().find(|t| dims.distance(start, *t) == Some(d)).expect("land");
    let (home, away, drop) = (start, at(5), at(3));
    let kind = |key: &str| UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16);
    game.debug_found_city(&rules, PlayerId(0), home);
    game.debug_found_city(&rules, PlayerId(0), away);
    game.debug_reveal(PlayerId(0), &classic_rules::math::tiles_in_hex_radius(&dims, home, 8));
    let fighter = game.debug_spawn_unit(&rules, PlayerId(0), home, kind("core:unit/fighter"));
    let trooper = game.debug_spawn_unit(&rules, PlayerId(0), home, kind("core:unit/paratrooper"));
    let mut records = Vec::new();
    for command in [Command::Rebase { unit: fighter, city: game.view().auth.indexes().city_on_tile(away).unwrap() }, Command::Paradrop { unit: trooper, destination: drop }] {
        let result = game.execute(&rules, command.clone()).unwrap();
        records.push(AppliedCommand { command, changes: result.into_journal() });
    }
    let observer = Observer::new(game.view(), &rules, PlayerId(0));
    let cues = derive(&observer, &records);
    let flight = cues.iter().find_map(|d| match &d.cue { Cue::Flight { from, to, reach, .. } => Some((*from, *to, *reach)), _ => None });
    assert_eq!(flight, Some((home, away, 5)), "the rebase is a flight of 5 tiles");
    let jump = cues.iter().find_map(|d| match &d.cue { Cue::Paradrop { from, to, reach, .. } => Some((*from, *to, *reach)), _ => None });
    assert_eq!(jump, Some((home, drop, 3)), "the jump is a paradrop cue, not a walk");
    assert!(!cues.iter().any(|d| matches!(&d.cue, Cue::Move { unit, .. } if unit.kind == kind("core:unit/paratrooper").0)), "no walking cue for the jump");
    assert!(cues.iter().all(|d| d.origin == Origin::Own));
}

/// A sweep of the player is a sortie of the aircraft to the tile (it stays at home); the
/// strike cue carries the distance, and the sweeper is the attacker.
#[test]
fn an_air_sweep_becomes_a_sortie_cue() {
    let session = new_match(7);
    let rules = game_rules();
    let mut game = session.game().clone();
    let dims = game.view().auth.map().dimensions;
    let home = {
        let u = game.view().auth.indexes().units_of(PlayerId(0))[0];
        game.view().auth.units().get(u).unwrap().core.tile
    };
    let kind = UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/fighter").unwrap() as u16);
    game.debug_found_city(&rules, PlayerId(0), home);
    let fighter = game.debug_spawn_unit(&rules, PlayerId(0), home, kind);
    let target = classic_rules::math::tiles_in_hex_radius(&dims, home, 3)
        .into_iter()
        .find(|t| dims.distance(home, *t) == Some(3))
        .expect("a tile three away");
    let command = Command::AirSweep { unit: fighter, target };
    let result = game.execute(&rules, command.clone()).unwrap();
    let records = vec![AppliedCommand { command, changes: result.into_journal() }];
    let observer = Observer::new(game.view(), &rules, PlayerId(0));
    let cues = derive(&observer, &records);
    let sortie = cues.iter().find_map(|d| match &d.cue {
        Cue::Attack(a) => Some((a.attacker.kind, a.from, a.target, a.reach, a.defender.is_none())),
        _ => None,
    });
    assert_eq!(sortie, Some((kind.0, home, target, 3, true)), "a sortie of 3 tiles, nobody there to fight");
    assert!(!cues.iter().any(|d| matches!(&d.cue, Cue::Move { .. } | Cue::Flight { .. })), "the sweeper does not change base");
    assert!(cues.iter().all(|d| d.origin == Origin::Own));
}
