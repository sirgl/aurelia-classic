//! The training grounds of E2 (classic/docs/ai-neural-architecture.md,
//! «Teaching techniques»; the files: classic/docs/ai-net-data.md, «E2:
//! training grounds»): short scenarios with a dense result, played by the
//! learner net like any E2 game (`selfplay::play`: the same seats, sampling,
//! sidecar and `.e2.json`) and trained by the same loop as further suites.
//!
//! - **Battles** ([`BattleKind`]): a suite game played by the scripted AI
//!   (`standard` on every seat) to a round, then two neighbouring nations put
//!   at war and given armies ([`set_battle`], built on `battle.rs`); the
//!   learner takes one side, `standard` the other, for 20–40 rounds. The
//!   scene is set with the harness setters, which are no commands: each is
//!   recorded as an [`Edit`] (`.e2.json` `ground.edits`, at its position in
//!   the log), and `ai-features --e2` makes them again before the command at
//!   that position (`apply_at`), so the log replays to the game's hash.
//! - **Economy windows**: a mid-game start from an expert's log
//!   (`selfplay::FromLogs`) at a round R drawn per group from a list, played
//!   for W rounds (drawn per group) and stopped there (no time victory: the
//!   game's last round is unchanged, so every player plays as in a whole
//!   game); R = 0 is an opening window, a new game of the source log's spec
//!   played from its start. The science each nation made in the window is
//!   recorded ([`EconomyRecord`]), and at the window's end the learner's
//!   science a turn and its empire's potential ([`Potential`]: the yields a
//!   turn and the stocks at the scripted AI's shadow prices, `ai::value::Prices`)
//!   beside the expert's at the same round of the source log
//!   ([`ExpertAt`], `selfplay::expert_at`): the trainer's rewards
//!   `economy_potential` and `economy_rate` compare them.
//!
//! Every draw is a function of the group, so a group's members share the
//! scene and differ only in the sampling (GRPO). The rewards are made by the
//! trainer from the raw numbers (`nn/aurelia_nn/train_e2.py`); the reference
//! here ([`battle_reward`]) is the same formula, for the logs and the tests.

use std::collections::BTreeMap;

use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::classic::Classic;
use classic_rules::ids::{PlayerId, TileId, UnitHandle, UnitKindId};
use classic_rules::{ClassicGame, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::mind::AiMind;
use serde::{Deserialize, Serialize};

use crate::battle::{self, Arm};
use crate::bench::modules::{majors, BenchRules, ClassicBench};

/// The kinds of battle ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BattleKind {
    /// Equal armies (the same units on both sides) on the open and rough
    /// ground between the two nations' nearest cities, out of every city's
    /// strike.
    Field,
    /// The learner's army (ranged and siege units heavy) against the
    /// enemy's city nearest it, held by a garrison.
    Siege,
    /// The learner's city nearest the enemy, its garrison against an army
    /// the scripted AI sends on it.
    Defence,
    /// The learner's capital early (rounds 10–40), its few units against the
    /// `rush-siege` book's army (ranged, siege and the melee that takes the
    /// city; [`rush_army`]).
    RushDefence,
    /// A small fight of units in the open, a rung of the skirmish ladder
    /// ([`SKIRMISH`]): from one blow at a unit at 1 hit point to 6 against 8;
    /// won when every enemy unit of the scene is dead.
    Skirmish,
    /// A battle window: two large armies (`--battle-army`, 8–14 in the loop)
    /// in contact or `stage` rounds of approach apart, played for the
    /// approach and a short window after it (`--battle-rounds`, 4–5);
    /// rewarded by the value destroyed and the learner's force kept ready
    /// ([`clash_phi`]).
    Clash,
}

impl BattleKind {
    /// The kinds of the battle eval and the suite's tests (the skirmish is
    /// the ladder's ground of its own: [`BattleKind::KINDS`]).
    pub const ALL: [BattleKind; 4] = [BattleKind::Field, BattleKind::Siege, BattleKind::Defence, BattleKind::RushDefence];
    /// Every kind `--battle` takes.
    pub const KINDS: [BattleKind; 6] = [BattleKind::Field, BattleKind::Siege, BattleKind::Defence, BattleKind::RushDefence, BattleKind::Skirmish, BattleKind::Clash];

    pub fn name(self) -> &'static str {
        match self {
            BattleKind::Field => "field",
            BattleKind::Siege => "siege",
            BattleKind::Defence => "defence",
            BattleKind::RushDefence => "rush-defence",
            BattleKind::Skirmish => "skirmish",
            BattleKind::Clash => "clash",
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        Self::KINDS.into_iter().find(|k| k.name() == text).ok_or_else(|| format!("unknown battle kind {text} (field, siege, defence, rush-defence, skirmish, clash)"))
    }

    /// The rounds the war may begin at (drawn per group).
    pub fn start_rounds(self) -> (u32, u32) {
        match self {
            BattleKind::RushDefence => (10, 40),
            _ => (battle::FIRST_START_ROUND, battle::LAST_START_ROUND),
        }
    }

    /// The learner holds a city (the scripted side attacks it).
    pub fn learner_holds(self) -> bool {
        matches!(self, BattleKind::Defence | BattleKind::RushDefence)
    }

    /// The draw weights of the arms of the main army's slots: melee,
    /// mounted, ranged, siege (a rush-defence's attackers are the book's
    /// army, [`rush_army`]).
    fn arm_weights(self) -> [u32; 4] {
        match self {
            BattleKind::Field => [3, 2, 3, 1],
            // No siege engines: they set up before they shoot and are a city's ground.
            BattleKind::Skirmish => [3, 1, 3, 0],
            BattleKind::Clash => [3, 2, 3, 0],
            BattleKind::Siege => [3, 1, 3, 3],
            BattleKind::Defence | BattleKind::RushDefence => [3, 2, 3, 2],
        }
    }
}

/// The arms in the order of `arm_weights`.
const ARMS: [Arm; 4] = [Arm::Melee, Arm::Mounted, Arm::Ranged, Arm::Siege];
/// A garrison's arms: melee and ranged, as many of each.
const GARRISON_WEIGHTS: [u32; 4] = [1, 0, 1, 0];

/// What a run plays instead of whole games (`selfplay::Config::grounds`).
#[derive(Debug, Clone, PartialEq)]
pub enum Grounds {
    /// Battles of one kind: the war's length in rounds and the main army's
    /// units, each drawn per group within the range; the map scripts the
    /// groups take in turn (the suite's seeds), lands where two nations can
    /// meet.
    /// `difficulty`: the kind's knob (`BattleRecord::difficulty`), 1 the
    /// documented setting the battle eval plays; the loop's curriculum moves
    /// it per kind toward an even win share.
    /// `stage`: the start stage (`BattleRecord::stage`; None: the documented
    /// placement), the reverse curriculum's knob.
    /// `rung`: the siege ladder's rung (`LADDER`; siege only, None: none),
    /// in place of `stage`. `minimal`: the minimal world the scene needs —
    /// the smallest map (`MINIMAL_SIZE`), two nations, no city-states and no
    /// barbarians (`export::LogSpec::bare`); the suite's map otherwise.
    /// `vary`: the siege's scenes drawn per group ([`Variety`]): the war's
    /// round within `VARY_START_ROUNDS`, the target among the enemy's cities,
    /// the units among the kinds of each role, the side the army comes from;
    /// a world without a war to set drawn again (`fit_world`).
    Battle { kind: BattleKind, rounds: (u32, u32), army: (u32, u32), scripts: Vec<String>, difficulty: f64, stage: Option<u32>, rung: Option<u32>, minimal: bool, vary: bool },
    /// Economy windows (with `selfplay::Config::from`): the start round drawn
    /// per group from `rounds` (± the start's jitter; a 0 is an opening
    /// window: from the game's start, no jitter), the window's length within
    /// `window`; an opening window's length is `opening` rounds.
    Economy { rounds: Vec<u32>, window: (u32, u32), opening: u32 },
    /// Openings ([`OpeningScene`]): the suite's games from round 0, played
    /// for `window` rounds and stopped there (the game's last round
    /// unchanged), the learner commanding its units only (`selfplay::Config::heads`).
    Opening { window: u32 },
}

/// An opening window's length in Standard rounds by default (scaled to the
/// source logs' speed: 60 at Quick).
pub const OPENING_STANDARD_ROUNDS: u32 = 90;

/// A minimal battle world's map size (two nations) and scripts: the one
/// whose two capitals share their land (`duel-land`, classic/docs/ai-books.md).
pub const MINIMAL_SIZE: &str = "duel";
pub const MINIMAL_SCRIPTS: [&str; 1] = ["pangaea"];

/// The war's rounds of a varied siege (`Grounds::Battle::vary`): from the
/// first archers to the late game (the eras of the scene's units).
pub const VARY_START_ROUNDS: (u32, u32) = (30, 170);

/// A varied siege's map scripts by default: every script of the generator
/// (`fit_world` draws another world where the nations share no land).
pub fn vary_scripts() -> Vec<String> {
    kernel::scenario::map_script::MapScript::ALL.iter().map(|s| s.key().to_owned()).collect()
}

/// Worlds a varied battle tries before it fails (`fit_world`).
const VARY_WORLDS: u64 = 8;

/// The map scripts of the battles by default: one landmass or few
/// (`small-continents` and `archipelago` rarely put two nations on one
/// landmass at Tiny: no war to set).
pub const BATTLE_SCRIPTS: [&str; 3] = ["pangaea", "continents", "fractal"];

/// `A-B` or `A` (A = B) as a range.
pub fn parse_range(text: &str, flag: &str) -> Result<(u32, u32), String> {
    let (a, b) = text.split_once('-').unwrap_or((text, text));
    let n = |v: &str| v.trim().parse::<u32>().map_err(|e| format!("{flag}: {e}"));
    let (a, b) = (n(a)?, n(b)?);
    if a > b || b == 0 {
        return Err(format!("{flag}: {a}-{b} is no range"));
    }
    Ok((a, b))
}

/// A job's ground, drawn per group.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum GroundJob {
    /// `difficulty`: the knob in thousandths; `stage`: the start stage.
    Battle { kind: BattleKind, start: u32, length: u32, army: u32, seed: u64, difficulty: u32, stage: Option<u32>, rung: Option<u32>, vary: bool },
    Economy { window: u32 },
    Opening { window: u32 },
}

impl GroundJob {
    /// The stem's suffix.
    pub fn suffix(&self) -> String {
        match self {
            GroundJob::Battle { kind, .. } => format!("-{}", kind.name()),
            GroundJob::Economy { window } => format!("-w{window}"),
            GroundJob::Opening { window } => format!("-o{window}"),
        }
    }
}

/// The battle difficulty knob's range: 1 is the documented setting. Wide
/// enough that the curriculum reaches an even win share where the net holds
/// almost every defence at 4 (run2: defence 95–100 %).
pub const DIFFICULTY_RANGE: (f64, f64) = (0.125, 8.0);

/// A knob in thousandths, within `DIFFICULTY_RANGE`.
pub fn knob_permille(d: f64) -> u32 {
    (d.clamp(DIFFICULTY_RANGE.0, DIFFICULTY_RANGE.1) * 1000.0).round() as u32
}

/// A held city's damage at the start at the hardest knob: × (1 − 1 / knob)
/// of its hit points above knob 1 (defence, rush-defence).
const HELD_CITY_DAMAGE: f64 = 0.75;

/// The hit points of the city on `tile` (0 without one).
fn city_hit_points(game: &GameState, rules: &CompiledRuleset, tile: TileId) -> f64 {
    let auth = game.view().auth;
    auth.indexes().city_on_tile(tile).and_then(|c| auth.cities().get(c)).map_or(0.0, |core| f64::from(classic_rules::combat::city::max_hit_points(auth, rules, core)))
}

/// `GROUNDS_TRACE` is set (`BattleScene::trace`).
fn trace_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("GROUNDS_TRACE").is_some_and(|v| !v.is_empty() && v != "0"))
}

/// The tiles a side needs at least to be cut to its room rather than refused
/// when it has room for fewer than half its units.
const MIN_ROOM: usize = 6;

/// `n` scaled by `d`, at least 1.
fn scaled(n: u32, d: f64) -> u32 {
    ((f64::from(n) * d).round() as u32).max(1)
}

/// SplitMix64's finaliser.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn within(seed: u64, (lo, hi): (u32, u32)) -> u32 {
    lo + (seed % u64::from(hi - lo + 1)) as u32
}

/// Sets each job's ground (`selfplay::jobs`): a battle's map script, start
/// round, length, army and seed; an economy window's start round and length.
/// A group's members get the same.
pub fn assign(grounds: &Grounds, jobs: &mut [crate::selfplay::Job]) {
    for job in jobs {
        let g = job.group as u64;
        match grounds {
            Grounds::Battle { kind, rounds, army, scripts, difficulty, stage, rung, minimal, vary } => {
                if !scripts.is_empty() {
                    job.spec.script = scripts[job.group % scripts.len()].clone();
                }
                if *minimal {
                    job.spec.size = MINIMAL_SIZE.to_owned();
                    job.spec.bare = true;
                }
                let seed = mix(job.spec.seed ^ (g << 32) ^ 0xba77_1e);
                let start = within(mix(seed ^ 1), if *vary { VARY_START_ROUNDS } else { kind.start_rounds() });
                let (length, army) = (within(mix(seed ^ 2), *rounds), within(mix(seed ^ 3), *army));
                job.ground = Some(GroundJob::Battle { kind: *kind, start, length, army, seed, difficulty: knob_permille(*difficulty), stage: *stage, rung: *rung, vary: *vary });
            }
            Grounds::Economy { window, opening, .. } => {
                let seed = mix(job.spec.seed ^ (g << 32) ^ 0xec0);
                let window = if job.start.as_ref().is_some_and(|s| s.round == 0) { (*opening).max(1) } else { within(mix(seed ^ 2), *window) };
                job.ground = Some(GroundJob::Economy { window });
            }
            Grounds::Opening { window } => job.ground = Some(GroundJob::Opening { window: (*window).max(1) }),
        }
    }
}

impl Grounds {
    /// An economy window's start round for group `group` of the source log
    /// of seed `seed`, drawn from its list (None: the configured round); the
    /// start's jitter is added to it (`selfplay::jobs`).
    pub fn start_round(&self, seed: u64, group: usize) -> Option<u32> {
        match self {
            Grounds::Economy { rounds, .. } if !rounds.is_empty() => Some(rounds[(mix(seed ^ ((group as u64) << 32) ^ 0xec1) % rounds.len() as u64) as usize]),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Edits: the scene's changes outside the commands

/// A change of the world outside the commands (a harness setter), as the
/// export makes it again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// The two players have met.
    Meet { a: u8, b: u8 },
    /// `a` declares war on `b` (the rules may refuse: then nothing changes).
    War { a: u8, b: u8 },
    /// A unit of `kind` for `player` on `tile` (`hp`: wounded to it).
    Spawn {
        player: u8,
        tile: u32,
        kind: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hp: Option<u16>,
    },
    /// The city on `tile` damaged to `damage` hit points.
    Damage { tile: u32, damage: u16 },
    /// Gold for `player`.
    Earn { player: u8, gold: u32 },
    /// `player` has explored `tiles`.
    Reveal { player: u8, tiles: Vec<u32> },
    /// The unit `id` on `tile` removed (a ladder scene cleared of the
    /// sides' other units).
    Remove { tile: u32, id: u64 },
}

/// An edit and the commands applied before it: the export makes it before
/// the command at `position`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub position: u64,
    pub op: Op,
}

/// Makes `op`; a spawned unit's handle.
pub fn apply(game: &mut GameState, rules: &CompiledRuleset, op: &Op) -> Option<UnitHandle> {
    match op {
        Op::Meet { a, b } => game.debug_meet(PlayerId(*a), PlayerId(*b)),
        Op::War { a, b } => game.debug_force_war(rules, PlayerId(*a), PlayerId(*b)),
        Op::Spawn { player, tile, kind, hp } => {
            let unit = game.debug_spawn_unit(rules, PlayerId(*player), TileId(*tile), UnitKindId(*kind));
            if let Some(hp) = hp {
                game.debug_set_unit_hp(unit, *hp);
            }
            return Some(unit);
        }
        Op::Damage { tile, damage } => {
            if let Some(city) = game.view().auth.indexes().city_on_tile(TileId(*tile)) {
                game.debug_set_city_damage(city, *damage);
            }
        }
        Op::Earn { player, gold } => game.debug_earn(PlayerId(*player), *gold),
        Op::Reveal { player, tiles } => game.debug_reveal(PlayerId(*player), &tiles.iter().map(|t| TileId(*t)).collect::<Vec<_>>()),
        Op::Remove { tile, id } => {
            let auth = game.view().auth;
            let unit = auth.indexes().units_on(TileId(*tile)).iter().copied().find(|h| auth.units().get(*h).is_some_and(|u| u.core.id.0 == *id));
            if let Some(unit) = unit {
                game.debug_remove_unit(rules, unit);
            }
        }
    }
    None
}

/// Makes the edits recorded at `position`, in order (the export's replay,
/// before the command at `position`).
pub fn apply_at(game: &mut GameState, rules: &CompiledRuleset, edits: &[Edit], position: u64) {
    for edit in edits.iter().filter(|e| e.position == position) {
        apply(game, rules, &edit.op);
    }
}

/// The edits of a game's `.e2.json` (`ground.edits`; none without).
pub fn edits_of(game: &serde_json::Value) -> Result<Vec<Edit>, String> {
    match game.get("ground").and_then(|g| g.get("edits")) {
        None | Some(serde_json::Value::Null) => Ok(Vec::new()),
        Some(edits) => serde_json::from_value(edits.clone()).map_err(|e| format!("ground.edits: {e}")),
    }
}

/// Makes and records edits.
#[derive(Debug, Clone, Default)]
struct Recorder {
    edits: Vec<Edit>,
}

impl Recorder {
    fn edit(&mut self, game: &mut GameState, rules: &CompiledRuleset, position: u64, op: Op) -> Option<UnitHandle> {
        let unit = apply(game, rules, &op);
        self.edits.push(Edit { position, op });
        unit
    }
}

// ---------------------------------------------------------------------------
// Records

/// One side's units of a battle (spawned for it) and what it lost.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ArmyRecord {
    pub player: u8,
    pub kinds: Vec<u16>,
    /// The units' production cost, summed.
    pub value: u32,
    /// Units gone (killed, captured) at the end.
    pub lost_units: u32,
    /// Cost × the share of health lost, summed (a unit gone: its whole cost).
    pub lost_value: f64,
}

impl ArmyRecord {
    /// The value-weighted share of the army lost, in [0, 1].
    pub fn lost_share(&self) -> f64 {
        if self.value == 0 {
            0.0
        } else {
            (self.lost_value / f64::from(self.value)).clamp(0.0, 1.0)
        }
    }
}

/// How a battle ground went.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BattleRecord {
    pub learner: u8,
    pub enemy: u8,
    /// The war's first round and its planned length.
    pub start_round: u32,
    pub length: u32,
    /// The round play stopped at (the start of; at most start + length).
    pub end_round: u32,
    /// The city fought over (siege: the enemy's; defence: the learner's;
    /// field: none) and its owner at the end (none: razed).
    pub target: Option<u32>,
    pub target_owner: Option<u8>,
    /// Siege: the first round the learner held the target.
    pub captured_round: Option<u32>,
    /// Defence: the learner held the target to the end.
    pub held: bool,
    /// The target's highest damage seen at a round's end, a share of its
    /// hit points.
    pub target_damage: f64,
    pub learner_army: ArmyRecord,
    pub enemy_army: ArmyRecord,
    /// The battle round by round (the trainer's dense reward).
    #[serde(default)]
    pub rounds: BattleRounds,
    /// Rounds that ended at peace (the war was declared again).
    pub peace_rounds: u32,
    /// The difficulty knob (1: the documented setting; `DIFFICULTY_RANGE`):
    /// field, the enemy's army over the learner's; siege, the garrison's
    /// size, and below 1 the city starts damaged by 0.75 × (1 − knob) of its
    /// hit points; defence and rush-defence, the attacking army's size
    /// (× max(knob, 0.5), cut to its room), below 1 its health (× max(knob,
    /// 0.3)), above 1 the held city starts damaged by 0.75 × (1 − 1 / knob)
    /// of its hit points.
    #[serde(default = "one")]
    pub difficulty: f64,
    /// The start stage (the reverse curriculum's knob, [`staged_tiles`];
    /// None: the documented placement): 0 the attacking army at the walls, each
    /// unit within its reach of the target (field: the armies in contact);
    /// stage s, s rounds of the army's march farther back, at most its home
    /// city's distance (field: the gap between the armies 1 + 2 s × the
    /// march). Siege: the learner's army; defence and rush-defence: the
    /// attackers; field: both armies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<u32>,
    /// With a stage or a rung: the attacking army's nearest unit's distance to the
    /// target at the start (field: the nearest two units of the two armies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_gap: Option<i32>,
    /// The siege ladder's rung ([`LADDER`]; None: none). A rung of today's
    /// stages also keeps `stage`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rung: Option<u32>,
    /// A ladder rung below today's stages: the ranged shots and the
    /// capturers' blows that take the city to 0 in one round by the previews
    /// ([`Rung`]; shots 0 without a ranged kind).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub need: Option<(u32, u32)>,
    /// A varied siege's draws ([`Variety`]; `--battle-vary`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variety: Option<Variety>,
    /// A siege ladder rung: its scene lets the learner take the city in its
    /// first turn by construction (`Rung::same_turn`); the curriculum may
    /// count only such captures (`e2_loop --rung-success first-turn`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_turn: Option<bool>,
    /// A ladder scene's drawn spawn order (`SpawnOrder::name`; None: shooters first, the default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn_order: Option<String>,
    /// Skirmish: the first round every enemy unit of the scene was dead or
    /// routed with a unit of the learner's left (the win; `BattleScene::routed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub won_round: Option<u32>,
    /// The reward's reference ([`battle_reward`]).
    pub reward: f64,
}

/// A battle round by round, one column per number: entry 0 is the scene as
/// set (the war's first round, before anyone moved), entry k the world at
/// the start of round `round[k]` (after the round before it was played).
/// The losses are cumulative and measured as at the end (`ArmyRecord`): the
/// learner's turn of round R did what lies between the entries of R and
/// R + 1. Values to 0.1, damage shares to 0.001: a 40-round battle is about
/// 1.5 KB of JSON.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BattleRounds {
    pub round: Vec<u32>,
    pub learner_lost_units: Vec<u32>,
    pub learner_lost_value: Vec<f32>,
    pub enemy_lost_units: Vec<u32>,
    pub enemy_lost_value: Vec<f32>,
    /// The target's damage now (not the highest), a share of its hit points;
    /// 0 without a target.
    pub target_damage: Vec<f32>,
    /// A clash: the learner's ready force now ([`ready_value`]); empty otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub learner_ready: Vec<f32>,
}

