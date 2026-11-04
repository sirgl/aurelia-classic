//! Census of the expansion-only systems (trade routes, tourism, great works, ideologies, the
//! World Congress) in a headless AI game: prints, every `step` rounds, what the systems produce so
//! the own formulas can be checked against the economy around them. Calibration tool, not a test.
//!
//! `cargo run --release -p classic-rules --example expansion_census -- [rounds] [seed] [players] [step] [width] [height]`

use classic_content::core::core_rules;
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::derived::{culture, income, tourism};
use classic_rules::ids::PlayerId;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::compile_rules;

fn arg(index: usize, default: u64) -> u64 {
    std::env::args().nth(index).and_then(|a| a.parse().ok()).unwrap_or(default)
}

fn main() {
    let rounds = arg(1, 300) as u32;
    let step = arg(4, 50) as u32;
    let config = ScenarioConfig {
        seed: arg(2, 0x5EED_0B5E),
        players: arg(3, 6) as u8,
        width: arg(5, 64) as u32,
        height: arg(6, 48) as u32,
        cities: 0,
        units: 0,
        water_ratio: 20,
    };
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = generate_new_game(&rules, &config);
    let mut driver = AiDriver::new(AiSettings::for_rules(&rules, rules.default_difficulty()), Vec::new());
    let mut next = step;
    let mut most_routes = 0usize;
    let mut route_rounds = 0u32;
    let mut most_units = 0usize;
    let mut most_slots = 0u32;
    while game.view().auth.turn().round < rounds && game.winner().is_none() {
        let report = driver.run(&mut game, &rules, u32::MAX);
        if report.waiting {
            break;
        }
        let round = game.view().auth.turn().round;
        let round_dbg = round;
        let routes_now = game.view().auth.module().trade().routes().len();
        most_routes = most_routes.max(routes_now);
        route_rounds += u32::from(routes_now > 0);
        {
            let auth = game.view().auth;
            for p in 0..auth.players().count() {
                let player = PlayerId(p as u8);
                if !auth.players().is_major(player) {
                    continue;
                }
                let units = auth
                    .indexes()
                    .units_of(player)
                    .iter()
                    .filter_map(|u| auth.units().get(*u))
                    .filter(|v| classic_rules::derived::trade::is_trade_unit(&rules, v.core.kind))
                    .count();
                most_units = most_units.max(units);
                if units > 0 && round_dbg % 10 == 0 {
                    for u in auth.indexes().units_of(player) {
                        let Some(v) = auth.units().get(*u) else { continue };
                        if !classic_rules::derived::trade::is_trade_unit(&rules, v.core.kind) {
                            continue;
                        }
                        let mut errs = std::collections::BTreeMap::new();
                        for (c, _) in auth.cities().iter() {
                            for kind in classic_rules::world::trade::RouteKind::ALL {
                                match classic_rules::derived::trade::route_status(auth, &rules, *u, c, kind) {
                                    Ok(_) => *errs.entry("ok".to_string()).or_insert(0) += 1,
                                    Err(e) => *errs.entry(format!("{e:?}")).or_insert(0) += 1,
                                }
                            }
                        }
                        eprintln!("round {round_dbg} P{p} unit {:?} errs {errs:?}", v.core.tile);
                    }
                }
                most_slots = most_slots.max(classic_rules::derived::trade::routes_possible(auth, &rules, player));
            }
        }
        if round >= next {
            next += step;
            print_round(&game, &rules, round);
        }
    }
    println!("routes: most at once {most_routes}, rounds with a route {route_rounds}, most trade units alive {most_units}, most route slots {most_slots}");
    println!("end: round={} winner={:?}", game.view().auth.turn().round, game.winner());
}

fn print_round(game: &classic_rules::GameState, rules: &classic_rules::CompiledRuleset, round: u32) {
    let auth = game.view().auth;
    println!("== round {round}");
    for p in 0..auth.players().count() {
        let player = PlayerId(p as u8);
        if !auth.players().is_major(player) || !auth.players().is_alive(player) {
            continue;
        }
        let cities = auth.indexes().cities_of(player);
        let pop: u32 = cities.iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.population).sum();
        let inc = income::player_income(auth, rules, player);
        let culture_turn = culture::culture_per_turn(auth, rules, player);
        let touri = tourism::player_tourism(auth, rules, player);
        let works: usize = cities.iter().map(|c| auth.module().tourism().city(*c).works.len()).sum();
        let routes: Vec<_> = auth.module().trade().owned_by(player).collect();
        let (mut og, mut os, mut ef, mut ep, mut intl) = (0u32, 0u32, 0u32, 0u32, 0u32);
        for r in &routes {
            og += r.origin_yields.gold;
            os += r.origin_yields.science;
            ef += r.destination_yields.food;
            ep += r.destination_yields.production;
            if matches!(r.kind, classic_rules::world::trade::RouteKind::International) {
                intl += 1;
            }
        }
        let levels: Vec<String> = (0..auth.players().count())
            .map(|q| PlayerId(q as u8))
            .filter(|q| *q != player && auth.players().is_major(*q))
            .map(|q| {
                tourism::influence_level(auth, player, q)
                    .map(|l| l.index().to_string())
                    .unwrap_or_else(|| "-".into())
            })
            .collect();
        let state = auth.module().tourism().player(player);
        let era = classic_rules::world::player::player_era(auth, rules, player);
        for c in cities {
            if let Some(core) = auth.cities().get(*c) {
                let ci = income::city_income(auth, rules, core);
                println!(
                    "CITY era={era} pop={} gold={} sci={} prod={} food={} culture={}",
                    core.population, ci.gold, ci.science, ci.production, ci.food, ci.culture
                );
            }
        }
        println!(
            "P{p} era={} cities={} pop={pop} techs={} gold/t={} sci/t={} culture/t={culture_turn} lifetime={} tourism={touri} works={works} routes={} (intl {intl}) route_gold={:.1} route_sci={:.1} food={:.1} prod={:.1} levels=[{}] opinion={:?}/{} ideology={:?}",
            classic_rules::world::player::player_era(auth, rules, player),
            cities.len(),
            auth.players().techs_of(player).len(),
            inc.gold,
            inc.science,
            auth.module().culture().of(player).lifetime,
            routes.len(),
            f64::from(og) / 100.0,
            f64::from(os) / 100.0,
            f64::from(ef) / 100.0,
            f64::from(ep) / 100.0,
            levels.join(","),
            state.opinion,
            state.dissatisfaction,
            auth.module().culture().of(player).ideology,
        );
    }
    let c = auth.module().congress();
    if c.founded() {
        println!(
            "congress: host={:?} special={} un={} next_in={} active={} projects={} completed={}",
            c.host(),
            c.special_session(),
            c.united_nations(),
            c.turns_until_session(),
            c.active().len(),
            c.projects().len(),
            c.completed().len()
        );
    }
}
