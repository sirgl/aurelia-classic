//! Player knowledge (platform/structure.md §21–22, R-07 AI stage B): Unknown /
//! Remembered / Visible tiles, resources revealed by technology, contacts,
//! and the non-interference contract — what a player cannot see cannot
//! change what it observes or what its AI does.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ai::{diplomacy_commands, war_declarations, GreedyPlanner, Planner};
use classic_rules::ids::{PlayerId, ResourceId, TerrainId, TileId, UnitKindId};
use classic_rules::knowledge::observation::AIObservation;
use classic_rules::knowledge::KnownTile;
use classic_rules::math::HexCoord;
use classic_rules::rules::SourceRules;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

fn game(rules: &CompiledRuleset, size: u32) -> GameState {
    GameState::new(
        rules,
        GameConfig {
            players: 2,
            width: size,
            height: size,
            rng_seed: 1,
        },
    )
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view()
        .auth
        .map()
        .dimensions
        .to_tile(HexCoord { q, r })
        .unwrap()
}

#[test]
fn tiles_are_unknown_remembered_or_visible() {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let mut game = game(&rules, 24);
    let scout = game.debug_spawn_unit(&rules, PlayerId(0), tile(&game, 5, 5), UnitKindId(0));
    game.debug_spawn_unit(&rules, PlayerId(1), tile(&game, 20, 20), UnitKindId(0));
    let (near, far) = (tile(&game, 7, 5), tile(&game, 15, 15));
    let view = game.player_view(&rules, PlayerId(0));
    assert!(matches!(view.tile(far), KnownTile::Unknown));
    assert!(matches!(view.tile(near), KnownTile::Visible(_)));
    // Two rounds later the scout has left: the tile is remembered as of the
    // round the scout walked away.
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(1),
        },
    )
    .unwrap();
    let left_in = game.view().auth.turn().round;
    let back = tile(&game, 3, 5);
    game.execute(
        &rules,
        Command::MoveUnit {
            unit: scout,
            destination: back,
        },
    )
    .unwrap();
    let view = game.player_view(&rules, PlayerId(0));
    match view.tile(near) {
        KnownTile::Remembered {
            last_seen_round, ..
        } => assert_eq!(last_seen_round, left_in),
        other => panic!("expected Remembered, got {other:?}"),
    }
}

#[test]
fn resources_show_only_with_their_technology() {
    let rules = compile_rules(core_rules()).unwrap();
    let iron = rules
        .resources
        .iter()
        .position(|r| r.key == "core:resource/iron")
        .unwrap();
    let bronze = rules.resources[iron]
        .required_tech
        .expect("iron needs bronze working");
    let mut game = game(&rules, 16);
    let spot = tile(&game, 6, 5);
    game.debug_set_terrain_batch(&rules, &[(spot, TerrainId(2))]);
    game.debug_set_resource_batch(&rules, &[(spot, ResourceId(iron as u16))]);
    game.debug_spawn_unit(&rules, PlayerId(0), tile(&game, 5, 5), UnitKindId(0));
    let resource = |game: &GameState| {
        let view = game.player_view(&rules, PlayerId(0));
        view.tile(spot).snapshot().unwrap().resource
    };
    assert_eq!(
        resource(&game),
        None,
        "iron is hidden before bronze working"
    );
    game.debug_learn(&rules, PlayerId(0), bronze);
    assert_eq!(resource(&game), Some(ResourceId(iron as u16)));
}

#[test]
fn contacts_remember_meeting_and_last_sight() {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let mut game = game(&rules, 24);
    game.debug_spawn_unit(&rules, PlayerId(0), tile(&game, 5, 5), UnitKindId(0));
    game.debug_spawn_unit(&rules, PlayerId(1), tile(&game, 20, 20), UnitKindId(0));
    let contact = |game: &GameState, viewer, other| game.player_view(&rules, viewer).contact(other);
    assert_eq!(contact(&game, PlayerId(0), PlayerId(1)).met_round, None);
    // A P1 unit appears in P0's sight: both have met, P0 saw P1, P1 did not
    // see P0 (sight is not mutual).
    game.debug_spawn_unit(&rules, PlayerId(1), tile(&game, 7, 5), UnitKindId(0));
    let seen = contact(&game, PlayerId(0), PlayerId(1));
    assert_eq!((seen.met_round, seen.last_seen_round), (Some(0), Some(0)));
    let other_side = contact(&game, PlayerId(1), PlayerId(0));
    assert_eq!(other_side.met_round, Some(0), "meeting is mutual");
    // Rounds pass with both standing still: the end of P0's turn refreshes
    // the sight it keeps.
    for _ in 0..3 {
        game.execute(
            &rules,
            Command::EndTurn {
                player: PlayerId(0),
            },
        )
        .unwrap();
        game.execute(
            &rules,
            Command::EndTurn {
                player: PlayerId(1),
            },
        )
        .unwrap();
    }
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    let round = game.view().auth.turn().round;
    assert_eq!(
        contact(&game, PlayerId(0), PlayerId(1)).last_seen_round,
        Some(round)
    );
}

