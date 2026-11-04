//! The lives of the land workers (classic/docs/ai-workers.md): per AI version,
//! over the solo games of a suite, how many workers the nations kept, what
//! each worker-turn went to (building, walking, standing, and the AI's
//! reason when it stood), the work the rules still offered in the cities'
//! radius, the workers taken (captured or killed) and what they were doing,
//! the gold their upkeep cost, and the tiles improved by the checkpoint
//! rounds (a third, two thirds and the end of the suite).
//!
//! `cargo run --release -p aurelia-qa --bin ai-workers -- [options]`
//!
//! --suite NAME     smoke|standard|duel|pace (default standard); its solo games
//! --speed NAME     the game speed (default quick)
//! --offset K       the suite's seed offset (default 0)
//! --games N        the first N games of the suite (default all)
//! --minds A,B      the versions, as the bench names them (default `standard`)
//! --jobs J         games in parallel (default 4)
//!
//! A worker-turn is the worker's round from one round start to the next: it
//! *builds* when it has a build under way at the next start or ordered one
//! during the round, it *walks* when it ends on another tile, it *stands*
//! otherwise (in a city of its owner, or outside). The work left is the
//! rules' own: on every land tile of the player in a city's work radius
//! with no improvement (or a pillaged one), the fewest build turns of an
//! improvement a worker may build there now (`world::builds::site`, the
//! player's technologies); the part on the tiles the cities work now is
//! counted apart. The upkeep is the allocator's own price of the workers:
//! the unit upkeep of all units less that of the units without them
//! (`derived::income::unit_upkeep_at`, the free units aside).

use std::collections::{BTreeMap, BTreeSet};

use aurelia_qa::bench::modules::{majors, BenchRules, ClassicBench};
use aurelia_qa::bench::suite::{Match, Mode, Suite};
use aurelia_qa::export::parallel;
use classic_rules::ai::improvements::yield_value;
use classic_rules::derived::tile_yields::{plot_yields, plot_yields_with};
use classic_rules::ids::{ImprovementId, PlayerId, TileId};
use classic_rules::knowledge::actions::acting_unit;
use classic_rules::rules::content::{Domain, ImprovementKind};
use classic_rules::world::builds::{site, Builder};
use classic_rules::world::facts::TruthFacts;
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::Advance;

/// Round bins of the time series.
const BINS: usize = 6;

#[derive(Debug, Default, Clone)]
struct Bin {
    seat_rounds: u64,
    cities: u64,
    workers: u64,
    builds: u64,
    walks: u64,
    stands: u64,
    stands_in_city: u64,
    /// Worker-turns standing while the player had no work left at all.
    stands_no_work: u64,
    work_left: u64,
    work_worked: u64,
    /// Seat-rounds with more workers than the work left in turns / 10.
    surplus_rounds: u64,
    upkeep: i64,
    built: u64,
}

#[derive(Debug, Default, Clone)]
struct GameWorkers {
    seats: usize,
    bins: [Bin; BINS],
    built: u64,
    gained_other: u64,
    lost: u64,
    /// When a worker was built: the work left a worker, the new one counted
    /// (turns: all the radius, and the tiles the cities work).
    built_at: Vec<(u64, u64)>,
    improved: [u64; 3],
    road_orders: u64,
    upkeep_total: i64,
    /// The AI's last reason for a worker in a round it stood, by name.
    standing_reasons: BTreeMap<String, u64>,
    /// The workers taken from the player (captured or killed), by what they
    /// did (`play`).
    lost_reasons: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Copy)]
struct Worker {
    tile: TileId,
    working: bool,
}

/// The improvements a land worker may build (as the AI's job finder reads
/// the data: regular or the nation's unique, not a prompt, built by work).
fn worker_improvements(rules: &CompiledRuleset) -> Vec<ImprovementId> {
    (0..rules.improvements.len())
        .filter(|i| {
            let def = &rules.improvements[*i];
            matches!(def.kind, ImprovementKind::Regular | ImprovementKind::Unique)
                && !def.unbuildable
                && def.work_turns > 0
                && !def.placement.water
                && !def.placement.prompt_when_complete
        })
        .map(|i| ImprovementId(i as u16))
        .collect()
}

