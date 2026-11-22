//! Part A: the inputs of `net2` (design §2.2, §2.4, §4.5) as functions of the seat's observation and
//! memory. Fog: everything is read from the `AIObservation`, the turn's brief and the seat's own memory
//! (`InputMemory`); nothing reads the world.
//!
//! `read` first writes the turn into the memory (`InputMemory::observe`: the cities lost and captured,
//! the foreign cities and their garrisons, the happiness, the sightings of the rivals' armies, the ties
//! to each rival), then reads the seat inputs G1–G10 (here), the per-rival inputs R1–R24
//! (`rival_inputs`) and the guard inputs of each own city.
//!
//! The memory is the inputs' own state, inside `NET2_SECTION` of the strategy memory (part B writes the
//! bytes; this file writes and reads them): the design's seen-city garrison flag and betrayal log are
//! here, not in the kernel's `SeenCity` (a platform type another repository owns).

use crate::ai::planner::TurnBrief;
use crate::ai::rival_model::Rivals;
use crate::ai::state::{AiState, UnitTask};
use crate::ai::strategy_net::layers::ramp;
use crate::ai::strategy_net::weights::fe;
use crate::ai::strategy_net::Milli;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;
use crate::world::player::PlayerKind;

use super::plan::{CityInputs, Inputs, SeatInputs, TargetInputs, MAX_TARGETS};

/// Standard rounds the rivals' sightings, the army estimate and the captures count (`pace_rounds`).
pub const SIGHT_ROUNDS: u32 = 10;
/// Standard rounds the war inputs (G3, R18) and a past betrayal's tie window run over.
pub const WAR_RAMP_ROUNDS: u32 = 30;
/// Standard rounds a lost city counts for G2 (`SeatInputs::cities_lost`).
pub const LOST_ROUNDS: u32 = 20;
/// Standard rounds a lost city stays remembered while its holder is unknown or it is not retaken.
pub const LOST_KEPT_ROUNDS: u32 = 100;
/// Standard rounds a forward city of a rival counts for `grab` (R21), and a city of ours is young (CityInputs::young).
pub const GRAB_ROUNDS: u32 = 40;
pub const YOUNG_ROUNDS: u32 = 20;
/// Standard rounds between two surveys of the routes (R2, R3).
pub const SURVEY_ROUNDS: u32 = 10;
/// A city of a rival this near one of ours (tiles) is a forward city when it came after ours (R21).
pub const GRAB_TILES: i32 = 4;
/// Rivals with a seen city this near an own city are its `rivals_near` (CityInputs).
pub const RIVALS_NEAR_TILES: i32 = 8;
/// Most lost and captured cities the memory keeps.
const LOG_KEPT: usize = 16;

/// A foreign city as the seat last saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeenCity {
    pub tile: TileId,
    pub owner: PlayerId,
    /// The round the memory first knew the city (0: it knew it when it started).
    pub first_seen: u32,
    /// The garrison flag (R6): at its last sight, no military unit of its owner on the city or next to it.
    /// None: never seen with the city and its neighbours in sight.
    pub open: Option<bool>,
}

/// A city of the seat's that another nation took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LostCity {
    pub tile: TileId,
    /// The round the seat found it gone.
    pub round: u32,
    /// The holder, once the memory has seen it.
    pub captor: Option<PlayerId>,
}

/// A city of another nation the seat took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capture {
    pub tile: TileId,
    pub round: u32,
    pub from: PlayerId,
}

/// What the seat sighted of one rival's army in a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sight {
    pub round: u32,
    /// Its non-civilian units in sight at once (what the demographics' soldiers row counts).
    pub units: u16,
    /// Their military power (`unit_power`).
    pub power: u32,
}

/// The sightings of one rival in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sights {
    pub player: PlayerId,
    pub samples: Vec<Sight>,
}

/// The ties between the seat and one rival, for the betrayal log (R13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ties {
    pub player: PlayerId,
    /// At war at the last turn.
    pub at_war: bool,
    /// The last round a tie held: friends, a pact, a research agreement or open borders either way.
    pub tie_round: Option<u32>,
    /// The seat had an operation, a raid or an objective against it at the last turn: the war is ours.
    pub planned: bool,
    /// Wars it declared on us within `WAR_RAMP_ROUNDS` of a tie (a deal, friendship), counted once each.
    pub betrayals: u8,
}

/// The routes to one rival's nearest seen city (R2, R3), taken every `SURVEY_ROUNDS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Survey {
    pub player: PlayerId,
    pub round: u32,
    pub our_cities: u16,
    /// The target the survey ran against: another nearest city asks for a new one.
    pub target: TileId,
    pub land: bool,
    pub sea: bool,
}

