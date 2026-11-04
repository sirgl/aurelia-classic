//! What the commander keeps between turns (`AiState::mind_memory`): each
//! operation's kind, target, phase, the round the phase began and the
//! option held (the hysteresis). `hybrid` keeps the strategy net's memory
//! in the same bytes: an envelope holds both (`split`, `join`); a mind that
//! keeps only one writes it alone, as before.

use super::commander::{OpKind, Phase};
use crate::ids::TileId;

/// One operation as remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpMemory {
    pub kind: OpKind,
    pub target: TileId,
    pub phase: Phase,
    pub since: u32,
    pub choice: u8,
}

/// The commander's memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitMemory {
    pub ops: Vec<OpMemory>,
}

/// The first byte of the commander's memory.
const MAGIC: u8 = 0x75;
/// The first byte of an envelope of the strategy net's and the commander's memories.
const ENVELOPE: u8 = 0xE5;

fn kind_code(k: OpKind) -> u8 {
    match k {
        OpKind::Attack => 0,
        OpKind::Defence => 1,
        OpKind::Camp => 2,
    }
}

fn phase_code(p: Phase) -> u8 {
    match p {
        Phase::Gather => 0,
        Phase::Approach => 1,
        Phase::Assault => 2,
        Phase::Hold => 3,
        Phase::Retreat => 4,
    }
}

impl UnitMemory {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![MAGIC, self.ops.len() as u8];
        for o in &self.ops {
            out.push(kind_code(o.kind));
            out.extend_from_slice(&o.target.0.to_le_bytes());
            out.push(phase_code(o.phase));
            out.extend_from_slice(&o.since.to_le_bytes());
            out.push(o.choice);
        }
        out
    }

    /// The memory in `bytes`; empty for anything else (another mind's bytes).
    pub fn decode(bytes: &[u8]) -> Self {
        let mut out = Self::default();
        let Some((&MAGIC, rest)) = bytes.split_first() else { return out };
        let Some((&n, mut rest)) = rest.split_first() else { return out };
        for _ in 0..n {
            if rest.len() < 11 {
                return Self::default();
            }
            let kind = match rest[0] {
                0 => OpKind::Attack,
                1 => OpKind::Defence,
                _ => OpKind::Camp,
            };
            let target = TileId(u32::from_le_bytes([rest[1], rest[2], rest[3], rest[4]]));
            let phase = match rest[5] {
                0 => Phase::Gather,
                1 => Phase::Approach,
                2 => Phase::Assault,
                3 => Phase::Hold,
                _ => Phase::Retreat,
            };
            let since = u32::from_le_bytes([rest[6], rest[7], rest[8], rest[9]]);
            let choice = rest[10];
            out.ops.push(OpMemory { kind, target, phase, since, choice });
            rest = &rest[11..];
        }
        out
    }
}

/// The strategy net's part and the commander's part of a mind's memory.
pub fn split(bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    match bytes.first() {
        Some(&ENVELOPE) if bytes.len() >= 5 => {
            let len = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
            let end = (5 + len).min(bytes.len());
            (bytes[5..end].to_vec(), bytes[end..].to_vec())
        }
        Some(&MAGIC) => (Vec::new(), bytes.to_vec()),
        _ => (bytes.to_vec(), Vec::new()),
    }
}

/// The two parts in one memory: alone when the other is empty.
pub fn join(strategy: &[u8], unit: &[u8]) -> Vec<u8> {
    if unit.is_empty() {
        return strategy.to_vec();
    }
    if strategy.is_empty() {
        return unit.to_vec();
    }
    let mut out = vec![ENVELOPE];
    out.extend_from_slice(&(strategy.len() as u32).to_le_bytes());
    out.extend_from_slice(strategy);
    out.extend_from_slice(unit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_round_trips_and_shares_the_bytes() {
        let m = UnitMemory { ops: vec![OpMemory { kind: OpKind::Defence, target: TileId(77), phase: Phase::Hold, since: 12, choice: 2 }] };
        assert_eq!(UnitMemory::decode(&m.encode()), m);
        assert_eq!(UnitMemory::decode(&[0x5A, 1, 2]), UnitMemory::default(), "another mind's bytes read as empty");
        let strategy = vec![0x5A, 9, 9, 9];
        let both = join(&strategy, &m.encode());
        assert_eq!(split(&both), (strategy.clone(), m.encode()));
        assert_eq!(split(&strategy), (strategy.clone(), Vec::new()));
        assert_eq!(join(&strategy, &[]), strategy);
    }
}
