//! Balanced starts (`kernel::scenario::start_balance`): the same seed gives the same map, every start
//! reaches the minimum food and production, the early strategic resources lie near every start, a top-up
//! stays inside its bounds, and the balance off is the generator as it was.
//!
//! The numbers behind the policy are in `classic/docs/start-balance.md`; the census that measured them is
//! `examples/start_census.rs`.

use classic_content::core::core_rules;
use classic_rules::derived::tile_yields::tile_yields_for;
use classic_rules::ids::{PlayerId, ResourceId, TileId};
use classic_rules::math::hex_disc;
use classic_rules::rules::content::ResourceClass;
use classic_rules::scenario::{
    generate_new_game_report, BalanceReport, LandOptions, MapScript, NewGameOptions, ScenarioConfig, StartBalance, StartPolicy,
};
use classic_rules::world::size::WorldSize;
use classic_rules::{compile_rules, CompiledRuleset, GameState};

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

/// Nations and city-states of a size, as the setup screen has them.
fn players(size: WorldSize) -> (u8, u8) {
    match size {
        WorldSize::Duel => (2, 4),
        WorldSize::Tiny => (4, 8),
        WorldSize::Small => (6, 12),
        _ => (8, 16),
    }
}

fn game(rules: &CompiledRuleset, script: MapScript, size: WorldSize, seed: u64, balance: StartBalance) -> (GameState, BalanceReport) {
    let (width, height) = size.grid();
    let (nations, minors) = players(size);
    let cfg = ScenarioConfig { seed, players: nations, width, height, cities: 0, units: 0, water_ratio: script.water_ratio(25) };
    let options = NewGameOptions { city_states: minors, script, start_balance: balance, ..NewGameOptions::default() };
    generate_new_game_report(rules, &cfg, None, options, LandOptions::CLASSIC)
}

/// The start tile of each nation: where its settler stands.
fn starts(game: &GameState, nations: u8) -> Vec<TileId> {
    let auth = game.view().auth;
    (0..nations)
        .map(|p| {
            let unit = auth.indexes().units_of(PlayerId(p)).first().copied().expect("a nation starts with a unit");
            auth.units().get(unit).expect("the unit lives").core.tile
        })
        .collect()
}

/// Food and production of the tiles within two tiles of a start, the start tile left out (the census' sums).
fn food_and_production(game: &GameState, rules: &CompiledRuleset, player: u8, start: TileId) -> (i32, i32) {
    let auth = game.view().auth;
    let dims = auth.map().dimensions;
    let (mut food, mut production) = (0, 0);
    for tile in hex_disc(dims, start, 2).filter(|t| *t != start) {
        let y = tile_yields_for(auth, rules, tile, PlayerId(player));
        food += y.food;
        production += y.production;
    }
    (food, production)
}

/// Everything about the map the balance may touch: terrain codes and deposits with their amounts, tile order.
fn fingerprint(game: &GameState) -> Vec<(u32, Option<u16>, Option<u16>, u8)> {
    let map = game.view().auth.map();
    (0..map.dimensions.tile_count() as u32)
        .map(|i| {
            let t = TileId(i);
            let resource = map.resource(t).flatten().map(|r| r.0);
            (i, map.terrain(t).map(|c| c.0), resource, map.resource_amount(t))
        })
        .collect()
}

/// The strategic resources a start is promised, by the policy and the rules.
fn promised(rules: &CompiledRuleset, policy: &StartPolicy) -> Vec<usize> {
    (0..rules.resources.len())
        .filter(|&k| {
            let def = &rules.resources[k];
            def.class == Some(ResourceClass::Strategic)
                && def.required_tech.is_none_or(|tech| {
                    rules
                        .technologies
                        .get(tech.0 as usize)
                        .and_then(|t| t.content_era)
                        .and_then(|era| rules.eras.get(era.0 as usize))
                        .is_some_and(|era| era.index <= policy.strategic_up_to_era)
                })
        })
        .collect()
}

