//! The data of the end-to-end net (classic/docs/ai-net-data.md): games of
//! the scripted mind kept as replay logs, and the shards of features made
//! from a log on demand.
//!
//! - `play_log`: one AI-only game of `ClassicMind` (no wrapper: the game is
//!   the plain one) with the kernel's command journal on
//!   (`replay/journal.rs`: every applied command, the world's hash after
//!   every round). The log is that journal beside the game's spec (`LogSpec`,
//!   JSON): the game is generated again from its seed, so a log is tens of
//!   kilobytes.
//! - `features`: replays a log command by command, checks every round's
//!   hash, and at each command of a nation builds that player's view of the
//!   exact world before the command: the turn's rows once at its first
//!   command (`net_e2e::encode::Turn`), the candidates of the command's head
//!   and the command's place among them (the expert's choice). Units that
//!   still have moves when their player ends its turn are a decision to
//!   wait. The result is a `Shard`, written as `.npy` files and `meta.json`
//!   (`write_shard`).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::net_e2e::encode::{self, head, tag, Candidate, Turn};
use classic_rules::ai::net_e2e::events::Tracker;
use classic_rules::ai::variant::{Variant, NET_BASE};
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::classic::Classic;
use classic_rules::knowledge::actions::acting_unit;
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::scenario::MapScript;
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ai::mind::AiMind;
use kernel::ids::{PlayerId, TileId};
use kernel::persistence::codec::CodecError;
use kernel::replay::hash::authoritative_hash;
use kernel::replay::journal::{take_record, JournalHeader, Record};
use serde::{Deserialize, Serialize};

use crate::bench::modules::{standing, ClassicBench};
use crate::bench::suite::{Mode, Suite};
use crate::spec::{size_from_name, GameSpec};

/// What a log was played from: enough to generate the same game again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogSpec {
    pub suite: String,
    pub seed: u64,
    pub script: String,
    pub size: String,
    /// The round of the time victory, the last one played.
    pub rounds: u32,
    pub difficulty: String,
    pub variant: String,
    /// The game speed's key (`core:speed/quick`; empty, and absent in the
    /// logs of before speeds: Standard).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub speed: String,
    /// A bare world: no city-states and no barbarians (a training ground's
    /// minimal scene, `grounds::Grounds::Battle::minimal`; absent: the
    /// size's city-states and normal barbarians, as before).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bare: bool,
}

impl LogSpec {
    /// The file stem of the game's log and shard:
    /// `<seed>-<suite>-<script>-<size>`.
    pub fn name(&self) -> String {
        format!("{}-{}-{}-{}", self.seed, self.suite, self.script, self.size)
    }

    pub fn game(&self) -> Result<GameSpec, String> {
        let script = MapScript::from_key(&self.script).ok_or_else(|| format!("unknown map script {}", self.script))?;
        let size = size_from_name(&self.size).ok_or_else(|| format!("unknown world size {}", self.size))?;
        let mut spec = GameSpec::new(script, size, self.seed).max_rounds(self.rounds).speed(&self.speed);
        spec.difficulty = self.difficulty.clone();
        if self.bare {
            spec.city_states = 0;
            spec.barbarians = classic_rules::world::barbarians::BarbarianMode::None;
        }
        Ok(spec)
    }
}

/// The first `games` games of `suite` in solo mode from seed offset
/// `offset` on (offset after offset), the round limit `rounds` or the
/// suite's, played by the net's base (`NET_BASE`, the expert it learns
/// from); `suite_specs_with` names another expert.
pub fn suite_specs(suite: &Suite, games: usize, offset: u64, rounds: Option<u32>) -> Vec<LogSpec> {
    suite_specs_with(suite, games, offset, rounds, NET_BASE)
}

/// `suite_specs` played by `variant` on every seat (kept in each spec).
/// The rows are read with `NET_BASE` whoever played (`features`).
pub fn suite_specs_with(suite: &Suite, games: usize, offset: u64, rounds: Option<u32>, variant: Variant) -> Vec<LogSpec> {
    let mut out = Vec::new();
    let mut at = offset;
    while out.len() < games {
        for m in suite.matches(Mode::Solo, at) {
            if out.len() == games {
                break;
            }
            out.push(LogSpec {
                suite: suite.name.to_owned(),
                seed: m.seed,
                script: m.map.script.to_owned(),
                size: m.map.size.to_owned(),
                rounds: rounds.unwrap_or(suite.rounds),
                difficulty: "core:handicap/prince".to_owned(),
                variant: variant.name().to_owned(),
                speed: String::new(),
                bare: false,
            });
        }
        at += 1;
    }
    out
}

/// A new game of `spec` set up as the AI driver sets it up (its
/// difficulty, every player the AI's) — the world a log starts from.
pub fn start(rules: &CompiledRuleset, spec: &GameSpec) -> GameState {
    let mut game = crate::record::new_game(rules, spec);
    AiDriver::new(AiSettings::for_rules(rules, &spec.difficulty), Vec::new()).configure_game(&mut game);
    game
}

/// A played game's log.
#[derive(Debug, Clone)]
pub struct PlayedLog {
    /// The kernel's command journal: header, commands, round checkpoints.
    pub journal: Vec<u8>,
    pub rounds: u32,
    pub hash: u64,
}

/// Plays `spec` to its end (or its last round) with `ClassicMind` of
/// `variant` on every seat and the command journal on.
pub fn play_log(rules: &CompiledRuleset, spec: &LogSpec) -> Result<PlayedLog, String> {
    let variant = Variant::from_name(&spec.variant).ok_or_else(|| format!("unknown variant {}", spec.variant))?;
    let game_spec = spec.game()?;
    let mut game = start(rules, &game_spec);
    let mut journal = game.begin_journal(rules);
    let settings = AiSettings::for_rules(rules, &game_spec.difficulty);
    let mut driver = kernel::ai::driver::AiDriver::<Classic, ClassicMind>::with_mind(settings, Vec::new(), ClassicMind::new(variant));
    driver.set_trace_capacity(0);
    while !game.is_over() && game.view().auth.turn().round <= spec.rounds {
        let report = driver.run(&mut game, rules, u32::MAX);
        if !report.round_done && !game.is_over() {
            return Err(format!("round {} waits for a human", game.view().auth.turn().round));
        }
        journal.extend(game.take_journal());
    }
    journal.extend(game.take_journal());
    Ok(PlayedLog { journal, rounds: game.view().auth.turn().round, hash: authoritative_hash(&game) })
}

/// Writes `<dir>/<name>.aurlog` (the journal) and `<dir>/<name>.json` (the
/// spec); returns the journal's path.
pub fn write_log(dir: &Path, spec: &LogSpec, log: &PlayedLog) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.aurlog", spec.name()));
    std::fs::write(&path, &log.journal)?;
    let json = serde_json::to_vec_pretty(spec).map_err(io::Error::other)?;
    std::fs::write(dir.join(format!("{}.json", spec.name())), json)?;
    Ok(path)
}

/// The spec and the journal of the log at `path` (the `.aurlog` or the
/// `.json` beside it).
pub fn read_log(path: &Path) -> Result<(LogSpec, Vec<u8>), String> {
    let json = path.with_extension("json");
    let journal = path.with_extension("aurlog");
    let spec: LogSpec = serde_json::from_slice(&std::fs::read(&json).map_err(|e| format!("{}: {e}", json.display()))?).map_err(|e| format!("{}: {e}", json.display()))?;
    let bytes = std::fs::read(&journal).map_err(|e| format!("{}: {e}", journal.display()))?;
    Ok((spec, bytes))
}

/// One player's final standing.
#[derive(Debug, Clone, Serialize)]
pub struct FinalStanding {
    pub player: u8,
    pub score: i64,
    /// Win 1, the best score at the end without a winner shared, else 0.
    pub points_share: f64,
    /// 1 = first (`bench::play::rank`'s order: the winner, the living by
    /// score, the eliminated later out higher).
    pub rank: u32,
    pub alive: bool,
    pub victory: Option<&'static str>,
}

/// What the exporter counted.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Counts {
    pub player_turns: u64,
    pub exported_turns: u64,
    /// Decisions kept, by head name.
    pub decisions: BTreeMap<String, u64>,
    /// Decisions dropped (the expert's command not among the candidates),
    /// by reason.
    pub dropped: BTreeMap<String, u64>,
    /// Long marches placed (`resolve_march`): on a point of interest
    /// exactly; as the move to its destination reached this turn; as the
    /// move to the tile its one step reached; on the nearest point of
    /// interest within `MARCH_NEAR` of its destination.
    pub march_exact: u64,
    pub march_reachable: u64,
    pub march_first_step: u64,
    pub march_near: u64,
    /// Unit decisions that are waits (moves left at the end of the turn).
    pub waits: u64,
    /// Religion decisions that keep the faith (a purchase was possible).
    pub faith_saves: u64,
    /// Citizen decisions whose choice keeps the turn's starting setting.
    pub citizens_keep: u64,
    /// The expert's citizen commands (`SetCityFocus`, `SetAvoidGrowth`,
    /// `SetSpecialists`): placed as the end-of-turn settings.
    pub citizen_commands: u64,
    /// Foundings whose picks assemble to the expert's command again, and
    /// those that do not (0 is the invariant).
    pub religion_assembled: u64,
    pub religion_assembly_mismatch: u64,
    /// Assembled foundings the rules refuse in the world before the command
    /// (0 is the invariant: the expert's command ran).
    pub religion_assembled_refused: u64,
    /// Foundings whose religion is not the first free one.
    pub religion_other_identity: u64,
    /// A city's further production orders of a turn after its first (the
    /// put-back of the old queue): not decisions.
    pub production_followups: u64,
    /// Commands of nations no head covers, by variant.
    pub other_commands: BTreeMap<String, u64>,
    /// Candidates over all decisions, and the most of one decision, by head.
    pub candidates: BTreeMap<String, u64>,
    pub max_candidates: BTreeMap<String, u64>,
    /// Commands replayed and round hashes checked.
    pub commands: u64,
    pub checkpoints: u64,
    /// With hindsight marches only (`features_with`): absent otherwise, so
    /// the `meta.json` of a plain export stays as it was.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub hindsight: Option<HindsightCounts>,
}

