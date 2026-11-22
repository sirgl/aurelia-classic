//! `net2`: one hand-written strategic net of a seat (work/new-net-design-2026-11-22.md). One forward
//! pass a turn: the strategist's intent (style, aim, posture, prices, city target) and the war, peace,
//! deal, defence, city-state and site levers the scripted executors read (`plan::Net2Plan`).
//!
//! | layer | what | file |
//! |---|---|---|
//! | L0 | the carried features (`strategy_net::situation`), the seat's, the rivals', the cities' inputs | `inputs`, `rival_inputs` (part A) |
//! | L1 | the 13 carried concepts (`strategy_net::concepts_net`), 4 seat concepts, 12 per rival | `concepts`, `weights::SEAT`, `weights::RIVAL` |
//! | — | the war order: start ≥ 550, kept ≥ 350 inside its budget, spent → peace | `levers` |
//! | L2 | the carried logits + `net2`'s tilt (`weights::TILT`) → softmax, held picks, lock | `strategy_net::intent_step` |
//! | L2 | the prices (+ the military tilt), the goals, the city target | `strategy_net::plan_with_intent` |
//! | L2 | peace, trust, help, guard, city-state share, embargo, escorts, generals | `levers::after_intent` |
//! | site | one bounded neuron per candidate tile | `site_head` |
//!
//! Every weight is in `weights` with its reason; `trace` exports them and the turns for the float twin
//! (`classic/nn/aurelia_nn/net2`). It does not move units: the commander, the tactician and the danger
//! net stay their own.

pub mod concepts;
pub mod inputs;
pub mod levers;
pub mod memory;
pub mod plan;
pub mod rival_inputs;
pub mod site_head;
pub mod trace;
pub mod weights;

#[cfg(test)]
mod tests;

use crate::ai::grand_strategy::Stakes;
use crate::ai::planner::TurnBrief;
use crate::ai::state::AiState;
use crate::ai::strategy_net::{self, wishes::Wishes, Memory, Net, Seat};
use crate::ai::value::Prices;
use crate::knowledge::observation::AIObservation;
use crate::rules::CompiledRuleset;

pub use plan::{Net2Plan, Plan2};

/// The turn of `net2` for a seat, at the place of `strategy_net::think` (`driver.rs`): the same
/// arguments; the strategist's plan with `net2`'s intent and prices, and the levers.
pub fn think(obs: &AIObservation<'_>, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes) -> Plan2 {
    let _span = prof::span("net2/think");
    let seat = Seat { obs, brief, rules, prices };
    let mut memory = Memory::decode(&state.mind_memory);
    let situation = strategy_net::situation(seat, state, stakes, &mut memory);
    let mut own = memory.net2.take().unwrap_or_default();
    let inputs = {
        let _span = prof::span("net2/inputs");
        inputs::read(obs, brief, state, rules, &situation.features, &situation.rivals, &mut own.inputs)
    };
    let net = net_of(brief);
    let carried = strategy_net::concepts_net(&situation.features, net);
    let pass = levers::before_intent(&inputs, &carried, &own.core, obs.round, obs.speed.pace_percent);
    let tilt = levers::tilt(&pass);
    let logits = std::env::var_os("NET2_TRACE").map(|_| strategy_net::logits_net(&situation.features, &memory.held, net));
    let intent = strategy_net::intent_step(seat, &mut memory, &situation, net, &tilt);
    let after = levers::after_intent(&pass, &intent, &inputs, obs.round);
    if let Some(logits) = &logits {
        let shares = levers::shares(&intent);
        trace::turn(&trace::Record { player: obs.player.0, round: obs.round, pace: obs.speed.pace_percent, inputs: &inputs, held: &own.core, logits, tempered: net.sharp, pass: &pass, tilt: &tilt, shares: &shares, after: &after });
    }
    let mut levers = after.plan;
    read_soldiers(&mut levers, &own.inputs);
    memory.net2 = Some(plan::Memory2 { core: pass.core.clone(), inputs: own.inputs });
    let _plan_span = prof::span("net2/plan_with_intent");
    let strategy = strategy_net::plan_with_intent(seat, state, memory, &situation, intent, &after.price_tilt, Wishes::default());
    Plan2 { strategy, net2: levers }
}

/// Part A's soldiers estimate of every rival read (§2.4 step 4), so that the war machine's `known_power`
/// reads it from the plan and never decodes the memory.
fn read_soldiers(levers: &mut Net2Plan, inputs: &inputs::InputMemory) {
    for read in &mut levers.rivals {
        if let Some(estimate) = inputs.soldiers_of(read.rival) {
            read.soldiers = plan::Soldiers::Estimate { count: estimate.value, doubt: estimate.doubt() };
        }
    }
}

/// The carried net `net2` thinks with: the brief's, without the threat's posture term (§4.3: removed)
/// and without the tall first (its −1200 is `net2`'s own term, `weights::TILT`).
pub fn net_of(brief: &TurnBrief) -> Net {
    Net { open: brief.war_open, sharp: brief.sharp, pays: brief.war_pays, threat: 0, tall_first: false }
}