impl BattleRounds {
    fn push(&mut self, round: u32, learner: &ArmyRecord, enemy: &ArmyRecord, damage: f64, ready: Option<f64>) {
        let tenth = |v: f64| ((v * 10.0).round() / 10.0) as f32;
        if let Some(r) = ready {
            self.learner_ready.push(tenth(r));
        }
        self.round.push(round);
        self.learner_lost_units.push(learner.lost_units);
        self.learner_lost_value.push(tenth(learner.lost_value));
        self.enemy_lost_units.push(enemy.lost_units);
        self.enemy_lost_value.push(tenth(enemy.lost_value));
        self.target_damage.push(((damage.clamp(0.0, 1.0) * 1000.0).round() / 1000.0) as f32);
    }

    pub fn len(&self) -> usize {
        self.round.len()
    }

    pub fn is_empty(&self) -> bool {
        self.round.is_empty()
    }
}

/// A nation's science in an economy window.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowSeat {
    pub player: u8,
    /// Science a turn, summed over the window's rounds.
    pub science: i64,
    pub techs_start: u32,
    pub techs_end: u32,
}

/// How an economy window went.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EconomyRecord {
    pub from_round: u32,
    pub window: u32,
    /// The round play stopped at (from + window, earlier when the game ended).
    pub end_round: u32,
    /// Every nation (eliminated ones too: what they made before).
    pub seats: Vec<WindowSeat>,
    /// The learners' science (their mean with several), the other nations'
    /// mean (those alive at the start), and learner / (learner + mean): the
    /// reward's science term, in [0, 1] (0.5 at par).
    pub learner_science: f64,
    pub opponents_mean: f64,
    pub science_share: f64,
    /// At the window's end (the start of `end_round`, after the learner's
    /// last turn): the learners' science a turn (their mean) and the
    /// expert's, the same players at the same round of the source log
    /// (None: no source log).
    #[serde(default)]
    pub learner_rate: f64,
    #[serde(default)]
    pub expert_rate: Option<f64>,
    /// The round of the source log the expert was measured at: `end_round`,
    /// earlier when the log ends before it.
    #[serde(default)]
    pub expert_round: Option<u32>,
    /// The learners' potential and the expert's (their means; score points).
    #[serde(default)]
    pub learner_potential: f64,
    #[serde(default)]
    pub expert_potential: Option<f64>,
    /// Per learner: its potential and the expert's, each component priced.
    #[serde(default)]
    pub potential: Vec<SeatPotential>,
}

// ---------------------------------------------------------------------------
// Potential: an empire's yields and stocks at the scripted AI's prices

/// The yields a turn of a potential, in order: science, production (the
/// cities' hammers), food (the cities' surplus), gold (net, after upkeep),
/// culture, faith.
pub const YIELDS: [&str; 6] = ["science", "production", "food", "gold", "culture", "faith"];
/// The stocks of a potential: the score's terms (cities, citizens, land,
/// world wonders, technologies; `systems::victory::score`). Buildings are no
/// stock of the score: what they make is in the yields.
pub const STOCKS: [&str; 5] = ["cities", "citizens", "land", "wonders", "techs"];

/// One player's shadow prices at one round, centi-points of the final score
/// per unit: a yield a turn kept to the last round (`Prices`), a stock
/// counted now (`ScorePoints`, the map's modifier in it) — the allocator's
/// currency, from the rules' data and the player's observation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PriceSheet {
    pub player: u8,
    pub round: u32,
    /// The AI variant whose price model priced it (the source log's).
    pub variant: String,
    /// The rounds the prices count to the end.
    pub rounds_left: u32,
    pub centi: BTreeMap<String, i64>,
}

/// What an empire holds: per component of `YIELDS` and `STOCKS` its raw
/// amount; its buildings (not priced). Zeros for a nation out of the game.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Holdings {
    pub raw: BTreeMap<String, f64>,
    pub buildings: u32,
}

/// One component of a potential: the raw amount, its price and value (score points).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub raw: f64,
    pub price: f64,
    pub value: f64,
}

/// An empire's potential: its holdings at a price sheet (score points; the
/// AI's estimate of the final score it holds and makes: the stocks counted
/// now, each yield a turn kept to the last round).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Potential {
    pub total: f64,
    pub yields: f64,
    pub stocks: f64,
    pub buildings: u32,
    pub components: BTreeMap<String, Component>,
}

/// A learner's potential and the expert's (the same player at the same
/// round of the source log), both at the expert's prices (one scale).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SeatPotential {
    pub player: u8,
    /// The prices both are valued at: the expert's (the learner's own when
    /// the expert's nation is out of the source game or there is no source).
    pub prices: PriceSheet,
    pub net: Potential,
    #[serde(default)]
    pub expert: Option<Potential>,
}

/// `player`'s holdings in `game` now.
pub fn holdings(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Holdings {
    let auth = game.view().auth;
    if !auth.players().is_alive(player) {
        return Holdings { raw: YIELDS.iter().chain(&STOCKS).map(|k| ((*k).to_owned(), 0.0)).collect(), buildings: 0 };
    }
    let cities: Vec<_> = auth.indexes().cities_of(player).iter().filter_map(|c| auth.cities().get(*c)).collect();
    let income = classic_rules::derived::income::player_income(auth, rules, player);
    let (mut production, mut food) = (0i64, 0i64);
    for core in &cities {
        let city = classic_rules::derived::income::city_income(auth, rules, core);
        production += i64::from(city.production);
        food += i64::from(city.food_surplus);
    }
    let map = auth.map();
    let land = (0..map.tile_count() as u32).filter(|t| map.owner(TileId(*t)).flatten() == Some(player) && !classic_rules::navigation::terrain::is_water(map, TileId(*t))).count();
    let is_wonder = |b: &classic_rules::ids::BuildingKindId| rules.buildings.get(b.0 as usize).is_some_and(|d| d.kind == kernel::rules::content::BuildingKind::WorldWonder);
    let wonders = cities.iter().flat_map(|c| c.buildings.iter()).filter(|b| is_wonder(b)).count();
    let raw = [
        ("science", f64::from(income.science)),
        ("production", production as f64),
        ("food", food as f64),
        ("gold", f64::from(income.gold)),
        ("culture", f64::from(income.culture)),
        ("faith", f64::from(income.faith)),
        ("cities", cities.len() as f64),
        ("citizens", cities.iter().map(|c| f64::from(c.population)).sum()),
        ("land", land as f64),
        ("wonders", wonders as f64),
        ("techs", auth.players().techs_of(player).len() as f64),
    ];
    Holdings { raw: raw.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(), buildings: cities.iter().map(|c| c.buildings.len() as u32).sum() }
}

/// `player`'s shadow prices in `game` now, as `variant`'s allocator makes
/// them (`ai::value::Prices::of_effects`, its price model and science pricing).
pub fn price_sheet(game: &GameState, rules: &CompiledRuleset, player: PlayerId, variant: Variant) -> PriceSheet {
    let obs = game.ai_observation(rules, player);
    let p = classic_rules::ai::value::Prices::of_effects(&obs, rules, variant.price_model(), variant.science_pricing(), variant.true_effects());
    let s = p.score;
    let centi = [
        ("science", p.science),
        ("production", p.production),
        ("food", p.food),
        ("gold", p.gold),
        ("culture", p.culture),
        ("faith", p.faith),
        ("cities", s.city),
        ("citizens", s.citizen),
        ("land", s.land),
        ("wonders", s.wonder),
        ("techs", s.tech),
    ];
    PriceSheet {
        player: player.0,
        round: game.view().auth.turn().round,
        variant: variant.name().to_owned(),
        rounds_left: p.rounds_left,
        centi: centi.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
    }
}

/// `holdings` at `prices`.
pub fn potential(holdings: &Holdings, prices: &PriceSheet) -> Potential {
    let mut out = Potential { buildings: holdings.buildings, ..Potential::default() };
    for (i, key) in YIELDS.iter().chain(&STOCKS).enumerate() {
        let raw = holdings.raw.get(*key).copied().unwrap_or(0.0);
        let price = prices.centi.get(*key).copied().unwrap_or(0) as f64 / 100.0;
        let value = raw * price;
        if i < YIELDS.len() {
            out.yields += value;
        } else {
            out.stocks += value;
        }
        out.components.insert((*key).to_owned(), Component { raw, price, value });
    }
    out.total = out.yields + out.stocks;
    out
}

/// The expert at a round of a source log (`selfplay::expert_at`): every
/// nation's holdings and, for the living, its price sheet.
#[derive(Debug, Clone, Default)]
pub struct ExpertAt {
    /// The round reached (the one asked, or the log's last).
    pub round: u32,
    pub seats: BTreeMap<u8, (Holdings, Option<PriceSheet>)>,
}

impl ExpertAt {
    /// The expert's snapshot of the source world `game` now, prices by `variant`.
    pub fn of(game: &GameState, rules: &CompiledRuleset, nations: &[PlayerId], variant: Variant) -> Self {
        let auth = game.view().auth;
        let seats = nations.iter().map(|p| (p.0, (holdings(game, rules, *p), auth.players().is_alive(*p).then(|| price_sheet(game, rules, *p, variant))))).collect();
        Self { round: auth.turn().round, seats }
    }
}

/// A game's ground (`.e2.json` `ground`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GroundRecord {
    /// `battle`, `economy` or `opening`.
    pub suite: String,
    /// The battle's kind, `window` for the economy and the opening.
    pub kind: String,
    #[serde(default)]
    pub battle: Option<BattleRecord>,
    #[serde(default)]
    pub economy: Option<EconomyRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opening: Option<OpeningRecord>,
    /// The changes outside the commands, in order.
    #[serde(default)]
    pub edits: Vec<Edit>,
}

fn one() -> f64 {
    1.0
}

/// The siege's shaping weight by default (`battle_reward_with`); the loop
/// moves it between 0.2 and 0.45 by the captures it sees.
pub const SIEGE_SHAPING: f64 = 0.4;

/// `battle_reward_with` at the default siege shaping.
pub fn battle_reward(kind: BattleKind, b: &BattleRecord) -> f64 {
    battle_reward_with(kind, b, SIEGE_SHAPING)
}

/// The reward of a battle ground, in [0, 1]:
/// - field: 0.5 + 0.5 (the enemy's lost share − the learner's), both value-weighted;
/// - siege: taken: 0.5 + 0.5 (1 − (round taken − start) / length); else
///   w × (0.5 × the target's highest damage share + 0.25 × the share of the
///   garrison's units killed + 0.25 × its value-weighted lost share), w < 0.5
///   (`siege_shaping`): a capture is always worth more;
/// - defence, rush-defence: 0.5 × held + 0.5 × the enemy's lost share;
/// - skirmish: 0.25 + 0.25 (the enemy's lost share − the learner's) + 0.5 W,
///   W the win's worth ([`skirmish_win`]): 1 for a win in the first round,
///   down to 0.5 at the round limit, 0 without a win.
pub fn battle_reward_with(kind: BattleKind, b: &BattleRecord, siege_shaping: f64) -> f64 {
    let enemy = b.enemy_army.lost_share();
    let r = match kind {
        BattleKind::Field => 0.5 + 0.5 * (enemy - b.learner_army.lost_share()),
        BattleKind::Skirmish => 0.25 + 0.25 * (enemy - b.learner_army.lost_share()) + 0.5 * skirmish_win(b),
        BattleKind::Clash => {
            let phi = clash_phi(b);
            0.5 + 0.5 * (phi.last().copied().unwrap_or(0.0) - phi.first().copied().unwrap_or(0.0))
        }
        BattleKind::Siege => match b.captured_round {
            Some(round) => 0.5 + 0.5 * (1.0 - f64::from(round.saturating_sub(b.start_round)) / f64::from(b.length.max(1))).clamp(0.0, 1.0),
            None => {
                let killed = f64::from(b.enemy_army.lost_units) / (b.enemy_army.kinds.len().max(1) as f64);
                siege_shaping.clamp(0.0, 0.49) * (0.5 * b.target_damage.clamp(0.0, 1.0) + 0.25 * killed.clamp(0.0, 1.0) + 0.25 * enemy)
            }
        },
        BattleKind::Defence | BattleKind::RushDefence => 0.5 * f64::from(u8::from(b.held)) + 0.5 * enemy,
    };
    r.clamp(0.0, 1.0)
}

/// A clash's potential per round entry, a share of the learner's army value:
/// Φ = (the enemy's value destroyed so far + the learner's ready force now)
/// / the learner's army value. The value destroyed is the enemy's lost value
/// (cost × the share of health lost; a unit dead, its cost), the ready force
/// [`ready_value`]'s. The reward is 0.5 + 0.5 (Φ(end) − Φ(start)), clamped
/// to [0, 1]: trading a whole unit for little damage loses, finishing a
/// wounded enemy pays its remaining value.
pub fn clash_phi(b: &BattleRecord) -> Vec<f64> {
    let r = &b.rounds;
    let value = f64::from(b.learner_army.value.max(1));
    r.enemy_lost_value.iter().zip(&r.learner_ready).map(|(e, m)| (f64::from(*e) + f64::from(*m)) / value).collect()
}

/// A unit counts whole in the ready force from this share of its hit points
/// up: at half health a striker still deals 83 % of a whole one's damage (1 %
/// less per 3 hit points missing, `combat::damage::WOUNDED_DAMAGE_PERCENT`)
/// and survives a blow; below it, its cost × hit points / this.
pub const READY_SHARE: f64 = 0.5;

/// The ready force of `side`'s `units` now: per living unit its cost ×
/// min(1, its hit points / (`READY_SHARE` × the most)).
fn ready_value(game: &GameState, rules: &CompiledRuleset, side: PlayerId, units: &[(UnitHandle, UnitKindId)]) -> f64 {
    let auth = game.view().auth;
    let full = f64::from(kernel::world::unit::UNIT_MAX_HP) * READY_SHARE;
    units
        .iter()
        .filter_map(|(h, k)| auth.units().get(*h).filter(|u| u.core.owner == side).map(|u| f64::from(rules.units[k.0 as usize].cost) * (f64::from(u.core.hp) / full).min(1.0)))
        .sum()
}

/// A clash's gap at approach stage `stage`, the nearest units of the two
/// armies: 1 at 0 (the fronts in contact), else 1 + 2 × stage × the slower
/// side's march (each side `stage` rounds of march from contact).
fn clash_gap(rules: &CompiledRuleset, stage: u32, mine: &[UnitKindId], theirs: &[UnitKindId]) -> i32 {
    1 + 2 * stage as i32 * march_step(rules, mine).min(march_step(rules, theirs))
}

/// A skirmish win's worth: 1 − 0.5 × (the rounds it took − 1) / the round
/// limit (1 for a win in the war's first round, above 0.5 at the limit);
/// 0 without a win.
pub fn skirmish_win(b: &BattleRecord) -> f64 {
    b.won_round.map_or(0.0, |round| 1.0 - 0.5 * f64::from(round.saturating_sub(b.start_round).saturating_sub(1)) / f64::from(b.length.max(1)))
}

// ---------------------------------------------------------------------------
// Battles: the scene

/// The world a battle job starts from is the suite game played by
/// `standard` on every seat to the war's round. Returns the commands
/// applied (they go to the game's journal) and the units each player could
/// build in the last round; None for other jobs. The scripted play is made
/// once per start (`spec`, round) in the process and its log replayed into
/// each member's game (`PREFIXES`): a group's members share it, and a replay
/// costs a fraction of the play.
pub fn play_prefix(rules: &CompiledRuleset, game: &mut GameState, spec: &crate::export::LogSpec, ground: Option<&GroundJob>) -> Result<Option<(u64, Kinds)>, String> {
    let Some(&GroundJob::Battle { start, .. }) = ground else { return Ok(None) };
    let key = format!("{spec:?}@{start}");
    let cell = PREFIXES.lock().map_err(|_| "the prefix cache is poisoned".to_owned())?.entry(key).or_default().clone();
    let prefix = cell
        .get_or_init(|| {
            let mut played = crate::export::start(rules, &spec.game()?);
            let mut journal = played.begin_journal(rules);
            // The world before the war: the scripted expert (`SCRIPTED_EXPERT`), as the scenes were measured.
            let mut driver = AiDriver::with_mind(ClassicBench::settings(rules), Vec::new(), ClassicMind::new(crate::bench::modules::SCRIPTED_EXPERT_VARIANT));
            driver.set_trace_capacity(0);
            let kinds = battle::play_to(rules, &mut played, &mut driver, start)?;
            journal.extend(played.take_journal());
            Ok(std::sync::Arc::new(Prefix { journal, commands: driver.counters().commands_applied, kinds }))
        })
        .clone()?;
    let replayed = replay_journal(rules, game, &prefix.journal)?;
    if replayed != prefix.commands {
        return Err(format!("the prefix replayed {replayed} commands, played {}", prefix.commands));
    }
    Ok(Some((prefix.commands, prefix.kinds.clone())))
}

/// The units each player could build (`battle::play_to`).
pub type Kinds = BTreeMap<PlayerId, Vec<UnitKindId>>;

/// A battle's scripted prefix: its journal, commands and buildable units.
#[derive(Debug)]
struct Prefix {
    journal: Vec<u8>,
    commands: u64,
    kinds: Kinds,
}

type PrefixCell = std::sync::Arc<std::sync::OnceLock<Result<std::sync::Arc<Prefix>, String>>>;

/// The prefixes played in this process, by start.
static PREFIXES: std::sync::Mutex<BTreeMap<String, PrefixCell>> = std::sync::Mutex::new(BTreeMap::new());

/// Replays a journal's commands into `game` (a new game of the journal's
/// start, its own journal on: the commands are recorded again), every round
/// hash checked. Returns the commands replayed.
fn replay_journal(rules: &CompiledRuleset, game: &mut GameState, journal: &[u8]) -> Result<u64, String> {
    use kernel::replay::journal::{take_record, JournalHeader, Record};
    let (header, mut reader) = JournalHeader::read(journal).map_err(|e| format!("the prefix: {e}"))?;
    if header.world != kernel::replay::hash::authoritative_hash(game) {
        return Err("the prefix starts from another world".to_owned());
    }
    let mut commands = 0;
    while !reader.is_at_end() {
        match take_record::<Classic>(&mut reader, game.view().auth).map_err(|e| format!("the prefix: record damaged ({e:?})"))? {
            Record::Command(command) => {
                game.execute(rules, command).map_err(|e| format!("the prefix diverged: a command refused ({e:?})"))?;
                commands += 1;
            }
            Record::Checkpoint { round, hash } => {
                if kernel::replay::hash::authoritative_hash(game) != hash {
                    return Err(format!("the prefix diverged after round {round}"));
                }
            }
            _ => return Err("the prefix holds a seat window".to_owned()),
        }
    }
    Ok(commands)
}

/// A battle being played (a copy plays a search's rollout, `search.rs`).
#[derive(Debug, Clone)]
pub struct BattleScene {
    kind: BattleKind,
    learner: PlayerId,
    enemy: PlayerId,
    start: u32,
    length: u32,
    target: Option<TileId>,
    /// The war objectives (the attacker of a defence, the field's enemy
    /// army, the siege's learner: its AI memory, as a declaration of war
    /// leaves it; the net's units do not read it), aimed again after a peace.
    aims: Vec<(PlayerId, TileId)>,
    armies: [(PlayerId, Vec<(UnitHandle, UnitKindId)>); 2],
    captured_round: Option<u32>,
    target_damage: f64,
    peace_rounds: u32,
    end_round: u32,
    difficulty: f64,
    stage: Option<u32>,
    start_gap: Option<i32>,
    rung: Option<u32>,
    need: Option<(u32, u32)>,
    variety: Option<Variety>,
    won_round: Option<u32>,
    /// A one-turn rung (`Rung::one_turn`): a capture counts in the learner's first turn only.
    one_turn: bool,
    same_turn: Option<bool>,
    spawn_order: Option<SpawnOrder>,
    /// The round the scripted learner's strikes were made in (`strike`).
    struck_round: Option<u32>,
    /// A skirmish's enemy started whole: a unit of it beaten below the ready
    /// share counts routed (`routed`); a wounded one must be killed.
    enemy_whole: bool,
    /// The learner's turns played.
    learner_turns: u32,
    recorder: Recorder,
    rounds: BattleRounds,
}

/// A side's units drawn from the arm weights and the kinds of each arm
/// (`battle::best_kinds`), an arm the side lacks replaced as `set_army`
/// does (`battle::fallback`); ordered melee first (they take the front
/// tiles).
fn plan(kinds: &BTreeMap<Arm, UnitKindId>, weights: [u32; 4], n: u32, seed: u64) -> Result<Vec<UnitKindId>, String> {
    let total: u32 = weights.iter().sum();
    let mut out = Vec::with_capacity(n as usize);
    for slot in 0..n {
        let mut x = (mix(seed ^ (u64::from(slot) << 8)) % u64::from(total.max(1))) as u32;
        let mut wanted = ARMS[0];
        for (arm, w) in ARMS.iter().zip(weights) {
            if x < w {
                wanted = *arm;
                break;
            }
            x -= w;
        }
        let mut arm = Some(wanted);
        while let Some(a) = arm {
            if let Some(&kind) = kinds.get(&a) {
                out.push((a, kind));
                break;
            }
            arm = battle::fallback(a);
        }
        if arm.is_none() {
            return Err("a side can field no melee unit".to_owned());
        }
    }
    out.sort_by_key(|(arm, kind)| (*arm, kind.0));
    Ok(out.into_iter().map(|(_, k)| k).collect())
}

/// The attack army of the `rush-siege` book (`ai::books::plans::Plan::of`,
/// classic/docs/ai-books.md) for the attacker's nation, each slot's count
/// × `d` (at least one a slot): per slot its first kind the nation fields
/// without a resource (its unique unit, else the generic one; the kind the
/// book builds), melee first (they take the front tiles). At knob 1: four
/// ranged, three siege or melee, two melee to take the city — the scripted
/// side's war aim at the capital (`battle::aim`) plays the attack. The
/// book's own mind on the attacker's seat (its muster, launch and
/// `press_siege`) was tried and took no capital at any knob (0 of 80, its
/// plan starts from its opening, `Phase::Build`): `standard` attacks.
/// The army replaces the old ranged and siege rush, which wore the capital
/// down and could not take it (no melee: run2 held 100 % at knob 4).
pub fn rush_army(rules: &CompiledRuleset, nation: u8, d: f64) -> Vec<UnitKindId> {
    use classic_rules::ai::books::{plans::Plan, Book};
    let plan = Plan::of(Book::RushSiege, rules, nation);
    let free = |k: &UnitKindId| rules.units[k.0 as usize].nation == Some(nation) || rules.units[k.0 as usize].required_resources.is_empty();
    let mut out: Vec<(Option<Arm>, UnitKindId)> = Vec::new();
    for slot in &plan.army {
        let Some(kind) = slot.kinds.iter().copied().find(free).or_else(|| slot.kinds.first().copied()) else { continue };
        let arm = battle::kind_arm(rules, kind);
        out.extend(std::iter::repeat_n((arm, kind), scaled(slot.count, d) as usize));
    }
    out.sort_by_key(|(arm, kind)| (*arm, kind.0));
    out.into_iter().map(|(_, k)| k).collect()
}

