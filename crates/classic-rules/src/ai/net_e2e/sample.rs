//! Sampling play for self-play (stage E2, classic/docs/ai-neural-architecture.md,
//! «The E2 recipe»; the files: classic/docs/ai-net-data.md, «E2: the net's own
//! games»).
//!
//! With a [`Sampling`] the net player ([`super::mind::NetPlayer::sampling`])
//! draws each choice from the softmax of the accepted candidates' logits at a
//! temperature instead of taking the largest, and keeps a [`Trace`]: per
//! player-turn its value, per decision what policy-gradient training needs
//! (the candidates' count, which the rules accepted, the choice and its
//! log-probability under the sampling distribution).
//!
//! - The logits are the model's Q8 integers (`value × 256`,
//!   classic/docs/ai-net-model.md): the probability of an accepted candidate
//!   is `exp(s / (256 T)) / Σ exp(s_j / (256 T))` over the accepted ones, so
//!   `T = 1` is the trained softmax. A refused candidate (a march the rules
//!   refuse now, a policy not adoptable) is outside the distribution.
//! - `T = 0` takes the largest accepted logit, ties to the lowest index: the
//!   choice of the plain net player, so the game is the argmax game.
//! - The draw is a function of (sampling seed, player, round, decision
//!   ordinal in the player-turn) alone ([`draw`]): the same seeds play the
//!   same game, and the game replays exactly from its command log whatever
//!   computed it.
//! - A decision's position is the number of commands the game applied before
//!   it was made ([`Trace::stamp`], set by whoever steps the game): the
//!   exporter replays the log to that point and rebuilds the candidates there.
//! - A net that defers (format 9) says so in the trace ([`Trace::defer`]):
//!   a unit's decision then ends with the defer candidate while the unit has
//!   a deferral left in its turn, which the exporter counts from the trace's
//!   own choices (a chosen `tag::DEFER` is a deferral the driver granted).

use crate::ai::net_e2e::encode::Candidate;
use crate::ids::PlayerId;

/// How the net samples: the temperature on the Q8 logits (0: argmax) and the
/// seed of the draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sampling {
    pub temperature: f64,
    pub seed: u64,
}

/// The actor of a decision, as the exporter finds it again in the replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Actor {
    /// Research, policy: the player.
    #[default]
    Player,
    /// A unit by its handle.
    Unit { index: u32, generation: u32 },
    /// A city's production by its handle; `cleared`: the scripted mind
    /// cleared the queue first (the candidates see nothing queued).
    City { index: u32, generation: u32, cleared: bool },
}

/// Not stamped yet ([`Trace::stamp`]).
pub const UNSTAMPED: u32 = u32::MAX;

/// One decision of the net.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionRecord {
    /// `encode::head`.
    pub head: u8,
    pub actor: Actor,
    /// Commands the game applied before the decision.
    pub position: u32,
    /// Candidates offered, and those the rules accepted (the softmax's).
    pub candidates: u16,
    pub accepted: u16,
    /// The chosen index (None: no candidate, or none accepted).
    pub chosen: Option<u16>,
    /// The chosen candidate's logit (Q8).
    pub score: i32,
    /// The chosen candidate's log-probability under the sampling
    /// distribution (natural log; 0 at `T = 0`).
    pub logp: f32,
    /// The chosen candidate's tag, target tile and parameter (`meta` 0, 1, 3;
    /// `-1` without a choice).
    pub chosen_meta: [i32; 3],
    /// [`rows_hash`] of the candidates.
    pub rows_hash: u32,
    /// The indices of the candidates the rules refused, ascending.
    pub refused: Vec<u16>,
}

/// One player-turn of the net with its decisions in the order they were made
/// (the decision's ordinal is its index here).
#[derive(Debug, Clone, PartialEq)]
pub struct TurnRecord {
    pub player: u8,
    pub round: u32,
    /// Commands the game applied before the turn began.
    pub position: u32,
    /// The value head's output at the turn's start (Q8, the expected final
    /// points share × 256); None for a scorer without one.
    pub value: Option<i32>,
    pub decisions: Vec<DecisionRecord>,
}

