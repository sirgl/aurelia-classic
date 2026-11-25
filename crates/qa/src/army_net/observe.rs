//! What the army net sees (DESIGN.md, «Inputs»). Every number is rounded through f16, the precision
//! the recorder stores, so the player's input equals the training input bit for bit.
//!
//! The net sees what its side KNOWS and no more (observation version 2, `known::OBS_VERSION`): this module reads the game
//! only through `known::Known` (units the side sees, cities in sight or as last sighted, explored tiles, the interface's
//! combat preview), in every family and in every game, fog or not. It cannot name the full state: the guard test
//! `tests/army_net_known.rs` fails on a `game.view()` here and, bit for bit, on any input that reads what the side does not know.
//!
//! Tokens, in this order: the objective; the units of both sides (ours first, by id); the cities of
//! both sides. Each token is `F_TOK` numbers (the layout below); `dist` holds the hex distance between
//! the tokens' tiles (the attention bias). Candidates: the legal actions of one of our units, each
//! `F_ACT` numbers and a promotion id.
//!
//! Typed tokens and the third parties (step 1.5 of the general-AI plan, gate T1 of the typed-token plan):
//! every token and candidate also has its EXTRAS (`X_TOK`, `X_ACT` numbers: the relation one-hot and the threat of the
//! third parties at war with us), and the units and cities of the players other than the side and `orders.foe` inside
//! the map window are the THIRD tokens (`Tokens::third`), read by the net through a separate path. The main stream and
//! its numbers are exactly the old ones, so a net without the new parts plays as before.

use classic_rules::combat::city::CITY_ATTACK_RANGE;
use classic_rules::ids::{PlayerId, TileId, UnitHandle};
use classic_rules::{Command, CompiledRuleset, GameState};
use kernel::world::unit::{Stance, UnitCore, MOVE_DENOMINATOR};

use super::known::{Known, OwnCity, SeenCity, SeenUnit};
use super::{base_strength, Orders, AIMS};
use crate::trainers::act;
use crate::trainers::lab::{arms_of, Arm, ARMS};

pub const F_TOK: usize = 48;
pub const F_ACT: usize = 40;
/// Tokens at most: the objective, 39 units and cities.
pub const T_MAX: usize = 40;
/// Distances in the attention bias are clipped here.
pub const D_MAX: u8 = 15;
/// A token's extras: the relation one-hot (`R_*`), then (our units) the third parties' war threat on its tile: those
/// that reach it next turn / 4, adjacent / 3, the nearest / 12.
pub const X_TOK: usize = 9;
/// A candidate's extras: its strike target's relation one-hot, "declares war" (a strike on a player not at war with
/// us), the third parties' war threat on its tile / 4, adjacent / 3.
pub const X_ACT: usize = 9;
/// The relations (`relation`).
pub const REL: usize = 6;
pub const R_OWN: u8 = 0;
pub const R_ALLY: u8 = 1;
pub const R_PEACE: u8 = 2;
pub const R_WAR: u8 = 3;
pub const R_CS: u8 = 4;
pub const R_BARB: u8 = 5;
pub const REL_NAMES: [&str; REL] = ["own", "ally", "peace", "war", "city-state", "barbarian"];
/// The candidate extra "declares war" (the new candidate kind of the plan: its logit shift starts at -10).
pub const X_DECLARES: usize = 6;
/// The field's token budget at stage 64 (plan 11.1): the third tokens fill what the main stream leaves, at most `X_CAP`.
pub const FIELD_BUDGET: usize = 32;
pub const X_CAP: usize = 16;
/// The observation radius of a unit's legal actions.
const RADIUS: i32 = 12;

/// Token kinds (the first four numbers).
pub const TOK_OBJECTIVE: usize = 0;
pub const TOK_OWN: usize = 1;
pub const TOK_FOE: usize = 2;
pub const TOK_CITY: usize = 3;

/// Candidate kinds (the first ten numbers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Done = 0,
    Move = 1,
    AttackUnit = 2,
    AttackCity = 3,
    Ranged = 4,
    Fortify = 5,
    SetUp = 6,
    Promote = 7,
    Pillage = 8,
    Other = 9,
}

/// One legal action of the acting unit.
#[derive(Debug, Clone)]
pub enum Action {
    /// The unit's turn is over (the references' `stand`).
    Done,
    Command(Command),
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub feat: [f32; F_ACT],
    /// The extras (`X_ACT`).
    pub x: [f32; X_ACT],
    /// Promotion id + 1 (0: not a promotion).
    pub promo: u16,
    /// The window index of the candidate's tile (its destination, its target, the unit's own); -1 outside.
    pub pos: i16,
    pub action: Action,
}

/// The tokens of one moment.
#[derive(Debug, Clone)]
pub struct Tokens {
    pub feat: Vec<[f32; F_TOK]>,
    /// `len × len`, row-major.
    pub dist: Vec<u8>,
    /// Our units the net may pick now (in the army, alive, moves left or a promotion to take).
    pub selectable: Vec<bool>,
    /// The unit of each token (None: the objective, a city).
    pub unit: Vec<Option<UnitHandle>>,
    pub tile: Vec<TileId>,
    /// The window index of each token's tile; -1 outside the window.
    pub pos: Vec<i16>,
    /// The play's stake (`Orders::price`): MBA adds its projection to the objective token.
    pub price: [f32; 3],
    /// Variant MH: the history tokens (`F_HIST` numbers), the army slot of each (-1: an enemy's), its age in turns.
    pub hist: Vec<[f32; F_HIST]>,
    pub hist_slot: Vec<i16>,
    pub hist_age: Vec<u8>,
    /// The main tokens' extras (`X_TOK`; zeros in a recording before v5).
    pub xfeat: Vec<[f32; X_TOK]>,
    /// The third parties' tokens (none in a recording before v5).
    pub third: Third,
}

/// The tokens of the third parties: units and cities of the players other than the side and `orders.foe` in the map
/// window (the fog families: only those the side sees), at most `FIELD_BUDGET` with the main stream and `X_CAP`, the
/// nearest to the objective first (dropped first when over the budget: the farthest).
#[derive(Debug, Clone, Default)]
pub struct Third {
    /// The token numbers (a unit: kind 2 and the foe's view of numbers 37, 39, 40; a city: kind 3, number 45 = 0).
    pub feat: Vec<[f32; F_TOK]>,
    /// The extras: the relation one-hot (numbers 6-8 zero).
    pub x: Vec<[f32; X_TOK]>,
    pub pos: Vec<i16>,
    pub tile: Vec<TileId>,
    /// `main × third` hex distances, row-major, clipped at `D_MAX`.
    pub dist: Vec<u8>,
}

