//! The games of stage E2 (classic/docs/ai-neural-architecture.md, «The E2
//! recipe»; the files: classic/docs/ai-net-data.md, «E2: the net's own
//! games»): the learner net on its seats samples its choices
//! (`net_e2e::sample`), opponents from a list play the other nations, and
//! each game is kept as its command log (`.aurlog` + `.json`, as `ai-play`
//! writes them), the sampled decisions (`.e2.bin`, `sample::Trace`) and the
//! game's setup and outcome per seat (`.e2.json`, [`GameMeta`]). No shard is
//! written while playing: `ai-features --e2` makes one from the log on
//! demand (`crate::e2`).
//!
//! A game is a function of its index: the group (`index / group size`)
//! fixes the map, the seed, the seats and the opponents; the member (`index
//! % group size`) only the sampling seed (GRPO: K continuations of one
//! start). A game whose `.e2.json` exists is done and not played again.
//!
//! Mid-game starts ([`FromLogs`], the curriculum toward the science
//! victory): the group's start is another game's log (an expert's, the
//! bench's, `ai-play`'s) replayed to the start of a round R; the learner
//! takes over one of its nations ([`LearnerSeat`]) and every seat plays on
//! from there with a fresh AI state (the net's memory empty). The game's log
//! is the whole game, the replayed prefix and the continuation, so
//! `ai-features --e2` replays it from round 0; the sidecar holds only the
//! decisions after R.

use std::path::{Path, PathBuf};
use std::time::Instant;

use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::net_e2e::sample::{Sampling, Trace};
use classic_rules::ai::variant::Variant;
use classic_rules::classic::Classic;
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::seats::Seated;
use kernel::ai::StepOutcome;
use kernel::ids::PlayerId;
use kernel::persistence::codec::CodecError;
use kernel::replay::hash::authoritative_hash;
use kernel::replay::journal::{take_record, JournalHeader, Record};
use serde::{Deserialize, Serialize};

use crate::bench::modules::{majors, BenchRules, ClassicBench};
use crate::bench::suite::{Mode, Suite};
use crate::export::{read_log, start, LogSpec};

/// Which nation of a replayed game the learner takes over at round R.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearnerSeat {
    /// The living nation with the most science a turn at R (ties: the
    /// lower id), as `ai-finish` picks its leader.
    BestScience,
    /// A living nation drawn from the group's index.
    Random,
    /// This player (a living nation).
    Player(u8),
}

impl LearnerSeat {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "best-science" => Ok(Self::BestScience),
            "random" => Ok(Self::Random),
            n => n.parse().map(Self::Player).map_err(|_| format!("--learner-seat: best-science, random or a player id, not {n}")),
        }
    }

    pub fn name(self) -> String {
        match self {
            Self::BestScience => "best-science".to_owned(),
            Self::Random => "random".to_owned(),
            Self::Player(n) => n.to_string(),
        }
    }
}

/// Mid-game starts: group `g` continues log `logs[(offset + g) % len]`
/// from the start of round `round` ± `jitter` (drawn from the group's index
/// and the log's seed, so a group's members share the position).
#[derive(Debug, Clone)]
pub struct FromLogs {
    /// The source logs (`.aurlog`, the `.json` spec beside each), in order.
    pub logs: Vec<(PathBuf, LogSpec)>,
    pub round: u32,
    pub jitter: u32,
    pub seat: LearnerSeat,
    /// The share of the groups whose learner is given the spaceship's
    /// technologies at R (0: none), decided per group ([`Start::give_space_techs`]).
    pub give_space_techs: f64,
}

impl FromLogs {
    /// The logs of `dir` (`*.aurlog` with a `.json` beside), sorted by
    /// name, those of the world sizes `sizes` (empty: all).
    pub fn read(dir: &Path, sizes: &[String], round: u32, jitter: u32, seat: LearnerSeat) -> Result<Self, String> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| Some(e.ok()?.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "aurlog") && p.with_extension("json").exists())
            .collect();
        paths.sort();
        let mut logs = Vec::new();
        for path in paths {
            let json = path.with_extension("json");
            let bytes = std::fs::read(&json).map_err(|e| format!("{}: {e}", json.display()))?;
            let spec: LogSpec = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", json.display()))?;
            if sizes.is_empty() || sizes.contains(&spec.size) {
                logs.push((path, spec));
            }
        }
        if logs.is_empty() {
            return Err(format!("no logs in {}", dir.display()));
        }
        Ok(Self { logs, round, jitter, seat, give_space_techs: 0.0 })
    }
}

/// Where a mid-game job starts: the source log and the round it is
/// replayed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    pub log: PathBuf,
    pub round: u32,
    /// The learner learns every technology of `grand_strategy::space_closure`
    /// at R, as `ai-finish` gives them ([`Gift`]).
    pub give_space_techs: bool,
}

/// Technologies given at a mid-game start (not commands: the log does not
/// hold them, so the E2 export applies them again before command
/// `position`, `e2::shard_with`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gift {
    /// The commands applied before it (the replayed prefix's).
    pub position: u64,
    /// Per learner, the technologies learnt, in order.
    pub players: Vec<(u8, Vec<u16>)>,
}

/// What the generator plays.
#[derive(Debug, Clone)]
pub struct Config {
    pub suite: Suite,
    /// The world sizes kept of the suite's maps (empty: all).
    pub sizes: Vec<String>,
    /// The last round (the time victory).
    pub rounds: u32,
    /// The learner: `<weights.bin>` or `dummy`, optionally `+<roles>` as on
    /// the bench (`net:<file>+founder`).
    pub net: String,
    /// Learner seats a game (consecutive nations from the group's first).
    pub net_seats: usize,
    /// The opponents' minds by the bench's names (`standard`,
    /// `pre-war-fix`, `net:<file>` …), dealt to the other seats in turn.
    pub opponents: Vec<String>,
    /// The sampling temperature on the Q8 logits (0: argmax).
    pub temperature: f64,
    /// Games of one start that differ only in the sampling seed (GRPO).
    pub group: usize,
    pub seed_offset: u64,
    /// Salt of every game's sampling seed.
    pub sample_seed: u64,
    /// Mid-game starts from logs (None: every game from round 0 on the
    /// suite's maps).
    pub from: Option<FromLogs>,
    /// A bench mind on the learner's seats instead of the sampling net
    /// (a control: no decision is recorded).
    pub learner_mind: Option<String>,
    /// The game speed's key of the games from round 0 (empty: Standard; a
    /// mid-game start keeps its log's speed).
    pub speed: String,
    /// Training grounds instead of whole games (`grounds::Grounds`; None:
    /// whole games).
    pub grounds: Option<crate::grounds::Grounds>,
    /// The decision heads the learner net plays (a comma list of
    /// `encode::head::NAMES`, e.g. `unit`); the others stay with its
    /// scripted base (`NET_BASE`, `standard`). None: every head (or
    /// `AURELIA_NET_HEADS`, as on the bench). Only the net's own decisions
    /// are sampled and recorded, so training sees those heads alone.
    pub heads: Option<String>,
    /// Battles: value the net's unit decisions by the search teacher's
    /// rollouts (`search::Values`, `--decision-values`; None: not valued).
    pub decision_values: Option<crate::search::ValuesConfig>,
    /// `--ladder-solvable`: a siege ladder scene the shallow search teacher
    /// does not take in the learner's first turn is drawn again
    /// (`ladder_draw`); off: every scene as drawn.
    pub ladder_solvable: bool,
}

