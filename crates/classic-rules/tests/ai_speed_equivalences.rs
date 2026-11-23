//! The structure changes that made the AI's turn cheaper (`perf/ai-turn`) change no answer: each fast
//! version is compared with the plain one it replaced, on the worlds of whole AI games.
//!
//! * `maintenance::route_maintenance` reads the territory without listing it;
//! * `religion::compute_religion_counts` shares the followers of a city between its two counts.

use classic_content::core::core_rules;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::derived::maintenance::{route_maintenance, territory, RAILROAD_MAINTENANCE, ROAD_MAINTENANCE};
use classic_rules::derived::religion::{compute_religion_counts, followers, majority, FollowerCounts};
use classic_rules::effects::player::player_modifiers;
use classic_rules::ids::PlayerId;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::rules::stats::{has_rule, PlayerRule, Stat};
use classic_rules::world::map::Route;
use classic_rules::world::state::AuthWorld;
use classic_rules::{compile_rules, CompiledRuleset};

/// `route_maintenance` as it was written: the listed territory, filtered and summed.
fn route_maintenance_listed(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> i32 {
    let map = auth.map();
    let stats = &player_modifiers(auth, rules, player).stats;
    let free_hills = has_rule(stats, PlayerRule::FreeHillsRouteMaintenance);
    let total: i64 = territory(auth, player)
        .into_iter()
        .filter(|(_, t)| auth.indexes().city_on_tile(*t).is_none())
        .filter(|(_, t)| !(free_hills && map.terrain(*t).is_some_and(|k| map.terrain_kinds().is_hills(k.0))))
        .map(|(_, t)| match map.route(t) {
            Route::None => 0,
            Route::Road => ROAD_MAINTENANCE,
            Route::Railroad => RAILROAD_MAINTENANCE,
        })
        .sum();
    let modifier = 100 + i64::from(stats.get(Stat::RouteMaintenancePercent));
    let total = total * modifier.max(0) / 100;
    let percent = auth.settings().handicap(rules, player).route_cost_percent;
    (total * i64::from(percent.max(0)) / 100) as i32
}

/// The follower counts of a religion as they were written: `followers` and `majority` of each city apart.
fn religion_counts_apart(auth: &AuthWorld, religion: classic_rules::rules::content::ReligionId) -> FollowerCounts {
    use classic_rules::world::religion::Creed;
    let mut out = FollowerCounts::default();
    let Some(founded) = auth.module().religion().religion(religion) else { return out };
    let creed = Creed::Religion(religion);
    for (handle, core) in auth.cities().iter() {
        let faith = auth.module().religion().city(handle);
        let n = followers(faith, core.population).iter().find(|c| c.0 == creed).map(|c| c.1).unwrap_or(0);
        let foreign = core.owner != founded.founder;
        out.followers += n;
        if foreign {
            out.foreign_followers += n;
        }
        if majority(faith, core.population) == Some(creed) {
            out.cities += 1;
            if foreign {
                out.foreign_cities += 1;
            }
        }
    }
    out
}

#[test]
fn the_fast_answers_equal_the_plain_ones_along_ai_games() {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut routes, mut religions, mut majorities) = (0, 0, 0);
    for seed in [3, 11] {
        let config = ScenarioConfig { seed, players: 4, width: 48, height: 36, cities: 0, units: 0, water_ratio: 20 };
        let mut game = generate_new_game(&rules, &config);
        let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new());
        while game.view().auth.turn().round < 140 && game.winner().is_none() {
            if driver.run(&mut game, &rules, u32::MAX).waiting {
                break;
            }
            if game.view().auth.turn().round % 5 != 0 {
                continue;
            }
            let auth = game.view().auth;
            for p in 0..auth.players().count() {
                let p = PlayerId(p as u8);
                let (fast, plain) = (route_maintenance(auth, &rules, p), route_maintenance_listed(auth, &rules, p));
                assert_eq!(fast, plain, "route maintenance of {p:?} in round {}", auth.turn().round);
                routes += usize::from(plain > 0);
            }
            for founded in auth.module().religion().religions() {
                let (fast, plain) = (compute_religion_counts(auth, founded.religion), religion_counts_apart(auth, founded.religion));
                assert_eq!(fast, plain, "followers of {:?} in round {}", founded.religion, auth.turn().round);
                religions += usize::from(plain.followers > 0);
                majorities += usize::from(plain.cities > 0);
            }
        }
    }
    assert!(routes > 0, "some player paid for roads");
    assert!(religions > 0, "some religion had followers");
    assert!(majorities > 0, "some religion held a city majority");
}

