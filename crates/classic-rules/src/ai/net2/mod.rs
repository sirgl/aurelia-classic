//! `net2`: one hand-written strategic net of a seat (work/new-net-design-2026-11-22.md). One forward
//! pass a turn: the strategist's intent (style, aim, posture, prices, city target) and the war, peace,
//! deal, defence, city-state and site levers the scripted executors read (`plan::Net2Plan`).
//!
//! | file | part | what |
//! |---|---|---|
//! | `plan` | B | the contract: inputs, outputs, memory |
//! | `inputs`, `rival_inputs` | A | the inputs from the observation |
//! | `site_head` | B | the site score (§3.4) |
//!
//! It does not move units: the commander, the tactician and the danger net stay their own.

pub mod inputs;
pub mod plan;
pub mod rival_inputs;
pub mod site_head;

use crate::ai::grand_strategy::Stakes;
use crate::ai::planner::TurnBrief;
use crate::ai::state::AiState;
use crate::ai::value::Prices;
use crate::knowledge::observation::AIObservation;
use crate::rules::CompiledRuleset;

pub use plan::{Net2Plan, Plan2};

/// The turn of `net2` for a seat, at the place of `strategy_net::think` (`driver.rs`): the same
/// arguments, the strategist's plan and the levers. CONTRACT STUB: the carried strategist and the
/// default levers (plays exactly as the strategist).
pub fn think(obs: &AIObservation<'_>, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes) -> Plan2 {
    let strategy = crate::ai::strategy_net::think(obs, brief, state, rules, prices, stakes);
    let net2 = Net2Plan { round: obs.round, tall_share: strategy.intent.style[0], ..Net2Plan::default() };
    Plan2 { strategy, net2 }
}
