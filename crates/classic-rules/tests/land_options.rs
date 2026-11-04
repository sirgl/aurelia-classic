//! The land's shares of a generated map (`kernel::scenario::LandOptions`,
//! classic/docs/ai-science-audit.md "The ideal case"): the classic options
//! are the map as it always was, bit for bit; the rich land of the science
//! audit's bench has the mountains, hills, forest, rivers and land resources
//! it asks for.

use classic_content::core::core_rules;
use classic_rules::ids::TileId;
use classic_rules::rules::roles::{FeatureRole, Ground};
use classic_rules::scenario::{generate_new_game_on, generate_new_game_with, LandOptions, MapScript, NewGameOptions, ScenarioConfig};
use classic_rules::world::size::WorldSize;
use classic_rules::{compile_rules, CompiledRuleset, GameState};

fn new_game(rules: &CompiledRuleset, seed: u64, land: Option<LandOptions>) -> GameState {
    let (width, height) = WorldSize::Small.grid();
    let script = MapScript::Continents;
    let cfg = ScenarioConfig { seed, players: 2, width, height, cities: 0, units: 0, water_ratio: script.water_ratio(25) };
    let options = NewGameOptions { city_states: 4, script, ..Default::default() };
    match land {
        None => generate_new_game_with(rules, &cfg, None, options),
        Some(land) => generate_new_game_on(rules, &cfg, None, options, land),
    }
}

/// Land tiles, mountains, hills, forest tiles, river edges and land deposits.
fn census(game: &GameState, rules: &CompiledRuleset) -> [usize; 6] {
    let map = game.view().auth.map();
    let palette = rules.terrain_kinds.palette();
    let code = |g: Ground| palette.code(g);
    let forest = rules.roles.feature(FeatureRole::Forest);
    let mut out = [0usize; 6];
    for i in 0..map.dimensions.tile_count() as u32 {
        let t = TileId(i);
        let Some(terrain) = map.terrain(t) else { continue };
        if terrain.0 == code(Ground::Water) {
            continue;
        }
        out[0] += 1;
        out[1] += usize::from(terrain.0 == code(Ground::Mountains));
        out[2] += usize::from(terrain.0 == code(Ground::Hills));
        out[3] += usize::from(forest.is_some() && map.feature(t).flatten() == forest);
        out[5] += usize::from(matches!(map.resource(t), Some(Some(_))));
    }
    out[4] = map.river_edges().len();
    out
}

/// `LandOptions::CLASSIC` generates the same world as the generator without
/// options: terrain, features, rivers, resources and starts.
#[test]
fn the_classic_options_are_the_classic_map() {
    let rules = compile_rules(core_rules()).unwrap();
    for seed in 1..=3 {
        let plain = new_game(&rules, seed, None);
        let classic = new_game(&rules, seed, Some(LandOptions::CLASSIC));
        assert_eq!(kernel::replay::hash::authoritative_hash(&plain), kernel::replay::hash::authoritative_hash(&classic), "seed {seed}");
        assert_eq!(LandOptions::default(), LandOptions::CLASSIC);
    }
}

/// The rich land: about twice the mountains, more hills, more forest, more
/// rivers and more land resources than the classic map of the same seed;
/// the mountains and hills take their exact quota of the land.
#[test]
fn the_rich_land_has_what_it_asks_for() {
    let rules = compile_rules(core_rules()).unwrap();
    for seed in 1..=3 {
        let classic = census(&new_game(&rules, seed, None), &rules);
        let rich = census(&new_game(&rules, seed, Some(LandOptions::RICH)), &rules);
        let share = |c: &[usize; 6], i: usize| 100.0 * c[i] as f64 / c[0] as f64;
        // Natural wonders may turn a few tiles into mountains or flat land.
        assert!((share(&classic, 1) - 6.0).abs() < 1.0, "seed {seed}: classic mountains {:.1} %", share(&classic, 1));
        assert!((share(&rich, 1) - 12.0).abs() < 1.0, "seed {seed}: rich mountains {:.1} %", share(&rich, 1));
        assert!((share(&rich, 2) - 22.0).abs() < 1.0, "seed {seed}: rich hills {:.1} %", share(&rich, 2));
        assert!(rich[3] > classic[3], "seed {seed}: forest {} against {}", rich[3], classic[3]);
        assert!(rich[4] > classic[4] * 3 / 2, "seed {seed}: river edges {} against {}", rich[4], classic[4]);
        assert!(rich[5] > classic[5] * 3 / 2, "seed {seed}: land deposits {} against {}", rich[5], classic[5]);
    }
}