/// THE SIEGE HOOK: what a siege scene sets on the target city beyond its
/// garrison and the knob's damage (the edits made and recorded with the
/// others, at the scene's position). None yet: the siege agent's city
/// defences (`ai/siege_walls.rs`, walls and their reach) plug in here, and
/// the trainer's shaping (`train_e2.battle_reward`, `--siege-shaping`, the
/// dense per-round damage) stays as it is until then.
fn siege_hook(_rules: &CompiledRuleset, _game: &GameState, _learner: PlayerId, _enemy: PlayerId, _city: TileId, _d: f64) -> Vec<Op> {
    Vec::new()
}

/// Free land around `city` of its owner (the city's tile first, then the
/// ring, then the next), nearest first: a garrison's places.
fn garrison_tiles(rules: &CompiledRuleset, game: &GameState, city: TileId, n: usize) -> Vec<TileId> {
    let auth = game.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    let owner = auth.indexes().city_on_tile(city).and_then(|c| auth.cities().get(c)).map(|c| c.owner);
    let mut tiles: Vec<(i32, TileId)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter_map(|t| Some((dims.distance(t, city).filter(|d| *d <= 2)?, t)))
        .filter(|&(d, t)| {
            let own = d == 0 || map.owner(t).flatten().is_none_or(|o| Some(o) == owner);
            own && map.land_area(t) == map.land_area(city)
                && (d == 0 || auth.indexes().city_on_tile(t).is_none())
                && auth.indexes().units_on(t).is_empty()
                && (d == 0 || battle::standable(rules, game, t))
        })
        .collect();
    tiles.sort();
    tiles.into_iter().take(n).map(|(_, t)| t).collect()
}

/// The documented field margin (`field_tiles`): a field army stands nearer
/// its own city than the enemy's by at least this.
const FIELD_MARGIN: i32 = 2;

/// Free land between two cities for a field army of `side`: on its half
/// (nearer `own` than `enemy` by at least `margin`, so the armies start
/// apart), out of every city's strike (3 tiles or more from any city),
/// nearest the middle line first, then nearest the line between the cities.
/// `short`: when the band has no room for all, the tiles of the side's half
/// short of the margin follow, the least short first (a start stage);
/// without, the army is cut to the band's room (the documented placement).
fn field_tiles(rules: &CompiledRuleset, game: &GameState, own: TileId, enemy: TileId, n: usize, margin: i32, short: bool) -> Vec<TileId> {
    let auth = game.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    let cities: Vec<TileId> = (0..auth.players().count() as u8)
        .flat_map(|p| auth.indexes().cities_of(PlayerId(p)).to_vec())
        .filter_map(|h| auth.cities().get(h).map(|c| c.tile))
        .collect();
    let area = map.land_area(own);
    let lowest = if short { 1 } else { margin };
    let mut tiles: Vec<(i32, i32, i32, TileId)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|&t| map.land_area(t) == area && auth.indexes().units_on(t).is_empty() && battle::standable(rules, game, t))
        .filter(|&t| cities.iter().all(|c| dims.distance(t, *c).is_some_and(|d| d > classic_rules::combat::city::CITY_ATTACK_RANGE)))
        .filter_map(|t| {
            let (da, db) = (dims.distance(t, own)?, dims.distance(t, enemy)?);
            let m = db - da;
            (m >= lowest).then_some(((m < margin) as i32, if m < margin { margin - m } else { m }, da + db, t))
        })
        .collect();
    tiles.sort();
    tiles.into_iter().take(n).map(|(_, _, _, t)| t).collect()
}

/// A unit's reach: the distance it strikes a tile from (its range; 1 for a
/// unit without a ranged attack).
fn reach(rules: &CompiledRuleset, kind: UnitKindId) -> i32 {
    classic_rules::ai::combat::ranged_range(rules, kind).unwrap_or(1)
}

/// A round of an army's march in tiles: its slowest unit's movement (at
/// least 1; terrain costs aside).
fn march_step(rules: &CompiledRuleset, kinds: &[UnitKindId]) -> i32 {
    kinds.iter().filter_map(|k| rules.units.get(k.0 as usize)).map(|d| i32::from(d.movement.max(1))).min().unwrap_or(1)
}

/// The start stage's land for an army of `side` (`kinds`, melee first)
/// attacking the city on `target`: each unit, in order, takes the free land
/// tile of the target's landmass at `reach + stage × march_step` tiles from
/// the target or farther (stage 0: within its reach, at the walls; stage s:
/// s rounds of march back), at most the distance of `side`'s city nearest
/// the target (its home: a far stage puts the army there), nearest that
/// distance and the line from home to the target first; `taken` (the
/// garrison's places) excluded. A unit with no tile at its distance takes
/// the nearest short of it.
/// `from`: the side the army comes from (a varied scene's draw,
/// [`approach`]): the line from it to the target in place of home's.
#[allow(clippy::too_many_arguments)]
fn staged_tiles(rules: &CompiledRuleset, game: &GameState, side: PlayerId, target: TileId, kinds: &[UnitKindId], back: Back, taken: &[TileId], from: Option<TileId>) -> Vec<TileId> {
    let auth = game.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    let area = map.land_area(target);
    let home = auth
        .indexes()
        .cities_of(side)
        .iter()
        .filter_map(|h| auth.cities().get(*h))
        .filter(|c| map.land_area(c.tile) == area)
        .filter_map(|c| Some((dims.distance(c.tile, target)?, c.tile)))
        .min();
    // (distance to the target, its detour off the line from home, tile).
    let mut free: Vec<(i32, i32, TileId)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|&t| {
            map.land_area(t) == area && !taken.contains(&t) && auth.indexes().city_on_tile(t).is_none() && auth.indexes().units_on(t).is_empty() && battle::standable(rules, game, t)
        })
        .filter_map(|t| {
            let d = dims.distance(t, target).filter(|d| *d >= 1)?;
            let line = from.and_then(|f| Some((dims.distance(f, target)?, f))).or(home);
            let detour = line.and_then(|(far, h)| Some(d + dims.distance(t, h)? - far)).unwrap_or(0);
            Some((d, detour, t))
        })
        .collect();
    let step = march_step(rules, kinds);
    let mut out = Vec::with_capacity(kinds.len());
    for &kind in kinds {
        let mut want = reach(rules, kind)
            + match back {
                Back::Rounds(stage) => stage as i32 * step,
                Back::Tiles(tiles) => tiles as i32,
            };
        if let Some((far, _)) = home {
            want = want.min(far.max(1));
        }
        let key = |&(d, detour, t): &(i32, i32, TileId)| if d >= want { (0, d - want + detour, d, t.0) } else { (1, want - d, d, t.0) };
        let Some(best) = (0..free.len()).min_by_key(|&i| key(&free[i])) else { break };
        out.push(free.swap_remove(best).2);
    }
    out
}

/// How far back of its reach an army starts (`staged_tiles`): rounds of its
/// march (a start stage) or tiles (a ladder's `Rung::Back`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Back {
    Rounds(u32),
    Tiles(u32),
}

/// The nearest distance between two sets of tiles (None: either empty).
fn gap(game: &GameState, a: &[TileId], b: &[TileId]) -> Option<i32> {
    let dims = game.view().auth.map().dimensions;
    a.iter().flat_map(|x| b.iter().filter_map(move |y| dims.distance(*x, *y))).min()
}

/// The tiles within `reach` of `centre`.
fn around(game: &GameState, centre: TileId, reach: i32) -> Vec<u32> {
    let dims = game.view().auth.map().dimensions;
    (0..game.view().auth.map().tile_count() as u32).filter(|t| dims.distance(TileId(*t), centre).is_some_and(|d| d <= reach)).collect()
}

// ---------------------------------------------------------------------------
// The siege ladder: a longer, easier reverse curriculum below today's stages

/// A rung of the siege ladder (`--battle-rung R`, `BattleRecord::rung`):
/// the scenes below today's start stages, easiest first. Every count comes
/// from the rules' combat previews against the target as set (`ladder_kinds`):
/// `shots` is the ranged shots whose expected damage, with one capturer's
/// blow, takes the city from its hit points to 0 in one round; `blows` the
/// capturers' blows that do it alone. `room` is the tiles a shooter strikes
/// the city from (its walls but the capturers', then the tiles within the
/// shooter's range with a line of sight). Rungs 1–8 fit the city to its
/// room: a city whose `shots` exceed half its room starts worn to the hit
/// points room / 2 shots and a blow take in a round (the full ring is then
/// twice the need, as on open ground).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rung {
    /// The city at 1 hit point (the least the rules leave a living city),
    /// one capturer at its walls: one blow takes it.
    Blow,
    /// One turn (`one_turn`): the city at 1 hit point, the capturer `lo..=hi`
    /// tiles from the nearest free wall, from where the rules' pathing takes
    /// it to a free wall this turn with a move left to strike (terrain, zones
    /// of control): move, then strike.
    Reach(u32, u32),
    /// One turn: the city at 1 hit point, every free wall held by a ranged
    /// unit of the learner, the capturer 2 tiles out: a shooter steps aside
    /// (to a tile the rules' pathing reaches), the capturer steps in and strikes.
    Blocked,
    /// One turn: the city at the hit points a volley and one blow take — the
    /// fewest shooters (in range, the far tiles first) whose least shots and
    /// the capturer's least blow take it, more than its greatest blow alone —
    /// the capturer `lo..=hi` tiles from a free wall (0: on it): shoot, (move,) strike.
    Volley(u32, u32),
    /// One turn: the capture-capable kind with the most moves far from the
    /// city — from where it reaches a free wall with a move left and a slower
    /// capture-capable kind would not — and up to two slower capturers where
    /// they reach no wall this turn; `volley`: with the shooters and the hit
    /// points of `Volley` (for the fast kind's blow).
    Fast { volley: bool },
    /// The capturers (`LADDER_CAPTURERS`) and `shots + extra` ranged units
    /// at the walls, at most the room (`FULL`: the whole room, the full ring):
    /// the ring shrinks by one shooter a rung.
    Ring { extra: i32 },
    /// Capturers only at the walls, `blows + extra` of them (at least 1).
    Melee { extra: i32 },
    /// The full ring `tiles` tiles farther back than its reach (`staged_tiles`):
    /// the distance grows by one tile a rung.
    Back { tiles: u32 },
    /// The full ring at the walls with the clutter of a real siege back, one
    /// element a rung: `units` enemy units next to the city (2–3 tiles out),
    /// `strike` a target with another enemy city whose strike reaches the
    /// walls, `reinforce` enemy units arriving (4–5 tiles out), and the city's
    /// hit points `hp` percent of the way from the fitted ones to its full.
    Clutter { units: u32, strike: bool, reinforce: u32, hp: u32 },
    /// The scene of the rung inside with the units created in a drawn order
    /// (`SpawnOrder::draw`: shooters first, capturers first or shuffled), so
    /// the order the units act in has to be chosen (a net's defer choice).
    Disorder(&'static Rung),
}

/// The order a ladder scene's learner units are created in, and so decide
/// in (`kernel::ai::driver`: by id, one pass, a unit that does not act is
/// done for the turn unless the mind defers it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnOrder {
    /// Shooters, other units, the capture kind last (the ladder's default).
    ShootersFirst,
    /// The capture kind first, then the others.
    CapturersFirst,
    /// A drawn order.
    Shuffled,
}

impl SpawnOrder {
    pub fn name(self) -> &'static str {
        match self {
            SpawnOrder::ShootersFirst => "shooters-first",
            SpawnOrder::CapturersFirst => "capturers-first",
            SpawnOrder::Shuffled => "shuffled",
        }
    }

    /// A scene's order drawn from its seed: each of the three alike.
    fn draw(seed: u64) -> Self {
        [SpawnOrder::ShootersFirst, SpawnOrder::CapturersFirst, SpawnOrder::Shuffled][(mix(seed ^ 0x5b0e_0d) % 3) as usize]
    }
}

/// `--battle-spawn-order random`: every siege ladder rung draws its spawn
/// order (`SpawnOrder::draw`); off (the default) a scene spawns shooters
/// first unless its rung is a `Rung::Disorder`.
static RANDOM_SPAWN_ORDER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Sets `--battle-spawn-order random` for the process.
pub fn set_random_spawn_order(on: bool) {
    RANDOM_SPAWN_ORDER.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// `Rung::Ring`'s whole room.
pub const FULL: i32 = 99;

/// The ladder's rungs below today's start stages; rung `LADDER.len() + s`
/// is today's siege at start stage s (the normal army and garrison, the
/// same scene and hash as `--battle-stage s`). One thing changes a rung, in
/// small steps (the owner, 2 November): the ring one shooter fewer a rung,
/// the clutter back one element a rung (enemy units, a neighbour's strike,
/// reinforcements, the city's hit points), melee only, the one-turn rungs
/// (shoot, move, step aside, ride in, then strike), the distance one tile a
/// rung. Every rung's scene is cleared first (`set_battle`: the sides' other
/// units removed, the most isolated target); the order is by the net's and
/// `standard`'s measured capture shares (classic/docs/ai-net-data.md,
/// «The siege ladder»): the net, which never moves a unit and strikes with it
/// in one turn yet, meets the one-turn movement rungs after the rings. The
/// `Disorder` rungs (the units created in a drawn order) stand where the
/// net's share on them fits: they change its share (the full ring 56 → 34 %,
/// the volley 47 → 25 %, the exact ring 38 → 16 %), not `standard`'s.
pub const LADDER: [Rung; 41] = [
    Rung::Ring { extra: FULL },
    Rung::Ring { extra: 3 },
    Rung::Clutter { units: 1, strike: false, reinforce: 0, hp: 0 },
    Rung::Ring { extra: 2 },
    Rung::Ring { extra: 1 },
    Rung::Clutter { units: 2, strike: false, reinforce: 0, hp: 0 },
    Rung::Volley(0, 0),
    Rung::Blow,
    Rung::Clutter { units: 3, strike: false, reinforce: 0, hp: 0 },
    Rung::Disorder(&Rung::Ring { extra: FULL }),
    Rung::Ring { extra: 0 },
    Rung::Clutter { units: 3, strike: true, reinforce: 0, hp: 0 },
    Rung::Disorder(&Rung::Volley(0, 0)),
    Rung::Clutter { units: 3, strike: true, reinforce: 1, hp: 0 },
    Rung::Clutter { units: 3, strike: true, reinforce: 2, hp: 0 },
    Rung::Clutter { units: 3, strike: true, reinforce: 3, hp: 0 },
    Rung::Clutter { units: 3, strike: true, reinforce: 3, hp: 20 },
    Rung::Clutter { units: 3, strike: true, reinforce: 3, hp: 40 },
    Rung::Clutter { units: 3, strike: true, reinforce: 3, hp: 60 },
    Rung::Disorder(&Rung::Ring { extra: 0 }),
    Rung::Clutter { units: 3, strike: true, reinforce: 3, hp: 80 },
    Rung::Clutter { units: 3, strike: true, reinforce: 3, hp: 100 },
    Rung::Melee { extra: 2 },
    Rung::Ring { extra: -1 },
    Rung::Ring { extra: -2 },
    Rung::Melee { extra: 1 },
    Rung::Melee { extra: 0 },
    Rung::Volley(2, 2),
    Rung::Volley(1, 1),
    Rung::Reach(2, 2),
    Rung::Fast { volley: false },
    Rung::Fast { volley: true },
    Rung::Reach(1, 1),
    Rung::Blocked,
    Rung::Reach(3, 3),
    Rung::Back { tiles: 1 },
    Rung::Back { tiles: 2 },
    Rung::Back { tiles: 3 },
    Rung::Back { tiles: 4 },
    Rung::Back { tiles: 5 },
    Rung::Back { tiles: 6 },
];

impl Rung {
    /// A capture counts only in the learner's first turn, and the battle ends after its first round.
    pub fn one_turn(self) -> bool {
        match self {
            Rung::Disorder(inner) => inner.one_turn(),
            _ => matches!(self, Rung::Reach(..) | Rung::Blocked | Rung::Volley(..) | Rung::Fast { .. }),
        }
    }
}

/// The capturers of a ring or a march rung.
const LADDER_CAPTURERS: usize = 2;

/// The learner's kinds for the ladder and their expected damage to the city.
#[derive(Debug, Clone)]
struct LadderKinds {
    /// The ranged kind (range > 1) whose shot from the walls does the most
    /// expected damage this round (a kind that must set up first only when
    /// no other shoots; the rules' cheapest land one when the learner builds
    /// none yet) and that damage.
    ranged: Option<(UnitKindId, u32)>,
    /// The kind whose melee blow on the city captures it (no `NoCapture`)
    /// with the most expected damage, the least taken on a tie, and that damage.
    capturer: (UnitKindId, u32),
    /// The ranged kind's least shot and the capturer's least and greatest
    /// blow (the previews' rolls).
    shot_min: u32,
    blow_range: (u32, u32),
    /// Every kind whose blow captures the city, with its least and greatest blow.
    capturers: Vec<(UnitKindId, u32, u32)>,
}

impl LadderKinds {
    /// The ranged shots that, with one capturer's blow, take `hp` to 0 (0 without a ranged kind).
    fn shots(&self, hp: u32) -> u32 {
        self.ranged.map_or(0, |(_, shot)| hp.saturating_sub(self.capturer.1).div_ceil(shot.max(1)).max(1))
    }

    /// The capturers' blows that take `hp` to 0 alone.
    fn blows(&self, hp: u32) -> u32 {
        hp.div_ceil(self.capturer.1.max(1)).max(1)
    }
}

/// The ladder's kinds of `learner` against the city on `city`, measured on
/// copies of `game` (as set: the war, the garrison): each candidate spawned
/// on the free tile `wall` next to the city, its attack planned by the rules
/// (`combat::exchange`; a city's strength does not depend on its damage).
/// `pick`: a varied scene's draw — the ranged kind and the capturer drawn
/// among every kind of the role (the shooters ready this round first) instead of the best.
fn ladder_kinds(rules: &CompiledRuleset, game: &GameState, learner: PlayerId, kinds: &[UnitKindId], city: TileId, wall: TileId, pick: Option<u64>) -> Result<LadderKinds, String> {
    use classic_rules::combat::damage::Roll;
    use classic_rules::combat::exchange::{plan_attack, plan_attack_from, AttackKind};
    let mut kinds = kinds.to_vec();
    kinds.sort_by_key(|k| k.0);
    kinds.dedup();
    // A learner that builds no ranged unit yet gets the rules' cheapest land military one (no nation's own).
    if !kinds.iter().any(|k| classic_rules::ai::combat::ranged_range(rules, *k).is_some_and(|r| r > 1)) {
        let cheapest = (0..rules.units.len() as u16)
            .map(UnitKindId)
            .filter(|&k| {
                let def = &rules.units[k.0 as usize];
                def.domain == kernel::rules::content::Domain::Land
                    && def.nation.is_none()
                    && battle::kind_arm(rules, k).is_some()
                    && kernel::ai::plan::role_of(rules, k) == kernel::ai::plan::AiRole::Military
                    && classic_rules::ai::combat::ranged_range(rules, k).is_some_and(|r| r > 1)
            })
            .min_by_key(|&k| (rules.units[k.0 as usize].cost, k.0));
        kinds.extend(cheapest);
    }
    // (ready this round, expected damage, kind, least) of the shooters; (expected damage, −taken, kind, least, greatest) of the capturers.
    let mut ranged: Option<(bool, u16, UnitKindId, u16)> = None;
    let mut capturer: Option<(u16, i32, UnitKindId, u16, u16)> = None;
    let (mut shooters, mut capturers) = (Vec::new(), Vec::new());
    for &kind in &kinds {
        let mut trial = game.clone();
        let unit = trial.debug_spawn_unit(rules, learner, wall, kind);
        let view = trial.view();
        if classic_rules::ai::combat::ranged_range(rules, kind).is_some_and(|r| r > 1) {
            let shot = match plan_attack(view, rules, unit, city, AttackKind::Ranged) {
                Ok(p) => Some((true, p.damage_to_defender(Roll::Expected), p.preview().to_defender.0)),
                Err(_) => plan_attack_from(view, rules, unit, wall, city, AttackKind::Ranged).ok().map(|p| (false, p.damage_to_defender(Roll::Expected), p.preview().to_defender.0)),
            };
            if let Some((ready, dmg, least)) = shot {
                shooters.push((ready, dmg, kind, least));
                if ranged.is_none_or(|r| (ready, dmg, kind.0) > (r.0, r.1, r.2 .0)) {
                    ranged = Some((ready, dmg, kind, least));
                }
            }
        } else if let Ok(p) = plan_attack(view, rules, unit, city, AttackKind::Melee) {
            let core = view.auth.units().get(unit).map(|u| *u.core).ok_or("a spawned unit")?;
            let no_capture = kernel::effects::unit::unit_modifiers(view.auth, rules, &core).has(classic_rules::rules::effects::UnitAbility::NoCapture);
            let (dealt, taken) = (p.damage_to_defender(Roll::Expected), p.damage_to_attacker(Roll::Expected));
            let (least, _, most) = p.preview().to_defender;
            if !no_capture && dealt > 0 {
                capturers.push((dealt, -i32::from(taken), kind, least, most));
            }
            if !no_capture && dealt > 0 && capturer.is_none_or(|c| (dealt, -i32::from(taken), kind.0) > (c.0, c.1, c.2 .0)) {
                capturer = Some((dealt, -i32::from(taken), kind, least, most));
            }
        }
    }
    let all: Vec<(UnitKindId, u32, u32)> = capturers.iter().map(|c| (c.2, u32::from(c.3), u32::from(c.4))).collect();
    if let Some(seed) = pick {
        let ready: Vec<_> = shooters.iter().copied().filter(|s| s.0).collect();
        let pool = if ready.is_empty() { shooters } else { ready };
        if !pool.is_empty() {
            ranged = Some(pool[(mix(seed ^ 0x5a) % pool.len() as u64) as usize]);
        }
        if !capturers.is_empty() {
            capturer = Some(capturers[(mix(seed ^ 0xca) % capturers.len() as u64) as usize]);
        }
    }
    let (blow, _, capturer, least, most) = capturer.ok_or("the learner fields no unit whose blow captures a city")?;
    Ok(LadderKinds {
        ranged: ranged.map(|(_, shot, kind, _)| (kind, u32::from(shot))),
        capturer: (capturer, u32::from(blow)),
        shot_min: ranged.map_or(0, |r| u32::from(r.3)),
        blow_range: (u32::from(least), u32::from(most)),
        capturers: all,
    })
}

/// Free land of the city's landmass at `d` tiles from it (`d` = 1: its walls),
/// `taken` excluded; in tile order.
fn ring_tiles(rules: &CompiledRuleset, game: &GameState, city: TileId, d: i32, taken: &[TileId]) -> Vec<TileId> {
    let auth = game.view().auth;
    let map = auth.map();
    (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|&t| {
            map.dimensions.distance(t, city) == Some(d)
                && map.land_area(t) == map.land_area(city)
                && !taken.contains(&t)
                && auth.indexes().city_on_tile(t).is_none()
                && auth.indexes().units_on(t).is_empty()
                && battle::standable(rules, game, t)
        })
        .collect()
}

/// `v` in a seeded order (Fisher–Yates on `mix`).
fn shuffle<T>(v: &mut [T], seed: u64) {
    for i in (1..v.len()).rev() {
        let j = (mix(seed ^ (i as u64) << 20) % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
}

/// A varied scene's side of approach: a free land tile of the target's
/// landmass drawn among those no farther than `side`'s home (its city nearest
/// the target) and at least 2 from the target — the army's line runs from it
/// (`staged_tiles`). None: no such tile.
fn approach(rules: &CompiledRuleset, game: &GameState, side: PlayerId, target: TileId, seed: u64) -> Option<TileId> {
    let auth = game.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    let area = map.land_area(target);
    let far = auth.indexes().cities_of(side).iter().filter_map(|h| auth.cities().get(*h)).filter(|c| map.land_area(c.tile) == area).filter_map(|c| dims.distance(c.tile, target)).min().unwrap_or(6).max(3);
    let tiles: Vec<TileId> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|&t| map.land_area(t) == area && dims.distance(t, target).is_some_and(|d| (2..=far).contains(&d)) && battle::standable(rules, game, t))
        .collect();
    (!tiles.is_empty()).then(|| tiles[(mix(seed ^ 0xa9) % tiles.len() as u64) as usize])
}

/// A varied army's kinds: per arm a kind drawn among `kinds` of that arm
/// (`battle::kind_arm`) instead of the strongest (`battle::best_kinds`).
fn drawn_kinds(rules: &CompiledRuleset, kinds: &[UnitKindId], seed: u64) -> BTreeMap<Arm, UnitKindId> {
    let mut by_arm: BTreeMap<Arm, Vec<UnitKindId>> = BTreeMap::new();
    for &k in kinds {
        if let Some(arm) = battle::kind_arm(rules, k) {
            by_arm.entry(arm).or_default().push(k);
        }
    }
    by_arm
        .into_iter()
        .map(|(arm, mut ks)| {
            ks.sort_by_key(|k| k.0);
            ks.dedup();
            (arm, ks[(mix(seed ^ (arm as u64) << 12) % ks.len() as u64) as usize])
        })
        .collect()
}

/// A tile of a varied scene, as the rules' data names it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneTile {
    /// `city`, or the unit kind's role in the scene: `capturer`, `ranged`, `army`.
    pub role: String,
    pub terrain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    pub river: bool,
    pub coastal: bool,
}

/// What a varied siege drew (`BattleRecord::variety`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Variety {
    /// The learner's era (`players().nation_era_of`) and its name in the rules.
    pub era: u8,
    pub era_name: String,
    /// The target is the enemy's city nearest the learner's (`front_city`).
    pub front: bool,
    /// The enemy's cities the target was drawn from.
    pub cities: u32,
    /// The capturer's and the ranged kind's names (ladder rungs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ranged: Option<String>,
    /// The city's tile, then each unit of the learner's army (a skirmish:
    /// the learner's units, then the enemy's, roles `enemy-…`).
    pub tiles: Vec<SceneTile>,
    /// A skirmish: each side's units' kinds by name, the enemy's era.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enemy_kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enemy_era: Option<u8>,
    /// A siege ladder scene: the distance from the target to the nearest
    /// other city (either side's), the units removed to clear the scene.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<u32>,
}