/// What a sampling net played.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    pub sampling: Sampling,
    /// The net offered the defer candidate (format 9; version 2 of the
    /// bytes, false in version 1).
    pub defer: bool,
    pub turns: Vec<TurnRecord>,
}

/// The magic and version of an encoded trace (`.e2.bin`): 2 adds the
/// defer flag; version 1 reads as before (no defer).
const MAGIC: &[u8; 4] = b"AUE2";
const VERSION: u32 = 2;

impl Trace {
    pub fn new(sampling: Sampling) -> Self {
        Self { sampling, defer: false, turns: Vec::new() }
    }

    /// The same trace of a net that offers the defer candidate (or not).
    pub fn with_defer(self, defer: bool) -> Self {
        Self { defer, ..self }
    }

    /// Whether the unit decision `d` of turn `t` was offered the defer
    /// candidate: the net defers and the same unit's earlier decisions of
    /// the turn chose it fewer than `deferrals` times (the driver's
    /// `DEFERS_PER_UNIT`).
    pub fn defer_offered(&self, t: usize, d: usize, deferrals: u32) -> bool {
        let turn = &self.turns[t];
        let record = &turn.decisions[d];
        if !self.defer || !matches!(record.actor, Actor::Unit { .. }) {
            return false;
        }
        let taken = turn.decisions[..d].iter().filter(|e| e.actor == record.actor && e.chosen.is_some() && e.chosen_meta[0] == crate::ai::net_e2e::encode::tag::DEFER).count();
        (taken as u32) < deferrals
    }

    /// A player-turn begins.
    pub fn begin_turn(&mut self, player: PlayerId, round: u32, value: Option<i32>) {
        self.turns.push(TurnRecord { player: player.0, round, position: UNSTAMPED, value, decisions: Vec::new() });
    }

    /// The choice among `candidates` of the open turn (`scores` their
    /// logits; `accepts`: the rules accept it now), recorded. None: no
    /// candidate, or none accepted.
    pub fn choose(&mut self, head: i32, actor: Actor, scores: &[i32], candidates: &[Candidate], accepts: impl Fn(&Candidate) -> bool) -> Option<usize> {
        let Sampling { temperature, seed } = self.sampling;
        let turn = self.turns.last_mut().expect("a decision inside a player-turn");
        let ordinal = turn.decisions.len() as u32;
        let ok: Vec<bool> = candidates.iter().map(accepts).collect();
        let refused: Vec<u16> = ok.iter().enumerate().filter(|(_, a)| !**a).map(|(i, _)| i as u16).collect();
        let picked = pick(&scores[..candidates.len().min(scores.len())], &ok, temperature, draw(seed, PlayerId(turn.player), turn.round, ordinal));
        let chosen_meta = picked.map_or([-1; 3], |(at, _)| [candidates[at].meta[0], candidates[at].meta[1], candidates[at].meta[3]]);
        turn.decisions.push(DecisionRecord {
            head: head as u8,
            actor,
            position: UNSTAMPED,
            candidates: candidates.len() as u16,
            accepted: (candidates.len() - refused.len()) as u16,
            chosen: picked.map(|(at, _)| at as u16),
            score: picked.map_or(0, |(at, _)| scores[at]),
            logp: picked.map_or(0.0, |(_, logp)| logp as f32),
            chosen_meta,
            rows_hash: rows_hash(candidates),
            refused,
        });
        picked.map(|(at, _)| at)
    }

    /// Every record made since the last stamp gets `position`.
    pub fn stamp(&mut self, position: u32) {
        for turn in self.turns.iter_mut().rev() {
            for d in turn.decisions.iter_mut().rev() {
                if d.position != UNSTAMPED {
                    return;
                }
                d.position = position;
            }
            if turn.position != UNSTAMPED {
                return;
            }
            turn.position = position;
        }
    }

