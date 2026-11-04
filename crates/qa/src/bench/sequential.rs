//! Sequential testing (`ai-tournament --sprt`): the suite's seed blocks are
//! played one after another — every game of a seed, all maps, both
//! seatings — and after every completed block a sequential probability
//! ratio test decides whether to stop (platform/docs/ai-bench.md,
//! «Sequential test»).
//!
//! The unit is the seed block, as in the bootstrap (`stats`): its games
//! share the nations and one world mirrored, so they are not independent.
//! The test is the generalised SPRT on the blocks' means with a normal
//! likelihood and the variance estimated from the blocks played: the log
//! likelihood ratio of a mean `s1` against `s0` after `n` blocks of mean
//! `m` and variance `v` is `n·(s1 − s0)·(m − (s0 + s1)/2)/v`. It stops at
//! Wald's bounds `ln(β/(1 − α))` (accept H0: the mean ≤ s0) and
//! `ln((1 − β)/α)` (accept H1: the mean ≥ s1), not before `min_blocks`
//! (the variance of fewer blocks is too rough) and at the latest after
//! `max_blocks` (undecided).
//!
//! With a reference (`--reference`: saved reports of another candidate
//! against the same baseline on the same seeds) a block's value is the
//! candidate's points share minus the reference's on the same games: the
//! paired difference, whose variance is smaller the more the two agree on
//! which seeds are hard.
//!
//! A decision depends only on the blocks before it in the schedule's order,
//! never on which games the threads finished first, so a run is the same
//! on any number of threads; games past the deciding block that were
//! already running are dropped.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::bench::modules::BenchRules;
use crate::bench::play::{play_match, MatchResult, Pairing};
use crate::bench::report::{candidate_points, MatchLine, TournamentReport};
use crate::bench::stats::Estimate;
use crate::bench::suite::{Match, Mode, Side, Suite, SEED_STRIDE};

/// The blocks and games a test plays at least by default (the larger of
/// the two): with fewer the variance of the blocks is too rough to stop on
/// — a simulation on measured blocks gave an α of 0.09 instead of 0.05 for
/// duels stopped at 4 blocks of 10 games, 0.065 at 6 (platform/docs/ai-bench.md,
/// «Sequential test»).
pub const DEFAULT_MIN_BLOCKS: usize = 4;
pub const DEFAULT_MIN_GAMES: usize = 60;

/// The floor of a block's variance, as a design effect of a share of ½
/// over the block's games: a run whose blocks all came out alike (two
/// equal versions) does not divide by zero.
const VARIANCE_FLOOR_DESIGN_EFFECT: f64 = 0.05;

/// The hypotheses on the mean of the blocks (the points share, or its
/// difference from the reference) and the error rates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sprt {
    pub s0: f64,
    pub s1: f64,
    pub alpha: f64,
    pub beta: f64,
}

impl Sprt {
    /// Wald's bounds of the log likelihood ratio: (accept H0, accept H1).
    pub fn bounds(&self) -> (f64, f64) {
        ((self.beta / (1.0 - self.alpha)).ln(), ((1.0 - self.beta) / self.alpha).ln())
    }

    /// The log likelihood ratio of H1 against H0 after these blocks' values
    /// (0 below two blocks); `floor` bounds their variance from below.
    pub fn llr(&self, values: &[f64], floor: f64) -> f64 {
        if values.len() < 2 {
            return 0.0;
        }
        let e = Estimate::of(values.iter().copied());
        let variance = (e.sd * e.sd).max(floor);
        (self.s1 - self.s0) * values.len() as f64 * (e.mean - (self.s0 + self.s1) / 2.0) / variance
    }

    /// The decision after these blocks' values, the test's plan given.
    pub fn decide(&self, values: &[f64], floor: f64, plan: &Plan) -> Option<Decision> {
        let (lower, upper) = self.bounds();
        let llr = self.llr(values, floor);
        if values.len() >= plan.min_blocks && llr >= upper {
            Some(Decision::H1)
        } else if values.len() >= plan.min_blocks && llr <= lower {
            Some(Decision::H0)
        } else if values.len() >= plan.max_blocks {
            Some(Decision::Undecided)
        } else {
            None
        }
    }
}

