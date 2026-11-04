//! The marches of the war operations of a bench suite's games: how near each army came to
//! its target, whether it ever saw it, and whether it stalled out of its sight
//! (classic/docs/ai-books.md «The march into the fog»).
//!
//! `cargo run --release -p aurelia-qa --bin ai-march -- [options]`
//!
//! --suite NAME         the bench suite (default duel-land)
//! --speed NAME         the game speed (default quick)
//! --candidate X        the candidate's variant (default standard)
//! --baseline Y         the baseline's variant; without it every seat plays the candidate
//! --seed-offset K      the suite's seeds moved by K strides, as the bench's (default 0)
//! --games N            only the first N games of the suite
//! --jobs N             games in parallel (default 4)
//! --stall R            rounds without the nearest member coming closer, out of the
//!                      target's sight, that make a stall (default 10)
//! --out FILE           one CSV row per march
//!
//! A march is one operation's run of marching rounds (`war_probe::MarchSample`), split where
//! the player's operation against the target stopped for more than two rounds (a book forms
//! its operation again at war: one attack). Its side is the seat's (candidate or baseline).

use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::play::{play_match, Pairing};
use aurelia_qa::bench::suite::{Match, Mode, Side, Suite};
use classic_rules::ai::war_probe;

struct Options {
    suite: String,
    speed: String,
    candidate: String,
    baseline: Option<String>,
    offset: u64,
    games: Option<usize>,
    jobs: usize,
    stall: u32,
    out: Option<String>,
}

