//! The net player: the end-to-end net's choices in a `ClassicMind`
//! (classic/docs/ai-neural-architecture.md, Playing).
//!
//! A [`NetPlayer`] answers the decisions of the heads the net covers — every
//! unit command, research, the production of each city, policies — and the
//! scripted mind keeps the rest (diplomacy, congress, spies, trade, citizens,
//! purchases, religion and the religious units). Per player turn:
//!
//! 1. `begin` (the mind's `start_turn`): the turn is encoded once from the
//!    observation with the exporter's own functions (`encode::Turn::encode`)
//!    and the scorer starts the turn with the player's memory; the research
//!    choice is made here when the research queue is empty (the scripted
//!    mind researches at the same point, `planner_research`).
//! 2. `decide_unit`: the unit's candidates (`encode::unit_candidates`: the
//!    legal list, marches to points of interest, waiting) are scored and the
//!    best one the rules accept is taken (ties: the lowest index); a march is
//!    `MarchTo` that tile, waiting is no command. Weights that know the
//!    defer choice (format 9, weights version 5) also get the defer
//!    candidate last while the unit has a deferral left this turn
//!    (`kernel::ai::driver::DEFERS_PER_UNIT`); taking it is no command with
//!    `Reason::Deferred`, and the driver moves the unit to the end of the
//!    turn's queue, where it decides again.
//! 3. `end_of_turn` (the mind's `plan_end_of_turn`): production for every
//!    city the scripted mind orders for or whose queue is empty
//!    (`encode::production_candidates`), the policy where the scripted mind
//!    adopts one (`encode::policy_candidates`). The scripted decisions of the
//!    covered kinds are dropped.
//!
//! The turn trace (weights version 3): every choice the net makes passes
//! through `choose`, which adds the candidate taken to the player's
//! `TurnCache` (`Model::note_choice`), so each later decision of the turn
//! sees the earlier ones, in the order made: research at the start, the
//! opening's production, the units, the end of the turn's production, the
//! policy. Decisions the scripted mind makes (other heads, religious units,
//! scripted roles) do not enter it.
//!
//! The memory (the player slot, the nation and unit slots by persistent id,
//! `model::Memory`; and the player's event tracker, `events::Tracker`, whose
//! log the turn's event rows are) lives in the player's AI state as the
//! mind's own bytes (`AiState::mind_memory`, [`encode_memory`]), saved with
//! the game: the
//! mind keeps nothing of a player between its turns, so a loaded game and a
//! mind rebuilt every round (the game window's AI worker) play on with the
//! same memory. The memory after a turn depends only on the turn's start
//! (`end_turn` reads the tokens `begin_turn` made), so it is written at the
//! start of the turn, where the mind may change the AI state.
//!
//! Scoring is behind [`Scorer`]: [`ModelScorer`] is the integer net
//! (`model.rs`), [`LinearScorer`] a deterministic stand-in (a fixed linear
//! function of the candidate row) for tests and timing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::ai::net_e2e::encode::{self, head, tag, Candidate, Turn};
use crate::ai::net_e2e::events::Tracker;
use crate::ai::net_e2e::model::{fnv1a64, Memory, Model, Speed, TurnCache, TurnInput, MAX_OUT};
use crate::ai::net_e2e::sample::{self, Sampling, Trace};
use crate::ai::planner::Decision;
use crate::ai::state::AiState;
use crate::ai::trace::{Actor, Reason};
use crate::command::Command;
use crate::ids::{EntityId, PlayerId, ProductionItem};
use crate::knowledge::actions::acting_unit;
use crate::knowledge::observation::{AIObservation, LocalObservation, OwnCity};
use crate::knowledge::public::PlayerQueries;
use crate::math::MapDimensions;
use crate::rules::CompiledRuleset;
use crate::world::player::PlayerKind;

/// The environment variable naming the weights file of `Variant::Net`
/// (`dummy`: the [`LinearScorer`]).
pub const WEIGHTS_ENV: &str = "AURELIA_NET_WEIGHTS";

/// The value of [`WEIGHTS_ENV`] that plays the [`LinearScorer`].
pub const DUMMY: &str = "dummy";

/// The tiles of a coarse region's side (`model.rs`, coarse regions on).
const COARSE_BLOCK: u32 = 16;

/// What scores the candidates of a decision.
pub trait Scorer: std::fmt::Debug + Send + Sync {
    /// The name the reports show (the weights file, or `dummy`).
    fn label(&self) -> String;

    /// The memory of a player at the start of a game.
    fn memory(&self) -> Memory;

    /// Whether the turn input needs the coarse regions.
    fn coarse(&self) -> bool {
        false
    }

    /// Whether the scorer has the decision head (`encode::head`); a head it
    /// lacks is left to the scripted mind.
    fn has_head(&self, _head: i32) -> bool {
        true
    }

    /// The layout hash the scorer was trained on (None: any layout).
    fn layout_hash(&self) -> Option<u64> {
        None
    }

    /// The start of a player's turn: encode it with the player's memory.
    fn begin_turn(&self, input: &TurnInput<'_>, memory: &Memory, cache: &mut TurnCache);

    /// One logit per candidate into `out` (cleared first).
    fn score(&self, cache: &TurnCache, head: usize, actor: i32, candidates: &[Candidate], out: &mut Vec<i32>);

    /// The memory after the player's turn.
    fn end_turn(&self, cache: &TurnCache, memory: &Memory) -> Memory;

    /// The decision just scored took `candidate` (the turn trace: the
    /// turn's later decisions see it; `model::Model::note_choice`).
    fn note_choice(&self, _cache: &mut TurnCache, _head: usize, _actor: i32, _candidate: &Candidate) {}

    /// The widths of the player, nation and unit slots (0: no memory).
    fn memory_widths(&self) -> [usize; 3] {
        [0; 3]
    }

    /// Whether the scorer scores the defer candidate (`encode::tag::DEFER`;
    /// weights version 5 with defer on).
    fn defers(&self) -> bool {
        false
    }
}

/// A deterministic stand-in for the net: a fixed linear function of the
/// candidate row (weights −8 … 8 by column), no memory.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinearScorer;

impl LinearScorer {
    fn weight(column: usize) -> i64 {
        ((column as i64 * 37 + 11) % 17) - 8
    }
}

impl Scorer for LinearScorer {
    fn label(&self) -> String {
        DUMMY.to_owned()
    }

    fn memory(&self) -> Memory {
        Memory::default()
    }

    fn begin_turn(&self, _input: &TurnInput<'_>, _memory: &Memory, _cache: &mut TurnCache) {}

    fn score(&self, _cache: &TurnCache, _head: usize, _actor: i32, candidates: &[Candidate], out: &mut Vec<i32>) {
        out.clear();
        out.extend(candidates.iter().map(|c| {
            let sum: i64 = c.row.iter().enumerate().map(|(i, v)| i64::from(*v) * Self::weight(i)).sum();
            sum.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
        }));
    }

    fn end_turn(&self, _cache: &TurnCache, memory: &Memory) -> Memory {
        memory.clone()
    }
}

/// The integer net of a weights file (`model.rs`).
#[derive(Debug, Clone)]
pub struct ModelScorer {
    model: Arc<Model>,
    label: String,
}

impl ModelScorer {
    pub fn new(model: Arc<Model>, label: impl Into<String>) -> Self {
        Self { model, label: label.into() }
    }

    /// The net of the weights file at `path`, read once per process.
    pub fn load(path: &Path) -> Result<Self, String> {
        Ok(Self::new(load_model(path)?, path.display().to_string()))
    }