/// What the hindsight labels did (`relabel_hindsight`).
#[derive(Debug, Clone, Default, Serialize)]
pub struct HindsightCounts {
    /// Move decisions relabelled as the march to where the unit stopped
    /// moving (or the nearest point of interest within `MARCH_NEAR` of it).
    pub hindsight_relabelled: u64,
    /// Move decisions of a run that keep their move: the unit reaches its
    /// stop this turn, no march candidate is near the stop, or the unit is
    /// gone before it stopped.
    pub hindsight_kept_move: u64,
    /// Expert marches far from every point of interest at their command,
    /// placed after all on the march to where the unit stopped (else
    /// dropped as before).
    pub hindsight_recovered: u64,
}

/// The arrays of one game's shard (ai-net-data.md).
#[derive(Debug, Clone, Default)]
pub struct Shard {
    pub name: String,
    pub spec: Option<LogSpec>,
    pub width: u32,
    pub height: u32,
    pub players: u32,
    pub final_round: u32,
    pub neighbors: Vec<[i32; 6]>,
    pub region: Vec<i32>,
    pub regions: usize,
    /// Round, player, first entity row, entity count, entity rows at the
    /// turn's start (format 8: the rows the playing net reads, the rest
    /// appended as the turn named them).
    pub turns: Vec<[i32; 5]>,
    pub tiles: Vec<i16>,
    pub globals: Vec<i16>,
    pub entities: Vec<i16>,
    pub entity_meta: Vec<[i32; 4]>,
    /// Player, first turn row, turn count: consecutive turns of one player.
    pub windows: Vec<[i32; 3]>,
    /// Training only, never an input of the playing mind: the turn's
    /// future (`AUX`) and the truth behind the fog (`TRUTH`), per turn.
    pub aux: Vec<i16>,
    pub truth: Vec<i16>,
    pub decisions: Vec<[i32; 6]>,
    pub candidates: Vec<i16>,
    pub candidate_meta: Vec<[i32; 4]>,
    /// Format 10, training only: per candidate row its `encode::A` targets
    /// (`candidate_targets.npy`; written when every row has them).
    pub candidate_targets: Vec<i16>,
    /// Format 7: per turn its first event row and event count; the event
    /// rows (`encode::V`) and their meta (kind, tile, actor entity row
    /// absolute in the shard, parameter).
    pub turn_events: Vec<[i32; 2]>,
    pub events: Vec<i16>,
    pub event_meta: Vec<[i32; 4]>,
    pub standings: Vec<FinalStanding>,
    pub counts: Counts,
    pub c_global: usize,
}

impl Shard {
    pub fn tile_count(&self) -> usize {
        self.neighbors.len()
    }
}

/// Whether the player-turn is in the 1-in-`every` sample (a hash of the
/// seed, the round and the player).
pub fn sampled(seed: u64, round: u32, player: PlayerId, every: u32) -> bool {
    every <= 1 || mix(seed ^ (u64::from(round) << 8) ^ u64::from(player.0)).is_multiple_of(u64::from(every))
}

fn mix(mut z: u64) -> u64 {
    z ^= 0x9e37_79b9_7f4a_7c15;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Which player-turns of the nations a shard keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// A deterministic 1-in-N of the player-turns (`sampled`); 1: all. The
    /// windows are the runs of consecutive turns of one player.
    Sample(u32),
    /// `per_player` windows of `length` consecutive turns of each nation
    /// (`choose_windows`); a nation with fewer turns than that keeps as many
    /// whole windows as fit, at least one of all its turns.
    Windows { length: u32, per_player: u32 },
}

/// The windows of one player with `turns` turns: `(first, length)` as
/// indices into its turns in round order. The turns are cut into as many
/// equal segments as windows, and each window lies in its own segment at
/// an offset drawn from the seed and the player: windows never overlap and
/// spread over the game.
pub fn choose_windows(seed: u64, player: PlayerId, turns: usize, length: usize, per_player: usize) -> Vec<(usize, usize)> {
    if turns == 0 || length == 0 || per_player == 0 {
        return Vec::new();
    }
    if turns <= length {
        return vec![(0, turns)];
    }
    let count = per_player.min(turns / length).max(1);
    let segment = turns / count;
    (0..count)
        .map(|i| {
            let room = (segment - length + 1) as u64;
            let offset = mix(seed ^ (u64::from(player.0) << 32) ^ ((i as u64) << 40)) % room;
            (i * segment + offset as usize, length)
        })
        .collect()
}

/// The turn being exported.
struct Open {
    round: u32,
    player: PlayerId,
    /// None: a turn outside the selection (or not a nation's).
    turn: Option<Turn>,
    /// The truth behind the fog at the turn's start (`truth_row`).
    truth: Vec<i16>,
    /// Its decisions: head, actor row of the turn (`-1`), candidates (target
    /// rows of the turn), the expert's index.
    decisions: Vec<(i32, i32, Vec<Candidate>, usize)>,
    /// Cities that made their production decision this turn.
    produced: BTreeSet<(u32, u32)>,
    /// Per own city at the turn's start its citizen settings' candidates
    /// (`encode::citizen_candidates`), placed at the end of the turn.
    citizens: Vec<(kernel::ids::CityHandle, Vec<Vec<Candidate>>)>,
}

/// An exported player-turn, before the shard's order is known.
pub(crate) struct Kept {
    pub(crate) round: u32,
    pub(crate) player: PlayerId,
    pub(crate) turn: Turn,
    pub(crate) truth: Vec<i16>,
    pub(crate) decisions: Vec<(i32, i32, Vec<Candidate>, usize)>,
}

pub(crate) fn bump(map: &mut BTreeMap<String, u64>, key: &str) {
    *map.entry(key.to_owned()).or_default() += 1;
}

fn variant_name(command: &Command) -> String {
    let text = format!("{command:?}");
    text.split([' ', '{', '(']).next().unwrap_or("").to_owned()
}

/// What the replay saw of one command of a nation.
enum Placed {
    /// The decision: head, actor row of the turn (`-1` none), candidates,
    /// the expert's index.
    Decision(i32, i32, Vec<Candidate>, usize),
    Dropped(String),
    /// Not a decision (another kind of command, a follow-up order).
    Other,
    /// Several decisions of one command (a founding's belief slots).
    Several(Vec<Placed>),
    /// A long march to no point of interest: placed once it has run
    /// (`resolve_march`).
    March { actor: i32, candidates: Vec<Candidate>, unit: kernel::ids::UnitHandle, destination: TileId },
    /// With hindsight marches: a long march far from every point of
    /// interest, kept with no choice (`PENDING`) until the unit stops
    /// (`relabel_hindsight`), dropped when that places it nowhere.
    Pending(i32, Vec<Candidate>),
}

/// The choice of a decision that waits for its run's end (`Placed::Pending`).
const PENDING: usize = usize::MAX;

/// The drop reason of a long march far from every point of interest.
const MARCH_FAR: &str = "march_far_from_points_of_interest";

/// The codec tag of `MoveUnit` among the unit candidates.
const MOVE_TAG: i32 = 0;

/// One move command of a unit (`MoveUnit`, `MarchTo`) for the hindsight
/// labels.
struct Step {
    /// The round of the player-turn.
    round: u32,
    /// Its decision: the kept turn's index and the decision's index in it
    /// (None: the turn is not exported, or the command was dropped).
    at: Option<(usize, usize)>,
    /// The unit's tile after the command ran (None: it is gone).
    tile: Option<TileId>,
}

/// The runs of moves of every unit of the nations, over the whole game.
#[derive(Default)]
struct Runs {
    /// Per unit (handle index, generation) its moves since it last stopped.
    open: BTreeMap<(u32, u32), Vec<Step>>,
    /// Ended runs: the moves and the tile where the unit stopped (None: the
    /// unit is gone).
    done: Vec<(Vec<Step>, Option<TileId>)>,
}

impl Runs {
    fn key(unit: kernel::ids::UnitHandle) -> (u32, u32) {
        (unit.index, unit.generation)
    }

    /// The unit stops moving on `tile`: its run, if any, ends there.
    fn stop(&mut self, unit: kernel::ids::UnitHandle, tile: Option<TileId>) {
        if let Some(steps) = self.open.remove(&Self::key(unit)) {
            self.done.push((steps, tile));
        }
    }

    /// Whether the unit made a move in the player-turn of `round`.
    fn moved_in(&self, unit: kernel::ids::UnitHandle, round: u32) -> bool {
        self.open.get(&Self::key(unit)).and_then(|s| s.last()).is_some_and(|s| s.round == round)
    }

