//! Save of the victory state (`world::victory`).

use crate::ids::PlayerId;
use crate::persistence::decode::DecodeError;
use crate::persistence::schema::SavedVictory;
use crate::world::victory::{Outcome, VictoryKind, VictoryOptions, VictoryState};

fn kind_tag(kind: VictoryKind) -> u8 {
    VictoryKind::ALL
        .iter()
        .position(|k| *k == kind)
        .unwrap_or(0) as u8
}

pub(crate) fn encode_victory(state: &VictoryState) -> SavedVictory {
    let options = state.options();
    SavedVictory {
        disabled: options.disabled,
        max_rounds: options.max_rounds,
        project_counts: state.project_counts().to_vec(),
        world_leader: state.world_leader().map(|p| p.0),
        outcome: state
            .outcome()
            .map(|o| (kind_tag(o.kind), o.winner.0, o.round)),
        continued: state.continued(),
    }
}

pub(crate) fn decode_victory(
    saved: &SavedVictory,
    players: usize,
    projects: usize,
) -> Result<VictoryState, DecodeError> {
    let corrupt = |what: &str| DecodeError::Corrupt(format!("victory: {what}"));
    if saved.project_counts.len() != players * projects {
        return Err(corrupt("project counts"));
    }
    let player = |p: u8| {
        ((p as usize) < players)
            .then_some(PlayerId(p))
            .ok_or_else(|| corrupt("player"))
    };
    let world_leader = saved.world_leader.map(player).transpose()?;
    let outcome = match saved.outcome {
        Some((kind, winner, round)) => Some(Outcome {
            kind: *VictoryKind::ALL
                .get(kind as usize)
                .ok_or_else(|| corrupt("kind"))?,
            winner: player(winner)?,
            round,
        }),
        None => None,
    };
    let options = VictoryOptions {
        disabled: saved.disabled,
        max_rounds: saved.max_rounds,
    };
    Ok(VictoryState::from_parts(
        players,
        projects,
        options,
        saved.project_counts.clone(),
        world_leader,
        outcome,
        saved.continued,
    ))
}