impl Third {
    pub fn len(&self) -> usize {
        self.feat.len()
    }
    pub fn is_empty(&self) -> bool {
        self.feat.is_empty()
    }
    /// Whether third token `j` is a city.
    pub fn is_city(&self, j: usize) -> bool {
        self.feat[j][TOK_CITY] > 0.5
    }
}

/// The relation of `owner`'s pieces to `side` (`orders.foe` is the side's foe of the orders): barbarian, at war, a
/// city-state, an ally (at peace and a defensive pact, a declaration of friendship, or at war with the foe, unless the
/// foe is the barbarians: every nation is at war with them), at peace.
pub fn relation(game: &GameState, rules: &CompiledRuleset, side: PlayerId, foe: PlayerId, owner: PlayerId) -> u8 {
    Known::new(game, rules, side).relation(foe, owner)
}

/// Hostile to the side: at war or the barbarians.
fn hostile(r: u8) -> bool {
    r == R_WAR || r == R_BARB
}

impl Tokens {
    pub fn len(&self) -> usize {
        self.feat.len()
    }
    pub fn is_empty(&self) -> bool {
        self.feat.is_empty()
    }
    pub fn index_of(&self, unit: UnitHandle) -> Option<usize> {
        self.unit.iter().position(|u| *u == Some(unit))
    }
}

/// What the tokens remember between our turns of one play: every unit's tile and hit points at the start of
/// our previous turn and of this one (numbers 46 and 47 of a unit's token: the hit points gained since our
/// previous turn began, whether it has left the tile it stood on then). A reference's commitments (a unit
/// resting to heal, the assault engaged) leave these traces.
///
/// Variant MH's history (M-HANDOFF.md, addition 3): the FACTS of the last turns, written by the game: every
/// command of an army unit with its outcome (`log_before` / `log_after`, from `act::execute` and `record::step`),
/// and at our turn's start the enemy's events since our last turn's end (`note_turn_end`): our units hit, foes that
/// came into or went out of sight. The tokens carry the events of this turn and the last `H_TURNS` (`Tokens::hist`).
#[derive(Default)]
struct History {
    round: Option<u32>,
    prev: std::collections::BTreeMap<kernel::ids::EntityId, (TileId, u16)>,
    this: std::collections::BTreeMap<kernel::ids::EntityId, (TileId, u16)>,
    orders: Option<Orders>,
    events: Vec<Event>,
    /// Our army (tile, hp) and the foes seen (tile) at our last turn's end, and its round.
    end: Option<(u32, std::collections::BTreeMap<kernel::ids::EntityId, (TileId, u16)>, std::collections::BTreeMap<kernel::ids::EntityId, TileId>)>,
    paused: bool,
}

/// A history token's numbers: the kind (own: move, strike a unit, strike a city, ranged, fortify / set up,
/// promote / pillage / other; enemy: our unit hit, a foe came into sight, went out of it), age / 3, the subject tile
/// against the objective (dq, dr / 12, distance / 12), dealt / 100, taken / 100, a kill, died or captured, moved / 5,
/// on a city (a strike at a city; a hit in a city's fire).
pub const F_HIST: usize = 20;
pub const H_MAX: usize = 32;
pub const H_TURNS: u32 = 3;
const HK_MOVE: usize = 0;
const HK_HIT: usize = 7;
const HK_SEEN: usize = 8;
const HK_GONE: usize = 9;

#[derive(Debug, Clone)]
struct Event {
    round: u32,
    slot: i16,
    f: [f32; F_HIST],
}

/// What a command was before it ran (for its outcome).
pub struct Pre {
    unit: UnitHandle,
    id: kernel::ids::EntityId,
    slot: i16,
    kind: usize,
    hp: u16,
    tile: TileId,
    target: Option<TileId>,
    target_hp: Option<i64>,
    target_city: bool,
}

/// The orders of the play the history belongs to (its army's slots, its objective).
pub fn set_history_orders(orders: &Orders) {
    HISTORY.with(|h| h.borrow_mut().orders = Some(orders.clone()));
}

/// The DAgger probe plays on a copy of the game: its commands are no facts of the play.
pub fn pause_history(on: bool) {
    HISTORY.with(|h| h.borrow_mut().paused = on);
}

/// Before a command of `side`: what the history needs to tell its outcome (None: not an army unit's, or paused).
pub fn log_before(game: &GameState, rules: &CompiledRuleset, side: PlayerId, cmd: &Command) -> Option<Pre> {
    let orders = HISTORY.with(|h| {
        let h = h.borrow();
        if h.paused {
            None
        } else {
            h.orders.clone()
        }
    })?;
    if side != orders.side {
        return None;
    }
    let unit = classic_rules::knowledge::actions::acting_unit(cmd)?;
    let k = Known::new(game, rules, side);
    let core = k.own_core(unit)?;
    let slot = orders.army.iter().position(|a| *a == core.id)? as i16;
    let kind = match cmd {
        Command::MoveUnit { .. } | Command::MarchTo { .. } => HK_MOVE,
        Command::AttackUnit { .. } => 1,
        Command::AttackCity { .. } => 2,
        Command::RangedAttack { .. } => 3,
        Command::Fortify { .. } | Command::SetUpRangedAttack { .. } => 4,
        _ => 5,
    };
    let target = k.target_tile(cmd);
    let (target_hp, target_city) = match target.and_then(|t| k.hp_at(t)) {
        Some((hp, city)) => (Some(hp), city),
        None => (None, false),
    };
    Some(Pre { unit, id: core.id, slot, kind, hp: core.hp, tile: core.tile, target, target_hp, target_city })
}

