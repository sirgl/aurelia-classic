//! The city-site search keeps its answers while it does less work:
//! `count_sites` is `best_sites(..).len()` cut at a cap, and a context
//! that remembers the scores of land tiles scores every tile as a context
//! that remembers nothing does. Checked at many points of AI games.

use classic_content::core::core_rules;
use classic_rules::ai::planner::PlanTools;
use classic_rules::ai::planner_map::{KnownMap, MapScratch};
use classic_rules::ai::planner_settle::{best_sites, count_sites, SiteContext};
use classic_rules::ai::trace::AiCounters;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::scenario::{generate_new_game, settler_kind, ScenarioConfig};
use classic_rules::{compile_rules, ClassicGame};

#[test]
fn the_capped_count_and_the_tile_memo_change_no_answer() {
    let rules = compile_rules(core_rules()).unwrap();
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut checked_counts = 0;
    let mut checked_scores = 0;
    for seed in [21u64, 22, 23] {
        let config = ScenarioConfig {
            seed,
            players: 4,
            width: 56,
            height: 40,
            cities: 0,
            units: 0,
            water_ratio: 20,
        };
        let mut game = generate_new_game(&rules, &config);
        let mut driver = AiDriver::new(settings.clone(), Vec::new());
        while game.view().auth.turn().round < 60 {
            driver.run(&mut game, &rules, u32::MAX);
            let round = game.view().auth.turn().round;
            if round % 6 != 0 {
                continue;
            }
            for p in 0..4u8 {
                let player = PlayerId(p);
                let obs = game.ai_observation(&rules, player);
                let Some(origin) = obs.own_cities.first().map(|c| c.tile) else {
                    continue;
                };
                let own: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
                let profile = KnownMap::of_kind(&obs.view, settler_kind(&rules)).profile;
                let context = |memo| SiteContext {
                    view: &obs.view,
                    profile,
                    rules: &rules,
                    player,
                    own_cities: &own,
                    foreign_cities: &[],
                    reserved: &[],
                    danger: &[],
                    tile_memo: memo,
                };
                let shared = context(Default::default());
                // The score of every tile around the capital: one context that
                // remembers, against a fresh context per tile.
                let dims = obs.view.dimensions();
                for tile in classic_rules::math::hex_disc(dims, origin, 9) {
                    let alone = context(Default::default()).score(tile);
                    assert_eq!(
                        shared.score(tile),
                        alone,
                        "seed {seed} round {round} player {p} tile {tile:?}"
                    );
                    // Asked twice, the memo answers the second time.
                    assert_eq!(shared.score(tile), alone);
                    checked_scores += 1;
                }
                let moves = 2 * classic_rules::world::unit::MOVE_DENOMINATOR;
                for cap in [1usize, 2, 3, 5] {
                    let limit = settings.budget.candidates;
                    let (mut s1, mut c1) = (MapScratch::default(), AiCounters::default());
                    let mut t1 = PlanTools {
                        rules: &rules,
                        settings: &settings,
                        scratch: &mut s1,
                        counters: &mut c1,
                    };
                    let all =
                        best_sites(&context(Default::default()), origin, moves, &mut t1, limit);
                    let (mut s2, mut c2) = (MapScratch::default(), AiCounters::default());
                    let mut t2 = PlanTools {
                        rules: &rules,
                        settings: &settings,
                        scratch: &mut s2,
                        counters: &mut c2,
                    };
                    let capped = count_sites(&shared, origin, moves, &mut t2, limit, cap);
                    assert_eq!(
                        capped,
                        all.len().min(cap),
                        "seed {seed} round {round} player {p} cap {cap}"
                    );
                    assert!(
                        c2.candidates <= c1.candidates,
                        "the capped search never scores more"
                    );
                    checked_counts += 1;
                }
            }
        }
    }
    assert!(
        checked_scores > 2000 && checked_counts > 40,
        "{checked_scores} scores, {checked_counts} counts"
    );
}