/// Everything an AI observation holds, in a comparable form.
fn digest(obs: &AIObservation<'_>) -> String {
    let tiles: Vec<KnownTile> = (0..obs.view.tile_count() as u32)
        .map(|t| obs.view.tile(TileId(t)))
        .collect();
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        obs.round,
        obs.own_units,
        obs.own_cities,
        obs.economy,
        obs.visible_foreign_units,
        obs.visible_foreign_cities,
        obs.relations,
        tiles
    )
}

/// Two worlds that differ only where player 0 cannot see: where a foreign
/// unit and a foreign city stand, the terrain of a far corner, and the
/// deposit on one hex (R352–R354: a hidden city and a hidden deposit). The
/// third value holds the hexes of those differences: a world stops being a
/// twin the moment player 0 has explored one of them (see `Twins::seen`).
fn twin_worlds(rules: &CompiledRuleset) -> (GameState, GameState, Vec<TileId>) {
    let mut hidden_tiles = Vec::new();
    let mut make = |hidden_q: i32, hidden_terrain: u16, deposit: u16| {
        let mut world = game(rules, 32);
        let settler = rules.units.iter().position(|u| u.founder).unwrap() as u16;
        let warrior = rules
            .units
            .iter()
            .position(|u| u.key == "core:unit/warrior")
            .unwrap() as u16;
        world.debug_spawn_unit(rules, PlayerId(0), tile(&world, 4, 4), UnitKindId(settler));
        world.debug_spawn_unit(rules, PlayerId(0), tile(&world, 5, 4), UnitKindId(warrior));
        let hidden = tile(&world, hidden_q, 28);
        world.debug_spawn_unit(rules, PlayerId(1), hidden, UnitKindId(warrior));
        let far_corner = tile(&world, 30, 30);
        world.debug_set_terrain_batch(rules, &[(far_corner, TerrainId(hidden_terrain))]);
        let city = tile(&world, hidden_q - 6, 30);
        world.debug_found_city(rules, PlayerId(1), city);
        let field = tile(&world, 26, 20);
        world.debug_set_resource_batch(rules, &[(field, ResourceId(deposit))]);
        hidden_tiles.extend([hidden, far_corner, city, field]);
        world
    };
    let (a, b) = (make(26, 0, 0), make(29, 3, 1));
    (a, b, hidden_tiles)
}

/// Has player 0 explored one of the hexes where the twins differ? From then
/// on a difference in what its AI does is no leak: it has looked.
fn seen(world: &GameState, hidden_tiles: &[TileId]) -> bool {
    let knowledge = &world.view().auth.knowledge()[0];
    hidden_tiles.iter().any(|t| knowledge.is_explored(*t))
}