fn scene_tile(rules: &CompiledRuleset, game: &GameState, tile: TileId, role: &str) -> SceneTile {
    let map = game.view().auth.map();
    SceneTile {
        role: role.to_owned(),
        terrain: map.terrain(tile).and_then(|code| rules.terrain_kind_of(code)).and_then(|k| rules.terrains.get(usize::from(k.0))).map_or_else(String::new, |d| d.name.clone()),
        feature: map.feature(tile).flatten().and_then(|f| rules.features.get(f.0 as usize)).map(|d| d.name.clone()),
        river: map.river(tile),
        coastal: map.coastal_land(tile),
    }
}

/// The world a varied battle job is played in (`selfplay::play`): its own
/// when two living nations there have cities on one landmass at the war's
/// round, else the first of `VARY_WORLDS` draws that has — the suite's
/// seed a stride (`SEED_STRIDE`) on and the next map script — so no group
/// fails for want of a war. The job unchanged for other grounds.
pub fn fit_world(rules: &CompiledRuleset, job: &crate::selfplay::Job) -> Result<crate::selfplay::Job, String> {
    let Some(GroundJob::Battle { vary: true, .. }) = job.ground else { return Ok(job.clone()) };
    let scripts: Vec<&str> = kernel::scenario::map_script::MapScript::ALL.iter().map(|s| s.key()).collect();
    let at = scripts.iter().position(|s| *s == job.spec.script).unwrap_or(0);
    let mut last = String::new();
    for k in 0..VARY_WORLDS {
        let mut tried = job.clone();
        tried.spec.seed = job.spec.seed + k * crate::bench::suite::SEED_STRIDE;
        tried.spec.script = scripts[(at + k as usize) % scripts.len()].to_owned();
        let mut game = crate::export::start(rules, &tried.spec.game()?);
        match play_prefix(rules, &mut game, &tried.spec, tried.ground.as_ref()) {
            Ok(_) if shares_land(&game) => return Ok(tried),
            Ok(_) => last = "no two nations on one landmass".to_owned(),
            Err(e) => last = e,
        }
    }
    Err(format!("no world of {VARY_WORLDS} draws has a war to set: {last}"))
}

/// Two living majors have cities on one landmass.
fn shares_land(game: &GameState) -> bool {
    let auth = game.view().auth;
    let map = auth.map();
    let areas: Vec<BTreeMap<u32, ()>> = majors(game)
        .into_iter()
        .filter(|p| auth.players().is_alive(*p))
        .map(|p| auth.indexes().cities_of(p).iter().filter_map(|h| auth.cities().get(*h)).filter_map(|c| map.land_area(c.tile)).map(|a| (a, ())).collect())
        .collect();
    areas.iter().enumerate().any(|(i, a)| areas[i + 1..].iter().any(|b| a.keys().any(|k| b.contains_key(k))))
}

/// The free walls of `walls` a unit of `kind` standing on `from` reaches this
/// turn with a move left to strike, by the rules' pathing (`own_reachable`:
/// terrain, borders, the enemy's zones of control as the player sees them)
/// on `base` (the scene so far).
fn reached_walls(rules: &CompiledRuleset, base: &GameState, side: PlayerId, kind: UnitKindId, from: TileId, walls: &[TileId]) -> Vec<TileId> {
    let mut trial = base.clone();
    let unit = trial.debug_spawn_unit(rules, side, from, kind);
    let view = trial.player_view(rules, side);
    view.own_reachable(unit).into_iter().filter(|(t, left)| *left > 0 && walls.contains(t)).map(|(t, _)| t).collect()
}

/// Free land of the city's landmass on `base` at 2..=`far` tiles from it,
/// nearest first (a varied scene: in a drawn order): a unit's places.
fn spots(rules: &CompiledRuleset, base: &GameState, city: TileId, far: i32, vary: Option<u64>) -> Vec<TileId> {
    let auth = base.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    let mut out: Vec<(i32, TileId)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter_map(|t| Some((dims.distance(t, city).filter(|d| (2..=far).contains(d))?, t)))
        .filter(|&(_, t)| map.land_area(t) == map.land_area(city) && auth.indexes().city_on_tile(t).is_none() && auth.indexes().units_on(t).is_empty() && battle::standable(rules, base, t))
        .collect();
    out.sort();
    let mut tiles: Vec<TileId> = out.into_iter().map(|(_, t)| t).collect();
    if let Some(seed) = vary {
        shuffle(&mut tiles, seed ^ 0x5b07);
    }
    tiles
}

/// The rules' cheapest land military kind that strikes in melee, belongs to
/// no nation, moves more than `than` a turn and whose blow captures the city
/// on `city` (planned from `wall` on a copy of `base`), with its least and
/// greatest blow: the fast capturer of a learner that fields none yet.
fn faster_capturer(rules: &CompiledRuleset, base: &GameState, learner: PlayerId, city: TileId, wall: TileId, than: u16) -> Option<(UnitKindId, u32, u32)> {
    use classic_rules::combat::exchange::{plan_attack, AttackKind};
    let mut kinds: Vec<UnitKindId> = (0..rules.units.len() as u16)
        .map(UnitKindId)
        .filter(|&k| {
            let def = &rules.units[k.0 as usize];
            def.domain == kernel::rules::content::Domain::Land
                && def.nation.is_none()
                && def.movement > than
                && battle::kind_arm(rules, k).is_some()
                && kernel::ai::plan::role_of(rules, k) == kernel::ai::plan::AiRole::Military
                && classic_rules::ai::combat::ranged_range(rules, k).is_none()
        })
        .collect();
    kinds.sort_by_key(|k| (rules.units[k.0 as usize].cost, k.0));
    kinds.into_iter().find_map(|kind| {
        let mut trial = base.clone();
        let unit = trial.debug_spawn_unit(rules, learner, wall, kind);
        let view = trial.view();
        let p = plan_attack(view, rules, unit, city, AttackKind::Melee).ok()?;
        let core = view.auth.units().get(unit).map(|u| *u.core)?;
        let no_capture = kernel::effects::unit::unit_modifiers(view.auth, rules, &core).has(classic_rules::rules::effects::UnitAbility::NoCapture);
        let (least, _, most) = p.preview().to_defender;
        (!no_capture && most > 0).then_some((kind, u32::from(least), u32::from(most)))
    })
}

/// The fewest shooters whose least shots and `blow`'s least take more than
/// `blow`'s greatest (no blow alone takes the city), and those hit points.
fn volley(k: &LadderKinds, blow: (u32, u32), room: usize) -> Result<(usize, u32), String> {
    if k.ranged.is_none() || k.shot_min == 0 {
        return Err("no ranged kind that hurts the city".to_owned());
    }
    let n = ((blow.1 + 1).saturating_sub(blow.0)).div_ceil(k.shot_min).max(1) as usize;
    if n > room {
        return Err(format!("a volley needs {n} shooters, room for {room}"));
    }
    Ok((n, n as u32 * k.shot_min + blow.0))
}

/// A one-turn rung's learner army and its tiles, the city's hit points and
/// the capturer (`Rung::one_turn`); `set` is the scene so far (the war, the
/// garrison), `walls` the free walls (drawn order), `shooting` the tiles a
/// shooter strikes from (the walls first).
#[allow(clippy::too_many_arguments)]
fn one_turn_scene(
    rules: &CompiledRuleset,
    set: &GameState,
    game: &GameState,
    learner: PlayerId,
    city: TileId,
    walls: &[TileId],
    shooting: &[TileId],
    held: &[TileId],
    k: &LadderKinds,
    rung: Rung,
    vary: Option<u64>,
) -> Result<(Vec<UnitKindId>, Vec<TileId>, u32, UnitKindId), String> {
    let dims = game.view().auth.map().dimensions;
    let _ = held;
    // The learner sees the land around the city, as the scene's reveal shows it.
    let mut base = set.clone();
    base.debug_reveal(learner, &around(set, city, 8).into_iter().map(TileId).collect::<Vec<_>>());
    let near = |t: TileId, ws: &[TileId]| ws.iter().filter_map(|w| dims.distance(t, *w)).min().unwrap_or(i32::MAX);
    // A capturer of `kind` on `base` placed `lo..=hi` tiles from the nearest of `free`, reaching one this turn.
    let place = |base: &GameState, kind: UnitKindId, free: &[TileId], lo: u32, hi: u32| -> Option<TileId> {
        spots(rules, base, city, hi as i32 + 1, vary)
            .into_iter()
            .filter(|t| (lo as i32..=hi as i32).contains(&near(*t, free)))
            .take(40)
            .find(|t| !reached_walls(rules, base, learner, kind, *t, free).is_empty())
    };
    let moves = |kind: UnitKindId| rules.units.get(kind.0 as usize).map_or(0, |d| d.movement);
    // The capturers to try, with their least and greatest blow: the drawn (or best) one, the learner's others
    // by moves, then the rules' faster one when the learner has none that covers the distance.
    let mut kinds: Vec<(UnitKindId, u32, u32)> = vec![(k.capturer.0, k.blow_range.0, k.blow_range.1)];
    let mut others: Vec<(UnitKindId, u32, u32)> = k.capturers.iter().copied().filter(|c| c.0 != k.capturer.0).collect();
    others.sort_by_key(|c| (std::cmp::Reverse(moves(c.0)), c.0 .0));
    kinds.extend(others);
    let fastest = kinds.iter().map(|c| moves(c.0)).max().unwrap_or(0);
    let fallback = walls.first().and_then(|w| faster_capturer(rules, &base, learner, city, *w, fastest));
    // The first of `kinds` (then the fallback) placed `lo..=hi` from `free`.
    let place_any = |base: &GameState, free: &[TileId], lo: u32, hi: u32| -> Option<((UnitKindId, u32, u32), TileId)> {
        kinds.iter().chain(fallback.iter()).find_map(|c| place(base, c.0, free, lo, hi).map(|t| (*c, t)))
    };
    let ranged = k.ranged.map(|r| r.0);
    match rung {
        Rung::Reach(lo, hi) => {
            let ((kind, _, _), at) = place_any(&base, walls, lo, hi).ok_or("no tile for a capturer that reaches a wall this turn")?;
            Ok((vec![kind], vec![at], 1, kind))
        }
        Rung::Blocked => {
            let shooter = ranged.ok_or("the learner fields no ranged unit (range > 1)")?;
            // Every free wall held by a shooter; the capturer next to one of them, 2 tiles out.
            let mut full = base.clone();
            let on: Vec<(TileId, UnitHandle)> = walls.iter().map(|w| (*w, full.debug_spawn_unit(rules, learner, *w, shooter))).collect();
            for at in spots(rules, &full, city, 2, vary).into_iter().take(40) {
                for &(w, unit) in on.iter().filter(|(w, _)| dims.distance(at, *w) == Some(1)) {
                    // The shooter on `w` has a tile to step to (not a wall, not the capturer's) …
                    let mut trial = full.clone();
                    trial.debug_spawn_unit(rules, learner, at, k.capturer.0);
                    let view = trial.player_view(rules, learner);
                    let aside = view.own_reachable(unit).into_iter().any(|(t, _)| t != at && t != w && dims.distance(t, city).is_some_and(|d| d >= 2));
                    // … and the capturer reaches `w` once it is free.
                    let mut freed = base.clone();
                    for &(o, _) in on.iter().filter(|(o, _)| *o != w) {
                        freed.debug_spawn_unit(rules, learner, o, shooter);
                    }
                    if aside && !reached_walls(rules, &freed, learner, k.capturer.0, at, &[w]).is_empty() {
                        let mut units = vec![k.capturer.0];
                        units.extend(std::iter::repeat_n(shooter, walls.len()));
                        let mut tiles = vec![at];
                        tiles.extend(walls.iter().copied());
                        return Ok((units, tiles, 1, k.capturer.0));
                    }
                }
            }
            Err("no blocked wall a shooter can leave and the capturer reach".to_owned())
        }
        Rung::Volley(lo, hi) => {
            let shooter = ranged.ok_or("the learner fields no ranged unit (range > 1)")?;
            // One wall kept free for the capturer; the shooters on the far tiles first.
            let keep = *walls.first().ok_or("no free wall")?;
            let mut stands: Vec<TileId> = shooting.iter().copied().filter(|t| !walls.contains(t)).collect();
            stands.extend(walls.iter().copied().filter(|w| *w != keep));
            // The capturer first (its blow sets the volley), on the kept wall or `lo..=hi` from a free one.
            let mut with = base.clone();
            let (capturer, at) = if lo == 0 {
                ((k.capturer.0, k.blow_range.0, k.blow_range.1), keep)
            } else {
                // Placed before the shooters stand: the shooters take the far tiles first and leave `keep` free.
                let mut probe = base.clone();
                for t in stands.iter().filter(|t| !walls.contains(t)) {
                    probe.debug_spawn_unit(rules, learner, *t, shooter);
                }
                place_any(&probe, walls, lo, hi).ok_or("no tile for a capturer that reaches a wall this turn")?
            };
            // The wall the capturer reaches stays free; the shooters take the others.
            let keep = if lo == 0 { keep } else { reached_walls(rules, &base, learner, capturer.0, at, walls).into_iter().next().unwrap_or(keep) };
            let stands: Vec<TileId> = shooting.iter().copied().filter(|t| !walls.contains(t)).chain(walls.iter().copied().filter(|w| *w != keep)).collect();
            let (n, hp) = volley(k, (capturer.1, capturer.2), stands.len())?;
            for t in &stands[..n] {
                with.debug_spawn_unit(rules, learner, *t, shooter);
            }
            let free: Vec<TileId> = walls.iter().copied().filter(|w| !stands[..n].contains(w)).collect();
            if lo > 0 && reached_walls(rules, &with, learner, capturer.0, at, &free).is_empty() {
                return Err("the volley's shooters block the capturer's way".to_owned());
            }
            let k_capturer = capturer.0;
            let mut units = vec![k_capturer];
            units.extend(std::iter::repeat_n(shooter, n));
            let mut tiles = vec![at];
            tiles.extend(stands[..n].iter().copied());
            Ok((units, tiles, hp, k_capturer))
        }
        Rung::Fast { volley: with_volley } => {
            // The fastest capture-capable kind the learner fields; the rules' faster one when all move alike.
            let &(mut fast, mut least, mut most) = k.capturers.iter().max_by_key(|c| (moves(c.0), c.1, c.0 .0)).ok_or("no capturer")?;
            let mut slow = k.capturers.iter().filter(|c| moves(c.0) < moves(fast)).min_by_key(|c| (moves(c.0), c.0 .0)).map(|c| c.0);
            if slow.is_none() {
                if let Some(f) = fallback {
                    slow = Some(fast);
                    (fast, least, most) = f;
                }
            }
            let mut with = base.clone();
            let (mut units, mut tiles, mut hp) = (Vec::new(), Vec::new(), 1);
            let mut free: Vec<TileId> = walls.to_vec();
            if with_volley {
                let shooter = ranged.ok_or("the learner fields no ranged unit (range > 1)")?;
                let keep = *walls.first().ok_or("no free wall")?;
                let mut stands: Vec<TileId> = shooting.iter().copied().filter(|t| !walls.contains(t)).collect();
                stands.extend(walls.iter().copied().filter(|w| *w != keep));
                let (n, h) = volley(k, (least, most), stands.len())?;
                for t in &stands[..n] {
                    with.debug_spawn_unit(rules, learner, *t, shooter);
                    units.push(shooter);
                    tiles.push(*t);
                }
                free.retain(|w| !stands[..n].contains(w));
                hp = h;
            }
            // The fast unit where it reaches a free wall this turn and a slower capturer would not, farthest first.
            let far = 3 * i32::from(moves(fast)).max(1) + 1;
            let mut at = None;
            let mut candidates = spots(rules, &with, city, far, vary);
            candidates.retain(|t| near(*t, &free) >= 2);
            if vary.is_none() {
                candidates.sort_by_key(|t| (-near(*t, &free), t.0));
            }
            for t in candidates.into_iter().take(60) {
                if reached_walls(rules, &with, learner, fast, t, &free).is_empty() {
                    continue;
                }
                if slow.is_some_and(|s| !reached_walls(rules, &with, learner, s, t, &free).is_empty()) {
                    continue;
                }
                at = Some(t);
                break;
            }
            let at = at.ok_or("no tile only the fast capturer reaches a wall from")?;
            with.debug_spawn_unit(rules, learner, at, fast);
            let mut army = vec![(fast, at)];
            // Up to two slower capturers where they reach no wall this turn.
            if let Some(s) = slow {
                for t in spots(rules, &with, city, far, vary).into_iter().filter(|t| near(*t, &free) >= 2).take(40) {
                    if army.len() == 3 {
                        break;
                    }
                    if reached_walls(rules, &with, learner, s, t, &free).is_empty() {
                        with.debug_spawn_unit(rules, learner, t, s);
                        army.push((s, t));
                    }
                }
            }
            let (mut first_units, mut first_tiles): (Vec<UnitKindId>, Vec<TileId>) = army.into_iter().unzip();
            first_units.extend(units);
            first_tiles.extend(tiles);
            Ok((first_units, first_tiles, hp, fast))
        }
        _ => Err("not a one-turn rung".to_owned()),
    }
}

/// A ladder scene's learner army (melee first) and its tiles, the city's
/// damage, the counts at the city's hit points as set.
struct LadderScene {
    /// The capturer's and the ranged kind (the previews' draw).
    capturer: UnitKindId,
    ranged: Option<UnitKindId>,
    units: Vec<UnitKindId>,
    tiles: Vec<TileId>,
    damage: Option<u16>,
    shots: u32,
    blows: u32,
    /// The enemy's units beyond the garrison (`Rung::Clutter`).
    enemy: Vec<(UnitKindId, TileId)>,
    /// A capture in the learner's first turn is possible by construction:
    /// rung 0, a one-turn rung, a ring (cluttered or not) of at least the
    /// shots the previews need (`BattleRecord::same_turn`).
    same_turn: bool,
}