/// Draws of a siege ladder scene at most (`--ladder-solvable`): the first
/// and seven more.
pub const LADDER_DRAWS: u32 = 8;

/// How a scene was drawn under `--ladder-solvable` (`.e2.json` `ladder_draw`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LadderDraw {
    /// The draw played (0: the scene as drawn without the filter).
    pub attempt: u32,
    /// Draws probed.
    pub draws: u32,
    /// The draw played is solvable (else none of `LADDER_DRAWS` was, and
    /// the first is played).
    pub solved: bool,
    /// A scene of the pool (`--ladder-pool`): solvable by the pool's own
    /// probe, not probed again (`draws` 0).
    #[serde(default)]
    pub pooled: bool,
}

/// A pool of solvable siege ladder scenes (`--ladder-pool FILE`,
/// `tools/ladder-pool.py`): the battle settings it was drawn with and per
/// rung the scenes the shallow search teacher, playing the learner, took
/// — in the learner's first turn where the scene allows one by
/// construction (`BattleScene::same_turn`), else within the war — each as
/// the (seed offset, index) of its job in a run of one game a group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LadderPool {
    pub suite: String,
    pub sizes: Vec<String>,
    pub speed: String,
    pub rounds: (u32, u32),
    pub army: (u32, u32),
    pub minimal: bool,
    pub vary: bool,
    pub rungs: std::collections::BTreeMap<u32, Vec<(u64, usize)>>,
}

static POOL: std::sync::OnceLock<LadderPool> = std::sync::OnceLock::new();

/// Sets the process's ladder pool (`ai-selfplay --ladder-pool`).
pub fn set_ladder_pool(pool: LadderPool) {
    let _ = POOL.set(pool);
}

/// The rung's pool, if the process has one for it.
fn pooled(rung: u32) -> Option<&'static Vec<(u64, usize)>> {
    POOL.get()?.rungs.get(&rung).filter(|v| !v.is_empty())
}

/// `--ladder-pool`: each siege ladder job of a rung the pool has plays a
/// scene of the pool instead of its own: the entry drawn from the job's
/// suite seed and group (a group's members share it), its world and scene
/// (`spec`, the ground's start, length, army and scene seed) those of the
/// pool's job, rebuilt from its seed offset and index; the job keeps its
/// group, member, sampling seed and difficulty. The pool's battle settings
/// must be the config's. Returns the jobs replaced.
pub fn pool_jobs(config: &Config, jobs: &mut [Job]) -> Result<usize, String> {
    let Some(pool) = POOL.get() else { return Ok(0) };
    let Some(crate::grounds::Grounds::Battle { kind: crate::grounds::BattleKind::Siege, rounds, army, minimal, vary, .. }) = &config.grounds else { return Ok(0) };
    let mine = (config.suite.name.to_owned(), config.sizes.clone(), config.speed.clone(), *rounds, *army, *minimal, *vary);
    let theirs = (pool.suite.clone(), pool.sizes.clone(), pool.speed.clone(), pool.rounds, pool.army, pool.minimal, pool.vary);
    let mut replaced = 0;
    for job in jobs.iter_mut() {
        let Some(crate::grounds::GroundJob::Battle { rung: Some(rung), difficulty, .. }) = job.ground else { continue };
        let Some(entries) = pooled(rung) else { continue };
        // Checked where a job would play a pool scene (a run of no game, or of another rung, takes any pool).
        if mine != theirs {
            return Err(format!("--ladder-pool: drawn with {theirs:?}, this run plays {mine:?}"));
        }
        let (offset, index) = entries[(mix(job.spec.seed ^ ((job.group as u64) << 20) ^ 0x9001) % entries.len() as u64) as usize];
        let source = Config { seed_offset: offset, group: 1, ..config.clone() };
        let Some(from) = jobs_of_ground(&source, index + 1).into_iter().nth(index) else { return Err(format!("--ladder-pool: rung {rung} has no job {index} at offset {offset}")) };
        job.spec = from.spec;
        job.ground = from.ground.map(|g| match g {
            crate::grounds::GroundJob::Battle { kind, start, length, army, seed, stage, rung, vary, .. } => crate::grounds::GroundJob::Battle { kind, start, length, army, seed, difficulty, stage, rung, vary },
            other => other,
        });
        replaced += 1;
    }
    Ok(replaced)
}

/// `jobs` with the grounds assigned (the pool's source jobs).
fn jobs_of_ground(config: &Config, games: usize) -> Vec<Job> {
    jobs(config, games)
}

/// One game to play.
#[derive(Debug, Clone)]
pub struct Job {
    pub index: usize,
    pub group: usize,
    pub member: usize,
    pub spec: LogSpec,
    /// The game's sampling seed (`sample::Sampling::seed`).
    pub sample_seed: u64,
    /// A mid-game start (None: from round 0).
    pub start: Option<Start>,
    /// A training ground (`grounds::GroundJob`), drawn per group.
    pub ground: Option<crate::grounds::GroundJob>,
}

impl Job {
    /// The file stem of the game's files: `g<group>m<member>-<seed>-<suite>-<script>-<size>`,
    /// `-r<R>` after it for a mid-game start.
    pub fn stem(&self) -> String {
        let from = self.start.as_ref().map_or_else(String::new, |s| format!("-r{}", s.round));
        let ground = self.ground.as_ref().map_or_else(String::new, |g| g.suffix());
        format!("g{:05}m{:02}-{}{from}{ground}", self.group, self.member, self.spec.name())
    }
}

/// A seat of a played game and how it ended (`.e2.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeatOutcome {
    pub player: u8,
    /// The mind's name (`learner` for the sampling net).
    pub mind: String,
    pub learner: bool,
    /// The module's score at the end (the time victory's measure).
    pub score: i64,
    /// The reward: 1 for the winner; without a winner the best score at the
    /// end shared among the living who hold it; else 0.
    pub points_share: f64,
    /// The bench's rank: the winner, then the living by score, then the
    /// eliminated, the later out the higher; ties by player.
    pub rank: u32,
    pub alive: bool,
    /// The round it was eliminated in.
    pub eliminated: Option<u32>,
    /// The kind of the victory when this seat won.
    pub victory: Option<String>,
    /// The round of that victory.
    #[serde(default)]
    pub victory_round: Option<u32>,
    /// The science victory's progress at the end ([`Space`]; zeros for a
    /// nation out of the game).
    #[serde(default)]
    pub space: Space,
}

