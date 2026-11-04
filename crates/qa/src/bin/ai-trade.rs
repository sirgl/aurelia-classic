//! The trade routes of AI versions in head-to-head games (classic/docs/ai-trade.md):
//! per side, when the route slots open (the first technology with a route
//! slot), when the first route starts, the slots open against the routes
//! running round by round, the routes by kind (internal food, internal
//! production, international to a nation or a city-state), what they deliver
//! (food and production to the destination of an internal route, gold and
//! science to the origin of an international one), the routes lost before
//! their end (plundered, cut by a war, a city lost), and why a slot stays
//! empty; then the routes, citizens, potential and score at a third, half,
//! two thirds and the end of the game, per side with a t interval over seed
//! blocks and the paired difference.
//!
//! `cargo run --release -p aurelia-qa --bin ai-trade -- [options]`
//!
//! --candidate X    the candidate (default standard)
//! --baseline Y     the other side (default standard); equal sides: solo
//! --suite NAME     standard|duel|smoke (default standard)
//! --speed NAME     the game speed (default quick)
//! --offset K       the first seed offset (default 0)
//! --repeat R       offsets K … K+R−1 (default 1)
//! --games N        the first N games of each offset (default all)
//! --jobs J         games in parallel (default 4)
//!
//! Why a slot stays empty, per seat-round with fewer routes running than
//! the slots at the start of the seat's turn (what its AI sees; each
//! seat-round counts once, the first that holds):
//! `idle-offers`: a trade unit stands in a city with a route to start (the
//! turn after a route ends, its unit is back); `idle-offers-hazard`: its
//! routes all have a hazard near the way (`trade::Hazards`); `idle-stuck`: a
//! trade unit stands with no route it may start (it holds the slot);
//! `queued`: a trade unit is in production; `offered`: a city chose its order
//! this turn with a trade unit among the allocator's candidates and took
//! another (`allocator::probe`); `chose-no-candidate`: a city that chose
//! could start a route by the rules (`derived::trade::prospects`) and had no
//! trade unit among its candidates; `busy-routes-*`: only cities that did not
//! choose could: `-no-value` when `trade-fit` prices a trade unit at nothing
//! in each (`trade::fit_prospect`: no destination left worth something),
//! `-head-building` when one of the priced cities builds a building (not a
//! wonder) first, `-head-unit-or-wonder` otherwise; `busy-no-route-*`: no
//! city could start a route: `-one-city`, `-no-origin` (no city has the
//! building internal routes need), `-other` (no foreign city in range).
//! Every route's end is counted by whether the AI's hazard test held when it
//! started, with the routes lost (plundered, cut by a war, a city lost).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use aurelia_qa::bench::modules::{majors, BenchRules, ClassicBench};
use aurelia_qa::bench::suite::{Match, Mode, Side, Suite};
use classic_rules::ai::variant::Variant;
use classic_rules::derived::trade::{is_trade_unit, prospects, routes_possible, trade_offers};
use classic_rules::ids::{PlayerId, ProductionItem, UnitKindId};
use classic_rules::world::trade::RouteKind;
use classic_rules::world::player::ClassicPlayers;
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::seats::Seated;
use kernel::ai::Advance;

struct Options {
    candidate: String,
    baseline: String,
    suite: String,
    speed: String,
    offset: u64,
    repeat: u64,
    games: Option<usize>,
    jobs: usize,
}

fn options() -> Result<Options, String> {
    let mut o = Options { candidate: "standard".into(), baseline: "standard".into(), suite: "standard".into(), speed: "quick".into(), offset: 0, repeat: 1, games: None, jobs: 4 };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--candidate" => o.candidate = value()?,
            "--baseline" => o.baseline = value()?,
            "--suite" => o.suite = value()?,
            "--speed" => o.speed = value()?,
            "--offset" => o.offset = value()?.parse().map_err(|e| format!("--offset: {e}"))?,
            "--repeat" => o.repeat = value()?.parse().map_err(|e| format!("--repeat: {e}"))?,
            "--games" => o.games = Some(value()?.parse().map_err(|e| format!("--games: {e}"))?),
            "--jobs" => o.jobs = value()?.parse().map_err(|e| format!("--jobs: {e}"))?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(o)
}

