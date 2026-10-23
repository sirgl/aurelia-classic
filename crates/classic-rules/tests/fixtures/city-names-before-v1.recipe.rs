use classic_rules::{compile_rules, persistence::encode::encode, replay::authoritative_hash, scenario::{generate, ScenarioConfig}};
fn main() {
    let rules=compile_rules(classic_content::classic::classic_rules()).unwrap();
    let mut game=generate(&ScenarioConfig { seed:20261015, players:2, width:16, height:16, cities:4, units:6, water_ratio:0 },&rules);
    let first=game.view().auth.cities().iter().next().unwrap().0;
    game.debug_set_city_name(first,"My Custom Capital".into());
    game.validate(&rules).unwrap();
    let save=encode(&game,&rules);
    let bytes=classic_rules::save_codec::to_bytes(&save);
    let out=std::path::Path::new("crates/client/tests/fixtures/city-names-before-v1.aurelia");
    assert!(!out.exists(),"baseline must never be overwritten");
    std::fs::write(out,bytes).unwrap();
    println!("rules_hash={} world_hash={} schema={} sim={} cities={:?}",rules.hash(),authoritative_hash(&game),save.header.save_schema_version,save.header.simulation_version,save.world.cities.iter().map(|c|&c.name).collect::<Vec<_>>());
}