    /// Before a command of a nation's turn: a unit command other than a
    /// move ends the unit's run on its tile; the end of the turn ends the
    /// run of every unit that still has moves and made no move this turn (it
    /// waited all turn; a unit that moved and has moves left keeps its run:
    /// the next turn tells whether it moves on).
    fn before(&mut self, game: &GameState, command: &Command, round: u32, player: PlayerId) {
        let auth = game.view().auth;
        match acting_unit(command) {
            Some(_) if matches!(command, Command::MoveUnit { .. } | Command::MarchTo { .. }) => {}
            Some(unit) => self.stop(unit, auth.units().get(unit).map(|u| u.core.tile)),
            None if matches!(command, Command::EndTurn { .. }) => {
                for handle in auth.indexes().units_of(player).iter() {
                    let Some(u) = auth.units().get(*handle) else { continue };
                    if u.core.moves_left > 0 && !self.moved_in(*handle, round) {
                        self.stop(*handle, Some(u.core.tile));
                    }
                }
            }
            None => {}
        }
    }

    /// At the end of the game: every run still open ends where its unit
    /// stands.
    fn finish(&mut self, game: &GameState) {
        let auth = game.view().auth;
        for ((index, generation), steps) in std::mem::take(&mut self.open) {
            let tile = auth.units().get(kernel::ids::UnitHandle { index, generation }).map(|u| u.core.tile);
            self.done.push((steps, tile));
        }
    }
}

/// Hindsight march labels: every ended run of moves of a unit with its stop
/// `T` known relabels the move decisions of its turns that do not reach `T`
/// as the march candidate to `T`, or to the nearest point of interest within
/// `MARCH_NEAR` of it (`encode::place_march`); a turn whose last move reaches
/// `T` keeps its moves (a plain move), as does a decision with no such
/// march. A march labelled as a march keeps its label. A pending march (far
/// from every point of interest at its command) is placed the same way (the
/// move to `T` in the turn it reaches it), else dropped.
fn relabel_hindsight(kept: &mut [Kept], runs: Runs, dims: classic_rules::math::MapDimensions, counts: &mut Counts) {
    let mut h = HindsightCounts::default();
    for (steps, stop) in runs.done {
        let mut last: BTreeMap<u32, Option<TileId>> = BTreeMap::new();
        for s in &steps {
            last.insert(s.round, s.tile);
        }
        for s in &steps {
            let Some((k, d)) = s.at else { continue };
            let (_, _, candidates, choice) = &mut kept[k].decisions[d];
            let reached = stop.is_some() && last.get(&s.round).copied().flatten() == stop;
            let march = stop.filter(|_| !reached).and_then(|t| encode::place_march(candidates, t, dims)).map(|(i, _)| i);
            if *choice == PENDING {
                let placed = match stop {
                    Some(t) if reached => candidates.iter().position(|c| c.meta[0] == MOVE_TAG && c.meta[1] == t.0 as i32),
                    _ => march,
                };
                if let Some(at) = placed {
                    *choice = at;
                    h.hindsight_recovered += 1;
                    count_decision(counts, head::UNIT, candidates.len());
                }
                continue;
            }
            if candidates[*choice].meta[0] != MOVE_TAG {
                continue;
            }
            match march {
                Some(at) => {
                    *choice = at;
                    h.hindsight_relabelled += 1;
                }
                None => h.hindsight_kept_move += 1,
            }
        }
    }
    for k in kept.iter_mut() {
        let before = k.decisions.len();
        k.decisions.retain(|d| d.3 != PENDING);
        for _ in k.decisions.len()..before {
            bump(&mut counts.dropped, MARCH_FAR);
        }
    }
    counts.hindsight = Some(h);
}

/// The game of a log, checked against the journal's start.
fn begin(rules: &CompiledRuleset, spec: &LogSpec, journal: &[u8]) -> Result<GameState, String> {
    let game = start(rules, &spec.game()?);
    let (header, _) = JournalHeader::read(journal).map_err(|e| e.to_string())?;
    if header.rules != rules.hash() {
        return Err("the log was recorded with other rules".to_owned());
    }
    if header.world != authoritative_hash(&game) {
        return Err(format!("the log starts from world {:016x}, the spec generates {:016x}", header.world, authoritative_hash(&game)));
    }
    Ok(game)
}

/// Replays the log and calls `before` with every command of a player-turn
/// before it runs (and the world it meets) and `after` once it ran; checks
/// every round's hash. Returns the final world, the hashes checked and the
/// commands replayed. `edit` is called before every command with the world
/// and the commands replayed so far: changes made outside the commands (an
/// E2 game's given technologies, `selfplay::Gift`; a training ground's
/// scene, `grounds::Edit`) are made again there.
pub fn replay_with<B>(
    rules: &CompiledRuleset,
    spec: &LogSpec,
    journal: &[u8],
    mut edit: impl FnMut(&mut GameState, u64),
    mut before: impl FnMut(&GameState, &Command, u32, PlayerId) -> B,
    mut after: impl FnMut(&GameState, B),
) -> Result<(GameState, u64, u64), String> {
    let mut game = begin(rules, spec, journal)?;
    let (_, mut reader) = JournalHeader::read(journal).map_err(|e| e.to_string())?;
    let mut record = 0usize;
    let mut checked = 0;
    let mut commands = 0;
    let mut edited = None;
    while !reader.is_at_end() {
        record += 1;
        let rest = &journal[journal.len() - reader.remaining()..];
        let next = match take_record::<Classic>(&mut reader, game.view().auth) {
            Ok(next) => next,
            Err(CodecError::Truncated) => break,
            // The command may name a unit an edit made before it (a training
            // ground's armies, `grounds::Edit`): the edits first, then again.
            Err(_) if edited != Some(commands) => {
                edit(&mut game, commands);
                edited = Some(commands);
                reader = kernel::persistence::codec::Reader::new(rest, reader.version());
                match take_record::<Classic>(&mut reader, game.view().auth) {
                    Ok(next) => next,
                    Err(error) => return Err(format!("record {record} is damaged ({error:?})")),
                }
            }
            Err(error) => return Err(format!("record {record} is damaged ({error:?})")),
        };
        let command = match next {
            Record::Command(command) => command,
            Record::Checkpoint { round, hash } => {
                let now = authoritative_hash(&game);
                if now != hash {
                    return Err(format!("diverged after round {round} (record {record}): world {now:016x}, recorded {hash:016x}"));
                }
                checked += 1;
                continue;
            }
            Record::CommandAs { .. } | Record::Window(_) => return Err(format!("record {record}: a seat window in an AI game")),
        };
        if edited != Some(commands) {
            edit(&mut game, commands);
            edited = Some(commands);
        }
        let auth = game.view().auth;
        let (round, player) = (auth.turn().round, auth.active_player());
        let carried = before(&game, &command, round, player);
        game.execute(rules, command).map_err(|e| format!("diverged in round {round} at record {record}: the command is refused ({e:?})"))?;
        commands += 1;
        after(&game, carried);
    }
    Ok((game, checked, commands))
}

/// Whose decisions a shard keeps, and the changes a game made outside its
/// commands (`features_of`): a self-play game of a bench mind
/// (`ai-selfplay --learner-mind`) read through its `.e2.json`
/// (`scope_of_game`).
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// The nations kept, each from its first round on (every nation from
    /// round 0 when `None`).
    pub players: Option<BTreeMap<u8, u32>>,
    /// A training ground's edits (`grounds::Edit`), made again before the
    /// command at their position.
    pub edits: Vec<crate::grounds::Edit>,
    /// The technologies given at a mid-game start (`selfplay::Gift`), made
    /// again before the command at its position.
    pub gift: Option<crate::selfplay::Gift>,
    /// The search teacher's defer choice (`search::SearchStats`, `.e2.json`
    /// `search`): `deferrals` are labelled as unit decisions that chose the
    /// defer candidate, `offered` decisions get it appended (format 9).
    pub search: Option<crate::search::SearchStats>,
}

impl Scope {
    fn keeps(&self, player: PlayerId, round: u32) -> bool {
        self.players.as_ref().is_none_or(|p| p.get(&player.0).is_some_and(|first| round >= *first))
    }

    fn edit(&self, game: &mut GameState, rules: &CompiledRuleset, commands: u64) {
        if let Some(gift) = self.gift.as_ref().filter(|g| g.position == commands) {
            crate::selfplay::apply_gift(game, rules, gift);
        }
        crate::grounds::apply_at(game, rules, &self.edits, commands);
    }
}

/// The scope of a self-play game from its `.e2.json`: its edits and gift,
/// and with `minds` the seats one of those minds played. A learner's seat
/// (`ai-selfplay --learner-mind`) is kept from the round the learner took
/// it: a training ground's war round (`ground.battle.start_round`) or a
/// mid-game start (`from_round`); before it `standard` or the source log
/// played the seat. Every other seat is kept from round 0.
pub fn scope_of_game(game: &serde_json::Value, minds: Option<&[String]>) -> Result<Scope, String> {
    let edits = crate::grounds::edits_of(game)?;
    let gift = crate::e2::gift_of(game)?;
    let players = match minds {
        None => None,
        Some(minds) => {
            let seats = game["seats"].as_array().ok_or("the sidecar has no seats")?;
            let taken = [&game["ground"]["battle"]["start_round"], &game["from_round"]].into_iter().filter_map(|r| r.as_u64()).max().unwrap_or(0) as u32;
            Some(
                seats
                    .iter()
                    .filter(|s| s["mind"].as_str().is_some_and(|m| minds.iter().any(|x| x == m)))
                    .filter_map(|s| {
                        let player = s["player"].as_u64().and_then(|p| u8::try_from(p).ok())?;
                        Some((player, if s["learner"].as_bool() == Some(true) { taken } else { 0 }))
                    })
                    .collect(),
            )
        }
    };
    let search = match game.get("search") {
        None | Some(serde_json::Value::Null) => None,
        Some(s) => Some(serde_json::from_value(s.clone()).map_err(|e| format!("search: {e}"))?),
    };
    Ok(Scope { players, edits, gift, search })
}