/// When the test may stop, in seed blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub min_blocks: usize,
    pub max_blocks: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// The mean is at most `s0`.
    H0,
    /// The mean is at least `s1`.
    H1,
    /// `max_blocks` played without a decision.
    Undecided,
}

/// What a sequential run decided, in the report (JSON `sequential`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sequential {
    pub sprt: Sprt,
    pub min_blocks: u32,
    pub max_blocks: u32,
    /// The reference's reports: the blocks' values are differences from it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference: Vec<String>,
    pub decision: Decision,
    pub llr: f64,
    pub lower: f64,
    pub upper: f64,
    pub blocks: u32,
    pub games: u32,
    /// The blocks' mean (the share, or the difference from the reference),
    /// its standard deviation over blocks, and a normal 95 % interval —
    /// descriptive: after an early stop the mean leans toward the side
    /// decided.
    pub mean: f64,
    pub block_sd: f64,
    pub low: f64,
    pub high: f64,
    /// With a reference: its points share on the same games.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_share: Option<f64>,
    /// The log likelihood ratio after every block.
    pub path: Vec<f64>,
}

impl Sequential {
    pub fn markdown(&self, out: &mut String) {
        let pct = |x: f64| 100.0 * x;
        let what = if self.reference.is_empty() { "points share" } else { "points share minus the reference's" };
        let bound = |x: f64| if self.reference.is_empty() { format!("{:.1} %", pct(x)) } else { format!("{:+.1} %", pct(x)) };
        let decision = match self.decision {
            Decision::H0 => format!("**accept H0: the {what} ≤ {}**", bound(self.sprt.s0)),
            Decision::H1 => format!("**accept H1: the {what} ≥ {}**", bound(self.sprt.s1)),
            Decision::Undecided => format!("**undecided after the cap of {} seed blocks**", self.max_blocks),
        };
        let _ = writeln!(
            out,
            "Sequential test (SPRT on the {what} per seed block, H0 ≤ {}, H1 ≥ {}, α {}, β {}): {decision} after {} games, {} seed blocks (at least {}, at most {}); LLR {:+.2}, bounds {:+.2} / {:+.2}.\n",
            bound(self.sprt.s0),
            bound(self.sprt.s1),
            self.sprt.alpha,
            self.sprt.beta,
            self.games,
            self.blocks,
            self.min_blocks,
            self.max_blocks,
            self.llr,
            self.lower,
            self.upper
        );
        let reference = match self.reference_share {
            Some(r) => format!(" (the candidate {:.1} %, the reference {:.1} % on the same games)", pct(self.mean + r), pct(r)),
            None => String::new(),
        };
        let _ = writeln!(
            out,
            "The {what}: {} (normal 95 % CI {}…{} over blocks, sd per block {:.1} %; descriptive after an early stop){reference}.\n",
            bound(self.mean),
            bound(self.low),
            bound(self.high),
            pct(self.block_sd)
        );
    }
}

/// The suite's seed blocks from `offset` on, seed by seed (the suite's
/// seeds at offset K, then at K + 1, …): every map in the suite's order,
/// both seatings. The first blocks of one offset are the games of a plain
/// run at that offset, in another order.
pub fn block_schedule(suite: &Suite, offset: u64, blocks: usize) -> Vec<Vec<Match>> {
    let per_offset = suite.seeds.len().max(1);
    (0..blocks)
        .map(|b| {
            let base = suite.seeds[b % per_offset];
            let seed = base + (offset + (b / per_offset) as u64) * SEED_STRIDE;
            suite.matches(Mode::Versus, 0).into_iter().filter(|m| m.seed == base).map(|m| Match { seed, ..m }).collect()
        })
        .collect()
}

/// The points of a reference's games by label, from its saved reports
/// (`--save`), checked to be the same suite, speed and baseline.
#[derive(Debug, Clone, Default)]
pub struct Reference {
    pub paths: Vec<String>,
    points: BTreeMap<String, f64>,
}

impl Reference {
    pub fn load(paths: &[String], suite: &Suite, pairing: &Pairing) -> Result<Self, String> {
        let mut reference = Reference { paths: paths.to_vec(), points: BTreeMap::new() };
        for path in paths {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let report: TournamentReport = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
            if report.mode != Mode::Versus || report.suite != suite.name || report.speed != suite.speed || report.baseline != pairing.baseline {
                return Err(format!(
                    "{path}: a reference must be head to head on suite {} at the same speed against {} (it is {} on {}{} against {})",
                    suite.name,
                    pairing.baseline,
                    report.candidate,
                    report.suite,
                    if report.speed.is_empty() { String::new() } else { format!(" at {}", report.speed) },
                    report.baseline
                ));
            }
            for m in &report.matches {
                reference.points.insert(m.label.clone(), line_points(m));
            }
        }
        Ok(reference)
    }

