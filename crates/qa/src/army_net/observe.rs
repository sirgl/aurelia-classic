//! What the army net sees (DESIGN.md, «Inputs»). Every number is rounded through f16, the precision
//! the recorder stores, so the player's input equals the training input bit for bit.
//!
//! Tokens, in this order: the objective; the units of both sides (ours first, by id); the cities of
//! both sides. Each token is `F_TOK` numbers (the layout below); `dist` holds the hex distance between
//! the tokens' tiles (the attention bias). Candidates: the legal actions of one of our units, each
//! `F_ACT` numbers and a promotion id.

use classic_rules::combat::city::{city_strength, hit_points, max_hit_points, CITY_ATTACK_RANGE};
use classic_rules::ids::{PlayerId, TileId, UnitHandle};
use classic_rules::knowledge::actions::unit_actions;
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::{ClassicGame, Command, CompiledRuleset, GameState};
use kernel::navigation::terrain::{is_hills, is_rough};
use kernel::world::unit::{Stance, UnitCore, MOVE_DENOMINATOR};

use super::{base_strength, Orders, AIMS};
use crate::trainers::act;
use crate::trainers::lab::{arms_of, Arm, ARMS};

pub const F_TOK: usize = 48;
pub const F_ACT: usize = 40;
/// Tokens at most: the objective, 39 units and cities.
pub const T_MAX: usize = 40;
/// Distances in the attention bias are clipped here.
pub const D_MAX: u8 = 15;
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
    /// Variant MH: the history tokens (`F_HIST` numbers), the army slot of each (-1: an enemy's), its age in turns.
    pub hist: Vec<[f32; F_HIST]>,
    pub hist_slot: Vec<i16>,
    pub hist_age: Vec<u8>,
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

fn hp_at(game: &GameState, rules: &CompiledRuleset, t: TileId, side: PlayerId) -> Option<(i64, bool)> {
    let auth = game.view().auth;
    if let Some(c) = auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)) {
        return Some((i64::from(hit_points(auth, rules, c)), true));
    }
    auth.indexes().units_on(t).iter().filter_map(|h| auth.units().get(*h)).find(|u| u.core.owner != side).map(|u| (i64::from(u.core.hp), false))
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
    let core = *game.view().auth.units().get(unit)?.core;
    let slot = orders.army.iter().position(|a| *a == core.id)? as i16;
    let kind = match cmd {
        Command::MoveUnit { .. } | Command::MarchTo { .. } => HK_MOVE,
        Command::AttackUnit { .. } => 1,
        Command::AttackCity { .. } => 2,
        Command::RangedAttack { .. } => 3,
        Command::Fortify { .. } | Command::SetUpRangedAttack { .. } => 4,
        _ => 5,
    };
    let target = target_tile(game, cmd);
    let (target_hp, target_city) = match target.and_then(|t| hp_at(game, rules, t, side)) {
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
    let auth = game.view().auth;
    let now = auth.units().get(p.unit).filter(|u| u.core.id == p.id).map(|u| *u.core);
    let mut f = [0f32; F_HIST];
    f[p.kind] = 1.0;
    let subject = p.target.or(now.map(|c| c.tile)).unwrap_or(p.tile);
    subject_numbers(game, &orders, subject, &mut f);
    if let (Some(t), Some(before)) = (p.target, p.target_hp) {
        let after = hp_at(game, rules, t, orders.side).filter(|(_, city)| *city == p.target_city).map_or(0, |x| x.0);
        f[14] = (before - after).max(0) as f32 / 100.0;
        f[16] = if after == 0 || (p.target_city && auth.indexes().city_on_tile(t).and_then(|c| auth.cities().get(c)).is_some_and(|c| c.owner == orders.side)) { 1.0 } else { 0.0 };
    }
    f[15] = f32::from(p.hp.saturating_sub(now.map_or(0, |c| c.hp))) / 100.0;
    f[17] = if now.is_none() { 1.0 } else { 0.0 };
    f[18] = now.map_or(0, |c| dist(game, c.tile, p.tile)) as f32 / 5.0;
    f[19] = if p.target_city { 1.0 } else { 0.0 };
    let round = auth.turn().round;
    HISTORY.with(|h| h.borrow_mut().events.push(Event { round, slot: p.slot, f }));
}

