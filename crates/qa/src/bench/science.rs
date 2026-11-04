//! The science victory of a side's games (classic/docs/ai-science-victory.md):
//! the share of the games a seat of the side won by science by round 300,
//! 400 and 500, the round of those victories, the technologies and the
//! science a turn at the checkpoints (the side's mean and each game's best
//! seat), and the spaceship: the prerequisite project and the parts
//! installed. Every mean has its seed-block bootstrap interval (`stats`).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::bench::modules::Standing;
use crate::bench::play::MatchResult;
use crate::bench::stats::{BlockEstimate, BlockSum};
use crate::bench::suite::Side;

/// The rounds a science victory is counted by (those before the suite's
/// last round, then the last round).
pub const BY_ROUNDS: [u32; 3] = [300, 400, 500];

/// The science victory of one side's seats over a tournament.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScienceSummary {
    /// Per round of `BY_ROUNDS` before the suite's last round, then the
    /// last round: the share of the games
    /// with a seat of the side that one of its seats won by science by then.
    pub victories: Vec<(u32, BlockEstimate)>,
    /// The round of the side's science victories, and the earliest.
    pub victory_round: BlockEstimate,
    pub earliest: Option<u32>,
    /// Per checkpoint (then the end): technologies and science a turn, the
    /// side's seats' mean and each game's best seat of the side.
    pub progress: Vec<ScienceAt>,
    /// The share of seats that completed the spaceship's prerequisite
    /// project, and the round they did.
    pub project: BlockEstimate,
    pub project_round: BlockEstimate,
    /// Parts installed per seat at the end, and the share of the games in
    /// which a seat of the side installed every part.
    pub parts: BlockEstimate,
    pub complete: BlockEstimate,
    /// The parts a spaceship needs (0: no science victory by a spaceship).
    pub parts_needed: u32,
}

/// Technologies and science a turn at one checkpoint (None: the end).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScienceAt {
    pub round: Option<u32>,
    pub techs: BlockEstimate,
    pub best_techs: BlockEstimate,
    pub science: BlockEstimate,
    pub best_science: BlockEstimate,
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

    fn estimate(self) -> BlockEstimate {
        BlockEstimate::of(&self.0.into_values().collect::<Vec<_>>())
    }
}

impl ScienceSummary {
    pub fn of(side: Side, results: &[MatchResult], rounds: u32, checkpoints: &[u32]) -> Self {
        let games: Vec<&MatchResult> = results.iter().filter(|r| r.seats.iter().any(|s| s.side == side)).collect();
        let won = |r: &MatchResult| r.winner().filter(|w| w.side == side && w.victory == Some("science")).map(|_| r.rounds);
        let mut by_rounds: Vec<u32> = BY_ROUNDS.into_iter().filter(|by| *by < rounds).collect();
        by_rounds.push(rounds);
        let victories = by_rounds
            .into_iter()
            .map(|by| {
                let mut blocks = Blocks::default();
                for r in &games {
                    blocks.add(r.seed, f64::from(u8::from(won(r).is_some_and(|at| at <= by))));
                }
                (by, blocks.estimate())
            })
            .collect();
        let mut victory_round = Blocks::default();
        for r in &games {
            if let Some(at) = won(r) {
                victory_round.add(r.seed, f64::from(at));
            }
        }
        let points: Vec<Option<u32>> = checkpoints.iter().map(|c| Some(*c)).chain([None]).collect();
        let progress = points.iter().enumerate().map(|(at, round)| science_at(side, &games, at, *round)).collect();
        let (mut project, mut project_round, mut parts, mut complete) = (Blocks::default(), Blocks::default(), Blocks::default(), Blocks::default());
        let parts_needed = games.iter().map(|r| r.space_parts).max().unwrap_or(0);
        for r in &games {
            let seats = r.seats.iter().filter(|s| s.side == side);
            for seat in seats.clone() {
                project.add(r.seed, f64::from(u8::from(seat.pace.space_project.is_some())));
                if let Some(at) = seat.pace.space_project {
                    project_round.add(r.seed, f64::from(at));
                }
                parts.add(r.seed, seat.pace.parts.len() as f64);
            }
            let done = r.space_parts > 0 && seats.clone().any(|s| s.pace.parts.len() as u32 >= r.space_parts);
            complete.add(r.seed, f64::from(u8::from(done)));
        }
        Self {
            victories,
            victory_round: victory_round.estimate(),
            earliest: games.iter().filter_map(|r| won(r)).min(),
            progress,
            project: project.estimate(),
            project_round: project_round.estimate(),
            parts: parts.estimate(),
            complete: complete.estimate(),
            parts_needed,
        }
    }

