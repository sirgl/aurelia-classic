//! What the end of a human's turn waits for (Unciv `NextTurnAction`, Civ V "blocking notifications"):
//! the choices and orders a player still owes this turn. One list, computed from the game state;
//! the interface shows it and walks to the first item, the rules and the AI do not read it.
//!
//! ```text
//! blocks the end of the turn          order in the list
//! a free great person owed            FreeGreatPerson
//! a captured city to decide           CityDecision
//! a city with no production           EmptyProduction
//! a free technology owed              FreeTechnology
//! no technology being researched      Research
//! a policy that can be adopted        Policy   (once a turn: the caller says it was chosen)
//! a pantheon that can be founded      Pantheon
//! a unit that can be promoted         Promotion
//! a great person with no order        GreatPerson
//! a unit with moves and no order      UnitOrder
//! delegates left in an open session   CongressVotes   (until the Congress screen is looked at this turn)
//! an offer waiting for the answer     OfferWaiting    (until the diplomacy screen is looked at this turn)
//! a spy with no city                  IdleSpy         (until the espionage screen is looked at this turn)
//! ```
//!
//! An item is in the list only if there is a command that the core accepts and that clears it: the end of
//! the turn waits for the list, so an item the player cannot clear would lock the turn.
//!
//! A unit the player put to rest (skip, sleep) has an order. The Congress vote, an offer and an idle spy can be
//! declined, and no command says so: they are asked for until the player has looked at their screen this turn
//! (`HandledThisTurn`), so the list never locks the turn.

use crate::combat::experience::promotion_ready;
use crate::derived::culture::{branch_statuses, policy_statuses};
use crate::derived::grants::{next_great_person_options, next_technology_options, researchable};
use crate::derived::great_people::great_person_of;
use crate::derived::religion::{free_beliefs, pantheon_status};
use crate::ids::{CityHandle, PlayerId, UnitHandle};
use crate::rules::content::BeliefCategory;
use crate::rules::CompiledRuleset;
use crate::world::city::CityStatus;
use crate::world::espionage::SpyState;
use crate::world::state::AuthWorld;
use crate::world::unit::{Stance, UnitCore};

/// One thing the player must do before the turn may end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingAction {
    /// A free great person is owed (choose its kind).
    FreeGreatPerson,
    /// A conquered city waits for annex, puppet, raze or liberate.
    CityDecision(CityHandle),
    /// The city builds nothing and runs no process.
    EmptyProduction(CityHandle),
    /// A free technology is owed.
    FreeTechnology,
    /// No technology is being researched although one can be.
    Research,
    /// A policy or a branch can be adopted now.
    Policy,
    /// The faith pays a pantheon.
    Pantheon,
    /// The unit has the experience and a promotion to take.
    Promotion(UnitHandle),
    /// A great person waits with moves left and no order.
    GreatPerson(UnitHandle),
    /// A unit has moves left and no order.
    UnitOrder(UnitHandle),
    /// A Congress session is open, the player has delegates left and a proposal to put them on.
    CongressVotes,
    /// An offer of another nation waits for the player's answer (its id).
    OfferWaiting(u32),
    /// A spy has no city (its index in the player's spies).
    IdleSpy(u8),
}

/// What the player already settled this turn without a trace in the game state.
#[derive(Debug, Clone, Copy, Default)]
pub struct HandledThisTurn<'a> {
    /// A policy was adopted or a branch opened this turn: one choice a turn is asked for.
    pub policy_chosen: bool,
    /// Units told to skip the turn or to sleep.
    pub resting: &'a [UnitHandle],
    /// The Congress screen was opened this turn.
    pub congress_seen: bool,
    /// The diplomacy screen (where offers are answered) was opened this turn.
    pub offers_seen: bool,
    /// The espionage screen was opened this turn.
    pub spies_seen: bool,
}

/// The actions `player` owes, in the order the end-of-turn button asks for them. Before the first city
/// is founded (and after the last is lost) there is no science, culture or faith to direct: only the
/// units are asked for orders.
pub fn pending_actions(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, handled: HandledThisTurn<'_>) -> Vec<PendingAction> {
    let mut pending = Vec::new();
    if !auth.indexes().cities_of(player).is_empty() {
        pending.extend(choices(auth, rules, player, handled));
    }
    pending.extend(unit_actions(auth, rules, player, handled.resting));
    pending
}