/// A nation's way to the science victory at the end of a game, from the
/// sources of the bench's science section (`bench::science`,
/// `BenchRules::spaceship`, `BenchRules::economy`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Space {
    /// Technologies known.
    pub techs: u32,
    /// Technologies of the spaceship's closure (`grand_strategy::space_closure`)
    /// known, and the closure's size.
    pub space_techs: u32,
    pub space_techs_needed: u32,
    /// The projects the parts need first (`victory::prerequisite_projects`,
    /// Apollo) done, and their count.
    pub projects: u32,
    pub projects_needed: u32,
    /// Parts installed (the rules' count, `BenchRules::spaceship`), parts
    /// built or installed (installed plus the part units on the map, each
    /// kind at most its need), and the parts a ship needs.
    pub parts_installed: u32,
    pub parts: u32,
    pub parts_needed: u32,
    /// Science a turn (`BenchRules::economy`).
    pub science: i32,
}

/// How a game ended (None in `GameMeta::victory`: not decided, the last
/// round played without the time victory).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Victory {
    pub winner: u8,
    /// `time`, `science`, `domination`, `culture`, `diplomatic`.
    pub kind: String,
    pub round: u32,
}

/// The learners' potential (`grounds::potential`: the empire's yields a
/// turn and stocks at the scripted AI's shadow prices) round by round, the
/// trainer's dense shaping reward (`train_e2.potential_turn_rewards`).
/// Entry 0 is the world when the learner starts playing (the start of the
/// first round played: after a mid-game start's prefix, a battle's scene),
/// entry k the start of round `round[k]` (after the round before it was
/// played); the last entry the world at the end (its round repeats the one
/// before when the game ended within a round). The learner's turn of round
/// R did what lies between the entry of R and the next one.
///
/// Per learner one price sheet values every entry: its own, taken at the
/// first entry where it holds a city (a game from round 0 starts with
/// none, and the prices of an empire without a city are no empire's),
/// priced by the scripted AI's variant (the source log's for a mid-game
/// start, else `standard`). A fixed sheet makes the potential a function of
/// the holdings alone, and a group's members share their start, so their
/// sheets are alike. The raw amounts are kept per component, so another
/// pricing is possible without a replay.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PotentialRounds {
    pub round: Vec<u32>,
    pub seats: Vec<SeatPotentialRounds>,
}

/// One learner's column of [`PotentialRounds`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SeatPotentialRounds {
    pub player: u8,
    /// The sheet `total` is valued at (None: the learner never held a city;
    /// `total` is then 0).
    pub prices: Option<crate::grounds::PriceSheet>,
    /// The potential per entry (score points, to 0.01).
    pub total: Vec<f32>,
    /// Per component of `grounds::YIELDS` and `grounds::STOCKS` its raw
    /// amount per entry (zeros once the learner is out).
    pub raw: std::collections::BTreeMap<String, Vec<f32>>,
}

/// [`PotentialRounds`] while the game plays.
struct PotentialTrack {
    variant: Variant,
    players: Vec<PlayerId>,
    round: Vec<u32>,
    holdings: Vec<Vec<crate::grounds::Holdings>>,
    prices: Vec<Option<crate::grounds::PriceSheet>>,
}

impl PotentialTrack {
    fn new(players: &[PlayerId], variant: Variant) -> Self {
        Self { variant, players: players.to_vec(), round: Vec::new(), holdings: vec![Vec::new(); players.len()], prices: vec![None; players.len()] }
    }

    /// The entry of the world now.
    fn push(&mut self, game: &GameState, rules: &CompiledRuleset) {
        self.round.push(round(game));
        for (i, p) in self.players.iter().enumerate() {
            let h = crate::grounds::holdings(game, rules, *p);
            if self.prices[i].is_none() && h.raw.get("cities").copied().unwrap_or(0.0) > 0.0 {
                self.prices[i] = Some(crate::grounds::price_sheet(game, rules, *p, self.variant));
            }
            self.holdings[i].push(h);
        }
    }

    fn finish(self) -> PotentialRounds {
        let seats = self
            .players
            .iter()
            .zip(self.holdings)
            .zip(self.prices)
            .map(|((p, hs), prices)| {
                let total = hs.iter().map(|h| prices.as_ref().map_or(0.0, |s| ((crate::grounds::potential(h, s).total * 100.0).round() / 100.0) as f32)).collect();
                let raw = crate::grounds::YIELDS
                    .iter()
                    .chain(&crate::grounds::STOCKS)
                    .map(|k| ((*k).to_owned(), hs.iter().map(|h| h.raw.get(*k).copied().unwrap_or(0.0) as f32).collect()))
                    .collect();
                SeatPotentialRounds { player: p.0, prices, total, raw }
            })
            .collect();
        PotentialRounds { round: self.round, seats }
    }
}

/// The `.e2.json` of a game: its setup, the sampling, the outcome per seat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameMeta {
    /// 3 (2: before the round-by-round potential; 1: before the mid-game
    /// starts and the space fields; older files read with the newer fields
    /// absent).
    pub format: u32,
    pub stem: String,
    pub index: usize,
    pub group: usize,
    pub member: usize,
    pub group_size: usize,
    pub spec: LogSpec,
    pub final_round: u32,
    /// The world's hash at the end.
    pub hash: u64,
    /// Commands applied (the journal's).
    pub commands: u64,
    pub temperature: f64,
    pub sample_seed: u64,
    /// The learner's weights (`dummy`: the linear stand-in).
    pub net: String,
    pub learners: Vec<u8>,
    /// A bench mind played the learner's seats (no sampling, no decision
    /// recorded); None: the net.
    #[serde(default)]
    pub learner_mind: Option<String>,
    /// The heads the learner net played (`Config::heads`); None: every head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_heads: Option<String>,
    /// `--learner-mind search`: what the search did, its deferrals and the
    /// decisions that offered the defer choice (`search::SearchStats`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<crate::search::SearchStats>,
    /// `--decision-values`: the net's unit decisions valued by the search's
    /// rollouts (`search::ValuesRecord`: per decision Q of the choice, the
    /// mean and the best of the candidates valued).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_values: Option<crate::search::ValuesRecord>,
    /// `--ladder-solvable`: how the siege ladder scene was drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ladder_draw: Option<LadderDraw>,
    /// A mid-game start: the round the game was replayed to, the source
    /// log, the rule that chose the learner's seat, the commands of the
    /// replayed prefix and the world's hash at R (equal across a group).
    #[serde(default)]
    pub from_round: Option<u32>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub learner_seat: Option<String>,
    #[serde(default)]
    pub from_commands: Option<u64>,
    #[serde(default)]
    pub from_hash: Option<u64>,
    /// The learner was given the spaceship's technologies at R
    /// (`--give-space-techs`), and what it was given.
    #[serde(default)]
    pub gave_space_techs: bool,
    #[serde(default)]
    pub gift: Option<Gift>,
    /// A training ground: its kind, raw outcome and the changes made
    /// outside the commands (`grounds::GroundRecord`; None: a whole game).
    #[serde(default)]
    pub ground: Option<crate::grounds::GroundRecord>,
    /// The learners' potential round by round ([`PotentialRounds`], format
    /// 3; None in older files): the trainer's dense shaping reward.
    #[serde(default)]
    pub potential: Option<PotentialRounds>,
    /// The game's victory (None: not decided).
    #[serde(default)]
    pub victory: Option<Victory>,
    pub seats: Vec<SeatOutcome>,
    pub turns: u64,
    pub decisions: u64,
    /// Format 9: the learners' unit decisions that offered the defer
    /// candidate, and those that took it (0 for a net without defer).
    #[serde(default)]
    pub defer_offered: u64,
    #[serde(default)]
    pub defers: u64,
    pub seconds: f64,
    pub log_bytes: u64,
    pub sidecar_bytes: u64,
}