/// After the command: its outcome becomes a fact of this round.
pub fn log_after(game: &GameState, rules: &CompiledRuleset, pre: Option<Pre>, ok: bool) {
    let Some(p) = pre else { return };
    if !ok {
        return;
    }
    let Some(orders) = HISTORY.with(|h| h.borrow().orders.clone()) else { return };
    let k = Known::new(game, rules, orders.side);
    let now = k.own_core(p.unit).filter(|c| c.id == p.id);
    let mut f = [0f32; F_HIST];
    f[p.kind] = 1.0;
    let subject = p.target.or(now.map(|c| c.tile)).unwrap_or(p.tile);
    subject_numbers(&k, &orders, subject, &mut f);
    if let (Some(t), Some(before)) = (p.target, p.target_hp) {
        let after = k.hp_at(t).filter(|(_, city)| *city == p.target_city).map_or(0, |x| x.0);
        f[14] = (before - after).max(0) as f32 / 100.0;
        f[16] = if after == 0 || (p.target_city && k.city_owner_on(t) == Some(orders.side)) { 1.0 } else { 0.0 };
    }
    f[15] = f32::from(p.hp.saturating_sub(now.map_or(0, |c| c.hp))) / 100.0;
    f[17] = if now.is_none() { 1.0 } else { 0.0 };
    f[18] = now.map_or(0, |c| k.dist(c.tile, p.tile)) as f32 / 5.0;
    f[19] = if p.target_city { 1.0 } else { 0.0 };
    let round = k.round();
    HISTORY.with(|h| h.borrow_mut().events.push(Event { round, slot: p.slot, f }));
}

fn subject_numbers(k: &Known, orders: &Orders, t: TileId, f: &mut [f32; F_HIST]) {
    let (ox, oy) = k.col_row(orders.tile);
    let (x, y) = k.col_row(t);
    f[11] = (x - ox) / 12.0;
    f[12] = (y - oy) / 12.0;
    f[13] = k.dist(t, orders.tile) as f32 / 12.0;
}

/// The foes the side sees.
fn foes_seen(k: &Known, orders: &Orders) -> std::collections::BTreeMap<kernel::ids::EntityId, TileId> {
    k.seen_units(orders.foe).into_iter().map(|u| (u.id, u.tile)).collect()
}

fn army_now(k: &Known, orders: &Orders) -> std::collections::BTreeMap<kernel::ids::EntityId, (TileId, u16)> {
    k.own_units().into_iter().filter(|(_, c)| orders.army.contains(&c.id)).map(|(_, c)| (c.id, (c.tile, c.hp))).collect()
}

/// Our turn ends: what the enemy's events will be told against.
pub fn note_turn_end(game: &GameState, rules: &CompiledRuleset) {
    let Some(orders) = HISTORY.with(|h| h.borrow().orders.clone()) else { return };
    let k = Known::new(game, rules, orders.side);
    let snap = (k.round(), army_now(&k, &orders), foes_seen(&k, &orders));
    HISTORY.with(|h| h.borrow_mut().end = Some(snap));
}

/// The enemy's events since our last turn's end (at the first look of our turn).
fn note_enemy_events(k: &Known, round: u32) {
    let Some((orders, end)) = HISTORY.with(|h| {
        let mut h = h.borrow_mut();
        let o = h.orders.clone()?;
        Some((o, h.end.take()?))
    }) else {
        return;
    };
    let (_, army, seen) = end;
    let now_army = army_now(k, &orders);
    let now_seen = foes_seen(k, &orders);
    // The foe's cities the side knows (in sight, or as last sighted).
    let foe_cities: Vec<TileId> = k.foreign_cities(orders.foe).into_iter().map(|c| c.tile).collect();
    let mut events = Vec::new();
    for (id, (tile, hp)) in &army {
        let after = now_army.get(id);
        let lost = hp.saturating_sub(after.map_or(0, |x| x.1));
        if lost == 0 {
            continue;
        }
        let mut f = [0f32; F_HIST];
        f[HK_HIT] = 1.0;
        let t = after.map_or(*tile, |x| x.0);
        subject_numbers(k, &orders, t, &mut f);
        f[15] = f32::from(lost) / 100.0;
        f[17] = if after.is_none() { 1.0 } else { 0.0 };
        f[19] = if foe_cities.iter().any(|c| k.dist(*c, t) <= CITY_ATTACK_RANGE) { 1.0 } else { 0.0 };
        let slot = orders.army.iter().position(|a| a == id).map_or(-1, |s| s as i16);
        events.push(Event { round, slot, f });
    }
    for (id, t) in &now_seen {
        if !seen.contains_key(id) {
            let mut f = [0f32; F_HIST];
            f[HK_SEEN] = 1.0;
            subject_numbers(k, &orders, *t, &mut f);
            events.push(Event { round, slot: -1, f });
        }
    }
    for (id, t) in &seen {
        if !now_seen.contains_key(id) {
            let mut f = [0f32; F_HIST];
            f[HK_GONE] = 1.0;
            subject_numbers(k, &orders, *t, &mut f);
            events.push(Event { round, slot: -1, f });
        }
    }
    HISTORY.with(|h| h.borrow_mut().events.extend(events));
}

/// The facts of one round of ours: whether our army struck a city, which army slots moved.
pub struct RoundFacts {
    pub city_struck: bool,
    pub moved_slots: Vec<i16>,
    pub struck_slots: Vec<i16>,
}

pub fn facts_of_round(round: u32) -> RoundFacts {
    HISTORY.with(|h| {
        let h = h.borrow();
        let mut f = RoundFacts { city_struck: false, moved_slots: Vec::new(), struck_slots: Vec::new() };
        for e in h.events.iter().filter(|e| e.round == round && e.slot >= 0) {
            if e.f[2] > 0.5 || (e.f[3] > 0.5 && e.f[19] > 0.5) {
                f.city_struck = true;
            }
            if (e.f[1] > 0.5 || e.f[2] > 0.5 || e.f[3] > 0.5) && !f.struck_slots.contains(&e.slot) {
                f.struck_slots.push(e.slot);
            }
            if e.f[HK_MOVE] > 0.5 && e.f[18] > 0.0 && !f.moved_slots.contains(&e.slot) {
                f.moved_slots.push(e.slot);
            }
        }
        f
    })
}

/// The army slots that struck a city this round (the condition of `city_strikes`), and those that attacked anything.
pub fn struck_slots_of_round(round: u32) -> (Vec<i16>, Vec<i16>) {
    HISTORY.with(|h| {
        let (mut city, mut any) = (Vec::new(), Vec::new());
        for e in h.borrow().events.iter().filter(|e| e.round == round && e.slot >= 0) {
            if (e.f[2] > 0.5 || (e.f[3] > 0.5 && e.f[19] > 0.5)) && !city.contains(&e.slot) {
                city.push(e.slot);
            }
            if (e.f[1] > 0.5 || e.f[2] > 0.5 || e.f[3] > 0.5) && !any.contains(&e.slot) {
                any.push(e.slot);
            }
        }
        (city, any)
    })
}

/// No history (the ablation).
pub fn mask_history(t: &mut Tokens) {
    t.hist.clear();
    t.hist_slot.clear();
    t.hist_age.clear();
}