    pub fn model(&self) -> &Model {
        &self.model
    }
}

impl Scorer for ModelScorer {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn memory(&self) -> Memory {
        Memory::new(self.model.config())
    }

    fn coarse(&self) -> bool {
        self.model.config().coarse
    }

    fn has_head(&self, head: i32) -> bool {
        usize::try_from(head).is_ok_and(|h| h < self.model.config().n_decision_heads)
    }

    fn layout_hash(&self) -> Option<u64> {
        Some(self.model.layout_hash())
    }

    fn begin_turn(&self, input: &TurnInput<'_>, memory: &Memory, cache: &mut TurnCache) {
        self.model.begin_turn(input, memory, cache);
    }

    fn score(&self, cache: &TurnCache, head: usize, actor: i32, candidates: &[Candidate], out: &mut Vec<i32>) {
        out.clear();
        if candidates.is_empty() {
            return;
        }
        let mut query = [0i16; MAX_OUT];
        let query = &mut query[..self.model.config().d_model];
        self.model.query(cache, head, actor, query);
        // A net of an older format reads the prefix of the row it knows
        // (`layout_format`).
        let width = self.model.config().c_candidate;
        out.extend(candidates.iter().map(|c| self.model.score(cache, head, actor, query, &c.row[..width.min(c.row.len())], &c.meta)));
    }

    fn end_turn(&self, cache: &TurnCache, memory: &Memory) -> Memory {
        self.model.end_turn(cache, memory)
    }

    fn note_choice(&self, cache: &mut TurnCache, head: usize, actor: i32, candidate: &Candidate) {
        self.model.note_choice(cache, head, actor, &candidate.row[..self.model.config().c_candidate.min(candidate.row.len())], &candidate.meta);
    }

    fn memory_widths(&self) -> [usize; 3] {
        let cfg = self.model.config();
        [cfg.mem_player, cfg.mem_nation, cfg.mem_unit]
    }

    fn defers(&self) -> bool {
        self.model.config().defer
    }
}

/// The environment variables of the forward pass's speed
/// ([`Speed`]; none changes a value): `AURELIA_NET_SIMD=0` the portable
/// loops instead of the aarch64 kernels, `AURELIA_NET_THREADS=N` threads
/// for a turn's encoding (1), `AURELIA_NET_INCREMENTAL=0` every tile state
/// recomputed every turn.
pub const SIMD_ENV: &str = "AURELIA_NET_SIMD";
pub const THREADS_ENV: &str = "AURELIA_NET_THREADS";
pub const INCREMENTAL_ENV: &str = "AURELIA_NET_INCREMENTAL";

/// `AURELIA_NET_DEFER`: `off` the defer choice off whatever the weights
/// file says (the A/B of a net with and without it); a number `B` (logits,
/// may be negative or 0) the choice on with the bias `round(B × 256)` in
/// place of the file's, also for a file without defer (whose tag row of the
/// defer bucket is what its training left there). Unset: the file's. Read
/// when the file is first loaded (`load_model`).
pub const DEFER_ENV: &str = "AURELIA_NET_DEFER";

/// The speed the environment asks for ([`SIMD_ENV`], [`THREADS_ENV`],
/// [`INCREMENTAL_ENV`]; unset: [`Speed::default`]).
pub fn speed_from_env() -> Result<Speed, String> {
    let mut speed = Speed::default();
    let flag = |name: &str, default: bool| match std::env::var(name).as_deref() {
        Err(_) => Ok(default),
        Ok("0") => Ok(false),
        Ok("1") => Ok(true),
        Ok(other) => Err(format!("{name}={other}: 0 or 1")),
    };
    speed.simd = flag(SIMD_ENV, speed.simd)?;
    speed.incremental = flag(INCREMENTAL_ENV, speed.incremental)?;
    if let Ok(n) = std::env::var(THREADS_ENV) {
        speed.threads = n.parse().ok().filter(|&n| n >= 1).ok_or_else(|| format!("{THREADS_ENV}={n}: a number of threads ≥ 1"))?;
    }
    Ok(speed)
}

/// A weights file read and checked once per process (the bench seats one
/// mind per game; the games share the model), at the environment's speed
/// ([`speed_from_env`]).
pub fn load_model(path: &Path) -> Result<Arc<Model>, String> {
    static MODELS: OnceLock<Mutex<BTreeMap<PathBuf, Arc<Model>>>> = OnceLock::new();
    let models = MODELS.get_or_init(Default::default);
    let mut models = models.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(model) = models.get(path) {
        return Ok(model.clone());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("the net's weights file {}: {e}", path.display()))?;
    let model = Model::from_bytes(&bytes).map_err(|e| format!("the net's weights file {}: {e}", path.display()))?;
    let model = model.with_speed(speed_from_env()?);
    let model = Arc::new(match std::env::var(DEFER_ENV) {
        Err(_) => model,
        Ok(v) if v == "off" => model.with_defer(None),
        Ok(v) => {
            let logits: f64 = v.parse().map_err(|_| format!("{DEFER_ENV}={v}: `off` or the bias in logits"))?;
            model.with_defer(Some((logits * 256.0).round() as i32))
        }
    });
    models.insert(path.to_owned(), model.clone());
    Ok(model)
}

/// The layout's canonical text (classic/docs/ai-net-model.md, the weights
/// file): per table in the order tile, global, entity, candidate, event
/// (format 7), `table:`
/// then `name/scale;` per feature and a newline; a scale is an integer when
/// it is one, else the shortest decimal that reads back (Python's repr).
pub fn layout_text(rules: &CompiledRuleset) -> String {
    layout_text_of(rules, usize::MAX)
}

/// [`layout_text`] with the candidate table cut to its first `candidate`
/// columns: the layout of an older format whose candidate columns are a
/// prefix of this build's (format 9: `encode::FORMAT_9_CANDIDATE_COLUMNS`).
pub fn layout_text_of(rules: &CompiledRuleset, candidate: usize) -> String {
    let layout = encode::layout(rules);
    let cut = &layout.candidate[..candidate.min(layout.candidate.len())];
    let mut out = String::new();
    for (table, fields) in [("tile", &layout.tile[..]), ("global", &layout.global[..]), ("entity", &layout.entity[..]), ("candidate", cut), ("event", &layout.event[..])] {
        out.push_str(table);
        out.push(':');
        for field in fields {
            let s = field.scale;
            let scale = if s.num % s.den == 0 { (s.num / s.den).to_string() } else { format!("{:?}", s.value()) };
            out.push_str(&format!("{}/{scale};", field.name));
        }
        out.push('\n');
    }
    out
}

/// The hash a weights file of these rules' layout carries.
pub fn layout_hash(rules: &CompiledRuleset) -> u64 {
    fnv1a64(layout_text(rules).as_bytes())
}

/// The layouts a weights file may carry to play in this build, by hash:
/// this build's (`encode::FORMAT`), and format 9's — the same tables with the
/// candidate rows cut before the format-10 columns
/// (`encode::FORMAT_9_CANDIDATE_COLUMNS`), which such a net reads as its
/// own: a format-9 net plays here as it played in a format-9 build. Returns
/// the format of `hash`, None for another layout.
pub fn layout_format(rules: &CompiledRuleset, hash: u64) -> Option<u32> {
    if hash == layout_hash(rules) {
        Some(encode::FORMAT)
    } else if hash == fnv1a64(layout_text_of(rules, encode::FORMAT_9_CANDIDATE_COLUMNS).as_bytes()) {
        Some(9)
    } else {
        None
    }
}

/// What the net did in a game (all players of the mind).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NetStats {
    /// Player turns begun.
    pub turns: u64,
    /// Decisions per head (`encode::head`): unit, research, production,
    /// policy.
    pub decisions: [u64; 4],
    /// Unit decisions that chose to wait.
    pub waits: u64,
    /// Unit decisions that chose a long march.
    pub marches: u64,
    /// Best candidates the rules refused now, passed over for the next.
    pub masked: u64,
    /// Unit decisions left to the scripted mind (religious units, scripted
    /// roles).
    pub scripted_units: u64,
    /// Scripted decisions of the covered kinds dropped.
    pub dropped: u64,
    /// Saved memories that did not fit the scorer (another weights file, a
    /// damaged blob) and were started afresh.
    pub memory_resets: u64,
    /// Event rows of the turns begun (format 7, `events.rs`).
    pub events: u64,
    /// Unit decisions that offered the defer candidate, and that took it
    /// (format 9).
    pub defer_offered: u64,
    pub defers: u64,
}

/// The neighbour, region and coarse tables of one map.
#[derive(Debug, Clone)]
struct MapTables {
    dims: MapDimensions,
    neighbors: Vec<i32>,
    region: Vec<i32>,
    coarse: Vec<i32>,
}

impl MapTables {
    fn of(dims: MapDimensions) -> Self {
        let neighbors = encode::neighbor_table(dims).into_iter().flatten().collect();
        let (region, _) = encode::region_table(dims);
        let across = dims.width.div_ceil(COARSE_BLOCK);
        let coarse = (0..dims.tile_count() as u32).map(|n| ((n / dims.width / COARSE_BLOCK) * across + (n % dims.width) / COARSE_BLOCK) as i32).collect();
        Self { dims, neighbors, region, coarse }
    }
}

/// One player's turn in progress.
#[derive(Debug)]
struct OpenTurn {
    player: PlayerId,
    turn: Turn,
    cache: TurnCache,
    /// The units the scripted mind commands: the religious units (religion
    /// is not a head yet) and those of the scripted roles, by id and by
    /// handle.
    scripted: BTreeSet<EntityId>,
    scripted_handles: BTreeSet<(u32, u32)>,
    /// The research the net chose at the start of the turn (None: the queue
    /// was not empty or nothing is researchable).
    research: Option<Command>,
    round: u32,
    /// DAgger: the research candidates of the turn's start and the labeled
    /// decisions.
    research_candidates: Vec<Candidate>,
    labeled: Vec<Labeled>,
    /// Deferrals the net took this turn, per unit (each granted: the driver
    /// grants `DEFERS_PER_UNIT` a unit, and the candidate is offered only
    /// while one is left).
    deferred: BTreeMap<EntityId, u32>,
    /// The cities whose first production order of the turn was given: by
    /// the scripted mind (None) or by the net (its item, if it ordered one).
    /// The city's later orders of the turn (the scripted put-back of the
    /// old queue) stand, but an order of the net's item again.
    produced: BTreeMap<(u32, u32), Option<Option<ProductionItem>>>,
}

#[derive(Debug, Default)]
struct NetState {
    open: Option<OpenTurn>,
    tables: Option<MapTables>,
    /// Each player's cache, kept between its turns: its buffers, and its
    /// tile states for the incremental convolutions (`Speed::incremental`).
    caches: BTreeMap<PlayerId, TurnCache>,
    /// DAgger: the turns recorded (None: not recording).
    recording: Option<Recording>,
    /// E2: the sampled choices recorded (None: argmax, nothing recorded;
    /// `sample.rs`).
    e2: Option<Trace>,
    layout_checked: bool,
    stats: NetStats,
    scores: Vec<i32>,
}

/// The kind of decision a scripted command is, among those the net covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Covered {
    Unit,
    Research,
    Production(crate::ids::CityHandle),
    Policy,
}