/// The rounds each nation played, in order (a replay without features).
pub fn nation_turns(rules: &CompiledRuleset, spec: &LogSpec, journal: &[u8]) -> Result<BTreeMap<u8, Vec<u32>>, String> {
    nation_turns_in(rules, spec, journal, &Scope::default())
}

/// `nation_turns` of a game with changes outside its commands (`Scope`).
fn nation_turns_in(rules: &CompiledRuleset, spec: &LogSpec, journal: &[u8], scope: &Scope) -> Result<BTreeMap<u8, Vec<u32>>, String> {
    let mut turns: BTreeMap<u8, Vec<u32>> = BTreeMap::new();
    replay_with(
        rules,
        spec,
        journal,
        |game, commands| scope.edit(game, rules, commands),
        |game, _, round, player| {
            if game.view().auth.players().is_major(player) {
                let rounds = turns.entry(player.0).or_default();
                if rounds.last() != Some(&round) {
                    rounds.push(round);
                }
            }
        },
        |_, ()| {},
    )?;
    Ok(turns)
}

/// Replays `journal` from `spec`'s world and builds the shard of the
/// nations' decisions on the player-turns `selection` keeps. Any divergence
/// (a refused command, a wrong round hash) is an error.
pub fn features(rules: &CompiledRuleset, spec: &LogSpec, journal: &[u8], selection: Selection) -> Result<Shard, String> {
    features_with(rules, spec, journal, selection, false)
}

/// `features`, and with `hindsight_marches` the hindsight march labels
/// (`relabel_hindsight`): a unit's moves are labelled with the march to
/// where it stops moving, known once the game has run on. Only choices
/// change, never a row.
pub fn features_with(rules: &CompiledRuleset, spec: &LogSpec, journal: &[u8], selection: Selection, hindsight_marches: bool) -> Result<Shard, String> {
    features_of(rules, spec, journal, selection, hindsight_marches, &Scope::default())
}

/// `features_with` keeping only the decisions of the nations `scope` keeps,
/// each from its first round (all when `scope.players` is `None`): the
/// seats an expert played in a game against other minds, whose own choices
/// are no labels; a game's changes outside its commands (a training
/// ground's scene, a mid-game gift) made again where they were made.
pub fn features_of(rules: &CompiledRuleset, spec: &LogSpec, journal: &[u8], selection: Selection, hindsight_marches: bool, scope: &Scope) -> Result<Shard, String> {
    let chosen = |player: PlayerId, round: u32| scope.keeps(player, round);
    // The turns to keep, and the window each belongs to.
    let windows: Option<BTreeMap<(u8, u32), usize>> = match selection {
        Selection::Sample(_) => None,
        Selection::Windows { length, per_player } => {
            let mut keep = BTreeMap::new();
            let mut id = 0;
            for (player, rounds) in nation_turns_in(rules, spec, journal, scope)? {
                let rounds: Vec<u32> = rounds.into_iter().filter(|r| chosen(PlayerId(player), *r)).collect();
                for (first, len) in choose_windows(spec.seed, PlayerId(player), rounds.len(), length as usize, per_player as usize) {
                    for round in &rounds[first..first + len] {
                        keep.insert((player, *round), id);
                    }
                    id += 1;
                }
            }
            Some(keep)
        }
    };
    let keeps = |round: u32, player: PlayerId| {
        chosen(player, round)
            && match (&windows, selection) {
                (Some(keep), _) => keep.contains_key(&(player.0, round)),
                (None, Selection::Sample(every)) => sampled(spec.seed, round, player, every),
                (None, _) => true,
            }
    };
    let probe = begin(rules, spec, journal)?;
    let dims = probe.view().auth.map().dimensions;
    let (region, regions) = encode::region_table(dims);
    let mut shard = Shard {
        name: spec.name(),
        spec: Some(spec.clone()),
        width: dims.width,
        height: dims.height,
        players: probe.view().auth.players().count() as u32,
        neighbors: encode::neighbor_table(dims),
        region,
        regions,
        c_global: encode::global_width(rules),
        ..Shard::default()
    };
    drop(probe);
    // The net's base (`Variant::Net` plays on `NET_BASE`): the rows its mind reads at play,
    // whichever variant played the log (`spec.variant`, replayed by its commands alone).
    let mind = ClassicMind::new(NET_BASE);
    let settings = AiSettings::for_rules(rules, &spec.game()?.difficulty);
    let open: std::cell::RefCell<Option<Open>> = std::cell::RefCell::new(None);
    let kept: std::cell::RefCell<Vec<Kept>> = std::cell::RefCell::new(Vec::new());
    let counts = std::cell::RefCell::new(Counts::default());
    let summaries: std::cell::RefCell<Vec<(u32, BTreeMap<u8, Summary>)>> = std::cell::RefCell::new(Vec::new());
    let runs: std::cell::RefCell<Runs> = std::cell::RefCell::new(Runs::default());
    // Format 7: every nation's events, observed at each of its turn starts
    // from the first (as the net player observes its own at play).
    let trackers: std::cell::RefCell<BTreeMap<u8, Tracker>> = std::cell::RefCell::new(BTreeMap::new());
    // The position of the command being replayed (the search's deferral marks).
    let position = std::cell::Cell::new(0u64);
    let placed_before = |game: &GameState, command: &Command, round: u32, player: PlayerId| -> Option<Placed> {
        let mut open = open.borrow_mut();
        let mut counts = counts.borrow_mut();
        if open.as_ref().is_none_or(|o| (o.round, o.player) != (round, player)) {
            if let Some(done) = open.take() {
                close(&mut kept.borrow_mut(), &mut counts, done);
            }
            let major = game.view().auth.players().is_major(player);
            if major {
                counts.player_turns += 1;
            }
            if summaries.borrow().last().is_none_or(|(r, _)| *r != round) {
                summaries.borrow_mut().push((round, summarize(game, rules)));
            }
            let mut citizens = Vec::new();
            let turn = major
                .then(|| {
                    let obs = game.ai_observation(rules, player);
                    let mut trackers = trackers.borrow_mut();
                    let tracker = trackers.entry(player.0).or_default();
                    if !keeps(round, player) {
                        tracker.observe(&obs, rules);
                        return None;
                    }
                    let mut turn = Turn::encode_tracked(&obs, rules, tracker);
                    for city in &obs.own_cities {
                        citizens.push((city.handle, encode::citizen_candidates(&mut turn, &obs, city, rules)));
                    }
                    Some(turn)
                })
                .flatten();
            let truth = if turn.is_some() { truth_row(game, rules, player) } else { Vec::new() };
            *open = Some(Open { round, player, turn, truth, decisions: Vec::new(), produced: BTreeSet::new(), citizens });
        }
        let current = open.as_mut().expect("a turn is open");
        current.turn.as_ref()?;
        let at = position.get();
        if let Some(search) = &scope.search {
            for &(_, unit) in search.deferrals.iter().filter(|(p, _)| *p == at) {
                if let Some(placed) = deferral(game, rules, &mind, &settings, current, unit) {
                    record_placed(&mut counts, current, placed);
                }
            }
        }
        if matches!(command, Command::EndTurn { .. }) {
            waits(game, rules, &mind, &settings, current, &mut counts);
            faith_save(game, rules, current, &mut counts);
            citizens(game, current, &mut counts);
            None
        } else {
            let placed = place(game, rules, &mind, &settings, current, command, &mut counts);
            Some(offer_defer(game, scope, at, command, placed))
        }
    };
    // The decision a command placed (kept turn, index), for the hindsight labels.
    let placed_after = |game: &GameState, pending: Option<Placed>| -> Option<(usize, usize)> {
        let placed = pending?;
        let mut counts = counts.borrow_mut();
        let placed = match placed {
            Placed::March { actor, candidates, unit, destination } => resolve_march(game, actor, candidates, unit, destination, hindsight_marches, &mut counts),
            placed => placed,
        };
        let mut open = open.borrow_mut();
        let current = open.as_mut()?;
        let before = current.decisions.len();
        record_placed(&mut counts, current, placed);
        (current.decisions.len() == before + 1).then(|| (kept.borrow().len(), before))
    };
    let (game, checked, commands) = replay_with(
        rules,
        spec,
        journal,
        |game, commands| {
            position.set(commands);
            scope.edit(game, rules, commands)
        },
        |game, command, round, player| {
            let moved = (hindsight_marches && game.view().auth.players().is_major(player))
                .then(|| {
                    runs.borrow_mut().before(game, command, round, player);
                    acting_unit(command).filter(|_| matches!(command, Command::MoveUnit { .. } | Command::MarchTo { .. }))
                })
                .flatten();
            (moved, placed_before(game, command, round, player))
        },
        |game, (moved, pending)| {
            let at = placed_after(game, pending);
            if let Some(unit) = moved {
                let round = open.borrow().as_ref().map_or(0, |o| o.round);
                let tile = game.view().auth.units().get(unit).map(|u| u.core.tile);
                runs.borrow_mut().open.entry(Runs::key(unit)).or_default().push(Step { round, at, tile });
            }
        },
    )?;
    if let Some(done) = open.borrow_mut().take() {
        close(&mut kept.borrow_mut(), &mut counts.borrow_mut(), done);
    }
    let mut kept = kept.into_inner();
    let mut counts = counts.into_inner();
    if hindsight_marches {
        let mut runs = runs.into_inner();
        runs.finish(&game);
        relabel_hindsight(&mut kept, runs, game.view().auth.map().dimensions, &mut counts);
    }
    shard.counts = counts;
    shard.counts.commands = commands;
    shard.counts.checkpoints = checked;
    shard.final_round = game.view().auth.turn().round;
    shard.standings = standings(&game, rules);
    let mut summaries = summaries.into_inner();
    summaries.push((u32::MAX, summarize(&game, rules)));
    let standings = shard.standings.clone();
    let future = Future { summaries: &summaries, standings: &standings };
    assemble(&mut shard, kept, windows.as_ref(), &future);
    Ok(shard)
}