/// Why a seat's slot stays empty in a round (see the module's text).
const WHY: [&str; 12] = ["idle-offers", "idle-offers-hazard", "idle-stuck", "queued", "offered", "chose-no-candidate", "busy-routes-no-value", "busy-routes-head-unit-or-wonder", "busy-routes-head-building", "busy-no-route-one-city", "busy-no-route-no-origin", "busy-no-route-other"];

/// A seat at a checkpoint.
#[derive(Debug, Clone, Copy, Default)]
struct At {
    alive: bool,
    slots: u32,
    routes: u32,
    food: u32,
    production: u32,
    international: u32,
    city_states: u32,
    citizens: u32,
    cities: u32,
    potential: f64,
    score: i64,
}

/// One seat of one game.
#[derive(Debug, Clone, Default)]
struct Seat {
    side: Option<Side>,
    /// The first round with a route slot, the first round with a route.
    slot_round: Option<u32>,
    route_round: Option<u32>,
    /// Seat-rounds with a slot open; routes running summed over them.
    slot_rounds: u64,
    slot_sum: u64,
    route_sum: u64,
    /// Hundredths delivered over the game: food and production to own
    /// cities, gold and science to the origin.
    food: u64,
    production: u64,
    gold: u64,
    science: u64,
    started: u32,
    lost: u32,
    /// Routes ended (finished or lost) by whether the AI's hazard test
    /// (`trade::Hazards::near`) held when they started: [ended, lost] for
    /// no hazard, then for a hazard.
    by_hazard: [[u32; 2]; 2],
    /// Routes started by kind: internal food, internal production, international.
    started_kind: [u32; 3],
    why: [u64; WHY.len()],
    at: Vec<At>,
}

#[derive(Debug, Clone)]
struct GameResult {
    seed: u64,
    seats: Vec<Seat>,
}

/// A running route as last seen: its progress and length, and whether a
/// hazard lay near its way when it started.
#[derive(Debug, Clone, Copy)]
struct Seen {
    remaining: u32,
    speed: u32,
    hazard: bool,
}

/// A seat at the start of its turn (what its AI sees), for `why_empty`.
#[derive(Debug, Clone, Default)]
struct Pending {
    empty: bool,
    idle_offers: bool,
    idle_hazard: bool,
    idle_stuck: bool,
    queued: bool,
    /// Cities that could start a route by the rules (`prospects`).
    with_routes: Vec<kernel::ids::EntityId>,
    /// Some city prices a trade unit above nothing (`trade::fit_prospect`).
    valued: bool,
    building_head: bool,
    cities: usize,
    origin: bool,
}

fn main() {
    let options = match options() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("ai-trade: {e}");
            std::process::exit(1);
        }
    };
    let rules = ClassicBench::rules();
    let suite = Suite::named(&options.suite).unwrap_or_else(|| panic!("unknown suite {}", options.suite));
    let def = rules.speed_named(&options.speed).unwrap_or_else(|| panic!("unknown speed {}", options.speed));
    let suite = suite.at_speed(&def.key, &def.values);
    let solo = options.candidate == options.baseline;
    let mode = if solo { Mode::Solo } else { Mode::Versus };
    let mut games: Vec<Match> = Vec::new();
    for k in options.offset..options.offset + options.repeat {
        let mut m = suite.matches(mode, k);
        if let Some(n) = options.games {
            m.truncate(n);
        }
        games.extend(m);
    }
    let checkpoints: Vec<u32> = [3u32, 2, 3, 1].iter().zip([1u32, 1, 2, 1]).map(|(d, n)| suite.rounds * n / d).collect();
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<GameResult>>> = Mutex::new(vec![None; games.len()]);
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.clamp(1, games.len().max(1)) {
            scope.spawn(|| {
                let rules = ClassicBench::rules();
                TRADE_KINDS.with(|k| *k.borrow_mut() = (0..rules.units.len()).map(|i| UnitKindId(i as u16)).filter(|u| is_trade_unit(&rules, *u)).collect());
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(game) = games.get(at) else { break };
                    let result = play(&rules, &suite, game, &options, &checkpoints);
                    eprintln!("ai-trade: {} done ({}/{})", game.label(), at + 1, games.len());
                    results.lock().expect("no thread panicked")[at] = Some(result);
                }
            });
        }
    });
    let results: Vec<GameResult> = results.into_inner().expect("no thread panicked").into_iter().map(|r| r.expect("every game played")).collect();
    print!("{}", report(&options, &suite, &checkpoints, &results, solo));
}

