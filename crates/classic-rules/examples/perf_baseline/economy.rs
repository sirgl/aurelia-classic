//! Cases of the economy and society systems (M4 track A) on the content
//! representative game (`core_rep_game` after warm-up rounds). Wall times
//! are informational; the counters are exact.

use classic_rules::ClassicGame;
use std::hint::black_box;
use std::time::Instant;

use classic_rules::derived::yields::city_breakdown;
use classic_rules::ids::PlayerId;
use classic_rules::metrics::WorkCounters;
use classic_rules::{CompiledRuleset, GameState};

use super::{ms, percentile_us, Case};

/// Every city's citizen assignment and yields (what the end turn and the
/// derived yields evaluate per city).
fn city_breakdowns(out: &mut Vec<Case>, game: &GameState, rules: &CompiledRuleset) {
    let auth = game.view().auth;
    let mut counters = WorkCounters::default();
    let mut samples = Vec::new();
    for _ in 0..20 {
        let t0 = Instant::now();
        let mut c = WorkCounters::default();
        for (_, core) in auth.cities().iter() {
            let b = black_box(city_breakdown(auth, rules, core));
            c.entities_visited += 1;
            c.tiles_visited += b.visited;
        }
        samples.push(ms(t0.elapsed()));
        counters = c;
    }
    out.push(Case {
        name: "economy.city_breakdown.core.rep",
        counters,
        extra: vec![
            ("p50_us", percentile_us(&samples, 50)),
            ("p95_us", percentile_us(&samples, 95)),
        ],
        wall_ms: samples.iter().sum::<f64>() / samples.len() as f64,
        wall_only: false,
    });
}

/// Worker decisions of player 0 with one worker per own city.
fn worker_ai(out: &mut Vec<Case>, game: &mut GameState, rules: &CompiledRuleset) {
    let worker = rules
        .units
        .iter()
        .position(|u| u.key == "core:unit/worker")
        .unwrap();
    let kind = classic_rules::ids::UnitKindId(worker as u16);
    let cities: Vec<_> = game
        .view()
        .auth
        .indexes()
        .cities_of(PlayerId(0))
        .iter()
        .filter_map(|c| game.view().auth.cities().get(*c).map(|core| core.tile))
        .collect();
    for tile in &cities {
        game.debug_spawn_unit(rules, PlayerId(0), *tile, kind);
    }
    let mut samples = Vec::new();
    let mut orders = 0;
    for _ in 0..10 {
        let t0 = Instant::now();
        let obs = game.ai_observation(rules, PlayerId(0));
        orders = black_box(classic_rules::ai::improvements::worker_commands(&obs, rules)).len();
        samples.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "economy.worker_ai.core.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("workers", cities.len() as u64),
            ("orders", orders as u64),
            ("p50_us", percentile_us(&samples, 50)),
            ("p95_us", percentile_us(&samples, 95)),
        ],
        wall_ms: samples.iter().sum::<f64>() / samples.len() as f64,
        wall_only: true,
    });
}

/// Great person points of every city (the GreatPeople city step reads
/// them) and the great person decisions of player 0 with one great
/// scientist per own city.
fn great_people(out: &mut Vec<Case>, game: &mut GameState, rules: &CompiledRuleset) {
    let auth = game.view().auth;
    let mut samples = Vec::new();
    let mut counters = WorkCounters::default();
    for _ in 0..20 {
        let t0 = Instant::now();
        let mut c = WorkCounters::default();
        for (_, core) in auth.cities().iter() {
            let b = city_breakdown(auth, rules, core);
            let player = classic_rules::effects::player::player_modifiers(auth, rules, core.owner).stats.clone();
            black_box(classic_rules::derived::great_people::city_points(rules, &b, &player));
            c.entities_visited += 1;
            c.tiles_visited += b.visited;
        }
        samples.push(ms(t0.elapsed()));
        counters = c;
    }
    out.push(Case {
        name: "economy.great_people.core.rep",
        counters,
        extra: vec![
            ("p50_us", percentile_us(&samples, 50)),
            ("p95_us", percentile_us(&samples, 95)),
        ],
        wall_ms: samples.iter().sum::<f64>() / samples.len() as f64,
        wall_only: false,
    });
    let scientist = rules
        .units
        .iter()
        .position(|u| u.key == "core:unit/great-scientist")
        .unwrap();
    let kind = classic_rules::ids::UnitKindId(scientist as u16);
    let tiles: Vec<_> = game
        .view()
        .auth
        .indexes()
        .cities_of(PlayerId(0))
        .iter()
        .filter_map(|c| game.view().auth.cities().get(*c).map(|core| core.tile))
        .collect();
    for tile in &tiles {
        game.debug_spawn_unit(rules, PlayerId(0), *tile, kind);
    }
    let mut samples = Vec::new();
    let mut orders = 0;
    for _ in 0..10 {
        let t0 = Instant::now();
        let obs = game.ai_observation(rules, PlayerId(0));
        orders = black_box(classic_rules::ai::great_people::great_person_commands(&obs, rules)).len();
        samples.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "economy.great_people_ai.core.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("great_people", tiles.len() as u64),
            ("orders", orders as u64),
            ("p50_us", percentile_us(&samples, 50)),
            ("p95_us", percentile_us(&samples, 95)),
        ],
        wall_ms: samples.iter().sum::<f64>() / samples.len() as f64,
        wall_only: true,
    });
}

