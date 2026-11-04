//! Traced games of the strategy net (`classic_rules::ai::strategy_net`,
//! classic/docs/ai-strategy-net.md): one bench match with the strategy net on
//! the candidate seats, every review of its intent and goals, its research,
//! culture and production choices printed as they happen, then the seats'
//! final standing. Also prints the net's weight count.
//!
//! `cargo run --release -p aurelia-qa --bin ai-strategy-trace -- [options]`
//!
//! --suite NAME         the bench suite (default standard)
//! --speed NAME         the game speed (default quick)
//! --map SCRIPT         the map script (default pangaea)
//! --size NAME          the map size (default small)
//! --seed N             the seed (default 1)
//! --seating even|odd   which seats are the candidate's (default even)
//! --candidate NAME     the variant on the candidate seats (default strategy-net)
//! --baseline NAME      the variant on the others (default standard)
//! --only KINDS         the kinds of lines printed, comma-separated: review,
//!                      research, policy, build (default all)

use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::bench::play::{play_match, Pairing};
use aurelia_qa::bench::suite::{MapSpec, Match, Seating, Suite};
use classic_rules::ai::strategy_net::{trace, weights};

fn main() {
    if let Err(e) = run() {
        eprintln!("ai-strategy-trace: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut suite_name = "standard".to_owned();
    let mut speed = "quick".to_owned();
    let mut script = "pangaea".to_owned();
    let mut size = "small".to_owned();
    let mut seed = 1u64;
    let mut seating = Seating::Even;
    let mut candidate = "strategy-net".to_owned();
    let mut baseline = "standard".to_owned();
    let mut only: Option<Vec<String>> = None;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--suite" => suite_name = value()?,
            "--speed" => speed = value()?,
            "--map" => script = value()?,
            "--size" => size = value()?,
            "--seed" => seed = value()?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--seating" => {
                seating = match value()?.as_str() {
                    "even" => Seating::Even,
                    "odd" => Seating::Odd,
                    other => return Err(format!("--seating {other}: even or odd")),
                }
            }
            "--candidate" => candidate = value()?,
            "--baseline" => baseline = value()?,
            "--only" => only = Some(value()?.split(',').map(|s| s.trim().to_uppercase()).collect()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let rules = ClassicBench::rules();
    let suite = Suite::named(&suite_name).ok_or_else(|| format!("unknown suite {suite_name}"))?;
    let def = rules.speed_named(&speed).ok_or_else(|| format!("unknown speed {speed}"))?;
    let suite = suite.at_speed(&def.key, &def.values);
    let script: &'static str = Box::leak(script.into_boxed_str());
    let size: &'static str = Box::leak(size.into_boxed_str());
    let game = Match { map: MapSpec { script, size }, seed, seating };
    let pairing = Pairing { baseline, candidate };
    println!("strategy net: {} hand weights", weights::count());
    trace::enable(true);
    let result = play_match::<ClassicBench>(&rules, &suite, &game, &pairing)?;
    trace::enable(false);
    for line in trace::take() {
        let kind = line.split_whitespace().nth(2).unwrap_or("");
        if only.as_ref().is_none_or(|o| o.iter().any(|k| k == kind)) {
            println!("{line}");
        }
    }
    println!("{}: {} rounds, hash {:016x}", result.label, result.rounds, result.hash);
    for seat in &result.seats {
        let last = seat.standings.last();
        println!(
            "  p{} {:?} {}: rank {}, victory {}, score {}, cities {}, techs {}",
            seat.player.0,
            seat.side,
            seat.leader,
            seat.rank,
            seat.victory.unwrap_or("-"),
            last.map_or(0, |s| s.score),
            last.map_or(0, |s| s.cities),
            last.map_or(0, |s| s.techs),
        );
    }
    Ok(())
}
