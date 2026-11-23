//! The teachers' decisions on the trainers' scenes (DESIGN.md, «Teacher data»).
//!
//! A play is recorded when `ai-exam suite --record DIR` asks for it (`want`). The recorder belongs to
//! the thread that plays (a play runs on one thread from its start to its end), and sees every
//! command of the learner's side at the moment it is given: the references' commands through
//! `trainers::act` (`try_cmd`, `strike`, `stand`), an AI variant's through the driver (`step`, which
//! is `AiDriver::step` with the command seen before it is executed). For each command of a unit of
//! the army it keeps the tokens and the unit's legal actions BEFORE the command, and the label: the
//! token of the unit (the pointer) and the legal action the command amounts to (a move by the tile
//! the unit ended on, the others by equality). The end of the side's turn is a decision too: the
//! pointer says "end". After the play, each decision of a round gets the damage each of our units
//! took in the enemy's turn that followed (the auxiliary target).
//!
//! The format (little-endian), one play after another:
//! `"ARM3" u32 | json_len u32 | json (the play's row: family, era, instance, attempt, player, win,
//! measures; the counts) | W × W × S_CH f16 static planes | n u32 | n decisions`; a decision:
//! `"ARMD" u32 | T u16 | C u16 | pointer i16 (-1: end, -2: none) | action i16 (-1: none) | round u16 |
//! flags u16 | T × F_TOK f16 | T × T u8 distances | T u8 selectable | T f16 danger (-1: unknown) |
//! C × F_ACT f16 | C u16 promotion + 1 | T i16 window index | C i16 window index | T i16 army slot (-1: none) |
//! TRACK i16 the teacher's own state`. Version 2 (`"ARMP"`) had no slots and no track, and 0 for the flags.
//!
//! Variant M's memory (M-HANDOFF.md) updates at our turn's end: the decision there has the flag
//! `MEMORY_STATE`. A teacher's play: its end of the turn. The net's own play (DAgger): an unlabelled decision
//! (pointer -2) at its turn's end, so the memory follows the net's trajectory. The track of a memory state is
//! the teacher's own state then (the staged assault's: engaged, waited, the stage, the resting slots as a bit
//! mask; -1 where the teacher has none); privileged, read only by training's auxiliary head.

use std::cell::RefCell;
use std::collections::BTreeMap;

use classic_rules::classic::Classic;
use classic_rules::ids::UnitHandle;
use classic_rules::knowledge::actions::acting_unit;
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::{Advance, StepOutcome};
use kernel::ids::EntityId;

use super::observe::{self, f32_to_f16, Action, Candidate, Tokens, F_ACT, F_TOK};
use super::Orders;

pub const PLAY_MAGIC: u32 = u32::from_le_bytes(*b"ARMP");
pub const PLAY_MAGIC3: u32 = u32::from_le_bytes(*b"ARM3");
/// A decision's flag: our turn ends here, the memory updates.
pub const MEMORY_STATE: u16 = 1;
/// The teacher's state numbers of a decision: engaged, waited, stage, resting slots (bits).
pub const TRACK: usize = 4;
pub const NO_TRACK: [i16; TRACK] = [-1; TRACK];
pub const DECISION_MAGIC: u32 = u32::from_le_bytes(*b"ARMD");

struct Decision {
    tokens: Tokens,
    candidates: Vec<Candidate>,
    pointer: i16,
    action: i16,
    round: u16,
    /// The army unit of each token (None for the others).
    ids: Vec<Option<EntityId>>,
    /// Hit points of the army's units at this moment.
    hp: BTreeMap<EntityId, u16>,
    end: bool,
    /// The auxiliary target of each token (`fill_danger`).
    danger: Vec<f32>,
    flags: u16,
    track: [i16; TRACK],
}

/// What a command was before it ran.
pub struct Pending {
    tokens: Tokens,
    candidates: Vec<Candidate>,
    pointer: usize,
    unit: UnitHandle,
    id: EntityId,
    command: Option<Command>,
    /// Hit points of the army's units before the command.
    hp: BTreeMap<EntityId, u16>,
    ids: Vec<Option<EntityId>>,
}