#[test]
fn hidden_differences_do_not_reach_the_observation_or_the_ai() {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut a, mut b, hidden_tiles) = twin_worlds(&rules);
    for round in 0..12 {
        let obs_a = a.ai_observation(&rules, PlayerId(0));
        let obs_b = b.ai_observation(&rules, PlayerId(0));
        assert_eq!(
            digest(&obs_a),
            digest(&obs_b),
            "observation differs in round {round}"
        );
        assert!(!seen(&a, &hidden_tiles) && !seen(&b, &hidden_tiles), "round {round}: player 0 explored a hidden hex");
        let mut commands = war_declarations(&obs_a);
        commands.extend(GreedyPlanner.plan(obs_a.clone(), &rules));
        commands.extend(diplomacy_commands(&obs_a));
        let mut commands_b = war_declarations(&obs_b);
        commands_b.extend(GreedyPlanner.plan(obs_b.clone(), &rules));
        commands_b.extend(diplomacy_commands(&obs_b));
        assert_eq!(
            format!("{commands:?}"),
            format!("{commands_b:?}"),
            "commands differ in round {round}"
        );
        for command in commands {
            let ok_a = a.execute(&rules, command.clone()).is_ok();
            let ok_b = b.execute(&rules, command).is_ok();
            assert_eq!(ok_a, ok_b);
        }
        for world in [&mut a, &mut b] {
            world
                .execute(
                    &rules,
                    Command::EndTurn {
                        player: PlayerId(0),
                    },
                )
                .unwrap();
            world
                .execute(
                    &rules,
                    Command::EndTurn {
                        player: PlayerId(1),
                    },
                )
                .unwrap();
        }
    }
    // The worlds really differ, and player 0 really played.
    use classic_rules::replay::authoritative_hash;
    assert_ne!(authoritative_hash(&a), authoritative_hash(&b));
    assert_eq!(a.view().auth.indexes().cities_of(PlayerId(0)).len(), 1, "player 0 founded its city");
}

/// The M6 AI (driver, memory, planner) under the same contract: player 0's
/// AI plays 30 rounds in both worlds (player 1 only ends its turns) and
/// sees, decides, remembers and traces exactly the same.
#[test]
fn hidden_differences_do_not_reach_the_m6_ai() {
    use classic_rules::ai::{AiDriver, AiSettings};
    let rules = compile_rules(core_rules()).unwrap();
    let (mut a, mut b, hidden_tiles) = twin_worlds(&rules);
    let new_driver = || {
        let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), vec![PlayerId(1)]);
        driver.record_commands();
        driver
    };
    let (mut ai_a, mut ai_b) = (new_driver(), new_driver());
    let trace = |driver: &AiDriver| -> Vec<String> { driver.trace().entries().map(|e| e.to_string()).collect() };
    let mut compared = 0;
    for round in 0..30 {
        assert_eq!(
            digest(&a.ai_observation(&rules, PlayerId(0))),
            digest(&b.ai_observation(&rules, PlayerId(0))),
            "observation differs in round {round}"
        );
        ai_a.run(&mut a, &rules, u32::MAX);
        ai_b.run(&mut b, &rules, u32::MAX);
        // A scout that walks up to a hidden hex has looked: what it does from
        // there on may differ. The twins are compared until that round.
        if seen(&a, &hidden_tiles) || seen(&b, &hidden_tiles) {
            break;
        }
        compared += 1;
        assert_eq!(format!("{:?}", ai_a.commands()), format!("{:?}", ai_b.commands()), "commands differ in round {round}");
        assert_eq!(ai_a.states(), ai_b.states(), "AI memory differs in round {round}");
        assert_eq!(trace(&ai_a), trace(&ai_b), "trace differs in round {round}");
        assert_eq!(ai_a.counters(), ai_b.counters(), "even the work done is the same (round {round})");
        for world in [&mut a, &mut b] {
            world.execute(&rules, Command::EndTurn { player: PlayerId(1) }).unwrap();
        }
    }
    use classic_rules::replay::authoritative_hash;
    assert_ne!(authoritative_hash(&a), authoritative_hash(&b));
    // The comparison is not empty: the AI explored, founded and moved for
    // many rounds before it came near a hidden hex.
    assert!(compared >= 15, "only {compared} rounds were compared before a hidden hex was explored");
    let commands = format!("{:?}", ai_a.commands());
    assert!(commands.contains("FoundCity") && commands.contains("MoveUnit"), "the AI played: {commands}");
}

/// A scout of player 0 at (5,5) sees the hex (7,5); it can walk to (3,5) and
/// back, and the hex is out of its sight from there.
struct Watch {
    rules: CompiledRuleset,
    game: GameState,
    scout: classic_rules::ids::UnitHandle,
    spot: TileId,
}

impl Watch {
    fn new() -> Self {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = game(&rules, 24);
        game.debug_spawn_unit(&rules, PlayerId(1), tile(&game, 20, 20), UnitKindId(0));
        let scout = game.debug_spawn_unit(&rules, PlayerId(0), tile(&game, 5, 5), UnitKindId(0));
        let spot = tile(&game, 7, 5);
        game.debug_set_terrain_batch(&rules, &[(spot, TerrainId(2))]);
        Self { rules, game, scout, spot }
    }