/// Our strikes on a city, by round (relative to `start`), from the facts of the play: "r:n;...".
pub fn city_strikes(start: u32) -> String {
    HISTORY.with(|h| {
        let mut by: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
        for e in &h.borrow().events {
            if e.f[2] > 0.5 || (e.f[3] > 0.5 && e.f[19] > 0.5) {
                *by.entry(e.round.saturating_sub(start)).or_default() += 1;
            }
        }
        by.iter().map(|(r, n)| format!("{r}:{n}")).collect::<Vec<_>>().join(";")
    })
}

/// The history tokens now: this turn's and the last `H_TURNS` turns' facts, the most recent first, at most `H_MAX`.
fn history_tokens(round: u32) -> (Vec<[f32; F_HIST]>, Vec<i16>, Vec<u8>) {
    HISTORY.with(|h| {
        let h = h.borrow();
        let mut out = (Vec::new(), Vec::new(), Vec::new());
        for e in h.events.iter().rev() {
            let age = round.saturating_sub(e.round);
            if age > H_TURNS || out.0.len() >= H_MAX {
                break;
            }
            let mut f = e.f;
            f[10] = age as f32 / 3.0;
            round_all(&mut f);
            out.0.push(f);
            out.1.push(e.slot);
            out.2.push(age as u8);
        }
        out
    })
}

thread_local! {
    static HISTORY: std::cell::RefCell<History> = std::cell::RefCell::new(History::default());
}

/// The units' tiles and hit points at the start of our previous turn (numbers 46 and 47 read them; the
/// observation dump writes them as the twin's input).
pub fn history_prev() -> Vec<(kernel::ids::EntityId, TileId, u16)> {
    HISTORY.with(|h| h.borrow().prev.iter().map(|(id, (t, hp))| (*id, *t, *hp)).collect())
}

/// A new play: nothing remembered.
pub fn reset_history() {
    HISTORY.with(|h| *h.borrow_mut() = History::default());
}

/// The first look of a round: this turn's start becomes the remembered one.
fn remember_turn_start(field: &Field, round: u32) {
    HISTORY.with(|h| {
        let mut h = h.borrow_mut();
        if h.round == Some(round) {
            return;
        }
        let now: std::collections::BTreeMap<_, _> = field.own.iter().map(|u| (u.id, (u.tile, u.hp))).chain(field.foes.iter().map(|u| (u.id, (u.tile, u.hp)))).collect();
        h.prev = std::mem::replace(&mut h.this, now);
        h.round = Some(round);
    });
}

/// The first look of a round (the history's enemy events).
fn first_look(k: &Known, round: u32) {
    let new = HISTORY.with(|h| h.borrow().round != Some(round));
    if new {
        note_enemy_events(k, round);
    }
}

/// Nets trained before the history numbers existed read zeros there (`ARMY_NET_NO_HISTORY=1`).
fn history_off() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var_os("ARMY_NET_NO_HISTORY").is_some())
}

fn since_last_turn(u: &Nums) -> (f32, f32) {
    if history_off() {
        return (0.0, 0.0);
    }
    HISTORY.with(|h| match h.borrow().prev.get(&u.id) {
        Some((tile, hp)) => ((f32::from(u.hp) - f32::from(*hp)) / 100.0, if *tile != u.tile { 1.0 } else { 0.0 }),
        None => (0.0, 0.0),
    })
}

/// x rounded to the nearest f16 (ties to even), as an f32.
pub fn f16_round(x: f32) -> f32 {
    f16_to_f32(f32_to_f16(x))
}

pub fn f32_to_f16(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let man = bits & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if man != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = man | 0x80_0000;
        let shift = (14 - e) as u32;
        let half = 1u32 << (shift - 1);
        let rest = m & ((1u32 << shift) - 1);
        let mut v = m >> shift;
        if rest > half || (rest == half && v & 1 == 1) {
            v += 1;
        }
        return sign | v as u16;
    }
    let mut v = ((e as u32) << 10) | (man >> 13);
    let rest = man & 0x1fff;
    if rest > 0x1000 || (rest == 0x1000 && v & 1 == 1) {
        v += 1;
    }
    sign | v as u16
}

pub fn f16_to_f32(h: u16) -> f32 {
    let sign = u32::from(h & 0x8000) << 16;
    let exp = u32::from((h >> 10) & 0x1f);
    let man = u32::from(h & 0x3ff);
    let bits = if exp == 0 {
        if man == 0 {
            sign
        } else {
            let mut e = 127 - 15 + 1;
            let mut m = man;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            sign | ((e as u32) << 23) | ((m & 0x3ff) << 13)
        }
    } else if exp == 0x1f {
        sign | 0x7f80_0000 | (man << 13)
    } else {
        sign | ((exp + 127 - 15) << 23) | (man << 13)
    };
    f32::from_bits(bits)
}

fn round_all(v: &mut [f32]) {
    for x in v.iter_mut() {
        *x = f16_round(*x);
    }
}

/// A unit's numbers as the side knows them: its own (all of them), or another player's (what the game shows of it:
/// kind, tile, owner and hit points; its private numbers read as those of a fresh unit that has not moved or struck).
#[derive(Debug, Clone, Copy)]
struct Nums {
    handle: UnitHandle,
    id: kernel::ids::EntityId,
    tile: TileId,
    owner: PlayerId,
    kind: classic_rules::ids::UnitKindId,
    hp: u16,
    moves_left: u16,
    moves_max: u16,
    attacks_made: u8,
    fortified: bool,
    set_up: bool,
    level: u8,
    experience: u32,
}

impl Nums {
    fn own(handle: UnitHandle, c: &UnitCore) -> Nums {
        Nums { handle, id: c.id, tile: c.tile, owner: c.owner, kind: c.kind, hp: c.hp, moves_left: c.moves_left, moves_max: c.moves_max, attacks_made: c.attacks_made, fortified: matches!(c.stance, Stance::Fortified { .. }), set_up: matches!(c.stance, Stance::SetUp), level: c.level, experience: c.experience }
    }

    fn seen(rules: &CompiledRuleset, u: &SeenUnit) -> Nums {
        let moves_max = (rules.units[u.kind.0 as usize].movement as u16).saturating_mul(MOVE_DENOMINATOR);
        Nums { handle: u.handle, id: u.id, tile: u.tile, owner: u.owner, kind: u.kind, hp: u.hp, moves_left: moves_max, moves_max, attacks_made: 0, fortified: false, set_up: false, level: 1, experience: 0 }
    }
}