/// The scene of ladder rung `rung` for `learner` against the city on `city`
/// (the garrison `guard` and the knob's damage `damage` set on a copy
/// before the previews).
#[allow(clippy::too_many_arguments)]
fn ladder_scene(rules: &CompiledRuleset, game: &GameState, learner: PlayerId, enemy: PlayerId, kinds: &[UnitKindId], enemy_kinds: &[UnitKindId], city: TileId, guard: &[(UnitKindId, TileId)], damage: Option<u16>, rung: Rung, vary: Option<u64>) -> Result<LadderScene, String> {
    use classic_rules::combat::{exchange::plan_attack_from, AttackKind};
    let held: Vec<TileId> = guard.iter().map(|(_, t)| *t).collect();
    let mut walls = ring_tiles(rules, game, city, 1, &held);
    // A varied scene: the walls taken from any side (the capturers', rung 0's), the far shooters too.
    if let Some(seed) = vary {
        shuffle(&mut walls, seed ^ 0x3a11);
    }
    let &wall = walls.first().ok_or("no free land at the city's walls")?;
    let mut set = game.clone();
    for &(kind, tile) in guard {
        set.debug_spawn_unit(rules, enemy, tile, kind);
    }
    let max = city_hit_points(&set, rules, city) as u32;
    let k = ladder_kinds(rules, &set, learner, kinds, city, wall, vary)?;
    // The shooters' tiles: the walls first, then the farther tiles a shooter strikes the city from (range, line of sight: the rules' plan).
    let mut shooting = walls.clone();
    if let Some((kind, _)) = k.ranged {
        let mut trial = set.clone();
        let shooter = trial.debug_spawn_unit(rules, learner, wall, kind);
        let range = classic_rules::ai::combat::ranged_range(rules, kind).unwrap_or(1);
        for d in 2..=range {
            let mut far: Vec<TileId> = ring_tiles(rules, game, city, d, &held).into_iter().filter(|t| plan_attack_from(trial.view(), rules, shooter, *t, city, AttackKind::Ranged).is_ok()).collect();
            if let Some(seed) = vary {
                shuffle(&mut far, seed ^ 0xfa ^ d as u64);
            }
            shooting.extend(far);
        }
    }
    if rung.one_turn() {
        let (units, tiles, hp, capturer) = one_turn_scene(rules, &set, game, learner, city, &walls, &shooting, &held, &k, rung, vary)?;
        let hp = hp.min(max).max(1);
        let damage = (hp < max).then(|| (max - hp) as u16);
        return Ok(LadderScene { capturer, ranged: k.ranged.map(|r| r.0), units, tiles, damage, shots: k.shots(hp), blows: k.blows(hp), enemy: Vec::new(), same_turn: true });
    }
    let room = shooting.len().saturating_sub(LADDER_CAPTURERS) as u32;
    let full_hp = max.saturating_sub(u32::from(damage.unwrap_or(0)));
    let mut hp = full_hp;
    match rung {
        Rung::Blow => hp = 1,
        // The city fitted to its room: the full ring twice the need.
        _ if k.ranged.is_some() && 2 * k.shots(hp) > room => {
            let (_, shot) = k.ranged.expect("a ranged kind");
            hp = hp.min((room / 2).max(1) * shot + k.capturer.1);
        }
        _ => {}
    }
    // The clutter's hit points: `hp` percent of the way back to the city's own.
    if let Rung::Clutter { hp: up, .. } = rung {
        hp += (full_hp.saturating_sub(hp)) * up.min(100) / 100;
    }
    let damage = (hp < max).then(|| (max - hp) as u16);
    let (shots, blows) = (k.shots(hp), k.blows(hp));
    let ranged = |n: u32| -> Result<Vec<UnitKindId>, String> {
        let (kind, _) = k.ranged.ok_or("the learner fields no ranged unit (range > 1)")?;
        Ok(vec![kind; n.max(1) as usize])
    };
    let capturers = vec![k.capturer.0; LADDER_CAPTURERS];
    let ring = |n: u32| -> Result<(Vec<UnitKindId>, Vec<TileId>), String> {
        let mut units = capturers.clone();
        units.extend(ranged(n)?);
        let mut tiles = shooting.clone();
        tiles.truncate(units.len());
        units.truncate(tiles.len());
        Ok((units, tiles))
    };
    let mut enemy_units: Vec<(UnitKindId, TileId)> = Vec::new();
    let (units, tiles) = match rung {
        Rung::Blow => (vec![k.capturer.0], vec![wall]),
        Rung::Ring { extra } => ring(if extra >= FULL { room } else { (shots as i32 + extra).clamp(1, room.max(1) as i32) as u32 })?,
        Rung::Back { tiles: back } => {
            let mut units = capturers.clone();
            units.extend(ranged(room.max(1))?);
            let from = vary.and_then(|seed| approach(rules, game, learner, city, seed));
            let tiles = staged_tiles(rules, game, learner, city, &units, Back::Tiles(back), &held, from);
            (units, tiles)
        }
        Rung::Melee { extra } => {
            let units = vec![k.capturer.0; (blows as i32 + extra).clamp(1, battle::ARMY_UNITS as i32) as usize];
            let from = vary.and_then(|seed| approach(rules, game, learner, city, seed));
            let tiles = staged_tiles(rules, game, learner, city, &units, Back::Tiles(0), &held, from);
            (units, tiles)
        }
        Rung::Clutter { units: near, reinforce, .. } => {
            let (units, tiles) = ring(room)?;
            // The enemy's units: `near` 2–3 tiles from the city, `reinforce` 4–5 tiles out (they march in).
            let pool: Vec<UnitKindId> = enemy_kinds.iter().copied().filter(|k| battle::kind_arm(rules, *k).is_some()).collect();
            if !pool.is_empty() {
                let mut taken: Vec<TileId> = tiles.clone();
                taken.extend(held.iter().copied());
                for (n, (lo, hi)) in [(near, (2, 3)), (reinforce, (4, 5))] {
                    let mut free: Vec<TileId> = (lo..=hi).flat_map(|d| ring_tiles(rules, game, city, d, &taken)).collect();
                    if let Some(seed) = vary {
                        shuffle(&mut free, seed ^ 0xc1a7 ^ lo as u64);
                    }
                    for (i, t) in free.into_iter().take(n as usize).enumerate() {
                        let kind = pool[(mix(vary.unwrap_or(0) ^ 0xe7 ^ (i as u64) << 8 ^ lo as u64) % pool.len() as u64) as usize];
                        enemy_units.push((kind, t));
                        taken.push(t);
                    }
                }
            }
            (units, tiles)
        }
        Rung::Reach(..) | Rung::Blocked | Rung::Volley(..) | Rung::Fast { .. } => unreachable!("one-turn rungs return above"),
        Rung::Disorder(_) => unreachable!("set_battle passes the rung inside"),
    };
    let shooters = units.iter().filter(|u| Some(**u) == k.ranged.map(|r| r.0)).count() as u32;
    let same_turn = match rung {
        Rung::Blow => true,
        Rung::Ring { .. } | Rung::Clutter { .. } => shooters >= shots && units.contains(&k.capturer.0),
        _ => false,
    };
    Ok(LadderScene { capturer: k.capturer.0, ranged: k.ranged.map(|r| r.0), units, tiles, damage, shots, blows, enemy: enemy_units, same_turn })
}

// ---------------------------------------------------------------------------
// The skirmish ladder: small fights of units in the open

/// The enemy's kinds on a skirmish rung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Foe {
    /// The learner's own: the enemy's units are copies of the learner's
    /// first ones (its melee first): an even match of kinds.
    Same,
    /// The enemy's own kinds: what it builds in the era (`--battle-vary`:
    /// one drawn per arm, else the strongest of each).
    Own,
    /// The strongest the enemy builds of each arm (`battle::best_kinds`).
    Best,
}

/// A rung of the skirmish ladder (`--battle skirmish --battle-rung R`,
/// `BattleRecord::rung`): the learner's and the enemy's units, one of the
/// learner's ranged (`ranged`, when it fields a ranged kind), the enemy's
/// kinds and health (percent of the most; 0: 1 hit point), the start's gap
/// (0: one learner unit in reach of the enemy's, the attack previewed by
/// the rules from its tile; g ≥ 1: the nearest units g tiles apart), the
/// clutter of the land (0 clean: flat open land without a feature or a
/// river; 1 any land out of the cities' strike; 2 any land two tiles or
/// more from a city) and the round limit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkirmishRung {
    pub learner: u32,
    pub enemy: u32,
    pub ranged: bool,
    pub foe: Foe,
    pub enemy_hp: u16,
    pub gap: i32,
    pub clutter: u8,
    pub rounds: u32,
}

#[allow(clippy::too_many_arguments)]
const fn rung(learner: u32, enemy: u32, ranged: bool, foe: Foe, enemy_hp: u16, gap: i32, clutter: u8, rounds: u32) -> SkirmishRung {
    SkirmishRung { learner, enemy, ranged, foe, enemy_hp, gap, clutter, rounds }
}

/// The skirmish ladder (classic/docs/ai-net-data.md, «The skirmish ladder»):
/// the learner's advantage shrinking a step a rung, ordered so that the
/// scripted teacher (`standard` with the scene's strikes, `strike`) wins
/// each rung at most about 10 points less than the one before — a wounded
/// enemy unit to kill alone (1 hit point, 25, 30 %) or with two blows in a
/// turn (50, 65 %), a whole one routed by two, the enemy's own kinds, the
/// armies apart, larger, on rough land, near the cities, the enemy's
/// strongest, one against one, a wounded one to finish alone; the last
/// even, six a side, the difficulty knob scaling the enemy's count (the
/// loop moves it there only).
pub const SKIRMISH: [SkirmishRung; 21] = [
    rung(1, 1, false, Foe::Same, 0, 0, 0, 3),  // a 1-HP unit in reach: one blow kills
    rung(1, 1, false, Foe::Same, 25, 0, 0, 4),
    rung(1, 1, false, Foe::Same, 30, 0, 0, 4),
    rung(2, 1, true, Foe::Same, 50, 0, 0, 5),  // two blows in a turn (one ranged)
    rung(2, 1, true, Foe::Same, 65, 0, 0, 5),
    rung(2, 1, true, Foe::Same, 100, 0, 0, 6),  // whole: routed counts
    rung(3, 2, true, Foe::Own, 100, 0, 0, 8),  // the enemy's own kinds
    rung(2, 1, true, Foe::Same, 100, 2, 0, 7),  // apart
    rung(5, 3, true, Foe::Own, 100, 3, 0, 10),
    rung(6, 4, true, Foe::Own, 100, 3, 0, 12),
    rung(8, 6, true, Foe::Own, 100, 3, 0, 14),
    rung(8, 6, true, Foe::Own, 100, 3, 1, 14),  // rough land
    rung(3, 2, true, Foe::Own, 100, 3, 1, 9),
    rung(8, 6, true, Foe::Own, 100, 3, 2, 14),  // near the cities
    rung(2, 1, true, Foe::Own, 100, 3, 2, 8),
    rung(2, 1, true, Foe::Best, 100, 3, 2, 8),  // the enemy's strongest
    rung(6, 5, true, Foe::Own, 100, 3, 2, 12),
    rung(4, 3, true, Foe::Own, 100, 3, 1, 10),
    rung(1, 1, false, Foe::Same, 100, 0, 0, 8),  // one against one
    rung(1, 1, false, Foe::Same, 35, 0, 0, 4),  // a wounded one to finish alone
    rung(6, 6, true, Foe::Own, 100, 3, 2, 14),  // even, × the knob
];

/// A skirmish without a rung is the last.
pub const SKIRMISH_DEFAULT_RUNG: u32 = SKIRMISH.len() as u32 - 1;

/// A rung of the battle window's advantage ladder (`--battle clash
/// --battle-rung R`): the enemy's health (percent of the most), its count
/// (percent of the learner's, at least 1) and the approach stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClashRung {
    pub enemy_hp: u16,
    pub enemy_count: u16,
    pub stage: u32,
}

/// The battle window's ladder (classic/docs/ai-net-data.md, «The battle
/// window»): in contact the enemy at half the learner's count, its health
/// 40 % up to whole, then its count up to 85 %; then each approach stage
/// (1, 2) at half and three quarters the count; then even at stages 0–2.
/// Ordered by the scripted teacher's share of windows that gain value.
/// Without a rung the window is even at `--battle-stage`.
pub const CLASH: [ClashRung; 12] = [
    ClashRung { enemy_hp: 40, enemy_count: 50, stage: 0 },
    ClashRung { enemy_hp: 70, enemy_count: 50, stage: 0 },
    ClashRung { enemy_hp: 100, enemy_count: 50, stage: 0 },
    ClashRung { enemy_hp: 100, enemy_count: 75, stage: 0 },
    ClashRung { enemy_hp: 100, enemy_count: 85, stage: 0 },
    ClashRung { enemy_hp: 100, enemy_count: 50, stage: 1 },
    ClashRung { enemy_hp: 100, enemy_count: 75, stage: 1 },
    ClashRung { enemy_hp: 100, enemy_count: 50, stage: 2 },
    ClashRung { enemy_hp: 100, enemy_count: 75, stage: 2 },
    ClashRung { enemy_hp: 100, enemy_count: 100, stage: 0 },
    ClashRung { enemy_hp: 100, enemy_count: 100, stage: 1 },
    ClashRung { enemy_hp: 100, enemy_count: 100, stage: 2 },
];

/// A clash's scene as a skirmish rung: `n` and `m` units, its groups apart.
fn rung_clash(n: u32, m: u32, enemy_hp: u16, gap: i32) -> SkirmishRung {
    rung(n, m, false, Foe::Own, enemy_hp, gap.max(1), 1, u32::MAX)
}

/// Free land a skirmish may stand on: the landmass of `area`, no unit or
/// city on it; by `clutter` (`SkirmishRung::clutter`): 0 flat open land (the
/// least enter cost on the map, no feature, no river) out of every city's
/// strike, 1 any land out of the strike, 2 any land two tiles or more from a
/// city. Out of the strike falls back to two tiles when the land has too
/// little room, clean land to any land.
fn open_land(rules: &CompiledRuleset, game: &GameState, area: Option<u32>, need: usize, clutter: u8) -> Vec<TileId> {
    let auth = game.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    let cities: Vec<TileId> = (0..auth.players().count() as u8)
        .flat_map(|p| auth.indexes().cities_of(PlayerId(p)).to_vec())
        .filter_map(|h| auth.cities().get(h).map(|c| c.tile))
        .collect();
    let free: Vec<(i32, TileId)> = (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|&t| map.land_area(t) == area && auth.indexes().units_on(t).is_empty() && auth.indexes().city_on_tile(t).is_none() && battle::standable(rules, game, t))
        .map(|t| (cities.iter().filter_map(|c| dims.distance(t, *c)).min().unwrap_or(i32::MAX), t))
        .collect();
    let flat = free.iter().filter_map(|(_, t)| kernel::navigation::cost::enter_cost_on_map(map, *t)).min();
    let clean = |t: TileId| map.feature(t).flatten().is_none() && !map.river(t) && kernel::navigation::cost::enter_cost_on_map(map, t) == flat;
    let strike = classic_rules::combat::city::CITY_ATTACK_RANGE;
    let pick = |far: i32, clean_only: bool| -> Vec<TileId> { free.iter().filter(|(d, t)| *d >= far && (!clean_only || clean(*t))).map(|(_, t)| *t).collect() };
    let tries: &[(i32, bool)] = match clutter {
        0 => &[(strike + 1, true), (2, true), (strike + 1, false), (2, false)],
        1 => &[(strike + 1, false), (2, false)],
        _ => &[(2, false)],
    };
    let mut last = Vec::new();
    for &(far, clean_only) in tries {
        last = pick(far, clean_only);
        if last.len() >= need {
            break;
        }
    }
    last
}

/// A skirmish's scene: each side's units (melee first) and tiles, the
/// enemy's health.
struct SkirmishScene {
    learner: (Vec<UnitKindId>, Vec<TileId>),
    enemy: (Vec<UnitKindId>, Vec<TileId>),
    enemy_hp: Option<u16>,
}

/// The attack `kind` of `side` would make from `from` on the unit on
/// `target` in `game` (spawned on a copy): its expected damage, None when
/// the rules refuse it (out of reach, no line of sight, not ready).
fn previewed_blow(rules: &CompiledRuleset, game: &GameState, side: PlayerId, kind: UnitKindId, from: TileId, target: TileId) -> Option<u16> {
    use classic_rules::combat::damage::Roll;
    use classic_rules::combat::exchange::{plan_attack, AttackKind};
    let mut trial = game.clone();
    let unit = trial.debug_spawn_unit(rules, side, from, kind);
    let how = if classic_rules::ai::combat::ranged_range(rules, kind).is_some() { AttackKind::Ranged } else { AttackKind::Melee };
    plan_attack(trial.view(), rules, unit, target, how).ok().map(|p| p.damage_to_defender(Roll::Expected)).filter(|d| *d > 0)
}

/// The enemy's hit points on a rung (`SkirmishRung::enemy_hp`; None: whole).
fn enemy_hit_points(pct: u16) -> Option<u16> {
    let max = kernel::world::unit::UNIT_MAX_HP;
    match pct {
        0 => Some(1),
        p if p >= 100 => None,
        p => Some((u32::from(max) * u32::from(p) / 100).max(1) as u16),
    }
}

/// The scene of skirmish rung `step` between `learner` and `enemy`, their
/// units planned (`learner_units`, `enemy_units`, melee first): the open
/// land of the landmass the two nations share (`open_land` by the rung's
/// clutter), the enemy's anchor on the middle line between their front
/// cities (`vary`: drawn), the learner's on its side (`vary`: any side).
/// Gap 0 with one unit a side: the learner's unit on a tile in its reach
/// from which the rules' preview plans its attack this turn. Else the
/// enemy's units packed around its anchor and the learner's on the land at
/// `max(gap, 1)` tiles or more from every enemy unit, its front at the gap.
fn skirmish_scene(rules: &CompiledRuleset, game: &GameState, learner: PlayerId, enemy: PlayerId, step: SkirmishRung, learner_units: Vec<UnitKindId>, enemy_units: Vec<UnitKindId>, vary: Option<u64>) -> Result<SkirmishScene, String> {
    let own = battle::front_city(game, enemy, learner).ok_or("the learner has no city on the enemy's land")?;
    let far = battle::front_city(game, learner, enemy).ok_or("the enemy has no city on the learner's land")?;
    let map = game.view().auth.map();
    let dims = map.dimensions;
    let need = learner_units.len() + enemy_units.len();
    let land = open_land(rules, game, map.land_area(far), need, step.clutter);
    // The enemy's anchors in the order they are tried.
    let mut centres = land.clone();
    match vary {
        Some(seed) => shuffle(&mut centres, seed ^ 0xce17),
        None => centres.sort_by_key(|&t| {
            let (a, b) = (dims.distance(t, own).unwrap_or(0), dims.distance(t, far).unwrap_or(0));
            ((a - b).abs(), a + b, t.0)
        }),
    }
    let enemy_hp = enemy_hit_points(step.enemy_hp);
    let dist = |a: TileId, b: TileId| dims.distance(a, b).unwrap_or(i32::MAX);
    if step.gap == 0 && learner_units.len() == 1 && enemy_units.len() == 1 {
        // In reach: the learner's attack previewed from its tile.
        let (lk, ek) = (learner_units[0], enemy_units[0]);
        let r = reach(rules, lk);
        for &centre in centres.iter().take(64) {
            let mut set = game.clone();
            set.debug_spawn_unit(rules, enemy, centre, ek);
            let mut froms: Vec<TileId> = land.iter().copied().filter(|&t| t != centre && (1..=r).contains(&dist(t, centre))).collect();
            match vary {
                Some(seed) => shuffle(&mut froms, seed ^ u64::from(centre.0) ^ 0xf0),
                // Its full reach first, then nearest its own city.
                None => froms.sort_by_key(|&t| (-dist(t, centre), dist(t, own), t.0)),
            }
            if let Some(from) = froms.into_iter().take(12).find(|&from| previewed_blow(rules, &set, learner, lk, from, centre).is_some()) {
                return Ok(SkirmishScene { learner: (vec![lk], vec![from]), enemy: (vec![ek], vec![centre]), enemy_hp });
            }
        }
        return Err("no tile in reach of the enemy from which the learner's unit can attack".to_owned());
    }
    // Two groups: the learner's front at the gap from the enemy's nearest unit.
    let g = step.gap.max(1);
    let apart = g + 2;
    for (i, &centre) in centres.iter().enumerate().take(64) {
        let mut sides: Vec<TileId> = land.iter().copied().filter(|&t| dist(t, centre) == apart).collect();
        match vary {
            Some(seed) => shuffle(&mut sides, seed ^ (i as u64) << 24 ^ 0x51de),
            None => sides.sort_by_key(|&t| (dist(t, own), t.0)),
        }
        for &anchor in sides.iter().take(6) {
            let mut theirs: Vec<TileId> = land.iter().copied().filter(|t| dist(*t, centre) < dist(*t, anchor)).collect();
            theirs.sort_by_key(|&t| (dist(t, centre), dist(t, anchor), t.0));
            theirs.truncate(enemy_units.len());
            let near = |t: TileId| theirs.iter().map(|e| dist(t, *e)).min().unwrap_or(i32::MAX);
            let mut mine: Vec<TileId> = land.iter().copied().filter(|&t| !theirs.contains(&t) && near(t) >= g).collect();
            mine.sort_by_key(|&t| (near(t), dist(t, anchor), t.0));
            mine.truncate(learner_units.len());
            if theirs.len() == enemy_units.len() && mine.len() == learner_units.len() {
                return Ok(SkirmishScene { learner: (learner_units, mine), enemy: (enemy_units, theirs), enemy_hp });
            }
        }
    }
    Err(format!("no open land holds {} and {} units {g} tiles apart", learner_units.len(), enemy_units.len()))
}

/// A side's units for a skirmish rung: `n` slots by the kind's arm weights
/// from `kinds` (melee first); with `ranged`, the second slot a ranged kind
/// when the side fields one.
fn skirmish_units(kinds: &BTreeMap<Arm, UnitKindId>, weights: [u32; 4], n: u32, ranged: bool, seed: u64) -> Result<Vec<UnitKindId>, String> {
    let mut units = plan(kinds, weights, n, seed)?;
    if ranged && n >= 2 {
        if let Some(&shooter) = kinds.get(&Arm::Ranged) {
            if !units.contains(&shooter) {
                // The first slot stays the melee one; the last of the plan becomes the shooter.
                let last = units.len() - 1;
                units[last] = shooter;
            }
            if let Some(&melee) = kinds.get(&Arm::Melee) {
                units[0] = melee;
            }
        }
    }
    Ok(units)
}

/// A ladder scene's target among `enemy`'s cities on the learner's land
/// with a free wall: the most isolated (the farthest from any other city,
/// either side's), drawn with `vary` among those at least `CLEAN_RADIUS`
/// from every other city (else the most isolated); `strike`: one whose
/// nearest other city is the enemy's and strikes its walls (within the
/// city strike range + 1), drawn among those (else the clean choice).
/// With the target its isolation and the candidates' count.
fn ladder_target(rules: &CompiledRuleset, game: &GameState, learner: PlayerId, enemy: PlayerId, strike: bool, vary: Option<u64>) -> Option<(TileId, i32, u32)> {
    let auth = game.view().auth;
    let map = auth.map();
    let dims = map.dimensions;
    let lands: Vec<Option<u32>> = auth.indexes().cities_of(learner).iter().filter_map(|h| auth.cities().get(*h)).map(|c| map.land_area(c.tile)).collect();
    let all: Vec<(TileId, PlayerId)> = (0..auth.players().count() as u8).flat_map(|p| auth.indexes().cities_of(PlayerId(p)).to_vec()).filter_map(|h| auth.cities().get(h)).map(|c| (c.tile, c.owner)).collect();
    // (isolation, the nearest other city is the enemy's and within strike of the walls, tile).
    let mut cands: Vec<(i32, bool, TileId)> = all
        .iter()
        .filter(|(t, o)| *o == enemy && lands.contains(&map.land_area(*t)) && !ring_tiles(rules, game, *t, 1, &[]).is_empty())
        .map(|&(t, _)| {
            let nearest = all.iter().filter(|(u, _)| *u != t).filter_map(|(u, o)| Some((dims.distance(t, *u)?, *o))).min_by_key(|(d, _)| *d);
            let isolation = nearest.map_or(i32::MAX, |(d, _)| d);
            let striking = nearest.is_some_and(|(d, o)| o == enemy && d <= classic_rules::combat::city::CITY_ATTACK_RANGE + 1);
            (isolation, striking, t)
        })
        .collect();
    cands.sort_by_key(|&(iso, _, t)| (std::cmp::Reverse(iso), t.0));
    let n = cands.len() as u32;
    let pool: Vec<(i32, bool, TileId)> = if strike && cands.iter().any(|c| c.1) {
        cands.iter().copied().filter(|c| c.1).collect()
    } else {
        let clean: Vec<_> = cands.iter().copied().filter(|c| c.0 >= CLEAN_RADIUS).collect();
        if clean.is_empty() { cands.iter().copied().take(1).collect() } else { clean }
    };
    let pick = match vary {
        Some(v) if !pool.is_empty() => pool[(mix(v ^ 0xc17) % pool.len() as u64) as usize],
        _ => *pool.first()?,
    };
    Some((pick.2, pick.0, n))
}

