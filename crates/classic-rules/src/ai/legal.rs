//! The AI orders only what the rules allow (classic/docs/ai-legal-orders.md).
//!
//! Every order of the AI passes the rules' own check, `PlayerQueries::refusal`
//! (the validation of the command the interface greys its buttons with),
//! before it leaves the AI: never a formula copied from the rules. The check
//! sits where an order is chosen, so that a refused one is replaced by the
//! next best, and again where the decisions of a turn are handed on, so that
//! no refused order is sent even when a planner forgot to ask:
//!
//! - production (`planner_cities::plan_cities_with`): a city whose chosen
//!   order is refused plans again with that item banned for it
//!   (`Empire::refused`; the pickers skip a banned item), a few times; what
//!   is still refused after that is withheld;
//! - purchases and research: asked where the offer is made;
//! - unit orders and the advisors' proposals: `filter` replaces a refused
//!   decision by `Reason::Forbidden`, and a unit's task counts it as a failed
//!   attempt (`AiState::note_failure`), as a refusal at the core did.
//!
//! What can still reach the core refused is the change of the world between
//! the check and the execution inside one turn: an earlier order of the same
//! batch (two cities buying the same item, a treasury spent by the first
//! purchase).

use crate::ai::planner::Decision;
use crate::ai::trace::{AiCounters, Reason};
use crate::command::{Command, CommandError, Refusal};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::knowledge::view::PlayerView;

/// Times a city plans again after its choice was refused.
pub const MAX_REPLANS: usize = 4;

/// Do the rules accept the decision's command now (a decision without a
/// command is nothing to refuse)?
pub fn permitted(view: &PlayerView<'_>, decision: &Decision) -> bool {
    decision.command.as_ref().is_none_or(|command| view.accepts(command))
}

/// What the rules say of each decision of one city's list of orders, as
/// the game is now. A list is a sequence. After a `ClearProduction` the
/// queue the orders meet is not the one the rules see yet (the same list
/// puts the old head back), so an item the city "has queued already" is no
/// refusal there; every other refusal of the order is. A purchase after an
/// order asks for a head that is not there yet and is not asked here (the
/// last look, `AiMind::permits`, asks it when it is sent).
fn verdicts(view: &PlayerView<'_>, decisions: &[Decision]) -> Vec<Option<CommandError>> {
    let (mut cleared, mut ordered) = (false, false);
    decisions
        .iter()
        .map(|d| {
            let command = d.command.as_ref()?;
            let verdict = match command {
                Command::HurryProduction { .. } if ordered => None,
                _ => view.refusal(command),
            };
            match command {
                Command::ClearProduction { .. } => cleared = true,
                Command::EnqueueProduction { .. } => ordered = true,
                _ => {}
            }
            let queue_only = matches!(verdict, Some(CommandError::Refused(Refusal::AlreadyQueued)));
            if queue_only && cleared && matches!(command, Command::EnqueueProduction { .. }) {
                None
            } else {
                verdict
            }
        })
        .collect()
}

/// The decisions with the refused ones replaced by `Reason::Forbidden`
/// (counted as withheld). Not sent, but no refusal of a choice (counted as
/// redundant, `Reason::Idle`): an order for an item the city has queued
/// already, and the put-back of a queued head that the rules no longer let
/// the city build (a settler of a city that has shrunk, a caravan with no
/// route left): clearing the queue to put a war order first drops it.
pub fn filter(obs: &AIObservation<'_>, decisions: Vec<Decision>, counters: Option<&mut AiCounters>) -> Vec<Decision> {
    let verdicts = verdicts(&obs.view, &decisions);
    let queued = |command: &Command| match command {
        Command::EnqueueProduction { city, item, .. } => obs.own_cities.iter().any(|c| c.handle == *city && c.queue.contains(item)),
        _ => false,
    };
    let (mut withheld, mut redundant) = (0, 0);
    let out = decisions
        .into_iter()
        .zip(verdicts)
        .map(|(decision, verdict)| match verdict {
            None => decision,
            Some(error) if matches!(error, CommandError::Refused(Refusal::AlreadyQueued)) || decision.command.as_ref().is_some_and(queued) => {
                redundant += 1;
                Decision::refuse(decision.actor, Reason::Idle)
            }
            Some(_) => {
                withheld += 1;
                Decision::refuse(decision.actor, Reason::Forbidden)
            }
        })
        .collect();
    if let Some(counters) = counters {
        counters.orders_withheld += withheld;
        counters.orders_redundant += redundant;
    }
    out
}

/// The production orders among the decisions that the rules refuse for a
/// reason that calls for another order: the items to ban. An item the city
/// has queued already (`Refusal::AlreadyQueued`) is no refusal to plan
/// around: the order's purpose is served (`filter` drops it).
pub fn refused_items(view: &PlayerView<'_>, decisions: &[Decision]) -> Vec<crate::ids::ProductionItem> {
    decisions
        .iter()
        .zip(verdicts(view, decisions))
        .filter_map(|(d, verdict)| match (&d.command, verdict) {
            (Some(Command::EnqueueProduction { item, .. }), Some(error)) if !matches!(error, CommandError::Refused(Refusal::AlreadyQueued)) => Some(*item),
            _ => None,
        })
        .collect()
}

/// The technologies the rules let the player research now, when its queue
/// is empty (None otherwise: no research is chosen this turn).
pub fn researchable(obs: &AIObservation<'_>, brief: &crate::ai::planner::TurnBrief, rules: &crate::rules::CompiledRuleset) -> Option<Vec<crate::ids::TechId>> {
    if !brief.research_queue.is_empty() {
        return None;
    }
    Some(
        (0..rules.technologies.len())
            .map(|i| crate::ids::TechId(i as u16))
            .filter(|tech| obs.view.accepts(&Command::ChooseTechnology { player: obs.player, technology: *tech }))
            .collect(),
    )
}

/// A unit's decision whose command the rules refuse becomes `Forbidden`,
/// and the unit's task counts a failed attempt, as a refusal at the core did.
pub fn unit_order(near: &crate::knowledge::observation::LocalObservation<'_>, state: &mut crate::ai::state::AiState, decision: Decision) -> Decision {
    if permitted(&near.view, &decision) {
        return decision;
    }
    if let crate::ai::trace::Actor::Unit(unit) = decision.actor {
        state.note_failure(unit);
    }
    Decision::refuse(decision.actor, Reason::Forbidden)
}