fn subject_numbers(game: &GameState, orders: &Orders, t: TileId, f: &mut [f32; F_HIST]) {
    let (ox, oy) = col_row(game, orders.tile);
    let (x, y) = col_row(game, t);
    f[11] = (x - ox) / 12.0;
    f[12] = (y - oy) / 12.0;
    f[13] = dist(game, t, orders.tile) as f32 / 12.0;
}

/// The foes the side sees (all of them where the play is not in the fog).
fn foes_seen(game: &GameState, rules: &CompiledRuleset, orders: &Orders) -> std::collections::BTreeMap<kernel::ids::EntityId, TileId> {
    let auth = game.view().auth;
    let view = game.player_view(rules, orders.side);
    auth.indexes().units_of(orders.foe).iter().filter_map(|h| auth.units().get(*h)).map(|u| *u.core).filter(|c| !orders.fog || view.sees_unit(c)).map(|c| (c.id, c.tile)).collect()
}

fn army_now(game: &GameState, orders: &Orders) -> std::collections::BTreeMap<kernel::ids::EntityId, (TileId, u16)> {
    let auth = game.view().auth;
    auth.indexes().units_of(orders.side).iter().filter_map(|h| auth.units().get(*h)).filter(|u| orders.army.contains(&u.core.id)).map(|u| (u.core.id, (u.core.tile, u.core.hp))).collect()
}

/// Our turn ends: what the enemy's events will be told against.
pub fn note_turn_end(game: &GameState, rules: &CompiledRuleset) {
    let Some(orders) = HISTORY.with(|h| h.borrow().orders.clone()) else { return };
    let snap = (game.view().auth.turn().round, army_now(game, &orders), foes_seen(game, rules, &orders));
    HISTORY.with(|h| h.borrow_mut().end = Some(snap));
}

/// The enemy's events since our last turn's end (at the first look of our turn).
fn note_enemy_events(game: &GameState, rules: &CompiledRuleset, round: u32) {
    let Some((orders, end)) = HISTORY.with(|h| {
        let mut h = h.borrow_mut();
        let o = h.orders.clone()?;
        Some((o, h.end.take()?))
    }) else {
        return;
    };
    let (_, army, seen) = end;
    let now_army = army_now(game, &orders);
    let now_seen = foes_seen(game, rules, &orders);
    let foe_cities: Vec<TileId> = {
        let auth = game.view().auth;
        auth.indexes().cities_of(orders.foe).iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.tile).collect()
    };
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
        subject_numbers(game, &orders, t, &mut f);
        f[15] = f32::from(lost) / 100.0;
        f[17] = if after.is_none() { 1.0 } else { 0.0 };
        f[19] = if foe_cities.iter().any(|c| dist(game, *c, t) <= CITY_ATTACK_RANGE) { 1.0 } else { 0.0 };
        let slot = orders.army.iter().position(|a| a == id).map_or(-1, |s| s as i16);
        events.push(Event { round, slot, f });
    }
    for (id, t) in &now_seen {
        if !seen.contains_key(id) {
            let mut f = [0f32; F_HIST];
            f[HK_SEEN] = 1.0;
            subject_numbers(game, &orders, *t, &mut f);
            events.push(Event { round, slot: -1, f });
        }
    }
    for (id, t) in &seen {
        if !now_seen.contains_key(id) {
            let mut f = [0f32; F_HIST];
            f[HK_GONE] = 1.0;
            subject_numbers(game, &orders, *t, &mut f);
            events.push(Event { round, slot: -1, f });
        }
    }
    HISTORY.with(|h| h.borrow_mut().events.extend(events));
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

/// A new play: nothing remembered.
pub fn reset_history() {
    HISTORY.with(|h| *h.borrow_mut() = History::default());
}

