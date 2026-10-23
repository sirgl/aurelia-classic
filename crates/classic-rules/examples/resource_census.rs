//! Census of the map resources of a new game: for every map script and world size, how many
//! land, water and shallow-water tiles a map has and how many deposits of each resource the
//! generator places (the mean over seeds), and how many marine deposits lie within the work
//! radius of the players' starts and of the city-states.
//!
//! `cargo run --release -p classic-rules --example resource_census -- [sizes] [scripts] [seeds]`
//! sizes: comma list of duel,tiny,small,standard,large,huge (default duel,small,standard,large);
//! scripts: comma list of the map script keys (default all five); seeds: a count (default 20).
//! `--resources` prints the deposit counts of every resource, not only the sea and strategic ones.
use classic_content::core::core_rules;
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::rules::content::ResourceClass;
use classic_rules::rules::roles::Ground;
use classic_rules::scenario::{generate_new_game_with, MapScript, NewGameOptions, ScenarioConfig};
use classic_rules::world::size::WorldSize;
use classic_rules::compile_rules;
use std::collections::BTreeMap;

/// Nations and city-states of a map size.
fn players_of(size: WorldSize) -> (u8, u8) {
    match size {
        WorldSize::Duel => (2, 4),
        WorldSize::Tiny => (4, 8),
        WorldSize::Small => (6, 12),
        WorldSize::Standard => (8, 16),
        WorldSize::Large => (10, 20),
        WorldSize::Huge => (12, 24),
    }
}

fn size_named(name: &str) -> WorldSize {
    match name {
        "duel" => WorldSize::Duel,
        "tiny" => WorldSize::Tiny,
        "small" => WorldSize::Small,
        "standard" => WorldSize::Standard,
        "large" => WorldSize::Large,
        "huge" => WorldSize::Huge,
        other => panic!("unknown size {other}"),
    }
}

#[derive(Default)]
struct Totals {
    maps: u32,
    land: u64,
    water: u64,
    coast: u64,
    shallow: u64,
    deposits: BTreeMap<String, u64>,
    /// Marine deposits within 3 tiles of a major civilization's start (sum), and the number
    /// of starts without one.
    near_start: u64,
    /// Starts with water within 3 tiles, and of them the ones without a marine deposit there.
    starts: u64,
    starts_without_fish: u64,
    near_city_state: u64,
    city_states: u64,
    /// Wall time of the generation alone, microseconds.
    generation_micros: u128,
}

