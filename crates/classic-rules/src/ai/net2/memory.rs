//! The bytes of `net2`'s memory (`plan::Memory2`), the body of `strategy_net`'s `NET2_SECTION`: the
//! core's war order and spent orders, then the inputs' own (`inputs::InputMemory`). A body that does
//! not read is no memory: the net starts fresh, as on another mind's bytes.

use crate::ids::PlayerId;

use super::inputs::InputMemory;
use super::plan::{CoreMemory, Memory2, SpentWar, WarKind, WarOrder, WarPlan};

/// The body's format version (its first byte).
const VERSION: u8 = 1;

impl Memory2 {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![VERSION];
        encode_war(&self.core.war, &mut out);
        out.push(self.core.spent.len().min(usize::from(u8::MAX)) as u8);
        for spent in self.core.spent.iter().take(usize::from(u8::MAX)) {
            out.push(spent.rival.0);
            out.extend_from_slice(&spent.until.to_le_bytes());
        }
        self.inputs.encode(&mut out);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Memory2> {
        let mut r = Bytes(bytes);
        if r.u8()? != VERSION {
            return None;
        }
        let war = decode_war(&mut r)?;
        let n = r.u8()?;
        let mut spent = Vec::with_capacity(usize::from(n));
        for _ in 0..n {
            spent.push(SpentWar { rival: PlayerId(r.u8()?), until: r.u32()? });
        }
        let (inputs, _) = InputMemory::decode(r.0)?;
        Some(Memory2 { core: CoreMemory { war, spent }, inputs })
    }
}

fn encode_war(war: &WarPlan, out: &mut Vec<u8>) {
    match war {
        WarPlan::NoWar => out.push(0),
        WarPlan::Order(o) => {
            out.push(1);
            out.push(o.rival.0);
            out.push(kind_code(o.kind));
            out.extend_from_slice(&o.since.to_le_bytes());
            out.extend_from_slice(&o.budget.to_le_bytes());
            out.extend_from_slice(&o.value.to_le_bytes());
        }
    }
}

fn decode_war(r: &mut Bytes<'_>) -> Option<WarPlan> {
    match r.u8()? {
        0 => Some(WarPlan::NoWar),
        1 => {
            let rival = PlayerId(r.u8()?);
            let kind = *WarKind::ALL.get(usize::from(r.u8()?))?;
            let since = r.u32()?;
            let budget = r.u32()?;
            let value = r.i64()?;
            Some(WarPlan::Order(WarOrder { rival, kind, since, budget, value }))
        }
        _ => None,
    }
}

fn kind_code(kind: WarKind) -> u8 {
    WarKind::ALL.iter().position(|k| *k == kind).expect("every kind is in ALL") as u8
}

struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|b| b[0])
    }
    fn u32(&mut self) -> Option<u32> {
        self.take::<4>().map(u32::from_le_bytes)
    }
    fn i64(&mut self) -> Option<i64> {
        self.take::<8>().map(i64::from_le_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_memory_reads_back() {
        let order = WarOrder { rival: PlayerId(3), kind: WarKind::Overseas, since: 120, budget: 20, value: 640 };
        let memory = Memory2 { core: CoreMemory { war: WarPlan::Order(order), spent: vec![SpentWar { rival: PlayerId(2), until: 140 }] }, inputs: InputMemory::default() };
        assert_eq!(Memory2::decode(&memory.encode()), Some(memory));
        assert_eq!(Memory2::decode(&Memory2::default().encode()), Some(Memory2::default()));
        assert_eq!(Memory2::decode(&[9, 0]), None, "another version is no memory");
    }
}