impl Covered {
    fn head(self) -> i32 {
        match self {
            Covered::Unit => head::UNIT,
            Covered::Research => head::RESEARCH,
            Covered::Production(_) => head::PRODUCTION,
            Covered::Policy => head::POLICY,
        }
    }
}

fn covered(decision: &Decision) -> Option<Covered> {
    let Some(command) = decision.command.as_ref() else {
        return matches!(decision.actor, Actor::Unit(_)).then_some(Covered::Unit);
    };
    if acting_unit(command).is_some() {
        return Some(Covered::Unit);
    }
    match *command {
        Command::ChooseTechnology { .. } | Command::PlanResearch { .. } => Some(Covered::Research),
        Command::EnqueueProduction { city, .. } | Command::ClearProduction { city, .. } => Some(Covered::Production(city)),
        Command::AdoptPolicy { .. } | Command::UnlockPolicyBranch { .. } | Command::ChooseIdeology { .. } => Some(Covered::Policy),
        _ => None,
    }
}

/// The reason a net decision is traced under.
fn reason_of(command: &Command) -> Reason {
    match command {
        Command::FoundCity { .. } => Reason::FoundCity,
        Command::MoveUnit { .. } | Command::MarchTo { .. } => Reason::Explore,
        Command::AttackUnit { .. } | Command::RangedAttack { .. } => Reason::AttackUnit,
        Command::AttackCity { .. } => Reason::AttackCity,
        Command::Fortify { .. } => Reason::Fortify,
        Command::BuildImprovement { .. } | Command::ChopForest { .. } => Reason::BuildImprovement,
        Command::BuildRoad { .. } => Reason::BuildRoad,
        Command::Pillage { .. } => Reason::Pillage,
        Command::Rebase { .. } => Reason::Rebase,
        Command::AirSweep { .. } => Reason::AirSweep,
        Command::Intercept { .. } => Reason::Intercept,
        Command::Nuke { .. } => Reason::LaunchNuke,
        Command::Paradrop { .. } => Reason::Paradrop,
        Command::RebaseToCarrier { .. } => Reason::BaseOnCarrier,
        Command::ChooseTechnology { .. } => Reason::Research,
        Command::EnqueueProduction { item: ProductionItem::Building(_), .. } => Reason::ProduceBuilding,
        Command::EnqueueProduction { item: ProductionItem::Project(_), .. } => Reason::ProduceProject,
        Command::EnqueueProduction { item: ProductionItem::Unit(_), .. } | Command::ClearProduction { .. } => Reason::ProduceArmy,
        _ => Reason::SystemAdvisor,
    }
}

/// The environment variable that records the net's games for DAgger: a
/// directory; the bench writes a shard per game there (`aurelia-qa`), its
/// labels the scripted mind's choices at the states the net visited.
pub const DAGGER_ENV: &str = "AURELIA_NET_DAGGER";

/// A decision as a shard keeps it: head, actor row of the turn (`-1`), the
/// candidates, the expert's index among them.
pub type Labeled = (i32, i32, Vec<Candidate>, usize);

/// One player-turn the net played, with the scripted mind's choices as
/// labels (DAgger): the turn's rows (with the rows its decisions appended)
/// and its labeled decisions.
#[derive(Debug, Clone)]
pub struct RecordedTurn {
    pub round: u32,
    pub player: PlayerId,
    pub turn: Turn,
    pub decisions: Vec<Labeled>,
}

/// What a recording net kept: its turns, and the scripted choices that did
/// not map to a candidate, by reason (as the exporter counts them).
#[derive(Debug, Clone, Default)]
pub struct Recording {
    pub turns: Vec<RecordedTurn>,
    pub dropped: BTreeMap<String, u64>,
}

