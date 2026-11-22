//! The threat of a rival running away (`strategist-x`, `weights::threat`): per met rival, from its
//! public screens only (`ai::rival_model`), how near it is to win the race we cannot follow. The
//! plan names the rival with the largest threat when it is at least `weights::threat::MIN`.

use crate::ai::rival_model::{Rival, Rivals};

use super::layers::ONE;
use super::weights::threat as w;
use super::wishes::Threat;
use super::Milli;

/// The threat of one rival (0…1000).
pub fn of(model: &Rivals, rival: &Rival) -> Milli {
    let total = model.techs_total.max(1);
    let tree = (rival.techs.value * ONE / total).clamp(0, ONE);
    let lead = ((rival.techs.value - model.own_techs) * ONE * w::LEAD_SCALE / total).clamp(0, ONE);
    let pace = match rival.science {
        Some(theirs) if theirs + model.own_science > 0 => theirs.max(0) * ONE / (theirs.max(0) + model.own_science.max(0)).max(1),
        _ => ONE / 2,
    };
    let sum = w::BIAS.0 + w::TREE.0 * tree / ONE + w::LEAD.0 * lead / ONE + w::PACE.0 * 2 * (pace - ONE / 2) / ONE;
    sum.clamp(0, ONE)
}

/// The rival running away: the largest threat among the met rivals, at least `MIN` (ties: the lower id).
pub fn assess(model: &Rivals) -> Option<Threat> {
    let mut best: Option<Threat> = None;
    for rival in &model.rivals {
        let level = of(model, rival);
        if level < w::MIN.0 {
            continue;
        }
        if best.is_none_or(|b| level > b.level) {
            best = Some(Threat { rival: rival.player, level });
        }
    }
    best
}