/// SplitMix64's finaliser.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The first `games` games: groups of `config.group` games over the suite's
/// maps (those of `config.sizes`) and seeds, offset after offset.
pub fn jobs(config: &Config, games: usize) -> Vec<Job> {
    let mut out = jobs_of(config, games);
    if let Some(grounds) = &config.grounds {
        crate::grounds::assign(grounds, &mut out);
    }
    out
}

fn jobs_of(config: &Config, games: usize) -> Vec<Job> {
    let group = config.group.max(1);
    let groups = games.div_ceil(group);
    if let Some(from) = &config.from {
        return (0..games)
            .map(|index| {
                let (g, member) = (index / group, index % group);
                let (log, source) = &from.logs[(config.seed_offset as usize + g) % from.logs.len()];
                let spread = u64::from(from.jitter) * 2 + 1;
                let shift = (mix(source.seed ^ ((g as u64) << 32) ^ 0x5eed) % spread) as i64 - i64::from(from.jitter);
                // At most the source's last round: the log holds the rounds before it.
                let base = config.grounds.as_ref().and_then(|x| x.start_round(source.seed, g)).unwrap_or(from.round);
                // R = 0: an opening window, the game from its start (no jitter).
                let round = if base == 0 { 0 } else { (i64::from(base) + shift).clamp(1, i64::from(source.rounds.max(1))) as u32 };
                // The source's game (its speed too): only the time victory and the label change.
                let spec = LogSpec { rounds: config.rounds, variant: "net".to_owned(), ..source.clone() };
                let sample_seed = mix(mix(config.sample_seed ^ source.seed) ^ ((g as u64) << 24) ^ member as u64);
                let give_space_techs = from.give_space_techs > 0.0 && ((mix(source.seed ^ ((g as u64) << 40) ^ 0x9e1f) >> 11) as f64 / (1u64 << 53) as f64) < from.give_space_techs;
                Job { index, group: g, member, spec, sample_seed, start: Some(Start { log: log.clone(), round, give_space_techs }), ground: None }
            })
            .collect();
    }
    let mut starts = Vec::new();
    let mut offset = config.seed_offset;
    while starts.len() < groups {
        let before = starts.len();
        for m in config.suite.matches(Mode::Solo, offset) {
            if starts.len() < groups && (config.sizes.is_empty() || config.sizes.iter().any(|s| s == m.map.size)) {
                starts.push(m);
            }
        }
        if starts.len() == before {
            break;
        }
        offset += 1;
    }
    (0..games.min(starts.len() * group))
        .map(|index| {
            let (g, member) = (index / group, index % group);
            let m = starts[g];
            let spec = LogSpec {
                suite: config.suite.name.to_owned(),
                seed: m.seed,
                script: m.map.script.to_owned(),
                size: m.map.size.to_owned(),
                rounds: config.rounds,
                difficulty: "core:handicap/prince".to_owned(),
                variant: "net".to_owned(),
                speed: config.speed.clone(),
                bare: false,
            };
            let sample_seed = mix(mix(config.sample_seed ^ m.seed) ^ ((g as u64) << 24) ^ member as u64);
            Job { index, group: g, member, spec, sample_seed, start: None, ground: None }
        })
        .collect()
}

/// A played game.
#[derive(Debug)]
pub struct Played {
    /// The kernel's command journal (the `.aurlog`).
    pub journal: Vec<u8>,
    pub trace: Trace,
    pub meta: GameMeta,
}

/// The learner's players of a game of `nations` nations in group `group`,
/// as indices among the nations: `net_seats` consecutive from `group %
/// nations` on.
pub fn learner_seats(group: usize, nations: usize, net_seats: usize) -> Vec<usize> {
    (0..net_seats.min(nations)).map(|j| (group + j) % nations).collect()
}

/// The round the game is in.
fn round(game: &GameState) -> u32 {
    game.view().auth.turn().round
}

/// Replays the log `start.log` into `game` (a new game of the same map and
/// seed, its journal on) up to the start of round `start.round`: every
/// command of the rounds before it. The log's round hashes are checked on
/// the source's own world: `game` itself when it is the source's start,
/// else (another time victory: its round is part of the hash) a second
/// world of the source's spec in lockstep, the same commands applied to
/// both. Marks the nations eliminated on the way. Returns the commands
/// replayed.
fn replay_prefix(rules: &CompiledRuleset, game: &mut GameState, start_at: &Start, nations: &[PlayerId], eliminated: &mut [Option<u32>]) -> Result<u64, String> {
    replay_to(rules, game, &start_at.log, start_at.round, nations, eliminated, false)
}

/// `replay_prefix` of the log `log` to the start of round `to`; `short_ok`:
/// a log that ends (or a game that is over) before it is replayed to its end
/// instead of an error.
fn replay_to(rules: &CompiledRuleset, game: &mut GameState, log: &Path, to: u32, nations: &[PlayerId], eliminated: &mut [Option<u32>], short_ok: bool) -> Result<u64, String> {
    let (source, bytes) = read_log(log)?;
    let start_at = Start { log: log.to_path_buf(), round: to, give_space_techs: false };
    let label = start_at.log.display();
    let (header, mut reader) = JournalHeader::read(&bytes).map_err(|e| format!("{label}: {e}"))?;
    if header.rules != rules.hash() {
        return Err(format!("{label}: the log was recorded with other rules"));
    }
    let mut twin = if header.world == authoritative_hash(game) { None } else { Some(start(rules, &source.game()?)) };
    if let Some(twin) = &twin {
        if header.world != authoritative_hash(twin) {
            return Err(format!("{label}: the log starts from world {:016x}, its spec generates {:016x}", header.world, authoritative_hash(twin)));
        }
    }
    let mut commands = 0u64;
    let mut record = 0usize;
    while !reader.is_at_end() {
        record += 1;
        let next = match take_record::<Classic>(&mut reader, twin.as_ref().unwrap_or(game).view().auth) {
            Ok(next) => next,
            Err(CodecError::Truncated) => break,
            Err(error) => return Err(format!("{label}: record {record} is damaged ({error:?})")),
        };
        match next {
            Record::Command(command) => {
                if round(game) >= start_at.round || game.is_over() {
                    break;
                }
                let at = round(game);
                let refused = |e| format!("{label}: diverged in round {at} at record {record}: the command is refused ({e:?})");
                if let Some(twin) = twin.as_mut() {
                    twin.execute(rules, command.clone()).map_err(refused)?;
                }
                game.execute(rules, command).map_err(refused)?;
                commands += 1;
            }
            Record::Checkpoint { round: done, hash } => {
                let now = authoritative_hash(twin.as_ref().unwrap_or(game));
                if now != hash {
                    return Err(format!("{label}: diverged after round {done} (record {record}): world {now:016x}, recorded {hash:016x}"));
                }
                for (i, p) in nations.iter().enumerate() {
                    if eliminated[i].is_none() && !game.view().auth.players().is_alive(*p) {
                        eliminated[i] = Some(round(game));
                    }
                }
            }
            Record::CommandAs { .. } | Record::Window(_) => return Err(format!("{label}: record {record}: a seat window in an AI game")),
        }
    }
    if (round(game) != start_at.round || game.is_over()) && !short_ok {
        return Err(format!("{label}: the log ends in round {} (over: {}), before the start of round {}", round(game), game.is_over(), start_at.round));
    }
    Ok(commands)
}