/// The environment variable naming the decision heads the net plays
/// (`AURELIA_NET_HEADS=unit,research`; the names of `encode::head::NAMES`;
/// unset: every head the weights file has). A head not listed stays with
/// the scripted mind.
pub const HEADS_ENV: &str = "AURELIA_NET_HEADS";

/// The heads of a comma list of names (`encode::head::NAMES`) as a set of
/// head codes; an unknown name is an error.
pub fn parse_heads(list: &str) -> Result<BTreeSet<i32>, String> {
    list.split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(|name| head::NAMES.iter().position(|h| *h == name).map(|i| i as i32).ok_or_else(|| format!("{HEADS_ENV}: no head `{name}` ({})", head::NAMES.join(", "))))
        .collect()
}

/// The environment variable naming the unit roles the scripted mind keeps
/// (`AURELIA_NET_SCRIPTED=founder,builder`; [`ScriptedRoles::parse`]).
pub const SCRIPTED_ENV: &str = "AURELIA_NET_SCRIPTED";

/// The unit roles (`kernel::ai::plan::AiRole`) whose decisions stay with the
/// scripted mind while the net plays the rest: `founder` (settlers),
/// `builder` (workers), `civilian` (great people and other civilians),
/// `naval` (sea and air units), `military` (land combat units).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScriptedRoles {
    pub founder: bool,
    pub builder: bool,
    pub civilian: bool,
    pub naval: bool,
    pub military: bool,
}

impl ScriptedRoles {
    pub const NAMES: [&'static str; 5] = ["founder", "builder", "civilian", "naval", "military"];

    /// A comma list of role names (empty: none); an unknown name is an error.
    pub fn parse(list: &str) -> Result<Self, String> {
        let mut out = Self::default();
        for name in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            let flag = match name {
                "founder" => &mut out.founder,
                "builder" => &mut out.builder,
                "civilian" => &mut out.civilian,
                "naval" => &mut out.naval,
                "military" => &mut out.military,
                other => return Err(format!("{SCRIPTED_ENV}: no unit role `{other}` ({})", Self::NAMES.join(", "))),
            };
            *flag = true;
        }
        Ok(out)
    }

    /// The roles in the order of `NAMES`, comma-separated.
    pub fn names(self) -> String {
        let on = [self.founder, self.builder, self.civilian, self.naval, self.military];
        Self::NAMES.iter().zip(on).filter(|(_, on)| *on).map(|(n, _)| *n).collect::<Vec<_>>().join(",")
    }

    /// Whether the scripted mind keeps a unit of `kind`.
    pub fn keeps(self, rules: &CompiledRuleset, kind: crate::ids::UnitKindId) -> bool {
        use kernel::ai::plan::AiRole;
        match kernel::ai::plan::role_of(rules, kind) {
            AiRole::Settler => self.founder,
            AiRole::Worker => self.builder,
            AiRole::Civilian => self.civilian,
            AiRole::Naval => self.naval,
            AiRole::Military => self.military,
        }
    }
}

/// The net's part of a `ClassicMind` (`Variant::Net`).
pub struct NetPlayer {
    scorer: Arc<dyn Scorer>,
    state: Mutex<NetState>,
    scripted: ScriptedRoles,
    /// The heads the net plays (None: every head the scorer has).
    heads: Option<BTreeSet<i32>>,
}

impl std::fmt::Debug for NetPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetPlayer").field("scorer", &self.scorer.label()).field("scripted", &self.scripted.names()).field("stats", &self.stats()).finish()
    }
}

impl NetPlayer {
    pub fn new(scorer: Arc<dyn Scorer>) -> Self {
        Self { scorer, state: Mutex::new(NetState::default()), scripted: ScriptedRoles::default(), heads: None }
    }

    /// The same player playing only `heads` (codes of `encode::head`); the
    /// others stay with the scripted mind.
    pub fn with_heads(self, heads: BTreeSet<i32>) -> Self {
        Self { heads: Some(heads), ..self }
    }

    /// Whether the net makes the decisions of `head`: the scorer has it and
    /// it is among the heads chosen.
    pub fn uses_head(&self, head: i32) -> bool {
        self.scorer.has_head(head) && self.heads.as_ref().is_none_or(|h| h.contains(&head))
    }

    /// The same player with the units of `roles` left to the scripted mind.
    pub fn with_scripted(self, roles: ScriptedRoles) -> Self {
        Self { scripted: roles, ..self }
    }

    /// The roles the scripted mind keeps.
    pub fn scripted(&self) -> ScriptedRoles {
        self.scripted
    }

    /// The [`LinearScorer`] player (tests, timing).
    pub fn dummy() -> Self {
        Self::new(Arc::new(LinearScorer))
    }

    /// The player of the weights file at `path` (`dummy`: the
    /// [`LinearScorer`]; `remote:SOCKET[,…]`: the net a server runs,
    /// `remote.rs`; `v2:W.v2[,…]`: the v2 net in process, `v2/mod.rs`).
    /// It records its turns for DAgger when [`DAGGER_ENV`] is set.
    pub fn from_path(path: &str) -> Result<Self, String> {
        #[cfg(unix)]
        if let Some(spec) = path.strip_prefix(crate::ai::net_e2e::remote::PREFIX) {
            let net = Self::new(crate::ai::net_e2e::remote::scorer(spec)?);
            return Ok(if std::env::var_os(DAGGER_ENV).is_some() { net.recording() } else { net });
        }
        if let Some(spec) = path.strip_prefix(crate::ai::net_e2e::v2::PREFIX) {
            let net = Self::new(crate::ai::net_e2e::v2::scorer(spec)?);
            return Ok(if std::env::var_os(DAGGER_ENV).is_some() { net.recording() } else { net });
        }
        let net = if path == DUMMY { Self::dummy() } else { Self::new(Arc::new(ModelScorer::load(Path::new(path))?)) };
        Ok(if std::env::var_os(DAGGER_ENV).is_some() { net.recording() } else { net })
    }

    /// The player of the weights file [`WEIGHTS_ENV`] names.
    pub fn from_env() -> Result<Self, String> {
        let path = std::env::var(WEIGHTS_ENV).map_err(|_| format!("the AI variant `net` needs {WEIGHTS_ENV}=<weights.bin> (or `{DUMMY}`)"))?;
        let roles = ScriptedRoles::parse(&std::env::var(SCRIPTED_ENV).unwrap_or_default())?;
        let net = Self::from_path(&path)?.with_scripted(roles);
        match std::env::var(HEADS_ENV) {
            Ok(list) => Ok(net.with_heads(parse_heads(&list)?)),
            Err(_) => Ok(net),
        }
    }

    pub fn label(&self) -> String {
        let mut label = self.scorer.label();
        if self.scripted != ScriptedRoles::default() {
            label = format!("{label}+{}", self.scripted.names());
        }
        if let Some(heads) = &self.heads {
            let names: Vec<&str> = heads.iter().filter_map(|h| head::NAMES.get(*h as usize).copied()).collect();
            label = format!("{label} heads={}", names.join(","));
        }
        label
    }

    pub fn stats(&self) -> NetStats {
        self.lock().stats
    }