/// Counts of the play: decisions kept, and the commands that could not be labelled, by kind.
#[derive(Debug, Default, Clone)]
pub struct Counts {
    pub kept: u32,
    pub ends: u32,
    pub dropped: BTreeMap<String, u32>,
}

/// Whose commands become decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Every command of the army: the teacher plays the seat.
    Teacher,
    /// None: the net plays the seat (DAgger), its own commands are not labels.
    Muted,
    /// The first command of a teacher's turn played on a copy of the game (`probe`), then none.
    Probe { taken: bool },
}

struct Recorder {
    orders: Orders,
    decisions: Vec<Decision>,
    counts: Counts,
    mode: Mode,
    /// The army's hit points when our turn of each round ended (the danger's start).
    turn_end_hp: BTreeMap<u16, BTreeMap<EntityId, u16>>,
    /// The teacher's own state for the next memory state (`set_track`).
    track: [i16; TRACK],
}

thread_local! {
    static WANT: RefCell<bool> = const { RefCell::new(false) };
    static ACTIVE: RefCell<Option<Recorder>> = const { RefCell::new(None) };
}

/// The next play on this thread is recorded (or not).
pub fn want(on: bool) {
    WANT.with(|w| *w.borrow_mut() = on);
}

pub fn wanted() -> bool {
    WANT.with(|w| *w.borrow())
}

pub fn active() -> bool {
    ACTIVE.with(|a| a.borrow().is_some())
}

/// Starts recording the play of `orders.side`.
pub fn start(orders: Orders) {
    ACTIVE.with(|a| *a.borrow_mut() = Some(Recorder { orders, decisions: Vec::new(), counts: Counts::default(), mode: Mode::Teacher, turn_end_hp: BTreeMap::new(), track: NO_TRACK }));
}

/// The teacher's own state now (written with the next memory state of a teacher's play).
pub fn set_track(track: [i16; TRACK]) {
    with(|r| r.track = track);
}

/// The army slot of each token: the unit's index in `Orders::army` (-1: not an army unit, or past the slots).
pub fn slots_of(game: &GameState, orders: &Orders, tokens: &Tokens) -> Vec<i16> {
    let auth = game.view().auth;
    tokens
        .unit
        .iter()
        .map(|u| u.and_then(|h| auth.units().get(h)).and_then(|x| orders.army.iter().position(|a| *a == x.core.id)).filter(|s| *s < super::model::SLOTS).map_or(-1, |s| s as i16))
        .collect()
}

/// The net plays the seat: its commands are not recorded (DAgger).
pub fn mute() {
    with(|r| r.mode = Mode::Muted);
}

/// Every recordable decision is kept (a teacher's whole turn, played on a copy of the game).
pub fn teach() {
    with(|r| r.mode = Mode::Teacher);
}

/// The next recordable decision (of a teacher's turn on a copy of the game) is kept, the rest dropped.
pub fn probe() {
    with(|r| r.mode = Mode::Probe { taken: false });
}

/// After a probe: whether it kept a decision; the recorder is muted again.
pub fn probe_done() -> bool {
    with(|r| {
        let taken = r.mode == Mode::Probe { taken: true };
        r.mode = Mode::Muted;
        taken
    })
    .unwrap_or(false)
}

/// Whether a decision is kept now (and the probe's slot taken).
fn keeps(r: &mut Recorder) -> bool {
    match r.mode {
        Mode::Teacher => true,
        Mode::Muted | Mode::Probe { taken: true } => false,
        Mode::Probe { taken: false } => {
            r.mode = Mode::Probe { taken: true };
            true
        }
    }
}

fn listens() -> bool {
    with(|r| r.mode == Mode::Teacher || r.mode == Mode::Probe { taken: false }).unwrap_or(false)
}

fn with<R>(f: impl FnOnce(&mut Recorder) -> R) -> Option<R> {
    ACTIVE.with(|a| a.borrow_mut().as_mut().map(f))
}

fn drop_count(counts: &mut Counts, why: &str) {
    *counts.dropped.entry(why.to_owned()).or_default() += 1;
}

fn command_name(c: &Command) -> String {
    let s = format!("{c:?}");
    s.split([' ', '{', '(']).next().unwrap_or("?").to_owned()
}

