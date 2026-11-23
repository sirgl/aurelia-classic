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
pub mod credit;
pub mod inputs;
pub mod levers;
pub mod memory;
pub mod offence;
pub mod orders;
pub mod plan;
pub mod rival_inputs;
pub mod science;
pub mod site_head;
pub mod trace;
pub mod weights;

#[cfg(test)]
mod tests;

use crate::ai::grand_strategy::Stakes;
use crate::ai::planner::TurnBrief;
use crate::ai::state::AiState;
use crate::ai::strategy_net::weights::pd;
use crate::ai::strategy_net::{self, wishes::Wishes, Memory, Net, Seat};
use crate::ai::value::Prices;
use crate::knowledge::observation::AIObservation;
use crate::rules::CompiledRuleset;

pub use plan::{Net2Plan, Part, Parts, Plan2};

/// The turn of `net2` for a seat, at the place of `strategy_net::think` (`driver.rs`): the same
/// arguments; the strategist's plan with `net2`'s intent and prices, and the levers.
/// `parts`: the parts of `net2` the seat plays (`plan::Parts`; a part that does not play is `nets-x-site`'s).
pub fn think(obs: &AIObservation<'_>, brief: &TurnBrief, state: &mut AiState, rules: &CompiledRuleset, prices: &Prices, stakes: &Stakes, parts: Parts) -> Plan2 {
    let _span = prof::span("net2/think");
    let seat = Seat { obs, brief, rules, prices };
    let mut memory = Memory::decode(&state.mind_memory);
    let situation = strategy_net::situation(seat, state, stakes, &mut memory);
    let mut own = memory.net2.take().unwrap_or_default();
    let mut inputs = {
        let _span = prof::span("net2/inputs");
        inputs::read(obs, brief, state, rules, &situation.features, &situation.rivals, &mut own.inputs)
    };
    // `science-flow`: how long our gold waited for each partner's agreement (`r::CHEST_WAIT`); 0 for every other variant.
    if brief.science_flow {
        science::read_waits(&own.science, obs, &mut inputs.rivals);
    }
    let net = net_of(brief, parts);
    let carried = strategy_net::concepts_net(&situation.features, net);
    let pass = levers::before_intent(&inputs, &carried, &own.core, obs.round, obs.speed.pace_percent, parts);
    let tilt = levers::tilt(&pass);
    let logits = std::env::var_os("NET2_TRACE").map(|_| strategy_net::logits_net(&situation.features, &memory.held, net));
    let mut intent = strategy_net::intent_step(seat, &mut memory, &situation, net, &tilt);
    inputs.seat.size_model = size_model(parts, &intent);
    let after = levers::after_intent(&pass, &intent, &inputs, obs.round);
    cap_cities(obs, parts, &mut intent, after.city_cap);
    if let Some(logits) = &logits {
        let shares = levers::shares(&intent);
        trace::turn(&trace::Record { player: obs.player.0, round: obs.round, pace: obs.speed.pace_percent, inputs: &inputs, held: &own.core, logits, tempered: net.sharp, pass: &pass, tilt: &tilt, shares: &shares, after: &after });
    }
    space_trace(obs.player.0, obs.round, &inputs, after.part_hold);
    let mut levers = after.plan;
    // `science-flow`: the science flow's levers (`science`); every other variant keeps them off.
    if brief.science_flow {
        levers.science = science::plan(obs, rules, &pass, &intent, &inputs, &mut own.science);
    }
    read_soldiers(&mut levers, &own.inputs);
    // The offence whose inputs the pass has (`offence`; the driver clears it for `net2-defence`).
    offence::apply(&mut levers, &pass);
    memory.net2 = Some(plan::Memory2 { core: pass.core.clone(), inputs: own.inputs, science: own.science });
    let _plan_span = prof::span("net2/plan_with_intent");
    let price_tilt = if parts.plays(Part::Economy) { after.price_tilt } else { [0; pd::N] };
    let strategy = strategy_net::plan_with_intent(seat, state, memory, &situation, intent, &price_tilt, Wishes::default());
    Plan2 { strategy, net2: levers }
}

/// G15 `size_model`: the new cities the rules' size model pays (`tall_real_step` wrote them into the intent), 250 a
/// city up to 4; 0 for a seat that does not play the real tall style or without the strategy part.
fn size_model(parts: Parts, intent: &strategy_net::Intent) -> strategy_net::Milli {
    if !intent.tall_real || !parts.plays(Part::Strategy) {
        return 0;
    }
    strategy_net::Milli::from(intent.more_cities.min(4)) * 250
}

/// A real tall seat's new cities are the net's (`weights::CITY_CAP`), not the rules' size model's (`tall_real_step`
/// wrote the model's); the executors (`tall_real::adjust`, `Prices::city_cap`) read the plan's target made of them.
/// Without the strategy part the seat keeps the model, as `nets-x-site` plays.
fn cap_cities(obs: &AIObservation<'_>, parts: Parts, intent: &mut strategy_net::Intent, city_cap: strategy_net::Milli) {
    if !intent.tall_real || !parts.plays(Part::Strategy) {
        return;
    }
    let more = levers::cities_of(city_cap);
    if strategy_net::trace::enabled() {
        strategy_net::trace::push(format!("r{} p{} CAP cities {} model more {} net {} → more {}", obs.round, obs.player.0, obs.own_cities.len(), intent.more_cities, city_cap, more));
    }
    intent.more_cities = more;
}

/// `NET2_SPACE_TRACE=<file>`: one line a turn of the space race (the parts' needs, the free copies, the hold's level).
/// It decides nothing.
fn space_trace(player: u8, round: u32, inputs: &plan::Inputs, level: strategy_net::Milli) {
    if inputs.space.is_empty() {
        return;
    }
    let Some(path) = std::env::var_os("NET2_SPACE_TRACE") else { return };
    let needs: Vec<String> = inputs.space.iter().map(|x| format!("[{},{},{}]", x.resource.0, x.need, x.free)).collect();
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write as _;
        let _ = writeln!(file, "{{\"p\":{player},\"r\":{round},\"hold\":{level},\"needs\":[{}]}}", needs.join(","));
    }
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
/// and without the tall first (its −1200 is `net2`'s own term, `weights::TILT`); when the strategy is not
/// `net2`'s part, with the tall first, as `nets-x-site` thinks.
pub fn net_of(brief: &TurnBrief, parts: Parts) -> Net {
    Net { open: brief.war_open, sharp: brief.sharp, pays: brief.war_pays, threat: 0, tall_first: !parts.plays(Part::Strategy) }
}
