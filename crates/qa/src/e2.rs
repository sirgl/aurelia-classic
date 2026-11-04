//! The shard of an E2 game (classic/docs/ai-net-data.md, «E2: the net's own
//! games»): the log of a self-play game (`selfplay`) is replayed and, at the
//! moment of each of the learner's decisions (`sample::DecisionRecord::position`,
//! the commands applied before it), the candidates are built again exactly as
//! the net player built them (the same functions on the same world, the
//! turn's rows grown in the same order), so the rows are byte for byte those
//! the net scored. The label of a decision is the net's own choice; the
//! behaviour log-probability, the accepted candidates, the value per turn and
//! the reward go to files beside the format-5 ones (`write`).
//!
//! A decision whose rebuilt candidates differ from the sidecar's (count, rows
//! hash, the chosen candidate's tag, tile and parameter) is dropped and
//! counted (`E2Counts::dropped`); 0 is the invariant. A net that defers
//! (format 9, `sample::Trace::defer`) had the defer candidate last in a
//! unit's decision while the unit had a deferral left in its turn
//! (`Trace::defer_offered`): rebuilt the same way, so a deferral is a
//! decision like any other (no command follows it in the log).

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::net_e2e::encode::{self, head, Candidate, Turn};
use classic_rules::ai::net_e2e::events::Tracker;
use classic_rules::ai::net_e2e::sample::{self, Trace};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::knowledge::observation::AIObservation;
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::ai::mind::AiMind;
use kernel::ids::{CityHandle, PlayerId, UnitHandle};
use serde::Serialize;

use crate::export::{self, Counts, Kept, LogSpec, Selection, Shard, Summary};

/// What the E2 export counted.
#[derive(Debug, Clone, Default, Serialize)]
pub struct E2Counts {
    /// The sidecar's player-turns and decisions.
    pub turns: u64,
    pub decisions: u64,
    /// Turns and decisions written (the selection's turns).
    pub exported_turns: u64,
    pub aligned: u64,
    /// Decisions with no candidate (recorded for the rows; not written).
    pub empty: u64,
    /// Decisions dropped, by reason: `candidate_count`, `rows`, `choice`
    /// (the rebuilt candidates differ from the sidecar's), `none_accepted`,
    /// `unit_not_seen`, `city_not_seen`.
    pub dropped: BTreeMap<String, u64>,
    /// Unit decisions with a command: the next command of the log is that
    /// command (sent), or the driver's last look withheld it (not sent).
    pub unit_commands_sent: u64,
    pub unit_commands_not_sent: u64,
    /// The replayed world's hash at the end (the game's, `GameMeta::hash`).
    pub hash: u64,
}

/// The E2 arrays of a shard, in the shard's decision, candidate and turn
/// order.
#[derive(Debug, Clone, Default)]
pub struct E2Arrays {
    /// Per decision: the behaviour log-probability of the choice.
    pub logp: Vec<f32>,
    /// Per decision: ordinal in the player-turn, accepted candidates, the
    /// chosen logit (Q8), position (commands applied before it).
    pub decisions: Vec<[i32; 4]>,
    /// Per candidate: 1 when the rules accepted it at play (inside the
    /// softmax), 0 when refused.
    pub accepted: Vec<u8>,
    /// Per turn: the value head's output (Q8); `i32::MIN` when not recorded.
    pub value: Vec<i32>,
    /// Per decision: the search's values of a valued decision
    /// (`search::DecisionValue`): Q of the choice, the mean and the best of
    /// the candidates valued, their count; NaN for a decision not valued.
    pub dvalues: Vec<[f32; 4]>,
}

/// A game's valued decisions (`.e2.json` `decision_values.values`) by
/// (player, round, ordinal in the turn): [Q chosen, Q mean, Q best, count].
pub type ValueMap = BTreeMap<(u8, u32, u32), [f32; 4]>;

/// The valued decisions of a game's `.e2.json` (empty without them).
pub fn values_of(game: &serde_json::Value) -> Result<ValueMap, String> {
    let Some(values) = game.get("decision_values").and_then(|v| v.get("values")) else { return Ok(ValueMap::new()) };
    let values: Vec<crate::search::DecisionValue> = serde_json::from_value(values.clone()).map_err(|e| format!("decision_values: {e}"))?;
    Ok(values.into_iter().map(|v| ((v.player, v.round, v.ordinal), [v.q_chosen, v.q_mean, v.q_best, v.q.len() as f32])).collect())
}