#[test]
fn the_promised_resources_of_classic_are_horses_and_iron() {
    let rules = rules();
    let keys: Vec<&str> = promised(&rules, &StartPolicy::CLASSIC).into_iter().map(|k| rules.resources[k].key.as_str()).collect();
    assert_eq!(keys, ["core:resource/horses", "core:resource/iron"]);
}

#[test]
fn the_same_seed_gives_the_same_map() {
    let rules = rules();
    for (script, size, seed) in [
        (MapScript::Continents, WorldSize::Small, 3),
        (MapScript::Fractal, WorldSize::Standard, 11),
        (MapScript::Archipelago, WorldSize::Tiny, 5),
    ] {
        let (a, report_a) = game(&rules, script, size, seed, StartBalance::ON);
        let (b, report_b) = game(&rules, script, size, seed, StartBalance::ON);
        assert_eq!(fingerprint(&a), fingerprint(&b), "{script:?} {size:?} {seed}");
        assert_eq!(report_a, report_b);
        assert_eq!(starts(&a, players(size).0), starts(&b, players(size).0));
    }
}

#[test]
fn every_start_reaches_the_minimum_food_and_production() {
    let rules = rules();
    let policy = StartPolicy::CLASSIC;
    let mut checked = 0;
    for script in [MapScript::Continents, MapScript::Fractal, MapScript::Pangaea] {
        for size in [WorldSize::Duel, WorldSize::Small, WorldSize::Standard] {
            for seed in 1..=25 {
                let (g, _) = game(&rules, script, size, seed, StartBalance::ON);
                for (p, start) in starts(&g, players(size).0).into_iter().enumerate() {
                    let (food, production) = food_and_production(&g, &rules, p as u8, start);
                    assert!(food >= policy.min_food, "{script:?} {size:?} seed {seed} player {p}: food {food}");
                    assert!(production >= policy.min_production, "{script:?} {size:?} seed {seed} player {p}: production {production}");
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 600, "{checked} starts checked");
}

#[test]
fn every_start_has_horses_and_iron_within_three_tiles_and_not_under_it() {
    let rules = rules();
    let kinds = promised(&rules, &StartPolicy::CLASSIC);
    let (mut starts_seen, mut missing) = (0, 0);
    for script in [MapScript::Continents, MapScript::Fractal, MapScript::Pangaea, MapScript::SmallContinents] {
        for size in [WorldSize::Duel, WorldSize::Small, WorldSize::Standard] {
            for seed in 1..=20 {
                let (g, report) = game(&rules, script, size, seed, StartBalance::ON);
                let map = g.view().auth.map();
                let dims = map.dimensions;
                let mut lacking = 0;
                for start in starts(&g, players(size).0) {
                    for &kind in &kinds {
                        let near = hex_disc(dims, start, 3)
                            .filter(|t| *t != start)
                            .any(|t| map.resource(t).flatten() == Some(ResourceId(kind as u16)));
                        starts_seen += 1;
                        if !near {
                            lacking += 1;
                        }
                    }
                }
                assert_eq!(lacking, report.strategic_missing as usize, "{script:?} {size:?} seed {seed}: the report counts the misses");
                missing += lacking;
            }
        }
    }
    // A start with no free tile near it at all may miss one (the report says so); on these maps none does.
    assert_eq!(missing, 0, "{missing} of {starts_seen} promised deposits missing");
}

#[test]
fn a_guaranteed_deposit_is_small_and_fixed() {
    let rules = rules();
    let (on, report) = game(&rules, MapScript::Continents, WorldSize::Standard, 4, StartBalance::ON);
    let (off, none) = game(&rules, MapScript::Continents, WorldSize::Standard, 4, StartBalance { strategic: false, ..StartBalance::ON });
    assert!(report.strategic_added > 0);
    assert_eq!(none.strategic_added, 0, "the switch off puts no strategic deposit");
    // The tiles that hold a strategic deposit in `on` and not in `off` are the guaranteed ones: the smallest quantity.
    let (map_on, map_off) = (on.view().auth.map(), off.view().auth.map());
    let mut found = 0;
    for i in 0..map_on.dimensions.tile_count() as u32 {
        let t = TileId(i);
        let (a, b) = (map_on.resource(t).flatten(), map_off.resource(t).flatten());
        if a != b {
            let def = &rules.resources[a.expect("a deposit was added").0 as usize];
            assert_eq!(def.class, Some(ResourceClass::Strategic));
            assert_eq!(map_on.resource_amount(t), *def.quantities.iter().min().unwrap());
            found += 1;
        }
    }
    assert!(found > 0 && found as u32 <= report.strategic_added, "{found} added tiles");
}

#[test]
fn a_raised_minimum_is_met_by_bounded_edits_and_the_world_stays_valid() {
    let rules = rules();
    // The bar is above what the land gives, so the top-up has to work.
    let policy = StartPolicy { min_food: 30, min_production: 16, even_percent: 100, edit_budget: 3, ..StartPolicy::CLASSIC };
    let balance = StartBalance { policy, ..StartBalance::ON };
    let (mut edited_games, mut edits) = (0, 0);
    for seed in 1..=12 {
        let (g, report) = game(&rules, MapScript::Fractal, WorldSize::Small, seed, balance);
        g.validate(&rules).unwrap();
        let nations = u32::from(players(WorldSize::Small).0);
        // The budget of the top-up, and at most one turned tile for each promised deposit.
        let touched = report.bonus_added + report.terrain_turned;
        let promised = promised(&rules, &policy).len() as u32;
        assert!(touched <= (u32::from(policy.edit_budget) + promised) * nations, "seed {seed}: {touched} edits for {nations} starts");
        edited_games += u32::from(touched > 0);
        edits += touched;
        // The top-up is deterministic too.
        let (again, again_report) = game(&rules, MapScript::Fractal, WorldSize::Small, seed, balance);
        assert_eq!(fingerprint(&g), fingerprint(&again));
        assert_eq!(report, again_report);
    }
    assert!(edited_games > 0 && edits > 0, "a bar above the land must cause edits");
}

#[test]
fn the_balance_off_keeps_the_old_starts() {
    // Standard map, continents, seed 1: the eight starts the generator chose before the balance (the census' BEFORE rows).
    let rules = rules();
    let (g, report) = game(&rules, MapScript::Continents, WorldSize::Standard, 1, StartBalance::OFF);
    assert_eq!(report, BalanceReport::default());
    let dims = g.view().auth.map().dimensions;
    let coords: Vec<(i32, i32)> = starts(&g, 8)
        .into_iter()
        .map(|t| {
            let c = dims.to_coord(t).unwrap();
            (c.q, c.r)
        })
        .collect();
    assert_eq!(coords, [(31, 16), (68, 39), (68, 13), (8, 19), (51, 9), (19, 42), (72, 24), (45, 22)]);
}

#[test]
fn the_starts_of_a_game_are_closer_in_value_than_before() {
    // The value of a start as the census measures it: food + production within two tiles. Over 30 games of two
    // sizes the balance halves the median spread (the census over 1,600 games: 12 before, 6 after).
    let rules = rules();
    let spread = |balance: StartBalance| -> Vec<i32> {
        let mut out = Vec::new();
        for size in [WorldSize::Small, WorldSize::Standard] {
            for seed in 1..=15 {
                let (g, _) = game(&rules, MapScript::Continents, size, seed, balance);
                let values: Vec<i32> = starts(&g, players(size).0)
                    .into_iter()
                    .enumerate()
                    .map(|(p, s)| {
                        let (f, pr) = food_and_production(&g, &rules, p as u8, s);
                        f + pr
                    })
                    .collect();
                out.push(values.iter().max().unwrap() - values.iter().min().unwrap());
            }
        }
        out.sort_unstable();
        out
    };
    let (before, after) = (spread(StartBalance::OFF), spread(StartBalance::ON));
    let (median_before, median_after) = (before[before.len() / 2], after[after.len() / 2]);
    assert!(median_after * 3 <= median_before * 2, "median spread {median_before} -> {median_after}");
    assert!(after[after.len() - 1] < before[before.len() - 1], "the widest game gets narrower: {before:?} -> {after:?}");
}
