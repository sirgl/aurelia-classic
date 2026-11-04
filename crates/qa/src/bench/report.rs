//! What a tournament gave: per side (candidate, baseline) its wins by
//! kind, mean rank, eliminations, standings at the checkpoints with 95 %
//! intervals, refusals and turn times; head to head, the candidate's win
//! share and the paired score difference over seed blocks (`stats`). The
//! report is markdown for a person and JSON for the next revision
//! (`--save`, `--compare`).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::bench::modules::Standing;
use crate::bench::pace::PaceSummary;
use crate::bench::science::ScienceSummary;
use crate::bench::sequential::Sequential;
use crate::bench::play::{CombatRecord, MatchResult, Pairing, SeatResult};
use crate::bench::stats::{BlockEstimate, BlockSum, Estimate};
use crate::bench::suite::{Mode, Side, Suite};
use crate::percentiles;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TournamentReport {
    pub rules: String,
    pub suite: String,
    pub mode: Mode,
    pub baseline: String,
    pub candidate: String,
    pub seed_offset: u64,
    pub games: u32,
    pub rounds: u32,
    pub checkpoints: Vec<u32>,
    /// The game speed's key (empty, and absent from reports of before
    /// speeds: Standard).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub speed: String,
    /// Hash of every match's final world, in the suite's order: equal
    /// hashes, equal games.
    pub result_hash: String,
    /// Head to head only.
    pub versus: Option<Versus>,
    pub sides: Vec<SideSummary>,
    pub round_ms: Timing,
    pub wall_seconds: f64,
    pub jobs: usize,
    pub matches: Vec<MatchLine>,
    /// A sequential run's test (`--sprt`, `bench::sequential`); absent
    /// from a plain run's report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequential: Option<Sequential>,
}

/// Head to head over seed blocks (every game of one seed, both seatings:
/// `stats`). The primary metric, fixed before any run, is `share`: the
/// candidate's points per game — a win 1, a game without a winner (the
/// best score tied at the last round) ½ — against `expected`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Versus {
    pub games: u32,
    pub candidate_wins: u32,
    pub draws: u32,
    /// The win share of equal versions: the candidate's share of the seats.
    pub expected: f64,
    /// The candidate's points per game with the seed-block bootstrap
    /// interval.
    pub share: BlockEstimate,
    /// The bootstrap variance of the share over the binomial variance of
    /// as many independent games (None: the share is 0 or 1).
    pub design_effect: Option<f64>,
    /// The paired difference: per game, the candidate seats' mean final
    /// score minus the baseline seats' — both versions on the same world
    /// in both seatings, so the world's own size and wealth cancel.
    pub score_difference: BlockEstimate,
    pub blocks: Vec<SeedBlock>,
}

/// The games of one seed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SeedBlock {
    pub seed: u64,
    pub games: u32,
    pub points: f64,
    /// The sum over its games of the candidate's minus the baseline's
    /// mean final score.
    pub score_difference: f64,
}

/// What the primary metric says: its interval above, around or below the
/// share of equal versions — once the series has its planned blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    TooFewBlocks,
    Stronger,
    NoDifference,
    Weaker,
}

/// The seed blocks a verdict needs: fewer give a rough bootstrap interval
/// (docs/ai-bench.md, «Сколько сидов»).
pub const PLANNED_BLOCKS: u32 = 20;

impl Versus {
    fn of(blocks: Vec<SeedBlock>, candidate_wins: u32, draws: u32, expected: f64) -> Self {
        let sums = |value: fn(&SeedBlock) -> f64| -> Vec<BlockSum> { blocks.iter().map(|b| BlockSum { total: value(b), games: b.games }).collect() };
        let games = blocks.iter().map(|b| b.games).sum();
        let share = BlockEstimate::of(&sums(|b| b.points));
        Self {
            games,
            candidate_wins,
            draws,
            expected,
            share,
            design_effect: share.design_effect(games),
            score_difference: BlockEstimate::of(&sums(|b| b.score_difference)),
            blocks,
        }
    }

    /// Several runs of one suite (one stratum): every seed block of every
    /// run, each weighted by its games.
    pub fn pooled(runs: &[&Versus]) -> Self {
        let games: u32 = runs.iter().map(|v| v.games).sum();
        let expected = runs.iter().map(|v| v.expected * f64::from(v.games)).sum::<f64>() / f64::from(games.max(1));
        Self::of(
            runs.iter().flat_map(|v| v.blocks.iter().copied()).collect(),
            runs.iter().map(|v| v.candidate_wins).sum(),
            runs.iter().map(|v| v.draws).sum(),
            expected,
        )
    }

    pub fn verdict(&self) -> Verdict {
        if self.share.blocks < PLANNED_BLOCKS {
            Verdict::TooFewBlocks
        } else if self.share.low > self.expected {
            Verdict::Stronger
        } else if self.share.high < self.expected {
            Verdict::Weaker
        } else {
            Verdict::NoDifference
        }
    }

