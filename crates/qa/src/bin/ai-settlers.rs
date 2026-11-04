//! The lives of the settlers (classic/docs/ai-neural-architecture.md,
//! Playing): per AI version, the settlers its nations had, how long they
//! lived, how long from birth to the city, how many never founded one, how
//! far they walked.
//!
//! `cargo run --release -p aurelia-qa --bin ai-settlers -- [options]`
//!
//! --suite NAME     smoke|standard|duel|pace (default smoke); its solo games
//! --games N        the first N games of the suite (default all)
//! --rounds R       stop after round R (default the suite's)
//! --minds A,B      the versions, as the bench names them (default
//!                  `standard,net`; `net+founder`, `net:<file>+founder,builder`)
//! --jobs J         games in parallel (default 3)

use std::collections::BTreeMap;

use aurelia_qa::bench::modules::{majors, BenchRules, ClassicBench};
use aurelia_qa::bench::suite::{Match, Mode, Suite};
use aurelia_qa::export::parallel;
use classic_rules::knowledge::actions::acting_unit;
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::Advance;
use kernel::ids::EntityId;

#[derive(Debug, Default, Clone)]
struct Life {
    born: u32,
    /// Round starts the settler was alive at.
    rounds: u32,
    founded: Option<u32>,
    walked: u32,
    /// Founder at the game's start (not built).
    initial: bool,
}

#[derive(Debug, Default, Clone)]
struct GameSettlers {
    seats: usize,
    lives: BTreeMap<u64, Life>,
    cities: usize,
}

fn play(rules: &CompiledRuleset, suite: &Suite, game_match: &Match, mind: &str, rounds: u32) -> GameSettlers {
    let mut game: GameState = ClassicBench::new_game(rules, &game_match.map, game_match.seed, suite.rounds);
    let mind = ClassicBench::mind(mind).unwrap_or_else(|| panic!("no AI version {mind}"));
    let mut driver = AiDriver::with_mind(ClassicBench::settings(rules), Vec::new(), mind);
    driver.set_trace_capacity(0);
    driver.configure_game(&mut game);
    let seats = majors(&game);
    let founder = |game: &GameState, h| game.view().auth.units().get(h).filter(|u| rules.units.get(usize::from(u.core.kind.0)).is_some_and(|d| d.founder)).map(|u| (u.core.id, u.core.tile, u.core.owner));
    let mut out = GameSettlers { seats: seats.len(), ..Default::default() };
    let round = |game: &GameState| game.view().auth.turn().round;
    let start = round(&game);
    while !game.is_over() && round(&game) <= rounds {
        // The settlers alive at the round's start.
        let auth = game.view().auth;
        for &p in &seats {
            for h in auth.indexes().units_of(p) {
                if let Some((EntityId(id), _, _)) = founder(&game, *h) {
                    let life = out.lives.entry(id).or_insert_with(|| Life { born: round(&game), initial: round(&game) == start, ..Default::default() });
                    life.rounds += 1;
                }
            }
        }
        loop {
            match driver.advance(&game, rules) {
                Advance::Worked => {}
                Advance::RoundDone | Advance::Waiting => break,
                Advance::Command(command) | Advance::EndTurn(command) => {
                    let actor = acting_unit(&command).and_then(|h| founder(&game, h).map(|f| (h, f)));
                    let now = round(&game);
                    let result = game.execute(rules, command.clone());
                    let applied = result.is_ok();
                    driver.executed(result);
                    let Some((handle, (EntityId(id), from, owner))) = actor.filter(|_| applied) else { continue };
                    if !seats.contains(&owner) {
                        continue;
                    }
                    let life = out.lives.entry(id).or_insert_with(|| Life { born: now, ..Default::default() });
                    if matches!(command, Command::FoundCity { .. }) {
                        life.founded = Some(now);
                    } else if let Some((_, to, _)) = founder(&game, handle) {
                        life.walked += game.view().auth.map().dimensions.distance(from, to).unwrap_or(0) as u32;
                    }
                }
            }
        }
    }
    out.cities = seats.iter().map(|p| game.view().auth.indexes().cities_of(*p).len()).sum();
    out
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut suite, mut games, mut rounds, mut minds, mut jobs) = ("smoke".to_owned(), usize::MAX, None, "standard,net".to_owned(), 3usize);
    while let Some(flag) = args.next() {
        let value = args.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--suite" => suite = value,
            "--games" => games = value.parse().expect("--games N"),
            "--rounds" => rounds = Some(value.parse().expect("--rounds R")),
            "--minds" => minds = value,
            "--jobs" => jobs = value.parse().expect("--jobs J"),
            other => panic!("unknown option {other}"),
        }
    }
    let suite = Suite::named(&suite).unwrap_or_else(|| panic!("unknown suite {suite}"));
    let rules = ClassicBench::rules();
    let matches: Vec<Match> = suite.matches(Mode::Solo, 0).into_iter().take(games).collect();
    let rounds = rounds.unwrap_or(suite.rounds);
    println!("suite {}, {} games, rounds ≤ {rounds}\n", suite.name, matches.len());
    println!("| version | seats | settlers (built) | founded | never founded | rounds alive, mean | rounds birth → city, mean | median | tiles walked a settler | tiles walked by a settler that never founded | cities a seat |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for mind in split_minds(&minds) {
        let results = parallel(&matches, jobs, |m| play(&rules, &suite, m, &mind, rounds));
        let seats: usize = results.iter().map(|r| r.seats).sum();
        let lives: Vec<&Life> = results.iter().flat_map(|r| r.lives.values()).collect();
        let built = lives.iter().filter(|l| !l.initial).count();
        let founded: Vec<u32> = lives.iter().filter_map(|l| l.founded.map(|f| f - l.born)).collect();
        let never = lives.iter().filter(|l| l.founded.is_none()).count();
        let mean = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
        let alive = mean(&lives.iter().map(|l| f64::from(l.rounds)).collect::<Vec<_>>());
        let to_city = mean(&founded.iter().map(|d| f64::from(*d)).collect::<Vec<_>>());
        let mut sorted = founded.clone();
        sorted.sort_unstable();
        let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0);
        let walked = mean(&lives.iter().map(|l| f64::from(l.walked)).collect::<Vec<_>>());
        let walked_never = mean(&lives.iter().filter(|l| l.founded.is_none()).map(|l| f64::from(l.walked)).collect::<Vec<_>>());
        let cities = results.iter().map(|r| r.cities).sum::<usize>() as f64 / seats.max(1) as f64;
        println!(
            "| {mind} | {seats} | {} ({built}) | {} | {never} | {alive:.1} | {to_city:.1} | {median} | {walked:.1} | {walked_never:.1} | {cities:.2} |",
            lives.len(),
            founded.len()
        );
    }
}

/// The versions of `--minds`: a comma starts a new version unless the word
/// after it is a unit role (`net+founder,builder`).
fn split_minds(list: &str) -> Vec<String> {
    let roles = classic_rules::ai::net_e2e::mind::ScriptedRoles::NAMES;
    let mut out: Vec<String> = Vec::new();
    for word in list.split(',').map(str::trim).filter(|w| !w.is_empty()) {
        match out.last_mut() {
            Some(last) if last.contains('+') && roles.contains(&word) => {
                last.push(',');
                last.push_str(word);
            }
            _ => out.push(word.to_owned()),
        }
    }
    out
}
