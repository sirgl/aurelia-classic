//! Resources at sea on generated classic maps (`kernel::scenario::sea_resources`): fish, whales,
//! pearls, crab and sea oil are placed by their `sea_density`, on the shallow sea only, apart
//! from one another, deterministically by the seed, and without moving the land.
//! The numbers behind the densities are in `lands/own-game-docs/DECISIONS.md`, "Awaiting numbers".

use classic_content::core::core_rules;
use classic_rules::ids::{PlayerId, ResourceId, TileId};
use classic_rules::rules::content::ResourceClass;
use classic_rules::rules::roles::Ground;
use classic_rules::scenario::{generate_new_game_with, MapScript, NewGameOptions, ScenarioConfig, StartBalance};
use classic_rules::world::size::WorldSize;
use classic_rules::{compile_rules, CompiledRuleset, GameState};

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

/// The same content with no resource at sea: the map as it was generated before.
fn rules_without_sea() -> CompiledRuleset {
    let mut source = core_rules();
    for resource in &mut source.resources {
        resource.sea_density = 0;
    }
    compile_rules(source).unwrap()
}

/// The numbers of nations and city-states of a size.
fn players(size: WorldSize) -> (u8, u8) {
    match size {
        WorldSize::Duel => (2, 4),
        WorldSize::Tiny => (4, 8),
        WorldSize::Small => (6, 12),
        _ => (8, 16),
    }
}

fn new_game(rules: &CompiledRuleset, script: MapScript, size: WorldSize, seed: u64) -> GameState {
    let (width, height) = size.grid();
    let (nations, minors) = players(size);
    let cfg = ScenarioConfig { seed, players: nations, width, height, cities: 0, units: 0, water_ratio: script.water_ratio(25) };
    generate_new_game_with(rules, &cfg, None, NewGameOptions { city_states: minors, script, ..Default::default() })
}

/// Deposits of a map in tile order: tile and resource key.
fn deposits(game: &GameState, rules: &CompiledRuleset) -> Vec<(u32, String)> {
    let map = game.view().auth.map();
    (0..map.dimensions.tile_count() as u32)
        .filter_map(|i| match map.resource(TileId(i)) {
            Some(Some(r)) => Some((i, rules.resources[r.0 as usize].key.clone())),
            _ => None,
        })
        .collect()
}

fn key_of(rules: &CompiledRuleset, resource: ResourceId) -> &str {
    &rules.resources[resource.0 as usize].key
}

fn count(deposits: &[(u32, String)], name: &str) -> usize {
    let key = format!("core:resource/{name}");
    deposits.iter().filter(|(_, k)| *k == key).count()
}

const SEEDS: std::ops::RangeInclusive<u64> = 1..=6;

/// Every map type places every sea resource, in numbers of the same order as the land's
/// luxuries, and the modern strategic resources are balanced to one another by land and sea.
#[test]
fn every_map_type_places_the_sea_resources_and_balances_oil() {
    let rules = rules();
    for script in MapScript::ALL {
        let mut total = std::collections::BTreeMap::<&str, usize>::new();
        for seed in SEEDS {
            let game = new_game(&rules, script, WorldSize::Small, seed);
            let all = deposits(&game, &rules);
            for name in ["fish", "whales", "pearls", "crab", "oil", "aluminum", "uranium"] {
                *total.entry(name).or_default() += count(&all, name);
            }
        }
        let maps = SEEDS.count();
        let mean = |name: &str| total[name] as f64 / maps as f64;
        let key = script.key();
        assert!(mean("fish") >= 6.0, "{key}: fish per small map {}", mean("fish"));
        for name in ["whales", "pearls", "crab"] {
            assert!(mean(name) >= 2.0, "{key}: {name} per small map {}", mean(name));
        }
        // Oil, land and sea together, is as common as aluminium, the other resource of its era
        // (a sea-rich map has somewhat more oil, a land-rich one somewhat less).
        let oil = mean("oil") / mean("aluminum");
        assert!((0.6..=2.2).contains(&oil), "{key}: oil to aluminium {oil}");
        let uranium = mean("uranium") / mean("aluminum");
        assert!((0.5..=1.5).contains(&uranium), "{key}: uranium to aluminium {uranium}");
    }
}