/// The expert of an economy window: the source log `log` replayed on a new
/// world of its own spec to the start of round `round` (or its end, when it
/// ends before), every nation's holdings and prices there (`ExpertAt`), and
/// the log's AI variant (its price model). Made once per (log, round) in the
/// process (`EXPERTS`): a group's members end their windows at one round
/// and share it. Continuing each member's prefix replay instead would cost
/// as much per member; the cache costs one replay of R + W rounds per
/// group, a fraction of the window's play.
pub fn expert_at(rules: &CompiledRuleset, log: &Path, round: u32) -> Result<std::sync::Arc<(crate::grounds::ExpertAt, Variant)>, String> {
    let key = (log.to_path_buf(), round);
    let cell = EXPERTS.lock().map_err(|_| "the expert cache is poisoned".to_owned())?.entry(key).or_default().clone();
    cell.get_or_init(|| {
        let (source, _) = read_log(log)?;
        let variant = Variant::from_name(&source.variant).unwrap_or(Variant::Standard);
        let mut game = start(rules, &source.game()?);
        let nations = majors(&game);
        replay_to(rules, &mut game, log, round, &nations, &mut vec![None; nations.len()], true)?;
        Ok(std::sync::Arc::new((crate::grounds::ExpertAt::of(&game, rules, &nations, variant), variant)))
    })
    .clone()
}

type ExpertCell = std::sync::Arc<std::sync::OnceLock<Result<std::sync::Arc<(crate::grounds::ExpertAt, Variant)>, String>>>;

/// The experts made in this process, by (source log, round).
static EXPERTS: std::sync::Mutex<std::collections::BTreeMap<(PathBuf, u32), ExpertCell>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Gives `player` every technology of the spaceship's closure
/// (`grand_strategy::space_closure`) it lacks, each once its prerequisites
/// are known, the lowest id first — as `ai-finish` does. Returns them in
/// the order learnt.
pub fn give_space_techs(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) -> Vec<u16> {
    let mut closure: Vec<classic_rules::ids::TechId> = classic_rules::ai::grand_strategy::space_closure(rules).into_iter().collect();
    closure.sort_by_key(|t| t.0);
    let mut given = Vec::new();
    loop {
        let known = game.view().auth.players().techs_of(player).to_vec();
        let next = closure.iter().copied().find(|t| !known.contains(t) && rules.technologies[t.0 as usize].requires.iter().all(|r| known.contains(r)));
        match next {
            Some(tech) => {
                game.debug_discover(rules, player, tech);
                given.push(tech.0);
            }
            None => break,
        }
    }
    given
}

/// Applies a recorded gift again (the E2 export's replay).
pub fn apply_gift(game: &mut GameState, rules: &CompiledRuleset, gift: &Gift) {
    for (player, techs) in &gift.players {
        for tech in techs {
            game.debug_discover(rules, PlayerId(*player), classic_rules::ids::TechId(*tech));
        }
    }
}

/// The world a job starts from: the new game, replayed to its round for a
/// mid-game start.
pub fn start_position(rules: &CompiledRuleset, job: &Job) -> Result<GameState, String> {
    let mut game = start(rules, &job.spec.game()?);
    if let Some(at) = &job.start {
        let nations = majors(&game);
        replay_prefix(rules, &mut game, at, &nations, &mut vec![None; nations.len()])?;
    }
    Ok(game)
}

/// The seat rule at the start: `best-science` at the game's first round
/// (an opening window: no nation makes science yet) is `random`.
fn seat_rule(seat: LearnerSeat, game: &GameState) -> LearnerSeat {
    if seat == LearnerSeat::BestScience && round(game) == 0 {
        LearnerSeat::Random
    } else {
        seat
    }
}

/// The nation (its index among `nations`) the learner takes over.
fn pick_seat(game: &GameState, rules: &CompiledRuleset, nations: &[PlayerId], seat: LearnerSeat, group: usize, seed: u64) -> Result<usize, String> {
    let auth = game.view().auth;
    let alive: Vec<usize> = (0..nations.len()).filter(|&i| auth.players().is_alive(nations[i])).collect();
    if alive.is_empty() {
        return Err("no nation is alive".to_owned());
    }
    match seat {
        LearnerSeat::BestScience => Ok(*alive
            .iter()
            .max_by_key(|&&i| (classic_rules::derived::income::player_income(auth, rules, nations[i]).science, std::cmp::Reverse(nations[i].0)))
            .expect("a living nation")),
        LearnerSeat::Random => Ok(alive[(mix(seed ^ ((group as u64) << 16) ^ 0x5ea7) % alive.len() as u64) as usize]),
        LearnerSeat::Player(n) => alive.iter().copied().find(|&i| nations[i].0 == n).ok_or_else(|| format!("player {n} is no living nation at the start")),
    }
}

