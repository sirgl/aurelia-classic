//! Save schema of great works, influence, public opinion and archaeology
//! (`world::tourism`): plain values, content ids as u16, cities by their
//! position in the saved city list, units by entity id. Decoding checks
//! every id and the invariants the tourism system keeps (a work sits in
//! one slot, slots hold works of their type and never overflow).

use crate::ids::{BuildingKindId, CityHandle, EntityId, PlayerId, TileId};
use crate::persistence::DecodeError;
use crate::rng::StreamRng;
use crate::rules::content::PolicyBranchId;
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;
use crate::world::tourism::{
    ArchRecord, Archaeology, ArtifactKind, CityWorks, Dig, DigSite, GreatWork, GreatWorkClass,
    GreatWorkId, PendingDig, PlacedWork, PlayerTourism, PublicOpinion, TourismParts, TourismState,
};

/// (tile, artifact kind 1..=6, era, first player, second player).
pub type SavedRecord = (u32, u8, u8, u8, Option<u8>);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedPlayerTourism {
    pub influence: Vec<u64>,
    pub influence_before: Vec<u64>,
    pub lifetime_before: u64,
    /// 0 content, 1 murmurs, 2 unrest, 3 upheaval.
    pub opinion: u8,
    pub dissatisfaction: u16,
    pub preferred: Option<u16>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedTourism {
    /// (class 0 art, 1 artifact, 2 literature, 3 music; era; creator).
    pub works: Vec<(u8, u8, u8)>,
    /// One per saved city, in the saved city order: (building, work).
    pub cities: Vec<Vec<(u16, u32)>>,
    pub players: Vec<SavedPlayerTourism>,
    pub records: Vec<SavedRecord>,
    /// (record, hidden).
    pub sites: Vec<(SavedRecord, bool)>,
    /// (unit, player, tile, turns left).
    pub digs: Vec<(u64, u8, u32, u16)>,
    /// (player, (record, hidden)).
    pub pending: Vec<(u8, (SavedRecord, bool))>,
    /// (tile, era).
    pub landmarks: Vec<(u32, u8)>,
    pub triggered: bool,
    pub rng: u64,
}

fn class_code(class: GreatWorkClass) -> u8 {
    match class {
        GreatWorkClass::Art => 0,
        GreatWorkClass::Artifact => 1,
        GreatWorkClass::Literature => 2,
        GreatWorkClass::Music => 3,
    }
}

fn class_of(code: u8) -> Option<GreatWorkClass> {
    Some(match code {
        0 => GreatWorkClass::Art,
        1 => GreatWorkClass::Artifact,
        2 => GreatWorkClass::Literature,
        3 => GreatWorkClass::Music,
        _ => return None,
    })
}

fn opinion_code(opinion: PublicOpinion) -> u8 {
    match opinion {
        PublicOpinion::Content => 0,
        PublicOpinion::Murmurs => 1,
        PublicOpinion::Unrest => 2,
        PublicOpinion::Upheaval => 3,
    }
}

fn opinion_of(code: u8) -> Option<PublicOpinion> {
    Some(match code {
        0 => PublicOpinion::Content,
        1 => PublicOpinion::Murmurs,
        2 => PublicOpinion::Unrest,
        3 => PublicOpinion::Upheaval,
        _ => return None,
    })
}

fn record_code(tile: TileId, r: &ArchRecord) -> SavedRecord {
    (
        tile.0,
        r.kind.value(),
        r.era,
        r.first.0,
        r.second.map(|p| p.0),
    )
}

pub(crate) fn encode_tourism(auth: &AuthWorld) -> SavedTourism {
    let state = auth.module().tourism();
    let a = state.archaeology();
    let site = |s: &DigSite| (record_code(s.tile, &s.record), s.hidden);
    SavedTourism {
        works: state
            .works()
            .iter()
            .map(|w| (class_code(w.class), w.era, w.creator.0))
            .collect(),
        cities: auth
            .cities()
            .iter()
            .map(|(handle, _)| {
                let works = &state.city(handle).works;
                works.iter().map(|w| (w.building.0, w.work.0)).collect()
            })
            .collect(),
        players: state.players().iter().map(encode_player).collect(),
        records: a.records.iter().map(|(t, r)| record_code(*t, r)).collect(),
        sites: a.sites.iter().map(site).collect(),
        digs: a
            .digs
            .iter()
            .map(|d| (d.unit.0, d.player.0, d.tile.0, d.turns_left))
            .collect(),
        pending: a
            .pending
            .iter()
            .map(|p| (p.player.0, site(&p.site)))
            .collect(),
        landmarks: a.landmarks.iter().map(|(t, e)| (t.0, *e)).collect(),
        triggered: a.triggered,
        rng: a.rng.state(),
    }
}

fn encode_player(p: &PlayerTourism) -> SavedPlayerTourism {
    SavedPlayerTourism {
        influence: p.influence.clone(),
        influence_before: p.influence_before.clone(),
        lifetime_before: p.lifetime_before,
        opinion: opinion_code(p.opinion),
        dissatisfaction: p.dissatisfaction,
        preferred: p.preferred.map(|b| b.0),
    }
}

/// What decoding checks ids against.
pub(crate) struct TourismContext<'a> {
    pub rules: &'a CompiledRuleset,
    pub players: usize,
    pub tiles: usize,
    pub cities: &'a [CityHandle],
    pub units: &'a [EntityId],
}

