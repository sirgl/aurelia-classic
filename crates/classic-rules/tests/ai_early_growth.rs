//! `early-growth` (work/nets/early-growth): what the executors get from `net2`'s early-growth levers in real games and
//! observations. The workers' jobs ahead count a city's open tiles up to its citizens and two more while a city lacks
//! its worker (`weights::WORKER_SHORT`, `JOBS_CAP`); the site head's shared land (`weights::site::SHARED`) lowers the
//! sites that share tiles with an own city and moves the best site away from them. Each test fails with its weights at
//! 0. The tests name classic content only to set the scene.

#![cfg(feature = "experimental-ai")]

use classic_content::core::core_rules;
use classic_rules::ai::driver::AiDriver as ClassicDriver;
use classic_rules::ai::net2::growth::jobs_ahead;
use classic_rules::ai::net2::plan::Growth;
use classic_rules::ai::net2::{weights, Net2Plan};
use classic_rules::ai::planner::{PlanTools, TurnBrief};
use classic_rules::ai::planner_map::{KnownMap, MapScratch};
use classic_rules::ai::planner_settle::{best_sites_by, HeadRead, Site, SiteContext, SiteQuality};
use classic_rules::ai::site::SiteInputs;
use classic_rules::ai::state::AiState;
use classic_rules::ai::trace::AiCounters;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{PlayerId, TechId, TerrainId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::scenario::{generate_new_game, settler_kind, ScenarioConfig};
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// One city of `population` on grassland, the ancient technologies known; a rival far away.
fn grassland_city(population: u32) -> (GameState, CompiledRuleset, TileId) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 30, rng_seed: 5 });
    let ancient = rules.eras.iter().find(|e| e.key == "core:era/ancient").unwrap().index;
    for (i, tech) in rules.technologies.iter().enumerate() {
        if tech.content_era.and_then(|e| rules.eras.get(e.0 as usize)).map_or(0, |e| e.index) <= ancient {
            game.debug_learn(&rules, P0, TechId(i as u16));
        }
    }
    let grass = TerrainId(rules.terrains.iter().position(|t| t.key == "core:terrain/grassland").unwrap() as u16);
    let centre = tile(&game, 10, 10);
    let dims = game.view().auth.map().dimensions;
    let land: Vec<(TileId, TerrainId)> = classic_rules::math::hex_disc(dims, centre, 4).map(|t| (t, grass)).collect();
    game.debug_set_terrain_batch(&rules, &land);
    let city = game.debug_found_city(&rules, P0, centre);
    game.debug_found_city(&rules, P1, tile(&game, 30, 20));
    game.debug_set_population(&rules, city, population);
    (game, rules, centre)
}

fn jobs(game: &GameState, rules: &CompiledRuleset) -> u32 {
    let obs = game.ai_observation(rules, P0);
    let brief = TurnBrief::from_observation(&obs, rules);
    let state = AiState::new(P0, game.view().auth.turn().round);
    jobs_ahead(&obs, &brief, &state, rules)
}

/// A city of 1 without a worker counts the jobs of its citizen's tile and the next two (3 of its 6 owned tiles); a
/// city of 8 counts all six it owns; with a worker for the one city there are no jobs ahead (the allocator's own
/// count decides).
#[test]
fn the_jobs_ahead_count_two_citizens_more_until_a_worker_a_city() {
    let (game, rules, _) = grassland_city(1);
    assert_eq!(jobs(&game, &rules), 3, "the citizen's tile and two more");
    let (game, rules, _) = grassland_city(8);
    assert_eq!(jobs(&game, &rules), 6, "every owned tile below the cap of 10");
    let (mut game, rules, centre) = grassland_city(1);
    let worker = classic_rules::ids::UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/worker").unwrap() as u16);
    game.debug_spawn_unit(&rules, P0, centre, worker);
    assert_eq!(jobs(&game, &rules), 0, "a worker a city");
}