    fn lock(&self) -> MutexGuard<'_, NetState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The start of a player's turn: a nation's turn is encoded and begun
    /// (city-states and barbarians stay scripted). Returns whether the net
    /// plays this turn.
    pub fn begin(&self, sight: &AIObservation<'_>, rules: &CompiledRuleset, state: &mut AiState) -> bool {
        let mut st = self.lock();
        let st = &mut *st;
        if st.open.is_some() {
            self.finish(st);
        }
        if sight.view.own_kind() != PlayerKind::Major {
            return false;
        }
        if !st.layout_checked {
            if let Some(theirs) = self.scorer.layout_hash() {
                let ours = layout_hash(rules);
                assert!(
                    layout_format(rules, theirs).is_some(),
                    "the net's weights file {} was trained on another feature layout: its layout hash {theirs:#018x}, this build's {ours:#018x} (encoder format {}); the net does not play with it — export it again from shards of this build",
                    self.scorer.label(),
                    encode::FORMAT
                );
            }
            st.layout_checked = true;
        }
        // The memory: the net's slots (when they fit the scorer) and the
        // player's event tracker (format 7), which the turn's rows read.
        let (saved, mut tracker) = decode_memory(&state.mind_memory, self.scorer.as_ref());
        let turn = {
            let _span = prof::span("net/encode_turn");
            Turn::encode_tracked(sight, rules, &mut tracker)
        };
        st.stats.events += turn.event_count() as u64;
        let dims = turn.dims;
        if st.tables.as_ref().is_none_or(|t| (t.dims.width, t.dims.height) != (dims.width, dims.height)) {
            st.tables = Some(MapTables::of(dims));
        }
        let tables = st.tables.as_ref().expect("the map's tables");
        let meta: Vec<i32> = turn.entity_meta.iter().flat_map(|m| [m[0], m[1], m[2]]).collect();
        let ids: Vec<i32> = turn.entity_meta.iter().map(|m| m[3]).collect();
        let coarse: &[i32] = if self.scorer.coarse() { &tables.coarse } else { &[] };
        let event_meta: Vec<i32> = turn.event_meta.iter().flatten().copied().collect();
        let input = TurnInput {
            tiles: &turn.tiles,
            neighbors: &tables.neighbors,
            region: &tables.region,
            global: &turn.global,
            entities: &turn.entities,
            entity_meta: &meta,
            entity_ids: &ids,
            coarse_region: coarse,
            aim: &[],
            events: &turn.events,
            event_meta: &event_meta,
        };
        let memory = match saved {
            Some(memory) => memory,
            None => {
                if !state.mind_memory.is_empty() {
                    st.stats.memory_resets += 1;
                }
                self.scorer.memory()
            }
        };
        let mut cache = st.caches.remove(&sight.player).unwrap_or_default();
        {
            let _span = prof::span("net/begin_turn");
            self.scorer.begin_turn(&input, &memory, &mut cache);
        }
        if let Some(e2) = st.e2.as_mut() {
            e2.begin_turn(sight.player, sight.round, self.scorer.layout_hash().map(|_| cache.value()));
        }
        let next = {
            let _span = prof::span("net/end_turn");
            self.scorer.end_turn(&cache, &memory)
        };
        state.mind_memory = encode_memory(&next, &tracker, self.scorer.as_ref());
        let religious_handles = crate::ai::religion::religious_units(sight, rules);
        let kept: Vec<_> = sight.own_units.iter().filter(|u| religious_handles.contains(&u.handle) || self.scripted.keeps(rules, u.kind)).collect();
        let scripted = kept.iter().map(|u| u.id).collect();
        let scripted_handles = kept.iter().map(|u| (u.handle.index, u.handle.generation)).collect();
        st.stats.turns += 1;
        let mut open = OpenTurn { player: sight.player, turn, cache, scripted, scripted_handles, research: None, round: sight.round, research_candidates: Vec::new(), labeled: Vec::new(), deferred: BTreeMap::new(), produced: BTreeMap::new() };
        if sight.economy.research_queue.is_empty() && self.uses_head(head::RESEARCH) {
            let candidates = encode::research_candidates(sight, rules);
            if let Some(at) = self.choose(&mut st.scores, &mut st.stats, &mut st.e2, &mut open.cache, head::RESEARCH, (-1, sample::Actor::Player), &candidates, |_| true) {
                open.research = candidates[at].command.clone();
            }
            if st.recording.is_some() {
                open.research_candidates = candidates;
            }
        }
        st.open = Some(open);
        true
    }

    /// The best candidate the rules accept (`accepts`): the largest logit,
    /// ties to the lowest index; a refused best one is passed over. While
    /// sampling (`e2`), drawn from the accepted ones and recorded with the
    /// `actor`'s key (`sample.rs`). The candidate taken joins the turn trace
    /// (`Scorer::note_choice`): every decision the net makes passes here,
    /// in the order made.
    #[allow(clippy::too_many_arguments)]
    fn choose(&self, scores: &mut Vec<i32>, stats: &mut NetStats, e2: &mut Option<Trace>, cache: &mut TurnCache, head: i32, (actor, key): (i32, sample::Actor), candidates: &[Candidate], accepts: impl Fn(&Candidate) -> bool) -> Option<usize> {
        let at = self.pick(scores, stats, e2, cache, head, (actor, key), candidates, accepts);
        if let Some(at) = at {
            self.scorer.note_choice(cache, head as usize, actor, &candidates[at]);
        }
        at
    }

    #[allow(clippy::too_many_arguments)]
    fn pick(&self, scores: &mut Vec<i32>, stats: &mut NetStats, e2: &mut Option<Trace>, cache: &TurnCache, head: i32, (actor, key): (i32, sample::Actor), candidates: &[Candidate], accepts: impl Fn(&Candidate) -> bool) -> Option<usize> {
        if candidates.is_empty() {
            // Recorded all the same: the exporter rebuilds the turn's rows the same way.
            return e2.as_mut().and_then(|e2| e2.choose(head, key, &[], candidates, accepts));
        }
        stats.decisions[head as usize] += 1;
        {
            let _span = prof::span("net/score");
            self.scorer.score(cache, head as usize, actor, candidates, scores);
        }
        if let Some(e2) = e2.as_mut() {
            return e2.choose(head, key, scores, candidates, accepts);
        }
        let _span = prof::span("net/choose_accepted");
        let mut masked = vec![false; candidates.len()];
        loop {
            let best = (0..candidates.len()).filter(|&i| !masked[i]).fold(None, |best: Option<usize>, i| match best {
                Some(b) if scores[b] >= scores[i] => Some(b),
                _ => Some(i),
            })?;
            if accepts(&candidates[best]) {
                return Some(best);
            }
            masked[best] = true;
            stats.masked += 1;
        }
    }

    /// Ends the open turn (its cache is kept for the next; a recording
    /// keeps its rows and labels).
    fn finish(&self, st: &mut NetState) {
        if let Some(open) = st.open.take() {
            st.caches.insert(open.player, open.cache);
            if let Some(recording) = st.recording.as_mut() {
                recording.turns.push(RecordedTurn { round: open.round, player: open.player, turn: open.turn, decisions: open.labeled });
            }
        }
    }

    /// The same player recording its turns for DAgger.
    pub fn recording(self) -> Self {
        self.lock().recording = Some(Recording::default());
        self
    }

    /// Whether it records its turns.
    pub fn records(&self) -> bool {
        self.lock().recording.is_some()
    }

