//! The cities' production per AI version (classic/docs/ai-neural-architecture.md,
//! Playing): how often an order changes, how often the head of a queue
//! changes before it is built, how many items are completed.
//!
//! `cargo run --release -p aurelia-qa --bin ai-production -- [options]`
//!
//! --suite NAME     smoke|standard|duel|pace (default smoke); its solo games
//! --games N        the first N games of the suite (default all)
//! --rounds R       stop after round R (default the suite's)
//! --minds A,B      the versions, as the bench names them (default `standard,net`)
//! --jobs J         games in parallel (default 3)
//!
//! The production stockpile of a city is kept when its order changes
//! (`kernel::systems::production`): a switch costs no hammers, only the
//! turns the stockpile spends elsewhere.

use std::collections::BTreeMap;

use aurelia_qa::bench::modules::{majors, BenchRules, ClassicBench};
use aurelia_qa::bench::suite::{Match, Mode, Suite};
use aurelia_qa::export::parallel;
use classic_rules::ids::ProductionItem;
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::{Advance, StepOutcome};
use kernel::transaction::journal::{CityChange, UnitChange};

#[derive(Debug, Default, Clone, Copy)]
struct Totals {
    seats: u64,
    city_turns: u64,
    enqueues: u64,
    clears: u64,
    /// The head at a turn's end differs from the last turn's end with no
    /// item of the city completed in between.
    switches: u64,
    buildings: u64,
    units: u64,
    settlers: u64,
    workers: u64,
    net_asked: u64,
    cities_end: u64,
}

impl Totals {
    fn add(mut self, o: Totals) -> Totals {
        self.seats += o.seats;
        self.city_turns += o.city_turns;
        self.enqueues += o.enqueues;
        self.clears += o.clears;
        self.switches += o.switches;
        self.buildings += o.buildings;
        self.units += o.units;
        self.settlers += o.settlers;
        self.workers += o.workers;
        self.net_asked += o.net_asked;
        self.cities_end += o.cities_end;
        self
    }
}

fn play(rules: &CompiledRuleset, suite: &Suite, game_match: &Match, mind: &str, rounds: u32) -> Totals {
    let mut game: GameState = ClassicBench::new_game(rules, &game_match.map, game_match.seed, suite.rounds);
    let mind = ClassicBench::mind(mind).unwrap_or_else(|| panic!("no AI version {mind}"));
    let mut driver = AiDriver::with_mind(ClassicBench::settings(rules), Vec::new(), mind);
    driver.set_trace_capacity(0);
    driver.record_journal();
    driver.configure_game(&mut game);
    let seats = majors(&game);
    let mut t = Totals { seats: seats.len() as u64, ..Default::default() };
    // Per city (index, generation): the head at its owner's last turn end,
    // and whether it completed something since.
    let mut heads: BTreeMap<(u32, u32), Option<ProductionItem>> = BTreeMap::new();
    let mut completed: BTreeMap<(u32, u32), bool> = BTreeMap::new();
    let round = |game: &GameState| game.view().auth.turn().round;
    while !game.is_over() && round(&game) <= rounds {
        loop {
            let player = game.view().auth.active_player();
            let advance = driver.advance(&game, rules);
            let command = match advance {
                Advance::Worked => continue,
                Advance::RoundDone | Advance::Waiting => break,
                Advance::Command(c) | Advance::EndTurn(c) => c,
            };
            let mine = seats.contains(&player);
            if mine {
                match command {
                    Command::EnqueueProduction { .. } => t.enqueues += 1,
                    Command::ClearProduction { .. } => t.clears += 1,
                    _ => {}
                }
            }
            let end = matches!(command, Command::EndTurn { .. });
            if end && mine {
                // The player's turn ends: its cities' heads now.
                let auth = game.view().auth;
                for h in auth.indexes().cities_of(player) {
                    let Some(city) = auth.cities().get(*h) else { continue };
                    let key = (h.index, h.generation);
                    let head = city.queue.first().copied();
                    t.city_turns += 1;
                    if let Some(before) = heads.get(&key) {
                        if *before != head && before.is_some() && !completed.get(&key).copied().unwrap_or(false) {
                            t.switches += 1;
                        }
                    }
                    heads.insert(key, head);
                    completed.insert(key, false);
                }
            }
            let result = game.execute(rules, command);
            let outcome = driver.executed(result);
            let auth = game.view().auth;
            for applied in driver.take_journal() {
                for change in &applied.changes.city_changes {
                    if let CityChange::BuildingAdded { city, .. } = change {
                        if auth.cities().get(*city).is_some_and(|c| seats.contains(&c.owner)) {
                            t.buildings += 1;
                            completed.insert((city.index, city.generation), true);
                        }
                    }
                }
                for change in &applied.changes.unit_changes {
                    if let UnitChange::Created(h) = change {
                        if let Some(u) = auth.units().get(*h).filter(|u| seats.contains(&u.core.owner)) {
                            t.units += 1;
                            match kernel::ai::plan::role_of(rules, u.core.kind) {
                                kernel::ai::plan::AiRole::Settler => t.settlers += 1,
                                kernel::ai::plan::AiRole::Worker => t.workers += 1,
                                _ => {}
                            }
                            if let Some(c) = auth.indexes().cities_of(u.core.owner).iter().find(|c| auth.cities().get(**c).is_some_and(|c| c.tile == u.core.tile)) {
                                completed.insert((c.index, c.generation), true);
                            }
                        }
                    }
                }
            }
            if let StepOutcome::RoundDone = outcome {
                break;
            }
        }
    }
    t.net_asked = driver.mind_mut().net().map_or(0, |n| n.stats().decisions[2]);
    t.cities_end = seats.iter().map(|p| game.view().auth.indexes().cities_of(*p).len() as u64).sum();
    t
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
    println!("| version | seats | city-turns | net asked / city-turn | orders / city-turn | clears / city-turn | head switches before done / city-turn | buildings / seat | units / seat | settlers / seat | workers / seat | items / city-turn | cities / seat at end |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for mind in minds.split(';').map(str::trim).filter(|m| !m.is_empty()) {
        let t = parallel(&matches, jobs, |m| play(&rules, &suite, m, mind, rounds)).into_iter().fold(Totals::default(), Totals::add);
        let per = |n: u64| n as f64 / t.city_turns.max(1) as f64;
        let seat = |n: u64| n as f64 / t.seats.max(1) as f64;
        println!(
            "| {mind} | {} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.1} | {:.1} | {:.1} | {:.1} | {:.3} | {:.2} |",
            t.seats,
            t.city_turns,
            per(t.net_asked),
            per(t.enqueues),
            per(t.clears),
            per(t.switches),
            seat(t.buildings),
            seat(t.units),
            seat(t.settlers),
            seat(t.workers),
            per(t.buildings + t.units),
            seat(t.cities_end)
        );
    }
}