    /// The primary metric, the paired difference and the design effect.
    fn markdown(&self, out: &mut String) {
        let verdict = match self.verdict() {
            Verdict::TooFewBlocks => format!("no verdict below {PLANNED_BLOCKS} seed blocks (`--repeat` adds them)"),
            Verdict::Stronger => "the candidate is stronger".to_owned(),
            Verdict::NoDifference => "no difference shown".to_owned(),
            Verdict::Weaker => "the candidate is weaker".to_owned(),
        };
        let s = &self.share;
        let _ = writeln!(
            out,
            "**Primary metric, the candidate's points share (win 1, draw ½): {:.1} % (95 % CI {:.1}–{:.1} %, seed-block bootstrap), equal versions {:.1} %: {verdict}.**\n",
            100.0 * s.mean,
            100.0 * s.low,
            100.0 * s.high,
            100.0 * self.expected
        );
        let d = &self.score_difference;
        let deff = self.design_effect.map_or_else(|| "—".to_owned(), |e| format!("{e:.2}"));
        let _ = writeln!(
            out,
            "Wins {} of {} games, draws {}; {} seed blocks, design effect {deff} (above 1: an interval over independent games is too narrow). Paired difference per seed block, the candidate's minus the baseline's mean final score: {:+.1} (95 % CI {:+.1}…{:+.1}).\n",
            self.candidate_wins, self.games, self.draws, s.blocks, d.mean, d.low, d.high
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideSummary {
    pub side: Side,
    pub variant: String,
    /// Seats played (a game counts once per seat).
    pub seat_games: u32,
    /// Victories by kind.
    pub wins: BTreeMap<String, u32>,
    pub mean_rank: f64,
    pub eliminations: u32,
    /// At the checkpoints, then at the end.
    pub standings: Vec<StandingSummary>,
    /// Commands the game rejected, of all the AI's commands.
    pub rejected_share: f64,
    /// Decisions without a command, of all decisions.
    pub no_command_share: f64,
    /// One player's turn.
    pub turn_ms: Timing,
    /// One player's turn without its end: the AI's own time (reports
    /// saved before it read as zeros).
    #[serde(default)]
    pub think_ms: Timing,
    /// The side's combat record over all its seats (reports saved before
    /// it existed read as zeros).
    #[serde(default)]
    pub combat: CombatRecord,
    /// Its seats by leader (empty: the module has no leaders).
    #[serde(default)]
    pub leaders: BTreeMap<String, LeaderRecord>,
    /// How its seats found the rivals' original capitals.
    #[serde(default)]
    pub scouting: Scouting,
    /// How dense its seats' decisions were in the opening (reports saved
    /// before it read as empty).
    #[serde(default)]
    pub tempo: Tempo,
    /// When its seats reached the milestones of the game's pace, and how
    /// the games ended (reports saved before read empty).
    #[serde(default)]
    pub pace: PaceSummary,
    /// Its science victories, technologies, science and spaceship
    /// (`bench::science`; reports saved before read empty).
    #[serde(default)]
    pub science: ScienceSummary,
}

/// The opening's tempo of a group of seats: their meaningful decisions in
/// each of the first rounds (`BenchRules::OPENING_ROUNDS`), as the mean a
/// round and the longest run of rounds without one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tempo {
    pub seats: u32,
    pub rounds: u32,
    pub decisions: u32,
    /// The longest idle run of each seat, sorted.
    pub idle: Vec<u32>,
    /// The fewest decisions a round one seat made.
    pub fewest_per_round: f64,
}

impl Tempo {
    fn of(seats: &[&SeatResult]) -> Self {
        let mut tempo = Tempo::default();
        for seat in seats.iter().filter(|s| !s.opening.is_empty()) {
            tempo.add(&Tempo::of_seat(&seat.opening));
        }
        tempo
    }

    fn of_seat(opening: &[u32]) -> Self {
        let decisions: u32 = opening.iter().sum();
        let rounds = opening.len() as u32;
        let (mut run, mut idle) = (0, 0);
        for &count in opening {
            run = if count == 0 { run + 1 } else { 0 };
            idle = u32::max(idle, run);
        }
        Tempo { seats: 1, rounds, decisions, idle: vec![idle], fewest_per_round: f64::from(decisions) / f64::from(rounds.max(1)) }
    }

    fn add(&mut self, other: &Tempo) {
        let first = self.seats == 0;
        self.fewest_per_round = if first { other.fewest_per_round } else { self.fewest_per_round.min(other.fewest_per_round) };
        self.seats += other.seats;
        self.rounds = self.rounds.max(other.rounds);
        self.decisions += other.decisions;
        self.idle.extend(&other.idle);
        self.idle.sort_unstable();
    }

    /// Decisions a round, over all seats.
    fn per_round(&self) -> f64 {
        f64::from(self.decisions) / f64::from((self.seats * self.rounds).max(1))
    }

    fn mean_idle(&self) -> f64 {
        self.idle.iter().map(|&i| f64::from(i)).sum::<f64>() / self.idle.len().max(1) as f64
    }

    fn line(&self) -> String {
        if self.seats == 0 {
            return "—".to_owned();
        }
        let longest = self.idle.last().copied().unwrap_or(0);
        let over = self.idle.iter().filter(|&&i| i > 3).count();
        format!(
            "{:.2} decisions a round (fewest seat {:.2}), longest idle run mean {:.1}, max {longest}, seats over 3: {over} of {}",
            self.per_round(),
            self.fewest_per_round,
            self.mean_idle(),
            self.seats
        )
    }
}

/// How a side's seats found the original capitals of their rivals (the
/// tiles the domination victory must take).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scouting {
    /// Rival capitals over the side's seats.
    pub capitals: u32,
    /// The round each found one's tile became known to the seat, sorted.
    pub rounds: Vec<u32>,
}

impl Scouting {
    fn of(seats: &[&SeatResult]) -> Self {
        let sightings = seats.iter().flat_map(|s| s.sightings.iter());
        let mut rounds: Vec<u32> = sightings.clone().filter_map(|(_, round)| *round).collect();
        rounds.sort_unstable();
        Self { capitals: sightings.count() as u32, rounds }
    }

    fn add(&mut self, other: &Scouting) {
        self.capitals += other.capitals;
        self.rounds.extend(&other.rounds);
        self.rounds.sort_unstable();
    }

    /// Found ones, found by `round`, and the round of the median sighting.
    fn line(&self) -> String {
        if self.capitals == 0 {
            return "—".to_owned();
        }
        let share = |n: usize| 100.0 * n as f64 / f64::from(self.capitals);
        let by_100 = self.rounds.iter().filter(|r| **r <= 100).count();
        let median = self.rounds.get(self.rounds.len() / 2).map_or_else(|| "—".to_owned(), u32::to_string);
        format!(
            "{} of {} ({:.0} %), by r100 {:.0} %, median sighting r{median}",
            self.rounds.len(),
            self.capitals,
            share(self.rounds.len()),
            share(by_100)
        )
    }
}

/// What the seats of one leader did, summed (means are sums over seats).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LeaderRecord {
    pub seats: u32,
    pub wins: u32,
    /// Final score and world wonders, summed over the seats.
    pub score: i64,
    pub wonders: u32,
    pub cities: u32,
    /// Population, technologies, military and food delivered at the end,
    /// summed (reports saved before these read as zeros).
    #[serde(default)]
    pub population: u32,
    #[serde(default)]
    pub techs: u32,
    #[serde(default)]
    pub military: u64,
    #[serde(default)]
    pub delivered: u64,
    /// Cities founded, summed, and the fewest one seat founded.
    #[serde(default)]
    pub founded: u32,
    #[serde(default)]
    pub fewest_founded: u32,
    pub combat: CombatRecord,
    /// Seats by the grand strategy's goal at the end.
    pub paths: BTreeMap<String, u32>,
    /// The opening's tempo of its seats (reports saved before read empty).
    #[serde(default)]
    pub tempo: Tempo,
}

