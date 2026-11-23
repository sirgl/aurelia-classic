//! The bytes of `net2`'s memory (`plan::Memory2`), the body of `strategy_net`'s `NET2_SECTION`: the
//! core's war order (with its aim, opening and raid), the spent orders and the hires, then the inputs'
//! own (`inputs::InputMemory`), then `science-flow`'s waits behind their marker, only when there are any
//! (`SCIENCE_SECTION`), so the other variants' memory keeps its bytes. A body that does not read is no
//! memory: the net starts fresh, as on another mind's bytes.

use crate::ids::{PlayerId, TileId};

use super::inputs::InputMemory;
use super::plan::{AimWhy, ChestWait, CoreMemory, Memory2, Opening, OrderPhase, ScienceMemory, SpentWar, WarAim, WarKind, WarOrder, WarPlan};

/// The body's format version (its first byte).
const VERSION: u8 = 3;
/// The marker of `science-flow`'s section after the inputs' (`Memory2::science`), written only when it holds anything.
const SCIENCE_SECTION: u8 = 0x53;
/// The marker of `layout-defence`'s section after the science's (`Memory2::whole`), written only when it holds anything.
const WHOLE_SECTION: u8 = 0x48;

impl Memory2 {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![VERSION];
        encode_war(&self.core.war, &mut out);
        encode_spent(&self.core.spent, &mut out);
        encode_spent(&self.core.hires, &mut out);
        self.inputs.encode(&mut out);
        if self.science != ScienceMemory::default() {
            encode_science(&self.science, &mut out);
        }
        if !self.whole.is_empty() {
            out.push(WHOLE_SECTION);
            out.push(self.whole.len().min(usize::from(u8::MAX)) as u8);
            out.extend(self.whole.iter().take(usize::from(u8::MAX)).map(|p| p.0));
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Memory2> {
        let mut r = Bytes(bytes);
        if r.u8()? != VERSION {
            return None;
        }
        let war = decode_war(&mut r)?;
        let spent = decode_spent(&mut r)?;
        let hires = decode_spent(&mut r)?;
        let (inputs, read) = InputMemory::decode(r.0)?;
        let mut rest = Bytes(r.0.get(read..)?);
        let science = if rest.0.first() == Some(&SCIENCE_SECTION) { decode_science(&mut rest)? } else { ScienceMemory::default() };
        let whole = if rest.0.first() == Some(&WHOLE_SECTION) { decode_whole(&mut rest)? } else { Vec::new() };
        Some(Memory2 { core: CoreMemory { war, spent, hires }, inputs, science, whole })
    }
}

fn decode_whole(r: &mut Bytes<'_>) -> Option<Vec<PlayerId>> {
    r.u8()?;
    let n = r.u8()?;
    (0..n).map(|_| r.u8().map(PlayerId)).collect()
}

fn encode_science(science: &ScienceMemory, out: &mut Vec<u8>) {
    out.push(SCIENCE_SECTION);
    out.extend_from_slice(&science.round.to_le_bytes());
    out.push(science.waits.len().min(usize::from(u8::MAX)) as u8);
    for wait in science.waits.iter().take(usize::from(u8::MAX)) {
        out.push(wait.rival.0);
        out.extend_from_slice(&wait.rounds.to_le_bytes());
    }
}

fn decode_science(r: &mut Bytes<'_>) -> Option<ScienceMemory> {
    r.u8()?;
    let round = r.u32()?;
    let n = r.u8()?;
    let mut waits = Vec::with_capacity(usize::from(n));
    for _ in 0..n {
        waits.push(ChestWait { rival: PlayerId(r.u8()?), rounds: r.u32()? });
    }
    Some(ScienceMemory { round, waits })
}

fn encode_spent(list: &[SpentWar], out: &mut Vec<u8>) {
    out.push(list.len().min(usize::from(u8::MAX)) as u8);
    for spent in list.iter().take(usize::from(u8::MAX)) {
        out.push(spent.rival.0);
        out.extend_from_slice(&spent.until.to_le_bytes());
    }
}

fn decode_spent(r: &mut Bytes<'_>) -> Option<Vec<SpentWar>> {
    let n = r.u8()?;
    let mut list = Vec::with_capacity(usize::from(n));
    for _ in 0..n {
        list.push(SpentWar { rival: PlayerId(r.u8()?), until: r.u32()? });
    }
    Some(list)
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
            match o.phase {
                OrderPhase::Preparing => out.push(0),
                OrderPhase::Fighting { since } => {
                    out.push(1);
                    out.extend_from_slice(&since.to_le_bytes());
                }
            }
            out.extend_from_slice(&o.value.to_le_bytes());
            encode_aim(o.aim, out);
            encode_opening(o.opening, out);
            out.push(u8::from(o.raid));
        }
    }
}