fn options() -> Result<Options, String> {
    let mut o = Options { suite: "duel-land".into(), speed: "quick".into(), candidate: "standard".into(), baseline: None, offset: 0, games: None, jobs: 4, stall: 10, out: None };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        let number = |v: String| v.parse::<u64>().map_err(|e| format!("{e}"));
        match flag.as_str() {
            "--suite" => o.suite = value()?,
            "--speed" => o.speed = value()?,
            "--candidate" => o.candidate = value()?,
            "--baseline" => o.baseline = Some(value()?),
            "--seed-offset" => o.offset = number(value()?)?,
            "--games" => o.games = Some(number(value()?)? as usize),
            "--jobs" => o.jobs = number(value()?)? as usize,
            "--stall" => o.stall = number(value()?)? as u32,
            "--out" => o.out = Some(value()?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(o)
}

fn main() {
    if let Err(e) = options().and_then(|o| play(&o)) {
        eprintln!("ai-march: {e}");
        std::process::exit(1);
    }
}

/// One march: an operation's run of marching rounds.
#[derive(Debug, Clone)]
struct March {
    game: String,
    seed: u64,
    side: Side,
    player: u8,
    target: u32,
    first: u32,
    last: u32,
    /// The nearest member's distance at the first round, its least, and at the last round.
    start: i32,
    closest: i32,
    end: i32,
    /// The target seen in any round of the march, and the first such round.
    sighted: Option<u32>,
    /// The longest run of rounds out of the target's sight in which the nearest member
    /// came no closer than at the run's start, and the distance it stood at.
    stuck: u32,
    stuck_at: i32,
    /// The cities of the enemy the player took from the march's start to three rounds
    /// after its end.
    took: u32,
}

impl March {
    fn stalled(&self, rounds: u32) -> bool {
        self.sighted.is_none() && self.stuck >= rounds
    }
}

fn marches(game: &Match, sides: &[(u8, Side)], probe: &war_probe::WarProbe) -> Vec<March> {
    let mut samples = probe.marches.clone();
    samples.sort_by_key(|s| (s.player.0, s.target, s.round));
    let mut out: Vec<March> = Vec::new();
    let mut runs: Vec<Vec<war_probe::MarchSample>> = Vec::new();
    for s in samples {
        match runs.last_mut() {
            Some(run) if run.last().is_some_and(|l| l.player == s.player && l.target == s.target && s.round <= l.round + 3) => run.push(s),
            _ => runs.push(vec![s]),
        }
    }
    for run in runs {
        let (first, last) = (run[0], run[run.len() - 1]);
        let side = sides.iter().find(|(p, _)| *p == first.player.0).map_or(Side::Candidate, |(_, s)| *s);
        let distances: Vec<i32> = run.iter().map(|s| s.nearest).filter(|d| *d >= 0).collect();
        let (mut stuck, mut stuck_at) = (0u32, -1);
        let mut anchor: Option<(u32, i32)> = None;
        for s in run.iter().filter(|s| s.nearest >= 0) {
            if s.sighted {
                anchor = None;
                continue;
            }
            match anchor {
                Some((from, at)) if s.nearest >= at => {
                    let length = s.round - from;
                    if length > stuck {
                        (stuck, stuck_at) = (length, at);
                    }
                }
                _ => anchor = Some((s.round, s.nearest)),
            }
        }
        let took = probe.captures.iter().filter(|(round, player, _, from)| *player == first.player && *from == Some(first.enemy) && *round >= first.round && *round <= last.round + 3).count() as u32;
        out.push(March {
            game: game.label(),
            seed: game.seed,
            side,
            player: first.player.0,
            target: first.target,
            first: first.round,
            last: last.round,
            start: distances.first().copied().unwrap_or(-1),
            closest: distances.iter().copied().min().unwrap_or(-1),
            end: distances.last().copied().unwrap_or(-1),
            sighted: run.iter().find(|s| s.sighted).map(|s| s.round),
            stuck,
            stuck_at,
            took,
        });
    }
    out
}

fn play(o: &Options) -> Result<(), String> {
    let rules = ClassicBench::rules();
    let suite = Suite::named(&o.suite).ok_or("unknown suite")?;
    let def = rules.speed_named(&o.speed).ok_or_else(|| format!("unknown speed {}", o.speed))?;
    let suite = suite.at_speed(&def.key, &def.values);
    let mode = if o.baseline.is_some() { Mode::Versus } else { Mode::Solo };
    let mut games = suite.matches(mode, o.offset);
    if let Some(n) = o.games {
        games.truncate(n);
    }
    let pairing = Pairing { baseline: o.baseline.clone().unwrap_or_else(|| o.candidate.clone()), candidate: o.candidate.clone() };
    let next = AtomicUsize::new(0);
    let found: Mutex<Vec<(usize, Vec<March>)>> = Mutex::new(Vec::new());
    let started = std::time::Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..o.jobs.clamp(1, games.len().max(1)) {
            scope.spawn(|| {
                let rules = ClassicBench::rules();
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(game) = games.get(at) else { break };
                    war_probe::enable(true);
                    let result = play_match::<ClassicBench>(&rules, &suite, game, &pairing);
                    let probe = war_probe::take();
                    war_probe::enable(false);
                    let result = match result {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("{e}");
                            continue;
                        }
                    };
                    let sides: Vec<(u8, Side)> = result.seats.iter().map(|s| (s.player.0, s.side)).collect();
                    let list = marches(game, &sides, &probe);
                    eprintln!("{} {} marches, {:.0} s", game.label(), list.len(), started.elapsed().as_secs_f64());
                    found.lock().expect("marches").push((at, list));
                }
            });
        }
    });
    let mut found = found.into_inner().map_err(|e| e.to_string())?;
    found.sort_by_key(|(at, _)| *at);
    let all: Vec<March> = found.into_iter().flat_map(|(_, m)| m).collect();
    println!("{} games of {} at {} ({} vs {}), offset {}, stall {} rounds", games.len(), o.suite, o.speed, o.candidate, pairing.baseline, o.offset, o.stall);
    println!("side       marches  sighted    unsighted  stalled    took a city  median closest  median stuck-at");
    for side in [Side::Candidate, Side::Baseline] {
        if o.baseline.is_none() && side == Side::Baseline {
            continue;
        }
        let of: Vec<&March> = all.iter().filter(|m| m.side == side).collect();
        let n = of.len().max(1) as f64;
        let share = |k: usize| format!("{k} ({:.0} %)", 100.0 * k as f64 / n);
        let sighted = of.iter().filter(|m| m.sighted.is_some()).count();
        let stalled: Vec<&&March> = of.iter().filter(|m| m.stalled(o.stall)).collect();
        let took = of.iter().filter(|m| m.took > 0).count();
        let median = |mut v: Vec<i32>| {
            v.sort();
            v.get(v.len() / 2).map_or("-".to_owned(), |x| x.to_string())
        };
        println!(
            "{:<10} {:>7}  {:>9}  {:>9}  {:>9}  {:>11}  {:>14}  {:>15}",
            format!("{side:?}").to_lowercase(),
            of.len(),
            share(sighted),
            share(of.len() - sighted),
            share(stalled.len()),
            share(took),
            median(of.iter().map(|m| m.closest).collect()),
            median(stalled.iter().map(|m| m.stuck_at).collect()),
        );
    }
    println!("{:.0} s", started.elapsed().as_secs_f64());
    if let Some(path) = &o.out {
        let mut text = String::from("game,seed,side,player,target,first,last,start,closest,end,sighted,stuck,stuck_at,stalled,took\n");
        for m in &all {
            let sighted = m.sighted.map_or(String::new(), |r| r.to_string());
            text.push_str(&format!(
                "{},{},{:?},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                m.game, m.seed, m.side, m.player, m.target, m.first, m.last, m.start, m.closest, m.end, sighted, m.stuck, m.stuck_at, m.stalled(o.stall), m.took
            ));
        }
        std::fs::File::create(path).and_then(|mut f| f.write_all(text.as_bytes())).map_err(|e| format!("{path}: {e}"))?;
        println!("wrote {path}");
    }
    Ok(())
}
