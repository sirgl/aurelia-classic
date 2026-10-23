//! Save of the World Congress (`world::congress`).

use crate::ids::PlayerId;
use crate::persistence::decode::DecodeError;
use crate::persistence::schema::SavedCongress;
use crate::rules::content::ProjectId;
use crate::world::congress::{
    Active, CongressLists, CongressState, LeagueRewards, ProjectProgress, Proposal, ResolutionKind, Vote,
};

pub(crate) fn encode_congress(c: &CongressState) -> SavedCongress {
    SavedCongress {
        founded: c.founded(),
        host: c.host().map(|p| p.0),
        special: c.special_session(),
        united_nations: c.united_nations(),
        turns_until_session: c.turns_until_session(),
        session_round: c.session_round(),
        leader_vote_due: c.leader_vote_due(),
        proposers: c.proposers().iter().map(|p| p.0).collect(),
        proposals: c
            .proposals()
            .iter()
            .map(|p| (p.kind.tag(), p.choice, p.proposer.map(|x| x.0), p.repeal))
            .collect(),
        delegates: c.delegates().to_vec(),
        votes: c
            .votes()
            .iter()
            .map(|v| (v.voter.0, v.proposal, v.choice, v.delegates))
            .collect(),
        active: c
            .active()
            .iter()
            .map(|a| (a.kind.tag(), a.choice))
            .collect(),
        extra: c.extra().to_vec(),
        last_round: c.last_round(),
        projects: c
            .projects()
            .iter()
            .map(|p| (p.project.0, p.contributions.clone()))
            .collect(),
        completed: c.completed().iter().map(|p| p.0).collect(),
        rewards: c
            .rewards()
            .iter()
            .map(|r| (r.happiness, r.culture_boost_turns, r.tourism_boost_turns))
            .collect(),
    }
}

fn kind(tag: u8) -> Result<ResolutionKind, DecodeError> {
    ResolutionKind::ALL
        .get(tag as usize)
        .copied()
        .ok_or_else(|| DecodeError::Corrupt("congress: resolution".into()))
}

pub(crate) fn decode_congress(
    s: &SavedCongress,
    players: usize,
    projects: usize,
) -> Result<CongressState, DecodeError> {
    let corrupt = |what: &str| DecodeError::Corrupt(format!("congress: {what}"));
    let player = |p: u8| {
        ((p as usize) < players)
            .then_some(PlayerId(p))
            .ok_or_else(|| corrupt("player"))
    };
    let sizes = s.delegates.len() == players && s.extra.len() == players && s.rewards.len() == players;
    if !sizes || s.special as usize >= 4 {
        return Err(corrupt("sizes"));
    }
    let project = |p: u16| {
        ((p as usize) < projects)
            .then_some(ProjectId(p))
            .ok_or_else(|| corrupt("project"))
    };
    let mut progress = Vec::with_capacity(s.projects.len());
    for (id, contributions) in &s.projects {
        if contributions.len() != players {
            return Err(corrupt("contributions"));
        }
        progress.push(ProjectProgress {
            project: project(*id)?,
            contributions: contributions.clone(),
        });
    }
    let mut proposals = Vec::with_capacity(s.proposals.len());
    for (tag, choice, proposer, repeal) in &s.proposals {
        proposals.push(Proposal {
            kind: kind(*tag)?,
            choice: *choice,
            proposer: proposer.map(player).transpose()?,
            repeal: *repeal,
        });
    }
    let mut votes = Vec::with_capacity(s.votes.len());
    for (voter, proposal, choice, delegates) in &s.votes {
        votes.push(Vote {
            voter: player(*voter)?,
            proposal: *proposal,
            choice: *choice,
            delegates: *delegates,
        });
    }
    let mut active = Vec::with_capacity(s.active.len());
    for (tag, choice) in &s.active {
        active.push(Active {
            kind: kind(*tag)?,
            choice: *choice,
        });
    }
    let lists = CongressLists {
        proposers: s
            .proposers
            .iter()
            .map(|p| player(*p))
            .collect::<Result<_, _>>()?,
        proposals,
        delegates: s.delegates.clone(),
        votes,
        active,
        extra: s.extra.clone(),
        last_round: s.last_round,
        projects: progress,
        completed: s.completed.iter().map(|p| project(*p)).collect::<Result<_, _>>()?,
        rewards: s
            .rewards
            .iter()
            .map(|(happiness, culture, tourism)| LeagueRewards {
                happiness: *happiness,
                culture_boost_turns: *culture,
                tourism_boost_turns: *tourism,
            })
            .collect(),
    };
    Ok(CongressState::from_parts(
        s.founded,
        s.host.map(player).transpose()?,
        s.special,
        s.united_nations,
        s.turns_until_session,
        s.session_round,
        s.leader_vote_due,
        lists,
    ))
}