/// The trade unit kinds a player knows (the rules' trade units whose
/// technology it has).
fn known_trade_kinds(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Vec<UnitKindId> {
    let techs = game.view().auth.players().techs_of(player);
    (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .filter(|k| is_trade_unit(rules, *k) && rules.units[k.0 as usize].required_tech.is_none_or(|t| techs.contains(&t)))
        .collect()
}

fn play(rules: &CompiledRuleset, suite: &Suite, game_match: &Match, options: &Options, checkpoints: &[u32]) -> GameResult {
    let mut game = ClassicBench::new_game_at(rules, &game_match.map, game_match.seed, suite.rounds, &suite.speed);
    let seats: Vec<(PlayerId, Side)> = majors(&game).into_iter().enumerate().map(|(i, p)| (p, game_match.seating.side(i))).collect();
    let mind = |name: &str| ClassicBench::mind(name).unwrap_or_else(|| panic!("no variant {name}"));
    let candidates: Vec<_> = seats.iter().filter(|(_, s)| *s == Side::Candidate).map(|(p, _)| (*p, mind(&options.candidate))).collect();
    let seated = Seated::new(mind(&options.baseline), candidates);
    let mut driver = AiDriver::with_mind(ClassicBench::settings(rules), Vec::new(), seated);
    driver.set_trace_capacity(0);
    classic_rules::ai::allocator::probe::enable(true);
    let mut out: Vec<Seat> = seats.iter().map(|(_, s)| Seat { side: Some(*s), ..Seat::default() }).collect();
    let mut seen: Vec<BTreeMap<(u32, u32, u8), Seen>> = vec![BTreeMap::new(); seats.len()];
    let standard = Variant::Standard;
    let mut pending: Vec<Option<Pending>> = vec![None; seats.len()];
    while !game.is_over() && game.view().auth.turn().round <= suite.rounds {
        let mut last_active = None;
        loop {
            let active = game.view().auth.active_player();
            if Some(active) != last_active {
                last_active = Some(active);
                if let Some(index) = seats.iter().position(|(p, _)| *p == active) {
                    pending[index] = Some(turn_start(&game, rules, active));
                }
            }
            match driver.advance(&game, rules) {
                Advance::Worked => {}
                Advance::RoundDone | Advance::Waiting => break,
                Advance::Command(command) | Advance::EndTurn(command) => {
                    let result = game.execute(rules, command);
                    driver.executed(result);
                }
            }
        }
        let choices = classic_rules::ai::allocator::probe::take();
        let now = game.view().auth.turn().round;
        let auth = game.view().auth;
        for (index, (player, _)) in seats.iter().enumerate() {
            let seat = &mut out[index];
            if !auth.players().is_alive(*player) {
                continue;
            }
            // Routes running now, delivered yields, new and lost routes.
            let mut now_seen = BTreeMap::new();
            let mut running = 0u32;
            for route in auth.module().trade().owned_by(*player) {
                running += 1;
                seat.food += u64::from(route.destination_yields.food);
                seat.production += u64::from(route.destination_yields.production);
                seat.gold += u64::from(route.origin_yields.gold);
                seat.science += u64::from(route.origin_yields.science);
                let key = (route.origin.index, route.destination.index, route.kind.code());
                let length = route.length();
                let progress = if route.forward { u32::from(route.position) } else { 2 * length - u32::from(route.position) };
                let remaining = u32::from(route.circuits_total.saturating_sub(route.circuits_done)) * 2 * length - progress.min(2 * length);
                let speed = classic_rules::derived::trade::speed(route.domain);
                let mut hazard = seen[index].get(&key).is_some_and(|s| s.hazard);
                if !seen[index].contains_key(&key) {
                    let obs = game.ai_observation(rules, *player);
                    let hazards = classic_rules::ai::trade::Hazards::of(&obs, rules);
                    let tile = |c: kernel::ids::CityHandle| auth.cities().get(c).map(|core| core.tile);
                    hazard = tile(route.origin).is_some_and(|o| hazards.near(&obs, o, tile(route.destination)));
                    seat.started += 1;
                    seat.started_kind[match route.kind {
                        RouteKind::Food => 0,
                        RouteKind::Production => 1,
                        RouteKind::International => 2,
                    }] += 1;
                }
                now_seen.insert(key, Seen { remaining, speed, hazard });
            }
            for (key, was) in &seen[index] {
                if !now_seen.contains_key(key) {
                    let lost = was.remaining > was.speed;
                    seat.by_hazard[usize::from(was.hazard)][0] += 1;
                    if lost {
                        seat.lost += 1;
                        seat.by_hazard[usize::from(was.hazard)][1] += 1;
                    }
                }
            }
            seen[index] = now_seen;
            let slots = routes_possible(auth, rules, *player);
            if slots > 0 && seat.slot_round.is_none() {
                seat.slot_round = Some(now);
            }
            if running > 0 && seat.route_round.is_none() {
                seat.route_round = Some(now);
            }
            if slots > 0 {
                seat.slot_rounds += 1;
                seat.slot_sum += u64::from(slots);
                seat.route_sum += u64::from(running.min(slots));
            }
            if let Some(p) = pending[index].take().filter(|p| p.empty) {
                seat.why[why_empty(&p, *player, &choices)] += 1;
            }
            if checkpoints.contains(&now) {
                seat.at.push(at(&game, rules, *player, slots, standard));
            }
        }
    }
    classic_rules::ai::allocator::probe::enable(false);
    // A game over early: the checkpoints left take its end.
    for (index, (player, _)) in seats.iter().enumerate() {
        while out[index].at.len() < checkpoints.len() {
            let slots = routes_possible(game.view().auth, rules, *player);
            out[index].at.push(at(&game, rules, *player, slots, standard));
        }
    }
    GameResult { seed: game_match.seed, seats: out }
}

/// The seat's state at the start of its turn: whether a route slot is
/// empty, and what stands in the way (see `why_empty`).
fn turn_start(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Pending {
    let auth = game.view().auth;
    let running = auth.module().trade().owned_by(player).count() as u32;
    let mut p = Pending { empty: auth.players().is_alive(player) && running < routes_possible(auth, rules, player), ..Pending::default() };
    if !p.empty {
        return p;
    }
    let obs = game.ai_observation(rules, player);
    let hazards = classic_rules::ai::trade::Hazards::of(&obs, rules);
    let tile = |c: kernel::ids::CityHandle| auth.cities().get(c).map(|core| core.tile);
    for unit in auth.indexes().units_of(player) {
        let Some(view) = auth.units().get(*unit) else { continue };
        if !is_trade_unit(rules, view.core.kind) {
            continue;
        }
        let offers = trade_offers(auth, rules, *unit);
        if offers.is_empty() {
            p.idle_stuck = true;
        } else if offers.iter().any(|o| !hazards.near(&obs, view.core.tile, tile(o.destination))) {
            p.idle_offers = true;
        } else {
            p.idle_hazard = true;
        }
    }
    let trade_item = |i: &ProductionItem| matches!(i, ProductionItem::Unit(k) if is_trade_unit(rules, *k));
    let cities = auth.indexes().cities_of(player);
    p.cities = cities.len();
    p.queued = cities.iter().filter_map(|c| auth.cities().get(*c)).any(|c| c.queue.iter().any(trade_item));
    let kinds = known_trade_kinds(game, rules, player);
    p.with_routes = cities
        .iter()
        .filter(|c| kinds.iter().any(|k| !prospects(auth, rules, **c, *k).is_empty()))
        .filter_map(|c| auth.cities().get(*c).map(|core| core.id))
        .collect();
    p.origin = cities.iter().filter_map(|c| auth.cities().get(*c)).any(|core| {
        core.buildings.iter().any(|b| {
            let abilities = &rules.buildings[b.0 as usize].module.abilities;
            abilities.contains(&classic_rules::rules::content::BuildingAbility::FoodTradeRoutes) || abilities.contains(&classic_rules::rules::content::BuildingAbility::ProductionTradeRoutes)
        })
    });
    if !p.with_routes.is_empty() {
        // `trade-fit`'s own price of a trade unit in each city (`trade::fit_prospect`).
        let v = Variant::Standard;
        let prices = classic_rules::ai::value::Prices::of_effects(&obs, rules, v.price_model(), v.science_pricing(), v.true_effects());
        let valued: Vec<_> = obs.own_cities.iter().filter(|c| classic_rules::ai::trade::fit_prospect(&obs, rules, &prices, &hazards, c, 0).is_some()).collect();
        p.valued = !valued.is_empty();
        p.building_head = valued.iter().any(|c| match c.queue.first() {
            Some(ProductionItem::Building(b)) => {
                let def = &rules.buildings[b.0 as usize];
                !def.wonder && def.kind != kernel::rules::content::BuildingKind::NationalWonder
            }
            _ => false,
        });
    }
    p
}

/// Why a slot of `player` was empty at the start of its turn (an index into
/// `WHY`), with the allocator's choices of the round (`allocator::probe`).
fn why_empty(p: &Pending, player: PlayerId, choices: &[classic_rules::ai::allocator::probe::Choice]) -> usize {
    if p.idle_offers {
        return 0;
    }
    if p.idle_hazard {
        return 1;
    }
    if p.idle_stuck {
        return 2;
    }
    if p.queued {
        return 3;
    }
    let mine = || choices.iter().filter(|c| c.player == player);
    let offered = mine().any(|c| c.options.iter().any(|o| matches!(o.item, ProductionItem::Unit(_)) && o.value > 0 && is_trade_option(c, o)));
    if offered {
        return 4;
    }
    if mine().any(|c| p.with_routes.contains(&c.city)) {
        return 5;
    }
    if !p.with_routes.is_empty() {
        return if !p.valued {
            6
        } else if p.building_head {
            8
        } else {
            7
        };
    }
    if p.cities < 2 {
        9
    } else if !p.origin {
        10
    } else {
        11
    }
}

/// The option is a trade unit (the probe keeps the item only).
fn is_trade_option(_choice: &classic_rules::ai::allocator::probe::Choice, option: &classic_rules::ai::allocator::probe::Candidate) -> bool {
    TRADE_KINDS.with(|k| matches!(option.item, ProductionItem::Unit(kind) if k.borrow().contains(&kind)))
}

thread_local! {
    /// The rules' trade unit kinds (set once per thread).
    static TRADE_KINDS: std::cell::RefCell<Vec<UnitKindId>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn at(game: &GameState, rules: &CompiledRuleset, player: PlayerId, slots: u32, prices: Variant) -> At {
    let auth = game.view().auth;
    if !auth.players().is_alive(player) {
        return At::default();
    }
    let mut a = At { alive: true, slots, ..At::default() };
    for route in auth.module().trade().owned_by(player) {
        a.routes += 1;
        match route.kind {
            RouteKind::Food => a.food += 1,
            RouteKind::Production => a.production += 1,
            RouteKind::International => {
                a.international += 1;
                if auth.cities().get(route.destination).is_some_and(|c| auth.players().is_city_state(c.owner)) {
                    a.city_states += 1;
                }
            }
        }
    }
    for c in auth.indexes().cities_of(player) {
        if let Some(core) = auth.cities().get(*c) {
            a.cities += 1;
            a.citizens += core.population;
        }
    }
    let holdings = aurelia_qa::grounds::holdings(game, rules, player);
    let sheet = aurelia_qa::grounds::price_sheet(game, rules, player, prices);
    a.potential = aurelia_qa::grounds::potential(&holdings, &sheet).total;
    a.score = ClassicBench::score(game, rules, player);
    a
}

// ---- report ----

/// Two-sided 95 % t quantile for `df` degrees of freedom.
fn t95(df: usize) -> f64 {
    const T: [f64; 30] = [12.71, 4.30, 3.18, 2.78, 2.57, 2.45, 2.36, 2.31, 2.26, 2.23, 2.20, 2.18, 2.16, 2.14, 2.13, 2.12, 2.11, 2.10, 2.09, 2.09, 2.08, 2.07, 2.07, 2.06, 2.06, 2.06, 2.05, 2.05, 2.05, 2.04];
    if df == 0 {
        return f64::NAN;
    }
    T.get(df - 1).copied().unwrap_or(if df < 60 { 2.01 } else { 1.98 })
}

/// Mean and 95 % t half-width of per-block values.
fn mean_t(values: &[f64]) -> (f64, f64) {
    let n = values.len();
    if n == 0 {
        return (f64::NAN, f64::NAN);
    }
    let mean = values.iter().sum::<f64>() / n as f64;
    if n < 2 {
        return (mean, f64::NAN);
    }
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    (mean, t95(n - 1) * (var / n as f64).sqrt())
}

/// Per seed block, the mean of `value` over a side's seats (alive or not).
fn by_block(results: &[GameResult], side: Side, value: &dyn Fn(&Seat) -> Option<f64>) -> BTreeMap<u64, f64> {
    let mut sums: BTreeMap<u64, (f64, u32)> = BTreeMap::new();
    for g in results {
        for s in g.seats.iter().filter(|s| s.side == Some(side)) {
            if let Some(v) = value(s) {
                let e = sums.entry(g.seed).or_default();
                e.0 += v;
                e.1 += 1;
            }
        }
    }
    sums.into_iter().filter(|(_, (_, n))| *n > 0).map(|(k, (s, n))| (k, s / f64::from(n))).collect()
}

fn line(results: &[GameResult], solo: bool, name: &str, value: &dyn Fn(&Seat) -> Option<f64>) -> String {
    let fmt = |(m, h): (f64, f64)| if h.is_nan() { format!("{m:.2}") } else { format!("{m:.2} ± {h:.2}") };
    let cand = by_block(results, Side::Candidate, value);
    if solo {
        let v: Vec<f64> = cand.values().copied().collect();
        return format!("| {name} | {} |\n", fmt(mean_t(&v)));
    }
    let base = by_block(results, Side::Baseline, value);
    let c: Vec<f64> = cand.values().copied().collect();
    let b: Vec<f64> = base.values().copied().collect();
    let diff: Vec<f64> = cand.iter().filter_map(|(k, v)| base.get(k).map(|w| v - w)).collect();
    format!("| {name} | {} | {} | {} |\n", fmt(mean_t(&c)), fmt(mean_t(&b)), fmt(mean_t(&diff)))
}

fn report(options: &Options, suite: &Suite, checkpoints: &[u32], results: &[GameResult], solo: bool) -> String {
    use std::fmt::Write as _;
    let blocks: std::collections::BTreeSet<u64> = results.iter().map(|g| g.seed).collect();
    let mut s = String::new();
    let _ = writeln!(s, "# Trade routes: {} vs {} — suite {}, speed {}, {} games, {} seed blocks, offsets {}..{}\n", options.candidate, options.baseline, suite.name, options.speed, results.len(), blocks.len(), options.offset, options.offset + options.repeat);
    let head = if solo { "| metric | value |\n|---|---|\n".to_owned() } else { format!("| metric (mean per seat, 95 % t over seed blocks) | {} | {} | difference |\n|---|---|---|---|\n", options.candidate, options.baseline) };
    s.push_str(&head);
    let opt = |v: Option<u32>| v.map(f64::from);
    s.push_str(&line(results, solo, "first round with a route slot", &|x| opt(x.slot_round)));
    s.push_str(&line(results, solo, "first round with a route (seats with one)", &|x| opt(x.route_round)));
    s.push_str(&line(results, solo, "seats with a route by the end (share)", &|x| Some(f64::from(u8::from(x.route_round.is_some())))));
    s.push_str(&line(results, solo, "slots used (routes running / slots, seat-rounds with a slot)", &|x| (x.slot_sum > 0).then(|| x.route_sum as f64 / x.slot_sum as f64)));
    s.push_str(&line(results, solo, "routes started", &|x| Some(f64::from(x.started))));
    s.push_str(&line(results, solo, "  of them internal food", &|x| Some(f64::from(x.started_kind[0]))));
    s.push_str(&line(results, solo, "  of them internal production", &|x| Some(f64::from(x.started_kind[1]))));
    s.push_str(&line(results, solo, "  of them international", &|x| Some(f64::from(x.started_kind[2]))));
    s.push_str(&line(results, solo, "routes lost before their end", &|x| Some(f64::from(x.lost))));
    s.push_str(&line(results, solo, "  routes ended with no hazard at start", &|x| Some(f64::from(x.by_hazard[0][0]))));
    s.push_str(&line(results, solo, "    of them lost", &|x| Some(f64::from(x.by_hazard[0][1]))));
    s.push_str(&line(results, solo, "  routes ended with a hazard at start", &|x| Some(f64::from(x.by_hazard[1][0]))));
    s.push_str(&line(results, solo, "    of them lost", &|x| Some(f64::from(x.by_hazard[1][1]))));
    s.push_str(&line(results, solo, "food delivered (whole, game)", &|x| Some(x.food as f64 / 100.0)));
    s.push_str(&line(results, solo, "production delivered (whole, game)", &|x| Some(x.production as f64 / 100.0)));
    s.push_str(&line(results, solo, "gold to the origin (whole, game)", &|x| Some(x.gold as f64 / 100.0)));
    s.push_str(&line(results, solo, "science to the origin (whole, game)", &|x| Some(x.science as f64 / 100.0)));
    for (i, name) in WHY.iter().enumerate() {
        s.push_str(&line(results, solo, &format!("empty slot-rounds: {name}"), &|x| Some(x.why[i] as f64)));
    }
    for (i, round) in checkpoints.iter().enumerate() {
        let a = move |x: &Seat| x.at.get(i).copied();
        s.push_str(&line(results, solo, &format!("r{round} route slots"), &|x| a(x).map(|a| f64::from(a.slots))));
        s.push_str(&line(results, solo, &format!("r{round} routes running"), &|x| a(x).map(|a| f64::from(a.routes))));
        s.push_str(&line(results, solo, &format!("r{round}   internal food"), &|x| a(x).map(|a| f64::from(a.food))));
        s.push_str(&line(results, solo, &format!("r{round}   internal production"), &|x| a(x).map(|a| f64::from(a.production))));
        s.push_str(&line(results, solo, &format!("r{round}   international"), &|x| a(x).map(|a| f64::from(a.international))));
        s.push_str(&line(results, solo, &format!("r{round}   of them to city-states"), &|x| a(x).map(|a| f64::from(a.city_states))));
        s.push_str(&line(results, solo, &format!("r{round} seats without a route (share)"), &|x| a(x).filter(|a| a.alive).map(|a| f64::from(u8::from(a.routes == 0)))));
        s.push_str(&line(results, solo, &format!("r{round} cities"), &|x| a(x).map(|a| f64::from(a.cities))));
        s.push_str(&line(results, solo, &format!("r{round} citizens"), &|x| a(x).map(|a| f64::from(a.citizens))));
        s.push_str(&line(results, solo, &format!("r{round} potential (standard's prices)"), &|x| a(x).map(|a| a.potential)));
        s.push_str(&line(results, solo, &format!("r{round} score"), &|x| a(x).map(|a| a.score as f64)));
    }
    s
}
