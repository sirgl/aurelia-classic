//! Save schema of faith and religion (`world::religion`): plain values,
//! content ids as u16, cities by their position in the saved city list,
//! units by entity id. Decoding checks every id and the invariants the
//! religion system keeps (one religion per founder, beliefs taken once,
//! pressure entries unique per creed).

use crate::ids::{BeliefId, CityHandle, EntityId, PlayerId};
use crate::persistence::DecodeError;
use crate::rng::StreamRng;
use crate::rules::content::ReligionId;
use crate::classic::effects::GreatPersonKind;
use crate::rules::CompiledRuleset;
use crate::world::religion::{
    AdoptionBonus, CityFaith, Creed, FoundedReligion, PlayerFaith, Pressure, ReligionParts, ReligionState,
    ReligiousUnit,
};
use crate::world::state::AuthWorld;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedPlayerFaith {
    pub faith: u32,
    pub pantheon: Option<u16>,
    pub religion: Option<u16>,
    pub prophets: u16,
    /// (great person kind index, bought).
    pub bought: Vec<(u8, u16)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedReligion {
    pub religion: u16,
    pub founder: u8,
    /// Position of the holy city in the saved city list.
    pub holy_city: u32,
    pub beliefs: Vec<u16>,
    pub enhanced: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedCityFaith {
    /// (creed, pressure): creed 0 atheism, 1 pantheon, 2 + religion id.
    pub pressure: Vec<(u16, u32)>,
    pub population: u32,
    /// The city paid its first-conversion bonus (`AdoptionBonus::Paid`).
    pub adoption_bonus_paid: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedReligiousUnit {
    pub unit: u64,
    pub religion: u16,
    pub spreads_left: u8,
    /// Codec 21; 0 in older saves.
    pub strength_lost: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedFaith {
    pub players: Vec<SavedPlayerFaith>,
    pub religions: Vec<SavedReligion>,
    /// One per saved city, in the saved city order.
    pub cities: Vec<SavedCityFaith>,
    pub units: Vec<SavedReligiousUnit>,
    pub pantheons: u16,
    pub prophet_rng: u64,
}

fn creed_code(creed: Creed) -> u16 {
    match creed {
        Creed::Atheism => 0,
        Creed::Pantheon => 1,
        Creed::Religion(r) => 2 + r.0,
    }
}

fn creed_of(code: u16, rules: &CompiledRuleset) -> Option<Creed> {
    match code {
        0 => Some(Creed::Atheism),
        1 => Some(Creed::Pantheon),
        n if usize::from(n - 2) < rules.module.religions.len() => Some(Creed::Religion(ReligionId(n - 2))),
        _ => None,
    }
}

const KINDS: [GreatPersonKind; 9] = [
    GreatPersonKind::Artist,
    GreatPersonKind::Engineer,
    GreatPersonKind::Merchant,
    GreatPersonKind::Scientist,
    GreatPersonKind::Writer,
    GreatPersonKind::Musician,
    GreatPersonKind::General,
    GreatPersonKind::Admiral,
    GreatPersonKind::Prophet,
];

pub(crate) fn encode_faith(auth: &AuthWorld) -> SavedFaith {
    let state = auth.module().religion();
    let ordinals = crate::persistence::encode::city_ordinals(auth);
    let players = (0..auth.players().count())
        .map(|i| encode_player(state.player(PlayerId(i as u8))))
        .collect();
    let religions = state
        .religions()
        .iter()
        .map(|r| SavedReligion {
            religion: r.religion.0,
            founder: r.founder.0,
            holy_city: ordinals[r.holy_city.index as usize],
            beliefs: r.beliefs.iter().map(|b| b.0).collect(),
            enhanced: r.enhanced,
        })
        .collect();
    let cities = auth
        .cities()
        .iter()
        .map(|(handle, _)| {
            let faith = state.city(handle);
            SavedCityFaith {
                pressure: faith
                    .pressure
                    .iter()
                    .map(|p| (creed_code(p.creed), p.amount))
                    .collect(),
                population: faith.population,
                adoption_bonus_paid: faith.adoption_bonus == AdoptionBonus::Paid,
            }
        })
        .collect();
    let units = state
        .units()
        .iter()
        .map(|u| SavedReligiousUnit {
            unit: u.unit.0,
            religion: u.religion.0,
            spreads_left: u.spreads_left,
            strength_lost: u.strength_lost,
        })
        .collect();
    SavedFaith {
        players,
        religions,
        cities,
        units,
        pantheons: state.pantheons(),
        prophet_rng: state.prophet_rng().state(),
    }
}

fn encode_player(p: &PlayerFaith) -> SavedPlayerFaith {
    SavedPlayerFaith {
        faith: p.faith,
        pantheon: p.pantheon.map(|b| b.0),
        religion: p.religion.map(|r| r.0),
        prophets: p.prophets,
        bought: p
            .bought
            .iter()
            .map(|(k, n)| (k.index() as u8, *n))
            .collect(),
    }
}

/// Untrusted data: every id resolves, the invariants hold.
pub(crate) fn decode_faith(
    saved: &SavedFaith,
    rules: &CompiledRuleset,
    players: usize,
    cities: &[CityHandle],
    units: &[EntityId],
) -> Result<ReligionState, DecodeError> {
    if saved.players.len() != players || saved.cities.len() != cities.len() {
        return Err(corrupt("faith players/cities"));
    }
    let mut out_players = Vec::with_capacity(players);
    for (i, p) in saved.players.iter().enumerate() {
        out_players
            .push(decode_player(p, rules).ok_or_else(|| corrupt(&format!("faith player {i}")))?);
    }
    let religions = decode_religions(saved, rules, players, cities)?;
    let mut out_cities = Vec::new();
    for (ordinal, c) in saved.cities.iter().enumerate() {
        let faith =
            decode_city(c, rules).ok_or_else(|| corrupt(&format!("faith city {ordinal}")))?;
        let slot = cities[ordinal].index as usize;
        if out_cities.len() <= slot {
            out_cities.resize(slot + 1, CityFaith::default());
        }
        out_cities[slot] = faith;
    }
    let mut out_units = Vec::with_capacity(saved.units.len());
    for u in &saved.units {
        let religion = ReligionId(u.religion);
        let known = units.contains(&EntityId(u.unit)) && religions.iter().any(|r| r.religion == religion);
        if !known || u.strength_lost > 100 {
            return Err(corrupt("religious unit"));
        }
        out_units.push(ReligiousUnit {
            unit: EntityId(u.unit),
            religion,
            spreads_left: u.spreads_left,
            strength_lost: u.strength_lost,
        });
    }
    check_founders(&out_players, &religions)?;
    Ok(ReligionState::from_parts(ReligionParts {
        players: out_players,
        religions,
        cities: out_cities,
        units: out_units,
        pantheons: saved.pantheons,
        prophet_rng: StreamRng::restore(saved.prophet_rng),
    }))
}

fn decode_player(p: &SavedPlayerFaith, rules: &CompiledRuleset) -> Option<PlayerFaith> {
    let pantheon = match p.pantheon {
        Some(b) if usize::from(b) < rules.module.beliefs.len() => Some(BeliefId(b)),
        Some(_) => return None,
        None => None,
    };
    let religion = match p.religion {
        Some(r) if usize::from(r) < rules.module.religions.len() => Some(ReligionId(r)),
        Some(_) => return None,
        None => None,
    };
    let mut bought = Vec::with_capacity(p.bought.len());
    for (k, n) in &p.bought {
        bought.push((*KINDS.get(usize::from(*k))?, *n));
    }
    Some(PlayerFaith {
        faith: p.faith,
        pantheon,
        religion,
        prophets: p.prophets,
        bought,
    })
}

fn decode_religions(
    saved: &SavedFaith,
    rules: &CompiledRuleset,
    players: usize,
    cities: &[CityHandle],
) -> Result<Vec<FoundedReligion>, DecodeError> {
    let mut out: Vec<FoundedReligion> = Vec::with_capacity(saved.religions.len());
    let mut taken: Vec<BeliefId> = Vec::new();
    for r in &saved.religions {
        let id = ReligionId(r.religion);
        let valid = usize::from(r.religion) < rules.module.religions.len()
            && usize::from(r.founder) < players
            && (r.holy_city as usize) < cities.len()
            && !out.iter().any(|x| x.religion == id);
        if !valid {
            return Err(corrupt("religion"));
        }
        let mut beliefs = Vec::with_capacity(r.beliefs.len());
        for b in &r.beliefs {
            let belief = BeliefId(*b);
            if usize::from(*b) >= rules.module.beliefs.len() || taken.contains(&belief) {
                return Err(corrupt("religion belief"));
            }
            taken.push(belief);
            beliefs.push(belief);
        }
        out.push(FoundedReligion {
            religion: id,
            founder: PlayerId(r.founder),
            holy_city: cities[r.holy_city as usize],
            beliefs,
            enhanced: r.enhanced,
        });
    }
    Ok(out)
}

fn decode_city(c: &SavedCityFaith, rules: &CompiledRuleset) -> Option<CityFaith> {
    let mut pressure: Vec<Pressure> = Vec::with_capacity(c.pressure.len());
    for (code, amount) in &c.pressure {
        let creed = creed_of(*code, rules)?;
        if *amount == 0 || pressure.last().is_some_and(|p| p.creed >= creed) {
            return None;
        }
        pressure.push(Pressure {
            creed,
            amount: *amount,
        });
    }
    let adoption_bonus = if c.adoption_bonus_paid {
        AdoptionBonus::Paid
    } else {
        AdoptionBonus::Unpaid
    };
    Some(CityFaith {
        pressure,
        population: c.population,
        adoption_bonus,
    })
}

/// A player's religion is founded by that player, and a founder has at
/// most one religion.
fn check_founders(
    players: &[PlayerFaith],
    religions: &[FoundedReligion],
) -> Result<(), DecodeError> {
    for (i, p) in players.iter().enumerate() {
        let founded: Vec<&FoundedReligion> = religions
            .iter()
            .filter(|r| r.founder == PlayerId(i as u8))
            .collect();
        let consistent = match p.religion {
            Some(r) => founded.len() == 1 && founded[0].religion == r,
            None => founded.is_empty(),
        };
        if !consistent {
            return Err(corrupt("religion founder"));
        }
    }
    Ok(())
}

fn corrupt(what: &str) -> DecodeError {
    DecodeError::Corrupt(what.to_string())
}

/// Mix the religion state into the world hash. An untouched state (no
/// faith, pantheon, religion, pressure or religious unit) mixes nothing.
pub(crate) fn hash_faith(auth: &AuthWorld, mix: &mut dyn FnMut(u64)) {
    let saved = encode_faith(auth);
    let untouched = saved.religions.is_empty()
        && saved.units.is_empty()
        && saved.pantheons == 0
        && saved
            .players
            .iter()
            .all(|p| *p == SavedPlayerFaith::default())
        && saved.cities.iter().all(|c| c.pressure.is_empty());
    if untouched {
        return;
    }
    mix(0x6661_6974);
    for p in &saved.players {
        mix(u64::from(p.faith));
        mix(p.pantheon.map(|b| u64::from(b) + 1).unwrap_or(0));
        mix(p.religion.map(|r| u64::from(r) + 1).unwrap_or(0));
        mix(u64::from(p.prophets));
        for (k, n) in &p.bought {
            mix(u64::from(*k) << 16 | u64::from(*n));
        }
    }
    for r in &saved.religions {
        mix(0x1_0000 | u64::from(r.religion));
        mix(u64::from(r.founder) << 32 | u64::from(r.holy_city));
        for b in &r.beliefs {
            mix(u64::from(*b));
        }
        mix(u64::from(r.enhanced));
    }
    for (i, c) in saved.cities.iter().enumerate() {
        if c.pressure.is_empty() && c.population == 0 {
            continue;
        }
        mix(0x2_0000 | i as u64);
        for (creed, amount) in &c.pressure {
            mix(u64::from(*creed) << 32 | u64::from(*amount));
        }
        mix(u64::from(c.population));
        if c.adoption_bonus_paid {
            mix(0x3_0000 | i as u64);
        }
    }
    for u in &saved.units {
        mix(u.unit);
        mix(u64::from(u.religion) << 8 | u64::from(u.spreads_left));
        if u.strength_lost > 0 {
            mix(0x5354_5245_4e47 << 8 | u64::from(u.strength_lost));
        }
    }
    mix(u64::from(saved.pantheons));
    mix(saved.prophet_rng);
}
