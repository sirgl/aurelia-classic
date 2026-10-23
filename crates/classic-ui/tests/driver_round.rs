//! A playable cycle without GPU: the driver's rounds move the game forward.
//! GameClient is built with Renderer::headless: all the logic is on the CPU.

use classic_rules::ClassicGame;
use client::simulation_driver::{DriverConfig, SimulationDriver};
use client::GameClient;
use classic_rules::ids::PlayerId;
use classic_rules::rules::SourceRules;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{compile_rules, Command};
use render::renderer::Renderer;
use std::sync::Arc;

fn small_game() -> GameClient<classic_rules::classic::Classic> {
    small_game_of(SourceRules::default())
}

fn small_game_of(source: SourceRules) -> GameClient<classic_rules::classic::Classic> {
    let rules = compile_rules(source).unwrap();
    let cfg = ScenarioConfig {
        seed: 77,
        players: 3,
        width: 16,
        height: 16,
        cities: 0,
        units: 0,
        water_ratio: 0,
    };
    let mut game = generate(&cfg, &rules);
    // Guaranteed 2 units per player: otherwise commit elimination
    // (no cities and no units = dead) kills the unitless before their EndTurn.
    let dims = game.view().auth.map().dimensions;
    // Cities scattered (they keep players alive), units clustered in the centre (contact ->
    // visibility -> wars and fights; otherwise the AI has nothing to do and done=0).
    let city_spots = [(2, 12), (8, 2), (13, 12)];
    let unit_spots = [(7, 7), (8, 7), (9, 7), (9, 8), (8, 8), (7, 8)];
    for (p, (cx, cy)) in city_spots.iter().enumerate() {
        let ct = dims
            .to_tile(classic_rules::math::HexCoord { q: *cx, r: *cy })
            .unwrap();
        game.debug_found_city(&rules, PlayerId(p as u8), ct);
    }
    for (i, (qx, qy)) in unit_spots.iter().enumerate() {
        let t = dims
            .to_tile(classic_rules::math::HexCoord { q: *qx, r: *qy })
            .unwrap();
        game.debug_spawn_unit(
            &rules,
            PlayerId((i / 2) as u8),
            t,
            classic_rules::ids::UnitKindId(0),
        );
    }
    GameClient::new(game, Arc::new(rules), Renderer::headless(), PlayerId(0))
}

#[test]
fn driver_rounds_advance_game() {
    let mut client = small_game();
    let t0 = client.game.view().auth.turn().round;
    let mut driver = SimulationDriver::new(DriverConfig::default(), &client.rules);
    let mut total_done = 0;
    for _ in 0..5 {
        let (done, _errs) = driver.round(&mut client);
        total_done += done;
        client.game.validate(&client.rules).unwrap();
    }
    // One driver round is one game round (P0 + 2 AI end their turns):
    // five rounds later the round counter is +5 and P0 is active again.
    assert_eq!(client.game.view().auth.turn().round, t0 + 5);
    assert_eq!(client.game.view().auth.active_player(), PlayerId(0));
    assert!(total_done > 0, "AI must act");
    // The observer projection is fresh and valid.
    assert_eq!(client.render_world.observer, 0);
}

#[test]
fn round_refreshes_projection_once_per_ai_player() {
    // Projection batching: N quiet commands -> one refresh at each presentation boundary
    // (an AI player ends its turn), so every layer revision moves at most once per AI
    // player (not once per command: hundreds of AI commands).
    let mut client = small_game();
    let before = client.render_world.layers;
    let mut driver = SimulationDriver::new(DriverConfig::default(), &client.rules);
    let (applied, _) = driver.round(&mut client);
    assert!(applied > 2, "the AI acted");
    let after = client.render_world.layers;
    let ai_players = 2;
    for layer in render::world::state::ALL_LAYERS {
        let bumps = after.get(layer) - before.get(layer);
        assert!(bumps <= ai_players + 1, "{layer:?} refreshed {bumps} times");
    }
    client.game.validate(&client.rules).unwrap();
}

