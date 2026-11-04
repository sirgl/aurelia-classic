//! Two audits of a mind's turns, measured from its commands and the game
//! they change (classic/docs/ai-allocator.md «Idle cities», classic/docs/ai-scouting.md
//! «The opening»):
//!
//! - **idle cities**: at each of its turn's ends (the `EndTurn` command,
//!   before the cities produce) a city whose queue is empty and which runs no
//!   process builds nothing this turn; its hammers go to the city's stock
//!   (`kernel::systems::production`: the stock is kept and spent on the next
//!   order, so they are delayed, not lost; a capture zeroes it). Next to it
//!   the share a replay frame shows empty (`frames.rs` snapshots the queue
//!   at the round's boundary, after the cities produced), and the reasons the
//!   mind gave for the idle city that turn (its decision trace);
//! - **the opening's scouting**: the units the seat starts with, the round
//!   it gets its first recon unit, what its first military units do in the
//!   first rounds (a turn with an `Explore` decision, a turn ended in one of
//!   its cities, else the field), the share of the map it has explored, the
//!   nations and city-states it has met, the ruins it has claimed and the
//!   rivals' capitals it has seen, at the checkpoint rounds.
//!
//! `cargo run --release -p aurelia-qa --bin ai-opening -- [options]`
//!
//! --suite NAME     smoke|standard|duel|pace (default standard); its solo games
//! --games N        the first N games of the suite (default all)
//! --rounds R       stop after round R (default the suite's)
//! --speed NAME     the game speed (default quick; the suite's rounds at it)
//! --minds A;B      the minds, as the bench names them (default `standard`)
//! --checks a,b,c   the scouting checkpoints (default 10,20,30)
//! --jobs J         games in parallel (default 3)

use std::collections::{BTreeMap, BTreeSet};

use aurelia_qa::bench::modules::{majors, BenchRules, ClassicBench};
use aurelia_qa::bench::suite::{Match, Mode, Suite};
use aurelia_qa::export::parallel;
use classic_rules::classic::ClassicChange;
use classic_rules::rules::content::{ImprovementKind, UnitRole};
use classic_rules::world::player::ClassicKind;
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::trace::{Actor, Reason};
use kernel::ai::{Advance, StepOutcome};
use kernel::ids::{EntityId, PlayerId, TileId};
use kernel::transaction::journal::UnitChange;
use kernel::world::player::PlayerKind;

const TRACE: usize = 1 << 16;

#[derive(Debug, Default, Clone)]
struct Idle {
    city_turns: u64,
    /// Empty queue and no process at the turn's end.
    idle: u64,
    /// Empty queue with a process (its hammers become gold, science or a
    /// league project) at the turn's end.
    processing: u64,
    /// Hammers that went to the stock of an idle city.
    idle_hammers: u64,
    /// All hammers of the cities at their turn's ends.
    hammers: u64,
    /// Idle city-turns by the reasons the mind traced for the city that turn.
    reasons: BTreeMap<String, u64>,
    /// Idle city-turns by the city's last completion: the item completed at
    /// the previous turn's end, or older.
    after_completion: u64,
    /// Idle stretches (consecutive idle turns of a city) by length.
    stretches: BTreeMap<u32, u64>,
    /// Cities seen at a round's boundary (a replay frame) and those empty there.
    frame_cities: u64,
    frame_empty: u64,
    /// Stock left in cities at the end (hammers of idle turns not yet spent).
    stock_end: u64,
    /// Idle city-turns by situation: the round's tenth of the game, the
    /// processes the rules open to the player (none, one, both), the city's size.
    situations: BTreeMap<String, u64>,
}

impl Idle {
    fn add(mut self, o: &Idle) -> Idle {
        self.city_turns += o.city_turns;
        self.idle += o.idle;
        self.processing += o.processing;
        self.idle_hammers += o.idle_hammers;
        self.hammers += o.hammers;
        for (k, v) in &o.reasons {
            *self.reasons.entry(k.clone()).or_default() += v;
        }
        self.after_completion += o.after_completion;
        for (k, v) in &o.stretches {
            *self.stretches.entry(*k).or_default() += v;
        }
        self.frame_cities += o.frame_cities;
        self.frame_empty += o.frame_empty;
        self.stock_end += o.stock_end;
        for (k, v) in &o.situations {
            *self.situations.entry(k.clone()).or_default() += v;
        }
        self
    }
}