/// Plays `job` to its end (or its last round); a mid-game job from the
/// start of its round in the source log on.
pub fn play(rules: &CompiledRuleset, config: &Config, job: &Job) -> Result<Played, String> {
    let clock = Instant::now();
    // `--ladder-solvable`: the first draw of the siege ladder scene the shallow search takes in the learner's first turn.
    let pool = match job.ground {
        Some(crate::grounds::GroundJob::Battle { rung: Some(rung), .. }) => pooled(rung).is_some(),
        _ => false,
    };
    let (ladder_draw, drawn) = if pool && ladder_scene(job) {
        // A scene of the pool (`pool_jobs`): solvable by the pool's probe.
        (Some(LadderDraw { attempt: 0, draws: 0, solved: true, pooled: true }), None)
    } else {
        match (config.ladder_solvable && ladder_scene(job)).then(|| solvable_draw(rules, config, job)).transpose()?.flatten() {
            Some(draw) => (Some(draw), Some(redrawn(job, draw.attempt))),
            None => (None, None),
        }
    };
    let job = drawn.as_ref().unwrap_or(job);
    // A varied battle's world: its own, or another draw with a war to set (`grounds::fit_world`).
    let fitted = crate::grounds::fit_world(rules, job)?;
    let job = &fitted;
    let mut game = start(rules, &job.spec.game()?);
    let mut journal = game.begin_journal(rules);
    let nations = majors(&game);
    let mut eliminated: Vec<Option<u32>> = vec![None; nations.len()];
    // A battle's world: the suite game played by `standard` to the war's round.
    let battle = crate::grounds::play_prefix(rules, &mut game, &job.spec, job.ground.as_ref())?;
    let prefix = match &job.start {
        Some(at) => Some(replay_prefix(rules, &mut game, at, &nations, &mut eliminated)?),
        None => battle.as_ref().map(|(commands, _)| *commands),
    };
    let from_hash = prefix.map(|_| authoritative_hash(&game));
    let prefix_commands = prefix.unwrap_or(0);
    let mut scene = crate::grounds::Scene::set(rules, &mut game, job.ground.as_ref(), &nations, battle.as_ref().map(|(_, k)| k), prefix_commands)?;
    let seat_name = config.from.as_ref().filter(|_| job.start.is_some()).map(|f| seat_rule(f.seat, &game).name());
    let learners = match (&job.start, &config.from) {
        _ if scene.learner().is_some() => learner_seats(nations.iter().position(|p| Some(*p) == scene.learner()).unwrap_or(0), nations.len(), 1),
        (Some(_), Some(from)) => learner_seats(pick_seat(&game, rules, &nations, seat_rule(from.seat, &game), job.group, job.spec.seed)?, nations.len(), config.net_seats),
        _ => learner_seats(job.group, nations.len(), config.net_seats),
    };
    let sampling = Sampling { temperature: config.temperature, seed: job.sample_seed };
    let mind = |name: &str| ClassicBench::mind(name).ok_or_else(|| format!("no AI variant {name}"));
    // `--learner-mind search[:…]`: the search teacher (`search.rs`), its base mind on the learner's seat.
    let search_config = config.learner_mind.as_deref().and_then(crate::search::SearchConfig::parse).transpose()?;
    let mut seat_names = Vec::new();
    let opponents = if config.opponents.is_empty() { vec![crate::bench::modules::SCRIPTED_EXPERT.to_owned()] } else { config.opponents.clone() };
    let mut seats = Vec::new();
    let mut names = Vec::new();
    let mut dealt = 0;
    for (i, player) in nations.iter().enumerate() {
        if learners.contains(&i) {
            let seat = match &config.learner_mind {
                Some(name) => {
                    let name = search_config.as_ref().map_or(name.as_str(), |s| s.base.as_str());
                    seat_names.push((*player, name.to_owned()));
                    mind(name)?
                }
                None => {
                    let net = match &config.heads {
                        None => mind(&format!("net:{}", config.net))?,
                        Some(heads) => net_with_heads(&config.net, heads)?,
                    };
                    net.net().expect("a net seat").set_sampling(sampling);
                    net
                }
            };
            seats.push((*player, seat));
            names.push((*player, "learner".to_owned(), true));
        } else {
            let name = &opponents[(job.group + dealt) % opponents.len()];
            dealt += 1;
            seats.push((*player, mind(name)?));
            seat_names.push((*player, name.clone()));
            names.push((*player, name.clone(), false));
        }
    }
    let others = mind(&opponents[0])?;
    let learner_players: Vec<PlayerId> = learners.iter().map(|&i| nations[i]).collect();
    let gift = match &job.start {
        Some(at) if at.give_space_techs => Some(Gift { position: prefix_commands, players: learner_players.iter().map(|p| (p.0, give_space_techs(&mut game, rules, *p))).collect() }),
        _ => None,
    };
    let samples = config.learner_mind.is_none();
    let mut driver = AiDriver::<Classic, Seated<ClassicMind>>::with_mind(ClassicBench::settings(rules), Vec::new(), Seated::new(others, seats));
    driver.set_trace_capacity(0);
    scene.aim(&mut driver, &game);
    // The learners' potential at the start and after every round (the trainer's dense shaping).
    let price_variant = match (&job.start, &config.from) {
        (Some(at), Some(from)) => from.logs.iter().find(|(p, _)| *p == at.log).and_then(|(_, s)| Variant::from_name(&s.variant)),
        _ => None,
    };
    let mut track = PotentialTrack::new(&learner_players, price_variant.unwrap_or(Variant::Standard));
    track.push(&game, rules);
    let mut over = false;
    // A scripted learner's strikes in a unit fight (`grounds::BattleScene::strike`): commands outside the driver.
    let scripted = config.learner_mind.is_some();
    // `--decision-values`: the net's unit decisions in a battle valued by the search's rollouts, the scripted
    // expert (`SCRIPTED_EXPERT`) its base.
    let mut values = match (&config.decision_values, &scene) {
        (Some(c), crate::grounds::Scene::Battle(_)) if samples => {
            let mut seats = seat_names.clone();
            seats.extend(learner_players.iter().map(|p| (*p, crate::bench::modules::SCRIPTED_EXPERT.to_owned())));
            Some(crate::search::Values::new(c.clone(), seats, opponents[0].clone(), job.sample_seed))
        }
        _ => None,
    };
    // The learner's decisions seen so far (a step that decides a unit records one).
    let mut seen = 0usize;
    let mut search = search_config.map(|c| crate::search::Search::new(c, seat_names, opponents[0].clone()));
    let mut struck = 0u64;
    while !game.is_over() && round(&game) <= job.spec.rounds && !over {
        if scripted {
            let position = prefix_commands + driver.counters().commands_applied + struck;
            struck += crate::search::scripted_turn(search.as_mut(), rules, &mut game, &mut scene, &driver, position, true);
        }
        loop {
            // Positions count the log's commands from round 0: the replayed prefix first.
            let position = u32::try_from(prefix_commands + driver.counters().commands_applied + struck).map_err(|_| "more than 2^32 commands".to_owned())?;
            let outcome = driver.step(&mut game, rules);
            if samples {
                for p in &learner_players {
                    driver.mind_mut().mind(*p).net().expect("a learner seat").stamp_trace(position);
                }
            }
            if let (Some(values), crate::grounds::Scene::Battle(battle)) = (values.as_mut(), &scene) {
                // The driver decides a unit in a step of its own and plays its command in the next:
                // the world now is the world of the decision.
                let net = driver.mind_mut().mind(learner_players[0]).net().expect("a learner seat");
                let last = net
                    .peek_trace(|t| {
                        let n = t.decisions();
                        let at = t.turns.len().checked_sub(1).filter(|&i| n == seen + 1 && !t.turns[i].decisions.is_empty());
                        let last = at.map(|i| {
                            let turn = &t.turns[i];
                            let d = turn.decisions.len() - 1;
                            (turn.round, d as u32, turn.decisions[d].clone(), t.defer_offered(i, d, kernel::ai::driver::DEFERS_PER_UNIT))
                        });
                        (n, last)
                    })
                    .expect("the learner samples");
                seen = last.0;
                if let Some((turn_round, ordinal, record, offered)) = last.1.filter(|l| i32::from(l.2.head) == classic_rules::ai::net_e2e::encode::head::UNIT) {
                    values.value(rules, &game, battle, &driver, turn_round, ordinal, &record, offered);
                }
            }
            match outcome {
                StepOutcome::Worked => {}
                StepOutcome::PlayerDone(player) => {
                    scene.after_player(&game, rules, player);
                    if scripted && !game.is_over() {
                        let position = prefix_commands + driver.counters().commands_applied + struck;
                        struck += crate::search::scripted_turn(search.as_mut(), rules, &mut game, &mut scene, &driver, position, false);
                    }
                }
                StepOutcome::RoundDone => break,
                StepOutcome::Waiting if game.is_over() => break,
                StepOutcome::Waiting => return Err(format!("round {} waits for a human", round(&game))),
            }
        }
        journal.extend(game.take_journal());
        for (i, p) in nations.iter().enumerate() {
            if eliminated[i].is_none() && !game.view().auth.players().is_alive(*p) {
                eliminated[i] = Some(round(&game));
            }
        }
        let position = prefix_commands + driver.counters().commands_applied + struck;
        over = scene.after_round(rules, &mut game, &mut driver, position);
        track.push(&game, rules);
    }
    journal.extend(game.take_journal());
    if let Some(v) = &values {
        let t = &v.record.stats;
        eprintln!("decision values {}: {} decisions, {} valued, {} sampled out, {} skipped, {} mismatched, {} rollouts, {:.1} s", job.stem(), t.decisions, t.valued, t.sampled_out, t.skipped, t.mismatched, t.rollouts, t.seconds);
    }
    if let Some(s) = &search {
        let t = &s.stats;
        eprintln!("search {}: {} turns, {} decisions, {} rollouts, {} commands, {} deferrals, {:.1} s", job.stem(), t.turns, t.decisions, t.rollouts, t.commands, t.deferrals.len(), t.seconds);
    }
    // An economy window's expert: the source log at the round the window stopped.
    let expert = match (&job.start, scene.window_end()) {
        (Some(at), Some(end)) => Some(expert_at(rules, &at.log, end)?),
        _ => None,
    };
    // The learners' traces in the order of the game.
    let mut trace = Trace::new(sampling);
    if samples {
        for p in &learner_players {
            let mine = driver.mind_mut().mind(*p).net().expect("a learner seat").take_trace().expect("the learner samples");
            trace.defer |= mine.defer;
            trace.turns.extend(mine.turns);
        }
    }
    trace.turns.sort_by_key(|t| t.position);
    let deferrals = kernel::ai::driver::DEFERS_PER_UNIT;
    let defer_offered = (0..trace.turns.len()).map(|t| (0..trace.turns[t].decisions.len()).filter(|&d| trace.defer_offered(t, d, deferrals)).count() as u64).sum();
    let defers = trace.turns.iter().flat_map(|t| &t.decisions).filter(|d| d.chosen.is_some() && d.chosen_meta[0] == classic_rules::ai::net_e2e::encode::tag::DEFER).count() as u64;
    let seats = outcome(&game, rules, &nations, &eliminated, &names);
    let meta = GameMeta {
        format: 3,
        stem: job.stem(),
        index: job.index,
        group: job.group,
        member: job.member,
        group_size: config.group.max(1),
        spec: job.spec.clone(),
        final_round: round(&game),
        hash: authoritative_hash(&game),
        commands: prefix_commands + driver.counters().commands_applied + struck,
        temperature: config.temperature,
        sample_seed: job.sample_seed,
        net: config.net.clone(),
        learners: learner_players.iter().map(|p| p.0).collect(),
        learner_mind: config.learner_mind.clone(),
        search: search.map(|s| s.stats),
        decision_values: values.map(|v| v.record),
        ladder_draw,
        net_heads: config.heads.clone().filter(|_| config.learner_mind.is_none()),
        from_round: job.start.as_ref().map(|s| s.round),
        source: job.start.as_ref().map(|s| s.log.display().to_string()),
        learner_seat: seat_name,
        from_commands: prefix,
        from_hash,
        gave_space_techs: gift.is_some(),
        gift,
        ground: scene.finish(&game, rules, &learner_players, expert.as_ref().map(|e| (&e.0, e.1))),
        potential: Some(track.finish()),
        victory: game.outcome().map(|o| Victory { winner: o.winner.0, kind: crate::spec::victory_name(o.kind).to_owned(), round: o.round }),
        seats,
        turns: trace.turns.len() as u64,
        decisions: trace.decisions() as u64,
        defer_offered,
        defers,
        seconds: clock.elapsed().as_secs_f64(),
        log_bytes: 0,
        sidecar_bytes: 0,
    };
    Ok(Played { journal, trace, meta })
}