const AIMS: [AimWhy; 7] = [AimWhy::Capital, AimWhy::Young, AimWhy::Resource, AimWhy::Holy, AimWhy::Worth, AimWhy::Parts, AimWhy::Ease];

fn encode_aim(aim: Option<WarAim>, out: &mut Vec<u8>) {
    match aim {
        None => out.push(0),
        Some(aim) => {
            out.push(1);
            out.extend_from_slice(&aim.city.0.to_le_bytes());
            out.push(AIMS.iter().position(|w| *w == aim.why).expect("every aim is in AIMS") as u8);
        }
    }
}

fn decode_aim(r: &mut Bytes<'_>) -> Option<Option<WarAim>> {
    match r.u8()? {
        0 => Some(None),
        1 => {
            let city = TileId(r.u32()?);
            let why = *AIMS.get(usize::from(r.u8()?))?;
            Some(Some(WarAim { city, why }))
        }
        _ => None,
    }
}

fn encode_opening(opening: Opening, out: &mut Vec<u8>) {
    match opening {
        Opening::Plain => out.push(0),
        Opening::Staged { since, until } => {
            out.push(1);
            out.extend_from_slice(&since.to_le_bytes());
            out.extend_from_slice(&until.to_le_bytes());
        }
    }
}

fn decode_opening(r: &mut Bytes<'_>) -> Option<Opening> {
    match r.u8()? {
        0 => Some(Opening::Plain),
        1 => Some(Opening::Staged { since: r.u32()?, until: r.u32()? }),
        _ => None,
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
            let phase = match r.u8()? {
                0 => OrderPhase::Preparing,
                1 => OrderPhase::Fighting { since: r.u32()? },
                _ => return None,
            };
            let value = r.i64()?;
            let aim = decode_aim(r)?;
            let opening = decode_opening(r)?;
            let raid = r.u8()? == 1;
            Some(WarPlan::Order(WarOrder { rival, kind, since, budget, phase, value, aim, opening, raid }))
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
        let aim = Some(WarAim { city: TileId(77), why: AimWhy::Young });
        let order = WarOrder { rival: PlayerId(3), kind: WarKind::Overseas, since: 120, budget: 20, phase: OrderPhase::Fighting { since: 125 }, value: 640, aim, opening: Opening::Staged { since: 120, until: 130 }, raid: true };
        let spent = vec![SpentWar { rival: PlayerId(2), until: 140 }];
        let hires = vec![SpentWar { rival: PlayerId(4), until: 150 }];
        let inputs = InputMemory { infamy: 175, ..InputMemory::default() };
        let memory = Memory2 { core: CoreMemory { war: WarPlan::Order(order), spent, hires }, inputs, science: ScienceMemory::default(), whole: Vec::new() };
        assert_eq!(Memory2::decode(&memory.encode()), Some(memory.clone()));
        // Without waits the bytes are the ones before `science-flow`: the other variants' memory (and hashes) keep them.
        let mut before = vec![VERSION];
        encode_war(&memory.core.war, &mut before);
        encode_spent(&memory.core.spent, &mut before);
        encode_spent(&memory.core.hires, &mut before);
        memory.inputs.encode(&mut before);
        assert_eq!(memory.encode(), before);
        let science = ScienceMemory { round: 160, waits: vec![ChestWait { rival: PlayerId(2), rounds: 7 }] };
        let with_science = Memory2 { science, ..memory.clone() };
        assert_eq!(Memory2::decode(&with_science.encode()), Some(with_science.clone()));
        // `layout-defence`'s section after the science's, with or without it.
        let whole = vec![PlayerId(1), PlayerId(4)];
        assert_eq!(Memory2::decode(&Memory2 { whole: whole.clone(), ..with_science }.encode()).map(|m| m.whole), Some(whole.clone()));
        assert_eq!(Memory2::decode(&Memory2 { whole: whole.clone(), ..memory }.encode()).map(|m| m.whole), Some(whole));
        assert_eq!(Memory2::decode(&Memory2::default().encode()), Some(Memory2::default()));
        assert_eq!(Memory2::decode(&[9, 0]), None, "another version is no memory");
    }
}