/// Untrusted data: every id resolves, the invariants hold.
pub(crate) fn decode_tourism(
    saved: &SavedTourism,
    cx: &TourismContext<'_>,
) -> Result<TourismState, DecodeError> {
    if saved.players.len() != cx.players || saved.cities.len() != cx.cities.len() {
        return Err(corrupt("tourism players/cities"));
    }
    let works = decode_works(saved, cx.players)?;
    let cities = decode_cities(saved, cx, &works)?;
    let mut players = Vec::with_capacity(cx.players);
    for p in &saved.players {
        players.push(decode_player(p, cx).ok_or_else(|| corrupt("tourism player"))?);
    }
    Ok(TourismState::from_parts(TourismParts {
        works,
        cities,
        players,
        archaeology: decode_archaeology(saved, cx).ok_or_else(|| corrupt("archaeology"))?,
    }))
}

fn decode_works(saved: &SavedTourism, players: usize) -> Result<Vec<GreatWork>, DecodeError> {
    let mut out = Vec::with_capacity(saved.works.len());
    for (class, era, creator) in &saved.works {
        let class = class_of(*class).ok_or_else(|| corrupt("great work class"))?;
        if usize::from(*creator) >= players {
            return Err(corrupt("great work creator"));
        }
        out.push(GreatWork {
            class,
            era: *era,
            creator: PlayerId(*creator),
        });
    }
    Ok(out)
}

fn decode_cities(
    saved: &SavedTourism,
    cx: &TourismContext<'_>,
    works: &[GreatWork],
) -> Result<Vec<CityWorks>, DecodeError> {
    let mut placed = vec![false; works.len()];
    let mut out: Vec<CityWorks> = Vec::new();
    for (ordinal, list) in saved.cities.iter().enumerate() {
        let mut city = CityWorks::default();
        for (building, work) in list {
            let id = GreatWorkId(*work);
            let fits = works
                .get(*work as usize)
                .is_some_and(|w| slot_fits(cx.rules, *building, w.class, &city));
            if !fits || std::mem::replace(&mut placed[*work as usize], true) {
                return Err(corrupt("great work slot"));
            }
            city.works.push(PlacedWork {
                building: BuildingKindId(*building),
                work: id,
            });
        }
        if city.works.is_empty() {
            continue;
        }
        let slot = cx.cities[ordinal].index as usize;
        if out.len() <= slot {
            out.resize(slot + 1, CityWorks::default());
        }
        out[slot] = city;
    }
    Ok(out)
}

/// The building has a free slot of the work's type in `city`.
fn slot_fits(
    rules: &CompiledRuleset,
    building: u16,
    class: GreatWorkClass,
    city: &CityWorks,
) -> bool {
    let Some((slot, count)) = rules
        .buildings
        .get(usize::from(building))
        .and_then(|b| b.module.great_work_slots)
    else {
        return false;
    };
    slot == class.slot() && city.count_in(BuildingKindId(building)) < usize::from(count)
}

fn decode_player(p: &SavedPlayerTourism, cx: &TourismContext<'_>) -> Option<PlayerTourism> {
    if p.influence.len() != cx.players || p.influence_before.len() != cx.players {
        return None;
    }
    let preferred = match p.preferred {
        Some(b) if usize::from(b) < cx.rules.module.policy_branches.len() => Some(PolicyBranchId(b)),
        Some(_) => return None,
        None => None,
    };
    Some(PlayerTourism {
        influence: p.influence.clone(),
        influence_before: p.influence_before.clone(),
        lifetime_before: p.lifetime_before,
        opinion: opinion_of(p.opinion)?,
        dissatisfaction: p.dissatisfaction,
        preferred,
    })
}