/// The first look of a round: this turn's start becomes the remembered one.
fn remember_turn_start(game: &GameState, field: &Field, round: u32) {
    HISTORY.with(|h| {
        let mut h = h.borrow_mut();
        if h.round == Some(round) {
            return;
        }
        let now: std::collections::BTreeMap<_, _> = field.own.iter().chain(&field.foes).map(|u| (u.id, (u.tile, u.hp))).collect();
        h.prev = std::mem::replace(&mut h.this, now);
        h.round = Some(round);
        let _ = game;
    });
}

/// The first look of a round (the history's enemy events).
fn first_look(game: &GameState, rules: &CompiledRuleset, round: u32) {
    let new = HISTORY.with(|h| h.borrow().round != Some(round));
    if new {
        note_enemy_events(game, rules, round);
    }
}

/// Nets trained before the history numbers existed read zeros there (`ARMY_NET_NO_HISTORY=1`).
fn history_off() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var_os("ARMY_NET_NO_HISTORY").is_some())
}

fn since_last_turn(u: &UnitCore) -> (f32, f32) {
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

fn dist(game: &GameState, a: TileId, b: TileId) -> i32 {
    game.view().auth.map().dimensions.distance(a, b).unwrap_or(99)
}

/// Column and row of a tile (offset coordinates of the map).
fn col_row(game: &GameState, t: TileId) -> (f32, f32) {
    let w = game.view().auth.map().dimensions.width as u32;
    ((t.0 % w) as f32, (t.0 / w) as f32)
}

/// The facts about the two sides that every token and candidate reads, gathered once per moment.
struct Field {
    own: Vec<UnitCore>,
    foes: Vec<UnitCore>,
    foe_cities: Vec<TileId>,
    own_cities: Vec<TileId>,
}

impl Field {
    fn gather(game: &GameState, rules: &CompiledRuleset, orders: &Orders) -> Field {
        let auth = game.view().auth;
        let units_of = |p: PlayerId| -> Vec<UnitCore> {
            let mut v: Vec<UnitCore> = auth.indexes().units_of(p).iter().filter_map(|h| auth.units().get(*h)).map(|u| *u.core).collect();
            v.sort_by_key(|c| c.id);
            v
        };
        let cities_of = |p: PlayerId| -> Vec<TileId> {
            let mut v: Vec<TileId> = auth.indexes().cities_of(p).iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.tile).collect();
            v.sort_by_key(|t| t.0);
            v
        };
        let mut foes = units_of(orders.foe);
        if orders.fog {
            let view = game.player_view(rules, orders.side);
            foes.retain(|c| view.sees_unit(c));
        }
        Field { own: units_of(orders.side), foes, foe_cities: cities_of(orders.foe), own_cities: cities_of(orders.side) }
    }

    fn military_foes<'a>(&'a self, rules: &'a CompiledRuleset) -> impl Iterator<Item = &'a UnitCore> + 'a {
        self.foes.iter().filter(move |u| !act::is_civilian(rules, u.kind))
    }

    /// Foe military units that could strike `t` in their next turn (move + reach).
    fn threat(&self, game: &GameState, rules: &CompiledRuleset, t: TileId) -> i32 {
        self.military_foes(rules).filter(|u| dist(game, u.tile, t) <= act::moves_of(rules, u.kind) + act::range_of(rules, u.kind).unwrap_or(1)).count() as i32
    }

    fn nearest_foe(&self, game: &GameState, rules: &CompiledRuleset, t: TileId) -> i32 {
        self.military_foes(rules).map(|u| dist(game, u.tile, t)).min().unwrap_or(30)
    }

    fn adjacent(&self, game: &GameState, rules: &CompiledRuleset, t: TileId, except: Option<kernel::ids::EntityId>) -> (i32, i32) {
        let foes = self.military_foes(rules).filter(|u| dist(game, u.tile, t) == 1).count() as i32;
        let friends = self.own.iter().filter(|u| Some(u.id) != except && !act::is_civilian(rules, u.kind) && dist(game, u.tile, t) <= 1).count() as i32;
        (foes, friends)
    }

    fn in_city_fire(&self, game: &GameState, t: TileId) -> bool {
        self.foe_cities.iter().any(|c| dist(game, *c, t) <= CITY_ATTACK_RANGE)
    }

    fn refuge_distance(&self, game: &GameState, orders: &Orders, t: TileId) -> i32 {
        match orders.refuge {
            Some(r) => dist(game, r, t),
            None => self.own_cities.iter().map(|c| dist(game, *c, t)).min().unwrap_or(30),
        }
    }
}