/// Stage C on the client: frame slices of any size (here one step per tick) give the
/// same world as running the AI turns at once; the projection is fresh at the end.
#[test]
fn ticks_of_any_size_give_the_same_round() {
    use client::simulation_driver::TickOutcome;
    use std::time::Duration;
    let mut sliced = small_game();
    let mut whole = small_game();
    let mut a = SimulationDriver::new(DriverConfig::default(), &sliced.rules);
    let mut b = SimulationDriver::new(DriverConfig::default(), &whole.rules);
    for _ in 0..5 {
        a.end_human_turn(&mut sliced).unwrap();
        let mut ticks = 0;
        while a.tick(&mut sliced, Duration::ZERO) == TickOutcome::Busy {
            assert!(a.busy(&sliced), "busy while ticks remain");
            ticks += 1;
        }
        assert!(ticks > 10, "one step per tick");
        assert!(!a.busy(&sliced));
        b.round(&mut whole);
    }
    let hash = |c: &GameClient<classic_rules::classic::Classic>| classic_rules::replay::hash::authoritative_hash(&c.game);
    assert_eq!(hash(&sliced), hash(&whole));
    let shown = sliced.render_world.layers;
    sliced.refresh_projection();
    assert_eq!(sliced.render_world.layers, shown, "the projection was already fresh");
}

/// The AI memory travels with the save bytes: a game saved between rounds and loaded
/// in a new client continues as the live one.
#[test]
fn save_bytes_carry_the_ai_memory() {
    use classic_rules::save_codec::{from_bytes, to_bytes};
    use classic_rules::persistence::{decode::decode, encode::encode};
    let mut live = small_game();
    let mut driver = SimulationDriver::new(DriverConfig::default(), &live.rules);
    for _ in 0..4 {
        driver.round(&mut live);
    }
    let mut save = encode(&live.game, &live.rules);
    save.ai = driver.save_ai();
    let bytes = to_bytes(&save);
    let read = from_bytes(&bytes).unwrap();
    assert_eq!(read.ai, save.ai);
    assert!(!read.ai.players.is_empty(), "AI players remembered something");
    let game = decode(&read, &live.rules).unwrap();
    let mut loaded = GameClient::new(game, live.rules.clone(), Renderer::headless(), PlayerId(0));
    let mut restored = SimulationDriver::new(DriverConfig::default(), &loaded.rules);
    restored.load_ai(&read.ai, &loaded).unwrap();
    for _ in 0..6 {
        driver.round(&mut live);
        restored.round(&mut loaded);
    }
    let hash = |c: &GameClient<classic_rules::classic::Classic>| classic_rules::replay::hash::authoritative_hash(&c.game);
    assert_eq!(hash(&loaded), hash(&live));
}

#[test]
fn human_orders_flow_through_client() {
    let mut client = small_game();
    // An order straight to the human: their own unit moves, the projection updates.
    // The target is the first passable neighbour (hills+ZOC may exceed the margin).
    let units = client
        .game
        .view()
        .auth
        .indexes()
        .units_of(PlayerId(0))
        .to_vec();
    let u = units[0];
    let dims = client.game.view().auth.map().dimensions;
    let cur = client.game.view().auth.units().get(u).unwrap().core.tile;
    let (cx, cy) = (cur.0 % dims.width, cur.0 / dims.width);
    let mut ordered = false;
    for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
        let nx = cx as i32 + dx;
        let ny = cy as i32 + dy;
        if nx < 0 || ny < 0 || nx >= dims.width as i32 || ny >= dims.height as i32 {
            continue;
        }
        let dest = classic_rules::ids::TileId(ny as u32 * dims.width + nx as u32);
        if client
            .submit(Command::MoveUnit {
                unit: u,
                destination: dest,
            })
            .is_ok()
        {
            ordered = true;
            break;
        }
    }
    assert!(ordered, "at least one neighbor must accept the unit");
    assert!(
        client.render_world.layers.units > 1,
        "projection must advance on orders"
    );
    client.game.validate(&client.rules).unwrap();
}