fn decode_record(r: &SavedRecord, cx: &TourismContext<'_>) -> Option<(TileId, ArchRecord)> {
    let (tile, kind, era, first, second) = *r;
    let kind = *ArtifactKind::ALL.iter().find(|k| k.value() == kind)?;
    let player_ok = |p: u8| usize::from(p) < cx.players;
    if (tile as usize) >= cx.tiles || !player_ok(first) || second.is_some_and(|p| !player_ok(p)) {
        return None;
    }
    let record = ArchRecord {
        kind,
        era,
        first: PlayerId(first),
        second: second.map(PlayerId),
    };
    Some((TileId(tile), record))
}

fn decode_site(s: &(SavedRecord, bool), cx: &TourismContext<'_>) -> Option<DigSite> {
    let (tile, record) = decode_record(&s.0, cx)?;
    Some(DigSite {
        tile,
        record,
        hidden: s.1,
    })
}

fn decode_archaeology(saved: &SavedTourism, cx: &TourismContext<'_>) -> Option<Archaeology> {
    let mut records = Vec::with_capacity(saved.records.len());
    for r in &saved.records {
        let entry = decode_record(r, cx)?;
        if records
            .last()
            .is_some_and(|(t, _): &(TileId, ArchRecord)| *t >= entry.0)
        {
            return None;
        }
        records.push(entry);
    }
    let sites = saved
        .sites
        .iter()
        .map(|s| decode_site(s, cx))
        .collect::<Option<Vec<_>>>()?;
    let mut digs = Vec::with_capacity(saved.digs.len());
    for (unit, player, tile, turns_left) in &saved.digs {
        let valid = cx.units.contains(&EntityId(*unit))
            && usize::from(*player) < cx.players
            && (*tile as usize) < cx.tiles;
        if !valid {
            return None;
        }
        digs.push(Dig {
            unit: EntityId(*unit),
            player: PlayerId(*player),
            tile: TileId(*tile),
            turns_left: *turns_left,
        });
    }
    let mut pending = Vec::with_capacity(saved.pending.len());
    for (player, site) in &saved.pending {
        if usize::from(*player) >= cx.players {
            return None;
        }
        pending.push(PendingDig {
            player: PlayerId(*player),
            site: decode_site(site, cx)?,
        });
    }
    let mut landmarks = Vec::with_capacity(saved.landmarks.len());
    for (tile, era) in &saved.landmarks {
        if (*tile as usize) >= cx.tiles {
            return None;
        }
        landmarks.push((TileId(*tile), *era));
    }
    Some(Archaeology {
        records,
        sites,
        digs,
        pending,
        landmarks,
        triggered: saved.triggered,
        rng: StreamRng::restore(saved.rng),
    })
}

fn corrupt(what: &str) -> DecodeError {
    DecodeError::Corrupt(what.to_string())
}

/// Mix the tourism state into the world hash. An untouched state mixes
/// nothing (worlds without works, influence or archaeology hash as before).
pub(crate) fn hash_tourism(auth: &AuthWorld, mix: &mut dyn FnMut(u64)) {
    if auth.module().tourism().is_untouched() {
        return;
    }
    let saved = encode_tourism(auth);
    mix(0x746F_7572);
    for (class, era, creator) in &saved.works {
        mix(u64::from(*class) << 16 | u64::from(*era) << 8 | u64::from(*creator));
    }
    for (i, list) in saved.cities.iter().enumerate() {
        for (building, work) in list {
            mix((i as u64) << 48 | u64::from(*building) << 32 | u64::from(*work));
        }
    }
    for p in &saved.players {
        p.influence
            .iter()
            .chain(&p.influence_before)
            .for_each(|v| mix(*v));
        mix(p.lifetime_before);
        mix(u64::from(p.opinion) << 32 | u64::from(p.dissatisfaction));
        mix(p.preferred.map(|b| u64::from(b) + 1).unwrap_or(0));
    }
    let record = |r: &SavedRecord| {
        u64::from(r.0) << 32
            | u64::from(r.1) << 24
            | u64::from(r.2) << 16
            | u64::from(r.3) << 8
            | r.4.map(|p| u64::from(p) + 1).unwrap_or(0)
    };
    saved.records.iter().for_each(|r| mix(record(r)));
    saved
        .sites
        .iter()
        .for_each(|(r, h)| mix(record(r) ^ u64::from(*h) << 63));
    for (unit, player, tile, turns) in &saved.digs {
        mix(*unit);
        mix(u64::from(*player) << 48 | u64::from(*tile) << 16 | u64::from(*turns));
    }
    for (player, (r, h)) in &saved.pending {
        mix(u64::from(*player) << 56 ^ record(r) ^ u64::from(*h) << 63);
    }
    saved
        .landmarks
        .iter()
        .for_each(|(t, e)| mix(u64::from(*t) << 8 | u64::from(*e)));
    mix(u64::from(saved.triggered));
    mix(saved.rng);
}