/// The estimate of one rival's soldiers (the fixed estimator, `rival_model::soldiers`) beside the
/// share R4 reads: the count and its doubt, for the war machine's `known_power` (design §2.4 step 4:
/// the estimate replaces `cities × 2.6` when the doubt is at most 300).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierRead {
    pub player: PlayerId,
    pub value: i64,
    pub lo: i64,
    pub hi: i64,
}

impl SoldierRead {
    /// The width of its range over its size, thousandths (0: exact).
    pub fn doubt(&self) -> i64 {
        crate::ai::rival_model::Guess { value: self.value, lo: self.lo, hi: self.hi }.doubt()
    }
}

/// What the inputs keep between turns (design §3.3 `NET2_SECTION`, the inputs' part). The core writes
/// the bytes inside its section by `encode` and reads them by `decode`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputMemory {
    /// The memory has seen a turn.
    pub started: bool,
    /// The round of the last turn written.
    pub round: u32,
    /// Own cities and the round the memory first knew each (0: at its start).
    pub own: Vec<(TileId, u32)>,
    pub foreign: Vec<SeenCity>,
    pub lost: Vec<LostCity>,
    pub captured: Vec<Capture>,
    /// The empire's excess happiness at past turns (G5).
    pub happy: Vec<(u32, i32)>,
    pub sights: Vec<Sights>,
    pub ties: Vec<Ties>,
    pub surveys: Vec<Survey>,
    /// Written by the turn's read: the soldiers estimate of each met rival, for the executors.
    pub soldiers: Vec<SoldierRead>,
    /// G11: our own estimate of the warmonger memory an average met major holds of us, points (the rules,
    /// `systems::diplomacy`: + `WARMONGER_ATTACKED_WEIGHT` × the era percent a declaration, + the conquest offset a
    /// city taken, `WARMONGER_DECAY` a round). The rules show a nation only its own memory of others.
    pub infamy: u32,
}

/// The version of the memory's bytes (its first byte); a body of another version does not read.
const MEMORY_VERSION: u8 = 1;

impl InputMemory {
    /// The estimate of `player`'s soldiers of the last read (None: not read, or no rival of that id).
    pub fn soldiers_of(&self, player: PlayerId) -> Option<&SoldierRead> {
        self.soldiers.iter().find(|s| s.player == player)
    }

    pub fn sights_of(&self, player: PlayerId) -> &[Sight] {
        self.sights.iter().find(|s| s.player == player).map_or(&[], |s| &s.samples)
    }

    pub fn ties_of(&self, player: PlayerId) -> Option<&Ties> {
        self.ties.iter().find(|t| t.player == player)
    }

    pub fn foreign_at(&self, tile: TileId) -> Option<&SeenCity> {
        self.foreign.iter().find(|c| c.tile == tile)
    }

    /// The most of `player`'s non-civilian units seen at once within the sight window: a lower bound of
    /// its soldiers (units that died since count only if they were seen together with the living).
    pub fn seen_units(&self, player: PlayerId, now: u32, window: u32) -> i64 {
        self.sights_of(player).iter().filter(|s| now.saturating_sub(s.round) <= window).map(|s| i64::from(s.units)).max().unwrap_or(0)
    }

    /// The military power of `player` sighted within the window, each sample decayed linearly to 0 at
    /// the window's end, the largest (a "decayed max", R5).
    pub fn seen_power(&self, player: PlayerId, now: u32, window: u32) -> i64 {
        let window = i64::from(window.max(1));
        self.sights_of(player)
            .iter()
            .filter_map(|s| {
                let age = i64::from(now.saturating_sub(s.round));
                (age <= window).then(|| i64::from(s.power) * (window - age) / window)
            })
            .max()
            .unwrap_or(0)
    }

    /// Our cities lost within `window` rounds of `now`.
    pub fn lost_within(&self, now: u32, window: u32) -> usize {
        self.lost.iter().filter(|l| now.saturating_sub(l.round) <= window).count()
    }

    /// The first round the memory knew our city on `tile` (None: not ours).
    pub fn own_since(&self, tile: TileId) -> Option<u32> {
        self.own.iter().find(|(t, _)| *t == tile).map(|(_, r)| *r)
    }