fn army_hp(game: &GameState, orders: &Orders) -> BTreeMap<EntityId, u16> {
    let auth = game.view().auth;
    auth.indexes().units_of(orders.side).iter().filter_map(|h| auth.units().get(*h)).filter(|u| orders.army.contains(&u.core.id)).map(|u| (u.core.id, u.core.hp)).collect()
}

fn ids_of(game: &GameState, orders: &Orders, tokens: &Tokens) -> Vec<Option<EntityId>> {
    let auth = game.view().auth;
    tokens.unit.iter().map(|u| u.and_then(|h| auth.units().get(h)).map(|x| x.core.id).filter(|id| orders.army.contains(id))).collect()
}

/// Before a command of `side`: the moment as the net would see it, when the command is a unit's of
/// the army. None when nothing is recorded.
pub fn before(game: &GameState, rules: &CompiledRuleset, side: classic_rules::ids::PlayerId, command: &Command) -> Option<Pending> {
    if !listens() {
        return None;
    }
    let orders = with(|r| r.orders.clone())?;
    if side != orders.side {
        return None;
    }
    let unit = acting_unit(command)?;
    let core = game.view().auth.units().get(unit)?.core;
    if !orders.army.contains(&core.id) {
        return None;
    }
    moment(game, rules, &orders, unit, core.id, Some(command.clone()))
}

fn moment(game: &GameState, rules: &CompiledRuleset, orders: &Orders, unit: UnitHandle, id: EntityId, command: Option<Command>) -> Option<Pending> {
    let tokens = observe::tokens(game, rules, orders);
    let Some(pointer) = tokens.index_of(unit).filter(|i| tokens.selectable[*i]) else {
        let why = format!("not selectable {}", command.as_ref().map_or("done".to_owned(), command_name));
        with(|r| drop_count(&mut r.counts, &why));
        return None;
    };
    let candidates = observe::candidates(game, rules, orders, unit);
    let hp = army_hp(game, orders);
    let ids = ids_of(game, orders, &tokens);
    Some(Pending { tokens, candidates, pointer, unit, id, command, hp, ids })
}

/// After the command: its label among the legal actions, and the decision kept.
pub fn after(game: &GameState, pending: Option<Pending>, ok: bool) {
    let Some(p) = pending else { return };
    if !ok {
        with(|r| drop_count(&mut r.counts, "refused"));
        return;
    }
    let label = match &p.command {
        None => p.candidates.iter().position(|c| matches!(c.action, Action::Done)),
        Some(cmd) => label_of(game, &p, cmd),
    };
    let Some(action) = label else {
        let why = p.command.as_ref().map_or("done".to_owned(), |c| format!("unlisted {}", command_name(c)));
        with(|r| drop_count(&mut r.counts, &why));
        return;
    };
    with(|r| {
        if !keeps(r) {
            return;
        }
        let round = game.view().auth.turn().round.saturating_sub(r.orders.start_round) as u16;
        r.decisions.push(Decision { tokens: p.tokens, candidates: p.candidates, pointer: p.pointer as i16, action: action as i16, round, ids: p.ids, hp: p.hp, end: false, danger: Vec::new(), flags: 0, track: NO_TRACK });
        r.counts.kept += 1;
    });
}

/// The legal action a command given at the pending moment amounts to.
fn label_of(game: &GameState, p: &Pending, cmd: &Command) -> Option<usize> {
    let moved_to = || {
        let auth = game.view().auth;
        auth.units().get(p.unit).filter(|u| u.core.id == p.id).map(|u| u.core.tile)
    };
    let is_move = |c: &Candidate, t| matches!(c.action, Action::Command(Command::MoveUnit { destination, .. }) if destination == t);
    match *cmd {
        Command::MoveUnit { destination, .. } | Command::MarchTo { destination, .. } => {
            let end = moved_to();
            if let Some(i) = end.and_then(|t| p.candidates.iter().position(|c| is_move(c, t))) {
                return Some(i);
            }
            p.candidates.iter().position(|c| is_move(c, destination))
        }
        // Commands have no equality: the same command prints the same.
        _ => {
            let want = format!("{cmd:?}");
            p.candidates.iter().position(|c| matches!(&c.action, Action::Command(x) if format!("{x:?}") == want))
        }
    }
}