/// One nation at the start of a round, as the replay knows it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Summary {
    score: i64,
    techs: i64,
    cities: Vec<u64>,
    /// At war with a nation or a city-state (barbarians aside).
    at_war: bool,
}

pub(crate) fn summarize(game: &GameState, rules: &CompiledRuleset) -> BTreeMap<u8, Summary> {
    let auth = game.view().auth;
    let players = auth.players();
    let barbarian = |q: PlayerId| matches!(players.kind_of(q), kernel::world::player::PlayerKind::Module(classic_rules::world::player::ClassicKind::Barbarian));
    (0..players.count() as u8)
        .map(PlayerId)
        .filter(|p| players.is_major(*p) && players.is_alive(*p))
        .map(|p| {
            let mut cities: Vec<u64> = auth.indexes().cities_of(p).iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.id.0).collect();
            cities.sort_unstable();
            let at_war = (0..players.count() as u8).map(PlayerId).any(|q| q != p && players.is_alive(q) && !barbarian(q) && auth.diplomacy().at_war(p, q));
            (p.0, Summary { score: classic_rules::world::victory::score(auth, rules, p), techs: players.techs_of(p).len() as i64, cities, at_war })
        })
        .collect()
}

/// The `aux.npy` layout: a turn's future as the replay knows it. Training
/// only: never an input of the playing mind.
pub const AUX: [(&str, f64); 10] = [
    ("score_in_10", 1.0),
    ("score_in_50", 1.0),
    ("techs_in_50", 1.0),
    ("cities_in_10", 1.0),
    ("cities_in_50", 1.0),
    ("loses_city_within_10", 1.0),
    ("at_war_within_10", 1.0),
    ("final_points_share", 1000.0),
    ("final_rank", 1.0),
    ("victory", 1.0),
];

/// Nations of `truth.npy`: the most a world size seats.
pub const TRUTH_NATIONS: usize = 12;

/// The `truth.npy` layout: per nation slot (0 the player, then the others
/// by id) score, technologies, cities, citizens, military strength, gold,
/// science per turn. Training only (a privileged critic).
pub fn truth_fields() -> Vec<(String, f64)> {
    let per = [("score", 1.0), ("techs", 1.0), ("cities", 1.0), ("citizens", 1.0), ("military", 0.1), ("gold", 0.1), ("science", 1.0)];
    (0..TRUTH_NATIONS).flat_map(|i| per.iter().map(move |(name, scale)| (format!("nation{i}_{name}"), *scale))).collect()
}

/// The truth behind the fog at this moment for `player`'s turn: every
/// nation's standing, the player first, then the others by id; a dead
/// nation or an empty slot is zeros.
pub fn truth_row(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> Vec<i16> {
    let auth = game.view().auth;
    let players = auth.players();
    let others = (0..players.count() as u8).map(PlayerId).filter(|p| *p != player && players.is_major(*p));
    let mut out = vec![0i16; TRUTH_NATIONS * 7];
    for (slot, p) in std::iter::once(player).chain(others).take(TRUTH_NATIONS).enumerate() {
        if !players.is_alive(p) {
            continue;
        }
        let cities: Vec<_> = auth.indexes().cities_of(p).iter().filter_map(|c| auth.cities().get(*c)).collect();
        let military: i64 = auth.units().iter_cores().filter(|u| u.owner == p).map(|u| i64::from(kernel::ai::plan::unit_power(rules, u.kind, u.hp))).sum();
        let values = [
            classic_rules::world::victory::score(auth, rules, p),
            players.techs_of(p).len() as i64,
            cities.len() as i64,
            cities.iter().map(|c| i64::from(c.population)).sum(),
            military / 10,
            i64::from(players.treasury_of(p)) / 10,
            i64::from(classic_rules::derived::income::player_income(auth, rules, p).science),
        ];
        for (i, v) in values.into_iter().enumerate() {
            out[slot * 7 + i] = v.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16;
        }
    }
    out
}

/// What the replay knows of the game after a turn.
pub(crate) struct Future<'a> {
    /// Per round start (ascending), the end last (`u32::MAX`).
    pub(crate) summaries: &'a [(u32, BTreeMap<u8, Summary>)],
    pub(crate) standings: &'a [FinalStanding],
}

impl Future<'_> {
    /// The nation at the start of `round`, or at the end when the game ended
    /// sooner (or it is dead: an empty summary).
    fn at(&self, round: u32, player: PlayerId) -> Summary {
        let at = self.summaries.iter().find(|(r, _)| *r >= round).or(self.summaries.last());
        at.and_then(|(_, s)| s.get(&player.0)).cloned().unwrap_or_default()
    }

    /// The `AUX` row of `player`'s turn in `round`.
    fn row(&self, player: PlayerId, round: u32) -> Vec<i16> {
        let now = self.at(round, player);
        let window = |k: u32| (round + 1..=round + k).map(|r| self.at(r, player));
        let loses = window(10).any(|later| now.cities.iter().any(|c| later.cities.binary_search(c).is_err()));
        let war = now.at_war || window(10).any(|later| later.at_war);
        let standing = self.standings.iter().find(|s| s.player == player.0);
        let victory = match standing.and_then(|s| s.victory) {
            None => 0,
            Some("science") => 1,
            Some("domination") => 2,
            Some("culture") => 3,
            Some("diplomatic") => 4,
            Some(_) => 5,
        };
        let values = [
            self.at(round + 10, player).score,
            self.at(round + 50, player).score,
            self.at(round + 50, player).techs,
            self.at(round + 10, player).cities.len() as i64,
            self.at(round + 50, player).cities.len() as i64,
            i64::from(loses),
            i64::from(war),
            standing.map_or(0, |s| (s.points_share * 1000.0).round() as i64),
            standing.map_or(0, |s| i64::from(s.rank)),
            victory,
        ];
        values.into_iter().map(|v| v.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16).collect()
    }
}

/// A kept turn closes.
fn close(kept: &mut Vec<Kept>, counts: &mut Counts, open: Open) {
    let Some(turn) = open.turn else { return };
    counts.exported_turns += 1;
    kept.push(Kept { round: open.round, player: open.player, turn, truth: open.truth, decisions: open.decisions });
}

/// The shard's rows in window order: windows by player, then by their first
/// round; inside a window its turns in round order. A window is the turns
/// `windows` assigns it, or (sampling) a run of consecutive rounds of one
/// player.
pub(crate) fn assemble(shard: &mut Shard, mut kept: Vec<Kept>, windows: Option<&BTreeMap<(u8, u32), usize>>, future: &Future<'_>) {
    kept.sort_by_key(|k| (k.player.0, k.round));
    let mut groups: Vec<Vec<Kept>> = Vec::new();
    let mut last: Option<(u8, u32, usize)> = None;
    for k in kept {
        let id = windows.and_then(|w| w.get(&(k.player.0, k.round)).copied()).unwrap_or(usize::MAX);
        let joins = last.is_some_and(|(player, round, window)| player == k.player.0 && round + 1 == k.round && window == id);
        last = Some((k.player.0, k.round, id));
        if joins {
            groups.last_mut().expect("a group is open").push(k);
        } else {
            groups.push(vec![k]);
        }
    }
    for group in groups {
        shard.windows.push([i32::from(group[0].player.0), shard.turns.len() as i32, group.len() as i32]);
        for k in group {
            let entity_base = shard.entity_meta.len() as i32;
            let turn_row = shard.turns.len() as i32;
            let turn = k.turn;
            shard.turns.push([turn.round as i32, i32::from(turn.player.0), entity_base, turn.entity_count() as i32, turn.start_entity_count() as i32]);
            shard.tiles.extend_from_slice(&turn.tiles);
            shard.globals.extend_from_slice(&turn.global);
            shard.aux.extend_from_slice(&future.row(k.player, k.round));
            shard.truth.extend_from_slice(&k.truth);
            shard.entities.extend_from_slice(&turn.entities);
            shard.entity_meta.extend_from_slice(&turn.entity_meta);
            shard.turn_events.push([shard.event_meta.len() as i32, turn.event_count() as i32]);
            shard.events.extend_from_slice(&turn.events);
            shard.event_meta.extend(turn.event_meta.iter().map(|m| [m[0], m[1], if m[2] >= 0 { m[2] + entity_base } else { -1 }, m[3]]));
            for (head_code, actor, candidates, choice) in k.decisions {
                let first = shard.candidate_meta.len() as i32;
                let actor = if actor >= 0 { actor + entity_base } else { -1 };
                shard.decisions.push([turn_row, head_code, actor, first, candidates.len() as i32, choice as i32]);
                for c in candidates {
                    shard.candidates.extend_from_slice(&c.row);
                    // Format 10, training only: the unit candidates' targets (zeros elsewhere).
                    let mut targets = c.targets.clone();
                    targets.resize(encode::A::COUNT, 0);
                    shard.candidate_targets.extend_from_slice(&targets);
                    let mut meta = c.meta;
                    if meta[2] >= 0 {
                        meta[2] += entity_base;
                    }
                    shard.candidate_meta.push(meta);
                }
            }
        }
    }
}

