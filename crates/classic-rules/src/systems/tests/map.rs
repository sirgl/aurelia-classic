//! Map generation used by the systems: resources and rivers.

use super::support::*;

#[test]
fn generator_places_resources_by_terrain() {
    use crate::ids::ResourceId;
    // Generator: resources only on their terrains, water is empty,
    // two runs with the same seed are pixel-identical.
    let rules = crate::fixtures::arena_content();
    let cfg = crate::scenario::ScenarioConfig {
        seed: 41,
        players: 2,
        width: 16,
        height: 16,
        cities: 0,
        units: 0,
        water_ratio: 10,
    };
    let g1 = crate::scenario::generate(&cfg, &rules);
    let g2 = crate::scenario::generate(&cfg, &rules);
    let h1 = crate::replay::authoritative_hash(&g1);
    let h2 = crate::replay::authoritative_hash(&g2);
    assert_eq!(h1, h2, "generator must be deterministic");
    let view = g1.view();
    let mut count = 0;
    for i in 0..view.auth.map().tile_count() {
        let t = crate::ids::TileId(i as u32);
        if let Some(Some(res)) = view.auth.map().resource(t) {
            count += 1;
            let terrain = view.auth.map().terrain(t).map(|x| x.0).unwrap();
            // Terrains come from the fixture data (wheat [0,1], gold [2,3]).
            if res == ResourceId(1) {
                assert!(
                    terrain == 0 || terrain == 1,
                    "wheat only on plains/grass, got {}",
                    terrain
                );
            } else {
                assert!(
                    terrain == 2 || terrain == 3,
                    "gold only on hills/desert, got {}",
                    terrain
                );
            }
            assert_ne!(
                terrain,
                crate::fixtures::WATER.0,
                "never on water"
            );
        }
    }
    assert!(count > 0, "density 8 must place some wheat on 16x16");
}

#[test]
fn generator_places_rivers_off_water() {
    // Rivers: present, not on water, deterministic by seed (hash).
    let rules = crate::fixtures::arena_content();
    let cfg = crate::scenario::ScenarioConfig {
        seed: 42,
        players: 2,
        width: 32,
        height: 32,
        cities: 0,
        units: 0,
        water_ratio: 15,
    };
    let g1 = crate::scenario::generate(&cfg, &rules);
    let g2 = crate::scenario::generate(&cfg, &rules);
    assert_eq!(
        crate::replay::authoritative_hash(&g1),
        crate::replay::authoritative_hash(&g2)
    );
    let view = g1.view();
    let mut count = 0;
    for i in 0..view.auth.map().tile_count() {
        let t = crate::ids::TileId(i as u32);
        if view.auth.map().river(t) {
            count += 1;
            assert_ne!(
                view.auth.map().terrain(t).map(|x| x.0).unwrap(),
                crate::fixtures::WATER.0,
                "rivers never on water"
            );
        }
    }
    assert!(count > 0, "32x32 must grow rivers");
}

#[test]
fn river_edges_canonicalize_and_sync_cache() {
    // Edges are canonical (min,max + sort + dedup); the tile cache has
    // both ends; the save round-trips the edges (the cache is built from them).
    let (mut game, rules) = arena_content();
    let dims = game.view().auth.map().dimensions;
    let a = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let b = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    let c = dims.to_tile(HexCoord { q: 9, r: 7 }).unwrap();
    game.debug_set_river_edges(&rules, &[(b, a), (b, c), (b, a)]);
    let view = game.view();
    let mut expect = vec![(a.0.min(b.0), a.0.max(b.0)), (b.0.min(c.0), b.0.max(c.0))];
    expect.sort();
    expect.dedup();
    assert_eq!(view.auth.map().river_edges(), &expect[..]);
    assert!(view.auth.map().river(a));
    assert!(view.auth.map().river(b));
    assert!(view.auth.map().river(c));
    let save = crate::persistence::encode::encode(&game, &rules);
    let loaded = crate::persistence::decode::decode(&save, &rules).unwrap();
    assert_eq!(
        loaded.view().auth.map().river_edges(),
        view.auth.map().river_edges()
    );
    assert_eq!(
        crate::replay::authoritative_hash(&game),
        crate::replay::authoritative_hash(&loaded)
    );
    game.validate(&rules).unwrap();
}