/// Sea deposits lie on water within two tiles of land, under no feature (ice, atoll) and no
/// natural wonder, and apart from one another; a land deposit lies on a terrain it lists; no city
/// stands on a deposit and no sea deposit is a land resource's neighbour in disguise (a resource
/// with no sea count is never at sea).
#[test]
fn deposits_lie_on_legal_tiles_only() {
    let rules = rules();
    let water = rules.terrain_kinds.palette().code(Ground::Water);
    for script in MapScript::ALL {
        for seed in 1..=3 {
            let game = new_game(&rules, script, WorldSize::Tiny, seed);
            let auth = game.view().auth;
            let map = auth.map();
            let dims = map.dimensions;
            let is_water = |t: TileId| map.terrain(t).map(|x| x.0) == Some(water);
            let near_land = |t: TileId, reach: usize| {
                let mut ring = vec![t];
                for _ in 0..reach {
                    ring = ring.iter().flat_map(|&x| dims.neighbors(x)).collect();
                    if ring.iter().any(|&x| !is_water(x)) {
                        return true;
                    }
                }
                false
            };
            for i in 0..dims.tile_count() as u32 {
                let tile = TileId(i);
                let Some(Some(resource)) = map.resource(tile) else {
                    continue;
                };
                let def = &rules.resources[resource.0 as usize];
                let label = format!("{} {} seed {seed} tile {i}", script.key(), def.key);
                if !is_water(tile) {
                    assert!(def.terrains.is_empty() || def.terrains.contains(&map.terrain(tile).unwrap().0), "{label}: a land deposit on a terrain it does not list");
                    assert!(def.density > 0, "{label}: a land deposit of a resource with no land count");
                    continue;
                }
                assert!(def.sea_density > 0, "{label}: a deposit at sea of a resource that is not placed there");
                assert!(near_land(tile, 2), "{label}: more than two tiles from land");
                assert_eq!(map.feature(tile), Some(None), "{label}: under a feature");
                assert!(map.natural_wonder(tile).is_none(), "{label}: on a natural wonder");
                for n in dims.neighbors(tile) {
                    if !is_water(n) {
                        continue;
                    }
                    if let Some(Some(other)) = map.resource(n) {
                        let same = other == resource;
                        let bonus = def.class == Some(ResourceClass::Bonus);
                        assert!(same && bonus, "{label}: beside {}", key_of(&rules, other));
                    }
                }
                assert!(auth.indexes().city_on_tile(tile).is_none(), "{label}: under a city");
            }
            for (_, city) in auth.cities().iter() {
                assert!(!is_water(city.tile), "{} seed {seed}: a city on water", script.key());
            }
        }
    }
}

/// A map is a function of its seed: the same seed gives the same deposits, another seed other ones.
#[test]
fn deposits_are_deterministic_by_seed() {
    let rules = rules();
    for script in MapScript::ALL {
        let first = deposits(&new_game(&rules, script, WorldSize::Small, 7), &rules);
        let again = deposits(&new_game(&rules, script, WorldSize::Small, 7), &rules);
        let other = deposits(&new_game(&rules, script, WorldSize::Small, 8), &rules);
        assert_eq!(first, again, "{}: the same seed", script.key());
        assert_ne!(first, other, "{}: another seed", script.key());
    }
}