pub(crate) fn count_decision(counts: &mut Counts, head_code: i32, candidates: usize) {
    let name = head::NAMES[head_code as usize];
    bump(&mut counts.decisions, name);
    *counts.candidates.entry(name.to_owned()).or_default() += candidates as u64;
    let max = counts.max_candidates.entry(name.to_owned()).or_default();
    *max = (*max).max(candidates as u64);
}

fn record_placed(counts: &mut Counts, open: &mut Open, placed: Placed) {
    match placed {
        Placed::Decision(head_code, actor, candidates, choice) => {
            count_decision(counts, head_code, candidates.len());
            open.decisions.push((head_code, actor, candidates, choice));
        }
        // Counted once `relabel_hindsight` places it.
        Placed::Pending(actor, candidates) => open.decisions.push((head::UNIT, actor, candidates, PENDING)),
        Placed::Dropped(reason) => bump(&mut counts.dropped, &reason),
        Placed::Several(all) => {
            for placed in all {
                record_placed(counts, open, placed);
            }
        }
        Placed::Other | Placed::March { .. } => {}
    }
}

/// A long march to no point of interest, after it ran: the move to its
/// destination when the unit reaches it this turn (a move candidate); else,
/// as the march moves the unit one step along its route, the move to the
/// tile it stepped on; else the march to the nearest point of interest
/// within `MARCH_NEAR` of its destination; else dropped (a march that did
/// not move the unit, to a tile far from every point of interest; with
/// `hindsight` such a march is pending: `relabel_hindsight` places it).
fn resolve_march(game: &GameState, actor: i32, candidates: Vec<Candidate>, unit: kernel::ids::UnitHandle, destination: TileId, hindsight: bool, counts: &mut Counts) -> Placed {
    let move_to = |tile: TileId| candidates.iter().position(|c| c.meta[0] == 0 && c.meta[1] == tile.0 as i32);
    if let Some(at) = move_to(destination) {
        counts.march_reachable += 1;
        return Placed::Decision(head::UNIT, actor, candidates, at);
    }
    let stepped = game.view().auth.units().get(unit).map(|u| u.core.tile);
    if let Some(at) = stepped.and_then(move_to) {
        counts.march_first_step += 1;
        return Placed::Decision(head::UNIT, actor, candidates, at);
    }
    match encode::place_march(&candidates, destination, game.view().auth.map().dimensions) {
        Some((at, _)) => {
            counts.march_near += 1;
            Placed::Decision(head::UNIT, actor, candidates, at)
        }
        None if stepped.is_none() => Placed::Dropped("march_unit_gone".to_owned()),
        None if hindsight => Placed::Pending(actor, candidates),
        None => Placed::Dropped(MARCH_FAR.to_owned()),
    }
}

/// The expert's command among the candidates of its head.
fn place(game: &GameState, rules: &CompiledRuleset, mind: &ClassicMind, settings: &AiSettings, open: &mut Open, command: &Command, counts: &mut Counts) -> Placed {
    let player = open.player;
    let turn = open.turn.as_mut().expect("a sampled turn");
    if let Some((founding, unit, religion, picks)) = encode::split_founding(command) {
        return founding_decisions(game, rules, player, turn, command, founding, unit, religion, &picks, counts);
    }
    if let Command::PurchaseWithFaith { city, item, .. } = *command {
        let obs = game.ai_observation(rules, player);
        let candidates = encode::faith_candidates(turn, &obs, rules);
        let tile = obs.own_cities.iter().find(|c| c.handle == city).map(|c| c.tile.0 as i32);
        let (code, id) = match item {
            classic_rules::ids::ProductionItem::Unit(k) => (tag::FAITH_UNIT, k.0),
            classic_rules::ids::ProductionItem::Building(k) => (tag::FAITH_BUILDING, k.0),
            classic_rules::ids::ProductionItem::Project(k) => (tag::FAITH_BUILDING, k.0),
        };
        return match candidates.iter().position(|c| c.meta[0] == code && Some(c.meta[1]) == tile && c.meta[3] == i32::from(id)) {
            Some(at) => Placed::Decision(head::RELIGION, -1, candidates, at),
            None => Placed::Dropped("faith_purchase_not_listed".to_owned()),
        };
    }
    if let Some(unit) = acting_unit(command) {
        let Some(near) = mind.sense_unit(game, rules, settings, player, unit) else {
            return Placed::Dropped("unit_not_seen".to_owned());
        };
        let actor = turn.own_unit_row(&near.unit, rules);
        let candidates = encode::unit_candidates_with_targets(turn, &near, rules);
        let choice = match *command {
            Command::MarchTo { destination, .. } => {
                let exact = candidates.iter().position(|c| c.meta[0] == tag::MARCH && c.meta[1] == destination.0 as i32);
                if exact.is_none() {
                    return Placed::March { actor, candidates, unit, destination };
                }
                counts.march_exact += 1;
                exact
            }
            _ if encode::blow(&near, command).is_some() => encode::place_blow(&near, &candidates, command).flatten(),
            _ => {
                let Some(code) = encode::unit_tag(command) else {
                    return Placed::Dropped(format!("unit_not_enumerated_{}", variant_name(command)));
                };
                let value = near.view.command_value(command);
                candidates.iter().position(|c| c.meta[0] == code && c.command.as_ref().is_some_and(|k| near.view.command_value(k) == value))
            }
        };
        return match choice {
            Some(at) => Placed::Decision(head::UNIT, actor, candidates, at),
            None => Placed::Dropped(format!("unit_not_listed_{}", variant_name(command))),
        };
    }
    match *command {
        Command::ChooseTechnology { technology: tech, .. } | Command::PlanResearch { target: tech, .. } => {
            let obs = game.ai_observation(rules, player);
            let candidates = encode::research_candidates(&obs, rules);
            match candidates.iter().position(|c| c.meta[3] == i32::from(tech.0)) {
                Some(at) => Placed::Decision(head::RESEARCH, -1, candidates, at),
                None => Placed::Dropped(format!("research_not_listed_{}", variant_name(command))),
            }
        }
        Command::EnqueueProduction { city, item, .. } => {
            if !open.produced.insert((city.index, city.generation)) {
                counts.production_followups += 1;
                return Placed::Other;
            }
            let obs = game.ai_observation(rules, player);
            let Some(own) = obs.own_cities.iter().find(|c| c.handle == city) else {
                return Placed::Dropped("production_city_not_seen".to_owned());
            };
            let actor = turn.own_city_row(own, &obs, rules);
            let candidates = encode::production_candidates(turn, &obs, own, rules);
            let (code, id) = match item {
                classic_rules::ids::ProductionItem::Unit(k) => (tag::PRODUCE_UNIT, k.0),
                classic_rules::ids::ProductionItem::Building(k) => (tag::PRODUCE_BUILDING, k.0),
                classic_rules::ids::ProductionItem::Project(k) => (tag::PRODUCE_PROJECT, k.0),
            };
            match candidates.iter().position(|c| c.meta[0] == code && c.meta[3] == i32::from(id)) {
                Some(at) => {
                    turn.note_order(city, item);
                    Placed::Decision(head::PRODUCTION, actor, candidates, at)
                }
                None => Placed::Dropped("production_not_listed".to_owned()),
            }
        }
        Command::AdoptPolicy { policy, .. } => policy_decision(game, rules, player, tag::ADOPT_POLICY, policy.0),
        Command::UnlockPolicyBranch { branch, .. } => policy_decision(game, rules, player, tag::UNLOCK_BRANCH, branch.0),
        Command::ChooseIdeology { ideology, .. } => policy_decision(game, rules, player, tag::CHOOSE_IDEOLOGY, ideology.0),
        Command::SetCityFocus { .. } | Command::SetAvoidGrowth { .. } | Command::SetSpecialists { .. } => {
            counts.citizen_commands += 1;
            Placed::Other
        }
        _ => {
            bump(&mut counts.other_commands, &variant_name(command));
            Placed::Other
        }
    }
}

fn policy_decision(game: &GameState, rules: &CompiledRuleset, player: PlayerId, code: i32, id: u16) -> Placed {
    let obs = game.ai_observation(rules, player);
    let candidates = encode::policy_candidates(&obs, rules);
    match candidates.iter().position(|c| c.meta[0] == code && c.meta[3] == i32::from(id)) {
        Some(at) => Placed::Decision(head::POLICY, -1, candidates, at),
        None => Placed::Dropped("policy_not_listed".to_owned()),
    }
}

/// A founding command as its single choices, one decision per belief slot
/// in slot order (`encode::founding_slots`), each among the beliefs legal
/// for the slot given the earlier picks; the picks assembled again must be
/// the expert's command (`encode::assemble_founding`), which is counted.
#[allow(clippy::too_many_arguments)]
fn founding_decisions(
    game: &GameState,
    rules: &CompiledRuleset,
    player: PlayerId,
    turn: &mut Turn,
    command: &Command,
    founding: encode::Founding,
    unit: Option<kernel::ids::UnitHandle>,
    religion: Option<classic_rules::rules::content::ReligionId>,
    picks: &[(i32, classic_rules::ids::BeliefId)],
    counts: &mut Counts,
) -> Placed {
    let obs = game.ai_observation(rules, player);
    let actor = unit.and_then(|u| obs.own_units.iter().find(|o| o.handle == u)).map_or(-1, |o| turn.own_unit_row(o, rules));
    let slots = encode::founding_slots(&obs.faith.overview, founding);
    let mut placed = Vec::new();
    let mut chosen: Vec<(i32, classic_rules::ids::BeliefId)> = Vec::new();
    for &slot in &slots {
        let Some(&(_, belief)) = picks.iter().find(|(s, _)| *s == slot) else {
            placed.push(Placed::Dropped("religion_slot_left_empty".to_owned()));
            continue;
        };
        let earlier: Vec<_> = chosen.iter().map(|(_, b)| *b).collect();
        let candidates = encode::belief_candidates(&obs, rules, slot, &earlier);
        placed.push(match candidates.iter().position(|c| c.meta[3] == i32::from(belief.0)) {
            Some(at) => Placed::Decision(head::RELIGION, actor, candidates, at),
            None => Placed::Dropped("religion_belief_not_listed".to_owned()),
        });
        chosen.push((slot, belief));
    }
    for (slot, _) in picks {
        if !slots.contains(slot) {
            placed.push(Placed::Dropped("religion_slot_not_offered".to_owned()));
        }
    }
    if religion.is_some() && religion != obs.faith.overview.free_religions.first().copied() {
        counts.religion_other_identity += 1;
    }
    match encode::assemble_founding(&obs, founding, unit, &chosen) {
        Some(again) if obs.view.command_value(&again) == obs.view.command_value(command) => {
            counts.religion_assembled += 1;
            if !obs.view.accepts(&again) {
                counts.religion_assembled_refused += 1;
            }
        }
        _ => counts.religion_assembly_mismatch += 1,
    }
    Placed::Several(placed)
}