/// Happiness of every player (the Happiness turn step and the AI
/// observation compute it once per player turn).
fn happiness(out: &mut Vec<Case>, game: &GameState, rules: &CompiledRuleset) {
    let auth = game.view().auth;
    let players = auth.players().count();
    let mut samples = Vec::new();
    let mut excess = 0i64;
    for _ in 0..20 {
        let t0 = Instant::now();
        excess = 0;
        for p in 0..players {
            let report = black_box(classic_rules::derived::happiness::player_happiness(auth, rules, PlayerId(p as u8)));
            excess += i64::from(report.excess());
        }
        samples.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "economy.happiness.core.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("players", players as u64),
            ("excess_sum", excess.unsigned_abs()),
            ("p50_us", percentile_us(&samples, 50)),
            ("p95_us", percentile_us(&samples, 95)),
        ],
        wall_ms: samples.iter().sum::<f64>() / samples.len() as f64,
        wall_only: true,
    });
}

/// Maintenance and city connections of every player (the Gold turn step
/// and the income shown compute them once per player).
fn upkeep(out: &mut Vec<Case>, game: &GameState, rules: &CompiledRuleset) {
    let auth = game.view().auth;
    let players = auth.players().count();
    let mut samples = Vec::new();
    let mut total = 0i64;
    for _ in 0..20 {
        let t0 = Instant::now();
        total = 0;
        for p in 0..players {
            let u = black_box(classic_rules::derived::maintenance::upkeep(auth, rules, PlayerId(p as u8)));
            total += i64::from(u.net_cost());
        }
        samples.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "economy.upkeep.core.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("players", players as u64),
            ("net_cost", total.unsigned_abs()),
            ("p50_us", percentile_us(&samples, 50)),
            ("p95_us", percentile_us(&samples, 95)),
        ],
        wall_ms: samples.iter().sum::<f64>() / samples.len() as f64,
        wall_only: true,
    });
}

/// Every player's resource ledger (production checks, the AI observation
/// and the UI build one per player).
fn resources(out: &mut Vec<Case>, game: &GameState, rules: &CompiledRuleset) {
    let auth = game.view().auth;
    let players = auth.players().count();
    let mut samples = Vec::new();
    let mut connected = 0u64;
    for _ in 0..20 {
        let t0 = Instant::now();
        connected = 0;
        for p in 0..players {
            let ledger = black_box(classic_rules::derived::resources::resource_ledger(auth, rules, PlayerId(p as u8)));
            connected += ledger.known().map(|r| ledger.total(r).max(0) as u64).sum::<u64>();
        }
        samples.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "economy.resources.core.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("players", players as u64),
            ("connected", connected),
            ("p50_us", percentile_us(&samples, 50)),
            ("p95_us", percentile_us(&samples, 95)),
        ],
        wall_ms: samples.iter().sum::<f64>() / samples.len() as f64,
        wall_only: true,
    });
}

/// Trade route plans (range, path search, circuits) from every city to the
/// next three cities of the store, land and sea: what the trade route
/// screen and the AI compute per trade unit.
fn trade_plans(out: &mut Vec<Case>, game: &GameState, rules: &CompiledRuleset) {
    use classic_rules::world::trade::TradeDomain;
    let auth = game.view().auth;
    let cities: Vec<_> = auth.cities().iter().map(|(h, _)| h).collect();
    let mut samples = Vec::new();
    let mut found = 0u64;
    for _ in 0..5 {
        let t0 = Instant::now();
        found = 0;
        for (i, from) in cities.iter().enumerate() {
            for to in cities.iter().skip(i + 1).take(3) {
                for domain in [TradeDomain::Land, TradeDomain::Sea] {
                    let plan = black_box(classic_rules::derived::trade::plan(auth, rules, *from, *to, domain));
                    found += u64::from(plan.is_ok());
                }
            }
        }
        samples.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "economy.trade_plans.core.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("cities", cities.len() as u64),
            ("routes_found", found),
            ("p50_us", percentile_us(&samples, 50)),
            ("p95_us", percentile_us(&samples, 95)),
        ],
        wall_ms: samples.iter().sum::<f64>() / samples.len() as f64,
        wall_only: true,
    });
}

pub fn cases(out: &mut Vec<Case>, game: &mut GameState, rules: &CompiledRuleset) {
    trade_plans(out, game, rules);
    city_breakdowns(out, game, rules);
    happiness(out, game, rules);
    upkeep(out, game, rules);
    resources(out, game, rules);
    great_people(out, &mut game.clone(), rules);
    worker_ai(out, game, rules);
}
