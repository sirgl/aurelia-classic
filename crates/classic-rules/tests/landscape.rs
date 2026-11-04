//! The generated land is classic-like (scenario/landscape.rs): every terrain
//! in plausible shares and in its climate band, every feature where the
//! classic rules put it, natural wonders by the placement rules of the
//! classic ruleset.

use classic_content::core::core_rules;
use classic_rules::scenario::landscape::{self, DESERT, GRASS, HILLS, MOUNTAIN, PLAINS, SNOW, TUNDRA, WATER};
use classic_rules::rules::roles::Ground;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{compile_rules, CompiledRuleset};

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn share(count: usize, of: usize) -> f64 {
    count as f64 * 100. / of as f64
}

#[test]
fn every_terrain_in_its_band_and_in_plausible_shares() {
    let land = landscape::land(200, 125, 15, 1);
    let again = landscape::land(200, 125, 15, 1);
    assert_eq!(land.terrain, again.terrain, "deterministic");
    let dry = land.terrain.iter().filter(|&&t| t != WATER).count();
    let count = |t: Ground| land.terrain.iter().filter(|&&x| x == t).count();
    assert_eq!(count(WATER), 25_000 * 15 / 100, "exact water quota");
    let bands = [(PLAINS, 15., 40.), (GRASS, 15., 40.), (DESERT, 5., 18.), (TUNDRA, 4., 14.), (SNOW, 2., 10.), (HILLS, 13., 15.), (MOUNTAIN, 5., 7.)];
    for (t, low, high) in bands {
        let s = share(count(t), dry);
        assert!((low..=high).contains(&s), "terrain {t:?}: {s:.1} % of the land");
    }
    for i in 0..land.tiles() {
        let lat = land.latitude(i);
        match land.base[i] {
            SNOW => assert!(lat > 750, "snow at latitude {lat}"),
            TUNDRA => assert!(lat > 640, "tundra at latitude {lat}"),
            DESERT => assert!(lat < 860, "desert at latitude {lat}"),
            _ => {}
        }
    }
    // Hills stand on every climate: their base terrain varies.
    let mut bases: Vec<Ground> = (0..land.tiles()).filter(|&i| land.terrain[i] == HILLS).map(|i| land.base[i]).collect();
    bases.sort();
    bases.dedup();
    assert!(bases.len() >= 4, "hills on {bases:?}");
}

#[test]
fn features_grow_where_the_rules_put_them() {
    let rules = rules();
    let key = |f: Option<classic_rules::ids::FeatureId>| f.map(|f| rules.features[f.0 as usize].key.as_str());
    let land = landscape::land(200, 125, 15, 1);
    let mut x = 7u64;
    let edges = landscape::rivers(&land, |n| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    });
    let river = landscape::river_tiles(&land, &edges);
    let features = landscape::features(&land, &river, &rules);
    let mut seen = std::collections::BTreeMap::new();
    for i in 0..land.tiles() {
        let Some(name) = key(features[i]) else { continue };
        *seen.entry(name).or_insert(0) += 1;
        let (t, base, lat) = (land.terrain[i], land.base[i], land.latitude(i));
        match name {
            "core:feature/ice" => assert!(t == WATER && lat > 800, "ice at {lat}"),
            "core:feature/atoll" => assert!(t == WATER && lat < 560),
            "core:feature/jungle" => assert!(base == GRASS && lat < 330),
            "core:feature/marsh" => assert!(t == GRASS),
            "core:feature/oasis" => assert!(t == DESERT && !river[i]),
            "core:feature/flood-plains" => assert!(t == DESERT && river[i]),
            "core:feature/forest" => assert!(matches!(base, GRASS | PLAINS | TUNDRA) && t != MOUNTAIN),
            other => panic!("unexpected feature {other}"),
        }
        // The feature's own rules (RequiresRiver, RequiresFlatlands).
        let def = &rules.features[features[i].unwrap().0 as usize];
        assert!(def.fits_site(t != HILLS && t != MOUNTAIN, river[i]), "{name} breaks its site rules");
    }
    let flood = rules.features.iter().find(|f| f.key == "core:feature/flood-plains").unwrap();
    assert!(flood.requires_river && flood.requires_flatlands);
    assert!(!flood.fits_site(true, false) && !flood.fits_site(false, true) && flood.fits_site(true, true));
    for name in ["ice", "atoll", "jungle", "marsh", "oasis", "flood-plains", "forest"] {
        assert!(seen.contains_key(format!("core:feature/{name}").as_str()), "no {name}: {seen:?}");
    }
    assert!(edges.len() > 300, "rivers: {} edges", edges.len());
}