/// One kept turn's E2 rows, beside its `Kept`.
#[derive(Default)]
struct TurnE2 {
    value: i32,
    dvalues: Vec<[f32; 4]>,
    logp: Vec<f32>,
    decisions: Vec<[i32; 4]>,
    accepted: Vec<u8>,
}

/// The learner's turn being rebuilt.
struct Open {
    /// Its index in the trace.
    t: usize,
    /// None: outside the selection (not rebuilt).
    turn: Option<Turn>,
    truth: Vec<i16>,
    decisions: Vec<(i32, i32, Vec<Candidate>, usize)>,
    e2: TurnE2,
}

/// The replay's state.
struct Rebuild<'a> {
    rules: &'a CompiledRuleset,
    trace: &'a Trace,
    keep: Vec<bool>,
    /// (turn, decision) in the trace's order, with its position.
    events: Vec<(u32, usize, Option<usize>)>,
    next: usize,
    open: Option<Open>,
    kept: Vec<(Kept, TurnE2)>,
    mind: ClassicMind,
    settings: AiSettings,
    counts: Counts,
    e2: E2Counts,
    summaries: Vec<(u32, BTreeMap<u8, Summary>)>,
    /// Format 7: each learner's events, observed at every turn start of the
    /// sidecar (kept or not) from its first: the turns the net player began,
    /// whose tracker started empty with the net's first turn in the game.
    trackers: BTreeMap<u8, Tracker>,
    /// The game's valued decisions (`values_of`).
    values: ValueMap,
}