/// The nearest own city of a tile, in tiles.
fn nearest_own(game: &GameState, own: &[TileId], at: TileId) -> i32 {
    let dims = game.view().auth.map().dimensions;
    own.iter().filter_map(|c| dims.distance(*c, at)).min().unwrap_or(i32::MAX)
}

fn best(sites: &[Site]) -> Option<&Site> {
    sites.iter().max_by_key(|s| (s.value, std::cmp::Reverse(s.tile)))
}

/// On the four-seat scenario every 7 rounds to r70: with the shared land's weight the site head never raises a site,
/// lowers the ones sharing tiles with an own city (within 6 of one), leaves the others, and the best site moves away
/// from the own cities more often than toward them.
#[test]
fn the_shared_land_moves_the_best_site_away_from_the_own_cities() {
    let rules = compile_rules(core_rules()).unwrap();
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let silent = Net2Plan::default();
    let shared = Net2Plan { growth: Growth { shared: weights::site::SHARED.0, ..Growth::default() }, ..Net2Plan::default() };
    let mut game = generate_new_game(&rules, &ScenarioConfig { seed: 0xA1_5EED, players: 4, width: 40, height: 32, cities: 0, units: 0, water_ratio: 15 });
    let mut driver = ClassicDriver::new(settings.clone(), Vec::new());
    let (mut lowered, mut farther, mut closer) = (0, 0, 0);
    while game.view().auth.turn().round < 70 {
        driver.run(&mut game, &rules, u32::MAX);
        let round = game.view().auth.turn().round;
        if round % 7 != 0 {
            continue;
        }
        for p in 0..4u8 {
            let player = PlayerId(p);
            let obs = game.ai_observation(&rules, player);
            let Some(origin) = obs.own_cities.first().map(|c| c.tile) else { continue };
            let own: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
            let others: Vec<(TileId, PlayerId)> = (0..4u8).filter(|q| *q != p).flat_map(|q| game.ai_observation(&rules, PlayerId(q)).own_cities.iter().map(move |c| (c.tile, PlayerId(q))).collect::<Vec<_>>()).collect();
            let foreign: Vec<TileId> = others.iter().map(|(t, _)| *t).collect();
            let context = SiteContext {
                view: &obs.view,
                profile: KnownMap::of_kind(&obs.view, settler_kind(&rules)).profile,
                rules: &rules,
                player,
                own_cities: &own,
                foreign_cities: &foreign,
                reserved: &[],
                danger: &[],
                tile_memo: Default::default(),
            };
            let inputs = SiteInputs { nation: 0, techs: &[], own_cities: &own, foreign_cities: &foreign, round, excess_happiness: 0 };
            let search = |plan: &Net2Plan| {
                let (mut scratch, mut counters) = (MapScratch::default(), AiCounters::default());
                let mut tools = PlanTools { rules: &rules, settings: &settings, scratch: &mut scratch, counters: &mut counters };
                best_sites_by(&context, origin, 2 * classic_rules::world::unit::MOVE_DENOMINATOR, &mut tools, settings.budget.candidates, &SiteQuality::Head(inputs, HeadRead { plan, rivals: &others }))
            };
            let (plain, spaced) = (search(&silent), search(&shared));
            for site in &spaced {
                let Some(base) = plain.iter().find(|s| s.tile == site.tile) else { continue };
                let near = nearest_own(&game, &own, site.tile) <= 6;
                assert!(site.score <= base.score, "round {round} player {p}: the shared land never raises a site");
                assert!(near || site.score == base.score, "round {round} player {p}: a site sharing no tile keeps its score");
                lowered += usize::from(site.score < base.score);
            }
            if let (Some(a), Some(b)) = (best(&plain), best(&spaced)) {
                let (da, db) = (nearest_own(&game, &own, a.tile), nearest_own(&game, &own, b.tile));
                farther += usize::from(db > da);
                closer += usize::from(db < da);
            }
        }
    }
    assert!(lowered > 0, "no site sharing land seen");
    assert!(farther > closer, "the best site moved away {farther} times, closer {closer}");
}