fn arm_slot(rules: &CompiledRuleset, kind: classic_rules::ids::UnitKindId) -> usize {
    if act::is_civilian(rules, kind) {
        return ARMS.len();
    }
    let arms = arms_of(rules, kind);
    // Hit-and-run is a trait on top of the base arm: the base arm goes first.
    let base = arms.iter().find(|a| **a != Arm::HitRun).or(arms.first());
    base.and_then(|a| ARMS.iter().position(|x| x == a)).unwrap_or(ARMS.len())
}

/// The tokens of the moment for `orders.side`.
pub fn tokens(game: &GameState, rules: &CompiledRuleset, orders: &Orders) -> Tokens {
    let field = Field::gather(game, rules, orders);
    let auth = game.view().auth;
    let round = auth.turn().round;
    first_look(game, rules, round);
    remember_turn_start(game, &field, round);
    let (ox, oy) = col_row(game, orders.tile);
    let (hist, hist_slot, hist_age) = history_tokens(round);
    let mut t = Tokens { feat: Vec::new(), dist: Vec::new(), selectable: Vec::new(), unit: Vec::new(), tile: Vec::new(), pos: Vec::new(), hist, hist_slot, hist_age };
    // The objective.
    let mut f = [0f32; F_TOK];
    f[TOK_OBJECTIVE] = 1.0;
    f[7 + orders.aim.index()] = 1.0;
    let left = (orders.start_round + orders.rounds).saturating_sub(round);
    f[7 + AIMS] = left as f32 / 30.0;
    f[8 + AIMS] = orders.rounds as f32 / 30.0;
    f[9 + AIMS] = if orders.optional { 1.0 } else { 0.0 };
    let own_army: Vec<&UnitCore> = field.own.iter().filter(|u| orders.army.contains(&u.id)).collect();
    f[10 + AIMS] = own_army.len() as f32 / 8.0;
    f[11 + AIMS] = field.military_foes(rules).count() as f32 / 8.0;
    f[12 + AIMS] = own_army.iter().map(|u| f32::from(u.hp)).sum::<f32>() / 800.0;
    f[13 + AIMS] = field.military_foes(rules).map(|u| f32::from(u.hp)).sum::<f32>() / 800.0;
    t.feat.push(f);
    t.selectable.push(false);
    t.unit.push(None);
    t.tile.push(orders.tile);
    let view = game.player_view(rules, orders.side);
    // Units: ours, then theirs.
    for (own, list) in [(true, &field.own), (false, &field.foes)] {
        for u in list.iter() {
            if t.len() >= T_MAX {
                break;
            }
            let Some(h) = auth.indexes().units_on(u.tile).iter().copied().find(|h| auth.units().get(*h).is_some_and(|x| x.core.id == u.id)) else { continue };
            let promotable = own && orders.army.contains(&u.id) && !view.promotions_available(h).is_empty();
            // A ranged unit set up (or with no moves left) may still shoot this turn.
            let may_shoot = act::range_of(rules, u.kind).is_some() && u.attacks_made == 0;
            let selectable = own && orders.army.contains(&u.id) && (u.moves_left > 0 || promotable || may_shoot);
            let mut f = [0f32; F_TOK];
            f[if own { TOK_OWN } else { TOK_FOE }] = 1.0;
            unit_features(game, rules, orders, &field, u, own, promotable, (ox, oy), &mut f);
            f[42] = if selectable { 1.0 } else { 0.0 };
            let (gained, moved) = since_last_turn(u);
            f[46] = gained;
            f[47] = moved;
            t.feat.push(f);
            t.selectable.push(selectable);
            t.unit.push(Some(h));
            t.tile.push(u.tile);
        }
    }
    // Cities.
    for (own, list) in [(true, &field.own_cities), (false, &field.foe_cities)] {
        for c in list.iter() {
            if t.len() >= T_MAX {
                break;
            }
            let Some(city) = auth.indexes().city_on_tile(*c).and_then(|h| auth.cities().get(h)) else { continue };
            let mut f = [0f32; F_TOK];
            f[TOK_CITY] = 1.0;
            let (x, y) = col_row(game, *c);
            f[4] = (x - ox) / 12.0;
            f[5] = (y - oy) / 12.0;
            f[6] = dist(game, *c, orders.tile) as f32 / 12.0;
            f[7] = f32::from(hit_points(auth, rules, city)) / f32::from(max_hit_points(auth, rules, city).max(1));
            f[11] = city_strength(auth, rules, city) as f32 / 100.0 / orders.mean_strength;
            f[12] = if city.strike_made { 0.0 } else { 1.0 };
            f[43] = if *c == orders.tile { 1.0 } else { 0.0 };
            f[44] = city.population as f32 / 10.0;
            f[45] = if own { 1.0 } else { 0.0 };
            t.feat.push(f);
            t.selectable.push(false);
            t.unit.push(None);
            t.tile.push(*c);
        }
    }
    for f in t.feat.iter_mut() {
        round_all(f);
    }
    t.pos = t.tile.iter().map(|x| pos_of(orders, *x)).collect();
    let n = t.len();
    t.dist = vec![0; n * n];
    for i in 0..n {
        for j in 0..n {
            t.dist[i * n + j] = dist(game, t.tile[i], t.tile[j]).clamp(0, i32::from(D_MAX)) as u8;
        }
    }
    t
}

