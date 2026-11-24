//! A saved game of the shipped AI (`standard`) plays on as if it had never been saved: the AI's
//! memory lives in the save, never in a thread or a static (the game is loaded and played on in
//! a new thread here). And a save of the previous build keeps loading and playing.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::persistence::decode::{decode_with, RulesMatch};
use classic_rules::persistence::encode::encode;
use classic_rules::replay::authoritative_hash;
use classic_rules::save_codec::{from_bytes, to_bytes};
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;

type Driver = AiDriver<classic_rules::classic::Classic, ClassicMind>;

fn standard(rules: &CompiledRuleset) -> Driver {
    AiDriver::with_mind(AiSettings::for_rules(rules, rules.default_difficulty()), Vec::new(), ClassicMind::new(Variant::Standard))
}

/// Play `rounds` rounds; the hash after each.
fn play(game: &mut GameState, rules: &CompiledRuleset, driver: &mut Driver, rounds: u32) -> Vec<u64> {
    let mut hashes = Vec::new();
    while hashes.len() < rounds as usize {
        if driver.run(game, rules, 100).round_done {
            hashes.push(authoritative_hash(game));
        }
    }
    hashes
}

/// The bytes of the game and the AI's memory, as the game saves them.
fn save(game: &GameState, rules: &CompiledRuleset, driver: &Driver) -> Vec<u8> {
    let mut save = encode(game, rules);
    save.ai = driver.save();
    to_bytes(&save)
}

/// The game and a `standard` AI with the memory of `bytes`, as the player's load does.
fn load(bytes: &[u8], rules: &CompiledRuleset) -> (GameState, Driver) {
    let save = from_bytes(bytes).expect("the bytes are a save");
    let (game, _) = decode_with(&save, rules, RulesMatch::SameRules).expect("the save decodes");
    let mut driver = standard(rules);
    driver.load(&save.ai, game.view().auth.map().tile_count(), rules).expect("the AI memory loads");
    (game, driver)
}

/// Save after 30 rounds, load in a new thread, play 30 more: every round's hash is the one of
/// the game that was never saved.
#[test]
fn a_saved_standard_game_loaded_in_a_new_thread_plays_on_as_the_unbroken_one() {
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig { seed: 0x5A7E_10AD, players: 4, width: 40, height: 32, cities: 0, units: 0, water_ratio: 15 };
    let mut game = generate_new_game(&rules, &config);
    let mut driver = standard(&rules);
    play(&mut game, &rules, &mut driver, 30);
    let bytes = save(&game, &rules, &driver);
    let unbroken = play(&mut game, &rules, &mut driver, 30);

    let loaded = std::thread::spawn(move || {
        let rules = compile_rules(core_rules()).unwrap();
        let (mut game, mut driver) = load(&bytes, &rules);
        play(&mut game, &rules, &mut driver, 30)
    })
    .join()
    .expect("the loaded game plays");
    assert_eq!(loaded, unbroken, "the loaded game diverged");
}

/// A save of the build before the fifteenth promotion (classic 266bd4d, `standard` of 4 nations
/// after 100 rounds; `fixtures/standard-266bd4d.recipe.rs`): it loads with its AI memory, the
/// world hashes as saved, and the new `standard` plays it on.
#[test]
fn a_save_of_the_previous_build_loads_and_plays_on() {
    const SAVE: &[u8] = include_bytes!("fixtures/standard-266bd4d.aurelia");
    const WORLD: u64 = 0x04ae_6400_8dae_4998;
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, mut driver) = load(SAVE, &rules);
    assert_eq!(authoritative_hash(&game), WORLD);
    play(&mut game, &rules, &mut driver, 20);
    game.validate(&rules).unwrap();
}

/// A save of main before the sixteenth promotion (classic 64df245, the old `standard` of 4 nations after 100
/// rounds; `fixtures/standard-64df245.recipe.rs`): its AI memory has no `net2` section; it loads, the world
/// hashes as saved, and the new `standard` (`net2`, with an empty memory of its own) plays it on.
#[test]
fn a_save_of_main_before_net2_loads_and_plays_on() {
    const SAVE: &[u8] = include_bytes!("fixtures/standard-64df245.aurelia");
    const WORLD: u64 = 0x8a1c_84b6_0a29_4860;
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, mut driver) = load(SAVE, &rules);
    assert_eq!(authoritative_hash(&game), WORLD);
    let hashes = play(&mut game, &rules, &mut driver, 40);
    assert!(hashes.windows(2).any(|w| w[0] != w[1]), "the game moves on");
    game.validate(&rules).unwrap();
}

/// A save of main before the seventeenth promotion (classic edfa519, the old `standard` of 4 nations after 100
/// rounds; `fixtures/standard-edfa519.recipe.rs`): its AI memory has no `science-flow` section; it loads, the world
/// hashes as saved, and the new `standard` (`early-growth`, with an empty memory of its own) plays it on.
#[test]
fn a_save_of_main_before_early_growth_loads_and_plays_on() {
    const SAVE: &[u8] = include_bytes!("fixtures/standard-edfa519.aurelia");
    const WORLD: u64 = 0x6a5c_0f0a_4fa3_318f;
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, mut driver) = load(SAVE, &rules);
    assert_eq!(authoritative_hash(&game), WORLD);
    let hashes = play(&mut game, &rules, &mut driver, 40);
    assert!(hashes.windows(2).any(|w| w[0] != w[1]), "the game moves on");
    game.validate(&rules).unwrap();
}