impl Rebuild<'_> {
    fn close(&mut self) {
        let Some(open) = self.open.take() else { return };
        let Some(turn) = open.turn else { return };
        let record = &self.trace.turns[open.t];
        let (round, player) = (record.round, PlayerId(record.player));
        self.counts.exported_turns += 1;
        self.e2.exported_turns += 1;
        self.kept.push((Kept { round, player, turn, truth: open.truth, decisions: open.decisions }, open.e2));
    }

    /// The events at `position` (the world before the command `command`, or
    /// the end of the game).
    fn at(&mut self, game: &GameState, position: u32, command: Option<&Command>) -> Result<(), String> {
        if let Some(&(p, t, _)) = self.events.get(self.next).filter(|(p, _, _)| *p < position) {
            return Err(format!("the decision of turn {t} at command {p} was passed (the sidecar does not fit the log)"));
        }
        let trace = self.trace;
        let mut obs: Option<(u8, AIObservation<'_>)> = None;
        let mut unit_command: Option<String> = None;
        while let Some(&(p, t, d)) = self.events.get(self.next) {
            if p != position {
                break;
            }
            self.next += 1;
            let record = &trace.turns[t];
            let player = PlayerId(record.player);
            if !self.keep[t] {
                // Outside the selection: nothing to rebuild but the events.
                if d.is_none() {
                    self.close();
                    let sight = game.ai_observation(self.rules, player);
                    self.trackers.entry(record.player).or_default().observe(&sight, self.rules);
                    self.open = Some(Open { t, turn: None, truth: Vec::new(), decisions: Vec::new(), e2: TurnE2::default() });
                }
                unit_command = None;
                continue;
            }
            if obs.as_ref().is_none_or(|(q, _)| *q != record.player) {
                obs = Some((record.player, game.ai_observation(self.rules, player)));
            }
            let sight = &obs.as_ref().expect("the observation").1;
            match d {
                None => {
                    self.close();
                    let tracker = self.trackers.entry(record.player).or_default();
                    let turn = Some(Turn::encode_tracked(sight, self.rules, tracker));
                    let truth = export::truth_row(game, self.rules, player);
                    let value = record.value.unwrap_or(i32::MIN);
                    self.open = Some(Open { t, turn, truth, decisions: Vec::new(), e2: TurnE2 { value, ..TurnE2::default() } });
                }
                Some(d) => {
                    unit_command = self.decision(game, sight, t, d)?;
                }
            }
        }
        if let Some(sent) = unit_command {
            if command.is_some_and(|c| format!("{c:?}") == sent) {
                self.e2.unit_commands_sent += 1;
            } else {
                self.e2.unit_commands_not_sent += 1;
            }
        }
        Ok(())
    }

    /// One decision rebuilt and checked; returns the chosen unit command
    /// (its text) when it is one, which the log's next command should be.
    fn decision(&mut self, game: &GameState, sight: &AIObservation<'_>, t: usize, d: usize) -> Result<Option<String>, String> {
        let trace = self.trace;
        let record = &trace.turns[t].decisions[d];
        let open = self.open.as_mut().filter(|o| o.t == t).ok_or_else(|| format!("a decision of turn {t} outside it"))?;
        let Some(turn) = open.turn.as_mut() else { return Ok(None) };
        let rules = self.rules;
        let player = PlayerId(trace.turns[t].player);
        let rebuilt: Result<(i32, Vec<Candidate>), &str> = match (i32::from(record.head), record.actor) {
            (head::UNIT, sample::Actor::Unit { index, generation }) => match self.mind.sense_unit(game, rules, &self.settings, player, UnitHandle { index, generation }) {
                Some(near) => {
                    let actor = turn.own_unit_row(&near.unit, rules);
                    let mut candidates = encode::unit_candidates(turn, &near, rules);
                    // Format 9: the defer candidate last while the unit had a deferral left.
                    if trace.defer_offered(t, d, kernel::ai::driver::DEFERS_PER_UNIT) {
                        candidates.push(encode::defer_candidate());
                    }
                    Ok((actor, candidates))
                }
                None => Err("unit_not_seen"),
            },
            (head::RESEARCH, _) => Ok((-1, encode::research_candidates(sight, rules))),
            (head::PRODUCTION, sample::Actor::City { index, generation, cleared }) => match sight.own_cities.iter().find(|c| c.handle == CityHandle { index, generation }) {
                Some(city) => {
                    let actor = turn.own_city_row(city, sight, rules);
                    let mut candidates = encode::production_candidates(turn, sight, city, rules);
                    if cleared {
                        for c in &mut candidates {
                            c.row[encode::C::ItemQueued as usize] = 0;
                        }
                    }
                    Ok((actor, candidates))
                }
                None => Err("city_not_seen"),
            },
            (head::POLICY, _) => Ok((-1, encode::policy_candidates(sight, rules))),
            (h, actor) => return Err(format!("a decision of head {h} by {actor:?}")),
        };
        let reason = match &rebuilt {
            Err(reason) => Some(*reason),
            Ok((_, c)) if c.len() != usize::from(record.candidates) => Some("candidate_count"),
            Ok((_, c)) if record.candidates == 0 && c.is_empty() => {
                self.e2.empty += 1;
                return Ok(None);
            }
            Ok((_, c)) if sample::rows_hash(c) != record.rows_hash => Some("rows"),
            Ok(_) if record.chosen.is_none() => Some("none_accepted"),
            Ok((_, c)) => {
                let at = usize::from(record.chosen.unwrap_or(0));
                let meta = c.get(at).map(|c| [c.meta[0], c.meta[1], c.meta[3]]);
                (meta != Some(record.chosen_meta)).then_some("choice")
            }
        };
        if let Some(reason) = reason {
            export::bump(&mut self.e2.dropped, reason);
            export::bump(&mut self.counts.dropped, &format!("e2_{reason}"));
            return Ok(None);
        }
        let (actor, candidates) = rebuilt.expect("checked");
        let chosen = usize::from(record.chosen.expect("checked"));
        let sent = (i32::from(record.head) == head::UNIT).then(|| candidates[chosen].command.as_ref().map(|c| format!("{c:?}"))).flatten();
        let mut accepted = vec![1u8; candidates.len()];
        for r in &record.refused {
            accepted[usize::from(*r)] = 0;
        }
        if let Some(Command::EnqueueProduction { city, item, .. }) = candidates[chosen].command {
            // As the net player notes its order for the phase's later cities.
            turn.note_order(city, item);
        }
        export::count_decision(&mut self.counts, i32::from(record.head), candidates.len());
        self.e2.aligned += 1;
        open.e2.logp.push(record.logp);
        let key = (trace.turns[t].player, trace.turns[t].round, d as u32);
        open.e2.dvalues.push(self.values.get(&key).copied().unwrap_or([f32::NAN; 4]));
        open.e2.decisions.push([d as i32, i32::from(record.accepted), record.score, record.position as i32]);
        open.e2.accepted.extend(accepted);
        open.decisions.push((i32::from(record.head), actor, candidates, chosen));
        Ok(sent)
    }
}