/// A reference ends the turn of an army unit (`act::stand`): the "done" action.
pub fn done(game: &GameState, rules: &CompiledRuleset, unit: UnitHandle) {
    if !listens() {
        return;
    }
    let Some(orders) = with(|r| r.orders.clone()) else { return };
    let Some(core) = game.view().auth.units().get(unit).map(|u| *u.core) else { return };
    if core.owner != orders.side || !orders.army.contains(&core.id) || game.view().auth.active_player() != orders.side {
        return;
    }
    if core.moves_left == 0 {
        return;
    }
    let p = moment(game, rules, &orders, unit, core.id, None);
    after(game, p, true);
}

/// The side ends its turn: the army's hit points for the danger, and (when a teacher plays) the
/// pointer's "end".
pub fn end_turn(game: &GameState, rules: &CompiledRuleset) {
    let Some(orders) = with(|r| r.orders.clone()) else { return };
    let hp = army_hp(game, &orders);
    let teacher = with(|r| {
        let round = game.view().auth.turn().round.saturating_sub(r.orders.start_round) as u16;
        r.turn_end_hp.insert(round, hp);
        r.mode == Mode::Teacher
    });
    if teacher == Some(true) {
        let track = with(|r| r.track).unwrap_or(NO_TRACK);
        push(game, rules, -1, MEMORY_STATE, track);
    }
}

/// The pointer's "end" at this moment, as a decision (a probe found nothing the army should do; a probe's
/// whole turn on a copy ends): not a memory state.
pub fn push_end(game: &GameState, rules: &CompiledRuleset) {
    push(game, rules, -1, 0, NO_TRACK);
}

/// The net's own turn ends (DAgger): an unlabelled memory state, with the expert's state then.
pub fn push_memory_state(game: &GameState, rules: &CompiledRuleset, track: [i16; TRACK]) {
    push(game, rules, -2, MEMORY_STATE, track);
}

fn push(game: &GameState, rules: &CompiledRuleset, pointer: i16, flags: u16, track: [i16; TRACK]) {
    let Some(orders) = with(|r| r.orders.clone()) else { return };
    let tokens = observe::tokens(game, rules, &orders);
    let ids = ids_of(game, &orders, &tokens);
    let hp = army_hp(game, &orders);
    with(|r| {
        let round = game.view().auth.turn().round.saturating_sub(r.orders.start_round) as u16;
        r.decisions.push(Decision { tokens, candidates: Vec::new(), pointer, action: -1, round, ids, hp, end: true, danger: Vec::new(), flags, track });
        if pointer == -1 {
            r.counts.ends += 1;
        }
    });
}

/// `AiDriver::step` for the learner's side while recording: the command is seen before it runs.
pub fn step<P: kernel::ai::mind::AiMind<Classic>>(driver: &mut AiDriver<Classic, P>, game: &mut GameState, rules: &CompiledRuleset) -> StepOutcome {
    driver.configure_game(game);
    match driver.advance(game, rules) {
        Advance::Worked => StepOutcome::Worked,
        Advance::RoundDone => StepOutcome::RoundDone,
        Advance::Waiting => StepOutcome::Waiting,
        Advance::Command(command) => {
            let side = game.view().auth.active_player();
            let pending = before(game, rules, side, &command);
            let result = game.execute(rules, command);
            after(game, pending, result.is_ok());
            driver.executed(result)
        }
        Advance::EndTurn(command) => {
            super::seat::turn_end(game, rules);
            end_turn(game, rules);
            let result = game.execute(rules, command);
            driver.executed(result)
        }
    }
}

/// Ends the recording: the play's bytes (header `meta`, then its decisions), and its counts.
pub fn finish(mut meta: serde_json::Value) -> Option<(Vec<u8>, Counts)> {
    let mut rec = ACTIVE.with(|a| a.borrow_mut().take())?;
    fill_danger(&mut rec.decisions, &rec.turn_end_hp);
    meta["kept"] = serde_json::json!(rec.counts.kept);
    meta["ends"] = serde_json::json!(rec.counts.ends);
    meta["dropped"] = serde_json::json!(rec.counts.dropped);
    let json = serde_json::to_vec(&meta).ok()?;
    let mut out = Vec::new();
    put_u32(&mut out, PLAY_MAGIC3);
    put_u32(&mut out, json.len() as u32);
    out.extend_from_slice(&json);
    for x in &rec.orders.statics {
        put_u16(&mut out, f32_to_f16(*x));
    }
    put_u32(&mut out, rec.decisions.len() as u32);
    for d in &rec.decisions {
        write_decision(&mut out, d, &rec.orders);
    }
    Some((out, rec.counts))
}