    /// Writes the turn into the memory: what changed in our cities and the foreign ones, the sample of
    /// the happiness and of the rivals' armies, the ties. Idempotent within a round.
    pub fn observe(&mut self, obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset) {
        let now = obs.round;
        if self.started && self.round == now {
            return;
        }
        let first = !self.started;
        let sight = obs.speed.pace_rounds(SIGHT_ROUNDS);
        if !first {
            let decay = crate::systems::diplomacy::WARMONGER_DECAY.unsigned_abs().saturating_mul(now.saturating_sub(self.round));
            self.infamy = self.infamy.saturating_sub(decay);
        }
        let known_before = self.foreign.clone();
        self.observe_cities(obs, state, first);
        self.observe_foreign(obs, state, rules, first);
        self.observe_lost();
        let percent = warmonger_percent(brief, rules);
        for capture in self.captured.iter().filter(|c| c.round == now && !first) {
            self.infamy = self.infamy.saturating_add(conquest_offset(&known_before, obs.own_cities.len(), capture.from) * percent / 100);
        }
        // The happiness samples: kept for a window and a half.
        if self.happy.last().is_none_or(|(r, _)| *r < now) {
            self.happy.push((now, brief.happiness));
        }
        self.happy.retain(|(r, _)| now.saturating_sub(*r) <= sight + sight / 2);
        self.observe_sights(obs, brief, rules, sight);
        let declared = self.observe_ties(obs, state);
        let weight = crate::systems::diplomacy::WARMONGER_ATTACKED_WEIGHT.unsigned_abs();
        self.infamy = self.infamy.saturating_add(declared * weight * percent / 100);
        let kept = obs.speed.pace_rounds(LOST_KEPT_ROUNDS);
        self.lost.retain(|l| now.saturating_sub(l.round) <= kept);
        self.captured.retain(|c| now.saturating_sub(c.round) <= kept);
        self.started = true;
        self.round = now;
    }

    fn observe_cities(&mut self, obs: &AIObservation<'_>, state: &AiState, first: bool) {
        let now = obs.round;
        let tiles: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
        // Gone since the last turn: lost.
        for (tile, _) in &self.own {
            if !tiles.contains(tile) && !self.lost.iter().any(|l| l.tile == *tile) {
                self.lost.push(LostCity { tile: *tile, round: now, captor: None });
            }
        }
        self.own.retain(|(t, _)| tiles.contains(t));
        for &tile in &tiles {
            if self.own.iter().any(|(t, _)| *t == tile) {
                continue;
            }
            // New: taken from a nation whose city the memory knew, else founded.
            match self.foreign.iter().find(|c| c.tile == tile) {
                Some(city) => self.captured.push(Capture { tile, round: now, from: city.owner }),
                None => {
                    if let Some(c) = state.seen_cities().iter().find(|c| c.tile == tile && c.owner != obs.player) {
                        self.captured.push(Capture { tile, round: now, from: c.owner });
                    }
                }
            }
            self.own.push((tile, if first { 0 } else { now }));
        }
        self.own.sort_by_key(|(t, _)| *t);
        if self.captured.len() > LOG_KEPT {
            self.captured.drain(..self.captured.len() - LOG_KEPT);
        }
        self.foreign.retain(|c| !tiles.contains(&c.tile));
    }

    fn observe_foreign(&mut self, obs: &AIObservation<'_>, state: &AiState, rules: &CompiledRuleset, first: bool) {
        let now = obs.round;
        let me = obs.player;
        // Every city of another nation the seat knows: in sight now or remembered by the state.
        let mut known: Vec<(TileId, PlayerId, bool)> = Vec::new();
        for c in &obs.visible_foreign_cities {
            if c.owner != me {
                known.push((c.tile, c.owner, true));
            }
        }
        for c in state.seen_cities() {
            if c.owner != me && !known.iter().any(|(t, _, _)| *t == c.tile) {
                known.push((c.tile, c.owner, false));
            }
        }
        let dims = obs.view.dimensions();
        let mut next: Vec<SeenCity> = Vec::with_capacity(known.len());
        for (tile, owner, visible) in known {
            let old = self.foreign.iter().find(|c| c.tile == tile);
            let mut open = old.and_then(|c| c.open);
            if visible {
                open = garrison_open(obs, rules, dims, tile, owner).or(open);
            }
            next.push(SeenCity { tile, owner, first_seen: old.map_or(if first { 0 } else { now }, |c| c.first_seen), open });
        }
        next.sort_by_key(|c| c.tile);
        self.foreign = next;
    }

    fn observe_lost(&mut self) {
        let own: Vec<TileId> = self.own.iter().map(|(t, _)| *t).collect();
        let foreign = &self.foreign;
        self.lost.retain_mut(|l| {
            if own.contains(&l.tile) {
                return false;
            }
            if let Some(c) = foreign.iter().find(|c| c.tile == l.tile) {
                l.captor = Some(c.owner);
            }
            true
        });
        if self.lost.len() > LOG_KEPT {
            self.lost.drain(..self.lost.len() - LOG_KEPT);
        }
    }