/// A clean ladder scene's target stands at least this far from any other
/// city: no other city's strike (its range) reaches the tiles two out from
/// the walls, where the ring and the approach stand.
const CLEAN_RADIUS: i32 = classic_rules::combat::city::CITY_ATTACK_RANGE + 3;

/// The two nations that go to war: the living majors' pairs with cities on
/// one landmass, nearest cities first; the first pair whose war the rules
/// accept (`battle::declare_war`, its edits recorded).
fn go_to_war(rules: &CompiledRuleset, game: &mut GameState, rec: &mut Recorder, position: u64) -> Result<[PlayerId; 2], String> {
    let mut pairs: Vec<(i32, PlayerId, PlayerId)> = Vec::new();
    {
        let auth = game.view().auth;
        let map = auth.map();
        let alive: Vec<PlayerId> = majors(game).into_iter().filter(|p| auth.players().is_alive(*p)).collect();
        let cities = |p: PlayerId| -> Vec<TileId> { auth.indexes().cities_of(p).iter().filter_map(|h| auth.cities().get(*h)).map(|c| c.tile).collect() };
        for (i, &a) in alive.iter().enumerate() {
            for &b in &alive[i + 1..] {
                let near = cities(a)
                    .iter()
                    .flat_map(|ta| cities(b).into_iter().map(move |tb| (*ta, tb)))
                    .filter(|(ta, tb)| map.land_area(*ta).is_some() && map.land_area(*ta) == map.land_area(*tb))
                    .filter_map(|(ta, tb)| map.dimensions.distance(ta, tb))
                    .min();
                if let Some(d) = near {
                    pairs.push((d, a, b));
                }
            }
        }
    }
    pairs.sort();
    for (_, a, b) in pairs {
        rec.edit(game, rules, position, Op::Meet { a: a.0, b: b.0 });
        rec.edit(game, rules, position, Op::War { a: a.0, b: b.0 });
        if game.view().auth.diplomacy().at_war(a, b) {
            return Ok([a, b]);
        }
    }
    Err("no two nations on one landmass could go to war".to_owned())
}

/// Sets the scene of a battle job on the world at the war's round (after
/// `play_prefix`): the war, the armies, the gold, the sight; every change
/// recorded at `position` (the prefix's commands).
pub fn set_battle(rules: &CompiledRuleset, game: &mut GameState, ground: &GroundJob, kinds: &BTreeMap<PlayerId, Vec<UnitKindId>>, position: u64) -> Result<BattleScene, String> {
    let &GroundJob::Battle { kind, start, length, army, seed, difficulty, stage, rung, vary } = ground else { return Err("not a battle".to_owned()) };
    if vary && !matches!(kind, BattleKind::Siege | BattleKind::Skirmish | BattleKind::Clash) {
        return Err("a varied battle is a siege, a skirmish or a clash".to_owned());
    }
    // A varied scene's draws (None: the documented scene).
    let vary = vary.then(|| mix(seed ^ 0x7a71e7));
    let mut variety: Option<Variety> = None;
    // A siege ladder's rung: one below today's stages, or today's stage `rung − LADDER.len()`.
    // A skirmish's rung (none: `SKIRMISH_DEFAULT_RUNG`); it has no stages.
    let skirmish = match kind {
        BattleKind::Skirmish if stage.is_some() => return Err("a skirmish has rungs, no stages".to_owned()),
        BattleKind::Skirmish => {
            let r = rung.unwrap_or(SKIRMISH_DEFAULT_RUNG);
            Some((r, *SKIRMISH.get(r as usize).ok_or_else(|| format!("the skirmish ladder has rungs 0-{}", SKIRMISH.len() - 1))?))
        }
        _ => None,
    };
    // A clash's advantage rung (none: even, at `stage`); it sets the stage.
    let clash_rung = match (kind, rung) {
        (BattleKind::Clash, Some(_)) if stage.is_some() => return Err("a clash's rung sets its stage: --battle-rung or --battle-stage".to_owned()),
        (BattleKind::Clash, Some(r)) => Some(*CLASH.get(r as usize).ok_or_else(|| format!("the battle window's ladder has rungs 0-{}", CLASH.len() - 1))?),
        _ => None,
    };
    let (ladder, stage) = match rung {
        _ if skirmish.is_some() => (None, None),
        _ if kind == BattleKind::Clash => (None, Some(clash_rung.map_or(stage.unwrap_or(0), |c| c.stage))),
        Some(_) if kind != BattleKind::Siege => return Err("a ladder rung is a siege's or a skirmish's".to_owned()),
        Some(r) => match LADDER.get(r as usize) {
            Some(step) => (Some(*step), None),
            None => (None, Some(r - LADDER.len() as u32)),
        },
        None => (None, stage),
    };
    // The spawn order of a ladder scene (`SpawnOrder`): a `Disorder` rung or `--battle-spawn-order random`
    // draws it, else shooters first; the rung inside a `Disorder` sets the scene.
    let (ladder, spawn_order) = match ladder {
        Some(Rung::Disorder(inner)) => (Some(*inner), Some(SpawnOrder::draw(seed))),
        Some(step) if RANDOM_SPAWN_ORDER.load(std::sync::atomic::Ordering::Relaxed) => (Some(step), Some(SpawnOrder::draw(seed))),
        other => (other, None),
    };
    let mut need = None;
    // A ladder scene's capturer kind (its units act last, `set_battle`'s spawn order).
    let mut ladder_capturer: Option<UnitKindId> = None;
    let mut ladder_same_turn: Option<bool> = None;
    let d = f64::from(difficulty) / 1000.0;
    let mut rec = Recorder::default();
    let sides = go_to_war(rules, game, &mut rec, position)?;
    let (learner, enemy) = if mix(seed ^ 7) & 1 == 0 { (sides[0], sides[1]) } else { (sides[1], sides[0]) };
    // A ladder rung's learner is the side whose turn comes first this round:
    // the city neither heals nor strikes before the learner's first move.
    let clash = kind == BattleKind::Clash;
    let (learner, enemy) = match (ladder, skirmish) {
        _ if clash => {
            let auth = game.view().auth;
            let (active, n) = (u32::from(auth.active_player().0), auth.players().count().max(1) as u32);
            let after = |p: PlayerId| (u32::from(p.0) + n - active) % n;
            if after(sides[0]) < after(sides[1]) { (sides[0], sides[1]) } else { (sides[1], sides[0]) }
        }
        (Some(_), _) | (_, Some(_)) => {
            let auth = game.view().auth;
            let (active, n) = (u32::from(auth.active_player().0), auth.players().count().max(1) as u32);
            let after = |p: PlayerId| (u32::from(p.0) + n - active) % n;
            if after(sides[0]) < after(sides[1]) { (sides[0], sides[1]) } else { (sides[1], sides[0]) }
        }
        (None, None) => (learner, enemy),
    };
    // A skirmish's round limit is its rung's.
    let length = skirmish.map_or(length, |(_, step)| length.min(step.rounds));
    // A clash's window follows its approach: `length` rounds after `stage` rounds of march.
    let length = if clash { length + stage.unwrap_or(0) } else { length };
    let empty = Vec::new();
    let kinds_of = |p: PlayerId| kinds.get(&p).unwrap_or(&empty);
    let best = |p: PlayerId| battle::best_kinds(rules, kinds_of(p));
    let garrison = (army / 3).max(if kind == BattleKind::RushDefence { 1 } else { 2 });
    // The attackers' health of a defence below knob 1 (None: whole), a siege's damaged city.
    let mut attacker_hp = None;
    let mut city_damage = None;
    let mut hooked: Vec<Op> = Vec::new();
    let weights = kind.arm_weights();
    // (side, units, tiles) to set down, the target, the scripted side's aim.
    let (target, aims, sets): (Option<TileId>, Vec<(PlayerId, TileId)>, [(PlayerId, Vec<UnitKindId>, Vec<TileId>); 2]) = match kind {
        BattleKind::Field => {
            let own = battle::front_city(game, enemy, learner).ok_or("the learner has no city on the enemy's land")?;
            let far = battle::front_city(game, learner, enemy).ok_or("the enemy has no city on the learner's land")?;
            // Equal armies: the kinds both sides can build, the same plan.
            let common: Vec<UnitKindId> = kinds_of(learner).iter().copied().filter(|k| kinds_of(enemy).contains(k)).collect();
            // The same plan; the enemy's army the knob times the learner's (its first slots the learner's).
            let units = plan(&battle::best_kinds(rules, &common), weights, army, seed)?;
            let others = plan(&battle::best_kinds(rules, &common), weights, scaled(army, d), seed)?;
            // A start stage: the gap between the armies 1 + 2 s × the march (each half s rounds back from contact).
            let margin = stage.map_or(FIELD_MARGIN, |s| 1 + 2 * s as i32 * march_step(rules, &units));
            let mine = field_tiles(rules, game, own, far, units.len(), margin, stage.is_some());
            let theirs = field_tiles(rules, game, far, own, others.len(), margin, stage.is_some());
            let units = units[..units.len().min(mine.len())].to_vec();
            let others = others[..others.len().min(theirs.len())].to_vec();
            (None, vec![(enemy, own)], [(learner, units, mine), (enemy, others, theirs)])
        }
        BattleKind::Skirmish | BattleKind::Clash => {
            let own = battle::front_city(game, enemy, learner).ok_or("the learner has no city on the enemy's land")?;
            let far = battle::front_city(game, learner, enemy).ok_or("the enemy has no city on the learner's land")?;
            // Each side's kinds: the strongest of each arm it fields, or (varied) one drawn per arm.
            let fielded = |p: PlayerId, salt: u64| match vary {
                None => best(p),
                Some(v) => drawn_kinds(rules, kinds_of(p), v ^ salt),
            };
            // The rung's units (the knob scales the enemy on the skirmish's last rung, always in a clash).
            let (step, n, m) = match skirmish {
                Some((r, step)) => (step, step.learner, if r as usize + 1 == SKIRMISH.len() { scaled(step.enemy, d) } else { step.enemy }),
                None => {
                    let (hp, count) = clash_rung.map_or((100, 100), |c| (c.enemy_hp, c.enemy_count));
                    let m = scaled(((f64::from(army) * f64::from(count) / 100.0).round() as u32).max(1), d);
                    (rung_clash(army, m, hp, 1), army, m)
                }
            };
            let units = skirmish_units(&fielded(learner, 0x1ea), weights, n, step.ranged, seed ^ 0x51)?;
            let others = match step.foe {
                Foe::Same => (0..m as usize).map(|i| units[i % units.len()]).collect(),
                Foe::Own => plan(&fielded(enemy, 0xe1e), weights, m, seed ^ 0xe5)?,
                Foe::Best => plan(&best(enemy), weights, m, seed ^ 0xe5)?,
            };
            // A clash's gap by its approach stage.
            let step = if clash { SkirmishRung { gap: clash_gap(rules, stage.unwrap_or(0), &units, &others), ..step } } else { step };
            let s = skirmish_scene(rules, game, learner, enemy, step, units, others, vary)?;
            attacker_hp = s.enemy_hp;
            if vary.is_some() {
                let auth = game.view().auth;
                let era = auth.players().nation_era_of(learner, rules);
                let name = |ks: &[UnitKindId]| ks.iter().filter_map(|k| rules.units.get(k.0 as usize)).map(|d| d.name.clone()).collect();
                variety = Some(Variety {
                    era,
                    era_name: rules.eras.iter().find(|e| e.index == era).map_or_else(String::new, |e| e.name.clone()),
                    kinds: name(&s.learner.0),
                    enemy_kinds: name(&s.enemy.0),
                    enemy_era: Some(auth.players().nation_era_of(enemy, rules)),
                    ..Variety::default()
                });
            }
            let _ = far;
            (None, vec![(enemy, own)], [(learner, s.learner.0, s.learner.1), (enemy, s.enemy.0, s.enemy.1)])
        }
        BattleKind::Siege => {
            let front = battle::front_city(game, learner, enemy).ok_or("the enemy has no city on the learner's land")?;
            // A ladder scene is cleared: every unit of both sides goes (the scene's are spawned below).
            let mut removed = 0u32;
            if ladder.is_some() {
                let gone: Vec<(u32, u64)> = {
                    let auth = game.view().auth;
                    let mut v: Vec<(u32, u64)> = auth.units().iter_cores().filter(|u| u.owner == learner || u.owner == enemy).map(|u| (u.tile.0, u.id.0)).collect();
                    v.sort();
                    v
                };
                for (tile, id) in gone {
                    rec.edit(game, rules, position, Op::Remove { tile, id });
                    removed += 1;
                }
            }
            let picked = ladder.and_then(|step| ladder_target(rules, game, learner, enemy, matches!(step, Rung::Clutter { strike: true, .. }), vary));
            let city = match (picked, vary) {
                (Some((city, isolation, cities)), _) => {
                    let auth = game.view().auth;
                    let era = auth.players().nation_era_of(learner, rules);
                    variety = vary.map(|_| Variety {
                        era,
                        era_name: rules.eras.iter().find(|e| e.index == era).map_or_else(String::new, |e| e.name.clone()),
                        front: city == front,
                        cities,
                        isolation: Some(isolation),
                        removed: Some(removed),
                        ..Variety::default()
                    });
                    city
                }
                (None, None) => front,
                (None, Some(v)) => {
                    // Any city of the enemy on the learner's land (with a free wall on a ladder rung).
                    let auth = game.view().auth;
                    let map = auth.map();
                    let lands: Vec<Option<u32>> = auth.indexes().cities_of(learner).iter().filter_map(|h| auth.cities().get(*h)).map(|c| map.land_area(c.tile)).collect();
                    let mut cities: Vec<TileId> = auth
                        .indexes()
                        .cities_of(enemy)
                        .iter()
                        .filter_map(|h| auth.cities().get(*h))
                        .map(|c| c.tile)
                        .filter(|t| lands.contains(&map.land_area(*t)) && (ladder.is_none() || !ring_tiles(rules, game, *t, 1, &[]).is_empty()))
                        .collect();
                    cities.sort_by_key(|t| t.0);
                    let city = if cities.is_empty() { front } else { cities[(mix(v ^ 0xc17) % cities.len() as u64) as usize] };
                    let era = auth.players().nation_era_of(learner, rules);
                    variety = Some(Variety {
                        era,
                        era_name: rules.eras.iter().find(|e| e.index == era).map_or_else(String::new, |e| e.name.clone()),
                        front: city == front,
                        cities: cities.len() as u32,
                        ..Variety::default()
                    });
                    city
                }
            };
            let units = match vary {
                None => plan(&best(learner), weights, army, seed)?,
                Some(v) => plan(&drawn_kinds(rules, kinds_of(learner), v), weights, army, seed)?,
            };
            let guard = plan(&best(enemy), GARRISON_WEIGHTS, scaled(garrison, d), seed ^ 0x9a)?;
            if d < 1.0 {
                city_damage = Some((city, (city_hit_points(game, rules, city) * 0.75 * (1.0 - d)).round() as u16));
            }
            if let Some(step) = ladder {
                // A ladder rung: the garrison's first unit in the city (none when a unit holds it), the walls the learner's.
                let held: Vec<TileId> = garrison_tiles(rules, game, city, 1).into_iter().filter(|t| *t == city).collect();
                let guard: Vec<UnitKindId> = guard.into_iter().take(held.len()).collect();
                let placed: Vec<(UnitKindId, TileId)> = guard.iter().copied().zip(held.iter().copied()).collect();
                let s = ladder_scene(rules, game, learner, enemy, kinds_of(learner), kinds_of(enemy), city, &placed, city_damage.map(|(_, x)| x), step, vary)?;
                city_damage = s.damage.map(|x| (city, x));
                need = Some((s.shots, s.blows));
                ladder_capturer = Some(s.capturer);
                ladder_same_turn = Some(s.same_turn);
                if let Some(v) = variety.as_mut() {
                    v.capturer = rules.units.get(s.capturer.0 as usize).map(|d| d.name.clone());
                    v.ranged = s.ranged.and_then(|r| rules.units.get(r.0 as usize)).map(|d| d.name.clone());
                }
                hooked = siege_hook(rules, game, learner, enemy, city, d);
                // The clutter's enemy units join the garrison's side.
                let (mut guard, mut held) = (guard, held);
                for (kind, tile) in s.enemy {
                    guard.push(kind);
                    held.push(tile);
                }
                (Some(city), vec![(learner, city)], [(learner, s.units, s.tiles), (enemy, guard, held)])
            } else {
            let held = garrison_tiles(rules, game, city, guard.len());
            let front: Vec<TileId> = match stage {
                Some(s) => staged_tiles(rules, game, learner, city, &units, Back::Rounds(s), &held, vary.and_then(|v| approach(rules, game, learner, city, v))),
                None => battle::front_tiles(rules, game, learner, city, units.len()).into_iter().map(|(_, t)| t).collect(),
            };
            hooked = siege_hook(rules, game, learner, enemy, city, d);
            (Some(city), vec![(learner, city)], [(learner, units, front), (enemy, guard, held)])
            }
        }
        BattleKind::Defence | BattleKind::RushDefence => {
            let city = match kind {
                BattleKind::RushDefence => {
                    let auth = game.view().auth;
                    kernel::world::city::capital_of(auth, rules, learner).and_then(|h| auth.cities().get(h)).map(|c| c.tile).or_else(|| battle::front_city(game, enemy, learner))
                }
                _ => battle::front_city(game, enemy, learner),
            }
            .ok_or("the learner has no city on the enemy's land")?;
            let units = if kind == BattleKind::RushDefence {
                rush_army(rules, game.view().auth.players().nation_of(enemy), d.max(0.5))
            } else {
                plan(&best(enemy), weights, scaled(army, d.max(0.5)), seed)?
            };
            if d < 1.0 {
                attacker_hp = Some((f64::from(kernel::world::unit::UNIT_MAX_HP) * d.max(0.3)).round() as u16);
            }
            if d > 1.0 {
                // The attack outgrows its room, and the scripted side's attacks are weak (a city's strike
                // and its garrison stop a rush of 36: classic/docs/ai-books.md): the city starts worn.
                city_damage = Some((city, (city_hit_points(game, rules, city) * HELD_CITY_DAMAGE * (1.0 - 1.0 / d)).round() as u16));
            }
            let guard = plan(&best(learner), GARRISON_WEIGHTS, garrison, seed ^ 0x9a)?;
            let held = garrison_tiles(rules, game, city, guard.len());
            let front: Vec<TileId> = match stage {
                Some(s) => staged_tiles(rules, game, enemy, city, &units, Back::Rounds(s), &held, None),
                None => battle::front_tiles(rules, game, enemy, city, units.len()).into_iter().map(|(_, t)| t).collect(),
            };
            (Some(city), vec![(enemy, city)], [(learner, guard, held), (enemy, units, front)])
        }
    };
    // The attackers' nearest distance to the target (field: between the armies), with a stage.
    // A ladder scene's units are spawned shooters first, the capturer kind last: a player's units decide
    // in the order of their ids (`kernel::ai::driver`, one pass, a unit that does not act is done for the
    // turn), so a capturer spawned first would decide before the volley and could not strike after it.
    let mut sets = sets;
    if let Some(capturer) = ladder_capturer {
        let (_, units, tiles) = &mut sets[0];
        let mut both: Vec<(UnitKindId, TileId)> = units.iter().copied().zip(tiles.iter().copied()).collect();
        let rank = |k: &UnitKindId| if *k == capturer { 2 } else if classic_rules::ai::combat::ranged_range(rules, *k).is_some_and(|r| r > 1) { 0 } else { 1 };
        match spawn_order.unwrap_or(SpawnOrder::ShootersFirst) {
            SpawnOrder::ShootersFirst => both.sort_by_key(|(k, _)| rank(k)),
            SpawnOrder::CapturersFirst => both.sort_by_key(|(k, _)| 2 - rank(k)),
            SpawnOrder::Shuffled => shuffle(&mut both, mix(seed ^ 0x5b0e_5f)),
        }
        let rest: Vec<TileId> = tiles.iter().copied().skip(both.len()).collect();
        *units = both.iter().map(|(k, _)| *k).collect();
        *tiles = both.iter().map(|(_, t)| *t).chain(rest).collect();
    }
    let start_gap = stage.or(ladder.map(|_| 0)).or(skirmish.map(|_| 0)).and_then(|_| {
        let placed = |i: usize| sets[i].2[..sets[i].1.len().min(sets[i].2.len())].to_vec();
        match target {
            Some(t) => gap(game, &placed(usize::from(kind.learner_holds())), &[t]),
            None => gap(game, &placed(0), &placed(1)),
        }
    });
    let mut armies: [(PlayerId, Vec<(UnitHandle, UnitKindId)>); 2] = [(learner, Vec::new()), (enemy, Vec::new())];
    for (i, (side, units, tiles)) in sets.iter().enumerate() {
        // A big army (a high knob) is cut to the room it has: the knob saturates there.
        // A ladder's garrison may be none (a unit of the city's owner holds it).
        if ladder.is_some() && *side == enemy && units.is_empty() {
            continue;
        }
        if tiles.is_empty() ||(tiles.len() * 2 < units.len() && tiles.len() < MIN_ROOM) {
            return Err(format!("player {} has room for {} of {} units", side.0, tiles.len(), units.len()));
        }
        let hp = if *side == enemy { attacker_hp } else { None };
        for (kind, tile) in units.iter().zip(tiles) {
            let unit = rec.edit(game, rules, position, Op::Spawn { player: side.0, tile: tile.0, kind: kind.0, hp }).expect("a spawned unit");
            armies[i].1.push((unit, *kind));
        }
    }
    if let Some((city, damage)) = city_damage {
        rec.edit(game, rules, position, Op::Damage { tile: city.0, damage });
    }
    for op in hooked {
        rec.edit(game, rules, position, op);
    }
    for side in [learner, enemy] {
        // The chest pays the upkeep of the spawned army. A ladder rung's garrison is a unit at most, and
        // the enemy's chest bought its city walls in its first turn (the "exact" rungs were not):
        // there the learner's side alone gets one.
        if ladder.is_some() && side == enemy {
            continue;
        }
        rec.edit(game, rules, position, Op::Earn { player: side.0, gold: battle::WAR_CHEST });
    }
    // Each side is shown the land between its units and what it fights over
    // (the AI's paths go over known land only).
    for (side, units) in &armies {
        let goal = match (kind, target) {
            (_, Some(t)) => t,
            (_, None) => {
                let other = if *side == learner { enemy } else { learner };
                battle::front_city(game, *side, other).unwrap_or(TileId(0))
            }
        };
        let dims = game.view().auth.map().dimensions;
        let auth = game.view().auth;
        let reach = units.iter().filter_map(|(h, _)| auth.units().get(*h)).filter_map(|u| dims.distance(u.core.tile, goal)).max().unwrap_or(0) + 2;
        let tiles = around(game, goal, reach);
        rec.edit(game, rules, position, Op::Reveal { player: side.0, tiles });
    }
    if let (Some(v), None) = (variety.as_mut(), target) {
        // A skirmish: the learner's units' tiles, then the enemy's.
        let auth = game.view().auth;
        for (i, (_, units)) in armies.iter().enumerate() {
            for (h, k) in units {
                if let Some(u) = auth.units().get(*h) {
                    let arm = match battle::kind_arm(rules, *k) {
                        Some(Arm::Melee) => "melee",
                        Some(Arm::Mounted) => "mounted",
                        Some(Arm::Ranged) => "ranged",
                        Some(Arm::Siege) => "siege",
                        None => "other",
                    };
                    v.tiles.push(scene_tile(rules, game, u.core.tile, &if i == 0 { arm.to_owned() } else { format!("enemy-{arm}") }));
                }
            }
        }
    }
    if let (Some(v), Some(t)) = (variety.as_mut(), target) {
        v.tiles.push(scene_tile(rules, game, t, "city"));
        let auth = game.view().auth;
        for (h, k) in &armies[0].1 {
            if let Some(u) = auth.units().get(*h) {
                let role = if classic_rules::ai::combat::ranged_range(rules, *k).is_some_and(|r| r > 1) { "ranged" } else { "melee" };
                v.tiles.push(scene_tile(rules, game, u.core.tile, role));
            }
        }
    }
    let mut scene = BattleScene {
        kind,
        learner,
        enemy,
        start,
        length,
        target,
        aims,
        armies,
        captured_round: None,
        target_damage: 0.0,
        peace_rounds: 0,
        end_round: start,
        difficulty: d,
        stage,
        start_gap,
        rung: skirmish.map(|(r, _)| r).or(rung),
        need,
        variety,
        won_round: None,
        one_turn: ladder.is_some_and(Rung::one_turn),
        same_turn: ladder_same_turn,
        spawn_order,
        learner_turns: 0,
        struck_round: None,
        enemy_whole: attacker_hp.is_none(),
        recorder: rec,
        rounds: BattleRounds::default(),
    };
    scene.snapshot(game, rules);
    Ok(scene)
}