/// The window of each kept (player, round).
type Windows = BTreeMap<(u8, u32), usize>;

/// Which of the trace's turns the shard keeps (`selection`, as
/// `export::features` keeps a nation's turns, here over the learner's
/// turns), and the window of each kept (player, round).
fn selection(spec: &LogSpec, trace: &Trace, selection: Selection) -> (Vec<bool>, Option<Windows>) {
    match selection {
        Selection::Sample(every) => (trace.turns.iter().map(|t| export::sampled(spec.seed, t.round, PlayerId(t.player), every)).collect(), None),
        Selection::Windows { length, per_player } => {
            let mut by_player: BTreeMap<u8, Vec<u32>> = BTreeMap::new();
            for t in &trace.turns {
                by_player.entry(t.player).or_default().push(t.round);
            }
            let mut windows = BTreeMap::new();
            let mut id = 0;
            for (player, rounds) in by_player {
                for (first, len) in export::choose_windows(spec.seed, PlayerId(player), rounds.len(), length as usize, per_player as usize) {
                    for round in &rounds[first..first + len] {
                        windows.insert((player, *round), id);
                    }
                    id += 1;
                }
            }
            (trace.turns.iter().map(|t| windows.contains_key(&(t.player, t.round))).collect(), Some(windows))
        }
    }
}

/// Replays the log of an E2 game and builds the shard of the learner's
/// decisions (named `name`) on the turns `selection` keeps, with its E2
/// arrays. Any divergence of the replay (a refused command, a wrong round
/// hash) or a sidecar that does not fit the log is an error.
pub fn shard(rules: &CompiledRuleset, spec: &LogSpec, name: &str, journal: &[u8], trace: &Trace, selection_of: Selection) -> Result<(Shard, E2Arrays, E2Counts), String> {
    shard_with(rules, spec, name, journal, trace, selection_of, None)
}

/// The gift of a game's `.e2.json` (`selfplay::GameMeta::gift`), if any.
pub fn gift_of(game: &serde_json::Value) -> Result<Option<crate::selfplay::Gift>, String> {
    match game.get("gift") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(gift) => serde_json::from_value(gift.clone()).map(Some).map_err(|e| format!("gift: {e}")),
    }
}

/// `shard` of a game whose learner was given technologies at its
/// mid-game start (`--give-space-techs`): the gift is made again before
/// the command at its position, as at play.
#[allow(clippy::too_many_arguments)]
pub fn shard_with(rules: &CompiledRuleset, spec: &LogSpec, name: &str, journal: &[u8], trace: &Trace, selection_of: Selection, gift: Option<&crate::selfplay::Gift>) -> Result<(Shard, E2Arrays, E2Counts), String> {
    shard_edited(rules, spec, name, journal, trace, selection_of, gift, &[], &ValueMap::new())
}

/// `shard` of a game as its `.e2.json` describes it: its gift
/// (`gift_of`) and a training ground's changes outside the commands
/// (`grounds::edits_of`) made again where they were made at play.
pub fn shard_of_game(rules: &CompiledRuleset, spec: &LogSpec, name: &str, journal: &[u8], trace: &Trace, selection_of: Selection, game: &serde_json::Value) -> Result<(Shard, E2Arrays, E2Counts), String> {
    let gift = gift_of(game)?;
    let edits = crate::grounds::edits_of(game)?;
    let values = values_of(game)?;
    shard_edited(rules, spec, name, journal, trace, selection_of, gift.as_ref(), &edits, &values)
}