impl LeaderRecord {
    fn add(&mut self, other: &LeaderRecord) {
        let first = self.seats == 0;
        self.seats += other.seats;
        self.wins += other.wins;
        self.score += other.score;
        self.wonders += other.wonders;
        self.cities += other.cities;
        self.population += other.population;
        self.techs += other.techs;
        self.military += other.military;
        self.delivered += other.delivered;
        self.fewest_founded = if first { other.fewest_founded } else { self.fewest_founded.min(other.fewest_founded) };
        self.founded += other.founded;
        self.combat = self.combat.add(other.combat);
        self.tempo.add(&other.tempo);
        for (path, n) in &other.paths {
            *self.paths.entry(path.clone()).or_insert(0) += n;
        }
    }

    fn of_seat(seat: &SeatResult) -> Self {
        let end = seat.standings.last().copied().unwrap_or_default();
        let paths = seat.path.map(|p| BTreeMap::from([(p.to_owned(), 1)])).unwrap_or_default();
        Self {
            seats: 1,
            wins: u32::from(seat.victory.is_some()),
            score: end.score,
            wonders: end.economy.wonders,
            cities: end.cities,
            population: end.population,
            techs: end.techs,
            military: u64::from(end.military),
            delivered: end.economy.delivered,
            founded: seat.founded,
            fewest_founded: seat.founded,
            combat: seat.combat,
            paths,
            tempo: if seat.opening.is_empty() { Tempo::default() } else { Tempo::of_seat(&seat.opening) },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StandingSummary {
    pub round: Option<u32>,
    pub score: Estimate,
    pub cities: Estimate,
    pub population: Estimate,
    pub techs: Estimate,
    pub military: Estimate,
    pub tiles: Estimate,
    /// Reports saved before these read as zeros.
    #[serde(default)]
    pub wonders: Estimate,
    #[serde(default)]
    pub gold: Estimate,
    #[serde(default)]
    pub production: Estimate,
    #[serde(default)]
    pub science: Estimate,
    #[serde(default)]
    pub treasury: Estimate,
    /// Excess happiness.
    #[serde(default)]
    pub happiness: Estimate,
    /// Share of the seats at the very-unhappy line (−10) or below.
    #[serde(default)]
    pub very_unhappy: Estimate,
    #[serde(default)]
    pub golden_ages: Estimate,
    /// Buildings that add gold, and happiness.
    #[serde(default)]
    pub gold_buildings: Estimate,
    #[serde(default)]
    pub happy_buildings: Estimate,
    #[serde(default)]
    pub delivered: Estimate,
    /// Faith a turn; the share of the seats that founded a religion, their
    /// own cities following it and its followers.
    #[serde(default)]
    pub faith: Estimate,
    #[serde(default)]
    pub religion: Estimate,
    #[serde(default)]
    pub faithful_cities: Estimate,
    #[serde(default)]
    pub followers: Estimate,
}

impl StandingSummary {
    fn of(round: Option<u32>, standings: &[Standing]) -> Self {
        let estimate = |value: fn(&Standing) -> f64| Estimate::of(standings.iter().map(value));
        Self {
            round,
            score: estimate(|s| s.score as f64),
            cities: estimate(|s| f64::from(s.cities)),
            population: estimate(|s| f64::from(s.population)),
            techs: estimate(|s| f64::from(s.techs)),
            military: estimate(|s| f64::from(s.military)),
            tiles: estimate(|s| f64::from(s.tiles)),
            wonders: estimate(|s| f64::from(s.economy.wonders)),
            gold: estimate(|s| f64::from(s.economy.gold)),
            production: estimate(|s| f64::from(s.economy.production)),
            science: estimate(|s| f64::from(s.economy.science)),
            treasury: estimate(|s| f64::from(s.economy.treasury)),
            happiness: estimate(|s| f64::from(s.economy.happiness)),
            very_unhappy: estimate(|s| f64::from(u8::from(s.economy.happiness <= classic_rules::derived::happiness::VERY_UNHAPPY_THRESHOLD))),
            golden_ages: estimate(|s| f64::from(s.economy.golden_ages)),
            gold_buildings: estimate(|s| f64::from(s.economy.buildings.gold)),
            happy_buildings: estimate(|s| f64::from(s.economy.buildings.happiness)),
            delivered: estimate(|s| s.economy.delivered as f64),
            faith: estimate(|s| f64::from(s.economy.faith)),
            religion: estimate(|s| f64::from(s.economy.religion)),
            faithful_cities: estimate(|s| f64::from(s.economy.faithful_cities)),
            followers: estimate(|s| f64::from(s.economy.followers)),
        }
    }

    fn metrics(&self) -> [(&'static str, Estimate); 21] {
        [
            ("score", self.score),
            ("cities", self.cities),
            ("population", self.population),
            ("techs", self.techs),
            ("military", self.military),
            ("tiles", self.tiles),
            ("wonders", self.wonders),
            ("gold", self.gold),
            ("production", self.production),
            ("science", self.science),
            ("treasury", self.treasury),
            ("happiness", self.happiness),
            ("seats at -10 or below", self.very_unhappy),
            ("golden ages", self.golden_ages),
            ("gold buildings", self.gold_buildings),
            ("happiness buildings", self.happy_buildings),
            ("food delivered", self.delivered),
            ("faith", self.faith),
            ("religion founded", self.religion),
            ("own cities of own religion", self.faithful_cities),
            ("followers", self.followers),
        ]
    }

    fn round_name(&self) -> String {
        self.round.map_or_else(|| "end".to_owned(), |r| format!("r{r}"))
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Timing {
    pub median: f64,
    pub p95: f64,
}

impl Timing {
    fn of(values: &[f64]) -> Self {
        let (median, p95, _) = percentiles(values);
        Self { median, p95 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchLine {
    pub label: String,
    pub rounds: u32,
    pub winner: Option<u8>,
    pub side: Option<Side>,
    pub victory: Option<String>,
    pub hash: String,
    /// Every nation's seat at the end (reports saved before read none): a
    /// nation keeps its start in both seatings of a seed, so its two seats
    /// compare the versions on one start.
    #[serde(default)]
    pub seats: Vec<SeatLine>,
}

/// One nation's seat of a match at its end.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeatLine {
    pub player: u8,
    pub leader: String,
    pub side: Side,
    pub rank: u32,
    pub end: Standing,
    pub combat: CombatRecord,
    pub founded: u32,
    /// Its standings at the checkpoints (reports saved before read none).
    #[serde(default)]
    pub checkpoints: Vec<Standing>,
    /// The rounds it reached the pace's milestones (reports saved before
    /// read none).
    #[serde(default)]
    pub pace: crate::bench::play::Pace,
}

impl SeatLine {
    fn of(seat: &SeatResult) -> Self {
        Self {
            player: seat.player.0,
            leader: seat.leader.clone(),
            side: seat.side,
            rank: seat.rank,
            end: seat.standings.last().copied().unwrap_or_default(),
            combat: seat.combat,
            founded: seat.founded,
            checkpoints: seat.standings.split_last().map_or_else(Vec::new, |(_, at)| at.to_vec()),
            pace: seat.pace.clone(),
        }
    }
}

/// How a tournament was run.
#[derive(Debug, Clone)]
pub struct RunInfo<'a> {
    pub rules: &'a str,
    pub suite: &'a Suite,
    pub mode: Mode,
    pub pairing: &'a Pairing,
    pub seed_offset: u64,
    pub jobs: usize,
    pub wall_seconds: f64,
}

impl TournamentReport {
    pub fn new(run: &RunInfo<'_>, results: &[MatchResult]) -> Self {
        let sides = match run.mode {
            Mode::Versus => vec![(Side::Candidate, &run.pairing.candidate), (Side::Baseline, &run.pairing.baseline)],
            Mode::Solo => vec![(Side::Candidate, &run.pairing.candidate)],
        };
        let round_ms: Vec<f64> = results.iter().flat_map(|r| r.round_ms.iter().copied()).collect();
        Self {
            rules: run.rules.to_owned(),
            suite: run.suite.name.to_owned(),
            mode: run.mode,
            baseline: run.pairing.baseline.clone(),
            candidate: run.pairing.candidate.clone(),
            seed_offset: run.seed_offset,
            games: results.len() as u32,
            rounds: run.suite.rounds,
            checkpoints: run.suite.checkpoints.clone(),
            speed: run.suite.speed.clone(),
            result_hash: format!("{:016x}", result_hash(results)),
            versus: (run.mode == Mode::Versus).then(|| versus(results)),
            sides: sides
                .into_iter()
                .map(|(side, variant)| side_summary(side, variant, results, run.suite.rounds, &run.suite.checkpoints))
                .collect(),
            round_ms: Timing::of(&round_ms),
            wall_seconds: run.wall_seconds,
            jobs: run.jobs,
            matches: results.iter().map(match_line).collect(),
            sequential: None,
        }
    }

    /// The report for a person: the head-to-head line, the sides, their
    /// standings (and every match with `matches`).
    pub fn markdown(&self, matches: bool) -> String {
        let mut out = String::new();
        let title = match self.mode {
            Mode::Versus => format!("`{}` against `{}`", self.candidate, self.baseline),
            Mode::Solo => format!("`{}` on every seat", self.candidate),
        };
        let speed = match self.speed.rsplit('/').next() {
            Some(s) if !s.is_empty() => format!(" at {s} speed"),
            _ => String::new(),
        };
        let _ = writeln!(out, "## AI bench: {} / {}{speed}: {title}\n", self.rules, self.suite);
        let _ = writeln!(
            out,
            "{} games, rounds ≤ {}, seed offset {}, result hash `{}`; wall {:.1} s on {} threads, round ms p50 {:.1} p95 {:.1}\n",
            self.games, self.rounds, self.seed_offset, self.result_hash, self.wall_seconds, self.jobs, self.round_ms.median, self.round_ms.p95
        );
        if let Some(v) = &self.versus {
            v.markdown(&mut out);
            if let Some(s) = &self.sequential {
                s.markdown(&mut out);
            }
            let combats: Vec<(&str, CombatRecord)> = self.sides.iter().map(|s| (s.variant.as_str(), s.combat)).collect();
            conquest_headline(&combats, &mut out);
        }
        self.sides_table(&mut out);
        self.combat_table(&mut out);
        let combats: Vec<(&SideSummary, CombatRecord)> = self.sides.iter().map(|s| (s, s.combat)).collect();
        wars_table(&combats, &mut out);
        for side in &self.sides {
            let _ = writeln!(out, "{} found rival capitals: {}\n", side.variant, side.scouting.line());
        }
        for side in self.sides.iter().filter(|s| s.tempo.seats > 0) {
            let _ = writeln!(out, "{} opening tempo (rounds 1–{}): {}\n", side.variant, side.tempo.rounds, side.tempo.line());
        }
        self.standings_table(&mut out);
        for side in &self.sides {
            side.pace.markdown(&side.variant, &mut out);
        }
        for side in &self.sides {
            side.science.markdown(&side.variant, &mut out);
        }
        for side in &self.sides {
            leaders_table(&side.variant, &side.leaders, &mut out);
        }
        if matches {
            self.matches_table(&mut out);
        }
        out
    }

    fn sides_table(&self, out: &mut String) {
        let _ = writeln!(out, "| side | variant | seats | wins | by kind | mean rank | eliminated | rejected % | no command % | turn ms p50 | turn ms p95 | AI ms p50 | AI ms p95 |");
        let _ = writeln!(out, "|---|---|---:|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|");
        for s in &self.sides {
            let wins: u32 = s.wins.values().sum();
            let kinds: Vec<String> = s.wins.iter().map(|(k, n)| format!("{k} {n}")).collect();
            let _ = writeln!(
                out,
                "| {:?} | {} | {} | {wins} | {} | {:.2} | {} | {:.2} | {:.1} | {:.2} | {:.2} | {:.2} | {:.2} |",
                s.side,
                s.variant,
                s.seat_games,
                kinds.join(", "),
                s.mean_rank,
                s.eliminations,
                100.0 * s.rejected_share,
                100.0 * s.no_command_share,
                s.turn_ms.median,
                s.turn_ms.p95,
                s.think_ms.median,
                s.think_ms.p95
            );
        }
        out.push('\n');
    }

    /// Totals over the side's seats: cities taken and lost, units killed
    /// and lost in fights, wars declared and declared on it.
    fn combat_table(&self, out: &mut String) {
        let rows = self.sides.iter().map(|s| (format!("{:?}", s.side), s.variant.as_str(), s.combat));
        combat_rows(rows, out);
    }

    fn standings_table(&self, out: &mut String) {
        let header: Vec<String> = self.sides.iter().map(|s| format!("{} (mean ± 95 %)", s.variant)).collect();
        let _ = writeln!(out, "| round | metric | {} |", header.join(" | "));
        let _ = writeln!(out, "|---|---|{}", "---:|".repeat(self.sides.len()));
        let Some(first) = self.sides.first() else { return };
        for (at, standing) in first.standings.iter().enumerate() {
            for (m, (name, _)) in standing.metrics().iter().enumerate() {
                let cells: Vec<String> = self
                    .sides
                    .iter()
                    .map(|s| {
                        let e = s.standings[at].metrics()[m].1;
                        format!("{:.1} ± {:.1}", e.mean, e.half_width())
                    })
                    .collect();
                let _ = writeln!(out, "| {} | {name} | {} |", standing.round_name(), cells.join(" | "));
            }
        }
        out.push('\n');
    }

    fn matches_table(&self, out: &mut String) {
        let _ = writeln!(out, "| match | rounds | winner | side | victory | hash |");
        let _ = writeln!(out, "|---|---:|---|---|---|---|");
        for m in &self.matches {
            let winner = m.winner.map_or_else(|| "—".to_owned(), |p| format!("P{p}"));
            let side = m.side.map_or_else(|| "—".to_owned(), |s| format!("{s:?}"));
            let victory = m.victory.as_deref().unwrap_or("—");
            let _ = writeln!(out, "| {} | {} | {winner} | {side} | {victory} | `{}` |", m.label, m.rounds, m.hash);
        }
        out.push('\n');
    }
}

/// The wars each side declared, by how they ended, and their mean length
/// in rounds.
fn wars_table(sides: &[(&SideSummary, CombatRecord)], out: &mut String) {
    let _ = writeln!(out, "| side | variant | wars declared | peace by deal | peace alone | elimination | running at the end | mean rounds | median rounds |");
    let _ = writeln!(out, "|---|---|---:|---:|---:|---:|---:|---:|---:|");
    for (s, c) in sides {
        let mean = f64::from(c.war_rounds) / f64::from(c.wars.max(1));
        let _ = writeln!(
            out,
            "| {:?} | {} | {} | {} | {} | {} | {} | {mean:.1} | {} |",
            s.side,
            s.variant,
            c.wars,
            c.peace_by_deal,
            c.peace_alone,
            c.eliminated,
            c.at_the_end,
            c.median_war().map_or_else(|| "-".to_owned(), |(lo, hi)| format!("{lo}-{hi}"))
        );
    }
    out.push('\n');
}

/// FNV-1a over every match's label, rounds and final world hash.
fn result_hash(results: &[MatchResult]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for r in results {
        feed(r.label.as_bytes());
        feed(&r.rounds.to_le_bytes());
        feed(&r.hash.to_le_bytes());
    }
    hash
}

fn versus(results: &[MatchResult]) -> Versus {
    let games = results.len() as u32;
    let candidate_wins = results
        .iter()
        .filter(|r| r.winner().is_some_and(|w| w.side == Side::Candidate))
        .count() as u32;
    let draws = results.iter().filter(|r| r.winner().is_none()).count() as u32;
    let expected = results
        .iter()
        .map(|r| r.seats.iter().filter(|s| s.side == Side::Candidate).count() as f64 / r.seats.len().max(1) as f64)
        .sum::<f64>()
        / f64::from(games.max(1));
    Versus::of(seed_blocks(results), candidate_wins, draws, expected)
}

/// The games grouped by seed, in the order of the seeds' first games.
fn seed_blocks(results: &[MatchResult]) -> Vec<SeedBlock> {
    let mut blocks: Vec<SeedBlock> = Vec::new();
    for r in results {
        let at = match blocks.iter().position(|b| b.seed == r.seed) {
            Some(at) => at,
            None => {
                blocks.push(SeedBlock { seed: r.seed, games: 0, points: 0.0, score_difference: 0.0 });
                blocks.len() - 1
            }
        };
        let block = &mut blocks[at];
        block.games += 1;
        block.points += candidate_points(r);
        block.score_difference += final_score(r, Side::Candidate) - final_score(r, Side::Baseline);
    }
    blocks
}

/// 1 for the candidate's win, ½ for a game without a winner.
pub(crate) fn candidate_points(r: &MatchResult) -> f64 {
    match r.winner() {
        Some(w) if w.side == Side::Candidate => 1.0,
        Some(_) => 0.0,
        None => 0.5,
    }
}

/// The mean final score of a side's seats in one game.
fn final_score(r: &MatchResult, side: Side) -> f64 {
    let scores = r.seats.iter().filter(|s| s.side == side).map(|s| s.standings.last().map_or(0.0, |e| e.score as f64));
    Estimate::of(scores).mean
}

fn side_summary(side: Side, variant: &str, results: &[MatchResult], last_round: u32, checkpoints: &[u32]) -> SideSummary {
    let seats: Vec<&SeatResult> = results.iter().flat_map(|r| r.seats.iter()).filter(|s| s.side == side).collect();
    let mut wins = BTreeMap::new();
    for kind in seats.iter().filter_map(|s| s.victory) {
        *wins.entry(kind.to_owned()).or_insert(0) += 1;
    }
    let rounds: Vec<Option<u32>> = checkpoints.iter().map(|r| Some(*r)).chain([None]).collect();
    let standings = rounds
        .iter()
        .enumerate()
        .map(|(at, round)| {
            let values: Vec<Standing> = seats.iter().filter_map(|s| s.standings.get(at).copied()).collect();
            StandingSummary::of(*round, &values)
        })
        .collect();
    let applied: u64 = seats.iter().map(|s| s.applied).sum();
    let rejected: u64 = seats.iter().map(|s| s.rejected).sum();
    let no_command: u64 = seats.iter().map(|s| s.no_command).sum();
    let turn_ms: Vec<f64> = seats.iter().flat_map(|s| s.turn_ms.iter().copied()).collect();
    let think_ms: Vec<f64> = seats.iter().flat_map(|s| s.think_ms.iter().copied()).collect();
    let share = |part: u64, all: u64| if all == 0 { 0.0 } else { part as f64 / all as f64 };
    SideSummary {
        side,
        variant: variant.to_owned(),
        seat_games: seats.len() as u32,
        wins,
        mean_rank: Estimate::of(seats.iter().map(|s| f64::from(s.rank))).mean,
        eliminations: seats.iter().filter(|s| s.eliminated.is_some()).count() as u32,
        standings,
        rejected_share: share(rejected, applied + rejected),
        no_command_share: share(no_command, applied + rejected + no_command),
        turn_ms: Timing::of(&turn_ms),
        think_ms: Timing::of(&think_ms),
        combat: seats.iter().map(|s| s.combat).fold(CombatRecord::default(), CombatRecord::add),
        leaders: leaders(&seats),
        scouting: Scouting::of(&seats),
        tempo: Tempo::of(&seats),
        pace: PaceSummary::of(side, results),
        science: ScienceSummary::of(side, results, last_round, checkpoints),
    }
}

fn leaders(seats: &[&SeatResult]) -> BTreeMap<String, LeaderRecord> {
    let mut out: BTreeMap<String, LeaderRecord> = BTreeMap::new();
    for seat in seats.iter().filter(|s| !s.leader.is_empty()) {
        out.entry(seat.leader.clone()).or_default().add(&LeaderRecord::of_seat(seat));
    }
    out
}

/// One table row per leader of a side: seats, wins, means of the final
/// score, cities, population, technologies, military, food delivered and
/// wonders, combat totals, goals at the end.
fn leaders_table(variant: &str, leaders: &BTreeMap<String, LeaderRecord>, out: &mut String) {
    if leaders.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "| {variant}: leader | seats | wins | score | cities | founded | fewest founded | population | techs | military | food delivered | wonders | captured | lost | wars | opening decisions a round | longest idle run | goal at the end |"
    );
    let _ = writeln!(out, "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|");
    for (name, r) in leaders {
        let mean = |sum: f64| sum / f64::from(r.seats.max(1));
        let paths: Vec<String> = r.paths.iter().map(|(p, n)| format!("{p} {n}")).collect();
        let _ = writeln!(
            out,
            "| {name} | {} | {} | {:.0} | {:.1} | {:.1} | {} | {:.1} | {:.1} | {:.0} | {:.0} | {:.1} | {} | {} | {} | {} | {} | {} |",
            r.seats,
            r.wins,
            mean(r.score as f64),
            mean(f64::from(r.cities)),
            mean(f64::from(r.founded)),
            r.fewest_founded,
            mean(f64::from(r.population)),
            mean(f64::from(r.techs)),
            mean(r.military as f64),
            mean(r.delivered as f64),
            mean(f64::from(r.wonders)),
            r.combat.captured,
            r.combat.cities_lost,
            r.combat.wars,
            if r.tempo.seats == 0 { "—".to_owned() } else { format!("{:.2}", r.tempo.per_round()) },
            r.tempo.idle.last().map_or_else(|| "—".to_owned(), u32::to_string),
            paths.join(", ")
        );
    }
    out.push('\n');
}

fn match_line(r: &MatchResult) -> MatchLine {
    let winner = r.winner();
    MatchLine {
        label: r.label.clone(),
        rounds: r.rounds,
        winner: winner.map(|w| w.player.0),
        side: winner.map(|w| w.side),
        victory: winner.and_then(|w| w.victory).map(str::to_owned),
        hash: format!("{:016x}", r.hash),
        seats: r.seats.iter().map(SeatLine::of).collect(),
    }
}

/// Head to head on several independent suites (`--repeat`): each run's
/// share, their spread, and every run's seed blocks pooled — the runs are
/// one stratum (the same suite), each block weighted by its games.
pub fn repeats_markdown(reports: &[TournamentReport]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "## Repeats: {} independent suites\n", reports.len());
    let _ = writeln!(out, "| seed offset | candidate wins | draws | share | 95 % CI | seed blocks | design effect | score difference | result hash |");
    let _ = writeln!(out, "|---:|---:|---:|---:|---|---:|---:|---:|---|");
    let runs: Vec<&Versus> = reports.iter().filter_map(|r| r.versus.as_ref()).collect();
    for (report, v) in reports.iter().zip(&runs) {
        let _ = writeln!(
            out,
            "| {} | {}/{} | {} | {:.1} % | {:.1}–{:.1} % | {} | {} | {:+.1} | `{}` |",
            report.seed_offset,
            v.candidate_wins,
            v.games,
            v.draws,
            100.0 * v.share.mean,
            100.0 * v.share.low,
            100.0 * v.share.high,
            v.share.blocks,
            v.design_effect.map_or_else(|| "—".to_owned(), |e| format!("{e:.2}")),
            v.score_difference.mean,
            report.result_hash
        );
    }
    let shares = Estimate::of(runs.iter().map(|v| v.share.mean));
    let (min, max) = runs.iter().fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v.share.mean), hi.max(v.share.mean)));
    let _ = writeln!(
        out,
        "\nSpread between runs: mean {:.1} %, sd {:.1} %, min {:.1} %, max {:.1} %.\n\n### Pooled: one stratum, every seed block weighted by its games\n",
        100.0 * shares.mean,
        100.0 * shares.sd,
        100.0 * min,
        100.0 * max,
    );
    Versus::pooled(&runs).markdown(&mut out);
    if let Some(first) = reports.first() {
        let combats: Vec<(&str, CombatRecord)> = first
            .sides
            .iter()
            .enumerate()
            .map(|(at, side)| (side.variant.as_str(), reports.iter().filter_map(|r| r.sides.get(at)).map(|s| s.combat).fold(CombatRecord::default(), CombatRecord::add)))
            .collect();
        conquest_headline(&combats, &mut out);
    }
    pooled_combat(reports, &mut out);
    pooled_wars(reports, &mut out);
    pooled_sides(reports, &mut out);
    out
}

/// Each side's wins by kind and seats by leader over every report.
fn pooled_sides(reports: &[TournamentReport], out: &mut String) {
    let Some(first) = reports.first() else { return };
    for (at, side) in first.sides.iter().enumerate() {
        let mut wins: BTreeMap<String, u32> = BTreeMap::new();
        let mut leaders: BTreeMap<String, LeaderRecord> = BTreeMap::new();
        let mut scouting = Scouting::default();
        for report in reports {
            let Some(s) = report.sides.get(at) else { continue };
            scouting.add(&s.scouting);
            for (kind, n) in &s.wins {
                *wins.entry(kind.clone()).or_insert(0) += n;
            }
            for (name, record) in &s.leaders {
                leaders.entry(name.clone()).or_default().add(record);
            }
        }
        let kinds: Vec<String> = wins.iter().map(|(k, n)| format!("{k} {n}")).collect();
        let _ = writeln!(out, "{} wins by kind: {}\n", side.variant, kinds.join(", "));
        let _ = writeln!(out, "{} found rival capitals: {}\n", side.variant, scouting.line());
        leaders_table(&side.variant, &leaders, out);
    }
}

/// The wars of each side over every report.
fn pooled_wars(reports: &[TournamentReport], out: &mut String) {
    let Some(first) = reports.first() else { return };
    let combats: Vec<(&SideSummary, CombatRecord)> = first
        .sides
        .iter()
        .enumerate()
        .map(|(at, side)| (side, reports.iter().filter_map(|r| r.sides.get(at)).map(|s| s.combat).fold(CombatRecord::default(), CombatRecord::add)))
        .collect();
    wars_table(&combats, out);
}

/// The combat totals of each side over every report.
fn pooled_combat(reports: &[TournamentReport], out: &mut String) {
    let Some(first) = reports.first() else { return };
    let rows = first.sides.iter().enumerate().map(|(at, side)| {
        let c = reports.iter().filter_map(|r| r.sides.get(at)).map(|s| s.combat).fold(CombatRecord::default(), CombatRecord::add);
        (format!("{:?}", side.side), side.variant.as_str(), c)
    });
    combat_rows(rows, out);
}

/// The headline beside the points share: the cities each side captured and
/// held at the end of its games (the normal aim of a war); then the capitals
/// each side lost (a capital taken is rare: a secondary number).
fn conquest_headline(combats: &[(&str, CombatRecord)], out: &mut String) {
    let sides: Vec<String> = combats.iter().map(|(variant, c)| format!("`{variant}` captured {} and held {} at the end", c.captured, c.held)).collect();
    let capitals: Vec<String> = combats.iter().map(|(variant, c)| format!("`{variant}` {}", c.capitals_lost)).collect();
    let _ = writeln!(out, "**Cities: {}.** Capitals lost: {}.\n", sides.join("; "), capitals.join(", "));
}

/// One line of combat totals per side; «attacks won» are the wars declared
/// on the side in which the declarer took one of its cities.
fn combat_rows<'s>(rows: impl Iterator<Item = (String, &'s str, CombatRecord)>, out: &mut String) {
    let _ = writeln!(out, "| side | variant | cities captured | captured cities held at the end | cities lost | capitals lost | units killed | units lost | wars declared | wars on it | attacks won |");
    let _ = writeln!(out, "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for (side, variant, c) in rows {
        let _ = writeln!(
            out,
            "| {side} | {variant} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            c.captured, c.held, c.cities_lost, c.capitals_lost, c.kills, c.units_lost, c.wars, c.attacked, c.attacks_lost
        );
    }
    out.push('\n');
}

