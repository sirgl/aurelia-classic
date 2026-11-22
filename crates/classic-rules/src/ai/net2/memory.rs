//! The bytes of `net2`'s memory (`plan::Memory2`), the body of `strategy_net`'s `NET2_SECTION`: the
//! core's war order (with its aim, opening and raid), the spent orders and the hires, then the inputs'
//! own (`inputs::InputMemory`). A body that does not read is no memory: the net starts fresh, as on
//! another mind's bytes.

use crate::ids::{PlayerId, TileId};

use super::inputs::InputMemory;
use super::plan::{AimWhy, CoreMemory, Memory2, Opening, SpentWar, WarAim, WarKind, WarOrder, WarPlan};

/// The body's format version (its first byte).
const VERSION: u8 = 2;

impl Memory2 {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![VERSION];
        encode_war(&self.core.war, &mut out);
        encode_spent(&self.core.spent, &mut out);
        encode_spent(&self.core.hires, &mut out);
        self.inputs.encode(&mut out);
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
        let (inputs, _) = InputMemory::decode(r.0)?;
        Some(Memory2 { core: CoreMemory { war, spent, hires }, inputs })
    }
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
            out.extend_from_slice(&o.value.to_le_bytes());
            encode_aim(o.aim, out);
            encode_opening(o.opening, out);
            out.push(u8::from(o.raid));
        }
    }
}

const AIMS: [AimWhy; 6] = [AimWhy::Capital, AimWhy::Young, AimWhy::Resource, AimWhy::Holy, AimWhy::Worth, AimWhy::Parts];

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
            let value = r.i64()?;
            let aim = decode_aim(r)?;
            let opening = decode_opening(r)?;
            let raid = r.u8()? == 1;
            Some(WarPlan::Order(WarOrder { rival, kind, since, budget, value, aim, opening, raid }))
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
        let order = WarOrder { rival: PlayerId(3), kind: WarKind::Overseas, since: 120, budget: 20, value: 640, aim, opening: Opening::Staged { since: 120, until: 130 }, raid: true };
        let spent = vec![SpentWar { rival: PlayerId(2), until: 140 }];
        let hires = vec![SpentWar { rival: PlayerId(4), until: 150 }];
        let memory = Memory2 { core: CoreMemory { war: WarPlan::Order(order), spent, hires }, inputs: InputMemory::default() };
        assert_eq!(Memory2::decode(&memory.encode()), Some(memory));
        assert_eq!(Memory2::decode(&Memory2::default().encode()), Some(Memory2::default()));
        assert_eq!(Memory2::decode(&[9, 0]), None, "another version is no memory");
    }
}