/// The facts about the two sides that every token and candidate reads, gathered once per moment, all of them as the
/// side knows them (`Known`).
struct Field {
    own: Vec<Nums>,
    foes: Vec<Nums>,
    foe_cities: Vec<SeenCity>,
    own_cities: Vec<OwnCity>,
    /// The third parties' units the side sees (with their relation) and the cities it knows.
    third: Vec<(Nums, u8)>,
    third_cities: Vec<(SeenCity, u8)>,
}

impl Field {
    fn gather(k: &Known, orders: &Orders) -> Field {
        let rules = k.rules();
        let own: Vec<Nums> = k.own_units().iter().map(|(h, c)| Nums::own(*h, c)).collect();
        let foes: Vec<Nums> = k.seen_units(orders.foe).iter().map(|u| Nums::seen(rules, u)).collect();
        let mut third: Vec<(Nums, u8)> = Vec::new();
        let mut third_cities: Vec<(SeenCity, u8)> = Vec::new();
        for p in (0..k.player_count()).map(PlayerId).filter(|p| *p != orders.side && *p != orders.foe) {
            let units = k.seen_units(p);
            let cities = k.foreign_cities(p);
            if units.is_empty() && cities.is_empty() {
                continue;
            }
            let r = k.relation(orders.foe, p);
            third.extend(units.iter().map(|u| (Nums::seen(rules, u), r)));
            third_cities.extend(cities.into_iter().map(|c| (c, r)));
        }
        third.sort_by_key(|(c, _)| c.id);
        third_cities.sort_by_key(|(c, _)| c.tile.0);
        Field { own, foes, foe_cities: k.foreign_cities(orders.foe), own_cities: k.own_cities(), third, third_cities }
    }

    /// The third parties' military units at war with the side (their threat enters the extras, not the old numbers).
    fn war_third<'a>(&'a self, rules: &'a CompiledRuleset) -> impl Iterator<Item = &'a Nums> + 'a {
        self.third.iter().filter(move |(u, r)| hostile(*r) && !act::is_civilian(rules, u.kind)).map(|(u, _)| u)
    }

    /// The third parties' war threat on tile `t`: (reach it next turn, adjacent, the nearest's distance).
    fn war_third_threat(&self, k: &Known, t: TileId) -> (i32, i32, i32) {
        let rules = k.rules();
        let mut reach = 0;
        let mut adj = 0;
        let mut near = 30;
        for u in self.war_third(rules) {
            let d = k.dist(u.tile, t);
            if d <= act::moves_of(rules, u.kind) + act::range_of(rules, u.kind).unwrap_or(1) {
                reach += 1;
            }
            if d == 1 {
                adj += 1;
            }
            near = near.min(d);
        }
        (reach, adj, near)
    }

    fn military_foes<'a>(&'a self, rules: &'a CompiledRuleset) -> impl Iterator<Item = &'a Nums> + 'a {
        self.foes.iter().filter(move |u| !act::is_civilian(rules, u.kind))
    }

    /// Foe military units that could strike `t` in their next turn (move + reach).
    fn threat(&self, k: &Known, t: TileId) -> i32 {
        let rules = k.rules();
        self.military_foes(rules).filter(|u| k.dist(u.tile, t) <= act::moves_of(rules, u.kind) + act::range_of(rules, u.kind).unwrap_or(1)).count() as i32
    }

    fn nearest_foe(&self, k: &Known, t: TileId) -> i32 {
        self.military_foes(k.rules()).map(|u| k.dist(u.tile, t)).min().unwrap_or(30)
    }

    fn adjacent(&self, k: &Known, t: TileId, except: Option<kernel::ids::EntityId>) -> (i32, i32) {
        let rules = k.rules();
        let foes = self.military_foes(rules).filter(|u| k.dist(u.tile, t) == 1).count() as i32;
        let friends = self.own.iter().filter(|u| Some(u.id) != except && !act::is_civilian(rules, u.kind) && k.dist(u.tile, t) <= 1).count() as i32;
        (foes, friends)
    }

    fn in_city_fire(&self, k: &Known, t: TileId) -> bool {
        self.foe_cities.iter().any(|c| k.dist(c.tile, t) <= CITY_ATTACK_RANGE)
    }

    fn refuge_distance(&self, k: &Known, orders: &Orders, t: TileId) -> i32 {
        match orders.refuge {
            Some(r) => k.dist(r, t),
            None => self.own_cities.iter().map(|c| k.dist(c.tile, t)).min().unwrap_or(30),
        }
    }
}

pub fn arm_slot(rules: &CompiledRuleset, kind: classic_rules::ids::UnitKindId) -> usize {
    if act::is_civilian(rules, kind) {
        return ARMS.len();
    }
    let arms = arms_of(rules, kind);
    // Hit-and-run is a trait on top of the base arm: the base arm goes first.
    let base = arms.iter().find(|a| **a != Arm::HitRun).or(arms.first());
    base.and_then(|a| ARMS.iter().position(|x| x == a)).unwrap_or(ARMS.len())
}

/// The tokens of the moment for `orders.side`, from what the side knows.
pub fn tokens(game: &GameState, rules: &CompiledRuleset, orders: &Orders) -> Tokens {
    build_tokens(&Known::new(game, rules, orders.side), orders)
}

