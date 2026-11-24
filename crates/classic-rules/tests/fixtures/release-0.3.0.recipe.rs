//! How `release-0.3.0-small.aurelia` and `release-0.3.0-big.aurelia` were made (release 0.4.0, 24 Nov 2026): this
//! file as an example of a checkout of the tag v0.3.0 (platform 5881e9b), with the shipped rule set
//! (`classic_rules()`) and the standard AI of that release. Small: 4 nations, 40x32, 100 rounds, world hash
//! d918cf7575c77222. Big: 6 nations, 60x40, 160 rounds, world hash 9237ef94dbcca77f. Never overwrite the files:
//! they stand for the saves the players made with 0.3.0.
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::persistence::encode::encode;
use classic_rules::replay::authoritative_hash;
use classic_rules::save_codec::to_bytes;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::compile_rules;
use kernel::ai::driver::AiDriver;

fn main() {
    let out = std::env::args().nth(1).unwrap();
    for (name, rounds, players, w, h) in [("small", 100u32, 4u8, 40, 32), ("big", 160, 6u8, 60, 40)] {
        let rules = compile_rules(classic_content::classic::classic_rules()).unwrap();
        let config = ScenarioConfig { seed: 0x03_5EED + players as u64, players, width: w, height: h, cities: 0, units: 0, water_ratio: 15 };
        let mut game = generate_new_game(&rules, &config);
        let mut driver = AiDriver::with_mind(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new(), ClassicMind::new(Variant::Standard));
        let mut n = 0;
        while n < rounds { if driver.run(&mut game, &rules, 100).round_done { n += 1; } }
        let mut save = encode(&game, &rules);
        save.ai = driver.save();
        let bytes = to_bytes(&save);
        std::fs::write(format!("{out}/v030-{name}.aurelia"), &bytes).unwrap();
        println!("{name}: {} bytes, hash {:016x}, ai version {}", bytes.len(), authoritative_hash(&game), save.ai.version);
    }
}
