//! How `standard-64df245.aurelia` was made (the sixteenth promotion, 23 Nov 2026): a scratch test in a
//! checkout of classic 64df245 (platform 15d5b3d), main before `standard` became `net2`. 4 nations of the
//! old `standard` (`nets-x-site`) played 100 rounds; the world and the AI memory (section version 6, mind
//! memory 1293/1153/1149/1213 bytes, without `net2`'s section) were saved. World hash 8a1c84b60a294860.
//! Never overwrite the file: it stands for saves the players made with that build.
use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::replay::authoritative_hash;
use classic_rules::save_codec::to_bytes;
use classic_rules::persistence::encode::encode;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::compile_rules;
use kernel::ai::driver::AiDriver;

#[test]
fn write_main_save() {
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig { seed: 0xA1_5EED, players: 4, width: 40, height: 32, cities: 0, units: 0, water_ratio: 15 };
    let mut game = generate_new_game(&rules, &config);
    let mut driver = AiDriver::with_mind(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new(), ClassicMind::new(Variant::Standard));
    let mut n = 0;
    while n < 100 { if driver.run(&mut game, &rules, 100).round_done { n += 1; } }
    let mut save = encode(&game, &rules);
    save.ai = driver.save();
    let mem: Vec<usize> = save.ai.players.iter().map(|p| p.mind_memory.len()).collect();
    let bytes = to_bytes(&save);
    std::fs::write("standard-64df245.aurelia", &bytes).unwrap();
    println!("SAVED {} bytes, hash {:016x}, ai version {}, mind memory {mem:?}", bytes.len(), authoritative_hash(&game), save.ai.version);
}