/// Regression (R-07 AI §1: "contact → war, lost contact → peace" made
/// diplomacy flip every turn). A war without an objective or a threat ends
/// in peace; no new war follows, and borders are not opened to a player
/// never met (the old driver made peace in the first round and opened the
/// borders with it). The game's AI makes peace by deal (classic/docs/ai-leaders.md
/// "Peace by value"): a war worth nothing to either side ends in a white
/// peace at the first offer, within a proposal interval (the AI before it
/// walked out alone after `planner_strategy::MIN_WAR_ROUNDS`).
#[test]
fn a_pointless_war_ends_in_peace_without_flipping() {
    use classic_rules::ai::diplomacy::PROPOSAL_INTERVAL;
    use classic_rules::ai::planner_strategy::MIN_WAR_ROUNDS;
    let rules = compile_rules(SourceRules::default()).unwrap();
    let cfg = ScenarioConfig {
        seed: 78,
        players: 3,
        width: 40,
        height: 40,
        cities: 0,
        units: 0,
        water_ratio: 0,
    };
    let mut game = generate(&cfg, &rules);
    let dims = game.view().auth.map().dimensions;
    for (p, (q, r)) in [(2, 2), (37, 37), (2, 37)].into_iter().enumerate() {
        let tile = dims.to_tile(classic_rules::math::HexCoord { q, r }).unwrap();
        game.debug_found_city(&rules, PlayerId(p as u8), tile);
    }
    // P1 and P2 met long ago (war needs contact); now they are far apart.
    game.debug_meet(PlayerId(1), PlayerId(2));
    // P1 declares the war on its own turn, then the round comes back to P0.
    game.execute(&rules, Command::EndTurn { player: PlayerId(0) })
        .unwrap();
    let declare = Command::DeclareWar {
        player: PlayerId(1),
        enemy: PlayerId(2),
    };
    game.execute(&rules, declare).unwrap();
    game.execute(&rules, Command::EndTurn { player: PlayerId(1) })
        .unwrap();
    game.execute(&rules, Command::EndTurn { player: PlayerId(2) })
        .unwrap();
    let mut client = GameClient::new(game, Arc::new(rules), Renderer::headless(), PlayerId(0));
    let mut driver = SimulationDriver::new(DriverConfig::default(), &client.rules);
    let (mut at_war, mut entry) = (Vec::new(), Vec::new());
    for _ in 0..MIN_WAR_ROUNDS + 30 {
        driver.round(&mut client);
        let diplomacy = client.game.view().auth.diplomacy();
        at_war.push(diplomacy.at_war(PlayerId(1), PlayerId(2)));
        entry.push(diplomacy.grants_entry(PlayerId(1), PlayerId(2)) || diplomacy.grants_entry(PlayerId(2), PlayerId(1)));
    }
    let war_rounds = at_war.iter().take_while(|w| **w).count() as u32;
    assert!(war_rounds >= 1, "the war was declared");
    assert!(war_rounds <= PROPOSAL_INTERVAL + 1, "a pointless war goes on: {at_war:?}");
    assert!(at_war[war_rounds as usize..].iter().all(|w| !w), "war and peace flip: {at_war:?}");
    // Borders open later only by the friendship chain of the diplomacy
    // advisor (embassies, friendship after `FRIENDSHIP_MIN_CONTACT_ROUNDS`), never with
    // the peace.
    let peace = war_rounds as usize;
    assert!(!entry[peace] && !entry[peace + 1], "borders opened with the peace: {entry:?}");
    client.game.validate(&client.rules).unwrap();
}