/// The work left of `player` in turns (all its radius, and the tiles its
/// cities work now).
fn work_left(game: &GameState, rules: &CompiledRuleset, kinds: &[ImprovementId], player: PlayerId) -> (u64, u64) {
    let auth = game.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    let facts = TruthFacts::new(auth, rules, player);
    let builder = Builder::of(auth, player);
    let mut tiles = BTreeSet::new();
    let mut worked = BTreeSet::new();
    for handle in auth.indexes().cities_of(player) {
        let Some(core) = auth.cities().get(*handle) else { continue };
        for tile in classic_rules::math::hex_disc(dims, core.tile, classic_rules::world::city::CITY_WORK_RADIUS) {
            tiles.insert(tile);
        }
        worked.extend(classic_rules::derived::yields::city_work(auth, rules, core).tiles());
    }
    let (mut all, mut on_worked) = (0, 0);
    for tile in tiles {
        if map.owner(tile).flatten() != Some(player) || map.is_water(tile) {
            continue;
        }
        let standing = map.improvement(tile).flatten();
        if standing.is_some() && !map.pillaged(tile) {
            continue;
        }
        // A build that adds to the tile's yields or connects its resource (the
        // rules' yields; a fort or a road-like build adds nothing).
        let now = yield_value(&plot_yields(&facts, rules, tile, &builder.techs).at_least(0));
        let resource = map.resource(tile).flatten().and_then(|r| rules.resources.get(r.0 as usize));
        let useful = |kind: ImprovementId, removes: bool| {
            let then = yield_value(&plot_yields_with(&facts, rules, tile, &builder.techs, kind, removes).at_least(0));
            then > now || resource.is_some_and(|r| r.improvements.contains(&kind))
        };
        let turns = match standing {
            Some(kind) => site(&facts, rules, &builder, tile, kind).ok().map(|p| p.work_turns),
            None => kinds
                .iter()
                .filter_map(|k| site(&facts, rules, &builder, tile, *k).ok().filter(|p| useful(*k, p.removes.is_some())))
                .map(|p| p.work_turns)
                .min(),
        };
        if let Some(turns) = turns {
            all += u64::from(turns);
            if worked.contains(&tile) {
                on_worked += u64::from(turns);
            }
        }
    }
    (all, on_worked)
}

fn improved(game: &GameState, player: PlayerId) -> u64 {
    let auth = game.view().auth;
    let map = auth.map();
    (0..map.dimensions.tile_count() as u32)
        .map(TileId)
        .filter(|t| map.owner(*t).flatten() == Some(player) && !map.is_water(*t) && map.improvement(*t).flatten().is_some() && !map.pillaged(*t))
        .count() as u64
}

