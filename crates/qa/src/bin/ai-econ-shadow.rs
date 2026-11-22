//! Where the economy nets choose otherwise than `standard` (classic/docs/ai-econ-nets.md): solo
//! games of an economy candidate (`econ` by default) with the shadow record on
//! (`classic_rules::ai::econ_shadow`). Every settler's choice of the site net is recorded beside the
//! site the standard quality ranks first in the same state; every choice of the choice net beside
//! the strategist's choice it was read over. The report: how often they differ, how the sites
//! differ (distances to the own and foreign cities, the land score), which orders and technologies
//! the choice net takes instead of which, and a few cases.
//!
//! `cargo run --release -p aurelia-qa --bin ai-econ-shadow -- [options]`
//!
//! --variant NAME   the candidate (default econ-site)
//! --suite NAME     its solo games (default pace)
//! --speed NAME     the game speed (default quick)
//! --games N        the first N games of the suite (default 10)
//! --seed-offset K  the suite's seeds moved by K strides (default 0)
//! --cases N        cases printed of each kind (default 12)
//! --jobs J         games in parallel (default 2)
//! --probe          no games: the site net's answer on its reference rows (`site_net::references`) as
//!                  one input moves (the round, the distances to the own and the foreign city)
//! --founded A,B    no shadow: the solo games of each variant, and where their cities were founded
//!                  (the distance to the nearest own and foreign city), the settlers that never founded
//!                  one, and the gaps left at the suite's third checkpoint (r198 at Quick): unowned land
//!                  tiles three or more tiles from every city and within five of two cities of one
//!                  major player, where another player can still found a city between them

use std::collections::BTreeMap;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::suite::{Match, Mode, Suite};
use aurelia_qa::export::parallel;
use classic_rules::ai::econ_shadow::{self, Case, SiteCase};
use classic_rules::ids::ProductionItem;
use classic_rules::{CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::Advance;

struct Options {
    variant: String,
    suite: String,
    speed: String,
    games: usize,
    seed_offset: u64,
    cases: usize,
    jobs: usize,
    probe: bool,
    founded: Option<String>,
}

fn options() -> Options {
    let mut o = Options { variant: "econ-site".to_owned(), suite: "pace".to_owned(), speed: "quick".to_owned(), games: 10, seed_offset: 0, cases: 12, jobs: 2, probe: false, founded: None };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--probe" {
            o.probe = true;
            continue;
        }
        let value = args.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--variant" => o.variant = value,
            "--suite" => o.suite = value,
            "--speed" => o.speed = value,
            "--games" => o.games = value.parse().expect("--games N"),
            "--seed-offset" => o.seed_offset = value.parse().expect("--seed-offset K"),
            "--cases" => o.cases = value.parse().expect("--cases N"),
            "--jobs" => o.jobs = value.parse().expect("--jobs J"),
            "--founded" => o.founded = Some(value),
            other => panic!("unknown option {other}"),
        }
    }
    o
}

/// One game with the shadow record on: its cases.
fn play(rules: &CompiledRuleset, suite: &Suite, game_match: &Match, variant: &str) -> Vec<Case> {
    let mut game: GameState = ClassicBench::new_game_at(rules, &game_match.map, game_match.seed, suite.rounds, &suite.speed);
    let mind = ClassicBench::mind(variant).unwrap_or_else(|| panic!("no AI version {variant}"));
    let mut driver = AiDriver::with_mind(ClassicBench::settings(rules), Vec::new(), mind);
    driver.set_trace_capacity(0);
    driver.configure_game(&mut game);
    econ_shadow::enable(true);
    while !game.is_over() {
        match driver.advance(&game, rules) {
            Advance::Worked | Advance::RoundDone | Advance::Waiting => {}
            Advance::Command(command) | Advance::EndTurn(command) => {
                let result = game.execute(rules, command);
                driver.executed(result);
            }
        }
    }
    let cases = econ_shadow::take();
    econ_shadow::enable(false);
    cases
}