    /// The reference's points share over these games; an error names the
    /// first game it lacks.
    pub fn share(&self, games: &[Match]) -> Result<f64, String> {
        let mut total = 0.0;
        for g in games {
            let label = g.label();
            total += self.points.get(&label).ok_or_else(|| format!("the reference has no game {label} (play it at that seed offset and add its report)"))?;
        }
        Ok(total / games.len().max(1) as f64)
    }
}

fn line_points(m: &MatchLine) -> f64 {
    match m.side {
        Some(Side::Candidate) => 1.0,
        Some(Side::Baseline) => 0.0,
        None => 0.5,
    }
}

/// A sequential run: the games played (the deciding block's last game
/// last) and the test's outcome.
pub struct SequentialRun {
    pub results: Vec<MatchResult>,
    pub outcome: Sequential,
}

/// Plays the blocks of `block_schedule(suite, offset, plan.max_blocks)` on
/// `jobs` threads until the test decides. Failed matches are reported by
/// label.
pub fn play_sequential<R: BenchRules>(
    suite: &Suite,
    pairing: &Pairing,
    offset: u64,
    sprt: Sprt,
    plan: Plan,
    reference: Option<&Reference>,
    jobs: usize,
) -> Result<SequentialRun, Vec<String>> {
    let blocks = block_schedule(suite, offset, plan.max_blocks);
    let reference_shares: Option<Vec<f64>> = reference
        .map(|r| blocks.iter().map(|b| r.share(b)).collect::<Result<_, _>>())
        .transpose()
        .map_err(|e| vec![e])?;
    let matches: Vec<Match> = blocks.iter().flatten().copied().collect();
    let ends: Vec<usize> = blocks
        .iter()
        .scan(0, |end, b| {
            *end += b.len();
            Some(*end)
        })
        .collect();
    let block_games = blocks.first().map_or(1, Vec::len);
    let floor = VARIANCE_FLOOR_DESIGN_EFFECT * 0.25 / block_games as f64;

    struct State {
        slots: Vec<Option<Result<MatchResult, String>>>,
        /// Games done in the schedule's order without a gap.
        prefix: usize,
        values: Vec<f64>,
        path: Vec<f64>,
        /// (blocks, decision) once decided.
        decided: Option<(usize, Decision)>,
        failed: bool,
    }
    let state = Mutex::new(State { slots: vec![None; matches.len()], prefix: 0, values: Vec::new(), path: Vec::new(), decided: None, failed: false });
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, matches.len().max(1)) {
            scope.spawn(|| {
                let rules = R::rules();
                while !stop.load(Ordering::Relaxed) {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(game) = matches.get(at) else { break };
                    let result = play_match::<R>(&rules, suite, game, pairing);
                    let mut s = state.lock().expect("no thread panicked holding the state");
                    s.failed |= result.is_err();
                    s.slots[at] = Some(result);
                    while s.decided.is_none() && s.prefix < matches.len() && s.slots[s.prefix].is_some() {
                        s.prefix += 1;
                        let Some(block) = ends.iter().position(|&e| e == s.prefix) else { continue };
                        let games = &s.slots[ends[block] - block_games..ends[block]];
                        let share = games.iter().filter_map(|g| g.as_ref().and_then(|r| r.as_ref().ok())).map(candidate_points).sum::<f64>() / block_games as f64;
                        let value = share - reference_shares.as_ref().map_or(0.0, |r| r[block]);
                        s.values.push(value);
                        let llr = sprt.llr(&s.values, floor);
                        s.path.push(llr);
                        if let Some(decision) = sprt.decide(&s.values, floor, &plan) {
                            s.decided = Some((block + 1, decision));
                        }
                    }
                    if s.failed || s.decided.is_some() {
                        stop.store(true, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    let s = state.into_inner().expect("no thread panicked holding the state");
    let failed: Vec<String> = s.slots.iter().flatten().filter_map(|r| r.as_ref().err().cloned()).collect();
    if !failed.is_empty() {
        return Err(failed);
    }
    let (played, decision) = s.decided.expect("the last block decides");
    let results: Vec<MatchResult> = s.slots.into_iter().take(ends[played - 1]).map(|r| r.expect("every game of a decided block played").expect("no game failed")).collect();
    let (lower, upper) = sprt.bounds();
    let e = Estimate::of(s.values.iter().copied());
    let half = e.half_width();
    let reference_share = reference_shares.map(|r| r[..played].iter().sum::<f64>() / played as f64);
    let outcome = Sequential {
        sprt,
        min_blocks: plan.min_blocks as u32,
        max_blocks: plan.max_blocks as u32,
        reference: reference.map(|r| r.paths.clone()).unwrap_or_default(),
        decision,
        llr: *s.path.last().unwrap_or(&0.0),
        lower,
        upper,
        blocks: played as u32,
        games: results.len() as u32,
        mean: e.mean,
        block_sd: e.sd,
        low: e.mean - half,
        high: e.mean + half,
        reference_share,
        path: s.path,
    };
    Ok(SequentialRun { results, outcome })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST: Sprt = Sprt { s0: 0.45, s1: 0.55, alpha: 0.05, beta: 0.05 };
    const PLAN: Plan = Plan { min_blocks: 4, max_blocks: 40 };

    #[test]
    fn wald_bounds() {
        let (lower, upper) = TEST.bounds();
        assert!((lower + 2.944).abs() < 1e-3 && (upper - 2.944).abs() < 1e-3, "{lower} {upper}");
    }

    /// A clearly weaker candidate stops at the earliest block allowed; a
    /// clearly stronger one too; equal blocks at the midpoint never stop.
    #[test]
    fn a_clear_answer_stops_at_the_minimum() {
        let weak = [0.1, 0.2, 0.15, 0.1, 0.2];
        assert_eq!(TEST.decide(&weak[..3], 1e-3, &PLAN), None, "not before the minimum");
        assert_eq!(TEST.decide(&weak[..4], 1e-3, &PLAN), Some(Decision::H0));
        let strong = [0.8, 0.9, 0.85, 0.9];
        assert_eq!(TEST.decide(&strong, 1e-3, &PLAN), Some(Decision::H1));
        let equal = vec![0.5; 40];
        assert_eq!(TEST.llr(&equal, 1e-3), 0.0);
        assert_eq!(TEST.decide(&equal[..39], 1e-3, &PLAN), None);
        assert_eq!(TEST.decide(&equal, 1e-3, &PLAN), Some(Decision::Undecided));
    }

    /// The likelihood ratio grows with the blocks and shrinks with their
    /// spread.
    #[test]
    fn the_ratio_weighs_mean_and_spread() {
        let tight = [0.6, 0.62, 0.58, 0.6];
        let loose = [0.9, 0.3, 0.8, 0.4];
        assert!(TEST.llr(&tight, 1e-4) > TEST.llr(&loose, 1e-4));
        let more: Vec<f64> = tight.iter().chain(tight.iter()).copied().collect();
        assert!(TEST.llr(&more, 1e-4) > TEST.llr(&tight, 1e-4));
    }

    /// The schedule is seed-major and its first blocks of one offset are
    /// the plain run's games.
    #[test]
    fn blocks_follow_the_seeds() {
        let suite = Suite::named("duel").unwrap();
        let blocks = block_schedule(&suite, 0, 10);
        assert_eq!(blocks.len(), 10);
        assert!(blocks.iter().all(|b| b.len() == 10));
        assert!(blocks[0].iter().all(|m| m.seed == 1) && blocks[7].iter().all(|m| m.seed == 8));
        assert!(blocks[8].iter().all(|m| m.seed == 1001) && blocks[9].iter().all(|m| m.seed == 1002));
        let mut plain: Vec<String> = suite.matches(Mode::Versus, 0).iter().map(Match::label).collect();
        let mut first: Vec<String> = blocks[..8].iter().flatten().map(Match::label).collect();
        plain.sort();
        first.sort();
        assert_eq!(plain, first);
        let later = block_schedule(&suite, 3, 1);
        assert!(later[0].iter().all(|m| m.seed == 3001));
    }
}