    fn resource_named(&self, key: &str) -> (ResourceId, Option<classic_rules::ids::TechId>) {
        let at = self.rules.resources.iter().position(|r| r.key == key).unwrap();
        (ResourceId(at as u16), self.rules.resources[at].required_tech)
    }

    fn go_to(&mut self, q: i32, r: i32) {
        // A new turn first: the scout has its moves again.
        for player in [PlayerId(0), PlayerId(1)] {
            if self.game.view().auth.active_player() == player {
                self.game.execute(&self.rules, Command::EndTurn { player }).unwrap();
            }
        }
        let destination = tile(&self.game, q, r);
        self.game.execute(&self.rules, Command::MoveUnit { unit: self.scout, destination }).unwrap();
    }

    fn view(&self) -> classic_rules::knowledge::PlayerView<'_> {
        self.game.player_view(&self.rules, PlayerId(0))
    }

    fn resource(&self) -> Option<ResourceId> {
        self.view().tile(self.spot).snapshot().unwrap().resource
    }
}

/// R355: the deposit of an explored hex is read from the map, not from the
/// memory. A deposit that appears while player 0 is not looking shows on the
/// remembered hex at once; the other layers stay the last observation.
#[test]
fn a_resource_that_appears_out_of_sight_shows_on_the_remembered_hex() {
    let mut watch = Watch::new();
    let (wheat, tech) = watch.resource_named("core:resource/wheat");
    assert_eq!(tech, None, "wheat shows without technology");
    assert!(watch.view().is_visible(watch.spot));
    watch.go_to(3, 5);
    assert!(!watch.view().is_visible(watch.spot));
    watch.game.debug_set_resource_batch(&watch.rules, &[(watch.spot, wheat)]);
    assert_eq!(watch.resource(), Some(wheat), "the explored hex shows the live deposit");
    watch.go_to(5, 5);
    assert_eq!(watch.resource(), Some(wheat), "in sight");
    watch.go_to(3, 5);
    assert_eq!(watch.resource(), Some(wheat), "and from memory");
}

/// A hex nobody explored shows nothing, deposit or not.
#[test]
fn an_unexplored_hex_shows_no_deposit() {
    let mut watch = Watch::new();
    let (wheat, _) = watch.resource_named("core:resource/wheat");
    let far = tile(&watch.game, 20, 5);
    watch.game.debug_set_resource_batch(&watch.rules, &[(far, wheat)]);
    assert!(matches!(watch.view().tile(far), KnownTile::Unknown));
}

/// The strategic-resource rule: a deposit shows as soon as its technology is
/// known, in sight or on an explored hex out of sight, whether or not the player ever saw
/// the deposit (R355).
#[test]
fn a_remembered_strategic_resource_shows_with_its_technology() {
    let mut watch = Watch::new();
    let (iron, tech) = watch.resource_named("core:resource/iron");
    let bronze = tech.expect("iron needs a technology");
    watch.game.debug_set_resource_batch(&watch.rules, &[(watch.spot, iron)]);
    assert_eq!(watch.resource(), None, "hidden before bronze working");
    watch.go_to(3, 5);
    assert_eq!(watch.resource(), None);
    watch.game.debug_learn(&watch.rules, PlayerId(0), bronze);
    assert_eq!(watch.resource(), Some(iron), "the remembered deposit shows with the technology");
    // A second deposit appearing out of sight, with the technology known.
    let ore = watch.rules.resources.iter().position(|r| r.required_tech.is_some() && r.key != "core:resource/iron").unwrap();
    let ore = ResourceId(ore as u16);
    let other = tile(&watch.game, 7, 6);
    watch.game.debug_set_resource_batch(&watch.rules, &[(other, ore)]);
    let seen = |watch: &Watch| watch.view().tile(other).snapshot().and_then(|s| s.resource);
    assert_eq!(seen(&watch), None, "its own technology is not known yet");
    let ore_tech = watch.rules.resources[ore.0 as usize].required_tech.unwrap();
    watch.game.debug_learn(&watch.rules, PlayerId(0), ore_tech);
    assert_eq!(seen(&watch), Some(ore), "an explored hex shows a deposit nobody saw, with the technology known");
}