/// Regression (PLAN M3: no contact memory): the driver made peace as soon
/// as the enemy left sight and declared war again on the next contact,
/// flipping every turn. Peace now needs PEACE_AFTER_UNSEEN_ROUNDS rounds
/// without sight, read from the contact memory.
#[test]
fn ai_keeps_a_war_while_the_enemy_was_seen_recently() {
    use classic_rules::ai::{diplomacy_commands, PEACE_AFTER_UNSEEN_ROUNDS};
    let rules = compile_rules(SourceRules::default()).unwrap();
    let mut game = classic_rules::GameState::new(
        &rules,
        classic_rules::GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 3,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let tile = |q, r| dims.to_tile(classic_rules::math::HexCoord { q, r }).unwrap();
    let kind = classic_rules::ids::UnitKindId(0);
    let scout = game.debug_spawn_unit(&rules, PlayerId(0), tile(5, 5), kind);
    game.debug_spawn_unit(&rules, PlayerId(1), tile(7, 5), kind);
    game.execute(&rules, Command::DeclareWar { player: PlayerId(0), enemy: PlayerId(1) })
        .unwrap();
    // The scout walks away: the enemy is out of sight from now on.
    game.execute(&rules, Command::MoveUnit { unit: scout, destination: tile(3, 5) })
        .unwrap();
    let wants_peace = |game: &classic_rules::GameState| {
        let obs = game.ai_observation(&rules, PlayerId(0));
        assert!(obs.visible_foreign_units.is_empty());
        let contact = obs.relation(PlayerId(1)).expect("met").contact;
        assert!(contact.last_seen_round.is_some());
        diplomacy_commands(&obs)
            .iter()
            .any(|c| matches!(c, Command::MakePeace { .. }))
    };
    assert!(!wants_peace(&game), "seen this round: the war goes on");
    for _ in 0..PEACE_AFTER_UNSEEN_ROUNDS {
        for player in [PlayerId(0), PlayerId(1)] {
            game.execute(&rules, Command::EndTurn { player }).unwrap();
        }
    }
    assert!(wants_peace(&game), "not seen for long: peace");
}

/// Regression (PLAN M3): GameClient always looked through player 0's eyes.
/// The host chooses the observer; the projection follows it.
#[test]
fn client_shows_the_world_through_the_chosen_observer() {
    let rules = Arc::new(compile_rules(SourceRules::default()).unwrap());
    let mut game = classic_rules::GameState::new(
        &rules,
        classic_rules::GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 5,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let tile = |q, r| dims.to_tile(classic_rules::math::HexCoord { q, r }).unwrap();
    let kind = classic_rules::ids::UnitKindId(0);
    game.debug_spawn_unit(&rules, PlayerId(0), tile(2, 2), kind);
    game.debug_spawn_unit(&rules, PlayerId(1), tile(13, 13), kind);
    let mut client = GameClient::new(game, rules, Renderer::headless(), PlayerId(1));
    let owners = |client: &GameClient<classic_rules::classic::Classic>| -> Vec<u8> {
        client.render_world.units.iter().map(|u| u.owner).collect()
    };
    assert_eq!(client.render_world.observer, 1);
    assert_eq!(owners(&client), vec![1], "P1 sees its unit, not P0's far one");
    client.set_observer(PlayerId(0));
    assert_eq!(client.render_world.observer, 0);
    assert_eq!(owners(&client), vec![0]);
}

/// A rename changes no yield, but the map label and the banner must show the new name
/// right after the command (the journal carries it into the projection).
#[test]
fn a_renamed_city_shows_its_new_name_in_the_projection_at_once() {
    let mut source = SourceRules::default();
    source.civs.push(classic_rules::rules::source::SourceCiv {
        name: "Rome".into(),
        leader: "Leader".into(),
        city_names: ["Rome", "Antium", "Capua", "Ostia"].map(String::from).to_vec(),
        ..Default::default()
    });
    let mut client = small_game_of(source);
    let mine = client.game.view().auth.cities().iter().find(|(_, c)| c.owner == PlayerId(0)).unwrap();
    let (city, tile) = (mine.0, mine.1.tile);
    let shown = |client: &GameClient<classic_rules::classic::Classic>| {
        client.render_world.cities.iter().find(|c| c.tile == tile).map(|c| c.name.clone())
    };
    let old = shown(&client).expect("the city is on the map");
    let free = kernel::world::city::free_city_names(client.game.view().auth, &client.rules, PlayerId(0));
    let (place, name) = free.first().map(|(place, name)| (*place, name.to_string())).expect("a free name");
    let revision = client.render_world.layers.cities;
    client.submit(Command::ChooseCityName { player: PlayerId(0), city, name: place }).unwrap();
    assert_ne!(old, name);
    assert_eq!(shown(&client), Some(name));
    assert!(client.render_world.layers.cities > revision, "the cities layer is redrawn");
}
