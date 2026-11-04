//! The pace of a side's games: when its nations founded their first two
//! cities, learned their first technology and entered each era, and how
//! the games ended — the numbers a module's pacing is compared on against
//! public references (docs/ai-bench.md, «Темп»). Every mean has its
//! seed-block bootstrap interval (`stats`): the games of one seed share the
//! nations, so a seed block is the unit of resampling.
//!
//! A milestone's round is a mean over the seats that reached it, next to
//! the share that did: a seat that never reached it has no round (the mean
//! is censored, the share says by how much).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::bench::play::{MatchResult, Pace, SeatResult};
use crate::bench::stats::{BlockEstimate, BlockSum};
use crate::bench::suite::Side;

/// The pace of one side's seats over a tournament.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PaceSummary {
    pub milestones: Vec<Milestone>,
    /// Games the time victory ended (or the last round without a winner).
    pub by_time: BlockEstimate,
    /// The round of the games any other victory ended.
    pub victory_round: BlockEstimate,
}

/// One milestone of the side's seats.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Milestone {
    pub name: String,
    /// The share of the side's seats that reached it.
    pub reached: BlockEstimate,
    /// The mean round of the seats that reached it.
    pub round: BlockEstimate,
    /// The mean round of each game's first seat to reach it (games where
    /// one did): the leader's pace.
    pub first: BlockEstimate,
}

/// Per seed block, the sums of one value over its seats or games.
#[derive(Default)]
struct Blocks(BTreeMap<u64, BlockSum>);

impl Blocks {
    fn add(&mut self, seed: u64, value: f64) {
        let block = self.0.entry(seed).or_insert(BlockSum { total: 0.0, games: 0 });
        block.total += value;
        block.games += 1;
    }

    /// Every seed of the tournament has a block, empty ones too (they
    /// weigh nothing).
    fn estimate(self) -> BlockEstimate {
        let blocks: Vec<BlockSum> = self.0.into_values().filter(|b| b.games > 0).collect();
        BlockEstimate::of(&blocks)
    }
}

/// The milestones every seat may reach: the cities, the first technology,
/// then the eras after the start one.
fn milestone_names(results: &[MatchResult]) -> Vec<String> {
    let eras = results.iter().map(|r| r.eras.len()).max().unwrap_or(0);
    let names = results.iter().find(|r| r.eras.len() == eras).map(|r| r.eras.clone()).unwrap_or_default();
    let mut out = vec!["first city".to_owned(), "second city".to_owned(), "first technology".to_owned()];
    out.extend(names.into_iter().skip(1).map(|era| format!("era: {era}")));
    out
}

/// The round `pace` reached the milestone at `index` of `milestone_names`.
fn round_of(pace: &Pace, index: usize) -> Option<u32> {
    match index {
        0 => pace.first_city,
        1 => pace.second_city,
        2 => pace.first_tech,
        era => pace.eras.get(era - 2).copied().flatten(),
    }
}

impl PaceSummary {
    pub fn of(side: Side, results: &[MatchResult]) -> Self {
        let names = milestone_names(results);
        let milestones = names.into_iter().enumerate().map(|(at, name)| milestone(side, results, at, name)).collect();
        let (mut by_time, mut victory_round) = (Blocks::default(), Blocks::default());
        for r in results.iter().filter(|r| r.seats.iter().any(|s| s.side == side)) {
            let timed = r.winner().and_then(|w| w.victory).is_none_or(|kind| kind == "time");
            by_time.add(r.seed, f64::from(u8::from(timed)));
            if !timed {
                victory_round.add(r.seed, f64::from(r.rounds));
            }
        }
        Self { milestones, by_time: by_time.estimate(), victory_round: victory_round.estimate() }
    }

    pub fn markdown(&self, variant: &str, out: &mut String) {
        if self.milestones.is_empty() {
            return;
        }
        let _ = writeln!(out, "| {variant}: pace | seats reached % | mean round (95 % CI) | first in its game: mean round (95 % CI) |");
        let _ = writeln!(out, "|---|---:|---:|---:|");
        for m in &self.milestones {
            let _ = writeln!(out, "| {} | {:.0} | {} | {} |", m.name, 100.0 * m.reached.mean, interval(&m.round), interval(&m.first));
        }
        let t = &self.by_time;
        let _ = writeln!(
            out,
            "\n{variant}: games ended by time {:.0} % (95 % CI {:.0}–{:.0} %); other victories at round {}\n",
            100.0 * t.mean,
            100.0 * t.low,
            100.0 * t.high,
            interval(&self.victory_round)
        );
    }
}

fn interval(e: &BlockEstimate) -> String {
    if e.blocks == 0 {
        return "—".to_owned();
    }
    format!("{:.0} ({:.0}–{:.0})", e.mean, e.low, e.high)
}

fn milestone(side: Side, results: &[MatchResult], at: usize, name: String) -> Milestone {
    let (mut reached, mut round, mut first) = (Blocks::default(), Blocks::default(), Blocks::default());
    for r in results {
        let rounds: Vec<u32> = r.seats.iter().filter(|s| s.side == side).filter_map(|s| round_of_seat(s, at)).collect();
        for seat in r.seats.iter().filter(|s| s.side == side) {
            reached.add(r.seed, f64::from(u8::from(round_of_seat(seat, at).is_some())));
        }
        for &value in &rounds {
            round.add(r.seed, f64::from(value));
        }
        if let Some(earliest) = rounds.iter().min() {
            first.add(r.seed, f64::from(*earliest));
        }
    }
    Milestone { name, reached: reached.estimate(), round: round.estimate(), first: first.estimate() }
}

fn round_of_seat(seat: &SeatResult, at: usize) -> Option<u32> {
    round_of(&seat.pace, at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pace_records_each_milestone_once() {
        let mut pace = Pace::default();
        pace.founded(1);
        pace.progress(1, 0, 0);
        pace.founded(9);
        pace.founded(12);
        pace.progress(8, 1, 0);
        pace.progress(40, 5, 2);
        pace.progress(41, 6, 2);
        assert_eq!((pace.first_city, pace.second_city, pace.first_tech), (Some(1), Some(9), Some(8)));
        // Entering era 2 directly enters era 1 in the same round.
        assert_eq!(pace.eras, vec![None, Some(40), Some(40)]);
        for (at, expected) in [(0, Some(1)), (1, Some(9)), (2, Some(8)), (3, Some(40)), (4, Some(40)), (5, None)] {
            assert_eq!(round_of(&pace, at), expected, "milestone {at}");
        }
    }
}
