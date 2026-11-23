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
    let (mut routes, mut religions) = (0, 0);
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
            }
        }
    }
    assert!(routes > 0, "some player paid for roads");
    assert!(religions > 0, "some religion had followers");
}
