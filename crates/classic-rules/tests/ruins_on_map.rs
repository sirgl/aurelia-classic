//! A new classic map carries Ancient Ruins: about one per eighty tiles of
//! land, on free land away from every start and city, apart from each
//! other, the same on every build of the same seed; and a unit that walks
//! onto a ruin takes its reward (`systems::ruins`).

use classic_content::core::core_rules;
use classic_rules::ids::TileId;
use classic_rules::replay::authoritative_hash;
use classic_rules::rules::content::ImprovementKind;
use classic_rules::scenario::ruins::{LAND_TILES_PER_RUINS, MIN_DISTANCE_BETWEEN, MIN_DISTANCE_FROM_START};
use classic_rules::scenario::{generate_new_game_with, MapScript, NewGameOptions, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset, GameState};

const SEEDS: [u64; 4] = [1, 7, 21, 99];
const SCRIPTS: [MapScript; 3] = [MapScript::Fractal, MapScript::Pangaea, MapScript::Archipelago];

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn new_game(rules: &CompiledRuleset, seed: u64, script: MapScript) -> GameState {
    let cfg = ScenarioConfig { seed, players: 4, width: 56, height: 36, cities: 0, units: 0, water_ratio: 30 };
    let options = NewGameOptions { city_states: 4, script, ..Default::default() };
    generate_new_game_with(rules, &cfg, None, options)
}

fn ruins_of(game: &GameState, rules: &CompiledRuleset) -> Vec<TileId> {
    let map = game.view().auth.map();
    (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| {
            map.improvement(*t)
                .flatten()
                .is_some_and(|i| rules.improvements[i.0 as usize].kind == ImprovementKind::AncientRuins)
        })
        .collect()
}

#[test]
fn a_new_map_has_ruins_in_proportion_to_its_land() {
    let rules = rules();
    for script in SCRIPTS {
        for seed in SEEDS {
            let game = new_game(&rules, seed, script);
            let map = game.view().auth.map();
            let land = (0..map.tile_count() as u32).filter(|t| !map.is_water(TileId(*t))).count();
            let ruins = ruins_of(&game, &rules).len();
            let wanted = land / LAND_TILES_PER_RUINS;
            assert!(ruins >= 3, "{script:?} seed {seed}: {ruins} ruins on {land} land tiles");
            assert!(ruins <= wanted, "{script:?} seed {seed}: {ruins} ruins, at most {wanted}");
            assert!(ruins * 4 >= wanted * 3, "{script:?} seed {seed}: {ruins} of {wanted} wanted");
        }
    }
}

#[test]
fn ruins_stand_on_free_land_away_from_starts_and_apart() {
    let rules = rules();
    for seed in SEEDS {
        let game = new_game(&rules, seed, MapScript::Fractal);
        let auth = game.view().auth;
        let map = auth.map();
        let dims = map.dimensions;
        let ruins = ruins_of(&game, &rules);
        let homes: Vec<TileId> = auth
            .units()
            .iter_cores()
            .map(|u| u.tile)
            .chain(auth.cities().iter().map(|(_, c)| c.tile))
            .collect();
        for (i, tile) in ruins.iter().enumerate() {
            assert!(!map.is_water(*tile), "seed {seed}: a ruin at sea");
            assert!(map.natural_wonder(*tile).is_none(), "seed {seed}: a ruin on a natural wonder");
            assert!(map.resource(*tile).flatten().is_none(), "seed {seed}: a ruin on a resource");
            for home in &homes {
                let d = dims.distance(*home, *tile).unwrap();
                assert!(d >= MIN_DISTANCE_FROM_START, "seed {seed}: a ruin {d} tiles from a start");
            }
            for other in &ruins[i + 1..] {
                let d = dims.distance(*other, *tile).unwrap();
                assert!(d >= MIN_DISTANCE_BETWEEN, "seed {seed}: two ruins {d} tiles apart");
            }
        }
    }
}

#[test]
fn the_same_seed_lays_the_same_ruins() {
    let rules = rules();
    let a = new_game(&rules, 21, MapScript::Fractal);
    let b = new_game(&rules, 21, MapScript::Fractal);
    assert_eq!(ruins_of(&a, &rules), ruins_of(&b, &rules));
    assert_eq!(authoritative_hash(&a), authoritative_hash(&b));
    let other = new_game(&rules, 22, MapScript::Fractal);
    assert_ne!(ruins_of(&a, &rules), ruins_of(&other, &rules), "another seed, another ruins");
}

#[test]
fn the_ai_explorers_take_ruins_in_the_first_hundred_rounds() {
    use classic_rules::ai::{AiDriver, AiSettings};
    let rules = rules();
    let mut game = new_game(&rules, 21, MapScript::Fractal);
    let before = ruins_of(&game, &rules).len();
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new());
    while game.view().auth.turn().round < 100 && game.winner().is_none() {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let after = ruins_of(&game, &rules).len();
    assert!(after < before, "{after} of {before} ruins left after 100 rounds");
    game.validate(&rules).unwrap();
}
