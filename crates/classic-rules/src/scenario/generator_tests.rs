//! Tests of the kernel's generator on the classic rules.

mod tests {
    use crate::scenario::generator::*;
    use crate::ids::{PlayerId, TileId};
    use crate::rules::roles::Ground;
    use crate::rules::source::StartBias;
    use crate::world::state::GameState;
    use crate::rules::roles::TerrainPalette;
    use crate::compile_rules;
    use crate::rules::source::SourceNation;
    use crate::rules::SourceRules;

    #[test]
    fn new_game_starts_dark_with_settlers() {
        // A start: a settler each, empty hands, a dark map.
        let rules = compile_rules(SourceRules::default()).unwrap();
        let cfg = ScenarioConfig {
            seed: 42,
            players: 4,
            width: 32,
            height: 32,
            cities: 0,
            units: 0,
            water_ratio: 10,
        };
        let game = generate_new_game(&rules, &cfg);
        assert_eq!(game.view().auth.turn().round, 0);
        assert_eq!(game.view().auth.cities().len(), 0);
        for p in 0..4u8 {
            assert_eq!(
                game.view().auth.indexes().units_of(PlayerId(p)).len(),
                1,
                "each player starts with exactly one settler"
            );
        }
        // Darkness: explored only around the starts (< 10% of the 1024 map).
        let explored: usize = game.view().auth.knowledge()[0].explored_count();
        assert!(explored > 0, "starting area revealed");
        assert!(explored < 400, "most of map dark, explored={}", explored);
        game.validate(&rules).unwrap();
    }

    /// Biomes are coherent patches: the exact share of water + most tiles
    /// surrounded by their own type (not white noise).
    #[test]
    fn biomes_are_coherent_patches_with_exact_water_share() {
        let rules = compile_rules(SourceRules::default()).unwrap();
        let cfg = ScenarioConfig {
            seed: 20260928,
            players: 2,
            width: 64,
            height: 64,
            cities: 0,
            units: 0,
            water_ratio: 10,
        };
        let game = generate(&cfg, &rules);
        let view = game.view();
        let (w, h) = (64i64, 64i64);
        let at = |q: i64, r: i64| -> Option<u16> {
            if q < 0 || r < 0 || q >= w || r >= h {
                return None;
            }
            view.auth
                .map()
                .terrain(crate::ids::TileId((r * w + q) as u32))
                .map(|t| t.0)
        };
        const E: [(i64, i64); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];
        let mut water = 0usize;
        let mut same3 = 0usize;
        let mut total = 0usize;
        let mut hills_core = false;
        let mut mountains = 0usize;
        let mut mountain_cluster = false;
        for r in 0..h {
            for q in 0..w {
                let t = at(q, r).unwrap();
                total += 1;
                if t == TerrainPalette::LEGACY.code(Ground::Water) {
                    water += 1;
                    continue;
                }
                let same = E
                    .iter()
                    .filter(|&&(dq, dr)| at(q + dq, r + dr) == Some(t))
                    .count();
                if same >= 3 {
                    same3 += 1;
                }
                if t == TerrainPalette::LEGACY.code(Ground::Hills) && same >= 4 {
                    hills_core = true;
                }
                if t == TerrainPalette::LEGACY.code(Ground::Mountains) {
                    mountains += 1;
                    if same >= 2 {
                        mountain_cluster = true;
                    }
                    // Mountains are impassable (like water, but land).
                    assert!(view.auth.map().terrain_kinds().enter_cost(crate::ids::TerrainId(t)).is_none());
                }
            }
        }
        // The exact water quota (sorting, not a threshold).
        assert_eq!(water, total * 10 / 100, "water share exact");
        // Coherence: >=60% of land surrounded by its own type (noise would give ~25%).
        let land = total - water;
        assert!(same3 * 100 >= land * 60, "patches coherent: {same3}/{land}");
        assert!(hills_core, "hills clusters exist (mountain cores)");
        // Mountains: present (~6% of land = 30% of the hills band), in clusters, impassable.
        assert!(
            mountains > land * 3 / 100,
            "mountains exist: {mountains}/{land}"
        );
        assert!(mountain_cluster, "mountains cluster into massifs");
        game.validate(&rules).unwrap();
    }

    /// A fixture: n nations with a chosen bias (the other content empty).
    fn nation_rules(n: usize, bias: StartBias) -> crate::rules::CompiledRuleset {
        let nations = (0..n)
            .map(|i| SourceNation {
                id: Some(crate::rules::source::StableContentKey(format!("nation{i}"))),
                name: format!("Nation {i}"),
                leader: format!("Leader {i}"),
                color: ((i as u8).wrapping_mul(40), 90, 200),
                start_bias: bias,
                effects: vec![],
                ..Default::default()
            })
            .collect();
        compile_rules(SourceRules {
            nations,
            ..SourceRules::default()
        })
        .unwrap()
    }

    fn settler_tile(g: &GameState, p: u8) -> TileId {
        let view = g.view();
        let hs = view.auth.indexes().units_of(PlayerId(p));
        assert_eq!(hs.len(), 1, "player {p} has exactly one settler");
        view.auth
            .units()
            .get(hs[0])
            .expect("settler present")
            .core
            .tile
    }