/// `shard_with`, and the edits of a training ground (`grounds::Edit`) made
/// before the command at their position, and the game's valued decisions
/// (`values_of`, the shard's `e2_dvalues.npy`).
#[allow(clippy::too_many_arguments)]
pub fn shard_edited(
    rules: &CompiledRuleset,
    spec: &LogSpec,
    name: &str,
    journal: &[u8],
    trace: &Trace,
    selection_of: Selection,
    gift: Option<&crate::selfplay::Gift>,
    edits: &[crate::grounds::Edit],
    values: &ValueMap,
) -> Result<(Shard, E2Arrays, E2Counts), String> {
    let (keep, windows) = selection(spec, trace, selection_of);
    let mut events = Vec::new();
    for (t, turn) in trace.turns.iter().enumerate() {
        events.push((turn.position, t, None));
        events.extend(turn.decisions.iter().enumerate().map(|(d, r)| (r.position, t, Some(d))));
    }
    if events.windows(2).any(|w| w[0].0 > w[1].0) || events.iter().any(|e| e.0 == sample::UNSTAMPED) {
        return Err("the sidecar's positions are not stamped in order".to_owned());
    }
    let mut rebuild = Rebuild {
        rules,
        trace,
        keep,
        events,
        next: 0,
        open: None,
        kept: Vec::new(),
        mind: ClassicMind::new(Variant::PreWarFix),
        settings: AiSettings::for_rules(rules, &spec.game()?.difficulty),
        counts: Counts::default(),
        e2: E2Counts { turns: trace.turns.len() as u64, decisions: trace.decisions() as u64, ..E2Counts::default() },
        summaries: Vec::new(),
        trackers: BTreeMap::new(),
        values: values.clone(),
    };
    rebuild.counts.player_turns = trace.turns.len() as u64;
    let mut position = 0u32;
    let mut failure: Option<String> = None;
    let (game, checked, commands) = export::replay_with(
        rules,
        spec,
        journal,
        |game, commands| {
            if let Some(gift) = gift.filter(|g| g.position == commands) {
                crate::selfplay::apply_gift(game, rules, gift);
            }
            crate::grounds::apply_at(game, rules, edits, commands);
        },
        |game, command, round, _player| {
            if rebuild.summaries.last().is_none_or(|(r, _)| *r != round) {
                rebuild.summaries.push((round, export::summarize(game, rules)));
            }
            if failure.is_none() {
                if let Err(e) = rebuild.at(game, position, Some(command)) {
                    failure = Some(e);
                }
            }
            position += 1;
        },
        |_, ()| {},
    )?;
    if let Some(e) = failure {
        return Err(e);
    }
    rebuild.at(&game, position, None)?;
    rebuild.close();
    if rebuild.next != rebuild.events.len() {
        return Err(format!("{} sidecar records after the log's end", rebuild.events.len() - rebuild.next));
    }
    let dims = game.view().auth.map().dimensions;
    let (region, regions) = encode::region_table(dims);
    let mut shard = Shard {
        name: name.to_owned(),
        spec: Some(spec.clone()),
        width: dims.width,
        height: dims.height,
        players: game.view().auth.players().count() as u32,
        neighbors: encode::neighbor_table(dims),
        region,
        regions,
        c_global: encode::global_width(rules),
        final_round: game.view().auth.turn().round,
        standings: export::standings(&game, rules),
        ..Shard::default()
    };
    let Rebuild { mut kept, mut counts, mut e2, mut summaries, .. } = rebuild;
    e2.hash = kernel::replay::hash::authoritative_hash(&game);
    counts.commands = commands;
    counts.checkpoints = checked;
    summaries.push((u32::MAX, export::summarize(&game, rules)));
    // `assemble` orders the turns by player and round (a stable sort): the E2 rows follow the same order.
    kept.sort_by_key(|(k, _)| (k.player.0, k.round));
    let mut arrays = E2Arrays::default();
    let mut turns = Vec::with_capacity(kept.len());
    for (k, rows) in kept {
        arrays.value.push(rows.value);
        arrays.logp.extend(rows.logp);
        arrays.dvalues.extend(rows.dvalues);
        arrays.decisions.extend(rows.decisions);
        arrays.accepted.extend(rows.accepted);
        turns.push(k);
    }
    shard.counts = counts;
    let standings = shard.standings.clone();
    let future = export::Future { summaries: &summaries, standings: &standings };
    export::assemble(&mut shard, turns, windows.as_ref(), &future);
    debug_assert_eq!(arrays.accepted.len(), shard.candidate_meta.len());
    Ok((shard, arrays, e2))
}