/// A siege ladder job (`--ladder-solvable` filters its scenes).
fn ladder_scene(job: &Job) -> bool {
    matches!(job.ground, Some(crate::grounds::GroundJob::Battle { kind: crate::grounds::BattleKind::Siege, rung: Some(_), .. }))
}

/// Draws of a scene that keep the job's world before another world is
/// drawn (`redrawn`).
const SCENE_DRAWS: u32 = 4;

/// Draw `attempt` of the job's siege ladder scene (0: the job itself): its
/// scene seed mixed with the attempt (where the target, the units and their
/// places are drawn, `grounds::set_battle`); from attempt `SCENE_DRAWS` on
/// also another world (the suite seed a stride past `fit_world`'s own
/// draws), its stem unchanged.
fn redrawn(job: &Job, attempt: u32) -> Job {
    let mut out = job.clone();
    if attempt == 0 {
        return out;
    }
    if let Some(crate::grounds::GroundJob::Battle { seed, .. }) = &mut out.ground {
        *seed = mix(*seed ^ (u64::from(attempt) << 48) ^ 0x501a_b1e);
    }
    if attempt >= SCENE_DRAWS {
        out.spec.seed += u64::from(attempt) * crate::bench::suite::SEED_STRIDE * 64;
    }
    out
}

/// The first draw of the job's siege ladder scene (`redrawn`) the shallow
/// search teacher takes in the learner's first turn on one of
/// `search::PROBE_SEEDS`, among the draws that allow a first-turn capture by
/// construction (`BattleScene::same_turn`); none of `LADDER_DRAWS`: the
/// first, unsolved. None: the scene as drawn allows none (a melee-only or a
/// march rung: the curriculum counts its captures in any turn), played as
/// drawn. A group's members share it: made once per (world, group, scene
/// seed) in the process (`DRAWS`).
fn solvable_draw(rules: &CompiledRuleset, config: &Config, job: &Job) -> Result<Option<LadderDraw>, String> {
    let seed = match &job.ground {
        Some(crate::grounds::GroundJob::Battle { seed, .. }) => *seed,
        _ => 0,
    };
    let key = (job.spec.name(), job.group, seed);
    let cell = DRAWS.lock().map_err(|_| "the draw cache is poisoned".to_owned())?.entry(key).or_default().clone();
    cell.get_or_init(|| {
        let opponents = if config.opponents.is_empty() { vec![crate::bench::modules::SCRIPTED_EXPERT.to_owned()] } else { config.opponents.clone() };
        for attempt in 0..LADDER_DRAWS {
            let Ok(variant) = crate::grounds::fit_world(rules, &redrawn(job, attempt)) else { continue };
            let Ok(spec) = variant.spec.game() else { continue };
            let mut game = start(rules, &spec);
            let nations = majors(&game);
            let Ok(battle) = crate::grounds::play_prefix(rules, &mut game, &variant.spec, variant.ground.as_ref()) else { continue };
            let position = battle.as_ref().map_or(0, |(c, _)| *c);
            let Ok(crate::grounds::Scene::Battle(scene)) = crate::grounds::Scene::set(rules, &mut game, variant.ground.as_ref(), &nations, battle.as_ref().map(|(_, k)| k), position) else { continue };
            if scene.same_turn() != Some(true) {
                if attempt == 0 {
                    return Ok(None);
                }
                continue;
            }
            let learner = scene.learner();
            let mut dealt = 0;
            let seats: Vec<(PlayerId, String)> = nations
                .iter()
                .map(|p| {
                    if *p == learner {
                        (*p, crate::bench::modules::SCRIPTED_EXPERT.to_owned())
                    } else {
                        dealt += 1;
                        (*p, opponents[(job.group + dealt - 1) % opponents.len()].clone())
                    }
                })
                .collect();
            if crate::search::PROBE_SEEDS.iter().any(|s| crate::search::first_turn_capture(rules, &game, &scene, seats.clone(), &opponents[0], *s)) {
                return Ok(Some(LadderDraw { attempt, draws: attempt + 1, solved: true, pooled: false }));
            }
        }
        Ok(Some(LadderDraw { attempt: 0, draws: LADDER_DRAWS, solved: false, pooled: false }))
    })
    .clone()
}