#[allow(clippy::too_many_arguments)]
fn unit_features(game: &GameState, rules: &CompiledRuleset, orders: &Orders, field: &Field, u: &UnitCore, own: bool, promotable: bool, (ox, oy): (f32, f32), f: &mut [f32; F_TOK]) {
    let auth = game.view().auth;
    let d = &rules.units[u.kind.0 as usize];
    let (x, y) = col_row(game, u.tile);
    f[4] = (x - ox) / 12.0;
    f[5] = (y - oy) / 12.0;
    f[6] = dist(game, u.tile, orders.tile) as f32 / 12.0;
    f[7] = f32::from(u.hp) / 100.0;
    f[8] = if u.moves_max > 0 { f32::from(u.moves_left) / f32::from(u.moves_max) } else { 0.0 };
    f[9] = f32::from(u.moves_max) / f32::from(MOVE_DENOMINATOR) / 5.0;
    f[10] = if u.attacks_made == 0 { 1.0 } else { 0.0 };
    f[11] = f32::from(d.strength) / orders.mean_strength;
    f[12] = f32::from(d.ranged_strength) / orders.mean_strength;
    f[13] = act::range_of(rules, u.kind).unwrap_or(0) as f32 / 3.0;
    f[14 + arm_slot(rules, u.kind)] = 1.0;
    f[27] = if matches!(u.stance, Stance::Fortified { .. }) { 1.0 } else { 0.0 };
    f[28] = if matches!(u.stance, Stance::SetUp) { 1.0 } else { 0.0 };
    f[29] = if d.domain == classic_rules::rules::content::Domain::Land && auth.map().is_water(u.tile) { 1.0 } else { 0.0 };
    f[30] = f32::from(u.level) / 5.0;
    f[31] = u.experience as f32 / 60.0;
    f[32] = if promotable { 1.0 } else { 0.0 };
    let side = if own { orders.side } else { orders.foe };
    f[33] = act::defence(game, rules, u.tile, side) as f32 / 100.0;
    f[34] = if is_hills(auth.map(), u.tile) { 1.0 } else { 0.0 };
    f[35] = if is_rough(rules, auth.map(), u.tile) { 1.0 } else { 0.0 };
    if own {
        f[36] = if field.in_city_fire(game, u.tile) { 1.0 } else { 0.0 };
        let (foes, friends) = field.adjacent(game, rules, u.tile, Some(u.id));
        f[37] = foes as f32 / 3.0;
        f[38] = friends as f32 / 3.0;
        f[39] = field.threat(game, rules, u.tile) as f32 / 4.0;
        f[40] = field.nearest_foe(game, rules, u.tile) as f32 / 12.0;
        f[41] = field.refuge_distance(game, orders, u.tile) as f32 / 12.0;
    } else {
        // The foe's view of the same: our units next to it, our units that reach it.
        let near_own = field.own.iter().filter(|o| !act::is_civilian(rules, o.kind) && dist(game, o.tile, u.tile) == 1).count();
        f[37] = near_own as f32 / 3.0;
        let reach = field.own.iter().filter(|o| !act::is_civilian(rules, o.kind) && dist(game, o.tile, u.tile) <= act::moves_of(rules, o.kind) + act::range_of(rules, o.kind).unwrap_or(1)).count();
        f[39] = reach as f32 / 4.0;
        f[40] = field.own.iter().filter(|o| !act::is_civilian(rules, o.kind)).map(|o| dist(game, o.tile, u.tile)).min().unwrap_or(30) as f32 / 12.0;
    }
}