    /// The turns recorded so far (taken; the recording goes on).
    pub fn take_recording(&self) -> Recording {
        let mut st = self.lock();
        let st = &mut *st;
        if let Some(open) = st.open.take() {
            st.open = Some(open);
        }
        st.recording.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// E2 self-play: from now on the player samples its choices and records
    /// them (`sample.rs`).
    pub fn set_sampling(&self, sampling: Sampling) {
        self.lock().e2 = Some(Trace::new(sampling).with_defer(self.scorer.defers()));
    }

    /// Whether the net offers the defer candidate (its scorer scores it).
    pub fn defers(&self) -> bool {
        self.scorer.defers()
    }

    /// While sampling: the records made since the last stamp get
    /// `position`, the commands the game applied before them.
    pub fn stamp_trace(&self, position: u32) {
        if let Some(e2) = self.lock().e2.as_mut() {
            e2.stamp(position);
        }
    }

    /// While sampling: `look` at the trace so far (read only; None when not
    /// sampling). The self-play's per-decision values read each decision
    /// as it is made (`aurelia_qa::search::Values`).
    pub fn peek_trace<R>(&self, look: impl FnOnce(&Trace) -> R) -> Option<R> {
        self.lock().e2.as_ref().map(look)
    }

    /// While sampling: the trace so far (taken; the sampling goes on).
    pub fn take_trace(&self) -> Option<Trace> {
        self.lock().e2.as_mut().map(|e2| std::mem::replace(e2, Trace::new(e2.sampling).with_defer(e2.defer)))
    }

    /// Whether the net plays `player`'s turn now.
    pub fn plays(&self, player: PlayerId) -> bool {
        self.lock().open.as_ref().is_some_and(|o| o.player == player)
    }

    /// The scripted decisions of a phase without those the net makes (unit
    /// commands but the religious units', research, production, policies);
    /// with `research` the net's research of the turn is added.
    /// Production orders are the net's where the scripted mind gives a
    /// city its first order of the turn and `sight` is at hand
    /// ([`NetPlayer::production_orders`]); without it (the strategic phase)
    /// the scripted mind's stand.
    pub fn replace_scripted(&self, player: PlayerId, decisions: Vec<Decision>, research: bool, sight: Option<&AIObservation<'_>>, rules: &CompiledRuleset) -> Vec<Decision> {
        let mut st = self.lock();
        let st = &mut *st;
        if st.open.as_ref().is_none_or(|o| o.player != player) {
            return decisions;
        }
        let decisions = self.production_orders(st, sight, rules, decisions);
        let open = st.open.as_mut().expect("the open turn");
        let mut out = Vec::with_capacity(decisions.len() + 1);
        for decision in decisions {
            match covered(&decision) {
                None | Some(Covered::Production(_)) => out.push(decision),
                Some(Covered::Unit) if scripted_unit(open, &decision) => out.push(decision),
                Some(kind) if !self.uses_head(kind.head()) => out.push(decision),
                Some(kind) => {
                    st.stats.dropped += 1;
                    if let (Some(recording), Covered::Research, Some(command)) = (st.recording.as_mut(), kind, decision.command.as_ref()) {
                        let tech = match *command {
                            Command::ChooseTechnology { technology, .. } | Command::PlanResearch { target: technology, .. } => i32::from(technology.0),
                            _ => -1,
                        };
                        match open.research_candidates.iter().position(|c| c.meta[3] == tech) {
                            Some(at) => open.labeled.push((head::RESEARCH, -1, open.research_candidates.clone(), at)),
                            None => bump(&mut recording.dropped, "research_not_listed"),
                        }
                    }
                }
            }
        }
        let open = st.open.as_ref().expect("the open turn");
        if research {
            if let Some(command) = open.research.clone() {
                out.push(Decision::act(Actor::Player, Reason::Research, command));
            }
        }
        out
    }

    /// One unit's decision (None: the scripted mind decides it — a
    /// religious unit, or a turn the net does not play).
    /// While recording, `expert` gives the scripted mind's decision of the
    /// unit, the label.
    pub fn decide_unit(&self, near: &LocalObservation<'_>, rules: &CompiledRuleset, expert: &mut dyn FnMut() -> Decision) -> Option<Decision> {
        let mut st = self.lock();
        let st = &mut *st;
        let open = st.open.as_mut().filter(|o| o.player == near.player && self.uses_head(head::UNIT))?;
        let unit = &near.unit;
        if open.scripted.contains(&unit.id) || self.scripted.keeps(rules, unit.kind) {
            st.stats.scripted_units += 1;
            return None;
        }
        let actor = open.turn.own_unit_row(unit, rules);
        let mut candidates = {
            let _span = prof::span("net/unit_candidates");
            encode::unit_candidates(&mut open.turn, near, rules)
        };
        // Format 9: the defer candidate last, while the unit has a deferral left.
        let listed = candidates.len();
        if self.scorer.defers() && open.deferred.get(&unit.id).copied().unwrap_or(0) < kernel::ai::driver::DEFERS_PER_UNIT {
            candidates.push(encode::defer_candidate());
            st.stats.defer_offered += 1;
        }
        let accepts = |c: &Candidate| c.command.as_ref().is_none_or(|k| c.meta[0] != tag::MARCH || near.view.accepts(k));
        let key = sample::Actor::Unit { index: unit.handle.index, generation: unit.handle.generation };
        let at = self.choose(&mut st.scores, &mut st.stats, &mut st.e2, &mut open.cache, head::UNIT, (actor, key), &candidates, accepts);
        if let Some(recording) = st.recording.as_mut() {
            // The expert never defers: its label is among the listed candidates, and
            // the defer candidate stays out of the imitation data (E2 learns it).
            match label_unit(near, &candidates[..listed], expert().command.as_ref()) {
                Ok(label) => open.labeled.push((head::UNIT, actor, candidates[..listed].to_vec(), label)),
                Err(reason) => bump(&mut recording.dropped, &reason),
            }
        }
        let actor = Actor::Unit(unit.id);
        if at.is_some_and(|at| candidates[at].meta[0] == tag::DEFER) {
            st.stats.defers += 1;
            *open.deferred.entry(unit.id).or_default() += 1;
            return Some(Decision::refuse(actor, Reason::Deferred));
        }
        let Some(command) = at.and_then(|at| candidates[at].command.clone()) else {
            st.stats.waits += 1;
            return Some(Decision::refuse(actor, Reason::Waiting));
        };
        if matches!(command, Command::MarchTo { .. }) {
            st.stats.marches += 1;
        }
        Some(Decision::act(actor, reason_of(&command), command))
    }

    /// The end of the player's turn: the scripted end-of-turn decisions with
    /// the net's production and policy in place of the scripted ones.
    pub fn end_of_turn(&self, sight: &AIObservation<'_>, rules: &CompiledRuleset, scripted: Vec<Decision>) -> Vec<Decision> {
        let mut st = self.lock();
        let st = &mut *st;
        if st.open.as_ref().is_none_or(|o| o.player != sight.player) {
            return scripted;
        }
        let scripted = self.production_orders(st, Some(sight), rules, scripted);
        let net_policy = self.uses_head(head::POLICY);
        let wants_policy = net_policy && scripted.iter().any(|d| covered(d) == Some(Covered::Policy));
        let policy = if wants_policy {
            let candidates = encode::policy_candidates(sight, rules);
            let open = st.open.as_mut().expect("the open turn");
            if let Some(recording) = st.recording.as_mut() {
                let expert = scripted.iter().find_map(|d| match d.command {
                    Some(Command::AdoptPolicy { policy, .. }) => Some((tag::ADOPT_POLICY, i32::from(policy.0))),
                    Some(Command::UnlockPolicyBranch { branch, .. }) => Some((tag::UNLOCK_BRANCH, i32::from(branch.0))),
                    Some(Command::ChooseIdeology { ideology, .. }) => Some((tag::CHOOSE_IDEOLOGY, i32::from(ideology.0))),
                    _ => None,
                });
                match expert.and_then(|(code, id)| candidates.iter().position(|c| c.meta[0] == code && c.meta[3] == id)) {
                    Some(at) => open.labeled.push((head::POLICY, -1, candidates.clone(), at)),
                    None => bump(&mut recording.dropped, "policy_not_listed"),
                }
            }
            let accepts = |c: &Candidate| c.command.as_ref().is_some_and(|k| sight.view.accepts(k));
            self.choose(&mut st.scores, &mut st.stats, &mut st.e2, &mut open.cache, head::POLICY, (-1, sample::Actor::Player), &candidates, accepts)
                .and_then(|at| candidates[at].command.clone())
                .map(|command| Decision::act(Actor::Player, Reason::SystemAdvisor, command))
        } else {
            None
        };
        let open = st.open.as_ref().expect("the open turn");
        let mut policy = policy;
        let mut out = Vec::with_capacity(scripted.len() + 4);
        for decision in scripted {
            match covered(&decision) {
                None | Some(Covered::Production(_)) => out.push(decision),
                Some(Covered::Unit) if scripted_unit(open, &decision) => out.push(decision),
                Some(kind) if !self.uses_head(kind.head()) => out.push(decision),
                Some(Covered::Policy) => {
                    out.extend(policy.take());
                    st.stats.dropped += 1;
                }
                Some(_) => st.stats.dropped += 1,
            }
        }
        self.finish(st);
        out
    }

    /// The scripted decisions with the net's production orders in place of
    /// the scripted mind's, decided where the exporter records the expert's
    /// production decision: a city's first `EnqueueProduction` of the turn
    /// (after the `ClearProduction` before it, if any). The net sees the
    /// candidates the exporter's expert saw there: after a clear, nothing is
    /// queued. Its order keeps the scripted mind's form: after a clear the
    /// clear and its choice (the head); without one its choice joins the
    /// queue (nothing when queued already). The city's later orders of the
    /// turn (the scripted put-back of the old queue) stand, so the old queue
    /// goes on behind the net's choice as behind the expert's; only an order
    /// of the net's item again is dropped. Without `sight`, or for a city not
    /// seen, the scripted orders stand.
    fn production_orders(&self, st: &mut NetState, sight: Option<&AIObservation<'_>>, rules: &CompiledRuleset, decisions: Vec<Decision>) -> Vec<Decision> {
        if !self.uses_head(head::PRODUCTION) {
            return decisions;
        }
        let mut out = Vec::with_capacity(decisions.len());
        let mut held: BTreeMap<(u32, u32), Decision> = BTreeMap::new();
        for decision in decisions {
            let Some(Covered::Production(city)) = covered(&decision) else {
                out.push(decision);
                continue;
            };
            let key = (city.index, city.generation);
            let open = st.open.as_mut().expect("the open turn");
            match open.produced.get(&key) {
                Some(Some(chosen)) => {
                    let again = matches!(decision.command, Some(Command::EnqueueProduction { item, .. }) if Some(item) == *chosen);
                    if again {
                        st.stats.dropped += 1;
                    } else {
                        out.push(decision);
                    }
                    continue;
                }
                Some(None) => {
                    out.push(decision);
                    continue;
                }
                None => {}
            }
            match decision.command {
                Some(Command::EnqueueProduction { item, .. }) => {
                    let cleared = held.remove(&key);
                    let own = sight.and_then(|s| s.own_cities.iter().find(|c| c.handle == city).map(|c| (s, c)));
                    match own {
                        Some((sight, own)) => {
                            st.stats.dropped += 1 + u64::from(cleared.is_some());
                            let orders = self.produce(st, sight, rules, own, cleared.is_some(), item);
                            let chosen = orders.iter().find_map(|d| match d.command {
                                Some(Command::EnqueueProduction { item, .. }) => Some(item),
                                _ => None,
                            });
                            st.open.as_mut().expect("the open turn").produced.insert(key, Some(chosen));
                            out.extend(orders);
                        }
                        None => {
                            open.produced.insert(key, None);
                            out.extend(cleared);
                            out.push(decision);
                        }
                    }
                }
                Some(Command::ClearProduction { .. }) if !held.contains_key(&key) => {
                    held.insert(key, decision);
                }
                _ => out.push(decision),
            }
        }
        // A clear with no order after it: the scripted mind's.
        out.extend(held.into_values());
        out
    }

    /// The net's production decision of one city at the scripted mind's
    /// first order of the turn (`expert`, the label while recording);
    /// `cleared`: the scripted mind cleared the queue first.
    fn produce(&self, st: &mut NetState, sight: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, cleared: bool, expert: ProductionItem) -> Vec<Decision> {
        let open = st.open.as_mut().expect("the open turn");
        let actor = open.turn.own_city_row(city, sight, rules);
        let mut candidates = encode::production_candidates(&mut open.turn, sight, city, rules);
        if cleared {
            // The exporter's expert ordered after its clear ran: nothing queued.
            for c in &mut candidates {
                c.row[encode::C::ItemQueued as usize] = 0;
            }
        }
        if let Some(recording) = st.recording.as_mut() {
            let (code, id) = item_code(expert);
            match candidates.iter().position(|c| c.meta[0] == code && c.meta[3] == id) {
                Some(at) => open.labeled.push((head::PRODUCTION, actor, candidates.clone(), at)),
                None => bump(&mut recording.dropped, "production_not_listed"),
            }
        }
        let key = sample::Actor::City { index: city.handle.index, generation: city.handle.generation, cleared };
        let Some(at) = self.choose(&mut st.scores, &mut st.stats, &mut st.e2, &mut open.cache, head::PRODUCTION, (actor, key), &candidates, |_| true) else {
            return Vec::new();
        };
        let Some(command @ Command::EnqueueProduction { item, .. }) = candidates[at].command.clone() else {
            return Vec::new();
        };
        // The phase's later cities see this order (the exporter sees it applied).
        open.turn.note_order(city.handle, item);
        let actor = Actor::City(city.id);
        if cleared {
            return vec![Decision::act(actor, Reason::ProduceArmy, Command::ClearProduction { player: sight.player, city: city.handle }), Decision::act(actor, reason_of(&command), command)];
        }
        if city.queue.contains(&item) {
            return Vec::new();
        }
        vec![Decision::act(actor, reason_of(&command), command)]
    }

    /// The candidates and actor row of the net's production decision for a
    /// city, as [`NetPlayer::production_orders`] builds them (tests: the
    /// exporter's rows at the same state).
    pub fn production_candidates(&self, sight: &AIObservation<'_>, rules: &CompiledRuleset, city: &OwnCity, cleared: bool) -> Option<(i32, Vec<Candidate>)> {
        let mut st = self.lock();
        let open = st.open.as_mut().filter(|o| o.player == sight.player)?;
        let actor = open.turn.own_city_row(city, sight, rules);
        let mut candidates = encode::production_candidates(&mut open.turn, sight, city, rules);
        if cleared {
            for c in &mut candidates {
                c.row[encode::C::ItemQueued as usize] = 0;
            }
        }
        Some((actor, candidates))
    }
}

/// A scripted decision for a unit the scripted mind commands (a religious
/// unit, a unit of a scripted role): kept.
fn scripted_unit(open: &OpenTurn, decision: &Decision) -> bool {
    let by_id = matches!(decision.actor, Actor::Unit(id) if open.scripted.contains(&id));
    let by_handle = decision.command.as_ref().and_then(acting_unit).is_some_and(|h| open.scripted_handles.contains(&(h.index, h.generation)));
    by_id || by_handle
}


/// The magic and the formats of a saved net memory (`AiState::mind_memory`):
/// 1 the net's slots; 2 the player's event tracker before them.
const MEMORY_MAGIC: &[u8; 4] = b"AUNM";
const MEMORY_FORMAT: u32 = 2;

/// A memory as the mind's bytes of the AI state, little-endian: magic
/// `AUNM`, u32 format (2), u32 length and bytes of the player's event
/// tracker (`events::Tracker::to_bytes`), u64 layout hash of the scorer (0:
/// none), u32 widths of the player, nation and unit slots; the player slot
/// (i16 × width); u32 nation count, their ids (i32, ascending), their slots
/// (i16); the same for the units. Format 1 is the same without the tracker.
pub fn encode_memory(memory: &Memory, tracker: &Tracker, scorer: &dyn Scorer) -> Vec<u8> {
    let [wp, wn, wu] = scorer.memory_widths();
    let events = tracker.to_bytes();
    let mut out = Vec::with_capacity(36 + events.len() + 2 * (memory.player.len() + memory.nations.len() + memory.units.len()) + 4 * (memory.nation_ids.len() + memory.unit_ids.len()));
    out.extend_from_slice(MEMORY_MAGIC);
    out.extend_from_slice(&MEMORY_FORMAT.to_le_bytes());
    out.extend_from_slice(&(events.len() as u32).to_le_bytes());
    out.extend_from_slice(&events);
    out.extend_from_slice(&scorer.layout_hash().unwrap_or(0).to_le_bytes());
    for w in [wp, wn, wu] {
        out.extend_from_slice(&(w as u32).to_le_bytes());
    }
    let i16s = |out: &mut Vec<u8>, v: &[i16]| v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
    i16s(&mut out, &memory.player);
    for (ids, values) in [(&memory.nation_ids, &memory.nations), (&memory.unit_ids, &memory.units)] {
        out.extend_from_slice(&(ids.len() as u32).to_le_bytes());
        ids.iter().for_each(|id| out.extend_from_slice(&id.to_le_bytes()));
        i16s(&mut out, values);
    }
    out
}

/// The memory of `bytes` when it fits `scorer` (its format, layout and
/// widths; ids ascending), None otherwise (none saved, another weights
/// file, damage); and the player's event tracker, which does not depend on
/// the scorer (empty when none is saved: format 1, damage).
pub fn decode_memory(bytes: &[u8], scorer: &dyn Scorer) -> (Option<Memory>, Tracker) {
    struct R<'a>(&'a [u8]);
    impl<'a> R<'a> {
        fn take(&mut self, n: usize) -> Option<&'a [u8]> {
            let (head, rest) = (self.0.get(..n)?, self.0.get(n..)?);
            self.0 = rest;
            Some(head)
        }
        fn u32(&mut self) -> Option<u32> {
            Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
        }
        fn i16s(&mut self, n: usize) -> Option<Vec<i16>> {
            Some(self.take(n.checked_mul(2)?)?.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect())
        }
        fn slots(&mut self, width: usize) -> Option<(Vec<i32>, Vec<i16>)> {
            let n = self.u32()? as usize;
            let ids: Vec<i32> = self.take(n.checked_mul(4)?)?.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c)).collect();
            if ids.windows(2).any(|w| w[0] >= w[1]) {
                return None;
            }
            Some((ids, self.i16s(n.checked_mul(width)?)?))
        }
    }
    let mut r = R(bytes);
    let format = match (r.take(4), r.u32()) {
        (Some(magic), Some(f)) if magic == MEMORY_MAGIC && (1..=MEMORY_FORMAT).contains(&f) => f,
        _ => return (None, Tracker::new()),
    };
    let mut tracker = Tracker::new();
    if format >= 2 {
        let Some(n) = r.u32() else { return (None, Tracker::new()) };
        let Some(events) = r.take(n as usize) else { return (None, Tracker::new()) };
        tracker = Tracker::from_bytes(events).unwrap_or_default();
    }
    let slots = (|| {
        let hash = u64::from_le_bytes(r.take(8)?.try_into().ok()?);
        let widths = [r.u32()? as usize, r.u32()? as usize, r.u32()? as usize];
        if hash != scorer.layout_hash().unwrap_or(0) || widths != scorer.memory_widths() {
            return None;
        }
        let player = r.i16s(widths[0])?;
        let (nation_ids, nations) = r.slots(widths[1])?;
        let (unit_ids, units) = r.slots(widths[2])?;
        if !r.0.is_empty() {
            return None;
        }
        let mut memory = scorer.memory();
        if memory.player.len() != player.len() {
            return None;
        }
        memory.player = player;
        (memory.nation_ids, memory.nations, memory.unit_ids, memory.units) = (nation_ids, nations, unit_ids, units);
        Some(memory)
    })();
    (slots, tracker)
}