/// The citizen settings of every city at the end of the turn, among the
/// candidates of its start: the expert's choice is the setting the city has
/// now (the current one kept when the expert issued nothing).
fn citizens(game: &GameState, open: &mut Open, counts: &mut Counts) {
    let auth = game.view().auth;
    for (city, groups) in std::mem::take(&mut open.citizens) {
        let Some(plan) = auth.cities().get(city).filter(|c| c.owner == open.player).map(|c| c.citizens.clone()) else { continue };
        for group in groups {
            let Some(param) = encode::citizen_param(&plan, &group) else { continue };
            match group.iter().position(|c| c.meta[3] == param) {
                Some(at) => {
                    if group[at].command.is_none() {
                        counts.citizens_keep += 1;
                    }
                    let actor = group[at].meta[2];
                    record_placed(counts, open, Placed::Decision(head::CITIZENS, actor, group, at));
                }
                None => bump(&mut counts.dropped, "citizens_not_listed"),
            }
        }
    }
}

/// At the end of a turn with faith in the pool: when anything may still be
/// bought with faith, the expert kept its faith (`FAITH_SAVE`).
fn faith_save(game: &GameState, rules: &CompiledRuleset, open: &mut Open, counts: &mut Counts) {
    let player = open.player;
    if game.view().auth.module().religion().player(player).faith == 0 {
        return;
    }
    let obs = game.ai_observation(rules, player);
    let turn = open.turn.as_mut().expect("a sampled turn");
    let candidates = encode::faith_candidates(turn, &obs, rules);
    if candidates.len() < 2 {
        return;
    }
    let at = candidates.len() - 1;
    counts.faith_saves += 1;
    record_placed(counts, open, Placed::Decision(head::RELIGION, -1, candidates, at));
}

/// The search teacher's deferral of the unit with persistent id `unit`
/// (`Scope::search`): a unit decision of the world before the command it
/// was taken before, the defer candidate last and chosen (format 9).
fn deferral(game: &GameState, rules: &CompiledRuleset, mind: &ClassicMind, settings: &AiSettings, open: &mut Open, unit: u64) -> Option<Placed> {
    let auth = game.view().auth;
    let handle = *auth.indexes().units_of(open.player).iter().find(|h| auth.units().get(**h).is_some_and(|u| u.core.id.0 == unit))?;
    let turn = open.turn.as_mut()?;
    let near = mind.sense_unit(game, rules, settings, open.player, handle)?;
    let actor = turn.own_unit_row(&near.unit, rules);
    let mut candidates = encode::unit_candidates_with_targets(turn, &near, rules);
    candidates.push(encode::defer_candidate());
    let at = candidates.len() - 1;
    Some(Placed::Decision(head::UNIT, actor, candidates, at))
}

/// A unit command the search teacher chose while it offered the defer
/// choice (`Scope::search`, `offered`): its decision gets the defer
/// candidate last, not chosen, as the net's own games offer it.
fn offer_defer(game: &GameState, scope: &Scope, at: u64, command: &Command, placed: Placed) -> Placed {
    let Some(search) = &scope.search else { return placed };
    let id = acting_unit(command).and_then(|h| game.view().auth.units().get(h)).map(|u| u.core.id.0);
    if !id.is_some_and(|id| search.offered.contains(&(at, id))) {
        return placed;
    }
    match placed {
        Placed::Decision(h, actor, mut candidates, choice) if h == head::UNIT => {
            candidates.push(encode::defer_candidate());
            Placed::Decision(h, actor, candidates, choice)
        }
        Placed::March { actor, mut candidates, unit, destination } => {
            candidates.push(encode::defer_candidate());
            Placed::March { actor, candidates, unit, destination }
        }
        other => other,
    }
}

/// Every unit of the player with moves left when it ends its turn: a
/// decision to wait (the expert gave it no further command), its
/// candidates those of the world before the end of the turn.
fn waits(game: &GameState, rules: &CompiledRuleset, mind: &ClassicMind, settings: &AiSettings, open: &mut Open, counts: &mut Counts) {
    let auth = game.view().auth;
    let player = open.player;
    let mut units: Vec<_> = auth
        .indexes()
        .units_of(player)
        .iter()
        .filter_map(|h| auth.units().get(*h).map(|u| (u.core.id, *h, u.core.moves_left)))
        .filter(|(_, _, moves)| *moves > 0)
        .collect();
    units.sort_by_key(|(id, _, _)| *id);
    for (_, handle, _) in units {
        let turn = open.turn.as_mut().expect("a sampled turn");
        let Some(near) = mind.sense_unit(game, rules, settings, player, handle) else { continue };
        let actor = turn.own_unit_row(&near.unit, rules);
        let candidates = encode::unit_candidates_with_targets(turn, &near, rules);
        let at = candidates.iter().position(|c| c.meta[0] == tag::WAIT).expect("waiting is always a candidate");
        counts.waits += 1;
        record_placed(counts, open, Placed::Decision(head::UNIT, actor, candidates, at));
    }
}

/// Every nation's final standing; the ranks as the bench ranks seats.
pub(crate) fn standings(game: &GameState, rules: &CompiledRuleset) -> Vec<FinalStanding> {
    let players = game.view().auth.players();
    let outcome = game.outcome();
    let winner = outcome.as_ref().map(|o| o.winner);
    let majors: Vec<PlayerId> = (0..players.count() as u8).map(PlayerId).filter(|p| players.is_major(*p)).collect();
    let scores: Vec<i64> = majors.iter().map(|p| standing::<ClassicBench>(game, rules, *p).score).collect();
    let alive = |p: PlayerId| players.is_alive(p);
    let best = majors.iter().zip(&scores).filter(|(p, _)| alive(**p)).map(|(_, s)| *s).max().unwrap_or(0);
    let tied = majors.iter().zip(&scores).filter(|(p, s)| alive(**p) && **s == best).count().max(1);
    let mut order: Vec<usize> = (0..majors.len()).collect();
    order.sort_by_key(|&i| (Some(majors[i]) != winner, !alive(majors[i]), -scores[i], majors[i].0));
    let mut out: Vec<FinalStanding> = majors
        .iter()
        .zip(&scores)
        .map(|(&p, &score)| {
            let points_share = match winner {
                Some(w) => f64::from(u8::from(w == p)),
                None if alive(p) && score == best => 1.0 / tied as f64,
                None => 0.0,
            };
            FinalStanding {
                player: p.0,
                score,
                points_share,
                rank: order.iter().position(|&i| majors[i] == p).unwrap_or(0) as u32 + 1,
                alive: alive(p),
                victory: outcome.as_ref().filter(|o| o.winner == p).map(|o| crate::spec::victory_name(o.kind)),
            }
        })
        .collect();
    out.sort_by_key(|s| s.player);
    out
}

/// One `.npy` file (format 1.0): `descr` `<i2` or `<i4`, C order.
pub(crate) fn npy(path: &Path, descr: &str, shape: &[usize], data: &[u8]) -> io::Result<()> {
    let dims = match shape {
        [n] => format!("({n},)"),
        _ => format!("({})", shape.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(", ")),
    };
    let mut header = format!("{{'descr': '{descr}', 'fortran_order': False, 'shape': {dims}, }}");
    let total = 10 + header.len() + 1;
    header.push_str(&" ".repeat((64 - total % 64) % 64));
    header.push('\n');
    let mut out = io::BufWriter::new(std::fs::File::create(path)?);
    out.write_all(b"\x93NUMPY\x01\x00")?;
    out.write_all(&(header.len() as u16).to_le_bytes())?;
    out.write_all(header.as_bytes())?;
    out.write_all(data)?;
    out.flush()
}