#[test]
fn natural_wonders_follow_their_placement_rules() {
    let rules = rules();
    let mut placed = std::collections::BTreeSet::new();
    for seed in 0..6u64 {
        let cfg = ScenarioConfig { cities: 0, units: 0, ..ScenarioConfig::representative_late_game() };
        let cfg = ScenarioConfig { seed: cfg.seed.wrapping_add(seed), ..cfg };
        let game = generate(&cfg, &rules);
        for (_, wonder) in check_sites(&game, &rules) {
            placed.insert(rules.natural_wonders[wonder.0 as usize].key.clone());
        }
        if seed == 0 {
            let again = generate(&cfg, &rules);
            assert_eq!(check_sites(&again, &rules), check_sites(&game, &rules), "deterministic");
        }
        game.validate(&rules).unwrap();
    }
    assert!(placed.len() >= 10, "most wonders find a site on some map: {placed:?}");
    assert!(placed.contains("core:natural-wonder/krakatoa"), "Krakatoa rises from the sea: {placed:?}");
}

/// The wonder sites of a generated map, each checked against its rules
/// (the rules that the core tile change does not erase).
fn check_sites(
    game: &classic_rules::GameState,
    rules: &CompiledRuleset,
) -> Vec<(classic_rules::ids::TileId, classic_rules::rules::content::NaturalWonderId)> {
    use classic_rules::rules::natural_wonders::{AdjacentBound, AdjacentKind, CoreShape, PlacementMethod};
    let map = game.view().auth.map();
    let sites: Vec<_> = (0..map.tile_count() as u32)
        .map(classic_rules::ids::TileId)
        .filter_map(|t| map.natural_wonder(t).map(|w| (t, w)))
        .collect();
    assert_eq!(sites.len(), landscape::natural_wonder_count(25_000), "{sites:?}");
    let dims = map.dimensions;
    let palette = rules.terrain_kinds.palette();
    let ground = |t: classic_rules::ids::TileId| palette.ground(map.terrain(t).unwrap().0).unwrap();
    let water = |t| ground(t) == WATER;
    for (i, (tile, wonder)) in sites.iter().enumerate() {
        let def = &rules.natural_wonders[wonder.0 as usize];
        let p = &def.placement;
        let terrain = ground(*tile);
        let around: Vec<classic_rules::ids::TileId> = dims.neighbors(*tile).into_iter().collect();
        match p.core_shape {
            CoreShape::Mountain => assert_eq!(terrain, MOUNTAIN, "{} stands on a mountain", def.key),
            CoreShape::Flat => assert!(terrain != HILLS && terrain != MOUNTAIN, "{} is flat", def.key),
            CoreShape::Keep => {}
        }
        if p.method == PlacementMethod::Land && p.avoid_ocean_coast {
            assert!(around.iter().all(|t| !water(*t)), "{} keeps away from the sea", def.key);
        }
        if p.surround_with_water {
            assert!(around.iter().all(|t| water(*t)), "{} stands in the sea", def.key);
        }
        for rule in &p.adjacent {
            let count = around
                .iter()
                .filter(|t| match rule.kind {
                    AdjacentKind::Hills => ground(**t) == HILLS,
                    AdjacentKind::Mountain => ground(**t) == MOUNTAIN,
                    AdjacentKind::HillsOrMountain => [HILLS, MOUNTAIN].contains(&ground(**t)),
                    AdjacentKind::Land => !water(**t),
                    AdjacentKind::Water => water(**t),
                    AdjacentKind::Feature(f) => map.feature(**t) == Some(Some(f)),
                    // Base terrains under hills are not kept in the world.
                    AdjacentKind::Terrain(_) => return false,
                })
                .count() as u8;
            match (rule.kind, rule.bound) {
                (AdjacentKind::Terrain(_), _) => {}
                (_, AdjacentBound::AtLeast(n)) => assert!(count >= n, "{}: {:?}", def.key, rule),
                (_, AdjacentBound::AtMost(n)) => assert!(count <= n, "{}: {:?}", def.key, rule),
            }
        }
        assert_eq!(map.feature(*tile), Some(None), "no feature under {}", def.key);
        for (other, _) in &sites[i + 1..] {
            assert!(dims.distance(*tile, *other).unwrap() >= 7, "wonders spaced");
        }
    }
    sites
}