/// One seat's opening.
#[derive(Debug, Default, Clone)]
struct Seat {
    start_units: Vec<String>,
    start_recon: bool,
    /// The round of its first recon unit built or bought.
    recon_round: Option<u32>,
    /// Unit-turns of the starting military units in the first checkpoint
    /// rounds: exploring, in a city, elsewhere.
    explore: u32,
    in_city: u32,
    field: u32,
    /// The farthest a starting military unit went from the first city.
    farthest: i32,
    /// At each checkpoint: explored percent of the map, nations met,
    /// city-states met, ruins claimed, rival capitals seen.
    at: Vec<[u32; 5]>,
    first_major_met: Option<u32>,
    first_cs_met: Option<u32>,
}

#[derive(Debug, Default, Clone)]
struct Game {
    idle: Idle,
    seats: Vec<Seat>,
    ruins_on_map: u32,
    city_states: u32,
}

fn play(rules: &CompiledRuleset, suite: &Suite, game_match: &Match, mind: &str, rounds: u32, checks: &[u32]) -> Game {
    let mut game: GameState = ClassicBench::new_game_at(rules, &game_match.map, game_match.seed, suite.rounds, &suite.speed);
    let mind = ClassicBench::mind(mind).unwrap_or_else(|| panic!("no AI version {mind}"));
    let mut driver = AiDriver::with_mind(ClassicBench::settings(rules), Vec::new(), mind);
    driver.set_trace_capacity(TRACE);
    driver.record_journal();
    driver.configure_game(&mut game);
    let seats = majors(&game);
    let auth = game.view().auth;
    let ruins = rules.improvements.iter().position(|d| d.kind == ImprovementKind::AncientRuins).map(|i| i as u16);
    let tiles = auth.map().dimensions.tile_count() as u32;
    let mut out = Game {
        ruins_on_map: (0..tiles).filter(|t| auth.map().improvement(TileId(*t)).flatten().is_some_and(|i| Some(i.0) == ruins)).count() as u32,
        city_states: (0..auth.players().count() as u8).filter(|p| matches!(auth.players().kind_of(PlayerId(*p)), PlayerKind::Module(ClassicKind::CityState(_)))).count() as u32,
        ..Default::default()
    };
    let recon = |kind: kernel::ids::UnitKindId| rules.units.get(kind.0 as usize).and_then(|d| d.role) == Some(UnitRole::Recon);
    let civilian = |kind: kernel::ids::UnitKindId| rules.units.get(kind.0 as usize).is_some_and(|d| d.strength == 0 && d.ranged_strength == 0);
    // The starting military units and the first city of each seat.
    let mut starters: BTreeMap<PlayerId, BTreeSet<EntityId>> = BTreeMap::new();
    let mut seat_of: BTreeMap<PlayerId, Seat> = BTreeMap::new();
    for &p in &seats {
        let mut s = Seat::default();
        for h in auth.indexes().units_of(p) {
            let Some(u) = auth.units().get(*h) else { continue };
            let name = rules.units.get(u.core.kind.0 as usize).map_or_else(String::new, |d| d.name.clone());
            s.start_units.push(name);
            s.start_recon |= recon(u.core.kind);
            if !civilian(u.core.kind) {
                starters.entry(p).or_default().insert(u.core.id);
            }
        }
        if s.start_recon {
            s.recon_round = Some(0);
        }
        seat_of.insert(p, s);
    }
    let mut ruins_claimed: BTreeMap<PlayerId, u32> = BTreeMap::new();
    // Per city id: idle at the last turn end (run length), completed at the last turn end.
    let mut run: BTreeMap<EntityId, u32> = BTreeMap::new();
    let mut completed_last: BTreeSet<EntityId> = BTreeSet::new();
    let mut last_round = 0;
    let mut checked = 0usize;
    let round = |game: &GameState| game.view().auth.turn().round;
    let check_at = |game: &GameState, seat_of: &mut BTreeMap<PlayerId, Seat>, ruins_claimed: &BTreeMap<PlayerId, u32>| {
        let auth = game.view().auth;
        let players = auth.players();
        for &p in &seats {
            let k = &auth.knowledge()[p.0 as usize];
            let explored = (0..tiles).filter(|t| k.is_explored(TileId(*t))).count() as u32;
            let (mut majors_met, mut cs_met) = (0, 0);
            for q in 0..players.count() as u8 {
                let q = PlayerId(q);
                if q == p || k.contact(q).met_round.is_none() {
                    continue;
                }
                match players.kind_of(q) {
                    PlayerKind::Major => majors_met += 1,
                    PlayerKind::Module(ClassicKind::CityState(_)) => cs_met += 1,
                    _ => {}
                }
            }
            let capitals = ClassicBench::rival_capitals(game, rules, p).iter().filter(|c| c.1).count() as u32;
            let s = seat_of.get_mut(&p).expect("seat");
            s.at.push([explored * 1000 / tiles.max(1), majors_met, cs_met, ruins_claimed.get(&p).copied().unwrap_or(0), capitals]);
        }
    };
    while !game.is_over() && round(&game) <= rounds {
        loop {
            let player = game.view().auth.active_player();
            let advance = driver.advance(&game, rules);
            let command = match advance {
                Advance::Worked => continue,
                Advance::RoundDone | Advance::Waiting => break,
                Advance::Command(c) | Advance::EndTurn(c) => c,
            };
            let now = round(&game);
            if now != last_round {
                // A round's boundary: what a replay frame shows; the checkpoints.
                while checked < checks.len() && now > checks[checked] {
                    check_at(&game, &mut seat_of, &ruins_claimed);
                    checked += 1;
                }
                let auth = game.view().auth;
                for &p in &seats {
                    for h in auth.indexes().cities_of(p) {
                        let Some(c) = auth.cities().get(*h) else { continue };
                        out.idle.frame_cities += 1;
                        out.idle.frame_empty += u64::from(c.queue.is_empty());
                    }
                }
                last_round = now;
            }
            let mine = seats.contains(&player);
            let end = matches!(command, Command::EndTurn { .. });
            // Before the turn ends: the cities' queues and stocks.
            let mut before: Vec<(EntityId, kernel::ids::CityHandle, bool, u32)> = Vec::new();
            if end && mine {
                let auth = game.view().auth;
                let mut traced: BTreeMap<EntityId, Vec<Reason>> = BTreeMap::new();
                let mut explored: BTreeSet<EntityId> = BTreeSet::new();
                for e in driver.trace().entries().filter(|e| e.player == player) {
                    match e.actor {
                        Actor::City(id) => traced.entry(id).or_default().push(e.reason),
                        Actor::Unit(id) if e.reason == Reason::Explore => {
                            explored.insert(id);
                        }
                        _ => {}
                    }
                }
                for h in auth.indexes().cities_of(player) {
                    let Some(c) = auth.cities().get(*h) else { continue };
                    let idle = c.queue.is_empty() && c.economy.process.is_none();
                    before.push((c.id, *h, idle, c.production));
                    out.idle.city_turns += 1;
                    out.idle.processing += u64::from(c.queue.is_empty() && c.economy.process.is_some());
                    if idle {
                        out.idle.idle += 1;
                        let reasons = traced.get(&c.id).map_or_else(|| "no city decision".to_owned(), |r| {
                            let mut r: Vec<String> = r.iter().map(|x| format!("{x:?}")).collect();
                            r.dedup();
                            r.join("+")
                        });
                        *out.idle.reasons.entry(reasons).or_default() += 1;
                        out.idle.after_completion += u64::from(completed_last.contains(&c.id));
                        let techs = auth.players().techs_of(player);
                        let open = [kernel::rules::roles::ProcessRole::Wealth, kernel::rules::roles::ProcessRole::Research]
                            .into_iter()
                            .filter(|r| rules.roles.process_tech(*r).is_some_and(|t| techs.contains(&t)))
                            .count();
                        let size = match c.population { 0..=2 => "size 1-2", 3..=5 => "size 3-5", _ => "size 6+" };
                        let key = format!("game tenth {}, {open} process(es) open, {size}", now * 10 / rounds.max(1));
                        *out.idle.situations.entry(key).or_default() += 1;
                    }
                }
                // The starting military units in the scouting window.
                if checks.last().is_some_and(|last| now <= *last) {
                    let first_city = auth.indexes().cities_of(player).first().and_then(|h| auth.cities().get(*h)).map(|c| c.tile);
                    let dims = auth.map().dimensions;
                    let s = seat_of.get_mut(&player).expect("seat");
                    for h in auth.indexes().units_of(player) {
                        let Some(u) = auth.units().get(*h) else { continue };
                        if !starters.get(&player).is_some_and(|ids| ids.contains(&u.core.id)) {
                            continue;
                        }
                        let in_city = auth.indexes().cities_of(player).iter().any(|c| auth.cities().get(*c).is_some_and(|c| c.tile == u.core.tile));
                        if explored.contains(&u.core.id) {
                            s.explore += 1;
                        } else if in_city {
                            s.in_city += 1;
                        } else {
                            s.field += 1;
                        }
                        if let Some(home) = first_city {
                            s.farthest = s.farthest.max(dims.distance(home, u.core.tile).unwrap_or(0));
                        }
                    }
                }
                completed_last.retain(|id| !before.iter().any(|b| b.0 == *id));
            }
            let result = game.execute(rules, command);
            let outcome = driver.executed(result);
            let auth = game.view().auth;
            for applied in driver.take_journal() {
                for change in &applied.changes.module_changes {
                    if let ClassicChange::RuinsExplored { player, .. } = change {
                        *ruins_claimed.entry(*player).or_default() += 1;
                    }
                }
                for change in &applied.changes.unit_changes {
                    if let UnitChange::Created(h) = change {
                        if let Some(u) = auth.units().get(*h).filter(|u| seats.contains(&u.core.owner)) {
                            if recon(u.core.kind) {
                                let s = seat_of.get_mut(&u.core.owner).expect("seat");
                                s.recon_round.get_or_insert(round(&game));
                            }
                        }
                    }
                }
            }
            if end && mine {
                // The turn's production: hammers into idle cities' stocks, completions.
                let obs = game.ai_observation(rules, player);
                let rates: BTreeMap<EntityId, u64> = obs.own_cities.iter().map(|o| (o.id, o.income.production.max(0) as u64)).collect();
                for (id, h, idle, stock) in before {
                    let Some(c) = auth.cities().get(h) else {
                        run.remove(&id);
                        continue;
                    };
                    out.idle.hammers += rates.get(&id).copied().unwrap_or(0);
                    if idle {
                        out.idle.idle_hammers += u64::from(c.production.saturating_sub(stock));
                        *run.entry(id).or_default() += 1;
                    } else {
                        if let Some(len) = run.remove(&id).filter(|l| *l > 0) {
                            *out.idle.stretches.entry(len).or_default() += 1;
                        }
                        if c.production < stock || c.queue.is_empty() {
                            completed_last.insert(id);
                        }
                    }
                }
                driver.set_trace_capacity(TRACE);
            }
            if let StepOutcome::RoundDone = outcome {
                break;
            }
        }
    }
    while checked < checks.len() {
        check_at(&game, &mut seat_of, &ruins_claimed);
        checked += 1;
    }
    for len in run.values().filter(|l| **l > 0) {
        *out.idle.stretches.entry(*len).or_default() += 1;
    }
    let auth = game.view().auth;
    out.idle.stock_end = seats.iter().flat_map(|p| auth.indexes().cities_of(*p).iter().filter_map(|h| auth.cities().get(*h)).filter(|c| c.queue.is_empty()).map(|c| u64::from(c.production)).collect::<Vec<_>>()).sum();
    out.seats = seats.iter().map(|p| seat_of.remove(p).unwrap_or_default()).collect();
    for s in &mut out.seats {
        s.first_major_met = None;
        s.first_cs_met = None;
    }
    // The first meetings' rounds.
    let players = auth.players();
    for (i, &p) in seats.iter().enumerate() {
        let k = &auth.knowledge()[p.0 as usize];
        for q in 0..players.count() as u8 {
            let q = PlayerId(q);
            let Some(met) = k.contact(q).met_round.filter(|_| q != p) else { continue };
            let slot = match players.kind_of(q) {
                PlayerKind::Major => &mut out.seats[i].first_major_met,
                PlayerKind::Module(ClassicKind::CityState(_)) => &mut out.seats[i].first_cs_met,
                _ => continue,
            };
            *slot = Some(slot.map_or(met, |m| m.min(met)));
        }
    }
    out
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        f64::NAN
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut suite, mut games, mut rounds, mut minds, mut jobs, mut speed, mut checks) =
        ("standard".to_owned(), usize::MAX, None, "standard".to_owned(), 3usize, "quick".to_owned(), vec![10u32, 20, 30]);
    while let Some(flag) = args.next() {
        let value = args.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--suite" => suite = value,
            "--games" => games = value.parse().expect("--games N"),
            "--rounds" => rounds = Some(value.parse().expect("--rounds R")),
            "--minds" => minds = value,
            "--jobs" => jobs = value.parse().expect("--jobs J"),
            "--speed" => speed = value,
            "--checks" => checks = value.split(',').map(|c| c.trim().parse().expect("--checks a,b,c")).collect(),
            other => panic!("unknown option {other}"),
        }
    }
    let rules = ClassicBench::rules();
    let suite = Suite::named(&suite).unwrap_or_else(|| panic!("unknown suite {suite}"));
    let def = rules.speed_named(&speed).unwrap_or_else(|| panic!("unknown speed {speed}"));
    let suite = suite.at_speed(&def.key, &def.values);
    let matches: Vec<Match> = suite.matches(Mode::Solo, 0).into_iter().take(games).collect();
    let rounds = rounds.unwrap_or(suite.rounds);
    println!("suite {} at {speed}, {} games, rounds ≤ {rounds}, checkpoints {checks:?}\n", suite.name, matches.len());
    for mind in minds.split(';').map(str::trim).filter(|m| !m.is_empty()) {
        let played = parallel(&matches, jobs, |m| play(&rules, &suite, m, mind, rounds, &checks));
        let idle = played.iter().fold(Idle::default(), |a, g| a.add(&g.idle));
        let seats: Vec<&Seat> = played.iter().flat_map(|g| &g.seats).collect();
        let n = seats.len().max(1) as f64;
        println!("## {mind}: {} seats", seats.len());
        let pct = |a: u64, b: u64| 100.0 * a as f64 / b.max(1) as f64;
        println!(
            "idle: {} of {} city-turns end empty without a process ({:.2} %); their hammers to the stock {} of {} ({:.2} %); \
             a frame shows {:.1} % of cities empty; idle right after a completion {:.1} %; stock left in empty cities at the end {}",
            idle.idle,
            idle.city_turns,
            pct(idle.idle, idle.city_turns),
            idle.idle_hammers,
            idle.hammers,
            pct(idle.idle_hammers, idle.hammers),
            pct(idle.frame_empty, idle.frame_cities),
            pct(idle.after_completion, idle.idle),
            idle.stock_end
        );
        let mut reasons: Vec<_> = idle.reasons.iter().collect();
        reasons.sort_by(|a, b| b.1.cmp(a.1));
        println!("empty queue with a process at the turn's end: {} ({:.2} %)", idle.processing, pct(idle.processing, idle.city_turns));
        println!("idle reasons: {}", reasons.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", "));
        println!("idle situations: {}", idle.situations.iter().map(|(k, v)| format!("[{k}] {v}")).collect::<Vec<_>>().join("; "));
        println!("idle stretches (turns: count): {}", idle.stretches.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(" "));
        let mut starts: BTreeMap<String, u32> = BTreeMap::new();
        for s in &seats {
            *starts.entry(s.start_units.join("+")).or_default() += 1;
        }
        println!("start units: {starts:?}");
        let recon: Vec<f64> = seats.iter().filter_map(|s| s.recon_round).map(f64::from).collect();
        let last = *checks.last().unwrap_or(&30);
        let recon_by = seats.iter().filter(|s| s.recon_round.is_some_and(|r| r <= last)).count();
        println!("first recon unit: {:.0} % of seats by round {last} (mean round of those that have one {:.1}, {} of {} ever)", 100.0 * recon_by as f64 / n, mean(&recon), recon.len(), seats.len());
        let (e, c, f) = seats.iter().fold((0, 0, 0), |a, s| (a.0 + s.explore, a.1 + s.in_city, a.2 + s.field));
        let t = f64::from((e + c + f).max(1));
        println!(
            "starting military units, unit-turns to round {last}: explore {:.1} %, in a city {:.1} %, elsewhere {:.1} %; farthest from the first city {:.1} tiles (mean)",
            100.0 * f64::from(e) / t,
            100.0 * f64::from(c) / t,
            100.0 * f64::from(f) / t,
            mean(&seats.iter().map(|s| f64::from(s.farthest)).collect::<Vec<_>>())
        );
        println!("| round | explored % of map | nations met | city-states met | ruins claimed | rival capitals seen |");
        println!("|---:|---:|---:|---:|---:|---:|");
        for (i, r) in checks.iter().enumerate() {
            let col = |j: usize| mean(&seats.iter().filter_map(|s| s.at.get(i)).map(|a| f64::from(a[j])).collect::<Vec<_>>());
            println!("| {r} | {:.1} | {:.2} | {:.2} | {:.2} | {:.2} |", col(0) / 10.0, col(1), col(2), col(3), col(4));
        }
        let met = |f: &dyn Fn(&Seat) -> Option<u32>| {
            let xs: Vec<f64> = seats.iter().filter_map(|s| f(s)).map(f64::from).collect();
            format!("{:.1} ({} of {} seats)", mean(&xs), xs.len(), seats.len())
        };
        println!("first nation met, round: {}; first city-state met, round: {}", met(&|s| s.first_major_met), met(&|s| s.first_cs_met));
        let ruins: u32 = played.iter().map(|g| g.ruins_on_map).sum();
        let cs: u32 = played.iter().map(|g| g.city_states).sum();
        println!("ruins on the maps at the start {:.1} per game, city-states {:.1} per game\n", f64::from(ruins) / played.len().max(1) as f64, f64::from(cs) / played.len().max(1) as f64);
    }
}