fn item_name(rules: &CompiledRuleset, item: ProductionItem) -> String {
    match item {
        ProductionItem::Unit(kind) => rules.units[usize::from(kind.0)].name.clone(),
        ProductionItem::Building(kind) => rules.buildings[usize::from(kind.0)].key.clone(),
        ProductionItem::Project(id) => format!("project {}", id.0),
    }
}

fn distance(d: Option<i32>) -> String {
    d.map_or_else(|| "—".to_owned(), |d| d.to_string())
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, n) = values.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    if n == 0 { 0.0 } else { sum / n as f64 }
}

/// The site part: the last choice of each settler (the one it founded on or died with), how often
/// the net's site is another, and how the two differ.
fn report_sites(cases: &[(usize, Case)], shown: usize) {
    let mut last: BTreeMap<(usize, u64), (u32, SiteCase, SiteCase)> = BTreeMap::new();
    let mut decisions = 0usize;
    let mut differ = 0usize;
    for (game, case) in cases {
        if let Case::Site { round, settler, net, standard, .. } = case {
            decisions += 1;
            differ += usize::from(net.tile != standard.tile);
            last.insert((*game, *settler), (*round, *net, *standard));
        }
    }
    let settlers = last.len();
    let other: Vec<&(u32, SiteCase, SiteCase)> = last.values().filter(|(_, n, s)| n.tile != s.tile).collect();
    println!("## Sites\n");
    println!("{decisions} site choices of the net, {differ} on another tile than the standard quality's best; {settlers} settlers, of whose last choice {} differ.\n", other.len());
    if other.is_empty() {
        return;
    }
    let near = |d: Option<i32>, at: i32| d.is_some_and(|d| d <= at);
    println!("| last choice of a settler, where they differ | net's site | standard's site |");
    println!("|---|---:|---:|");
    println!("| mean distance to the nearest own city | {:.2} | {:.2} |", mean(other.iter().filter_map(|(_, n, _)| n.own_distance.map(f64::from))), mean(other.iter().filter_map(|(_, _, s)| s.own_distance.map(f64::from))));
    println!("| mean distance to the nearest foreign city | {:.2} | {:.2} |", mean(other.iter().filter_map(|(_, n, _)| n.foreign_distance.map(f64::from))), mean(other.iter().filter_map(|(_, _, s)| s.foreign_distance.map(f64::from))));
    println!("| within 5 of a foreign city (forward) | {} | {} |", other.iter().filter(|(_, n, _)| near(n.foreign_distance, 5)).count(), other.iter().filter(|(_, _, s)| near(s.foreign_distance, 5)).count());
    println!("| within 3 of an own city (a gap) | {} | {} |", other.iter().filter(|(_, n, _)| near(n.own_distance, 3)).count(), other.iter().filter(|(_, _, s)| near(s.own_distance, 3)).count());
    println!("| mean land score (`SiteContext::score`) | {:.1} | {:.1} |", mean(other.iter().map(|(_, n, _)| f64::from(n.heuristic))), mean(other.iter().map(|(_, _, s)| f64::from(s.heuristic))));
    println!();
    println!("| round | net: own / foreign distance, land score | standard: own / foreign distance, land score |");
    println!("|---:|---|---|");
    for (round, n, s) in other.iter().take(shown) {
        println!("| {round} | {} / {}, {} | {} / {}, {} |", distance(n.own_distance), distance(n.foreign_distance), n.heuristic, distance(s.own_distance), distance(s.foreign_distance), s.heuristic);
    }
    println!();
}