/// `new_game` with the start balance off: the generator's own land, without the deposits the balance
/// puts near the starts (the balance values a start by the deposits at sea as well, so the starts, and
/// with them those deposits, depend on whether the sea has any).
fn new_game_unbalanced(rules: &CompiledRuleset, script: MapScript, size: WorldSize, seed: u64) -> GameState {
    let (width, height) = size.grid();
    let (nations, minors) = players(size);
    let cfg = ScenarioConfig { seed, players: nations, width, height, cities: 0, units: 0, water_ratio: script.water_ratio(25) };
    let options = NewGameOptions { city_states: minors, script, start_balance: StartBalance::OFF, ..Default::default() };
    generate_new_game_with(rules, &cfg, None, options)
}

/// The sea is drawn from hashes of its own: with or without sea resources the land, its features and
/// its land deposits are the same tile for tile (with the start balance off: see `new_game_unbalanced`).
#[test]
fn the_sea_does_not_move_the_land() {
    let (with, without) = (rules(), rules_without_sea());
    let water = with.terrain_kinds.palette().code(Ground::Water);
    for script in MapScript::ALL {
        let a = new_game_unbalanced(&with, script, WorldSize::Tiny, 5);
        let b = new_game_unbalanced(&without, script, WorldSize::Tiny, 5);
        let (ma, mb) = (a.view().auth.map(), b.view().auth.map());
        let n = ma.dimensions.tile_count() as u32;
        let land_deposits = |game: &GameState, rules: &CompiledRuleset| -> Vec<(u32, String)> {
            let map = game.view().auth.map();
            deposits(game, rules).into_iter().filter(|(i, _)| map.terrain(TileId(*i)).map(|t| t.0) != Some(water)).collect()
        };
        for i in 0..n {
            let t = TileId(i);
            assert_eq!(ma.terrain(t), mb.terrain(t), "{}: terrain {i}", script.key());
            assert_eq!(ma.feature(t), mb.feature(t), "{}: feature {i}", script.key());
            assert_eq!(ma.river(t), mb.river(t), "{}: river {i}", script.key());
        }
        assert_eq!(land_deposits(&a, &with), land_deposits(&b, &without), "{}: land deposits", script.key());
        assert!(deposits(&b, &without).iter().all(|(i, _)| ma.terrain(TileId(*i)).map(|t| t.0) != Some(water)), "no sea deposits without sea counts");
    }
}

/// A nation that starts with the sea in its work radius has a deposit at sea there.
#[test]
fn a_coastal_start_has_a_deposit_at_sea_in_its_work_radius() {
    let rules = rules();
    let water = rules.terrain_kinds.palette().code(Ground::Water);
    let mut coastal = 0;
    for script in MapScript::ALL {
        for seed in SEEDS {
            let game = new_game(&rules, script, WorldSize::Small, seed);
            let auth = game.view().auth;
            let map = auth.map();
            let dims = map.dimensions;
            let (nations, _) = players(WorldSize::Small);
            for p in 0..nations {
                let Some(start) = auth.units().iter_handles().find(|(_, u)| u.owner == PlayerId(p)).map(|(_, u)| u.tile) else {
                    continue;
                };
                let within = |t: TileId| dims.distance(start, t).is_some_and(|d| d <= 3);
                // Water under ice holds no deposit ("never under ice or an atoll"): a start by an iced sea has none to find.
                let sea: Vec<TileId> = (0..dims.tile_count() as u32)
                    .map(TileId)
                    .filter(|&t| within(t) && map.terrain(t).map(|x| x.0) == Some(water) && map.feature(t) == Some(None))
                    .collect();
                if sea.is_empty() {
                    continue;
                }
                coastal += 1;
                let marine = sea.iter().any(|&t| matches!(map.resource(t), Some(Some(_))));
                assert!(marine, "{} seed {seed} player {p}: coastal start without a sea deposit", script.key());
            }
        }
    }
    assert!(coastal > 40, "the maps have coastal starts to check: {coastal}");
}

/// A resource with a sea count above 100 % is refused by the compiler.
#[test]
fn a_sea_count_above_a_hundred_percent_is_refused() {
    let mut source = core_rules();
    source.resources[0].sea_density = 101;
    assert!(compile_rules(source).is_err());
}