    /// Decisions recorded.
    pub fn decisions(&self) -> usize {
        self.turns.iter().map(|t| t.decisions.len()).sum()
    }

    /// The trace's bytes (`.e2.bin`, classic/docs/ai-net-data.md), all
    /// little-endian: `AUE2`, u32 version 2, f64 temperature, u64 seed, u8
    /// defer (version 2 only), u32 turn count; per turn u8 player, u32 round, u32 position, u8 has value,
    /// i32 value, u32 decision count; per decision u8 head, u8 actor kind (0
    /// player, 1 unit, 2 city, 3 city after a clear), u32 handle index, u32
    /// handle generation, u32 position, u16 candidates, u16 accepted, u16
    /// chosen (`0xFFFF` none), i32 logit, f32 log-probability, i32 tag, i32
    /// target tile, i32 parameter, u32 rows hash, u16 refused count, u16 ×
    /// refused indices.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + self.turns.len() * 18 + self.decisions() * 48);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&self.sampling.temperature.to_le_bytes());
        out.extend_from_slice(&self.sampling.seed.to_le_bytes());
        out.push(u8::from(self.defer));
        out.extend_from_slice(&(self.turns.len() as u32).to_le_bytes());
        for t in &self.turns {
            out.push(t.player);
            out.extend_from_slice(&t.round.to_le_bytes());
            out.extend_from_slice(&t.position.to_le_bytes());
            out.push(u8::from(t.value.is_some()));
            out.extend_from_slice(&t.value.unwrap_or(0).to_le_bytes());
            out.extend_from_slice(&(t.decisions.len() as u32).to_le_bytes());
            for d in &t.decisions {
                let (kind, index, generation) = match d.actor {
                    Actor::Player => (0u8, 0, 0),
                    Actor::Unit { index, generation } => (1, index, generation),
                    Actor::City { index, generation, cleared } => (2 + u8::from(cleared), index, generation),
                };
                out.push(d.head);
                out.push(kind);
                out.extend_from_slice(&index.to_le_bytes());
                out.extend_from_slice(&generation.to_le_bytes());
                out.extend_from_slice(&d.position.to_le_bytes());
                out.extend_from_slice(&d.candidates.to_le_bytes());
                out.extend_from_slice(&d.accepted.to_le_bytes());
                out.extend_from_slice(&d.chosen.unwrap_or(u16::MAX).to_le_bytes());
                out.extend_from_slice(&d.score.to_le_bytes());
                out.extend_from_slice(&d.logp.to_le_bytes());
                for m in d.chosen_meta {
                    out.extend_from_slice(&m.to_le_bytes());
                }
                out.extend_from_slice(&d.rows_hash.to_le_bytes());
                out.extend_from_slice(&(d.refused.len() as u16).to_le_bytes());
                for r in &d.refused {
                    out.extend_from_slice(&r.to_le_bytes());
                }
            }
        }
        out
    }

    /// The trace of [`Trace::encode`]'s bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader(bytes);
        if r.take(4)? != MAGIC {
            return Err("not an E2 trace (magic)".to_owned());
        }
        let version = r.u32()?;
        if !(1..=VERSION).contains(&version) {
            return Err(format!("E2 trace version {version}, this build reads 1 … {VERSION}"));
        }
        let temperature = f64::from_le_bytes(r.array()?);
        let seed = u64::from_le_bytes(r.array()?);
        let defer = version >= 2 && r.u8()? != 0;
        let mut trace = Trace::new(Sampling { temperature, seed }).with_defer(defer);
        for _ in 0..r.u32()? {
            let player = r.u8()?;
            let round = r.u32()?;
            let position = r.u32()?;
            let has_value = r.u8()? != 0;
            let value = r.i32()?;
            let n = r.u32()?;
            let mut decisions = Vec::with_capacity(n.min(1 << 16) as usize);
            for _ in 0..n {
                let head = r.u8()?;
                let kind = r.u8()?;
                let (index, generation) = (r.u32()?, r.u32()?);
                let actor = match kind {
                    0 => Actor::Player,
                    1 => Actor::Unit { index, generation },
                    2 | 3 => Actor::City { index, generation, cleared: kind == 3 },
                    other => return Err(format!("actor kind {other}")),
                };
                let position = r.u32()?;
                let candidates = r.u16()?;
                let accepted = r.u16()?;
                let chosen = Some(r.u16()?).filter(|c| *c != u16::MAX);
                let score = r.i32()?;
                let logp = f32::from_le_bytes(r.array()?);
                let chosen_meta = [r.i32()?, r.i32()?, r.i32()?];
                let rows_hash = r.u32()?;
                let refused = (0..r.u16()?).map(|_| r.u16()).collect::<Result<Vec<_>, _>>()?;
                decisions.push(DecisionRecord { head, actor, position, candidates, accepted, chosen, score, logp, chosen_meta, rows_hash, refused });
            }
            trace.turns.push(TurnRecord { player, round, position, value: has_value.then_some(value), decisions });
        }
        if !r.0.is_empty() {
            return Err(format!("{} bytes after the E2 trace", r.0.len()));
        }
        Ok(trace)
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        if self.0.len() < n {
            return Err("the E2 trace is truncated".to_owned());
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        Ok(self.take(N)?.try_into().expect("N bytes"))
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.array()?))
    }
}

