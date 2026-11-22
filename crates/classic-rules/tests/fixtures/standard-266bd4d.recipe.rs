//! How `standard-266bd4d.aurelia` was made (review Y7 of the fifteenth promotion, 22 Nov 2026): a
//! scratch test in a checkout of classic 266bd4d (platform 7e23342), the build before `standard`
//! became `nets-x-site`. 4 nations of the old `standard` played 100 rounds; the world and the AI
//! memory (section version 6) were saved. World hash 04ae64008dae4998. Never overwrite the file:
//! it stands for saves the players made with that build.
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
fn write_old_save() {
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig { seed: 0xA1_5EED, players: 4, width: 40, height: 32, cities: 0, units: 0, water_ratio: 15 };
    let mut game = generate_new_game(&rules, &config);
    let mut driver = AiDriver::with_mind(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new(), ClassicMind::new(Variant::Standard));
    let mut n = 0;
    while n < 100 { if driver.run(&mut game, &rules, 100).round_done { n += 1; } }
    let mut save = encode(&game, &rules);
    save.ai = driver.save();
    std::fs::write("standard-266bd4d.aurelia", to_bytes(&save)).unwrap();
    println!("hash {:016x}", authoritative_hash(&game));
}
