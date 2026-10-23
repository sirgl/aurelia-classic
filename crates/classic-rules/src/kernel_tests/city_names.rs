use crate::world::city::next_city_name;
use crate::{
    compile_rules,
    ids::{PlayerId, TileId},
    rules::{
        source::{SourceCiv, StableContentKey},
        CompiledRuleset, SourceRules,
    },
    Command, GameState,
};

fn source(names: &[&str]) -> SourceRules {
    let mut source = crate::fixtures::parse_ron_text(
        r#"(
        units: [(id:"test:unit/settler", movement:2, strength:1, founder:true, cost:40)],
    )"#,
    );
    source.civs.push(SourceCiv {
        id: Some(StableContentKey("test:nation/rome".into())),
        name: "Rome".into(),
        leader: "Leader".into(),
        city_names: names.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    });
    source
}
fn game(rules: &CompiledRuleset) -> GameState {
    GameState::new(rules, crate::world::state::GameConfig {
        players: 2, width: 24, height: 16, rng_seed: 7,
    })
}

#[test]
fn names_are_global_across_players_and_survive_capture_and_custom_names() {
    let rules = compile_rules(source(&["Rome", "Antium"])).unwrap();
    let mut game = game(&rules);
    let first = game.debug_found_city(&rules, PlayerId(0), TileId(0));
    let second = game.debug_found_city(&rules, PlayerId(1), TileId(20));
    assert_eq!(game.view().auth.cities().get(first).unwrap().name, "Rome");
    assert_eq!(
        game.view().auth.cities().get(second).unwrap().name,
        "Antium"
    );
    // A captured city keeps its name and original owner; current owner is irrelevant to naming.
    let mut save = crate::persistence::encode::encode(&game, &rules);
    save.world.cities[0].owner = 1;
    for (owner, city) in save.world.owner.iter_mut().zip(&save.world.tile_city) {
        if *city == Some(0) { *owner = Some(1); }
    }
    let captured = crate::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(
        next_city_name(captured.view().auth, &rules, PlayerId(0)),
        "Rome 2"
    );
    let third = game.debug_found_city(&rules, PlayerId(0), TileId(200));
    game.debug_set_city_name(third, "rOmE 2".into());
    assert_eq!(
        next_city_name(game.view().auth, &rules, PlayerId(0)),
        "Antium 2"
    );
    game.debug_set_city_name(first, "Custom Capital".into());
    assert_eq!(
        next_city_name(game.view().auth, &rules, PlayerId(0)),
        "Rome"
    );
}

#[test]
fn fallback_is_readable_and_loaded_game_chooses_same_next_name() {
    let rules = compile_rules(source(&[])).unwrap();
    let mut game = game(&rules);
    game.debug_found_city(&rules, PlayerId(0), TileId(0));
    assert_eq!(
        next_city_name(game.view().auth, &rules, PlayerId(1)),
        "Rome Settlement 2"
    );
    let loaded = crate::persistence::decode::decode(
        &crate::persistence::encode::encode(&game, &rules),
        &rules,
    )
    .unwrap();
    assert_eq!(
        next_city_name(loaded.view().auth, &rules, PlayerId(1)),
        "Rome Settlement 2"
    );
}

#[test]
fn actual_found_city_and_harness_share_names_and_replay_is_deterministic() {
    let rules = compile_rules(source(&["Rome", "Antium"])).unwrap();
    let mut actual = game(&rules);
    let tile = (0..24 * 16)
        .map(TileId)
        .find(|&t| {
            crate::world::territory::city_site_check(actual.view().auth, PlayerId(0), t).is_ok()
        })
        .unwrap();
    let settler = actual.debug_spawn_unit(&rules, PlayerId(0), tile, crate::ids::UnitKindId(0));
    let initial = crate::persistence::encode::encode(&actual, &rules);
    let mut replay = crate::persistence::decode::decode(&initial, &rules).unwrap();
    let mut harness = game(&rules);
    let expected = harness.debug_found_city(&rules, PlayerId(0), tile);
    actual
        .execute(&rules, Command::FoundCity { settler })
        .unwrap();
    replay
        .execute(&rules, Command::FoundCity { settler })
        .unwrap();
    assert_eq!(
        actual.view().auth.cities().iter().next().unwrap().1.name,
        harness.view().auth.cities().get(expected).unwrap().name
    );
    assert_eq!(
        crate::replay::authoritative_hash(&actual),
        crate::replay::authoritative_hash(&replay)
    );
    let before = crate::replay::authoritative_hash(&actual);
    assert!(actual
        .execute(&rules, Command::FoundCity { settler })
        .is_err());
    assert_eq!(crate::replay::authoritative_hash(&actual), before);
}