    pub fn markdown(&self, variant: &str, out: &mut String) {
        if self.victories.is_empty() && self.progress.is_empty() {
            return;
        }
        let by: Vec<String> = self.victories.iter().map(|(round, e)| format!("by {round}: {}", share(e))).collect();
        let earliest = self.earliest.map_or_else(|| "—".to_owned(), |r| r.to_string());
        let _ = writeln!(
            out,
            "{variant}: science victories {}; their round {} (earliest {earliest})\n",
            by.join(", "),
            interval(&self.victory_round)
        );
        let _ = writeln!(out, "| {variant}: science | techs (mean) | techs (best seat) | science a turn (mean) | science a turn (best seat) |");
        let _ = writeln!(out, "|---|---:|---:|---:|---:|");
        for at in &self.progress {
            let round = at.round.map_or_else(|| "end".to_owned(), |r| r.to_string());
            let _ = writeln!(out, "| {round} | {} | {} | {} | {} |", interval(&at.techs), interval(&at.best_techs), interval(&at.science), interval(&at.best_science));
        }
        if self.parts_needed > 0 {
            let _ = writeln!(
                out,
                "\n{variant}: spaceship project done by {} of the seats (round {}); parts installed a seat {:.2} (95 % CI {:.2}–{:.2}) of {}; a whole ship in {} of the games\n",
                share(&self.project),
                interval(&self.project_round),
                self.parts.mean,
                self.parts.low,
                self.parts.high,
                self.parts_needed,
                share(&self.complete)
            );
        } else {
            out.push('\n');
        }
    }
}

/// The side's technologies and science at checkpoint `at` (index into the
/// seats' standings): per seat and per game's best seat.
fn science_at(side: Side, games: &[&MatchResult], at: usize, round: Option<u32>) -> ScienceAt {
    let (mut techs, mut best_techs, mut science, mut best_science) = (Blocks::default(), Blocks::default(), Blocks::default(), Blocks::default());
    for r in games {
        let standings: Vec<&Standing> = r.seats.iter().filter(|s| s.side == side).filter_map(|s| s.standings.get(at)).collect();
        for s in &standings {
            techs.add(r.seed, f64::from(s.techs));
            science.add(r.seed, f64::from(s.economy.science));
        }
        if let Some(t) = standings.iter().map(|s| s.techs).max() {
            best_techs.add(r.seed, f64::from(t));
        }
        if let Some(s) = standings.iter().map(|s| s.economy.science).max() {
            best_science.add(r.seed, f64::from(s));
        }
    }
    ScienceAt { round, techs: techs.estimate(), best_techs: best_techs.estimate(), science: science.estimate(), best_science: best_science.estimate() }
}

fn share(e: &BlockEstimate) -> String {
    if e.blocks == 0 {
        return "—".to_owned();
    }
    format!("{:.1} % ({:.1}–{:.1} %)", 100.0 * e.mean, 100.0 * e.low, 100.0 * e.high)
}

fn interval(e: &BlockEstimate) -> String {
    if e.blocks == 0 {
        return "—".to_owned();
    }
    format!("{:.1} ({:.1}–{:.1})", e.mean, e.low, e.high)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::play::{CombatRecord, Pace, SeatResult};
    use kernel::ids::PlayerId;

    fn game(seed: u64, rounds: u32, science_winner: Option<Side>, parts: [usize; 2]) -> MatchResult {
        let seat = |at: usize, side: Side| SeatResult {
            player: PlayerId(at as u8),
            side,
            leader: String::new(),
            path: None,
            rank: 1,
            victory: (science_winner == Some(side)).then_some("science"),
            eliminated: None,
            standings: vec![Standing { techs: 40 + at as u32, ..Standing::default() }; 2],
            applied: 0,
            rejected: 0,
            no_command: 0,
            turn_ms: Vec::new(),
            think_ms: Vec::new(),
            delivered: 0,
            combat: CombatRecord::default(),
            founded: 1,
            sightings: Vec::new(),
            opening: Vec::new(),
            pace: Pace { space_project: (parts[at] > 0).then_some(rounds - 40), parts: vec![rounds; parts[at]], ..Pace::default() },
        };
        MatchResult {
            label: format!("seed {seed}"),
            seed,
            rounds,
            hash: seed,
            seats: vec![seat(0, Side::Candidate), seat(1, Side::Baseline)],
            round_ms: Vec::new(),
            eras: Vec::new(),
            space_parts: 6,
        }
    }

    /// A science victory counts by every round at or after it, for the
    /// winner's side only; parts and the project are the seats' own.
    #[test]
    fn science_victories_count_by_their_round() {
        let results = [game(1, 380, Some(Side::Candidate), [6, 2]), game(2, 500, None, [3, 0]), game(3, 450, Some(Side::Baseline), [1, 6])];
        let s = ScienceSummary::of(Side::Candidate, &results, 500, &[300]);
        let by: Vec<(u32, f64)> = s.victories.iter().map(|(r, e)| (*r, e.mean)).collect();
        assert_eq!(by, vec![(300, 0.0), (400, 1.0 / 3.0), (500, 1.0 / 3.0)]);
        assert_eq!((s.earliest, s.victory_round.mean), (Some(380), 380.0));
        assert_eq!(s.parts.mean, 10.0 / 3.0);
        assert_eq!(s.complete.mean, 1.0 / 3.0);
        assert_eq!(s.project.mean, 1.0);
        assert_eq!(s.parts_needed, 6);
        assert_eq!(s.progress.len(), 2);
        assert_eq!((s.progress[0].techs.mean, s.progress[0].best_techs.mean), (40.0, 40.0));
        let b = ScienceSummary::of(Side::Baseline, &results, 320, &[]);
        let by: Vec<(u32, f64)> = b.victories.iter().map(|(r, e)| (*r, e.mean)).collect();
        assert_eq!(by, vec![(300, 0.0), (320, 0.0)]);
        assert_eq!(b.earliest, Some(450));
    }
}