    fn observe_sights(&mut self, obs: &AIObservation<'_>, brief: &TurnBrief, rules: &CompiledRuleset, window: u32) {
        let now = obs.round;
        for relation in obs.relations.iter().filter(|r| r.kind == PlayerKind::Major) {
            let owner = relation.other;
            let units = obs.visible_foreign_units.iter().filter(|u| u.owner == owner && !rules.is_civilian(u.kind)).count();
            let power: u32 = brief.sightings.iter().filter(|s| s.owner == owner && s.military).map(|s| s.power).sum();
            let at = match self.sights.iter().position(|s| s.player == owner) {
                Some(at) => at,
                None => {
                    self.sights.push(Sights { player: owner, samples: Vec::new() });
                    self.sights.len() - 1
                }
            };
            let samples = &mut self.sights[at].samples;
            if units > 0 {
                samples.push(Sight { round: now, units: units.min(usize::from(u16::MAX)) as u16, power });
            }
            samples.retain(|s| now.saturating_sub(s.round) <= window);
        }
        self.sights.sort_by_key(|s| s.player);
    }

    /// The ties of every met major, and the wars we declared since the last turn (a war that began while an
    /// operation, a raid or an objective of ours stood against it).
    fn observe_ties(&mut self, obs: &AIObservation<'_>, state: &AiState) -> u32 {
        let now = obs.round;
        let mut declared = 0;
        let tie_window = obs.speed.pace_rounds(WAR_RAMP_ROUNDS);
        let raided = crate::ai::raid::enemies(state);
        for relation in obs.relations.iter().filter(|r| r.kind == PlayerKind::Major) {
            let owner = relation.other;
            let at = match self.ties.iter().position(|t| t.player == owner) {
                Some(at) => at,
                None => {
                    self.ties.push(Ties { player: owner, at_war: relation.at_war, tie_round: None, planned: false, betrayals: 0 });
                    self.ties.len() - 1
                }
            };
            let ties = &mut self.ties[at];
            // A war that began since the last turn, with no operation of ours against it, after a tie
            // within the window: it declared on us (the rules tell only the victim of a declaration).
            if relation.at_war && !ties.at_war && !ties.planned && ties.tie_round.is_some_and(|t| now.saturating_sub(t) <= tie_window) {
                ties.betrayals = ties.betrayals.saturating_add(1).min(8);
            }
            if relation.at_war && !ties.at_war && ties.planned {
                declared += 1;
            }
            ties.at_war = relation.at_war;
            if relation.friends || relation.defensive_pact || relation.research_agreement || relation.we_grant_entry || relation.they_grant_entry {
                ties.tie_round = Some(now);
            }
            ties.planned = state.operations().iter().any(|o| o.enemy == owner)
                || raided.contains(&owner)
                || state.war_plan.is_some_and(|w| w.enemy == owner)
                || state.relation(owner).and_then(|m| m.objective).is_some();
        }
        self.ties.sort_by_key(|t| t.player);
        declared
    }

    /// Appends the bytes of the memory to `out`: a version, then the body prefixed by its length, so
    /// that a reader of a later version skips what it does not know.
    pub fn encode(&self, out: &mut Vec<u8>) {
        let mut body = Vec::new();
        let w = &mut body;
        put8(w, u8::from(self.started));
        put32(w, self.round);
        put16(w, self.own.len());
        for (t, r) in &self.own {
            put32(w, t.0);
            put32(w, *r);
        }
        put16(w, self.foreign.len());
        for c in &self.foreign {
            put32(w, c.tile.0);
            put8(w, c.owner.0);
            put32(w, c.first_seen);
            put8(w, match c.open {
                None => 0,
                Some(false) => 1,
                Some(true) => 2,
            });
        }
        put16(w, self.lost.len());
        for l in &self.lost {
            put32(w, l.tile.0);
            put32(w, l.round);
            put8(w, l.captor.map_or(u8::MAX, |p| p.0));
        }
        put16(w, self.captured.len());
        for c in &self.captured {
            put32(w, c.tile.0);
            put32(w, c.round);
            put8(w, c.from.0);
        }
        put16(w, self.happy.len());
        for (r, h) in &self.happy {
            put32(w, *r);
            put32(w, *h as u32);
        }
        put16(w, self.sights.len());
        for s in &self.sights {
            put8(w, s.player.0);
            put16(w, s.samples.len());
            for x in &s.samples {
                put32(w, x.round);
                put16(w, usize::from(x.units));
                put32(w, x.power);
            }
        }
        put16(w, self.ties.len());
        for t in &self.ties {
            put8(w, t.player.0);
            put8(w, u8::from(t.at_war) | u8::from(t.planned) << 1 | u8::from(t.tie_round.is_some()) << 2);
            put32(w, t.tie_round.unwrap_or(0));
            put8(w, t.betrayals);
        }
        put16(w, self.surveys.len());
        for s in &self.surveys {
            put8(w, s.player.0);
            put32(w, s.round);
            put16(w, usize::from(s.our_cities));
            put32(w, s.target.0);
            put8(w, u8::from(s.land) | u8::from(s.sea) << 1);
        }
        // Fields added after version 1 go last: a body without them reads them as 0.
        put32(w, self.infamy);
        // The soldiers are an output of the read, not memory; a restart reads them afresh.
        out.push(MEMORY_VERSION);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
    }