fn main() {
    let mut positional = Vec::new();
    let mut all_resources = false;
    for arg in std::env::args().skip(1) {
        if arg == "--resources" {
            all_resources = true;
        } else {
            positional.push(arg);
        }
    }
    let sizes: Vec<WorldSize> = positional
        .first()
        .map(|s| s.split(',').map(size_named).collect())
        .unwrap_or_else(|| vec![WorldSize::Duel, WorldSize::Small, WorldSize::Standard, WorldSize::Large]);
    let scripts: Vec<MapScript> = positional
        .get(1)
        .map(|s| s.split(',').map(|k| MapScript::from_key(k).expect("known map script")).collect())
        .unwrap_or_else(|| MapScript::ALL.to_vec());
    let seeds: u64 = positional.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);
    let rules = compile_rules(core_rules()).unwrap();
    let water_code = rules.terrain_kinds.palette().code(Ground::Water);
    let class_of = |key: &str| {
        rules.resources.iter().find(|r| r.key == key).and_then(|r| r.class)
    };
    for size in sizes {
        let (w, h) = size.grid();
        let (civs, minors) = players_of(size);
        for script in &scripts {
            let mut t = Totals::default();
            for seed in 1..=seeds {
                let cfg = ScenarioConfig {
                    seed,
                    players: civs,
                    width: w,
                    height: h,
                    cities: 0,
                    units: 0,
                    water_ratio: script.water_ratio(25),
                };
                let options = NewGameOptions { city_states: minors, script: *script, ..Default::default() };
                let started = std::time::Instant::now();
                let game = generate_new_game_with(&rules, &cfg, None, options);
                t.generation_micros += started.elapsed().as_micros();
                let auth = game.view().auth;
                let map = auth.map();
                let dims = map.dimensions;
                let n = w * h;
                let is_water = |tile: TileId| map.terrain(tile).map(|x| x.0) == Some(water_code);
                t.maps += 1;
                let mut marine_tiles = Vec::new();
                for i in 0..n {
                    let tile = TileId(i);
                    if is_water(tile) {
                        t.water += 1;
                        if dims.neighbors(tile).into_iter().any(|nb| !is_water(nb)) {
                            t.coast += 1;
                        }
                        if dims.neighbors(tile).into_iter().any(|nb| !is_water(nb))
                            || dims.neighbors(tile).into_iter().any(|nb| dims.neighbors(nb).into_iter().any(|nn| !is_water(nn)))
                        {
                            t.shallow += 1;
                        }
                    } else {
                        t.land += 1;
                    }
                    if let Some(Some(r)) = map.resource(tile) {
                        *t.deposits.entry(rules.resources[r.0 as usize].key.clone()).or_default() += 1;
                        if is_water(tile) {
                            marine_tiles.push(tile);
                        }
                    }
                }
                let near = |center: TileId| {
                    marine_tiles.iter().filter(|m| dims.distance(center, **m).is_some_and(|d| d <= 3)).count() as u64
                };
                for p in 0..civs {
                    let start = auth.units().iter_handles().find(|(_, u)| u.owner == PlayerId(p)).map(|(_, u)| u.tile);
                    if let Some(start) = start {
                        let wet = (0..n).any(|i| is_water(TileId(i)) && dims.distance(start, TileId(i)).is_some_and(|d| d <= 3));
                        if wet {
                            let found = near(start);
                            t.near_start += found;
                            t.starts += 1;
                            t.starts_without_fish += u64::from(found == 0);
                        }
                    }
                }
                for (_, city) in auth.cities().iter() {
                    if city.owner.0 >= civs {
                        t.near_city_state += near(city.tile);
                        t.city_states += 1;
                    }
                }
            }
            let m = f64::from(t.maps);
            println!(
                "## {} / {} ({}x{}, {} civs, {} city-states, {} seeds): land {:.0}, water {:.0}, coast {:.0}, shallow {:.0}",
                script.key(),
                format!("{size:?}").to_lowercase(),
                w,
                h,
                civs,
                minors,
                t.maps,
                t.land as f64 / m,
                t.water as f64 / m,
                t.coast as f64 / m,
                t.shallow as f64 / m
            );
            println!("   generation {:.1} ms per map", t.generation_micros as f64 / m / 1000.0);
            let mut line = Vec::new();
            for (key, count) in &t.deposits {
                let strategic = class_of(key) == Some(ResourceClass::Strategic);
                let marine = ["fish", "whales", "pearls", "crab"].iter().any(|s| key.ends_with(s));
                if all_resources || strategic || marine {
                    line.push(format!("{} {:.1}", key.trim_start_matches("core:resource/"), *count as f64 / m));
                }
            }
            let strategic_total: u64 = t
                .deposits
                .iter()
                .filter(|(k, _)| class_of(k) == Some(ResourceClass::Strategic))
                .map(|(_, c)| *c)
                .sum();
            println!("   deposits per map: {}", line.join(", "));
            println!(
                "   strategic total {:.1}; marine within 3 of a coastal start {:.2} (coastal starts without any {:.0} % of {:.1} per map), of a city-state {:.2}",
                strategic_total as f64 / m,
                t.near_start as f64 / t.starts.max(1) as f64,
                100.0 * t.starts_without_fish as f64 / t.starts.max(1) as f64,
                t.starts as f64 / m,
                t.near_city_state as f64 / t.city_states.max(1) as f64
            );
        }
    }
}
