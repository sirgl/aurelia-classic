//! Save of the city-state state (`world::city_states`): standings, minor
//! and major records as plain tuples, quests as (city-state, major,
//! (tag, payload), start, end + 1). `Option<u32>` rounds are stored + 1
//! (0 = none).

use crate::ids::{BuildingKindId, PlayerId, ResourceId, TileId};
use crate::persistence::decode::DecodeError;
use crate::persistence::schema::SavedCityStates;
use crate::rules::CompiledRuleset;
use crate::world::city_states::{
    CityStatesState, MajorRecord, Minor, Personality, Quest, QuestKind, Standing,
};

fn some_round(round: Option<u32>) -> u32 {
    round.map(|r| r + 1).unwrap_or(0)
}

fn round_of(value: u32) -> Option<u32> {
    value.checked_sub(1)
}

fn corrupt(what: &str) -> DecodeError {
    DecodeError::Corrupt(format!("city-states: {what}"))
}

fn personality_tag(p: Personality) -> u8 {
    Personality::ALL.iter().position(|x| *x == p).unwrap_or(0) as u8
}

fn quest_kind_tag(kind: QuestKind) -> (u8, u32) {
    match kind {
        QuestKind::KillCamp(t) => (0, t.0),
        QuestKind::ConnectResource(r) => (1, u32::from(r.0)),
        QuestKind::ConstructWonder(b) => (2, u32::from(b.0)),
        QuestKind::FindPlayer(p) => (3, u32::from(p.0)),
        QuestKind::FindNaturalWonder { known } => (4, u32::from(known)),
        QuestKind::GiveGold { gifted } => (5, gifted),
        QuestKind::PledgeToProtect => (6, 0),
        QuestKind::BullyCityState(p) => (7, u32::from(p.0)),
        QuestKind::DenounceMajor(p) => (8, u32::from(p.0)),
        QuestKind::ContestTechs { start } => (9, u32::from(start)),
        QuestKind::Invest => (10, 0),
    }
}

pub(crate) fn encode_city_states(state: &CityStatesState) -> SavedCityStates {
    let standings = state
        .standings()
        .iter()
        .map(|s| {
            let flags =
                u8::from(s.ever_friends) | u8::from(s.wary) << 1 | u8::from(s.contacted) << 2;
            (
                s.influence as u32,
                flags,
                some_round(s.last_bullied),
                some_round(s.pledged),
                some_round(s.pledge_broken),
            )
        })
        .collect();
    let counters = state
        .standings()
        .iter()
        .map(|s| (s.spawn_counter as u16, s.quest_countdown as u32))
        .collect();
    let minors = state
        .minors()
        .iter()
        .map(|m| {
            (
                m.ally.map(|p| p.0 + 1).unwrap_or(0),
                m.allied_since,
                personality_tag(m.personality),
                m.global_countdown as u32,
            )
        })
        .collect();
    let majors = state
        .majors()
        .iter()
        .map(|m| (m.minors_attacked, m.gold_gifted, m.ally_great_person_turns))
        .collect();
    let quests = state
        .quests()
        .iter()
        .map(|q| {
            (
                q.city_state.0,
                q.major.0,
                quest_kind_tag(q.kind),
                q.start,
                some_round(q.end),
            )
        })
        .collect();
    SavedCityStates {
        standings,
        counters,
        minors,
        majors,
        quests,
    }
}

fn decode_standing(row: (u32, u8, u32, u32, u32), counters: (u16, u32)) -> Standing {
    let (influence, flags, bullied, pledged, broken) = row;
    Standing {
        influence: influence as i32,
        ever_friends: flags & 1 != 0,
        wary: flags & 2 != 0,
        contacted: flags & 4 != 0,
        last_bullied: round_of(bullied),
        pledged: round_of(pledged),
        pledge_broken: round_of(broken),
        spawn_counter: counters.0 as i16,
        quest_countdown: counters.1 as i32,
    }
}

fn decode_minor(row: (u8, u32, u8, u32), players: usize) -> Result<Minor, DecodeError> {
    let (ally, since, personality, countdown) = row;
    let ally = match ally {
        0 => None,
        p if (p as usize) <= players => Some(PlayerId(p - 1)),
        _ => return Err(corrupt("ally")),
    };
    let personality = *Personality::ALL
        .get(personality as usize)
        .ok_or_else(|| corrupt("personality"))?;
    Ok(Minor {
        ally,
        allied_since: since,
        personality,
        global_countdown: countdown as i32,
    })
}

fn decode_quest_kind(
    (tag, v): (u8, u32),
    rules: &CompiledRuleset,
    players: usize,
    tiles: usize,
) -> Option<QuestKind> {
    let player = |v: u32| ((v as usize) < players).then_some(PlayerId(v as u8));
    Some(match tag {
        0 if (v as usize) < tiles => QuestKind::KillCamp(TileId(v)),
        1 if (v as usize) < rules.resources.len() => {
            QuestKind::ConnectResource(ResourceId(v as u16))
        }
        2 if (v as usize) < rules.buildings.len() => {
            QuestKind::ConstructWonder(BuildingKindId(v as u16))
        }
        3 => QuestKind::FindPlayer(player(v)?),
        4 => QuestKind::FindNaturalWonder { known: v as u16 },
        5 => QuestKind::GiveGold { gifted: v },
        6 => QuestKind::PledgeToProtect,
        7 => QuestKind::BullyCityState(player(v)?),
        8 => QuestKind::DenounceMajor(player(v)?),
        9 => QuestKind::ContestTechs { start: v as u16 },
        10 => QuestKind::Invest,
        _ => return None,
    })
}

pub(crate) fn decode_city_states(
    saved: &SavedCityStates,
    players: usize,
    tiles: usize,
    rules: &CompiledRuleset,
) -> Result<CityStatesState, DecodeError> {
    if saved.standings.len() != players * players
        || saved.counters.len() != players * players
        || saved.minors.len() != players
        || saved.majors.len() != players
    {
        return Err(corrupt("sizes"));
    }
    let standings = saved
        .standings
        .iter()
        .zip(&saved.counters)
        .map(|(row, counters)| decode_standing(*row, *counters))
        .collect();
    let minors = saved
        .minors
        .iter()
        .map(|row| decode_minor(*row, players))
        .collect::<Result<Vec<_>, _>>()?;
    let majors = saved
        .majors
        .iter()
        .map(|(attacked, gifted, great_person_turns)| MajorRecord {
            minors_attacked: *attacked,
            gold_gifted: *gifted,
            ally_great_person_turns: *great_person_turns,
        })
        .collect();
    let mut quests = Vec::with_capacity(saved.quests.len());
    for (cs, major, kind, start, end) in &saved.quests {
        if *cs as usize >= players || *major as usize >= players {
            return Err(corrupt("quest players"));
        }
        let kind =
            decode_quest_kind(*kind, rules, players, tiles).ok_or_else(|| corrupt("quest kind"))?;
        quests.push(Quest {
            city_state: PlayerId(*cs),
            major: PlayerId(*major),
            kind,
            start: *start,
            end: round_of(*end),
        });
    }
    Ok(CityStatesState::from_parts(
        players, standings, minors, majors, quests,
    ))
}