/// The choices of a player with a city: free picks, cities, research, policy, faith.
fn choices(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, handled: HandledThisTurn<'_>) -> Vec<PendingAction> {
    let mut pending = Vec::new();
    if !next_great_person_options(auth, rules, player).is_empty() {
        pending.push(PendingAction::FreeGreatPerson);
    }
    push_city_actions(auth, player, &mut pending);
    if !next_technology_options(auth, rules, player).is_empty() {
        pending.push(PendingAction::FreeTechnology);
    }
    if research_pending(auth, rules, player) {
        pending.push(PendingAction::Research);
    }
    if !handled.policy_chosen && policy_pending(auth, rules, player) {
        pending.push(PendingAction::Policy);
    }
    if pantheon_pending(auth, rules, player) {
        pending.push(PendingAction::Pantheon);
    }
    if !handled.congress_seen && congress_pending(auth, player) {
        pending.push(PendingAction::CongressVotes);
    }
    if !handled.offers_seen {
        if let Some(offer) = auth.module().relations().offers().iter().find(|o| o.to == player) {
            pending.push(PendingAction::OfferWaiting(offer.id));
        }
    }
    if !handled.spies_seen {
        if let Some(spy) = auth.module().espionage().spies_of(player).iter().position(|s| s.state == SpyState::Unassigned) {
            pending.push(PendingAction::IdleSpy(spy as u8));
        }
    }
    pending
}

/// A session is open and the player may still put delegates on a proposal (the vote would be accepted).
pub fn congress_pending(auth: &AuthWorld, player: PlayerId) -> bool {
    let congress = auth.module().congress();
    congress.in_session()
        && congress.delegates_left(player) > 0
        && (0..congress.proposals().len()).any(|index| {
            let choice = if congress.proposals()[index].kind.elects() { u32::from(player.0) } else { crate::systems::congress::YES };
            crate::systems::congress::vote_problem(auth, player, index as u8, choice, 1).is_none()
        })
}

/// No technology is being researched although one can be.
pub fn research_pending(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> bool {
    auth.players().research_queue_of(player).is_empty() && !researchable(auth, rules, player).is_empty()
}

/// A policy can be adopted now (a tenet, or a branch to open): the Civics screen's own statuses say which
/// cards are ready.
pub fn policy_pending(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> bool {
    policy_statuses(auth, rules, player).iter().any(Result::is_ok) || branch_statuses(auth, rules, player).iter().any(Result::is_ok)
}

/// The faith pays a pantheon and a belief is free for it.
pub fn pantheon_pending(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> bool {
    let beliefs = free_beliefs(auth, rules, BeliefCategory::Pantheon);
    beliefs.first().is_some_and(|belief| pantheon_status(auth, rules, player, *belief).is_ok())
}

fn push_city_actions(auth: &AuthWorld, player: PlayerId, pending: &mut Vec<PendingAction>) {
    for handle in auth.indexes().cities_of(player) {
        let Some(city) = auth.cities().get(*handle) else {
            continue;
        };
        match city.status {
            CityStatus::AwaitingDecision => pending.push(PendingAction::CityDecision(*handle)),
            CityStatus::Puppet | CityStatus::Razing => {}
            CityStatus::Own | CityStatus::Occupied => {
                // A city in resistance refuses every production order, and the resistance ends only with
                // the owner's turns: asking now would lock the end of the turn.
                if city.resistance_turns == 0 && city.queue.is_empty() && city.economy.process.is_none() {
                    pending.push(PendingAction::EmptyProduction(*handle));
                }
            }
        }
    }
}

/// The unit items of the list: promotions first, then great people, then the other units, each group in the
/// player's unit order. `resting`: units told to skip the turn or to sleep.
pub fn unit_actions(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId, resting: &[UnitHandle]) -> Vec<PendingAction> {
    let mut pending = Vec::new();
    let mut great_people = Vec::new();
    let mut others = Vec::new();
    for handle in auth.indexes().units_of(player) {
        let Some(unit) = auth.units().get(*handle) else {
            continue;
        };
        let core: &UnitCore = unit.core;
        // The needed experience is at least 1: a unit with none skips the (costly) modifier query.
        if core.experience > 0 && promotion_ready(auth, rules, core) {
            pending.push(PendingAction::Promotion(*handle));
        }
        if !has_no_order(core) || resting.contains(handle) {
            continue;
        }
        if great_person_of(rules, core.kind).is_some() {
            great_people.push(PendingAction::GreatPerson(*handle));
        } else {
            others.push(PendingAction::UnitOrder(*handle));
        }
    }
    pending.extend(great_people);
    pending.extend(others);
    pending
}

/// Moves left, standing ready, no work, no march: nothing tells the unit what to do.
fn has_no_order(core: &UnitCore) -> bool {
    core.moves_left > 0 && core.stance == Stance::Ready && core.work.is_none() && core.goal.is_none()
}