/// The choice part: the choices the net took over the strategist's, by pair.
fn report_choices(rules: &CompiledRuleset, cases: &[(usize, Case)], shown: usize) {
    let mut production: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut research: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut rounds = (Vec::new(), Vec::new());
    let mut read = (0usize, 0usize);
    // (kind, standing rank, taken rank) → choices, every choice read (kept ones too).
    let mut ranks: BTreeMap<(&str, u8, u8), usize> = BTreeMap::new();
    for (_, case) in cases {
        match case {
            Case::Production { ranks: r, .. } => *ranks.entry(("production", r.standing, r.taken)).or_insert(0) += 1,
            Case::Research { ranks: r, .. } => *ranks.entry(("research", r.standing, r.taken)).or_insert(0) += 1,
            Case::Site { .. } => {}
        }
        match case {
            Case::Production { .. } => read.0 += 1,
            Case::Research { .. } => read.1 += 1,
            Case::Site { .. } => {}
        }
        if is_same(case) {
            continue;
        }
        match case {
            Case::Production { round, standing, taken, .. } => {
                *production.entry((item_name(rules, *standing), item_name(rules, *taken))).or_insert(0) += 1;
                rounds.0.push(f64::from(*round));
            }
            Case::Research { round, standing, taken, .. } => {
                let name = |t: classic_rules::ids::TechId| rules.technologies[usize::from(t.0)].key.clone();
                *research.entry((name(*standing), name(*taken))).or_insert(0) += 1;
                rounds.1.push(f64::from(*round));
            }
            Case::Site { .. } => {}
        }
    }
    println!("## Ranks in the allocator's top (0: its best), every choice the net read\n");
    println!("| choice | strategist's rank | taken rank | choices |");
    println!("|---|---:|---:|---:|");
    for ((kind, standing, taken), n) in &ranks {
        println!("| {kind} | {standing} | {taken} | {n} |");
    }
    println!();
    for (title, pairs, at, read) in [("Production", &production, &rounds.0, read.0), ("Research", &research, &rounds.1, read.1)] {
        let total: usize = pairs.values().sum();
        println!("## {title}: the net over the strategist\n");
        println!("{read} choices of the strategist read by the net (its choice within the allocator's top); {total} taken over (mean round {:.0}).\n", mean(at.iter().copied()));
        let mut sorted: Vec<(&(String, String), &usize)> = pairs.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        println!("| strategist's | net's | times |");
        println!("|---|---|---:|");
        for ((standing, taken), n) in sorted.into_iter().take(shown) {
            println!("| {standing} | {taken} | {n} |");
        }
        println!();
    }
}

/// The site net's mean answer (citizens at +50) on its reference rows with one feature set to each
/// value, beside the rows as they are.
fn probe_site_net() {
    use classic_rules::ai::site::SITE_NAMES;
    use classic_rules::ai::site_net::{predict, references};
    let at = |name: &str| SITE_NAMES.iter().position(|n| *n == name).unwrap_or_else(|| panic!("no feature {name}"));
    let rows: Vec<_> = references().iter().map(|r| r.features).collect();
    let mean_with = |feature: usize, value: Option<i32>| {
        let answers = rows.iter().map(|row| {
            let mut row = *row;
            if let Some(v) = value {
                row[feature] = v;
            }
            f64::from(predict(&row)) / 1000.0
        });
        mean(answers)
    };
    println!("# The site net on its {} reference rows (mean citizens at +50)\n", rows.len());
    println!("as recorded: {:.2}\n", mean_with(0, None));
    for (name, values) in [
        ("round", vec![20, 40, 60, 90, 120, 160, 200]),
        ("own_city_distance", vec![3, 4, 5, 6, 7, 8, 10]),
        ("foreign_city_distance", vec![3, 4, 5, 6, 8, 10, 99]),
        ("own_cities", vec![1, 2, 3, 5, 8, 12]),
        ("foreign_tiles", vec![0, 2, 5, 10]),
    ] {
        let feature = at(name);
        let cells: Vec<String> = values.iter().map(|&v| format!("{v}: {:.2}", mean_with(feature, Some(v)))).collect();
        println!("| {name} | {} |", cells.join(" | "));
    }
}

/// The net kept the strategist's choice.
fn is_same(case: &Case) -> bool {
    match case {
        Case::Production { standing, taken, .. } => standing == taken,
        Case::Research { standing, taken, .. } => standing == taken,
        Case::Site { net, standard, .. } => net.tile == standard.tile,
    }
}

/// Where one game's cities were founded and what was left between them.
#[derive(Debug, Default, Clone)]
struct Founded {
    seats: usize,
    /// Per founding (the capitals' first cities aside): the distance to the nearest own and foreign city.
    own: Vec<i32>,
    foreign: Vec<i32>,
    settlers: usize,
    never: usize,
    gaps: usize,
    cities: usize,
}

