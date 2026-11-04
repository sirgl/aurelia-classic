//! The economy advisor (M4-A): plugs the economy decision modules into the
//! driver's registry (`ai::system`).
//!
//! ```text
//! start of turn   owed free technologies and great people are taken
//!                 (ai::grants); great people act where they stand (ai::great_people),
//!                 before the planner moves units; a happiness building
//!                 while happiness is short (ai::happiness); trade units
//!                 start their best route, a trade unit is queued while
//!                 routes are left (ai::trade); with priced happiness
//!                 (`PriceModel::Uses`) the allocator buys it instead
//! end of turn     city focus (ai::citizens; by the prices under
//!                 `PriceModel::Uses`)
//! ```
//!
//! Workers stay with the planner (`planner_workers`), which uses the core's
//! site rule (`world::builds::site`).

use crate::ai::system::{AdvisorInput, Proposal, SystemAdvisor};
use crate::command::Command;

/// Priority of a great person action (before ordinary economy orders).
pub const GREAT_PERSON_PRIORITY: i32 = 20;
/// Priority of a happiness building order.
pub const HAPPINESS_PRIORITY: i32 = 10;
/// Priority of a trade route and of a trade unit order.
pub const TRADE_PRIORITY: i32 = 8;
/// Priority of a city focus change.
pub const FOCUS_PRIORITY: i32 = 5;

/// The economy advisor (stateless).
#[derive(Debug, Clone, Copy, Default)]
pub struct EconomyAdvisor;

fn proposals(priority: i32, commands: Vec<Command>) -> Vec<Proposal> {
    commands
        .into_iter()
        .map(|command| Proposal { priority, command })
        .collect()
}

impl SystemAdvisor for EconomyAdvisor {
    /// Before the planner: its production orders of this turn then see
    /// the happiness building (no duplicate order from one observation).
    fn start_of_turn(&self, input: &AdvisorInput<'_, '_>) -> Vec<Proposal> {
        let great = if input.brief.abilities {
            crate::ai::unit_abilities::great_person_commands(input)
        } else {
            crate::ai::great_people::great_person_commands(input.observation, input.rules)
        };
        // Priced happiness is the allocator's to buy (`PriceModel::Uses`).
        let happiness = match valued(input) {
            Some(_) => Vec::new(),
            None => crate::ai::happiness::happiness_commands(input.observation, input.rules),
        };
        let free = crate::ai::grants::free_choice_commands(input.observation, input.rules);
        let mut out = proposals(GREAT_PERSON_PRIORITY + 1, free);
        out.extend(proposals(GREAT_PERSON_PRIORITY, great));
        out.extend(proposals(HAPPINESS_PRIORITY, happiness));
        let routes = match valued(input).filter(|_| input.brief.tall.plays()) {
            // `trade-fit`: the routes by what they deliver, kept safe; stuck units rehomed.
            Some(prices) if input.brief.trade_fit => crate::ai::trade::fit_route_commands(input.observation, input.rules, prices),
            Some(prices) => crate::ai::tall::route_commands(input.observation, prices, input.brief.tall),
            None => crate::ai::trade::route_commands(input.observation),
        };
        out.extend(proposals(TRADE_PRIORITY, routes));
        // `trade-fit`: a trade unit ahead of a building where it pays more.
        if let Some(prices) = valued(input).filter(|_| input.brief.trade_fit) {
            out.extend(proposals(TRADE_PRIORITY, crate::ai::trade::fit_ahead_commands(input.observation, input.rules, prices)));
        }
        // Priced trade units are the allocator's to build.
        if valued(input).is_none() {
            out.extend(proposals(TRADE_PRIORITY, crate::ai::trade::production_commands(input.observation, input.rules)));
        }
        out
    }

    fn end_of_turn(&self, input: &AdvisorInput<'_, '_>) -> Vec<Proposal> {
        let focus = crate::ai::citizens::focus_commands_priced(input.observation, input.rules, valued(input), input.brief.growth_policy);
        let mut out = proposals(FOCUS_PRIORITY, focus);
        if let Some(prices) = valued(input).filter(|_| input.brief.levers.specialists) {
            out.extend(proposals(FOCUS_PRIORITY, crate::ai::citizens::specialist_commands(input.observation, input.rules, prices)));
        }
        out
    }
}

/// The turn's prices when the AI decides its economy by them
/// (`PriceModel::Uses`); None: by the advisors' own rules.
fn valued<'i>(input: &'i AdvisorInput<'_, '_>) -> Option<&'i crate::ai::value::Prices> {
    input.brief.prices.as_ref().filter(|p| p.model == crate::ai::value::PriceModel::Uses)
}