type DrawCell = std::sync::Arc<std::sync::OnceLock<Result<Option<LadderDraw>, String>>>;

/// The draws chosen in this process, by (world, group, scene seed).
static DRAWS: std::sync::Mutex<std::collections::BTreeMap<(String, usize, u64), DrawCell>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// The learner net `W[+roles]` (as `ClassicBench::mind`'s `net:W+roles`)
/// playing only `heads` (a comma list of `encode::head::NAMES`).
pub fn net_with_heads(net: &str, heads: &str) -> Result<ClassicMind, String> {
    use classic_rules::ai::net_e2e::mind::{parse_heads, NetPlayer, ScriptedRoles};
    let (path, roles) = net.split_once('+').unwrap_or((net, ""));
    let player = NetPlayer::from_path(path)?.with_scripted(ScriptedRoles::parse(roles)?).with_heads(parse_heads(heads)?);
    Ok(ClassicMind::with_net(Variant::Net, player))
}

/// Every nation's outcome: the reward (points share), the bench's rank
/// (`bench::play::rank`), the score, the victory.
fn outcome(game: &GameState, rules: &CompiledRuleset, nations: &[PlayerId], eliminated: &[Option<u32>], names: &[(PlayerId, String, bool)]) -> Vec<SeatOutcome> {
    let players = game.view().auth.players();
    let winner = game.outcome();
    let scores: Vec<i64> = nations.iter().map(|p| ClassicBench::score(game, rules, *p)).collect();
    let alive: Vec<bool> = nations.iter().map(|p| players.is_alive(*p)).collect();
    let best = (0..nations.len()).filter(|&i| alive[i]).map(|i| scores[i]).max().unwrap_or(0);
    let tied = (0..nations.len()).filter(|&i| alive[i] && scores[i] == best).count().max(1);
    let mut order: Vec<usize> = (0..nations.len()).collect();
    order.sort_by_key(|&i| {
        let won = winner.as_ref().is_some_and(|o| o.winner == nations[i]);
        let standing = match eliminated[i] {
            None => -scores[i],
            Some(round) => -i64::from(round),
        };
        (!won, eliminated[i].is_some(), standing, nations[i])
    });
    nations
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let won = winner.as_ref().filter(|o| o.winner == *p);
            let points_share = match &winner {
                Some(o) => f64::from(u8::from(o.winner == *p)),
                None if alive[i] && scores[i] == best => 1.0 / tied as f64,
                None => 0.0,
            };
            let (_, mind, learner) = names.iter().find(|(q, _, _)| q == p).cloned().unwrap_or((*p, String::new(), false));
            SeatOutcome {
                player: p.0,
                mind,
                learner,
                score: scores[i],
                points_share,
                rank: order.iter().position(|&j| j == i).unwrap_or(0) as u32 + 1,
                alive: alive[i],
                eliminated: eliminated[i],
                victory: won.map(|o| crate::spec::victory_name(o.kind).to_owned()),
                victory_round: won.map(|o| o.round),
                space: if alive[i] { space(game, rules, *p) } else { Space::default() },
            }
        })
        .collect()
}

/// A living nation's way to the science victory now.
pub fn space(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Space {
    let known = game.view().auth.players().techs_of(player);
    let closure = classic_rules::ai::grand_strategy::space_closure(rules);
    let prerequisites = classic_rules::ai::victory::prerequisite_projects(rules);
    let victory = game.view().auth.module().victory();
    let obs = game.ai_observation(rules, player);
    let kinds = classic_rules::ai::victory::part_kinds(rules);
    let parts: u32 = classic_rules::world::victory::spaceship_parts(rules)
        .iter()
        .map(|&(project, needed)| {
            let walking = obs.own_units.iter().filter(|u| kinds.iter().any(|(k, p, _)| *k == u.kind && *p == project)).count() as u32;
            (u32::from(victory.project_count(player, project)) + walking).min(u32::from(needed))
        })
        .sum();
    Space {
        techs: known.len() as u32,
        space_techs: known.iter().filter(|t| closure.contains(t)).count() as u32,
        space_techs_needed: closure.len() as u32,
        projects: prerequisites.iter().filter(|p| victory.project_count(player, **p) > 0).count() as u32,
        projects_needed: prerequisites.len() as u32,
        parts_installed: ClassicBench::spaceship(game, rules, player).parts,
        parts,
        parts_needed: ClassicBench::spaceship_parts(rules),
        science: ClassicBench::economy(game, rules, player).science,
    }
}

/// The paths of a game's files in `dir`.
pub fn paths(dir: &Path, stem: &str) -> [PathBuf; 4] {
    ["aurlog", "json", "e2.bin", "e2.json"].map(|ext| dir.join(format!("{stem}.{ext}")))
}

/// Whether the game is done in `dir` (its `.e2.json`, written last, exists).
pub fn done(dir: &Path, job: &Job) -> bool {
    paths(dir, &job.stem())[3].exists()
}

/// Writes the game's four files into `dir`, each through a temporary file;
/// the `.e2.json` last, so a game whose `.e2.json` exists is whole. Returns
/// the meta written.
pub fn write(dir: &Path, played: &Played) -> std::io::Result<GameMeta> {
    std::fs::create_dir_all(dir)?;
    let [log, spec, sidecar, meta_path] = paths(dir, &played.meta.stem);
    let put = |path: &Path, bytes: &[u8]| -> std::io::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    };
    let trace = played.trace.encode();
    put(&log, &played.journal)?;
    put(&spec, &serde_json::to_vec_pretty(&played.meta.spec).map_err(std::io::Error::other)?)?;
    put(&sidecar, &trace)?;
    let mut meta = played.meta.clone();
    meta.log_bytes = played.journal.len() as u64;
    meta.sidecar_bytes = trace.len() as u64;
    put(&meta_path, &serde_json::to_vec_pretty(&meta).map_err(std::io::Error::other)?)?;
    Ok(meta)
}

/// Reads a game's `.e2.json`.
pub fn read_meta(path: &Path) -> Result<GameMeta, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}