fn play_founded(rules: &CompiledRuleset, suite: &Suite, game_match: &Match, variant: &str) -> Founded {
    use aurelia_qa::bench::modules::majors;
    use classic_rules::knowledge::actions::acting_unit;
    use classic_rules::Command;
    let mut game: GameState = ClassicBench::new_game_at(rules, &game_match.map, game_match.seed, suite.rounds, &suite.speed);
    let mind = ClassicBench::mind(variant).unwrap_or_else(|| panic!("no AI version {variant}"));
    let mut driver = AiDriver::with_mind(ClassicBench::settings(rules), Vec::new(), mind);
    driver.set_trace_capacity(0);
    driver.configure_game(&mut game);
    let seats = majors(&game);
    let at = suite.checkpoints.get(2).copied().unwrap_or(suite.rounds);
    let mut out = Founded { seats: seats.len(), ..Default::default() };
    let mut settlers: BTreeMap<u64, bool> = BTreeMap::new();
    let round = |game: &GameState| game.view().auth.turn().round;
    while !game.is_over() && round(&game) <= at {
        let auth = game.view().auth;
        for &p in &seats {
            for h in auth.indexes().units_of(p) {
                if let Some(u) = auth.units().get(*h).filter(|u| rules.units.get(usize::from(u.core.kind.0)).is_some_and(|d| d.founder)) {
                    settlers.entry(u.core.id.0).or_insert(false);
                }
            }
        }
        loop {
            match driver.advance(&game, rules) {
                Advance::Worked => {}
                Advance::RoundDone | Advance::Waiting => break,
                Advance::Command(command) | Advance::EndTurn(command) => {
                    let founding = match (&command, acting_unit(&command)) {
                        (Command::FoundCity { .. }, Some(h)) => game.view().auth.units().get(h).map(|u| (u.core.id.0, u.core.tile, u.core.owner)),
                        _ => None,
                    };
                    let distances = founding.map(|(id, tile, owner)| (id, nearest_cities(&game, tile, owner)));
                    let result = game.execute(rules, command);
                    let applied = result.is_ok();
                    driver.executed(result);
                    let Some((id, (own, foreign))) = distances.filter(|_| applied) else { continue };
                    settlers.insert(id, true);
                    if let Some(own) = own {
                        out.own.push(own);
                        out.foreign.push(foreign.unwrap_or(99));
                    }
                }
            }
        }
    }
    out.settlers = settlers.len();
    out.never = settlers.values().filter(|f| !**f).count();
    out.gaps = gaps(&game, &seats);
    out.cities = seats.iter().map(|p| game.view().auth.indexes().cities_of(*p).len()).sum();
    out
}

/// The nearest own and foreign city to `tile` (None: none).
fn nearest_cities(game: &GameState, tile: classic_rules::ids::TileId, owner: classic_rules::ids::PlayerId) -> (Option<i32>, Option<i32>) {
    let auth = game.view().auth;
    let dims = &auth.map().dimensions;
    let (mut own, mut foreign): (Option<i32>, Option<i32>) = (None, None);
    for (_, city) in auth.cities().iter() {
        let Some(d) = dims.distance(city.tile, tile) else { continue };
        let slot = if city.owner == owner { &mut own } else { &mut foreign };
        if slot.is_none_or(|x| d < x) {
            *slot = Some(d);
        }
    }
    (own, foreign)
}

/// Unowned land tiles at three or more from every city and within five of two cities of one major.
fn gaps(game: &GameState, seats: &[classic_rules::ids::PlayerId]) -> usize {
    let auth = game.view().auth;
    let map = auth.map();
    let cities: Vec<_> = auth.cities().iter().map(|(_, c)| (c.tile, c.owner)).collect();
    let mut count = 0;
    for t in 0..map.tile_count() as u32 {
        let tile = classic_rules::ids::TileId(t);
        if map.is_water(tile) || map.owner(tile).flatten().is_some() {
            continue;
        }
        let near: Vec<_> = cities.iter().filter_map(|(c, owner)| map.dimensions.distance(*c, tile).map(|d| (d, *owner))).collect();
        if near.iter().any(|(d, _)| *d < 3) {
            continue;
        }
        let between = seats.iter().any(|p| near.iter().filter(|(d, owner)| *d <= 5 && owner == p).count() >= 2);
        count += usize::from(between);
    }
    count
}