#[test]
fn pools_are_optional_validated_and_part_of_current_fingerprint() {
    let empty = compile_rules(source(&[])).unwrap();
    let named = compile_rules(source(&["Rome"])).unwrap();
    assert_ne!(empty.hash(), named.hash());
    assert_eq!(
        empty.hash_before_city_names(),
        named.hash_before_city_names()
    );
    for names in [&[""][..], &[" Rome"][..], &["Rome", "rome"][..]] {
        assert!(compile_rules(source(names)).is_err());
    }
}

#[test]
fn padded_mod_display_name_cannot_create_duplicate_fallback_names() {
    let mut source = source(&[]);
    source.civs[0].name = " Rome ".into();
    let rules = compile_rules(source).unwrap();
    let mut game = game(&rules);
    let first = game.debug_found_city(&rules, PlayerId(0), TileId(0));
    assert_eq!(game.view().auth.cities().get(first).unwrap().name, "Rome Settlement");
    game.debug_set_city_name(first, "  rOME Settlement  ".into());
    let second = game.debug_found_city(&rules, PlayerId(1), TileId(20));
    assert_eq!(game.view().auth.cities().get(second).unwrap().name, "Rome Settlement 2");
    assert_eq!(next_city_name(game.view().auth, &rules, PlayerId(0)), "Rome Settlement 3");
}

fn rename(game: &mut GameState, rules: &CompiledRuleset, player: u8, city: crate::ids::CityHandle, name: u16) -> Result<(), crate::command::CommandError> {
    game.execute(rules, Command::ChooseCityName { player: PlayerId(player), city, name }).map(|_| ())
}

fn name_of(game: &GameState, city: crate::ids::CityHandle) -> String {
    game.view().auth.cities().get(city).unwrap().name.clone()
}

#[test]
fn a_player_renames_a_city_to_a_free_name_of_its_list() {
    use crate::command::{CommandError, Refusal};
    let rules = compile_rules(source(&["Rome", "Antium", "Capua", "Ostia"])).unwrap();
    let mut game = game(&rules);
    let mine = game.debug_found_city(&rules, PlayerId(0), TileId(0));
    let theirs = game.debug_found_city(&rules, PlayerId(1), TileId(200));
    assert_eq!((name_of(&game, mine), name_of(&game, theirs)), ("Rome".into(), "Antium".into()));
    // The names nobody holds are the choices, in list order.
    let free = crate::world::city::free_city_names(game.view().auth, &rules, PlayerId(0));
    assert_eq!(free, [(2, "Capua"), (3, "Ostia")]);
    rename(&mut game, &rules, 0, mine, 3).unwrap();
    assert_eq!(name_of(&game, mine), "Ostia");
    // The name given up is free again; the founding takes the first free one.
    let third = game.debug_found_city(&rules, PlayerId(0), TileId(100));
    assert_eq!(name_of(&game, third), "Rome");
    // A held name, a place beyond the list, and a city of another player are refused.
    let before = crate::replay::authoritative_hash(&game);
    assert_eq!(rename(&mut game, &rules, 0, mine, 1), Err(CommandError::Refused(Refusal::NameTaken)));
    assert_eq!(rename(&mut game, &rules, 0, mine, 9), Err(CommandError::Refused(Refusal::NameTaken)));
    assert!(rename(&mut game, &rules, 0, theirs, 2).is_err());
    assert_eq!(crate::replay::authoritative_hash(&game), before, "a refusal changes nothing");
    game.validate(&rules).unwrap();
}

#[test]
fn a_rename_replays_saves_and_the_command_is_encoded_in_the_journal() {
    use kernel::persistence::codec::Reader;
    use kernel::replay::codec::CommandField;
    let rules = compile_rules(source(&["Rome", "Antium", "Capua"])).unwrap();
    let mut live = game(&rules);
    let city = live.debug_found_city(&rules, PlayerId(0), TileId(0));
    let initial = crate::persistence::encode::encode(&live, &rules);
    let command = Command::ChooseCityName { player: PlayerId(0), city, name: 2 };
    live.execute(&rules, command.clone()).unwrap();
    let mut fork = crate::persistence::decode::decode(&initial, &rules).unwrap();
    fork.execute(&rules, command.clone()).unwrap();
    assert_eq!(name_of(&fork, city), "Capua");
    assert_eq!(crate::replay::authoritative_hash(&live), crate::replay::authoritative_hash(&fork));
    let loaded = crate::persistence::decode::decode(&crate::persistence::encode::encode(&live, &rules), &rules).unwrap();
    assert_eq!(name_of(&loaded, city), "Capua");
    let mut bytes = Vec::new();
    command.put(live.view().auth, &mut bytes);
    let again = Command::take(&mut Reader::new(&bytes, 0), live.view().auth).unwrap();
    let mut written = Vec::new();
    again.put(live.view().auth, &mut written);
    assert_eq!(written, bytes);
    assert!(matches!(again, Command::ChooseCityName { name: 2, .. }));
}
