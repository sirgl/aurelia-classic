//! Save of the espionage state (`world::espionage`): spies as
//! ((name, rank, state, city ordinal + 1), (amount, rate, goal),
//! (diplomat, dead turns)) per player; cities by their saved position.

use crate::ids::CityHandle;
use crate::persistence::decode::DecodeError;
use crate::persistence::schema::{SavedEspionage, SavedSpy};
use crate::persistence::encode::CityOrdinals;
use crate::world::espionage::{EspionageState, Spy, SpyState, Theft};

fn encode_spy(spy: &Spy, city_ordinals: &CityOrdinals) -> SavedSpy {
    let city = spy.city.and_then(|c| city_ordinals.of(c)).map_or(0, |o| o + 1);
    (
        (spy.name, spy.rank, spy.state.tag(), city),
        (spy.amount as u64, spy.rate as u64, spy.goal as u64),
        (spy.diplomat, spy.dead_turns),
    )
}

pub(crate) fn encode_espionage(state: &EspionageState, city_ordinals: &CityOrdinals) -> SavedEspionage {
    SavedEspionage {
        spies: state
            .all_spies()
            .iter()
            .map(|spies| spies.iter().map(|s| encode_spy(s, city_ordinals)).collect())
            .collect(),
        steal_rights: state.all_steal_rights().to_vec(),
        renaissance_granted: state.renaissance_granted(),
        era_granted: state.era_granted().to_vec(),
        thefts: state
            .all_thefts()
            .iter()
            .filter_map(|t| {
                let city = city_ordinals.of(t.city)? + 1;
                Some((t.round, t.victim.0, t.thief.map(|p| p.0 + 1).unwrap_or(0), city))
            })
            .collect(),
    }
}

fn decode_spy(saved: &SavedSpy, cities: &[CityHandle]) -> Result<Spy, DecodeError> {
    let corrupt = |what: &str| DecodeError::Corrupt(format!("espionage: {what}"));
    let ((name, rank, state, city), (amount, rate, goal), (diplomat, dead_turns)) = *saved;
    let state = *SpyState::ALL
        .get(state as usize)
        .ok_or_else(|| corrupt("state"))?;
    let city = match city {
        0 => None,
        n => Some(*cities.get(n as usize - 1).ok_or_else(|| corrupt("city"))?),
    };
    Ok(Spy {
        name,
        rank: rank.min(crate::world::espionage::MAX_RANK),
        state,
        city,
        amount: amount as i64,
        rate: rate as i64,
        goal: goal as i64,
        diplomat,
        dead_turns,
    })
}

pub(crate) fn decode_espionage(
    saved: &SavedEspionage,
    players: usize,
    cities: &[CityHandle],
) -> Result<EspionageState, DecodeError> {
    if saved.spies.len() != players
        || saved.steal_rights.len() != players * players
        || saved.era_granted.len() != players
    {
        return Err(DecodeError::Corrupt("espionage: sizes".into()));
    }
    let spies = saved
        .spies
        .iter()
        .map(|list| list.iter().map(|s| decode_spy(s, cities)).collect())
        .collect::<Result<Vec<Vec<Spy>>, _>>()?;
    let thefts = saved
        .thefts
        .iter()
        .map(|&(round, victim, thief, city)| {
            let corrupt = |what: &str| DecodeError::Corrupt(format!("espionage: {what}"));
            if usize::from(victim) >= players || usize::from(thief) > players || city == 0 {
                return Err(corrupt("theft"));
            }
            let city = *cities.get(city as usize - 1).ok_or_else(|| corrupt("theft city"))?;
            let thief = thief.checked_sub(1).map(crate::ids::PlayerId);
            Ok(Theft { round, victim: crate::ids::PlayerId(victim), thief, city })
        })
        .collect::<Result<Vec<Theft>, DecodeError>>()?;
    Ok(EspionageState::from_parts(
        players,
        spies,
        saved.steal_rights.clone(),
        saved.renaissance_granted,
        saved.era_granted.clone(),
        thefts,
    ))
}
