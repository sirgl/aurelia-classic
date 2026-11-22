//! Part A: the inputs of `net2` (design §2.2, §2.4, §4.5) as functions of the seat's observation and
//! memory. STUB of the contract: every input at its neutral value; part A writes the computations and
//! owns this file and `rival_inputs.rs`.

use crate::ai::planner::TurnBrief;
use crate::ai::rival_model::Rivals;
use crate::ai::state::AiState;
use crate::ai::strategy_net::weights::fe;
use crate::ai::strategy_net::Milli;
use crate::knowledge::observation::AIObservation;
use crate::rules::CompiledRuleset;

use super::plan::{CityInputs, Inputs, SeatInputs};

/// What the inputs keep between turns (samples for G4, G5, R5, the R2/R3 survey and its round).
/// Part A owns its fields and its bytes; the core writes them inside `NET2_SECTION`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputMemory {}

impl InputMemory {
    /// Appends the bytes of the memory to `out`.
    pub fn encode(&self, _out: &mut Vec<u8>) {}

    /// The memory at the start of `bytes` and the bytes it took; None for bytes that do not read.
    pub fn decode(_bytes: &[u8]) -> Option<(InputMemory, usize)> {
        Some((InputMemory {}, 0))
    }
}

/// The turn's inputs: `situation` the carried features (`strategy_net::situation`), `rivals` the model
/// of the rivals' screens; `memory` is updated (samples, surveys) as the inputs need.
pub fn read(obs: &AIObservation<'_>, brief: &TurnBrief, state: &AiState, rules: &CompiledRuleset, situation: &[Milli; fe::N], rivals: &Rivals, memory: &mut InputMemory) -> Inputs {
    let seat = seat(obs, brief, state, rules, memory);
    let rivals = super::rival_inputs::read(obs, brief, state, rules, rivals, memory);
    let cities = cities(obs, brief, state);
    Inputs { situation: *situation, seat, rivals, cities, pairs: Vec::new(), aims: Vec::new() }
}

/// G1–G10 (§2.2).
fn seat(_obs: &AIObservation<'_>, _brief: &TurnBrief, _state: &AiState, _rules: &CompiledRuleset, _memory: &InputMemory) -> SeatInputs {
    SeatInputs { war_gain: 500, ..SeatInputs::default() }
}

/// The guard's inputs per own city (§4.5).
fn cities(obs: &AIObservation<'_>, _brief: &TurnBrief, _state: &AiState) -> Vec<CityInputs> {
    obs.own_cities.iter().map(|c| CityInputs { city: c.id, stack_owner: None, danger: 0, rivals_near: Vec::new(), young: 0 }).collect()
}
