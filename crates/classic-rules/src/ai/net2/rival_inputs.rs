//! Part A: the per-rival inputs of `net2` (design §2.3, R1–R24). STUB of the contract: every met major
//! rival a stranger (`RivalInputs::stranger`); part A writes the computations and owns this file.

use crate::ai::planner::TurnBrief;
use crate::ai::rival_model::Rivals;
use crate::ai::state::AiState;
use crate::knowledge::observation::AIObservation;
use crate::rules::CompiledRuleset;
use crate::world::player::PlayerKind;

use super::inputs::InputMemory;
use super::plan::{RivalInputs, MAX_RIVALS};

/// Every met major rival's inputs, by id, at most `MAX_RIVALS`.
pub fn read(obs: &AIObservation<'_>, _brief: &TurnBrief, _state: &AiState, _rules: &CompiledRuleset, _rivals: &Rivals, _memory: &mut InputMemory) -> Vec<RivalInputs> {
    let mut out = Vec::new();
    for relation in &obs.relations {
        if relation.kind != PlayerKind::Major || out.len() == MAX_RIVALS {
            continue;
        }
        let mut row = RivalInputs::stranger(relation.other);
        row.at_war = if relation.at_war { 1000 } else { 0 };
        row.friend = if relation.friends { 1000 } else { 0 };
        row.pact = if relation.defensive_pact { 1000 } else { 0 };
        row.denounced_us = if relation.they_denounced { 1000 } else { 0 };
        out.push(row);
    }
    out
}