fn report_founded(rules: &CompiledRuleset, suite: &Suite, matches: &[Match], variants: &str, jobs: usize) {
    println!("| variant | seats | cities at r{} a seat | founded (not capitals) | own distance, mean | at 3 from an own city | within 5 of a foreign city | foreign distance, mean | settlers | never founded | gaps a seat |", suite.checkpoints.get(2).copied().unwrap_or(suite.rounds));
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for variant in variants.split(',').map(str::trim).filter(|v| !v.is_empty()) {
        let games = parallel(matches, jobs, |m| play_founded(rules, suite, m, variant));
        let seats: usize = games.iter().map(|g| g.seats).sum();
        let own: Vec<i32> = games.iter().flat_map(|g| g.own.iter().copied()).collect();
        let foreign: Vec<i32> = games.iter().flat_map(|g| g.foreign.iter().copied()).collect();
        let per_seat = |n: usize| n as f64 / seats.max(1) as f64;
        println!(
            "| {variant} | {seats} | {:.2} | {} | {:.2} | {:.0} % | {:.0} % | {:.2} | {} | {} | {:.1} |",
            per_seat(games.iter().map(|g| g.cities).sum()),
            own.len(),
            mean(own.iter().map(|d| f64::from(*d))),
            100.0 * own.iter().filter(|d| **d <= 3).count() as f64 / own.len().max(1) as f64,
            100.0 * foreign.iter().filter(|d| **d <= 5).count() as f64 / foreign.len().max(1) as f64,
            mean(foreign.iter().filter(|d| **d < 99).map(|d| f64::from(*d))),
            games.iter().map(|g| g.settlers).sum::<usize>(),
            games.iter().map(|g| g.never).sum::<usize>(),
            per_seat(games.iter().map(|g| g.gaps).sum()),
        );
    }
}

fn main() {
    let o = options();
    if o.probe {
        probe_site_net();
        return;
    }
    if let Some(variants) = &o.founded {
        let rules = ClassicBench::rules();
        let suite = Suite::named(&o.suite).unwrap_or_else(|| panic!("unknown suite {}", o.suite));
        let def = rules.speed_named(&o.speed).unwrap_or_else(|| panic!("unknown speed {}", o.speed));
        let suite = suite.at_speed(&def.key, &def.values);
        let matches: Vec<Match> = suite.matches(Mode::Solo, o.seed_offset).into_iter().take(o.games).collect();
        println!("# Where the cities were founded: {} solo games of {} at {}, offset {}\n", matches.len(), suite.name, o.speed, o.seed_offset);
        report_founded(&rules, &suite, &matches, variants, o.jobs);
        return;
    }
    let rules = ClassicBench::rules();
    let suite = Suite::named(&o.suite).unwrap_or_else(|| panic!("unknown suite {}", o.suite));
    let def = rules.speed_named(&o.speed).unwrap_or_else(|| panic!("unknown speed {}", o.speed));
    let suite = suite.at_speed(&def.key, &def.values);
    let matches: Vec<Match> = suite.matches(Mode::Solo, o.seed_offset).into_iter().take(o.games).collect();
    println!("# {} against `standard`'s choice in its place: {} solo games of {} at {}, offset {}\n", o.variant, matches.len(), suite.name, o.speed, o.seed_offset);
    let per_game = parallel(&matches, o.jobs, |m| play(&rules, &suite, m, &o.variant));
    let cases: Vec<(usize, Case)> = per_game.into_iter().enumerate().flat_map(|(g, cases)| cases.into_iter().map(move |c| (g, c))).collect();
    report_sites(&cases, o.cases);
    report_choices(&rules, &cases, o.cases);
}