pub(crate) fn i16_bytes(values: &[i16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub(crate) fn i32_bytes<'a>(values: impl Iterator<Item = &'a i32>) -> Vec<u8> {
    values.flat_map(|v| v.to_le_bytes()).collect()
}

/// `meta.json` of a shard.
pub fn meta_json(shard: &Shard, rules: &CompiledRuleset) -> serde_json::Value {
    let layout = encode::layout(rules);
    let table = |fields: &[encode::Field]| fields.iter().map(|f| serde_json::json!({"name": f.name, "scale": f.scale.value()})).collect::<Vec<_>>();
    let spec = shard.spec.clone().unwrap_or(LogSpec {
        suite: String::new(),
        seed: 0,
        script: String::new(),
        size: String::new(),
        rounds: 0,
        difficulty: String::new(),
        variant: String::new(),
        speed: String::new(),
        bare: false,
    });
    serde_json::json!({
        "format": encode::FORMAT,
        "spec": {
            "suite": spec.suite,
            "seed": spec.seed,
            "script": spec.script,
            "size": spec.size,
            "width": shard.width,
            "height": shard.height,
            "players": shard.players,
            "rounds": spec.rounds,
            "final_round": shard.final_round,
            "variant": spec.variant,
            "difficulty": spec.difficulty,
        },
        "tiles": shard.tile_count(),
        "regions": shard.regions,
        "windows": shard.windows.len(),
        "layout": {
            "tile": table(&layout.tile),
            "global": table(&layout.global),
            "entity": table(&layout.entity),
            "candidate": table(&layout.candidate),
            "event": table(&layout.event),
            "aux": AUX.iter().map(|(name, scale)| serde_json::json!({"name": name, "scale": scale})).collect::<Vec<_>>(),
            "truth": truth_fields().iter().map(|(name, scale)| serde_json::json!({"name": name, "scale": scale})).collect::<Vec<_>>(),
            "candidate_targets": table(&encode::target_fields()),
        },
        "heads": head::NAMES,
        "event_kinds": classic_rules::ai::net_e2e::events::kind::NAMES,
        "tags": {
            "march": tag::MARCH, "wait": tag::WAIT, "defer": tag::DEFER, "research": tag::RESEARCH,
            "produce_unit": tag::PRODUCE_UNIT, "produce_building": tag::PRODUCE_BUILDING, "produce_project": tag::PRODUCE_PROJECT,
            "adopt_policy": tag::ADOPT_POLICY, "unlock_branch": tag::UNLOCK_BRANCH, "choose_ideology": tag::CHOOSE_IDEOLOGY,
            "pantheon": tag::PANTHEON, "found_founder": tag::FOUND_FOUNDER, "found_follower": tag::FOUND_FOLLOWER,
            "found_pantheon": tag::FOUND_PANTHEON, "found_bonus": tag::FOUND_BONUS, "enhance_follower": tag::ENHANCE_FOLLOWER,
            "enhance_enhancer": tag::ENHANCE_ENHANCER, "reformation": tag::REFORMATION, "faith_unit": tag::FAITH_UNIT,
            "faith_building": tag::FAITH_BUILDING, "faith_save": tag::FAITH_SAVE,
            "focus": tag::FOCUS, "avoid_growth": tag::AVOID_GROWTH, "specialists": tag::SPECIALISTS,
        },
        "standings": shard.standings,
        "counts": shard.counts,
    })
}

/// Writes the shard into `<dir>/<name>.shard/`; returns its directory and
/// its size in bytes.
pub fn write_shard(dir: &Path, shard: &Shard, rules: &CompiledRuleset) -> io::Result<(PathBuf, u64)> {
    let out = dir.join(format!("{}.shard", shard.name));
    std::fs::create_dir_all(&out)?;
    let t = shard.turns.len();
    let c_tile = encode::T::COUNT;
    npy(&out.join("neighbors.npy"), "<i4", &[shard.neighbors.len(), 6], &i32_bytes(shard.neighbors.iter().flatten()))?;
    npy(&out.join("region.npy"), "<i4", &[shard.region.len()], &i32_bytes(shard.region.iter()))?;
    npy(&out.join("turns.npy"), "<i4", &[t, 5], &i32_bytes(shard.turns.iter().flatten()))?;
    npy(&out.join("tiles.npy"), "<i2", &[shard.tiles.len() / c_tile, c_tile], &i16_bytes(&shard.tiles))?;
    npy(&out.join("globals.npy"), "<i2", &[t, shard.c_global], &i16_bytes(&shard.globals))?;
    npy(&out.join("entities.npy"), "<i2", &[shard.entity_meta.len(), encode::E::COUNT], &i16_bytes(&shard.entities))?;
    npy(&out.join("entity_meta.npy"), "<i4", &[shard.entity_meta.len(), 4], &i32_bytes(shard.entity_meta.iter().flatten()))?;
    npy(&out.join("windows.npy"), "<i4", &[shard.windows.len(), 3], &i32_bytes(shard.windows.iter().flatten()))?;
    npy(&out.join("aux.npy"), "<i2", &[t, AUX.len()], &i16_bytes(&shard.aux))?;
    npy(&out.join("truth.npy"), "<i2", &[t, truth_fields().len()], &i16_bytes(&shard.truth))?;
    npy(&out.join("decisions.npy"), "<i4", &[shard.decisions.len(), 6], &i32_bytes(shard.decisions.iter().flatten()))?;
    npy(&out.join("candidates.npy"), "<i2", &[shard.candidate_meta.len(), encode::C::COUNT], &i16_bytes(&shard.candidates))?;
    npy(&out.join("candidate_meta.npy"), "<i4", &[shard.candidate_meta.len(), 4], &i32_bytes(shard.candidate_meta.iter().flatten()))?;
    if shard.candidate_targets.len() == shard.candidate_meta.len() * encode::A::COUNT {
        npy(&out.join("candidate_targets.npy"), "<i2", &[shard.candidate_meta.len(), encode::A::COUNT], &i16_bytes(&shard.candidate_targets))?;
    }
    npy(&out.join("turn_events.npy"), "<i4", &[t, 2], &i32_bytes(shard.turn_events.iter().flatten()))?;
    npy(&out.join("events.npy"), "<i2", &[shard.event_meta.len(), encode::V::COUNT], &i16_bytes(&shard.events))?;
    npy(&out.join("event_meta.npy"), "<i4", &[shard.event_meta.len(), 4], &i32_bytes(shard.event_meta.iter().flatten()))?;
    let meta = serde_json::to_vec_pretty(&meta_json(shard, rules)).map_err(io::Error::other)?;
    std::fs::write(out.join("meta.json"), meta)?;
    let bytes = std::fs::read_dir(&out)?.filter_map(|e| e.ok()?.metadata().ok()).map(|m| m.len()).sum();
    Ok((out, bytes))
}

/// Runs `work` over `items` on `jobs` threads; results in item order.
pub fn parallel<T: Sync, R: Send>(items: &[T], jobs: usize, work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: std::sync::Mutex<Vec<Option<R>>> = std::sync::Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, items.len().max(1)) {
            scope.spawn(|| loop {
                let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(item) = items.get(at) else { break };
                let result = work(item);
                results.lock().expect("no thread panicked holding the results")[at] = Some(result);
            });
        }
    });
    results.into_inner().expect("no thread panicked holding the results").into_iter().map(|r| r.expect("every item ran")).collect()
}

/// Every nation at one round's start, for the `aux` rows of a shard
/// assembled after the game ([`recorded_shard`]).
#[derive(Debug, Clone)]
pub struct RoundSummary(BTreeMap<u8, Summary>);

/// The nations of `game` now (take one at every round's start).
pub fn round_summary(game: &GameState, rules: &CompiledRuleset) -> RoundSummary {
    RoundSummary(summarize(game, rules))
}

/// One player-turn recorded while the game was played (DAgger,
/// `classic_rules::ai::net_e2e::mind::RecordedTurn`): its rows, the truth at
/// its start (`truth_row`) and its labeled decisions (head, actor row of the
/// turn, candidates, the label's index).
pub struct RecordedTurn {
    pub round: u32,
    pub player: PlayerId,
    pub turn: Turn,
    pub truth: Vec<i16>,
    pub decisions: Vec<(i32, i32, Vec<Candidate>, usize)>,
}

/// The shard of turns recorded while `game` was played to its end (the
/// exporter's rows and windows: `windows` assigns each kept (player, round)
/// its window, as `features` does with `Selection::Windows`; without it a
/// window is a run of consecutive turns of a player; the `aux` rows from the
/// round summaries and the final standings).
#[allow(clippy::too_many_arguments)]
pub fn recorded_shard(rules: &CompiledRuleset, name: String, spec: Option<LogSpec>, game: &GameState, turns: Vec<RecordedTurn>, windows: Option<&BTreeMap<(u8, u32), usize>>, summaries: Vec<(u32, RoundSummary)>, mut counts: Counts) -> Shard {
    let dims = game.view().auth.map().dimensions;
    let (region, regions) = encode::region_table(dims);
    let mut shard = Shard {
        name,
        spec,
        width: dims.width,
        height: dims.height,
        players: game.view().auth.players().count() as u32,
        neighbors: encode::neighbor_table(dims),
        region,
        regions,
        c_global: encode::global_width(rules),
        final_round: game.view().auth.turn().round,
        standings: standings(game, rules),
        ..Shard::default()
    };
    let mut kept = Vec::with_capacity(turns.len());
    for t in turns {
        counts.exported_turns += 1;
        for (head_code, _, candidates, _) in &t.decisions {
            let name = head::NAMES[*head_code as usize];
            bump(&mut counts.decisions, name);
            *counts.candidates.entry(name.to_owned()).or_default() += candidates.len() as u64;
            let max = counts.max_candidates.entry(name.to_owned()).or_default();
            *max = (*max).max(candidates.len() as u64);
        }
        kept.push(Kept { round: t.round, player: t.player, turn: t.turn, truth: t.truth, decisions: t.decisions });
    }
    let mut summaries: Vec<(u32, BTreeMap<u8, Summary>)> = summaries.into_iter().map(|(r, s)| (r, s.0)).collect();
    summaries.push((u32::MAX, summarize(game, rules)));
    let standings = shard.standings.clone();
    let future = Future { summaries: &summaries, standings: &standings };
    assemble(&mut shard, kept, windows, &future);
    shard.counts = counts;
    shard
}