/// Writes the shard (`export::write_shard`, its `meta.json` with an `e2`
/// object: temperature, sampling seed, group, member, group size, the
/// learner's weights, `learners`, `seats`, `from_round`, `source`,
/// `learner_mind`, `victory`) and its E2 files: `e2_logp.npy`
/// (`<f4 [D]`), `e2_decisions.npy` (`<i4 [D, 4]`), `e2_accepted.npy`
/// (`|u1 [N]`), `e2_value.npy` (`<i4 [T]`), `e2_dvalues.npy` (`<f4 [D, 4]`,
/// `E2Arrays::dvalues`), `e2.json` (the game's
/// `.e2.json`, the counts). A shard that cannot be written whole is deleted.
pub fn write(dir: &Path, shard: &Shard, arrays: &E2Arrays, counts: &E2Counts, game: &serde_json::Value, rules: &CompiledRuleset) -> io::Result<(PathBuf, u64)> {
    let out = dir.join(format!("{}.shard", shard.name));
    let result = (|| {
        export::write_shard(dir, shard, rules)?;
        // meta.json gains `e2`: the sampling, the group, the learners and every seat's outcome (the reward).
        let meta_path = out.join("meta.json");
        let mut meta: serde_json::Value = serde_json::from_slice(&std::fs::read(&meta_path)?).map_err(io::Error::other)?;
        let field = |key: &str| game.get(key).cloned().unwrap_or(serde_json::Value::Null);
        meta["e2"] = serde_json::json!({
            "format": 1,
            "temperature": field("temperature"),
            "sample_seed": field("sample_seed"),
            "group": field("group"),
            "member": field("member"),
            "group_size": field("group_size"),
            "net": field("net"),
            "learners": field("learners"),
            "seats": field("seats"),
            // A mid-game start (null from round 0): the shard's turns are those after it.
            "from_round": field("from_round"),
            "source": field("source"),
            "learner_mind": field("learner_mind"),
            "victory": field("victory"),
        });
        std::fs::write(&meta_path, serde_json::to_vec_pretty(&meta).map_err(io::Error::other)?)?;
        let d = arrays.logp.len();
        let logp: Vec<u8> = arrays.logp.iter().flat_map(|v| v.to_le_bytes()).collect();
        export::npy(&out.join("e2_logp.npy"), "<f4", &[d], &logp)?;
        export::npy(&out.join("e2_decisions.npy"), "<i4", &[d, 4], &export::i32_bytes(arrays.decisions.iter().flatten()))?;
        export::npy(&out.join("e2_accepted.npy"), "|u1", &[arrays.accepted.len()], &arrays.accepted)?;
        export::npy(&out.join("e2_value.npy"), "<i4", &[arrays.value.len()], &export::i32_bytes(arrays.value.iter()))?;
        let dvalues: Vec<u8> = arrays.dvalues.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        export::npy(&out.join("e2_dvalues.npy"), "<f4", &[arrays.dvalues.len(), 4], &dvalues)?;
        let json = serde_json::json!({ "format": 1, "game": game, "counts": counts });
        std::fs::write(out.join("e2.json"), serde_json::to_vec_pretty(&json).map_err(io::Error::other)?)?;
        let bytes = std::fs::read_dir(&out)?.filter_map(|e| e.ok()?.metadata().ok()).map(|m| m.len()).sum();
        Ok((out.clone(), bytes))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&out);
    }
    result
}

/// The sidecar files beside a log: its trace (`.e2.bin`) and its game
/// (`.e2.json`).
pub fn read_sidecar(log: &Path) -> Result<(Trace, serde_json::Value), String> {
    let stem = log.with_extension("");
    let bin = stem.with_extension("e2.bin");
    let json = stem.with_extension("e2.json");
    let bytes = std::fs::read(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    let trace = Trace::decode(&bytes).map_err(|e| format!("{}: {e}", bin.display()))?;
    let game = serde_json::from_slice(&std::fs::read(&json).map_err(|e| format!("{}: {e}", json.display()))?).map_err(|e| format!("{}: {e}", json.display()))?;
    Ok((trace, game))
}