fn build_tokens(k: &Known, orders: &Orders) -> Tokens {
    let rules = k.rules();
    let field = Field::gather(k, orders);
    let round = k.round();
    first_look(k, round);
    remember_turn_start(&field, round);
    let (ox, oy) = k.col_row(orders.tile);
    let (hist, hist_slot, hist_age) = history_tokens(round);
    let mut t = Tokens { feat: Vec::new(), dist: Vec::new(), selectable: Vec::new(), unit: Vec::new(), tile: Vec::new(), pos: Vec::new(), price: orders.price, hist, hist_slot, hist_age, xfeat: Vec::new(), third: Third::default() };
    // The objective.
    let mut f = [0f32; F_TOK];
    f[TOK_OBJECTIVE] = 1.0;
    f[7 + orders.aim.index()] = 1.0;
    let left = (orders.start_round + orders.rounds).saturating_sub(round);
    f[7 + AIMS] = left as f32 / 30.0;
    f[8 + AIMS] = orders.rounds as f32 / 30.0;
    f[9 + AIMS] = if orders.optional { 1.0 } else { 0.0 };
    let own_army: Vec<&Nums> = field.own.iter().filter(|u| orders.army.contains(&u.id)).collect();
    f[10 + AIMS] = own_army.len() as f32 / 8.0;
    f[11 + AIMS] = field.military_foes(rules).count() as f32 / 8.0;
    f[12 + AIMS] = own_army.iter().map(|u| f32::from(u.hp)).sum::<f32>() / 800.0;
    f[13 + AIMS] = field.military_foes(rules).map(|u| f32::from(u.hp)).sum::<f32>() / 800.0;
    t.feat.push(f);
    t.xfeat.push([0.0; X_TOK]);
    t.selectable.push(false);
    t.unit.push(None);
    t.tile.push(orders.tile);
    let foe_rel = k.relation(orders.foe, orders.foe);
    // Units: ours, then theirs.
    for (own, list) in [(true, &field.own), (false, &field.foes)] {
        for u in list.iter() {
            if t.len() >= T_MAX {
                break;
            }
            let promotable = own && orders.army.contains(&u.id) && k.promotions_available(u.handle);
            // A ranged unit set up (or with no moves left) may still shoot this turn.
            let may_shoot = act::range_of(rules, u.kind).is_some() && u.attacks_made == 0;
            let selectable = own && orders.army.contains(&u.id) && (u.moves_left > 0 || promotable || may_shoot);
            let mut f = [0f32; F_TOK];
            f[if own { TOK_OWN } else { TOK_FOE }] = 1.0;
            unit_features(k, orders, &field, u, own, promotable, (ox, oy), &mut f);
            f[42] = if selectable { 1.0 } else { 0.0 };
            let (gained, moved) = since_last_turn(u);
            f[46] = gained;
            f[47] = moved;
            let mut x = [0f32; X_TOK];
            x[usize::from(if own { R_OWN } else { foe_rel })] = 1.0;
            if own {
                let (reach, adj, near) = field.war_third_threat(k, u.tile);
                x[6] = reach as f32 / 4.0;
                x[7] = adj as f32 / 3.0;
                x[8] = near as f32 / 12.0;
            }
            t.feat.push(f);
            t.xfeat.push(x);
            t.selectable.push(selectable);
            t.unit.push(Some(u.handle));
            t.tile.push(u.tile);
        }
    }
    // Cities.
    for (own, list) in [(true, field.own_cities.iter().map(|c| CityNums::own(c)).collect::<Vec<_>>()), (false, field.foe_cities.iter().map(CityNums::seen).collect::<Vec<_>>())] {
        for c in list.iter() {
            if t.len() >= T_MAX {
                break;
            }
            let f = city_features(k, orders, c, own, (ox, oy));
            let mut x = [0f32; X_TOK];
            x[usize::from(if own { R_OWN } else { foe_rel })] = 1.0;
            t.feat.push(f);
            t.xfeat.push(x);
            t.selectable.push(false);
            t.unit.push(None);
            t.tile.push(c.tile);
        }
    }
    for f in t.feat.iter_mut() {
        round_all(f);
    }
    for x in t.xfeat.iter_mut() {
        round_all(x);
    }
    t.pos = t.tile.iter().map(|x| pos_of(orders, *x)).collect();
    let n = t.len();
    t.dist = vec![0; n * n];
    for i in 0..n {
        for j in 0..n {
            t.dist[i * n + j] = k.dist(t.tile[i], t.tile[j]).clamp(0, i32::from(D_MAX)) as u8;
        }
    }
    t.third = third_tokens(k, orders, &field, &t, (ox, oy));
    t
}

/// A city's numbers as the side knows them: its own (all), another player's (the owner's population, the health bar
/// in sight; the strength and whether it struck are not shown: they read as an unhurt city that may still strike).
#[derive(Debug, Clone, Copy)]
struct CityNums {
    tile: TileId,
    population: u32,
    health: f32,
    strength: i64,
    struck: bool,
}

impl CityNums {
    fn own(c: &OwnCity) -> CityNums {
        CityNums { tile: c.tile, population: c.population, health: c.health, strength: c.strength, struck: c.struck }
    }

    fn seen(c: &SeenCity) -> CityNums {
        CityNums { tile: c.tile, population: c.population, health: c.health.unwrap_or(1.0), strength: 0, struck: false }
    }
}

/// A city's token numbers (`own`: the side's; else number 45 is 0).
fn city_features(k: &Known, orders: &Orders, city: &CityNums, own: bool, (ox, oy): (f32, f32)) -> [f32; F_TOK] {
    let mut f = [0f32; F_TOK];
    f[TOK_CITY] = 1.0;
    let (x, y) = k.col_row(city.tile);
    f[4] = (x - ox) / 12.0;
    f[5] = (y - oy) / 12.0;
    f[6] = k.dist(city.tile, orders.tile) as f32 / 12.0;
    f[7] = city.health;
    f[11] = city.strength as f32 / 100.0 / orders.mean_strength;
    f[12] = if city.struck { 0.0 } else { 1.0 };
    f[43] = if city.tile == orders.tile { 1.0 } else { 0.0 };
    f[44] = city.population as f32 / 10.0;
    f[45] = if own { 1.0 } else { 0.0 };
    f
}