    /// The memory at the start of `bytes` and the bytes it took; None for bytes that do not read (the
    /// seat then starts a memory afresh).
    pub fn decode(bytes: &[u8]) -> Option<(InputMemory, usize)> {
        if bytes.is_empty() {
            return Some((InputMemory::default(), 0));
        }
        let (&version, rest) = bytes.split_first()?;
        if version != MEMORY_VERSION {
            return None;
        }
        let len = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
        let body = rest.get(4..4 + len)?;
        let mut r = Reader { bytes: body, at: 0 };
        let mut m = InputMemory { started: r.u8()? != 0, round: r.u32()?, ..InputMemory::default() };
        for _ in 0..r.u16()? {
            m.own.push((TileId(r.u32()?), r.u32()?));
        }
        for _ in 0..r.u16()? {
            let (tile, owner, first_seen) = (TileId(r.u32()?), PlayerId(r.u8()?), r.u32()?);
            let open = match r.u8()? {
                0 => None,
                1 => Some(false),
                _ => Some(true),
            };
            m.foreign.push(SeenCity { tile, owner, first_seen, open });
        }
        for _ in 0..r.u16()? {
            let (tile, round, captor) = (TileId(r.u32()?), r.u32()?, r.u8()?);
            m.lost.push(LostCity { tile, round, captor: (captor != u8::MAX).then_some(PlayerId(captor)) });
        }
        for _ in 0..r.u16()? {
            m.captured.push(Capture { tile: TileId(r.u32()?), round: r.u32()?, from: PlayerId(r.u8()?) });
        }
        for _ in 0..r.u16()? {
            m.happy.push((r.u32()?, r.u32()? as i32));
        }
        for _ in 0..r.u16()? {
            let player = PlayerId(r.u8()?);
            let mut samples = Vec::new();
            for _ in 0..r.u16()? {
                samples.push(Sight { round: r.u32()?, units: r.u16()?, power: r.u32()? });
            }
            m.sights.push(Sights { player, samples });
        }
        for _ in 0..r.u16()? {
            let (player, flags, tie, betrayals) = (PlayerId(r.u8()?), r.u8()?, r.u32()?, r.u8()?);
            m.ties.push(Ties { player, at_war: flags & 1 != 0, planned: flags & 2 != 0, tie_round: (flags & 4 != 0).then_some(tie), betrayals });
        }
        for _ in 0..r.u16()? {
            let (player, round, our_cities, target, flags) = (PlayerId(r.u8()?), r.u32()?, r.u16()?, TileId(r.u32()?), r.u8()?);
            m.surveys.push(Survey { player, round, our_cities, target, land: flags & 1 != 0, sea: flags & 2 != 0 });
        }
        if r.at < body.len() {
            m.infamy = r.u32()?;
        }
        Some((m, 1 + 4 + len))
    }
}

fn put8(out: &mut Vec<u8>, v: u8) {
    out.push(v);
}