/// The damage each army unit took between our end of round r and our first decision of round r + 1
/// (all of it when the unit died), over 100; -1 when the play ended before.
fn fill_danger(decisions: &mut [Decision], turn_end_hp: &BTreeMap<u16, BTreeMap<EntityId, u16>>) {
    let mut first: BTreeMap<u16, usize> = BTreeMap::new();
    for (i, d) in decisions.iter().enumerate() {
        first.entry(d.round).or_insert(i);
    }
    let mut danger: BTreeMap<(u16, EntityId), f32> = BTreeMap::new();
    for (round, end) in turn_end_hp {
        let Some(&next) = first.get(&(round + 1)) else { continue };
        let start = &decisions[next].hp;
        for (id, hp) in end {
            let now = start.get(id).copied().unwrap_or(0);
            danger.insert((*round, *id), f32::from(hp.saturating_sub(now)) / 100.0);
        }
    }
    for d in decisions.iter_mut() {
        d.danger = d.ids.iter().map(|id| id.and_then(|id| danger.get(&(d.round, id)).copied()).unwrap_or(-1.0)).collect();
    }
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_decision(out: &mut Vec<u8>, d: &Decision, orders: &Orders) {
    let t = d.tokens.len();
    put_u32(out, DECISION_MAGIC);
    put_u16(out, t as u16);
    put_u16(out, d.candidates.len() as u16);
    out.extend_from_slice(&d.pointer.to_le_bytes());
    out.extend_from_slice(&d.action.to_le_bytes());
    put_u16(out, d.round);
    put_u16(out, d.flags);
    for f in &d.tokens.feat {
        for x in f.iter().take(F_TOK) {
            put_u16(out, f32_to_f16(*x));
        }
    }
    out.extend_from_slice(&d.tokens.dist);
    out.extend(d.tokens.selectable.iter().map(|s| u8::from(*s)));
    for v in &d.danger {
        put_u16(out, f32_to_f16(*v));
    }
    for c in &d.candidates {
        for x in c.feat.iter().take(F_ACT) {
            put_u16(out, f32_to_f16(*x));
        }
    }
    for c in &d.candidates {
        put_u16(out, c.promo);
    }
    for p in &d.tokens.pos {
        out.extend_from_slice(&p.to_le_bytes());
    }
    for c in &d.candidates {
        out.extend_from_slice(&c.pos.to_le_bytes());
    }
    for id in &d.ids {
        let s = id.and_then(|id| orders.army.iter().position(|a| *a == id)).filter(|s| *s < super::model::SLOTS).map_or(-1i16, |s| s as i16);
        out.extend_from_slice(&s.to_le_bytes());
    }
    for x in &d.track {
        out.extend_from_slice(&x.to_le_bytes());
    }
}

thread_local! {
    static LAST: RefCell<Option<(Vec<u8>, Counts)>> = const { RefCell::new(None) };
}

/// The play is over (`row`), or failed (None): the recording, if any, is closed; `take_last` gives it.
pub fn close(row: Option<&crate::trainers::Row>) {
    if !active() {
        return;
    }
    let Some(row) = row else {
        ACTIVE.with(|a| a.borrow_mut().take());
        return;
    };
    let meta = serde_json::json!({"family": row.family, "era": row.era, "instance": row.instance, "attempt": row.attempt, "player": row.player, "win": row.win, "rounds": row.rounds, "measures": row.measures});
    let out = finish(meta);
    LAST.with(|l| *l.borrow_mut() = out);
}

/// The last closed recording of this thread.
pub fn take_last() -> Option<(Vec<u8>, Counts)> {
    LAST.with(|l| l.borrow_mut().take())
}