/// The third tokens of the moment (`Third`): in the window, nearest to the objective first, within the budget.
fn third_tokens(k: &Known, orders: &Orders, field: &Field, main: &Tokens, origin: (f32, f32)) -> Third {
    let budget = X_CAP.min(FIELD_BUDGET.saturating_sub(main.len()));
    let mut out = Third::default();
    if budget == 0 {
        return out;
    }
    // (distance to the objective, unit 0 / city 1, order key, index).
    let mut pick: Vec<(i32, u8, u64, usize)> = Vec::new();
    for (i, (u, _)) in field.third.iter().enumerate() {
        if orders.window.index(u.tile).is_some() {
            pick.push((k.dist(u.tile, orders.tile), 0, u.id.0, i));
        }
    }
    for (i, (c, _)) in field.third_cities.iter().enumerate() {
        if orders.window.index(c.tile).is_some() {
            pick.push((k.dist(c.tile, orders.tile), 1, u64::from(c.tile.0), i));
        }
    }
    pick.sort_unstable();
    pick.truncate(budget);
    for (_, kind, _, i) in pick {
        let (f, tile, r) = if kind == 0 {
            let (u, r) = &field.third[i];
            let mut f = [0f32; F_TOK];
            f[TOK_FOE] = 1.0;
            unit_features(k, orders, field, u, false, false, origin, &mut f);
            let (gained, moved) = since_last_turn(u);
            f[46] = gained;
            f[47] = moved;
            (f, u.tile, *r)
        } else {
            let (c, r) = &field.third_cities[i];
            (city_features(k, orders, &CityNums::seen(c), false, origin), c.tile, *r)
        };
        let mut f = f;
        round_all(&mut f);
        let mut x = [0f32; X_TOK];
        x[usize::from(r)] = 1.0;
        out.feat.push(f);
        out.x.push(x);
        out.pos.push(pos_of(orders, tile));
        out.tile.push(tile);
    }
    let (n, m) = (main.len(), out.len());
    out.dist = vec![0; n * m];
    for i in 0..n {
        for j in 0..m {
            out.dist[i * m + j] = k.dist(main.tile[i], out.tile[j]).clamp(0, i32::from(D_MAX)) as u8;
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn unit_features(k: &Known, orders: &Orders, field: &Field, u: &Nums, own: bool, promotable: bool, (ox, oy): (f32, f32), f: &mut [f32; F_TOK]) {
    let rules = k.rules();
    let d = &rules.units[u.kind.0 as usize];
    let (x, y) = k.col_row(u.tile);
    let ground = k.ground(u.tile).unwrap_or_default();
    f[4] = (x - ox) / 12.0;
    f[5] = (y - oy) / 12.0;
    f[6] = k.dist(u.tile, orders.tile) as f32 / 12.0;
    f[7] = f32::from(u.hp) / 100.0;
    f[8] = if u.moves_max > 0 { f32::from(u.moves_left) / f32::from(u.moves_max) } else { 0.0 };
    f[9] = f32::from(u.moves_max) / f32::from(MOVE_DENOMINATOR) / 5.0;
    f[10] = if u.attacks_made == 0 { 1.0 } else { 0.0 };
    f[11] = f32::from(d.strength) / orders.mean_strength;
    f[12] = f32::from(d.ranged_strength) / orders.mean_strength;
    f[13] = act::range_of(rules, u.kind).unwrap_or(0) as f32 / 3.0;
    f[14 + arm_slot(rules, u.kind)] = 1.0;
    f[27] = if u.fortified { 1.0 } else { 0.0 };
    f[28] = if u.set_up { 1.0 } else { 0.0 };
    f[29] = if d.domain == classic_rules::rules::content::Domain::Land && ground.water { 1.0 } else { 0.0 };
    f[30] = f32::from(u.level) / 5.0;
    f[31] = u.experience as f32 / 60.0;
    f[32] = if promotable { 1.0 } else { 0.0 };
    f[33] = k.defence(u.tile, u.owner) as f32 / 100.0;
    f[34] = if ground.hills { 1.0 } else { 0.0 };
    f[35] = if ground.rough { 1.0 } else { 0.0 };
    if own {
        f[36] = if field.in_city_fire(k, u.tile) { 1.0 } else { 0.0 };
        let (foes, friends) = field.adjacent(k, u.tile, Some(u.id));
        f[37] = foes as f32 / 3.0;
        f[38] = friends as f32 / 3.0;
        f[39] = field.threat(k, u.tile) as f32 / 4.0;
        f[40] = field.nearest_foe(k, u.tile) as f32 / 12.0;
        f[41] = field.refuge_distance(k, orders, u.tile) as f32 / 12.0;
    } else {
        // The foe's view of the same: our units next to it, our units that reach it.
        let near_own = field.own.iter().filter(|o| !act::is_civilian(rules, o.kind) && k.dist(o.tile, u.tile) == 1).count();
        f[37] = near_own as f32 / 3.0;
        let reach = field.own.iter().filter(|o| !act::is_civilian(rules, o.kind) && k.dist(o.tile, u.tile) <= act::moves_of(rules, o.kind) + act::range_of(rules, o.kind).unwrap_or(1)).count();
        f[39] = reach as f32 / 4.0;
        f[40] = field.own.iter().filter(|o| !act::is_civilian(rules, o.kind)).map(|o| k.dist(o.tile, u.tile)).min().unwrap_or(30) as f32 / 12.0;
    }
}

fn pos_of(orders: &Orders, t: TileId) -> i16 {
    orders.window.index(t).map_or(-1, |i| i as i16)
}

/// The legal actions of our unit `unit` now, with their features; "done" first.
pub fn candidates(game: &GameState, rules: &CompiledRuleset, orders: &Orders, unit: UnitHandle) -> Vec<Candidate> {
    build_candidates(&Known::new(game, rules, orders.side), orders, unit)
}

fn build_candidates(k: &Known, orders: &Orders, unit: UnitHandle) -> Vec<Candidate> {
    let mut out = Vec::new();
    let Some(me) = k.own_core(unit) else { return out };
    let field = Field::gather(k, orders);
    let here = Spot::of(k, orders, &field, &me, me.tile);
    let mut done = Candidate { feat: [0.0; F_ACT], x: cand_extras(k, orders, &field, me.tile, None), promo: 0, pos: pos_of(orders, me.tile), action: Action::Done };
    done.feat[Kind::Done as usize] = 1.0;
    here.write(&here, &mut done.feat);
    out.push(done);
    let Some((commands, reach)) = k.legal(unit, RADIUS) else { return out };
    for cmd in commands {
        let Some(c) = candidate(k, orders, &field, (unit, &me), &here, &reach, cmd) else { continue };
        out.push(c);
    }
    for c in out.iter_mut() {
        round_all(&mut c.feat);
        round_all(&mut c.x);
    }
    out
}

/// A candidate's extras (`X_ACT`): the relation of the strike's target (`target`: its owner) and "declares war", the
/// third parties' war threat on the candidate's tile `t`.
fn cand_extras(k: &Known, orders: &Orders, field: &Field, t: TileId, target: Option<PlayerId>) -> [f32; X_ACT] {
    let mut x = [0f32; X_ACT];
    if let Some(owner) = target {
        let r = k.relation(orders.foe, owner);
        x[usize::from(r)] = 1.0;
        x[X_DECLARES] = if hostile(r) || r == R_OWN { 0.0 } else { 1.0 };
    }
    let (reach, adj, _) = field.war_third_threat(k, t);
    x[7] = reach as f32 / 4.0;
    x[8] = adj as f32 / 3.0;
    x
}

/// The facts of one tile for one of our units.
struct Spot {
    tile: TileId,
    dq: f32,
    dr: f32,
    defence: f32,
    hills: bool,
    rough: bool,
    city_fire: bool,
    adj_foes: i32,
    adj_friends: i32,
    threat: i32,
    nearest_foe: i32,
    objective: i32,
    refuge: i32,
    on_objective: bool,
    own_land: bool,
}

impl Spot {
    fn of(k: &Known, orders: &Orders, field: &Field, me: &UnitCore, t: TileId) -> Spot {
        let (x, y) = k.col_row(t);
        let (ux, uy) = k.col_row(me.tile);
        let (adj_foes, adj_friends) = field.adjacent(k, t, Some(me.id));
        let ground = k.ground(t).unwrap_or_default();
        Spot {
            tile: t,
            dq: x - ux,
            dr: y - uy,
            defence: k.defence(t, orders.side) as f32,
            hills: ground.hills,
            rough: ground.rough,
            city_fire: field.in_city_fire(k, t),
            adj_foes,
            adj_friends,
            threat: field.threat(k, t),
            nearest_foe: field.nearest_foe(k, t),
            objective: k.dist(t, orders.tile),
            refuge: field.refuge_distance(k, orders, t),
            on_objective: t == orders.tile,
            own_land: k.own_land(t),
        }
    }

    /// Numbers 10..=26 of a candidate: this tile, against the unit's own tile `here`.
    fn write(&self, here: &Spot, f: &mut [f32; F_ACT]) {
        f[10] = self.dq / 12.0;
        f[11] = self.dr / 12.0;
        f[12] = (self.dq.abs().max(self.dr.abs())) / 5.0;
        f[13] = self.defence / 100.0;
        f[14] = if self.hills { 1.0 } else { 0.0 };
        f[15] = if self.rough { 1.0 } else { 0.0 };
        f[16] = if self.city_fire { 1.0 } else { 0.0 };
        f[17] = self.adj_foes as f32 / 3.0;
        f[18] = self.adj_friends as f32 / 3.0;
        f[19] = self.threat as f32 / 4.0;
        f[20] = self.nearest_foe as f32 / 12.0;
        f[21] = (self.nearest_foe - here.nearest_foe) as f32 / 5.0;
        f[22] = self.objective as f32 / 12.0;
        f[23] = (self.objective - here.objective) as f32 / 5.0;
        f[24] = self.refuge as f32 / 12.0;
        f[25] = if self.on_objective { 1.0 } else { 0.0 };
        f[26] = if self.own_land { 1.0 } else { 0.0 };
    }
}

fn kind_of(cmd: &Command) -> Option<Kind> {
    Some(match cmd {
        Command::MoveUnit { .. } => Kind::Move,
        Command::AttackUnit { .. } => Kind::AttackUnit,
        Command::AttackCity { .. } => Kind::AttackCity,
        Command::RangedAttack { .. } => Kind::Ranged,
        Command::Fortify { .. } => Kind::Fortify,
        Command::SetUpRangedAttack { .. } => Kind::SetUp,
        Command::Promote { .. } => Kind::Promote,
        Command::Pillage { .. } => Kind::Pillage,
        Command::Rebase { .. } | Command::AirSweep { .. } | Command::Intercept { .. } | Command::RebaseToCarrier { .. } => Kind::Other,
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
fn candidate(k: &Known, orders: &Orders, field: &Field, (unit, me): (UnitHandle, &UnitCore), here: &Spot, reach: &[(TileId, u16)], cmd: Command) -> Option<Candidate> {
    let kind = kind_of(&cmd)?;
    let mut f = [0f32; F_ACT];
    f[kind as usize] = 1.0;
    let mut promo = 0u16;
    let mut subject = here.tile;
    let mut target = None;
    match &cmd {
        Command::MoveUnit { destination, .. } => {
            let spot = Spot::of(k, orders, field, me, *destination);
            spot.write(here, &mut f);
            subject = *destination;
            let left = reach.iter().find(|(t, _)| t == destination).map_or(0, |(_, l)| *l);
            f[27] = if me.moves_max > 0 { f32::from(left) / f32::from(me.moves_max) } else { 0.0 };
            let (blow, count) = blows_from(k, field, (unit, me), *destination);
            f[28] = blow;
            f[29] = count as f32 / 3.0;
        }
        Command::Promote { promotion, .. } => {
            promo = promotion.0 + 1;
            here.write(here, &mut f);
        }
        Command::AttackUnit { .. } | Command::AttackCity { .. } | Command::RangedAttack { .. } | Command::AirSweep { .. } => {
            let t = k.target_tile(&cmd)?;
            subject = t;
            target = k.target_owner(t);
            let spot = Spot::of(k, orders, field, me, t);
            spot.write(here, &mut f);
            strike_features(k, orders, (unit, me), t, &mut f);
        }
        _ => here.write(here, &mut f),
    }
    Some(Candidate { feat: f, x: cand_extras(k, orders, field, subject, target), promo, pos: pos_of(orders, subject), action: Action::Command(cmd) })
}

/// The best blow `me` could deal from `from` (dealt over the defender's hit points, 1 = a kill) and how
/// many foes it could strike there: the foes the side sees, by the interface's combat preview.
fn blows_from(k: &Known, field: &Field, (unit, me): (UnitHandle, &UnitCore), from: TileId) -> (f32, i32) {
    let reach = act::range_of(k.rules(), me.kind).unwrap_or(1);
    let mut best = 0f32;
    let mut count = 0;
    for foe in field.foes.iter() {
        if k.dist(foe.tile, from) > reach {
            continue;
        }
        count += 1;
        if let Some((dealt, _, hp)) = k.preview(unit, Some(from), foe.tile) {
            best = best.max((dealt as f32 / (hp.max(1)) as f32).min(1.0));
        }
    }
    for c in field.foe_cities.iter() {
        if k.dist(c.tile, from) <= reach {
            count += 1;
        }
    }
    (best, count)
}

fn strike_features(k: &Known, orders: &Orders, (unit, me): (UnitHandle, &UnitCore), t: TileId, f: &mut [f32; F_ACT]) {
    let rules = k.rules();
    match k.preview(unit, None, t) {
        Some((dealt, taken, hp)) => {
            f[30] = dealt as f32 / 100.0;
            f[31] = taken as f32 / 100.0;
            f[32] = hp as f32 / 100.0;
            f[33] = if dealt >= hp { 1.0 } else { 0.0 };
            f[34] = if taken >= i64::from(me.hp) { 1.0 } else { 0.0 };
        }
        None => f[39] = 1.0,
    }
    if let Some(u) = k.seen_unit_on(t) {
        f[35] = if act::range_of(rules, u.kind).is_some() { 1.0 } else { 0.0 };
        f[36] = if act::is_civilian(rules, u.kind) { 1.0 } else { 0.0 };
        f[38] = base_strength(rules, u.kind) / orders.mean_strength;
    }
    if k.city_owner_on(t).is_some() {
        f[37] = 1.0;
    }
}
