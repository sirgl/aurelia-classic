//! Map scripts shape the land as their names promise, on every seed of a
//! small matrix, with the exact water quota of the script.
use classic_rules::scenario::landscape::{self, Land};
use classic_rules::scenario::MapScript;

/// Land component sizes, largest first (edge neighbours).
fn components(land: &Land) -> Vec<usize> {
    let mut seen = vec![false; land.tiles()];
    let mut sizes = Vec::new();
    for start in 0..land.tiles() {
        if seen[start] || land.is_water(start) {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut size = 0;
        while let Some(i) = stack.pop() {
            size += 1;
            for n in land.neighbours(i) {
                if !seen[n] && !land.is_water(n) {
                    seen[n] = true;
                    stack.push(n);
                }
            }
        }
        sizes.push(size);
    }
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes
}

/// Duel, Small and Standard maps.
const SIZES: [(u32, u32); 3] = [(40, 24), (66, 42), (80, 52)];
const SIZE: (u32, u32) = (66, 42);
const SEEDS: [u64; 4] = [1, 7, 20260928, 99];

/// Every (seed, size) of the matrix.
fn matrix() -> impl Iterator<Item = (u64, (u32, u32))> {
    SEEDS.into_iter().flat_map(|seed| SIZES.into_iter().map(move |size| (seed, size)))
}

fn shaped(script: MapScript, seed: u64, size: (u32, u32)) -> (Land, Vec<usize>, usize) {
    let water = script.water_ratio(25);
    let land = landscape::land_shaped(size.0, size.1, water, seed, script);
    let tiles = land.tiles();
    let water_tiles = (0..tiles).filter(|&i| land.is_water(i)).count();
    assert_eq!(water_tiles, tiles * water as usize / 100, "{script:?} seed {seed}: exact water quota");
    let parts = components(&land);
    let total = tiles - water_tiles;
    (land, parts, total)
}

#[test]
fn fractal_is_the_original_generator() {
    for seed in SEEDS {
        let a = landscape::land(SIZE.0, SIZE.1, 25, seed);
        let b = landscape::land_shaped(SIZE.0, SIZE.1, 25, seed, MapScript::Fractal);
        assert_eq!(a.terrain, b.terrain);
    }
}

#[test]
fn pangaea_is_one_mass() {
    for (seed, size) in matrix() {
        let (_, parts, land) = shaped(MapScript::Pangaea, seed, size);
        assert!(parts[0] * 100 >= land * 70, "seed {seed} {size:?}: largest {} of {land}", parts[0]);
    }
}

#[test]
fn continents_are_two_large_masses() {
    for (seed, size) in matrix() {
        let (_, parts, land) = shaped(MapScript::Continents, seed, size);
        assert!(parts.len() >= 2, "seed {seed} {size:?}: {parts:?}");
        assert!(parts[1] * 100 >= land * 20, "seed {seed} {size:?}: second mass {parts:?} of {land}");
        assert!(parts[0] * 100 <= land * 75, "seed {seed} {size:?}: one mass dominates {parts:?}");
    }
}

#[test]
fn small_continents_are_several_masses() {
    for (seed, size) in matrix() {
        let (_, parts, land) = shaped(MapScript::SmallContinents, seed, size);
        let big = parts.iter().filter(|&&p| p * 100 >= land * 5).count();
        assert!(big >= 3, "seed {seed} {size:?}: {parts:?} of {land}");
        assert!(parts[0] * 100 <= land * 60, "seed {seed} {size:?}: {parts:?}");
    }
}

#[test]
fn archipelago_is_many_islands() {
    for (seed, size) in matrix() {
        let (_, parts, land) = shaped(MapScript::Archipelago, seed, size);
        assert!(parts.len() >= 8, "seed {seed} {size:?}: {parts:?}");
        assert!(parts[0] * 100 <= land * 35, "seed {seed} {size:?}: {parts:?} of {land}");
    }
}

#[test]
fn every_script_is_deterministic() {
    for script in MapScript::ALL {
        let water = script.water_ratio(25);
        let a = landscape::land_shaped(40, 24, water, 5, script);
        let b = landscape::land_shaped(40, 24, water, 5, script);
        assert_eq!(a.terrain, b.terrain, "{script:?}");
        assert_eq!(a.base, b.base, "{script:?}");
    }
}