/// What `side` lost of `units` (its spawned army) in `game` now.
fn army_record(game: &GameState, rules: &CompiledRuleset, side: PlayerId, units: &[(UnitHandle, UnitKindId)]) -> ArmyRecord {
    let auth = game.view().auth;
    let mut rec = ArmyRecord { player: side.0, kinds: units.iter().map(|(_, k)| k.0).collect(), ..ArmyRecord::default() };
    for (h, k) in units {
        let cost = rules.units[k.0 as usize].cost;
        rec.value += cost;
        match auth.units().get(*h).filter(|u| u.core.owner == side) {
            Some(u) => rec.lost_value += f64::from(cost) * f64::from(kernel::world::unit::UNIT_MAX_HP.saturating_sub(u.core.hp)) / f64::from(kernel::world::unit::UNIT_MAX_HP),
            None => {
                rec.lost_units += 1;
                rec.lost_value += f64::from(cost);
            }
        }
    }
    rec
}

/// The oracle's commands at most in one turn (`BattleScene::strike`).
const STRIKE_COMMANDS: usize = 64;

impl BattleScene {
    /// A scripted learner's strikes (a skirmish's or a clash's; the net
    /// learner reads none): at the start of the learner's turn, before its
    /// mind plays it, each of its scene units still able attacks the scene's
    /// enemy — the best attack by the rules' preview (a kill first, then
    /// the expected damage dealt less taken), a melee one only when the
    /// unit is expected to survive it — and a unit with no target in reach
    /// first walks this turn to the free tile nearest its reach of the
    /// nearest enemy unit. The commands go to the log as any (the caller
    /// counts them in the positions). `standard` gives its spawned units
    /// the war tasks of its plans (a city's guard, a march on a city) and
    /// stands next to an equal enemy unit without striking; the oracle is
    /// the ground's teacher for the unit fights. Returns the commands made.
    pub fn strike(&mut self, rules: &CompiledRuleset, game: &mut GameState) -> u64 {
        use classic_rules::combat::damage::Roll;
        use classic_rules::combat::exchange::{plan_attack, AttackKind};
        use classic_rules::Command;
        let round = game.view().auth.turn().round;
        if !matches!(self.kind, BattleKind::Skirmish | BattleKind::Clash) || game.view().auth.active_player() != self.learner || self.struck_round == Some(round) {
            return 0;
        }
        self.struck_round = Some(round);
        let (learner, enemy) = (self.learner, self.enemy);
        let mine: Vec<UnitHandle> = self.armies[0].1.iter().map(|(h, _)| *h).collect();
        let theirs: Vec<UnitHandle> = self.armies[1].1.iter().map(|(h, _)| *h).collect();
        let mut made = 0u64;
        let mut walked: Vec<UnitHandle> = Vec::new();
        for _ in 0..STRIKE_COMMANDS {
            let auth = game.view().auth;
            let dims = auth.map().dimensions;
            let alive = |h: &UnitHandle, side: PlayerId| auth.units().get(*h).filter(|u| u.core.owner == side).map(|u| *u.core);
            let foes: Vec<(UnitHandle, kernel::world::unit::UnitCore)> = theirs.iter().filter_map(|h| alive(h, enemy).map(|c| (*h, c))).collect();
            if foes.is_empty() {
                break;
            }
            // (score, command) of the best attack of every unit.
            let mut best: Option<(i64, Command)> = None;
            for h in &mine {
                let Some(me) = alive(h, learner) else { continue };
                let ranged = classic_rules::ai::combat::ranged_range(rules, me.kind).is_some();
                for (fh, foe) in &foes {
                    let (cmd, how) = if ranged {
                        (Command::RangedAttack { unit: *h, target: foe.tile }, AttackKind::Ranged)
                    } else {
                        (Command::AttackUnit { attacker: *h, defender: *fh }, AttackKind::Melee)
                    };
                    if !game.accepts_order(rules, learner, &cmd) {
                        continue;
                    }
                    let Ok(plan) = plan_attack(game.view(), rules, *h, foe.tile, how) else { continue };
                    let (dealt, taken) = (i64::from(plan.damage_to_defender(Roll::Expected)), i64::from(plan.damage_to_attacker(Roll::Expected)));
                    let kill = dealt >= i64::from(foe.hp);
                    if !kill && taken >= i64::from(me.hp) {
                        continue;
                    }
                    // A kill first (the healthiest one killed), then the exchange, the most worn target first.
                    let score = if kill { 1000 + i64::from(foe.hp) } else { 2 * (dealt - taken) - (i64::from(foe.hp) - dealt) };
                    if best.as_ref().is_none_or(|b| score > b.0) {
                        best = Some((score, cmd));
                    }
                }
            }
            if let Some((_, cmd)) = best {
                if game.execute(rules, cmd).is_ok() {
                    made += 1;
                    continue;
                }
                break;
            }
            // No attack: one unit that has not walked walks toward the nearest enemy.
            let mut walk: Option<(UnitHandle, Vec<TileId>)> = None;
            for h in &mine {
                if walked.contains(h) {
                    continue;
                }
                walked.push(*h);
                let Some(me) = alive(h, learner) else { continue };
                let want = reach(rules, me.kind);
                let Some(near) = foes.iter().map(|(_, f)| f.tile).min_by_key(|t| dims.distance(me.tile, *t).unwrap_or(i32::MAX)) else { continue };
                if dims.distance(me.tile, near).is_some_and(|d| d <= want) {
                    continue;
                }
                let steps = rules.units.get(me.kind.0 as usize).map_or(1, |d| i32::from(d.movement.max(1)));
                // Never into an enemy city's strike: a fleeing unit goes home, and a chase there is lost.
                let cities: Vec<TileId> = auth.indexes().cities_of(enemy).iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.tile).collect();
                let struck = |t: TileId| cities.iter().any(|c| dims.distance(t, *c).is_some_and(|d| d <= classic_rules::combat::city::CITY_ATTACK_RANGE));
                let mut tiles: Vec<TileId> = (0..auth.map().tile_count() as u32)
                    .map(TileId)
                    .filter(|t| *t != me.tile && dims.distance(me.tile, *t).is_some_and(|d| d <= steps) && auth.indexes().units_on(*t).is_empty() && !struck(*t))
                    .collect();
                tiles.sort_by_key(|t| {
                    let d = foes.iter().filter_map(|(_, f)| dims.distance(*t, f.tile)).min().unwrap_or(i32::MAX);
                    ((d - want).abs(), d, t.0)
                });
                tiles.truncate(40);
                walk = Some((*h, tiles));
                break;
            }
            let mut moved = false;
            if let Some((h, tiles)) = walk {
                moved = true;
                let n = tiles.len();
                let mut ok = false;
                for t in tiles {
                    let cmd = Command::MoveUnit { unit: h, destination: t };
                    if game.accepts_order(rules, learner, &cmd) && game.execute(rules, cmd).is_ok() {
                        made += 1;
                        ok = true;
                        break;
                    }
                }
                if trace_on() {
                    eprintln!("strike r{round}: walk of {h:?} over {n} tiles: {ok}");
                }
            }
            if !moved {
                break;
            }
        }
        made
    }

    pub fn learner(&self) -> PlayerId {
        self.learner
    }

    pub fn kind(&self) -> BattleKind {
        self.kind
    }

    /// The learner's army and the enemy's: the units spawned for each side.
    pub fn armies(&self) -> &[(PlayerId, Vec<(UnitHandle, UnitKindId)>); 2] {
        &self.armies
    }

    /// A siege ladder scene that lets the learner take the city in its
    /// first turn by construction (`BattleRecord::same_turn`; None: no
    /// ladder scene).
    pub fn same_turn(&self) -> Option<bool> {
        self.same_turn
    }

    /// The city fought over (none for a fight in the open).
    pub fn target(&self) -> Option<TileId> {
        self.target
    }

    /// The sides remember their war objectives (`battle::aim`).
    pub fn aim<P: AiMind<Classic>>(&self, driver: &mut AiDriver<Classic, P>, game: &GameState) {
        let round = game.view().auth.turn().round;
        for &(side, tile) in &self.aims {
            battle::aim(driver, game, side, tile, round);
        }
        // A ladder rung's learner army assaults the city from its first turn: a scripted learner's
        // siege plans see its spawned units as members (`UnitTask::Assault`; the net's units read no task).
        if let (Some(_), Some(city)) = (self.need, self.target) {
            let auth = game.view().auth;
            let (side, units) = &self.armies[0];
            let state = driver.state_mut(*side, round);
            for (h, _) in units {
                if let Some(u) = auth.units().get(*h).filter(|u| u.core.owner == *side) {
                    state.set_task(u.core.id, kernel::ai::state::UnitTask::Assault { target: city }, round);
                }
            }
        }
        self.hunt(driver, game);
    }

    /// A unit fight's learner hunts the scene's enemy: its war objective
    /// the land where the enemy unit nearest its army stands
    /// (`ObjectiveKind::Territory`) and each of its scene units' task the
    /// assault on that tile, renewed every round — a scripted learner's
    /// units walk on the enemy (its plans gave them a city's guard or a
    /// march on a city and they stood next to an enemy unit without
    /// striking); the net's units read no task.
    fn hunt<P: AiMind<Classic>>(&self, driver: &mut AiDriver<Classic, P>, game: &GameState) {
        use kernel::ai::state::{ObjectiveKind, UnitTask, WarObjective};
        if !matches!(self.kind, BattleKind::Skirmish | BattleKind::Clash) {
            return;
        }
        let auth = game.view().auth;
        let dims = auth.map().dimensions;
        let round = auth.turn().round;
        let mine: Vec<(kernel::ids::EntityId, TileId)> =
            self.armies[0].1.iter().filter_map(|(h, _)| auth.units().get(*h).filter(|u| u.core.owner == self.learner).map(|u| (u.core.id, u.core.tile))).collect();
        let theirs: Vec<TileId> = self.armies[1].1.iter().filter_map(|(h, _)| auth.units().get(*h).filter(|u| u.core.owner == self.enemy).map(|u| u.core.tile)).collect();
        let Some(target) = theirs.iter().copied().min_by_key(|t| (mine.iter().filter_map(|(_, m)| dims.distance(*m, *t)).sum::<i32>(), t.0)) else { return };
        let state = driver.state_mut(self.learner, round);
        state.relation_mut(self.enemy).objective = Some(WarObjective { tile: target, kind: ObjectiveKind::Territory, chosen_round: round });
        for (id, _) in &mine {
            state.set_task(*id, UnitTask::Assault { target }, round);
        }
    }

    /// After a round: the target's owner and damage; whether the battle is
    /// over (its length played, the learner out, the target taken or lost,
    /// an army gone); a peace broken (the war declared again, recorded at
    /// `position`, and the scripted side aimed again) when it goes on.
    pub fn after_round<P: AiMind<Classic>>(&mut self, rules: &CompiledRuleset, game: &mut GameState, driver: &mut AiDriver<Classic, P>, position: u64) -> bool {
        let round = game.view().auth.turn().round;
        self.end_round = round;
        self.snapshot(game, rules);
        let auth = game.view().auth;
        if let Some(tile) = self.target {
            if let Some(city) = auth.indexes().city_on_tile(tile).and_then(|c| auth.cities().get(c)) {
                let max = classic_rules::combat::city::max_hit_points(auth, rules, city).max(1);
                self.target_damage = self.target_damage.max(f64::from(city.damage) / f64::from(max));
                // A one-turn rung's capture is the learner's first turn's (`after_player`; here when no turn end was seen).
                if !self.kind.learner_holds() && city.owner == self.learner && self.captured_round.is_none() && (!self.one_turn || self.learner_turns == 0) {
                    self.captured_round = Some(round);
                }
            }
        }
        if trace_on() {
            self.trace(game, rules, driver, round);
        }
        let owner = self.target_owner(game);
        let gone = |(side, units): &(PlayerId, Vec<(UnitHandle, UnitKindId)>)| !units.is_empty() && units.iter().all(|(h, _)| auth.units().get(*h).is_none_or(|u| u.core.owner != *side));
        let army_gone = self.armies.iter().any(gone);
        let beaten = self.beaten(game, rules);
        if self.kind == BattleKind::Skirmish && self.won_round.is_none() && beaten && !gone(&self.armies[0]) {
            self.won_round = Some(round);
        }
        let over = round >= self.start + self.length
            || !auth.players().is_alive(self.learner)
            || self.captured_round.is_some()
            || (self.kind.learner_holds() && owner != Some(self.learner.0))
            || (matches!(self.kind, BattleKind::Field | BattleKind::Skirmish | BattleKind::Clash) && army_gone)
            || self.won_round.is_some()
            // A ladder rung below today's stages ends with the learner's army: nothing is left to learn.
            || (self.need.is_some() && self.armies[0].1.iter().all(|(h, _)| auth.units().get(*h).is_none_or(|u| u.core.owner != self.learner)))
            // A one-turn rung ends with its first round.
            || self.one_turn;
        if !over && !auth.diplomacy().at_war(self.learner, self.enemy) {
            self.peace_rounds += 1;
            let (a, b) = (self.learner.0, self.enemy.0);
            self.recorder.edit(game, rules, position, Op::War { a: b, b: a });
            self.aim(driver, game);
        } else if !over {
            self.hunt(driver, game);
        }
        over
    }

    /// A skirmish's enemy is beaten when each of its units is dead or, having
    /// started whole, below the ready share, or (started whole) when it has
    /// lost the ready share of its value: routed.
    fn beaten(&self, game: &GameState, rules: &CompiledRuleset) -> bool {
        let auth = game.view().auth;
        let ready = f64::from(kernel::world::unit::UNIT_MAX_HP) * READY_SHARE;
        let routed = self.enemy_whole && army_record(game, rules, self.enemy, &self.armies[1].1).lost_share() >= READY_SHARE;
        routed || self.armies[1].1.iter().all(|(h, _)| auth.units().get(*h).filter(|u| u.core.owner == self.enemy).is_none_or(|u| self.enemy_whole && f64::from(u.core.hp) < ready))
    }

    /// The record as if the battle were judged now, within a turn (the
    /// search's end-of-turn score, `search::turn_score`): the target's
    /// damage now, a capture now (a ladder rung's in the learner's first
    /// turn as the war's first round, as `after_player` records it) and a
    /// skirmish won now count in this round.
    pub fn record_now(&self, game: &GameState, rules: &CompiledRuleset) -> BattleRecord {
        let round = game.view().auth.turn().round;
        let mut now = self.clone();
        now.target_damage = now.target_damage.max(self.damage_now(game, rules));
        if !self.kind.learner_holds() && now.captured_round.is_none() && self.target_owner(game) == Some(self.learner.0) {
            now.captured_round = Some(if self.need.is_some() && self.learner_turns == 0 { self.start } else { round });
        }
        let auth = game.view().auth;
        let alive = self.armies[0].1.iter().any(|(h, _)| auth.units().get(*h).is_some_and(|u| u.core.owner == self.learner));
        if self.kind == BattleKind::Skirmish && now.won_round.is_none() && alive && self.beaten(game, rules) {
            now.won_round = Some(round);
        }
        now.finish(game, rules).battle.expect("a battle's record")
    }

    /// After a player's turn: a one-turn rung's capture is the learner's
    /// first turn's (`captured_round` the war's first round), none later.
    pub fn after_player(&mut self, game: &GameState, player: PlayerId) {
        if player != self.learner {
            return;
        }
        self.learner_turns += 1;
        // Every ladder rung: a capture in the learner's first turn is recorded as the war's first round.
        if self.need.is_some() && self.learner_turns == 1 && self.captured_round.is_none() && self.target_owner(game) == Some(self.learner.0) {
            self.captured_round = Some(self.start);
        }
    }

    /// `GROUNDS_TRACE=1`: one line a round on stderr — the war, the
    /// target's damage, hit points and owner, and per side each unit of its
    /// army alive (kind, distance to the target, health, its AI task's first
    /// letters; `-`: none, the net's units).
    fn trace<P: AiMind<Classic>>(&self, game: &GameState, rules: &CompiledRuleset, driver: &AiDriver<Classic, P>, round: u32) {
        let auth = game.view().auth;
        let dims = auth.map().dimensions;
        let city = self.target.and_then(|t| auth.indexes().city_on_tile(t)).and_then(|c| auth.cities().get(c));
        let hp = city.map_or_else(String::new, |c| format!(" city {}/{} p{}", c.damage, classic_rules::combat::city::max_hit_points(auth, rules, c), c.owner.0));
        let mut line = format!("grounds {} r{round} war={}{hp}", self.kind.name(), auth.diplomacy().at_war(self.learner, self.enemy));
        for (side, units) in &self.armies {
            let state = driver.states().get(*side);
            line += &format!(" | p{}:", side.0);
            for (h, k) in units {
                let Some(u) = auth.units().get(*h).filter(|u| u.core.owner == *side) else { continue };
                let d = self.target.and_then(|t| dims.distance(u.core.tile, t)).unwrap_or(-1);
                let task = state.and_then(|s| s.task_of(u.core.id)).map_or_else(|| "-".to_owned(), |t| format!("{:?}", t.task).chars().take(3).collect());
                line += &format!(" {}d{d}h{}{task}", k.0, u.core.hp);
            }
        }
        eprintln!("{line}");
    }

    /// The target's damage now, a share of its hit points (0 without one).
    fn damage_now(&self, game: &GameState, rules: &CompiledRuleset) -> f64 {
        let auth = game.view().auth;
        self.target.and_then(|t| auth.indexes().city_on_tile(t)).and_then(|c| auth.cities().get(c)).map_or(0.0, |city| {
            let max = classic_rules::combat::city::max_hit_points(auth, rules, city).max(1);
            f64::from(city.damage) / f64::from(max)
        })
    }

    /// The round's entry of `rounds` (`BattleRounds`).
    fn snapshot(&mut self, game: &GameState, rules: &CompiledRuleset) {
        let learner = army_record(game, rules, self.armies[0].0, &self.armies[0].1);
        let enemy = army_record(game, rules, self.armies[1].0, &self.armies[1].1);
        let damage = self.damage_now(game, rules);
        let ready = (self.kind == BattleKind::Clash).then(|| ready_value(game, rules, self.armies[0].0, &self.armies[0].1));
        self.rounds.push(game.view().auth.turn().round, &learner, &enemy, damage, ready);
    }

    fn target_owner(&self, game: &GameState) -> Option<u8> {
        let auth = game.view().auth;
        self.target.and_then(|t| auth.indexes().city_on_tile(t)).and_then(|c| auth.cities().get(c)).map(|c| c.owner.0)
    }

    /// The record at the end.
    pub fn finish(self, game: &GameState, rules: &CompiledRuleset) -> GroundRecord {
        let army = |(side, units): &(PlayerId, Vec<(UnitHandle, UnitKindId)>)| army_record(game, rules, *side, units);
        let owner = self.target_owner(game);
        let mut record = BattleRecord {
            learner: self.learner.0,
            enemy: self.enemy.0,
            start_round: self.start,
            length: self.length,
            end_round: self.end_round,
            target: self.target.map(|t| t.0),
            target_owner: owner,
            captured_round: self.captured_round,
            held: self.kind.learner_holds() && owner == Some(self.learner.0),
            target_damage: self.target_damage,
            learner_army: army(&self.armies[0]),
            enemy_army: army(&self.armies[1]),
            rounds: self.rounds,
            peace_rounds: self.peace_rounds,
            difficulty: self.difficulty,
            stage: self.stage,
            start_gap: self.start_gap,
            rung: self.rung,
            need: self.need,
            variety: self.variety,
            same_turn: self.same_turn,
            spawn_order: self.spawn_order.map(|o| o.name().to_owned()),
            won_round: self.won_round,
            reward: 0.0,
        };
        record.reward = battle_reward(self.kind, &record);
        GroundRecord { suite: "battle".to_owned(), kind: self.kind.name().to_owned(), battle: Some(record), economy: None, opening: None, edits: self.recorder.edits }
    }
}