/// SplitMix64's finaliser.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The uniform draw in [0, 1) of one decision: a function of the sampling
/// seed, the player, the round and the decision's ordinal in the
/// player-turn only.
pub fn draw(seed: u64, player: PlayerId, round: u32, ordinal: u32) -> f64 {
    let h = mix(mix(mix(mix(seed) ^ u64::from(player.0)) ^ u64::from(round)) ^ u64::from(ordinal));
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// The choice and its log-probability among the accepted (`ok`) logits
/// (`scores`, Q8) at `temperature` with the uniform draw `u`: `T ≤ 0` (or
/// not finite) the largest, ties to the lowest index (log-probability 0);
/// else the softmax of `s / (256 T)` sampled by inversion. None: nothing
/// accepted.
pub fn pick(scores: &[i32], ok: &[bool], temperature: f64, u: f64) -> Option<(usize, f64)> {
    let accepted = || (0..scores.len()).filter(|&i| ok[i]);
    let best = accepted().fold(None, |best: Option<usize>, i| match best {
        Some(b) if scores[b] >= scores[i] => Some(b),
        _ => Some(i),
    })?;
    if !(temperature.is_finite() && temperature > 0.0) {
        return Some((best, 0.0));
    }
    let scale = 1.0 / (256.0 * temperature);
    let top = f64::from(scores[best]);
    let weight = |i: usize| ((f64::from(scores[i]) - top) * scale).exp();
    let total: f64 = accepted().map(weight).sum();
    let target = u * total;
    let mut cumulative = 0.0;
    let mut at = best;
    for i in accepted() {
        cumulative += weight(i);
        if cumulative > target {
            at = i;
            break;
        }
    }
    Some((at, (f64::from(scores[at]) - top) * scale - total.ln()))
}

/// FNV-1a 32 over the candidates' rows (i16) and metas (i32), little-endian,
/// in order: the exporter rebuilds the same bytes at the same moment.
pub fn rows_hash(candidates: &[Candidate]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u32::from(*b);
            h = h.wrapping_mul(0x0100_0193);
        }
    };
    for c in candidates {
        for v in &c.row {
            eat(&v.to_le_bytes());
        }
        for m in c.meta {
            eat(&m.to_le_bytes());
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_temperature_is_the_argmax_with_ties_to_the_lowest() {
        let scores = [5, 9, 9, 1];
        assert_eq!(pick(&scores, &[true; 4], 0.0, 0.7), Some((1, 0.0)));
        assert_eq!(pick(&scores, &[true, false, true, true], 0.0, 0.7), Some((2, 0.0)));
        assert_eq!(pick(&scores, &[false; 4], 1.0, 0.7), None);
    }

    #[test]
    fn sampling_follows_the_softmax_of_the_accepted() {
        // Logits 0 and ln 3 (× 256): probabilities ¼ and ¾; the refused one is outside.
        let ln3 = (3f64.ln() * 256.0).round() as i32;
        let scores = [0, ln3, 10_000];
        let ok = [true, true, false];
        let (at, logp) = pick(&scores, &ok, 1.0, 0.1).unwrap();
        assert_eq!(at, 0);
        assert!((logp - 0.25f64.ln()).abs() < 1e-2, "{logp}");
        let (at, logp) = pick(&scores, &ok, 1.0, 0.9).unwrap();
        assert_eq!(at, 1);
        assert!((logp - 0.75f64.ln()).abs() < 1e-2, "{logp}");
        let hits = (0..10_000).filter(|i| pick(&scores, &ok, 1.0, draw(7, PlayerId(1), 3, *i)).unwrap().0 == 1).count();
        assert!((7_300..7_700).contains(&hits), "{hits}");
    }

    #[test]
    fn the_trace_bytes_round_trip() {
        let mut trace = Trace::new(Sampling { temperature: 0.5, seed: 42 });
        trace.begin_turn(PlayerId(2), 7, Some(-31));
        let c = |tag: i32| Candidate { row: vec![tag as i16; 3], meta: [tag, 5, -1, 9], command: None, targets: Vec::new() };
        let candidates = [c(1), c(2), c(3)];
        trace.choose(0, Actor::Unit { index: 4, generation: 1 }, &[10, 300, 20], &candidates, |c| c.meta[0] != 2);
        trace.stamp(17);
        trace.choose(2, Actor::City { index: 3, generation: 0, cleared: true }, &[], &[], |_| true);
        trace.stamp(18);
        let bytes = trace.encode();
        let back = Trace::decode(&bytes).unwrap();
        assert_eq!(back, trace);
        let deferring = trace.clone().with_defer(true);
        assert_eq!(Trace::decode(&deferring.encode()).unwrap(), deferring);
        // Version 1 (no defer byte) reads as a trace without defer.
        let mut v1 = bytes[..24].to_vec();
        v1[4..8].copy_from_slice(&1u32.to_le_bytes());
        v1.extend_from_slice(&bytes[25..]);
        assert_eq!(Trace::decode(&v1).unwrap(), trace);
        assert_eq!(back.turns[0].position, 17);
        assert_eq!(back.turns[0].decisions[0].refused, vec![1]);
        assert_eq!(back.turns[0].decisions[1].position, 18);
        assert_eq!(back.turns[0].decisions[1].chosen, None);
    }

    #[test]
    fn the_defer_candidate_is_offered_until_the_units_deferrals_are_taken() {
        use crate::ai::net_e2e::encode::tag;
        let c = |tag: i32| Candidate { row: vec![0; 2], meta: [tag, -1, -1, -1], command: None, targets: Vec::new() };
        let unit = Actor::Unit { index: 3, generation: 0 };
        let other = Actor::Unit { index: 4, generation: 0 };
        let mut trace = Trace::new(Sampling { temperature: 0.0, seed: 1 }).with_defer(true);
        trace.begin_turn(PlayerId(0), 5, None);
        // Unit 3 defers (the defer candidate scores best), unit 4 waits, unit 3 decides again.
        trace.choose(0, unit, &[0, 9], &[c(tag::WAIT), c(tag::DEFER)], |_| true);
        trace.choose(0, other, &[9, 0], &[c(tag::WAIT), c(tag::DEFER)], |_| true);
        trace.choose(0, unit, &[9], &[c(tag::WAIT)], |_| true);
        assert!(trace.defer_offered(0, 0, 1) && trace.defer_offered(0, 1, 1));
        assert!(!trace.defer_offered(0, 2, 1), "unit 3 deferred once");
        assert!(trace.defer_offered(0, 2, 2));
        assert!(!trace.clone().with_defer(false).defer_offered(0, 0, 1));
    }
}