/// Roads and railroads on flat land and on hills, for a nation that pays for hills and for the one
/// (Inca, whose hills are free and whose routes cost half) pays: the case the games rarely reach (a railroad on hills).
#[test]
fn route_maintenance_prices_railroads_and_free_hills_as_the_plain_sum_does() {
    use classic_rules::ids::{TerrainId, TileId};
    use classic_rules::math::HexCoord;
    use classic_rules::rules::roles::Ground;
    use classic_rules::{GameConfig, GameState};
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 });
    let at = |game: &GameState, q, r| game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap();
    let centre = at(&game, 5, 5);
    game.debug_found_city(&rules, PlayerId(0), centre);
    let (flat_road, flat_rail) = (at(&game, 6, 5), at(&game, 4, 5));
    let (hill_road, hill_rail) = (at(&game, 5, 6), at(&game, 5, 4));
    let hills = TerrainId(rules.terrain_kinds.palette().code(Ground::Hills));
    let plains = TerrainId(1);
    game.debug_set_terrain_batch(&rules, &[(flat_road, plains), (flat_rail, plains), (hill_road, hills), (hill_rail, hills)]);
    let nation = |key: &str| rules.nations.iter().position(|n| n.key == key).unwrap() as u8;
    let handicap = i64::from(game.view().auth.settings().handicap(&rules, PlayerId(0)).route_cost_percent);
    let p = PlayerId(0);
    let routes = |game: &mut GameState, list: &[(TileId, Route)]| game.debug_set_route_batch(&rules, list);
    let (r, rr) = (ROAD_MAINTENANCE, RAILROAD_MAINTENANCE);
    assert!(rr > r, "a railroad costs more than a road");
    // (routes laid, the sum a nation that pays for hills owes, the sum an Inca owes), before the handicap.
    let cases: [(Vec<(TileId, Route)>, i64, i64); 4] = [
        (vec![(flat_rail, Route::Railroad)], rr, rr),
        (vec![(hill_rail, Route::Railroad)], rr, 0),
        (vec![(hill_road, Route::Road), (flat_road, Route::Road)], 2 * r, r),
        (vec![(flat_road, Route::Road), (flat_rail, Route::Railroad), (hill_road, Route::Road), (hill_rail, Route::Railroad)], r + rr + r + rr, r + rr),
    ];
    let mut priced = 0;
    for (laid, plain_sum, inca_sum) in cases {
        routes(&mut game, &[(flat_road, Route::None), (flat_rail, Route::None), (hill_road, Route::None), (hill_rail, Route::None)]);
        routes(&mut game, &laid);
        // The Inca pay half of what is left (`production_modifiers.rs`), the sum cut before the handicap.
        for (key, raw) in [("core:nation/rome", plain_sum), ("core:nation/inca", inca_sum * 50 / 100)] {
            game.debug_set_player_nation(p, nation(key));
            let auth = game.view().auth;
            let (fast, plain) = (route_maintenance(auth, &rules, p), route_maintenance_listed(auth, &rules, p));
            assert_eq!(fast, plain, "{key} with {laid:?}");
            assert_eq!(i64::from(fast), raw * handicap / 100, "{key} with {laid:?}: the data's price");
            priced += usize::from(fast > 0);
        }
    }
    assert!(priced >= 6, "the cases pay for routes ({priced})");
}

/// `check_terrain` asks for fresh water only where a rule needs it; it must say what the eager
/// version said (the answer looked up first, always), on every tile of a real map for every
/// improvement, and it must not look where no rule asks.
#[test]
fn check_terrain_looks_up_fresh_water_lazily_and_answers_as_the_eager_one_did() {
    use classic_rules::derived::tile_yields::fresh_water;
    use classic_rules::ids::{ImprovementId, TileId};
    use classic_rules::world::builds::{check_terrain, check_terrain_with};
    use classic_rules::world::facts::TruthFacts;
    use std::cell::Cell;
    let rules = compile_rules(core_rules()).unwrap();
    let (mut asking, mut differing, mut fresh_tiles, mut dry_tiles) = (0, 0, 0, 0);
    for seed in [3u64, 7, 11] {
        let config = ScenarioConfig { seed, players: 3, width: 48, height: 36, cities: 0, units: 0, water_ratio: 20 };
        let game = generate_new_game(&rules, &config);
        let auth = game.view().auth;
        let facts = TruthFacts::new(auth, &rules, PlayerId(0));
        for tile in (0..auth.map().dimensions.tile_count()).map(|t| TileId(t as u32)) {
            let fresh = fresh_water(&facts, &rules, tile);
            fresh_tiles += usize::from(fresh);
            dry_tiles += usize::from(!fresh && !auth.map().is_water(tile));
            for (index, def) in rules.improvements.iter().enumerate() {
                let _ = ImprovementId(index as u16);
                for connects in [false, true] {
                    let eager = check_terrain_with(&facts, &rules, tile, def, connects, || fresh);
                    assert_eq!(check_terrain(&facts, &rules, tile, def, connects), eager, "{} on {tile:?}", def.key);
                    let looked = Cell::new(0);
                    let lazy = check_terrain_with(&facts, &rules, tile, def, connects, || {
                        looked.set(looked.get() + 1);
                        fresh
                    });
                    assert_eq!(lazy, eager);
                    let asks = def.placement.requires_flatlands_or_fresh_water || def.placement.fresh_water_makes_valid;
                    if !asks {
                        assert_eq!(looked.get(), 0, "{} asks no fresh water, yet it was looked up", def.key);
                    } else {
                        asking += 1;
                        // The answer matters for this improvement on some tile.
                        let opposite = check_terrain_with(&facts, &rules, tile, def, connects, || !fresh);
                        differing += usize::from(opposite != eager);
                    }
                }
            }
        }
    }
    assert!(fresh_tiles > 0 && dry_tiles > 0, "the maps have fresh and dry land ({fresh_tiles}, {dry_tiles})");
    assert!(asking > 0, "some improvement asks for fresh water");
    assert!(differing > 0, "fresh water changes some verdict, so the equality is not between constants");
}