// ---------------------------------------------------------------------------
// Economy windows

/// An economy window being played.
#[derive(Debug)]
pub struct WindowScene {
    from: u32,
    window: u32,
    end_round: u32,
    nations: Vec<PlayerId>,
    alive_at_start: Vec<bool>,
    science: Vec<i64>,
    techs_start: Vec<u32>,
}

impl WindowScene {
    pub fn new(game: &GameState, nations: &[PlayerId], window: u32) -> Self {
        let auth = game.view().auth;
        let from = auth.turn().round;
        Self {
            from,
            window,
            end_round: from,
            nations: nations.to_vec(),
            alive_at_start: nations.iter().map(|p| auth.players().is_alive(*p)).collect(),
            science: vec![0; nations.len()],
            techs_start: nations.iter().map(|p| auth.players().techs_of(*p).len() as u32).collect(),
        }
    }

    /// After a round: each living nation's science a turn added; whether the
    /// window is over.
    pub fn after_round(&mut self, game: &GameState, rules: &CompiledRuleset) -> bool {
        let auth = game.view().auth;
        for (i, p) in self.nations.iter().enumerate() {
            if auth.players().is_alive(*p) {
                self.science[i] += i64::from(classic_rules::derived::income::player_income(auth, rules, *p).science);
            }
        }
        self.end_round = auth.turn().round;
        self.end_round >= self.from + self.window
    }

    /// The record at the end; `expert`: the source log at the window's end
    /// round (`selfplay::expert_at`) and its AI variant (the price model of
    /// both potentials; without one the learner's own prices, no expert).
    pub fn finish(self, game: &GameState, rules: &CompiledRuleset, learners: &[PlayerId], expert: Option<(&ExpertAt, Variant)>) -> GroundRecord {
        let auth = game.view().auth;
        let mut rows = Vec::new();
        for p in learners {
            let mine = holdings(game, rules, *p);
            let theirs = expert.and_then(|(e, v)| e.seats.get(&p.0).map(|s| (s, v)));
            let variant = expert.map_or(Variant::Standard, |(_, v)| v);
            let prices = match theirs {
                Some(((_, Some(sheet)), _)) => sheet.clone(),
                _ => price_sheet(game, rules, *p, variant),
            };
            rows.push(SeatPotential { player: p.0, net: potential(&mine, &prices), expert: theirs.map(|((h, _), _)| potential(h, &prices)), prices });
        }
        let n = rows.len().max(1) as f64;
        let mean_of = |f: &dyn Fn(&SeatPotential) -> Option<f64>| -> Option<f64> { rows.iter().map(f).sum::<Option<f64>>().map(|s| s / n) };
        let rate = |p: &Potential| p.components.get("science").map_or(0.0, |c| c.raw);
        let learner_rate = mean_of(&|s| Some(rate(&s.net))).unwrap_or(0.0);
        let expert_rate = if expert.is_some() { mean_of(&|s| s.expert.as_ref().map(rate)) } else { None };
        let learner_potential = mean_of(&|s| Some(s.net.total)).unwrap_or(0.0);
        let expert_potential = if expert.is_some() { mean_of(&|s| s.expert.as_ref().map(|e| e.total)) } else { None };
        let seats: Vec<WindowSeat> = self
            .nations
            .iter()
            .enumerate()
            .map(|(i, p)| WindowSeat { player: p.0, science: self.science[i], techs_start: self.techs_start[i], techs_end: auth.players().techs_of(*p).len() as u32 })
            .collect();
        let mean = |xs: Vec<i64>| if xs.is_empty() { 0.0 } else { xs.iter().sum::<i64>() as f64 / xs.len() as f64 };
        let learner_science = mean((0..seats.len()).filter(|&i| learners.contains(&self.nations[i])).map(|i| seats[i].science).collect());
        let opponents_mean = mean((0..seats.len()).filter(|&i| !learners.contains(&self.nations[i]) && self.alive_at_start[i]).map(|i| seats[i].science).collect());
        let total = learner_science + opponents_mean;
        let economy = EconomyRecord {
            from_round: self.from,
            window: self.window,
            end_round: self.end_round,
            seats,
            learner_science,
            opponents_mean,
            science_share: if total > 0.0 { learner_science / total } else { 0.5 },
            learner_rate,
            expert_rate,
            expert_round: expert.map(|(e, _)| e.round),
            learner_potential,
            expert_potential,
            potential: rows,
        };
        GroundRecord { suite: "economy".to_owned(), kind: "window".to_owned(), battle: None, economy: Some(economy), opening: None, edits: Vec::new() }
    }
}


// ---------------------------------------------------------------------------
// Openings: the peacetime work of the units

/// An opening ground's length in Standard rounds by default (`--opening 0`;
/// scaled to the game's speed: 89 at Quick): long enough for the workers'
/// work to show (at Quick `standard` has 1 improved tile worked at round 60,
/// 6 at 90, 14 at 120), where Φ ranks the nations by their final score best
/// (classic/docs/ai-net-data.md, «The opening ground»).
pub const OPENING_GROUND_STANDARD_ROUNDS: u32 = 135;

/// The weights of the opening's potential Φ ([`opening_phi`]), each term a
/// share or a count of what the nation knows and holds (no content names;
/// the normalisers are the map's tiles, the game's rival nations and
/// city-states, the production cost of the nation's units at the start).
/// Chosen from whole games of `standard`, of the net's units and of the
/// net's units with `standard`'s workers (classic/docs/ai-net-data.md,
/// «The opening ground»): the potential carries the workers' work (their
/// yields) and ranks a group's games by their final score best; a quarter
/// of the explored share and the civilians lost (workers captured) add what
/// the potential misses; the other terms are recorded at weight 0 (they
/// add noise within a group and no ranking).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpeningWeights {
    /// log(P + P0), the empire's potential (`potential`; P0 at its first city).
    pub potential: f64,
    /// Tiles explored, a share of the map's.
    pub explored: f64,
    /// Rival nations met, a share of the rivals.
    pub majors_met: f64,
    /// City-states met, a share of the game's.
    pub city_states_met: f64,
    /// Rival cities seen (ever), per rival.
    pub rival_cities_seen: f64,
    /// Rival original capitals whose tile is known, a share of the rivals.
    pub rival_capitals_known: f64,
    /// Barbarian camps the nation cleared.
    pub camps_cleared: f64,
    /// The production cost of the units lost, a share of the units' at the start (a penalty).
    pub lost_value: f64,
    /// Tiles its cities work that bear a standing improvement (the workers' work in use).
    pub improved_worked: f64,
    /// Land of its territory bearing a standing improvement.
    pub improved: f64,
    /// Civilian units lost (the roles founder, builder, civilian: `kernel::ai::plan::role_of`; a penalty).
    pub civilians_lost: f64,
}

pub const OPENING_WEIGHTS: OpeningWeights = OpeningWeights {
    potential: 1.0,
    explored: 0.25,
    majors_met: 0.0,
    city_states_met: 0.0,
    rival_cities_seen: 0.0,
    rival_capitals_known: 0.0,
    camps_cleared: 0.0,
    lost_value: 0.0,
    improved_worked: 0.0,
    improved: 0.0,
    civilians_lost: 0.05,
};

/// One nation's opening round by round. Entry 0 is the world at the
/// ground's start (round 0), entry k the start of round `round[k]` (after
/// the round before it was played), as [`BattleRounds`]: the learner's turn
/// of round R did what lies between the entries of R and R + 1. Counts are
/// cumulative where the world forgets (rival cities seen ever, camps
/// cleared, units lost).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpeningSeat {
    pub player: u8,
    pub learner: bool,
    /// The normalisers: the map's tiles, the game's rival nations and
    /// city-states, the production cost of the nation's units at entry 0.
    pub tiles: u32,
    pub rivals: u32,
    pub city_states: u32,
    pub start_value: u32,
    /// The price sheet `potential` is valued at: the nation's own at its
    /// first entry with a city (`standard`'s price model; None: never a
    /// city, `potential` 0).
    pub prices: Option<PriceSheet>,
    pub explored: Vec<u32>,
    pub majors_met: Vec<u32>,
    pub city_states_met: Vec<u32>,
    pub rival_cities_seen: Vec<u32>,
    pub rival_capitals_known: Vec<u32>,
    pub camps_cleared: Vec<u32>,
    pub units_lost: Vec<u32>,
    /// The production cost of the units lost.
    pub lost_value: Vec<u32>,
    pub cities: Vec<u32>,
    #[serde(default)]
    pub improved_worked: Vec<u32>,
    #[serde(default)]
    pub improved: Vec<u32>,
    #[serde(default)]
    pub civilians_lost: Vec<u32>,
    /// The cities' hammers and food surplus a turn (the potential's raw
    /// `production` and `food`: where the workers' work shows first).
    #[serde(default)]
    pub production: Vec<f32>,
    #[serde(default)]
    pub food: Vec<f32>,
    /// The potential (score points, to 0.01).
    pub potential: Vec<f32>,
}

/// How an opening went: every major nation's columns (the learners'
/// marked), and the reference reward.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpeningRecord {
    pub window: u32,
    /// The round play stopped at (window, earlier when the game ended).
    pub end_round: u32,
    pub round: Vec<u32>,
    pub seats: Vec<OpeningSeat>,
    /// The learners' mean Φ(end) − Φ(start) at `OPENING_WEIGHTS` (`opening_phi`).
    pub reward: f64,
}

/// The opening's potential Φ of `seat` at entry `k`:
/// w_P × log(P + P0) + w_e × explored / tiles + w_m × majors met / rivals
/// + w_c × city-states met / city-states + w_s × rival cities seen / rivals
/// + w_k × rival capitals known / rivals + w_b × camps cleared − w_l × lost
/// value / start value + w_i × improved tiles worked + w_t × improved land
/// − w_v × civilians lost; P0 the potential at the first entry with a city (1
/// without one), so a nation without a city has a finite log P0 and its
/// first city is worth about log 2 (`train_e2.potential_phi`).
pub fn opening_phi(seat: &OpeningSeat, k: usize, w: &OpeningWeights) -> f64 {
    let share = |x: u32, of: u32| if of == 0 { 0.0 } else { f64::from(x) / f64::from(of) };
    let at = |v: &Vec<u32>| v.get(k).copied().unwrap_or(0);
    let p0 = seat.cities.iter().position(|c| *c > 0).and_then(|i| seat.potential.get(i)).map(|p| f64::from(*p)).filter(|p| *p > 0.0).unwrap_or(1.0);
    let p = seat.potential.get(k).map_or(0.0, |p| f64::from(*p).max(0.0));
    w.potential * (p + p0).ln()
        + w.explored * share(at(&seat.explored), seat.tiles)
        + w.majors_met * share(at(&seat.majors_met), seat.rivals)
        + w.city_states_met * share(at(&seat.city_states_met), seat.city_states)
        + w.rival_cities_seen * share(at(&seat.rival_cities_seen), seat.rivals)
        + w.rival_capitals_known * share(at(&seat.rival_capitals_known), seat.rivals)
        + w.camps_cleared * f64::from(at(&seat.camps_cleared))
        - w.lost_value * share(at(&seat.lost_value), seat.start_value)
        + w.improved_worked * f64::from(at(&seat.improved_worked))
        + w.improved * f64::from(at(&seat.improved))
        - w.civilians_lost * f64::from(at(&seat.civilians_lost))
}

/// One nation's tracking state.
#[derive(Debug, Default)]
struct OpeningTrack {
    seat: OpeningSeat,
    holdings: Vec<Holdings>,
    /// Own units at the last turn's end or entry: id → (kind, tile); own city tiles.
    units: BTreeMap<u64, (u16, u32)>,
    city_tiles: std::collections::BTreeSet<u32>,
    seen: std::collections::BTreeSet<u32>,
    cleared: std::collections::BTreeSet<(u32, u32)>,
    /// Units lost, their cost, civilians lost.
    lost: (u32, u32, u32),
}

/// `player`'s units now: id → (kind, tile).
fn own_units(game: &GameState, player: PlayerId) -> BTreeMap<u64, (u16, u32)> {
    game.view().auth.units().iter_cores().filter(|u| u.owner == player).map(|u| (u.id.0, (u.kind.0, u.tile.0))).collect()
}

/// An opening being played: every major's columns after every round.
#[derive(Debug)]
pub struct OpeningScene {
    window: u32,
    end_round: u32,
    round: Vec<u32>,
    tracks: Vec<OpeningTrack>,
}

impl OpeningScene {
    pub fn new(game: &GameState, rules: &CompiledRuleset, nations: &[PlayerId], window: u32) -> Self {
        use classic_rules::world::player::ClassicPlayers;
        let auth = game.view().auth;
        let players = auth.players();
        let city_states = (0..players.count() as u8).filter(|p| players.is_city_state(PlayerId(*p))).count() as u32;
        let tracks = nations
            .iter()
            .map(|p| OpeningTrack {
                seat: OpeningSeat { player: p.0, tiles: auth.map().tile_count() as u32, rivals: nations.len().saturating_sub(1) as u32, city_states, ..OpeningSeat::default() },
                ..OpeningTrack::default()
            })
            .collect();
        let mut scene = Self { window, end_round: auth.turn().round, round: Vec::new(), tracks };
        scene.snapshot(game, rules);
        for t in &mut scene.tracks {
            t.seat.start_value = t.units.values().map(|(k, _)| rules.units.get(*k as usize).map_or(0, |d| d.cost)).sum();
        }
        scene
    }

    /// The entry of the world now.
    fn snapshot(&mut self, game: &GameState, rules: &CompiledRuleset) {
        use classic_rules::world::player::ClassicPlayers;
        let auth = game.view().auth;
        let players = auth.players();
        let round = auth.turn().round;
        self.round.push(round);
        for t in &mut self.tracks {
            let p = PlayerId(t.seat.player);
            let knowledge = auth.knowledge().get(p.0 as usize);
            let explored = knowledge.map_or(0, |k| k.explored_count()) as u32;
            let (mut majors_met, mut cs_met) = (0u32, 0u32);
            if let Some(k) = knowledge {
                for (i, c) in k.contacts().iter().enumerate() {
                    let other = PlayerId(i as u8);
                    if other == p || c.met_round.is_none() {
                        continue;
                    }
                    if players.is_major(other) {
                        majors_met += 1;
                    } else if players.is_city_state(other) {
                        cs_met += 1;
                    }
                }
                for (tile, s) in k.sightings() {
                    if s.owner != p && players.is_major(s.owner) {
                        t.seen.insert(tile.0);
                    }
                }
            }
            let capitals = auth
                .cities()
                .iter()
                .map(|(_, c)| c)
                .filter(|c| c.original_capital && c.original_owner != p && players.is_major(c.original_owner) && knowledge.is_some_and(|k| k.is_explored(c.tile)))
                .count() as u32;
            for c in auth.module().barbarians().cleared().iter().filter(|c| c.player == p) {
                t.cleared.insert((c.tile.0, c.round));
            }
            t.units = own_units(game, p);
            t.city_tiles = auth.indexes().cities_of(p).iter().filter_map(|h| auth.cities().get(*h)).map(|c| c.tile.0).collect();
            let map = auth.map();
            let improved_worked = auth
                .indexes()
                .cities_of(p)
                .iter()
                .filter_map(|h| auth.cities().get(*h))
                .map(|core| classic_rules::derived::yields::city_work(auth, rules, core).tiles().filter(|tile| map.working_improvement(*tile).is_some()).count() as u32)
                .sum::<u32>();
            let improved = (0..map.tile_count() as u32).map(TileId).filter(|tile| map.owner(*tile).flatten() == Some(p) && map.working_improvement(*tile).is_some()).count() as u32;
            let h = holdings(game, rules, p);
            if t.seat.prices.is_none() && h.raw.get("cities").copied().unwrap_or(0.0) > 0.0 {
                t.seat.prices = Some(price_sheet(game, rules, p, Variant::Standard));
            }
            t.seat.explored.push(explored);
            t.seat.majors_met.push(majors_met);
            t.seat.city_states_met.push(cs_met);
            t.seat.rival_cities_seen.push(t.seen.len() as u32);
            t.seat.rival_capitals_known.push(capitals);
            t.seat.camps_cleared.push(t.cleared.len() as u32);
            t.seat.units_lost.push(t.lost.0);
            t.seat.lost_value.push(t.lost.1);
            t.seat.cities.push(t.city_tiles.len() as u32);
            t.seat.improved_worked.push(improved_worked);
            t.seat.improved.push(improved);
            t.seat.civilians_lost.push(t.lost.2);
            t.holdings.push(h);
        }
    }

    /// After `player`'s turn: each nation's units gone in it are lost when
    /// another player acted (killed, captured: the turns of rivals,
    /// barbarians, city-states); in the nation's own turn a unit gone was
    /// spent (a city founded, an improvement built by a unit used up, a
    /// great person's act, an upgrade) or fell in its own attack, and is not
    /// counted.
    pub fn after_player(&mut self, game: &GameState, rules: &CompiledRuleset, player: PlayerId) {
        for t in &mut self.tracks {
            let p = PlayerId(t.seat.player);
            let units = own_units(game, p);
            if p != player {
                for kind in t.units.iter().filter(|(id, _)| !units.contains_key(id)).map(|(_, (kind, _))| UnitKindId(*kind)) {
                    t.lost.0 += 1;
                    t.lost.1 += rules.units.get(kind.0 as usize).map_or(0, |d| d.cost);
                    t.lost.2 += u32::from(!matches!(kernel::ai::plan::role_of(rules, kind), kernel::ai::plan::AiRole::Military | kernel::ai::plan::AiRole::Naval));
                }
            }
            t.units = units;
        }
    }

    /// After a round: the entry; whether the opening is over.
    pub fn after_round(&mut self, game: &GameState, rules: &CompiledRuleset) -> bool {
        self.end_round = game.view().auth.turn().round;
        self.snapshot(game, rules);
        self.end_round >= self.window
    }

    /// The record at the end.
    pub fn finish(self, learners: &[PlayerId]) -> GroundRecord {
        let seats: Vec<OpeningSeat> = self
            .tracks
            .into_iter()
            .map(|t| {
                let mut seat = t.seat;
                seat.learner = learners.iter().any(|p| p.0 == seat.player);
                let raw = |k: &str| t.holdings.iter().map(|h| h.raw.get(k).copied().unwrap_or(0.0) as f32).collect::<Vec<f32>>();
                seat.production = raw("production");
                seat.food = raw("food");
                seat.potential = t.holdings.iter().map(|h| seat.prices.as_ref().map_or(0.0, |s| ((potential(h, s).total * 100.0).round() / 100.0) as f32)).collect();
                seat
            })
            .collect();
        let last = self.round.len().saturating_sub(1);
        let mine: Vec<f64> = seats.iter().filter(|s| s.learner).map(|s| opening_phi(s, last, &OPENING_WEIGHTS) - opening_phi(s, 0, &OPENING_WEIGHTS)).collect();
        let reward = if mine.is_empty() { 0.0 } else { mine.iter().sum::<f64>() / mine.len() as f64 };
        let opening = OpeningRecord { window: self.window, end_round: self.end_round, round: self.round, seats, reward };
        GroundRecord { suite: "opening".to_owned(), kind: "window".to_owned(), battle: None, economy: None, opening: Some(opening), edits: Vec::new() }
    }
}

/// A job's scene while it plays.
#[derive(Debug)]
pub enum Scene {
    None,
    Battle(Box<BattleScene>),
    Window(WindowScene),
    Opening(OpeningScene),
}

impl Scene {
    /// The scene of `ground` on the world at its start (a battle's set
    /// there, its edits at `position`).
    pub fn set(rules: &CompiledRuleset, game: &mut GameState, ground: Option<&GroundJob>, nations: &[PlayerId], kinds: Option<&BTreeMap<PlayerId, Vec<UnitKindId>>>, position: u64) -> Result<Self, String> {
        match ground {
            None => Ok(Scene::None),
            Some(g @ GroundJob::Battle { .. }) => {
                let empty = BTreeMap::new();
                Ok(Scene::Battle(Box::new(set_battle(rules, game, g, kinds.unwrap_or(&empty), position)?)))
            }
            Some(GroundJob::Economy { window }) => Ok(Scene::Window(WindowScene::new(game, nations, *window))),
            Some(GroundJob::Opening { window }) => Ok(Scene::Opening(OpeningScene::new(game, rules, nations, *window))),
        }
    }

    /// The learner's nation a battle gives (None: the job's own rule).
    pub fn learner(&self) -> Option<PlayerId> {
        match self {
            Scene::Battle(b) => Some(b.learner()),
            _ => None,
        }
    }

    pub fn aim<P: AiMind<Classic>>(&self, driver: &mut AiDriver<Classic, P>, game: &GameState) {
        if let Scene::Battle(b) = self {
            b.aim(driver, game);
        }
    }

    /// A scripted learner's strikes at its turn's start (`BattleScene::strike`); the commands made.
    pub fn strike(&mut self, rules: &CompiledRuleset, game: &mut GameState) -> u64 {
        match self {
            Scene::Battle(b) => b.strike(rules, game),
            _ => 0,
        }
    }

    /// After a player's turn (an opening counts the units lost in it).
    pub fn after_player(&mut self, game: &GameState, rules: &CompiledRuleset, player: PlayerId) {
        if let Scene::Battle(b) = self {
            b.after_player(game, player);
        }
        if let Scene::Opening(o) = self {
            o.after_player(game, rules, player);
        }
    }

    /// After a round: whether the ground is over.
    pub fn after_round<P: AiMind<Classic>>(&mut self, rules: &CompiledRuleset, game: &mut GameState, driver: &mut AiDriver<Classic, P>, position: u64) -> bool {
        match self {
            Scene::None => false,
            Scene::Battle(b) => b.after_round(rules, game, driver, position),
            Scene::Window(w) => w.after_round(game, rules),
            Scene::Opening(o) => o.after_round(game, rules),
        }
    }

    /// The record at the end; an economy window's expert (`WindowScene::finish`).
    pub fn finish(self, game: &GameState, rules: &CompiledRuleset, learners: &[PlayerId], expert: Option<(&ExpertAt, Variant)>) -> Option<GroundRecord> {
        match self {
            Scene::None => None,
            Scene::Battle(b) => Some(b.finish(game, rules)),
            Scene::Window(w) => Some(w.finish(game, rules, learners, expert)),
            Scene::Opening(o) => Some(o.finish(learners)),
        }
    }

    /// The round an economy window stopped at (None: no window).
    pub fn window_end(&self) -> Option<u32> {
        match self {
            Scene::Window(w) => Some(w.end_round),
            _ => None,
        }
    }
}