/// This report against a stored one of another revision (both solo on the
/// same suite): the difference of every mean with its 95 % interval; `*`
/// marks a difference whose interval leaves out zero.
pub fn comparison_markdown(current: &TournamentReport, stored: &TournamentReport) -> Result<String, String> {
    if current.mode != Mode::Solo || stored.mode != Mode::Solo {
        return Err("--compare reads solo reports (--solo) on both sides".to_owned());
    }
    if (current.rules.as_str(), current.suite.as_str(), current.seed_offset) != (stored.rules.as_str(), stored.suite.as_str(), stored.seed_offset) {
        return Err(format!(
            "the stored report is {}/{} offset {}, this one {}/{} offset {}",
            stored.rules, stored.suite, stored.seed_offset, current.rules, current.suite, current.seed_offset
        ));
    }
    let (now, then) = (&current.sides[0], &stored.sides[0]);
    let mut out = String::new();
    let same = if current.result_hash == stored.result_hash { "equal: the same games" } else { "differ" };
    let _ = writeln!(out, "## Against the stored report (`{}` then, `{}` now)\n", stored.candidate, current.candidate);
    let _ = writeln!(out, "Result hashes {} → {}: {same}.\n", stored.result_hash, current.result_hash);
    let _ = writeln!(out, "| round | metric | stored | now | now − stored (95 %) |");
    let _ = writeln!(out, "|---|---|---:|---:|---:|");
    for (a, b) in now.standings.iter().zip(&then.standings) {
        for ((name, x), (_, y)) in a.metrics().iter().zip(b.metrics().iter()) {
            let (difference, half) = x.minus(y);
            let mark = if difference.abs() > half { " *" } else { "" };
            let _ = writeln!(out, "| {} | {name} | {:.1} | {:.1} | {difference:+.1} ± {half:.1}{mark} |", a.round_name(), y.mean, x.mean);
        }
    }
    let line = |label: &str, then: String, now: String| format!("| {label} | {then} | {now} |\n");
    out.push_str("\n| | stored | now |\n|---|---|---|\n");
    out.push_str(&line("wins by kind", format!("{:?}", then.wins), format!("{:?}", now.wins)));
    out.push_str(&line("eliminated", then.eliminations.to_string(), now.eliminations.to_string()));
    let combat = |c: CombatRecord| {
        format!("{} / {} / {} / {} / {} / {} / {} / {}", c.captured, c.cities_lost, c.capitals_lost, c.kills, c.units_lost, c.wars, c.attacked, c.attacks_lost)
    };
    let label = "captured / cities lost / capitals lost / kills / units lost / wars / wars on it / attacks won";
    out.push_str(&line(label, combat(then.combat), combat(now.combat)));
    out.push_str(&line("mean rank", format!("{:.2}", then.mean_rank), format!("{:.2}", now.mean_rank)));
    out.push_str(&line("rejected %", format!("{:.2}", 100.0 * then.rejected_share), format!("{:.2}", 100.0 * now.rejected_share)));
    out.push_str(&line("no command %", format!("{:.1}", 100.0 * then.no_command_share), format!("{:.1}", 100.0 * now.no_command_share)));
    out.push_str(&line("turn ms p50 / p95", format!("{:.2} / {:.2}", then.turn_ms.median, then.turn_ms.p95), format!("{:.2} / {:.2}", now.turn_ms.median, now.turn_ms.p95)));
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::play::Pairing;
    use kernel::ids::PlayerId;

    /// A duel: the candidate on player 0, the baseline on player 1.
    fn duel(seed: u64, winner: Option<Side>, scores: [i64; 2]) -> MatchResult {
        let seat = |at: usize, side: Side| SeatResult {
            player: PlayerId(at as u8),
            side,
            leader: String::new(),
            path: None,
            rank: 1,
            victory: (winner == Some(side)).then_some("time"),
            eliminated: None,
            standings: vec![Standing { score: scores[at], ..Standing::default() }],
            applied: 0,
            rejected: 0,
            no_command: 0,
            turn_ms: vec![1.0],
            think_ms: vec![1.0],
            delivered: 0,
            combat: CombatRecord::default(),
            founded: 1,
            sightings: Vec::new(),
            opening: Vec::new(),
            pace: crate::bench::play::Pace::default(),
        };
        MatchResult {
            label: format!("seed {seed}"),
            seed,
            rounds: 300,
            hash: seed,
            seats: vec![seat(0, Side::Candidate), seat(1, Side::Baseline)],
            round_ms: vec![1.0],
            eras: Vec::new(),
            space_parts: 0,
        }
    }

    fn report(results: &[MatchResult]) -> TournamentReport {
        let suite = Suite::named("duel").unwrap();
        let pairing = Pairing { baseline: "allocator".to_owned(), candidate: "standard".to_owned() };
        let run = RunInfo { rules: "classic", suite: &suite, mode: Mode::Versus, pairing: &pairing, seed_offset: 0, jobs: 1, wall_seconds: 0.0 };
        TournamentReport::new(&run, results)
    }

    #[test]
    fn games_are_grouped_by_seed() {
        let results = [
            duel(1, Some(Side::Candidate), [700, 600]),
            duel(2, None, [500, 500]),
            duel(1, Some(Side::Baseline), [600, 640]),
            duel(2, Some(Side::Candidate), [520, 480]),
        ];
        let versus = report(&results).versus.unwrap();
        let blocks: Vec<(u64, u32, f64, f64)> = versus.blocks.iter().map(|b| (b.seed, b.games, b.points, b.score_difference)).collect();
        assert_eq!(blocks, [(1, 2, 1.0, 60.0), (2, 2, 1.5, 40.0)]);
        assert_eq!((versus.candidate_wins, versus.draws, versus.games), (2, 1, 4));
        assert_eq!((versus.share.mean, versus.score_difference.mean, versus.expected), (0.625, 25.0, 0.5));
        assert_eq!(versus.verdict(), Verdict::TooFewBlocks, "two seeds decide nothing");
    }

    /// The same games give the same report, intervals included; pooling
    /// two runs keeps every seed block.
    #[test]
    fn the_report_is_reproducible() {
        let results: Vec<MatchResult> = (0..40)
            .map(|i| {
                let winner = if i % 3 == 0 { Side::Baseline } else { Side::Candidate };
                duel(i / 2, Some(winner), [600 + i as i64, 600])
            })
            .collect();
        let (one, two) = (report(&results), report(&results));
        assert_eq!(one.markdown(true), two.markdown(true));
        let json = |r: &TournamentReport| serde_json::to_string(r).unwrap();
        assert_eq!(json(&one), json(&two));
        let versus = one.versus.as_ref().unwrap();
        assert_eq!(versus.share.blocks, 20);
        assert_eq!(versus.verdict(), Verdict::Stronger);

        let later: Vec<MatchResult> = results.iter().map(|r| MatchResult { seed: r.seed + 1000, ..r.clone() }).collect();
        let pooled = Versus::pooled(&[versus, report(&later).versus.as_ref().unwrap()]);
        assert_eq!((pooled.share.blocks, pooled.games, pooled.share.mean), (40, 80, versus.share.mean));
        assert!(pooled.share.high - pooled.share.low < versus.share.high - versus.share.low);
        assert_eq!(repeats_markdown(&[one.clone(), two.clone()]), repeats_markdown(&[one, two]));
    }
}