    /// Fair starts: land, margin from the edge, spread >= MIN_START_DIST and
    /// determinism by seed.
    #[test]
    fn smart_starts_are_on_land_spread_and_deterministic() {
        let rules = compile_rules(SourceRules::default()).unwrap();
        let cfg = ScenarioConfig {
            seed: 7,
            players: 6,
            width: 64,
            height: 64,
            cities: 0,
            units: 0,
            water_ratio: 10,
        };
        let g1 = generate_new_game(&rules, &cfg);
        let g2 = generate_new_game(&rules, &cfg);
        let s1: Vec<TileId> = (0..cfg.players).map(|p| settler_tile(&g1, p)).collect();
        let s2: Vec<TileId> = (0..cfg.players).map(|p| settler_tile(&g2, p)).collect();
        assert_eq!(s1, s2, "same seed ⇒ same starts");
        let map = g1.view().auth.map();
        let dims = map.dimensions;
        for &t in &s1 {
            let c = dims.to_coord(t).expect("in bounds");
            assert!(
                c.q >= START_MARGIN
                    && c.r >= START_MARGIN
                    && c.q < dims.width as i32 - START_MARGIN
                    && c.r < dims.height as i32 - START_MARGIN,
                "start {t:?} respects margin"
            );
            assert!(
                crate::navigation::cost::enter_cost_on_map(map, t).is_some(),
                "start {t:?} on land"
            );
        }
        for i in 0..s1.len() {
            for j in i + 1..s1.len() {
                let a = dims.to_coord(s1[i]).expect("in bounds");
                let b = dims.to_coord(s1[j]).expect("in bounds");
                let d = a.distance(b);
                assert!(d >= MIN_START_DIST, "spread {a:?} {b:?} dist={d}");
            }
        }
        g1.validate(&rules).unwrap();
    }

    /// Menu start: the human nation = P0, the rest unique, deterministic.
    #[test]
    fn human_nation_is_first_and_others_unique() {
        let rules = nation_rules(6, StartBias::None);
        let cfg = ScenarioConfig::quick_game();
        let nations_of = |g: &GameState| -> Vec<u8> {
            (0..cfg.players)
                .map(|p| g.view().auth.players().nation_of(PlayerId(p)))
                .collect()
        };
        let g1 = generate_player_game(&rules, &cfg, 4);
        let v = nations_of(&g1);
        assert_eq!(v[0], 4, "human picks nation 4 => player 0 has it: {v:?}");
        let mut uniq = v.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), v.len(), "nations unique: {v:?}");
        let g2 = generate_player_game(&rules, &cfg, 4);
        assert_eq!(v, nations_of(&g2), "same seed+choice ⇒ same assignment");
        g1.validate(&rules).unwrap();
    }

    /// Coastal bias: each nation's start is within radius 6 of water (fixed seed).
    #[test]
    fn coastal_bias_starts_reach_the_sea() {
        let rules = nation_rules(6, StartBias::Coastal);
        let cfg = ScenarioConfig {
            seed: 7,
            players: 6,
            width: 64,
            height: 64,
            cities: 0,
            units: 0,
            water_ratio: 10,
        };
        let g = generate_player_game(&rules, &cfg, 0);
        let view = g.view();
        let dims = view.auth.map().dimensions;
        for p in 0..cfg.players {
            let t = settler_tile(&g, p);
            let near_sea = crate::math::tiles_in_hex_radius(&dims, t, 6)
                .iter()
                .any(|&n| {
                    view.auth
                        .map()
                        .terrain(n)
                        .is_some_and(|g| view.auth.map().terrain_kinds().is_water(g.0))
                });
            assert!(near_sea, "player {p} start must be within 6 hexes of water");
        }
    }

    /// Forest grows on grassland, plains and tundra (the fixture's valid terrains)
    /// and on hills (a plot type over those), never on desert, snow,
    /// mountains or water; the density is reasonable and the layout is
    /// deterministic. The fixture content has forest as its only feature.
    #[test]
    fn generator_places_forests_on_grass_plains_tundra_and_hills_only() {
        let rules = crate::fixtures::arena_content();
        let cfg = ScenarioConfig {
            seed: 7,
            players: 2,
            width: 24,
            height: 16,
            cities: 1,
            units: 1,
            water_ratio: 30,
        };
        let scan = |g: &GameState| -> (Vec<Option<crate::ids::FeatureId>>, usize, usize) {
            let view = g.view();
            let map = view.auth.map();
            let mut feats = Vec::with_capacity(map.tile_count());
            let (mut grass, mut forest) = (0usize, 0usize);
            for i in 0..map.tile_count() as u32 {
                let t = TileId(i);
                let f = map.feature(t).flatten();
                let tr = map.terrain(t).map(|x| x.0).unwrap_or(99);
                if matches!(tr, 0 | 1 | 2 | 7) {
                    grass += 1;
                }
                if f.is_some() {
                    assert!(matches!(tr, 0 | 1 | 2 | 7), "forest on terrain {tr}");
                    forest += 1;
                }
                feats.push(f);
            }
            (feats, grass, forest)
        };
        let g1 = generate(&cfg, &rules);
        let (f1, grass, forest) = scan(&g1);
        assert!(forest > grass / 10, "some forests ({forest}/{grass})");
        assert!(forest < grass, "not every grass tile is forest");
        let g2 = generate(&cfg, &rules);
        let (f2, _, _) = scan(&g2);
        assert_eq!(f1, f2, "same seed ⇒ same forest layout");
        g1.validate(&rules).unwrap();
    }
}