fn pos_of(orders: &Orders, t: TileId) -> i16 {
    orders.window.index(t).map_or(-1, |i| i as i16)
}

/// The legal actions of our unit `unit` now, with their features; "done" first.
pub fn candidates(game: &GameState, rules: &CompiledRuleset, orders: &Orders, unit: UnitHandle) -> Vec<Candidate> {
    let mut out = Vec::new();
    let Some(me) = act::core_of(game, unit) else { return out };
    let field = Field::gather(game, rules, orders);
    let here = Spot::of(game, rules, orders, &field, &me, me.tile);
    let mut done = Candidate { feat: [0.0; F_ACT], promo: 0, pos: pos_of(orders, me.tile), action: Action::Done };
    done.feat[Kind::Done as usize] = 1.0;
    here.write(&here, &mut done.feat);
    out.push(done);
    let Some((near, _)) = game.ai_local_observation(rules, orders.side, unit, RADIUS) else { return out };
    let reach = act::reachable(game, rules, orders.side, unit);
    for cmd in unit_actions(&near) {
        let Some(c) = candidate(game, rules, orders, &field, &me, &here, &reach, cmd) else { continue };
        out.push(c);
    }
    for c in out.iter_mut() {
        round_all(&mut c.feat);
    }
    out
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
    fn of(game: &GameState, rules: &CompiledRuleset, orders: &Orders, field: &Field, me: &UnitCore, t: TileId) -> Spot {
        let auth = game.view().auth;
        let (x, y) = col_row(game, t);
        let (ux, uy) = col_row(game, me.tile);
        let (adj_foes, adj_friends) = field.adjacent(game, rules, t, Some(me.id));
        Spot {
            tile: t,
            dq: x - ux,
            dr: y - uy,
            defence: act::defence(game, rules, t, orders.side) as f32,
            hills: is_hills(auth.map(), t),
            rough: is_rough(rules, auth.map(), t),
            city_fire: field.in_city_fire(game, t),
            adj_foes,
            adj_friends,
            threat: field.threat(game, rules, t),
            nearest_foe: field.nearest_foe(game, rules, t),
            objective: dist(game, t, orders.tile),
            refuge: field.refuge_distance(game, orders, t),
            on_objective: t == orders.tile,
            own_land: auth.map().owner(t).flatten() == Some(orders.side),
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

/// The tile an attack command aims at.
fn target_tile(game: &GameState, cmd: &Command) -> Option<TileId> {
    let auth = game.view().auth;
    match *cmd {
        Command::AttackUnit { defender, .. } => auth.units().get(defender).map(|u| u.core.tile),
        Command::AttackCity { city, .. } => auth.cities().get(city).map(|c| c.tile),
        Command::RangedAttack { target, .. } | Command::AirSweep { target, .. } => Some(target),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn candidate(game: &GameState, rules: &CompiledRuleset, orders: &Orders, field: &Field, me: &UnitCore, here: &Spot, reach: &[(TileId, u16)], cmd: Command) -> Option<Candidate> {
    let kind = kind_of(&cmd)?;
    let mut f = [0f32; F_ACT];
    f[kind as usize] = 1.0;
    let mut promo = 0u16;
    let mut subject = here.tile;
    match &cmd {
        Command::MoveUnit { destination, .. } => {
            let spot = Spot::of(game, rules, orders, field, me, *destination);
            spot.write(here, &mut f);
            subject = *destination;
            let left = reach.iter().find(|(t, _)| t == destination).map_or(0, |(_, l)| *l);
            f[27] = if me.moves_max > 0 { f32::from(left) / f32::from(me.moves_max) } else { 0.0 };
            let (blow, count) = blows_from(game, rules, field, me, *destination);
            f[28] = blow;
            f[29] = count as f32 / 3.0;
        }
        Command::Promote { promotion, .. } => {
            promo = promotion.0 + 1;
            here.write(here, &mut f);
        }
        Command::AttackUnit { .. } | Command::AttackCity { .. } | Command::RangedAttack { .. } | Command::AirSweep { .. } => {
            let t = target_tile(game, &cmd)?;
            subject = t;
            let spot = Spot::of(game, rules, orders, field, me, t);
            spot.write(here, &mut f);
            strike_features(game, rules, orders, me, t, &mut f);
        }
        _ => here.write(here, &mut f),
    }
    Some(Candidate { feat: f, promo, pos: pos_of(orders, subject), action: Action::Command(cmd) })
}

fn field_unit(game: &GameState, me: &UnitCore) -> Option<UnitHandle> {
    let auth = game.view().auth;
    auth.indexes().units_on(me.tile).iter().copied().find(|h| auth.units().get(*h).is_some_and(|u| u.core.id == me.id))
}

/// The best blow `me` could deal from `from` (dealt over the defender's hit points, 1 = a kill) and how
/// many foes it could strike there.
fn blows_from(game: &GameState, rules: &CompiledRuleset, field: &Field, me: &UnitCore, from: TileId) -> (f32, i32) {
    let Some(h) = field_unit(game, me) else { return (0.0, 0) };
    let reach = act::range_of(rules, me.kind).unwrap_or(1);
    let mut best = 0f32;
    let mut count = 0;
    for foe in field.foes.iter() {
        if dist(game, foe.tile, from) > reach {
            continue;
        }
        count += 1;
        if let Some((dealt, _, hp)) = act::preview(game, rules, h, Some(from), foe.tile) {
            best = best.max((dealt as f32 / (hp.max(1)) as f32).min(1.0));
        }
    }
    for c in field.foe_cities.iter() {
        if dist(game, *c, from) <= reach {
            count += 1;
        }
    }
    (best, count)
}

fn strike_features(game: &GameState, rules: &CompiledRuleset, orders: &Orders, me: &UnitCore, t: TileId, f: &mut [f32; F_ACT]) {
    let auth = game.view().auth;
    let Some(h) = field_unit(game, me) else { return };
    match act::preview(game, rules, h, None, t) {
        Some((dealt, taken, hp)) => {
            f[30] = dealt as f32 / 100.0;
            f[31] = taken as f32 / 100.0;
            f[32] = hp as f32 / 100.0;
            f[33] = if dealt >= hp { 1.0 } else { 0.0 };
            f[34] = if taken >= i64::from(me.hp) { 1.0 } else { 0.0 };
        }
        None => f[39] = 1.0,
    }
    if let Some(u) = auth.indexes().units_on(t).iter().filter_map(|x| auth.units().get(*x)).find(|u| u.core.owner != orders.side) {
        f[35] = if act::range_of(rules, u.core.kind).is_some() { 1.0 } else { 0.0 };
        f[36] = if act::is_civilian(rules, u.core.kind) { 1.0 } else { 0.0 };
        f[38] = base_strength(rules, u.core.kind) / orders.mean_strength;
    }
    if auth.indexes().city_on_tile(t).is_some() {
        f[37] = 1.0;
    }
}