/// Memory is saved and loaded: the pictures read back the same, and the
/// save writes back to the same bytes.
#[test]
fn the_memory_survives_save_and_load() {
    use classic_rules::persistence::{decode::decode, encode::encode};
    let mut watch = Watch::new();
    let (wheat, _) = watch.resource_named("core:resource/wheat");
    let city = watch.game.debug_found_city(&watch.rules, PlayerId(1), tile(&watch.game, 7, 4));
    watch.game.debug_grow_city(&watch.rules, city, 2);
    watch.game.debug_set_resource_batch(&watch.rules, &[(watch.spot, wheat)]);
    watch.go_to(3, 5);
    watch.game.debug_grow_city(&watch.rules, city, 3);
    let picture = |game: &GameState| {
        let view = game.player_view(&watch.rules, PlayerId(0));
        let tiles: Vec<_> = (0..view.tile_count() as u32).map(|t| format!("{:?}", view.tile(TileId(t)))).collect();
        let cities: Vec<_> = view.remembered_cities().map(|(t, s)| (t, s.clone())).collect();
        (tiles, cities)
    };
    let (_, cities) = picture(&watch.game);
    assert_eq!(cities.len(), 1);
    assert!(cities[0].1.seen.is_some());
    let bytes = classic_rules::save_codec::to_bytes(&encode(&watch.game, &watch.rules));
    let loaded = decode(&classic_rules::save_codec::from_bytes(&bytes).unwrap(), &watch.rules).unwrap();
    assert_eq!(picture(&loaded), picture(&watch.game));
    assert_eq!(classic_rules::save_codec::to_bytes(&encode(&loaded, &watch.rules)), bytes);
    assert_eq!(classic_rules::replay::authoritative_hash(&loaded), classic_rules::replay::authoritative_hash(&watch.game));
}

/// A save from before sightings (no deposits, no city numbers in memory)
/// loads with the city known by name and owner, its numbers unknown (never
/// zero), and the deposits of the map.
#[test]
fn a_save_from_before_sightings_reads_the_numbers_as_unknown() {
    use classic_rules::persistence::{decode::decode, encode::encode};
    let mut watch = Watch::new();
    let (wheat, _) = watch.resource_named("core:resource/wheat");
    watch.game.debug_set_resource_batch(&watch.rules, &[(watch.spot, wheat)]);
    let city = watch.game.debug_found_city(&watch.rules, PlayerId(1), tile(&watch.game, 7, 4));
    let name = watch.game.view().auth.cities().get(city).unwrap().name.clone();
    watch.go_to(3, 5);
    let mut save = encode(&watch.game, &watch.rules);
    save.world.knowledge_resource.clear();
    save.world.knowledge_cities.clear();
    let loaded = decode(&save, &watch.rules).unwrap();
    let view = loaded.player_view(&watch.rules, PlayerId(0));
    let cities: Vec<_> = view.remembered_cities().map(|(_, s)| s.clone()).collect();
    assert_eq!(cities.len(), 1);
    assert_eq!((cities[0].owner, cities[0].name.as_str(), cities[0].seen.clone()), (PlayerId(1), name.as_str(), None));
    assert_eq!(view.tile(watch.spot).snapshot().unwrap().resource, Some(wheat), "the map's deposit");
}

/// A save whose memory contradicts itself is an error, never a silent zero or a ghost.
#[test]
fn a_save_with_contradicting_memory_is_refused() {
    use classic_rules::persistence::{decode::decode, encode::encode};
    let mut watch = Watch::new();
    watch.game.debug_found_city(&watch.rules, PlayerId(1), tile(&watch.game, 7, 4));
    watch.go_to(3, 5);
    let good = encode(&watch.game, &watch.rules);
    assert!(decode(&good, &watch.rules).is_ok());
    let refused = |damage: &dyn Fn(&mut classic_rules::persistence::schema::SavedWorldV1)| {
        let mut save = good.clone();
        damage(&mut save.world);
        decode(&save, &watch.rules).is_err()
    };
    assert!(refused(&|w| w.knowledge_cities[0].clear()), "a remembered city with no sighting");
    assert!(refused(&|w| w.knowledge_cities.push(Vec::new())), "a sighting list too many");
    assert!(refused(&|w| w.knowledge_resource.push(Vec::new())), "a deposit list too many");
    assert!(refused(&|w| w.knowledge_cities[0][0].3 = Some((1, 0, vec![2, 1]))), "buildings out of order");
    assert!(refused(&|w| w.knowledge_cities[0][0].1 = 9), "an owner outside the match");
}