fn play(rules: &CompiledRuleset, suite: &Suite, game_match: &Match, mind: &str, kinds: &[ImprovementId]) -> GameWorkers {
    let mut game: GameState = ClassicBench::new_game_at(rules, &game_match.map, game_match.seed, suite.rounds, &suite.speed);
    let mind = ClassicBench::mind(mind).unwrap_or_else(|| panic!("no AI version {mind}"));
    let mut driver = AiDriver::with_mind(ClassicBench::settings(rules), Vec::new(), mind);
    driver.set_trace_capacity(1 << 15);
    driver.configure_game(&mut game);
    let seats = majors(&game);
    let land_worker = |kind: classic_rules::ids::UnitKindId| rules.units.get(usize::from(kind.0)).is_some_and(|d| d.builder && d.domain == Domain::Land);
    let mut out = GameWorkers { seats: seats.len(), ..Default::default() };
    let round = |game: &GameState| game.view().auth.turn().round;
    let last = suite.rounds;
    let checkpoints = [last / 3, last * 2 / 3, last];
    let mut before: BTreeMap<(PlayerId, u64), Worker> = BTreeMap::new();
    let mut ordered: BTreeSet<u64> = BTreeSet::new();
    let mut reasons: BTreeMap<u64, String> = BTreeMap::new();
    let mut prev_reasons: BTreeMap<u64, String> = BTreeMap::new();
    let mut owners_before: BTreeMap<u64, PlayerId> = BTreeMap::new();
    let mut first = true;
    loop {
        let now = round(&game);
        let auth = game.view().auth;
        let max_rounds = auth.settings().speed(rules).max_rounds();
        // The workers at this round's start, and how each spent the last round.
        let mut current: BTreeMap<(PlayerId, u64), Worker> = BTreeMap::new();
        // Every unit by its id and owner: a worker that only changed its kind
        // (embarked) is neither lost nor gained.
        let owners: BTreeMap<u64, PlayerId> = auth.units().iter_cores().map(|u| (u.id.0, u.owner)).collect();
        for &p in &seats {
            for h in auth.indexes().units_of(p) {
                let Some(unit) = auth.units().get(*h) else { continue };
                if land_worker(unit.core.kind) {
                    current.insert((p, unit.core.id.0), Worker { tile: unit.core.tile, working: unit.core.work.is_some() });
                }
            }
        }
        let bin = ((now.saturating_sub(1)) as usize * BINS / last.max(1) as usize).min(BINS - 1);
        for &p in &seats {
            let cities = auth.indexes().cities_of(p);
            let city_tiles: BTreeSet<TileId> = cities.iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.tile).collect();
            let (left, on_worked) = work_left(&game, rules, kinds, p);
            let workers = current.keys().filter(|k| k.0 == p).count() as u64;
            let units = auth.indexes().units_of(p).len();
            let upkeep = i64::from(classic_rules::derived::income::unit_upkeep_at(units, now, max_rounds) - classic_rules::derived::income::unit_upkeep_at(units - workers as usize, now, max_rounds));
            if !first {
                let b = &mut out.bins[bin];
                b.seat_rounds += 1;
                b.cities += cities.len() as u64;
                b.workers += workers;
                b.work_left += left;
                b.work_worked += on_worked;
                b.upkeep += upkeep;
                out.upkeep_total += upkeep;
                if workers * 10 > left {
                    b.surplus_rounds += 1;
                }
                for (key, was) in before.iter().filter(|(k, _)| k.0 == p) {
                    let Some(is) = current.get(key) else {
                        if owners.get(&key.1) != Some(&p) {
                            out.lost += 1;
                            // What it did: on its build at the last round start, and its
                            // last reason in the round it was taken (or the round before,
                            // when the captor moved first).
                            let last = reasons.get(&key.1).or_else(|| prev_reasons.get(&key.1)).map_or("none", String::as_str);
                            let state = if was.working { format!("building, {last}") } else { last.to_owned() };
                            *out.lost_reasons.entry(state).or_default() += 1;
                        }
                        continue;
                    };
                    // The round a build ends the worker waited on it.
                    let finished = reasons.get(&key.1).is_some_and(|r| r == "Waiting");
                    if is.working || finished || ordered.contains(&key.1) {
                        b.builds += 1;
                    } else if is.tile != was.tile {
                        b.walks += 1;
                    } else {
                        b.stands += 1;
                        if city_tiles.contains(&is.tile) {
                            b.stands_in_city += 1;
                        }
                        if left == 0 {
                            b.stands_no_work += 1;
                        }
                        let why = reasons.get(&key.1).cloned().unwrap_or_else(|| "none".to_owned());
                        *out.standing_reasons.entry(why).or_default() += 1;
                    }
                }
                let old = before.keys().filter(|k| k.0 == p).count() as u64;
                for (key, is) in current.iter().filter(|(k, _)| k.0 == p) {
                    if before.contains_key(key) || owners_before.get(&key.1) == Some(&p) {
                        continue;
                    }
                    if city_tiles.contains(&is.tile) {
                        out.built += 1;
                        out.built_at.push((left / (old + 1), on_worked / (old + 1)));
                        out.bins[bin].built += 1;
                    } else {
                        out.gained_other += 1;
                    }
                }
            }
            if let Some(i) = checkpoints.iter().position(|c| *c == now) {
                out.improved[i] += improved(&game, p);
            }
        }
        first = false;
        before = current;
        owners_before = owners;
        ordered.clear();
        if game.is_over() || now >= last {
            if now < last {
                for (i, c) in checkpoints.iter().enumerate() {
                    if *c > now {
                        for &p in &seats {
                            out.improved[i] += improved(&game, p);
                        }
                    }
                }
            }
            break;
        }
        loop {
            match driver.advance(&game, rules) {
                Advance::Worked => {}
                Advance::RoundDone | Advance::Waiting => break,
                Advance::Command(command) | Advance::EndTurn(command) => {
                    let actor = acting_unit(&command).and_then(|h| game.view().auth.units().get(h).map(|u| (u.core.id.0, u.core.kind)));
                    let result = game.execute(rules, command.clone());
                    let applied = result.is_ok();
                    driver.executed(result);
                    if let Some((id, kind)) = actor.filter(|_| applied) {
                        let builds = matches!(command, Command::BuildImprovement { .. } | Command::BuildRoad { .. } | Command::ChopForest { .. });
                        if builds && land_worker(kind) {
                            ordered.insert(id);
                            if matches!(command, Command::BuildRoad { .. }) {
                                out.road_orders += 1;
                            }
                        }
                    }
                }
            }
        }
        // The last reason the AI gave for each worker in the round just played.
        prev_reasons = std::mem::take(&mut reasons);
        let played = now;
        for entry in driver.trace().entries().filter(|e| e.round == played) {
            if let kernel::ai::trace::Actor::Unit(id) = entry.actor {
                reasons.insert(id.0, format!("{:?}", entry.reason));
            }
        }
        // A fresh trace for the next round (the bench reads nothing else of it).
        driver.set_trace_capacity(1 << 15);
    }
    out
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut suite, mut speed, mut offset, mut games, mut minds, mut jobs) = ("standard".to_owned(), "quick".to_owned(), 0u64, usize::MAX, "standard".to_owned(), 4usize);
    while let Some(flag) = args.next() {
        let value = args.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--suite" => suite = value,
            "--speed" => speed = value,
            "--offset" => offset = value.parse().expect("--offset K"),
            "--games" => games = value.parse().expect("--games N"),
            "--minds" => minds = value,
            "--jobs" => jobs = value.parse().expect("--jobs J"),
            other => panic!("unknown option {other}"),
        }
    }
    let rules = ClassicBench::rules();
    let suite = Suite::named(&suite).unwrap_or_else(|| panic!("unknown suite {suite}"));
    let def = rules.speed_named(&speed).unwrap_or_else(|| panic!("unknown speed {speed}"));
    let suite = suite.at_speed(&def.key, &def.values);
    let matches: Vec<Match> = suite.matches(Mode::Solo, offset).into_iter().take(games).collect();
    let kinds = worker_improvements(&rules);
    println!("suite {} at {speed}, offset {offset}, {} games, {} rounds\n", suite.name, matches.len(), suite.rounds);
    let mut totals = Vec::new();
    for mind in minds.split(',').map(str::trim).filter(|m| !m.is_empty()) {
        let results = parallel(&matches, jobs, |m| play(&rules, &suite, m, mind, &kinds));
        println!("### {mind}\n");
        println!("| rounds | seat-rounds | cities a seat | workers a seat | workers a city | work left a seat (turns) | on worked tiles | work left a worker | worker-turns building | walking | standing | of them in a city | standing with no work left | seats with workers > work/10 | worker upkeep a seat-round (gold) | built a seat |");
        println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
        let mut sum = Bin::default();
        for i in 0..BINS {
            let mut b = Bin::default();
            for r in &results {
                let x = &r.bins[i];
                b.seat_rounds += x.seat_rounds;
                b.cities += x.cities;
                b.workers += x.workers;
                b.builds += x.builds;
                b.walks += x.walks;
                b.stands += x.stands;
                b.stands_in_city += x.stands_in_city;
                b.stands_no_work += x.stands_no_work;
                b.work_left += x.work_left;
                b.work_worked += x.work_worked;
                b.surplus_rounds += x.surplus_rounds;
                b.upkeep += x.upkeep;
                b.built += x.built;
            }
            let n = b.seat_rounds.max(1) as f64;
            let turns = (b.builds + b.walks + b.stands).max(1) as f64;
            let from = i as u32 * suite.rounds / BINS as u32 + 1;
            let to = (i as u32 + 1) * suite.rounds / BINS as u32;
            println!(
                "| {from}–{to} | {} | {:.2} | {:.2} | {:.2} | {:.1} | {:.1} | {:.1} | {:.1} % | {:.1} % | {:.1} % | {:.1} % | {:.1} % | {:.1} % | {:.2} | {:.2} |",
                b.seat_rounds,
                b.cities as f64 / n,
                b.workers as f64 / n,
                b.workers as f64 / b.cities.max(1) as f64,
                b.work_left as f64 / n,
                b.work_worked as f64 / n,
                b.work_left as f64 / b.workers.max(1) as f64,
                100.0 * b.builds as f64 / turns,
                100.0 * b.walks as f64 / turns,
                100.0 * b.stands as f64 / turns,
                100.0 * b.stands_in_city as f64 / turns,
                100.0 * b.stands_no_work as f64 / turns,
                100.0 * b.surplus_rounds as f64 / n,
                b.upkeep as f64 / n,
                b.built as f64 * f64::from(to + 1 - from) / n,
            );
            sum.builds += b.builds;
            sum.walks += b.walks;
            sum.stands += b.stands;
            sum.stands_in_city += b.stands_in_city;
            sum.stands_no_work += b.stands_no_work;
        }
        let seats: usize = results.iter().map(|r| r.seats).sum();
        let s = seats.max(1) as f64;
        let built: u64 = results.iter().map(|r| r.built).sum();
        let other: u64 = results.iter().map(|r| r.gained_other).sum();
        let lost: u64 = results.iter().map(|r| r.lost).sum();
        let roads: u64 = results.iter().map(|r| r.road_orders).sum();
        let upkeep: i64 = results.iter().map(|r| r.upkeep_total).sum();
        let improved: Vec<f64> = (0..3).map(|i| results.iter().map(|r| r.improved[i]).sum::<u64>() as f64 / s).collect();
        let at: Vec<(u64, u64)> = results.iter().flat_map(|r| r.built_at.iter().copied()).collect();
        let median = |mut v: Vec<u64>| {
            v.sort_unstable();
            v.get(v.len() / 2).copied().unwrap_or(0)
        };
        let radius_median = median(at.iter().map(|x| x.0).collect());
        let worked_median = median(at.iter().map(|x| x.1).collect());
        let few = at.iter().filter(|x| x.0 < 28).count();
        let few_worked = at.iter().filter(|x| x.1 < 28).count();
        println!();
        println!(
            "seats {seats}: workers built {:.2} a seat, taken from the others {:.2}, taken from it {:.2}; worker-turns a seat: building {:.1}, walking {:.1}, standing {:.1} (in a city {:.1}, with no work left {:.1}); road orders {:.1}; upkeep {:.0} gold a seat; tiles improved at the checkpoints {:.1} / {:.1} / {:.1}; when a worker was built, the work left a worker (the new one counted): median {} turns in the radius ({} of {} under 28), {} on the worked tiles ({} under 28)\n",
            built as f64 / s,
            other as f64 / s,
            lost as f64 / s,
            sum.builds as f64 / s,
            sum.walks as f64 / s,
            sum.stands as f64 / s,
            sum.stands_in_city as f64 / s,
            sum.stands_no_work as f64 / s,
            roads as f64 / s,
            upkeep as f64 / s,
            improved[0],
            improved[1],
            improved[2],
            radius_median,
            few,
            at.len(),
            worked_median,
            few_worked,
        );
        let mut why: BTreeMap<String, u64> = BTreeMap::new();
        for r in &results {
            for (k, v) in &r.standing_reasons {
                *why.entry(k.clone()).or_default() += v;
            }
        }
        let mut why: Vec<(String, u64)> = why.into_iter().collect();
        why.sort_by_key(|w| std::cmp::Reverse(w.1));
        let all: u64 = why.iter().map(|w| w.1).sum();
        println!("standing worker-turns by the AI's last reason that round: {}\n", why.iter().map(|(k, v)| format!("{k} {:.1} %", 100.0 * *v as f64 / all.max(1) as f64)).collect::<Vec<_>>().join(", "));
        let mut taken: BTreeMap<String, u64> = BTreeMap::new();
        for r in &results {
            for (k, v) in &r.lost_reasons {
                *taken.entry(k.clone()).or_default() += v;
            }
        }
        let mut taken: Vec<(String, u64)> = taken.into_iter().collect();
        taken.sort_by_key(|w| std::cmp::Reverse(w.1));
        let all: u64 = taken.iter().map(|w| w.1).sum();
        println!("workers taken by what they did: {}\n", taken.iter().map(|(k, v)| format!("{k} {:.1} %", 100.0 * *v as f64 / all.max(1) as f64)).collect::<Vec<_>>().join(", "));
        let last = results.iter().map(|r| &r.bins[BINS - 1]).fold((0u64, 0u64, 0u64, 0u64), |a, b| (a.0 + b.workers, a.1 + b.seat_rounds, a.2 + b.stands, a.3 + b.builds + b.walks + b.stands));
        let late = (last.0 as f64 / last.1.max(1) as f64, 100.0 * last.2 as f64 / last.3.max(1) as f64);
        totals.push((mind.to_owned(), seats, built, other, lost, sum.stands, upkeep, improved, late));
    }
    println!("| version | seats | workers built a seat | taken from the others | taken from it | standing worker-turns a seat | workers a seat, last sixth | standing, last sixth | upkeep a seat (gold) | improved at 1/3 | 2/3 | end |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for (mind, seats, built, other, lost, stands, upkeep, improved, late) in totals {
        let s = seats.max(1) as f64;
        println!(
            "| {mind} | {seats} | {:.2} | {:.2} | {:.2} | {:.1} | {:.2} | {:.1} % | {:.0} | {:.1} | {:.1} | {:.1} |",
            built as f64 / s,
            other as f64 / s,
            lost as f64 / s,
            stands as f64 / s,
            late.0,
            late.1,
            upkeep as f64 / s,
            improved[0],
            improved[1],
            improved[2]
        );
    }
}