fn bump(map: &mut BTreeMap<String, u64>, key: &str) {
    *map.entry(key.to_owned()).or_default() += 1;
}

/// The candidate tag and id of a production item.
fn item_code(item: ProductionItem) -> (i32, i32) {
    match item {
        ProductionItem::Unit(k) => (tag::PRODUCE_UNIT, i32::from(k.0)),
        ProductionItem::Building(k) => (tag::PRODUCE_BUILDING, i32::from(k.0)),
        ProductionItem::Project(k) => (tag::PRODUCE_PROJECT, i32::from(k.0)),
    }
}

/// The index of the scripted mind's unit command among the unit's
/// candidates, as the exporter places it (`aurelia-qa` `export::place`): no
/// command is waiting; a long march is the march to its destination when
/// that is a point of interest, else the move there when the unit reaches
/// it this turn, else the nearest march within `encode::MARCH_NEAR` (the
/// exporter's first-step placement needs the march run, so such a march is
/// dropped); an attack is the candidate that strikes its blow
/// (`encode::place_blow`); another command is the candidate of the same tag
/// and value. Err: the reason it is dropped (the exporter's names).
pub fn label_unit(near: &LocalObservation<'_>, candidates: &[Candidate], command: Option<&Command>) -> Result<usize, String> {
    let Some(command) = command else {
        return candidates.iter().position(|c| c.meta[0] == tag::WAIT).ok_or_else(|| "no_wait".to_owned());
    };
    let name = || format!("{command:?}").split([' ', '{', '(']).next().unwrap_or("").to_owned();
    if let Command::MarchTo { destination, .. } = *command {
        let exact = candidates.iter().position(|c| c.meta[0] == tag::MARCH && c.meta[1] == destination.0 as i32);
        let reachable = || candidates.iter().position(|c| c.meta[0] == 0 && c.meta[1] == destination.0 as i32);
        let near_poi = || encode::place_march(candidates, destination, near.view.dimensions()).map(|(at, _)| at);
        return exact.or_else(reachable).or_else(near_poi).ok_or_else(|| "march_far_from_points_of_interest".to_owned());
    }
    if let Some(at) = encode::place_blow(near, candidates, command) {
        return at.ok_or_else(|| format!("unit_not_listed_{}", name()));
    }
    let Some(code) = encode::unit_tag(command) else {
        return Err(format!("unit_not_enumerated_{}", name()));
    };
    let value = near.view.command_value(command);
    candidates
        .iter()
        .position(|c| c.meta[0] == code && c.command.as_ref().is_some_and(|k| near.view.command_value(k) == value))
        .ok_or_else(|| format!("unit_not_listed_{}", name()))
}