fn put16(out: &mut Vec<u8>, v: usize) {
    out.extend_from_slice(&(v.min(usize::from(u16::MAX)) as u16).to_le_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

struct Reader<'b> {
    bytes: &'b [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.bytes.get(self.at..self.at + n)?;
        self.at += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
}

/// The city on `tile` stands open (R6): in sight with its six neighbours, and none of the units in
/// sight there is a military unit of its owner. None when a neighbour is out of sight and no unit shows:
/// the garrison is unknown, not absent.
fn garrison_open(obs: &AIObservation<'_>, rules: &CompiledRuleset, dims: crate::math::MapDimensions, tile: TileId, owner: PlayerId) -> Option<bool> {
    let ring: Vec<TileId> = std::iter::once(tile).chain(dims.neighbors(tile)).collect();
    if obs.visible_foreign_units.iter().any(|u| u.owner == owner && !rules.is_civilian(u.kind) && ring.contains(&u.tile)) {
        return Some(false);
    }
    ring.iter().all(|t| obs.view.is_visible(*t)).then_some(true)
}

/// The turn's inputs: `situation` the carried features (`strategy_net::situation`), `rivals` the model
/// of the rivals' screens; `memory` is updated (samples, surveys) as the inputs need.
pub fn read(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, situation: &[Milli; fe::N], rivals: &Rivals, memory: &mut InputMemory) -> Inputs {
    memory.observe(obs, brief, state, rules);
    let seat = seat(obs, brief, rules, memory);
    let rival_rows = super::rival_inputs::read(obs, brief, state, rules, rivals, memory);
    let cities = cities(obs, brief, state, memory);
    let targets = targets(obs, brief, state, rules, memory);
    Inputs { situation: *situation, seat, rivals: rival_rows, cities, pairs: Vec::new(), aims: Vec::new(), targets }
}

/// The rules' era percent of the warmonger weights (`systems::diplomacy::WARMONGER_PERCENT`) at the average era of
/// the nations we know: ours and the met ones' (the rules average every living nation's).
fn warmonger_percent(brief: &TurnBrief, rules: &CompiledRuleset) -> u32 {
    let own = crate::ai::strategy_net::features::era_of(rules, &brief.techs);
    let eras: Vec<usize> = std::iter::once(own).chain(brief.met_eras.iter().map(|(_, e)| usize::from(*e))).collect();
    let era = eras.iter().sum::<usize>() / eras.len().max(1);
    let table = crate::systems::diplomacy::WARMONGER_PERCENT;
    table[era.min(table.len() - 1)].unsigned_abs()
}

/// The rules' warmonger offset of one city taken from `victim` (`warmonger_for_conquest`: the world's expected
/// cities over the world's cities × the victim's), with the cities we knew before the capture.
fn conquest_offset(known: &[SeenCity], own_cities: usize, victim: PlayerId) -> u32 {
    let expected = crate::systems::diplomacy::EXPECTED_CITY_COUNT.unsigned_abs();
    let world = (known.len() + own_cities).max(1) as u32;
    let victims = known.iter().filter(|c| c.owner == victim).count().max(1) as u32;
    1000 * expected / (world * victims)
}

/// The garrison of a target out of sight: unknown, as likely there as not.
const GARRISON_UNKNOWN: Milli = 500;

/// T1–T4 (`TargetInputs`): every city of a met major the state remembers within `DECISIVE_RANGE` of our nearest
/// city, the nearest first (the tile on a tie), at most `MAX_TARGETS`.
fn targets(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, memory: &InputMemory) -> Vec<TargetInputs> {
    let ours = i64::from(brief.military_power);
    let majors: Vec<PlayerId> = obs.relations.iter().filter(|r| r.kind == PlayerKind::Major).map(|r| r.other).collect();
    let mut found: Vec<(i32, TargetInputs)> = Vec::new();
    for city in state.seen_cities().iter().filter(|c| majors.contains(&c.owner)) {
        let Some(from) = brief.nearest_city(brief.dims, city.tile) else { continue };
        let Some(distance) = brief.dims.distance(from.tile, city.tile) else { continue };
        if distance > crate::ai::operations::DECISIVE_RANGE {
            continue;
        }
        let in_sight = obs.visible_foreign_cities.iter().any(|v| v.tile == city.tile);
        let garrison = if in_sight {
            let near = |tile: TileId| brief.dims.distance(tile, city.tile).is_some_and(|d| d <= crate::ai::operations::DEFENCE_RADIUS);
            let guard: i64 = brief.sightings.iter().filter(|s| s.owner == city.owner && s.military && near(s.tile)).map(|s| i64::from(s.power)).sum();
            share_of(guard, ours, 0)
        } else {
            match memory.foreign_at(city.tile).and_then(|c| c.open) {
                Some(true) => 0,
                _ => GARRISON_UNKNOWN,
            }
        };
        let estimate = crate::ai::operations::estimate_target(brief, rules, city);
        let strength = (estimate.strength * i64::from(estimate.hp) / 10_000).max(0);
        let target = TargetInputs {
            rival: city.owner,
            city: city.tile,
            size: ramp(i64::from(city.population) - 2, 0, 12),
            garrison,
            near: ramp(15 - i64::from(distance), 0, 10),
            walls: share_of(strength, ours, 1000),
        };
        found.push((distance, target));
    }
    found.sort_by_key(|(d, t)| (*d, t.city));
    found.into_iter().take(MAX_TARGETS).map(|(_, t)| t).collect()
}

/// `part` over `part + rest`, thousandths; `none` when both are 0.
fn share_of(part: i64, rest: i64, none: Milli) -> Milli {
    let (part, rest) = (part.max(0), rest.max(0));
    if part + rest == 0 {
        none
    } else {
        part * 1000 / (part + rest)
    }
}

/// G1–G10 (§2.2).
fn seat(obs: &AIObservation<'_>, brief: &TurnBrief, rules: &CompiledRuleset, memory: &InputMemory) -> SeatInputs {
    let me = obs.player;
    let now = obs.round;
    let war_ramp = obs.speed.pace_rounds(WAR_RAMP_ROUNDS);
    // G1: the original capital is held by another nation.
    let capital_lost = if obs.public.capitals.iter().any(|c| c.founder == me && c.holder != me) { 1000 } else { 0 };
    // G2: the cities lost within the window, 0…3.
    let cities_lost = ramp(memory.lost_within(now, obs.speed.pace_rounds(LOST_ROUNDS)) as i64, 0, 3);
    // G3: the longest current war with a nation.
    let wars: Vec<(PlayerId, u32)> = obs.relations.iter().filter(|r| r.kind == PlayerKind::Major && r.at_war).map(|r| (r.other, r.war_since)).collect();
    let longest = wars.iter().map(|(_, since)| now.saturating_sub(*since)).max().unwrap_or(0);
    let war_rounds = ramp(i64::from(longest), 0, i64::from(war_ramp));
    // G4: cities captured minus cities lost in the current wars, −2…+2 around 500.
    let war_gain = war_gain(memory, &wars);
    // G5: the fall of the happiness over the window.
    let unhappy_trend = unhappy_trend(memory, now, brief.happiness, obs.speed.pace_rounds(SIGHT_ROUNDS));
    // G6: a technology we know lets units embark.
    let embark = if obs.economy.techs.iter().any(|t| rules.technologies.get(t.0 as usize).is_some_and(|d| d.allows_embarking)) { 1000 } else { 0 };
    // G7: enemy aircraft in sight now (`air::enemy_air_seen`, which is private there).
    let air = obs
        .visible_foreign_units
        .iter()
        .filter(|u| obs.relation(u.owner).is_some_and(|r| r.at_war))
        .filter(|u| rules.units.get(u.kind.0 as usize).is_some_and(|d| d.domain == Domain::Air))
        .count();
    let enemy_air = ramp(air as i64, 0, 4);
    // G8: the treasury over a medium gift.
    let gold_stock = ramp(i64::from(obs.economy.treasury), 0, 3 * i64::from(crate::ai::city_states::MEDIUM_GIFT));
    let cs_allies = ramp(obs.city_states.iter().filter(|c| c.ally == Some(me)).count() as i64, 0, 4);
    let cs_contested = ramp(contested_city_states(obs), 0, 4);
    // G11: our infamy over the scripted AIs' denunciation line (`ai::diplomacy::DENOUNCE_WARMONGER`).
    let own_infamy = ramp(i64::from(memory.infamy), 0, i64::from(crate::ai::diplomacy::DENOUNCE_WARMONGER));
    // G12: the ties a declaration puts at risk: friendships, research agreements and pacts with majors.
    let majors = obs.relations.iter().filter(|r| r.kind == PlayerKind::Major);
    let ties: i64 = majors.map(|r| i64::from(r.friends) + i64::from(r.research_agreement) + i64::from(r.defensive_pact)).sum();
    let ties_at_risk = ramp(ties, 0, 4);
    SeatInputs { capital_lost, cities_lost, war_rounds, war_gain, unhappy_trend, embark, enemy_air, gold_stock, cs_allies, cs_contested, own_infamy, ties_at_risk, ..SeatInputs::default() }
}

/// G4 (§2.2): in each current war, the cities captured from the enemy since it began minus the cities
/// it took from us (its own, or taken by an enemy not yet seen holding them while a war runs); 0 at peace.
fn war_gain(memory: &InputMemory, wars: &[(PlayerId, u32)]) -> Milli {
    if wars.is_empty() {
        return 500;
    }
    let first_war = wars.iter().map(|(_, since)| *since).min().unwrap_or(0);
    let mut gain = 0i64;
    for (enemy, since) in wars {
        gain += memory.captured.iter().filter(|c| c.from == *enemy && c.round >= *since).count() as i64;
        gain -= memory.lost.iter().filter(|l| l.captor == Some(*enemy) && l.round >= *since).count() as i64;
    }
    // A loss whose captor is not known yet is charged to the war: it has run since `first_war`.
    gain -= memory.lost.iter().filter(|l| l.captor.is_none() && l.round >= first_war).count() as i64;
    500 + (gain.clamp(-2, 2) * 250)
}

/// G5 (§2.2): the happiness of the oldest sample at least half a window old, minus now's, 0…10 points.
fn unhappy_trend(memory: &InputMemory, now: u32, happiness: i32, window: u32) -> Milli {
    let base = memory.happy.iter().find(|(r, _)| now.saturating_sub(*r) >= window / 2).filter(|(r, _)| *r < now);
    match base {
        Some((_, then)) => ramp(i64::from(*then) - i64::from(happiness), 0, 10),
        None => 0,
    }
}

/// G10 (§2.2): the city-states where our influence trails the leader's by under 30 points. The leader's
/// influence is not on the screen: with another nation as the ally it is at least the alliance's
/// threshold, and the mean of our own alliances when we hold some (as `city_states::guard_targets`
/// reads it). A city-state at war with us is not counted.
fn contested_city_states(obs: &AIObservation<'_>) -> i64 {
    let me = obs.player;
    let held: Vec<i32> = obs.city_states.iter().filter(|c| c.ally == Some(me)).map(|c| c.influence).collect();
    obs.city_states
        .iter()
        .filter(|c| !c.at_war && c.ally.is_some_and(|a| a != me))
        .filter(|c| {
            let threshold = obs.view.city_state_terms(c.player).map_or(crate::world::city_states::ALLIES_THRESHOLD, |t| t.allies_at);
            let leader = if held.is_empty() { threshold } else { (held.iter().map(|v| i64::from(*v)).sum::<i64>() / held.len() as i64) as i32 }.max(threshold);
            leader - c.influence < crate::world::city_states::FRIENDS_THRESHOLD
        })
        .count() as i64
}

/// The guard's inputs per own city (§4.5).
fn cities(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, memory: &InputMemory) -> Vec<CityInputs> {
    let now = obs.round;
    let young_window = obs.speed.pace_rounds(YOUNG_ROUNDS);
    let radius = crate::ai::planner_strategy::THREAT_RADIUS;
    let known = crate::ai::net2::rival_inputs::known_cities(obs, state, memory);
    obs.own_cities
        .iter()
        .map(|c| {
            let near = |tile: TileId, r: i32| brief.dims.distance(tile, c.tile).is_some_and(|d| d <= r);
            // The owner of the strongest foreign stack near the city: units in the field, at war or not
            // (a garrison stays in its own city), the way the defence reads hostility.
            let mut by_owner: Vec<(PlayerId, u32)> = Vec::new();
            for s in brief.sightings.iter().filter(|s| s.military && near(s.tile, radius)) {
                if known.iter().any(|(t, o, _)| *t == s.tile && *o == s.owner) {
                    continue;
                }
                if !obs.relation(s.owner).is_some_and(|r| r.kind == PlayerKind::Major) {
                    continue;
                }
                match by_owner.iter_mut().find(|(o, _)| *o == s.owner) {
                    Some((_, p)) => *p += s.power,
                    None => by_owner.push((s.owner, s.power)),
                }
            }
            let stack_owner = by_owner.into_iter().filter(|(_, p)| *p > 0).max_by_key(|(o, p)| (*p, std::cmp::Reverse(*o))).map(|(o, _)| o);
            let danger = if brief.defence.as_ref().is_some_and(|w| w.danger(c.tile).is_some()) { 1000 } else { 0 };
            let mut rivals_near: Vec<PlayerId> = known.iter().filter(|(t, _, _)| near(*t, RIVALS_NEAR_TILES)).map(|(_, o, _)| *o).filter(|o| obs.relation(*o).is_some_and(|r| r.kind == PlayerKind::Major)).collect();
            rivals_near.sort();
            rivals_near.dedup();
            let young = if memory.own_since(c.tile).is_some_and(|first| now.saturating_sub(first) < young_window && first > 0) { 1000 } else { 0 };
            CityInputs { city: c.id, stack_owner, danger, rivals_near, young }
        })
        .collect()
}

/// The seat's units that are free for a war (`UnitTask`: not a garrison, an escort, a scout, a settler, a worker).
pub(crate) fn is_free(state: &AiState, unit: crate::ids::EntityId) -> bool {
    !matches!(
        state.task_of(unit).map(|t| t.task),
        Some(UnitTask::Defend { .. } | UnitTask::Escort { .. } | UnitTask::Scout { .. } | UnitTask::Settle { .. } | UnitTask::Improve { .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn city(tile: u32, owner: u8) -> SeenCity {
        SeenCity { tile: TileId(tile), owner: PlayerId(owner), first_seen: 0, open: None }
    }

    #[test]
    fn a_city_taken_costs_the_rules_conquest_offset() {
        // 9 known foreign cities, 3 of the victim's, 1 of ours: 1000 × 52 / (10 × 3) = 1733 points before the era.
        let mut known: Vec<SeenCity> = (0..6).map(|i| city(i, 2)).collect();
        known.extend((6..9).map(|i| city(i, 1)));
        assert_eq!(conquest_offset(&known, 1, PlayerId(1)), 1733);
        // An unknown victim counts as one city: the offset is not divided by zero.
        assert_eq!(conquest_offset(&known, 1, PlayerId(4)), 5200);
    }
}
