//! Minimal diplomacy decisions over `AIObservation` (M4 track C): answers
//! to deal offers, proposals (embassies, friendship, research agreements,
//! pacts, open borders, peace), denunciations. Values are a plain gold
//! scale; the relation data is the public diplomacy of the observation
//! and the AI's own warmonger memory.

use crate::ai::peace::PeaceMaking;
use crate::knowledge::public::PlayerQueries;
use crate::command::Command;
use crate::ids::PlayerId;
use crate::knowledge::observation::{AIObservation, Relation};
use crate::rules::CompiledRuleset;
use crate::world::player::PlayerKind;
use crate::world::relations::{DealItem, DealOffer};

/// No friendship before this many rounds of contact; own value, unverified.
pub const FRIENDSHIP_MIN_CONTACT_ROUNDS: u32 = 20;
/// The AI accepts a peace (and proposes one to a rival in the diplomacy
/// advisor) only after this many rounds of war. Not the length a war lasts
/// in the game's strategy: that is `planner_strategy::MIN_WAR_ROUNDS`, which
/// is longer (the strategy's own `MakePeace` waits for it; the pair is
/// fixed by the test `peace_is_accepted_before_the_strategy_asks_for_it`).
pub const ACCEPT_PEACE_AFTER_ROUNDS: u32 = 5;
/// Warmonger memory above which the AI distrusts (no friendship, pacts).
pub const DISTRUST_WARMONGER: i32 = 100;
/// Warmonger memory above which the AI denounces.
pub const DENOUNCE_WARMONGER: i32 = 250;
/// No friendly deal with a former enemy for this many rounds after the
/// peace treaty ends.
pub const RECENT_WAR_ROUNDS: u32 = 30;

fn relation<'o>(obs: &'o AIObservation<'_>, other: PlayerId) -> Option<&'o Relation> {
    obs.relation(other)
}

pub(crate) fn trusted(round: u32, r: &Relation) -> bool {
    let recent_war = r.peace_treaty_until > 0 && round < r.peace_treaty_until + RECENT_WAR_ROUNDS;
    !r.at_war
        && !recent_war
        && !r.they_denounced
        && !r.we_denounced
        && r.warmonger < DISTRUST_WARMONGER
}

/// Gold value of an item for the AI receiving it (`receiving`) or giving it.
pub(crate) fn item_value(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    other: PlayerId,
    item: DealItem,
    receiving: bool,
) -> i64 {
    let r = relation(obs, other);
    let trust = r.is_some_and(|r| trusted(obs.round, r));
    let sign = if receiving { 1 } else { -1 };
    match item {
        DealItem::Gold(g) => sign * i64::from(g),
        DealItem::GoldPerTurn(g) => sign * i64::from(g) * 20,
        DealItem::Resource { resource, amount } => {
            let luxury = rules
                .resources
                .get(resource.0 as usize)
                .is_some_and(|d| d.class == Some(crate::rules::content::ResourceClass::Luxury));
            sign * i64::from(amount) * if luxury { 150 } else { 80 }
        }
        DealItem::City(_) => sign * 600,
        DealItem::Embassy => {
            if receiving {
                30
            } else if trust {
                -10
            } else {
                -40
            }
        }
        DealItem::OpenBorders => {
            if receiving {
                40
            } else if trust {
                -30
            } else {
                -200
            }
        }
        DealItem::DefensivePact | DealItem::Friendship => {
            if trust {
                50
            } else {
                -1000
            }
        }
        DealItem::ResearchAgreement => {
            if trust && obs.economy.treasury >= research_cost(obs, rules) {
                100
            } else {
                -1000
            }
        }
        DealItem::Peace => peace_value(obs, other),
        DealItem::ThirdPartyWar(_) => -1000,
        DealItem::ThirdPartyPeace(_) => 0,
    }
}

/// The AI takes peace after `ACCEPT_PEACE_AFTER_ROUNDS` of war.
fn peace_value(obs: &AIObservation<'_>, other: PlayerId) -> i64 {
    match relation(obs, other) {
        Some(r) if r.at_war && obs.round >= r.war_since + ACCEPT_PEACE_AFTER_ROUNDS => 100,
        _ => -1000,
    }
}

/// The research agreement cost of the AI's own era.
fn research_cost(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> u32 {
    let era = obs
        .economy
        .techs
        .iter()
        .filter_map(|t| rules.technologies.get(t.0 as usize)?.content_era)
        .filter_map(|e| rules.eras.get(e.0 as usize))
        .max_by_key(|e| e.index);
    era.map(|e| e.research_agreement_cost).unwrap_or(100)
}

/// Accept an offer when its value for the AI is not negative.
pub fn accepts(obs: &AIObservation<'_>, rules: &CompiledRuleset, offer: &DealOffer) -> bool {
    let other = offer.from;
    let mut value = 0i64;
    for (giver, item) in &offer.items {
        let receiving = *giver != obs.player || item.is_mutual();
        value += item_value(obs, rules, other, *item, receiving);
    }
    value >= 0
}

/// Answers to every offer addressed to the AI.
pub fn answer_offers(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    obs.offers_to_us
        .iter()
        .map(|offer| Command::RespondDeal {
            player: obs.player,
            offer: offer.id,
            accept: accepts(obs, rules, offer),
        })
        .collect()
}

/// Proposals to met civilizations: embassy exchange, then friendship after
/// `FRIENDSHIP_MIN_CONTACT_ROUNDS` rounds of contact, research agreements and pacts with
/// friends, mutual open borders with friends, peace after long wars. One
/// offer per partner per call.
pub fn proposals(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
    let me = obs.player;
    let mut out = Vec::new();
    for r in &obs.relations {
        if r.kind != PlayerKind::Major {
            continue;
        }
        // Its offer is answered this turn (`answer_offers`, before the
        // proposals): what it settles is not in this observation yet, so a
        // proposal of the same items would be refused (an embassy that
        // exists, friends already).
        if obs.offers_to_us.iter().any(|o| o.from == r.other) {
            continue;
        }
        // An offer of this partner is answered in the same turn: the
        // observation would be stale for a proposal (the core refused
        // "embassy exists" when both sides proposed the same exchange).
        if obs.offers_to_us.iter().any(|o| o.from == r.other) {
            continue;
        }
        let items: Vec<(PlayerId, DealItem)> = if r.at_war {
            if obs.round >= r.war_since + 2 * ACCEPT_PEACE_AFTER_ROUNDS {
                vec![(me, DealItem::Peace)]
            } else {
                continue;
            }
        } else if !trusted(obs.round, r) {
            continue;
        } else if !r.our_embassy || !r.their_embassy {
            let mut v = Vec::new();
            if !r.their_embassy {
                v.push((me, DealItem::Embassy));
            }
            if !r.our_embassy {
                v.push((r.other, DealItem::Embassy));
            }
            v
        } else if !r.friends {
            let met = r.contact.met_round.unwrap_or(obs.round);
            if obs.round < met + FRIENDSHIP_MIN_CONTACT_ROUNDS {
                continue;
            }
            vec![(me, DealItem::Friendship)]
        } else if !r.research_agreement && obs.economy.treasury >= research_cost(obs, rules) + 50 {
            vec![(me, DealItem::ResearchAgreement)]
        } else if !r.we_grant_entry || !r.they_grant_entry {
            let mut v = Vec::new();
            if !r.we_grant_entry {
                v.push((me, DealItem::OpenBorders));
            }
            if !r.they_grant_entry {
                v.push((r.other, DealItem::OpenBorders));
            }
            v
        } else if !r.defensive_pact {
            vec![(me, DealItem::DefensivePact)]
        } else {
            continue;
        };
        let possible = items.iter().all(|(giver, item)| {
            let receiver = if *giver == me { r.other } else { me };
            obs.view
                .deal_item_problem(*giver, receiver, *item)
                .is_none()
        });
        if possible {
            out.push(Command::ProposeDeal {
                from: me,
                to: r.other,
                items,
            });
        }
    }
    out
}

/// Luxury swaps of an AI that prices happiness (`PriceModel::Uses`): to
/// each met civilization at peace with no other proposal this round, one
/// copy of a luxury kind it can give and the empire lacks, for one spare
/// copy (a second copy adds no happiness) of a kind it does not list.
/// The kind gained adds its happiness for the deal's rounds at no cost, so
/// every swap is worth making; kinds in table order.
pub fn luxury_swaps(obs: &AIObservation<'_>, rules: &CompiledRuleset, others: &[Command]) -> Vec<Command> {
    let me = obs.player;
    let ours = &obs.economy.resources;
    let luxury = |r: &crate::ids::ResourceId| rules.resources.get(r.0 as usize).is_some_and(|d| d.class == Some(crate::rules::content::ResourceClass::Luxury));
    let mut spares: Vec<crate::ids::ResourceId> = ours.known().filter(luxury).filter(|r| ours.tradeable(*r) >= 2).collect();
    let proposed = |other: PlayerId| others.iter().any(|c| matches!(c, Command::ProposeDeal { to, .. } if *to == other));
    let mut out = Vec::new();
    for r in &obs.relations {
        let busy = r.at_war || proposed(r.other) || obs.offers_to_us.iter().any(|o| o.from == r.other);
        if r.kind != PlayerKind::Major || busy || spares.is_empty() {
            continue;
        }
        let theirs = obs.view.tradeable_luxuries(r.other);
        let Some(&(wanted, _)) = theirs.iter().find(|(k, _)| ours.available(*k) <= 0) else { continue };
        let Some(at) = spares.iter().position(|k| theirs.iter().all(|(t, _)| t != k)) else { continue };
        let given = spares.remove(at);
        let one = |resource| DealItem::Resource { resource, amount: 1 };
        let items = vec![(r.other, one(wanted)), (me, one(given))];
        if obs.view.offer_problem(r.other, &items).is_none() {
            out.push(Command::ProposeDeal { from: me, to: r.other, items });
        }
    }
    out
}

/// Denounce civilizations whose warmonger memory passed DENOUNCE_WARMONGER.
pub fn denunciations(obs: &AIObservation<'_>) -> Vec<Command> {
    obs.relations
        .iter()
        .filter(|r| r.kind == PlayerKind::Major && !r.we_denounced && !r.friends)
        .filter(|r| r.warmonger >= DENOUNCE_WARMONGER)
        .map(|r| Command::Denounce {
            player: obs.player,
            target: r.other,
        })
        .collect()
}

/// Rounds between two rounds of proposals of one player.
pub const PROPOSAL_INTERVAL: u32 = 5;

/// The diplomacy system's advisor (`ai::system`): answers every offer at
/// the start of the turn; every PROPOSAL_INTERVAL rounds proposes deals and
/// denounces warmongers. How wars with civilizations end is `peace`'s:
/// alone, the strategy planner's (`planner_strategy`, `operations`), and no
/// peace is proposed here; by deal, every answer and every peace offer is
/// valued (`ai::peace`).
#[derive(Debug, Clone, Copy, Default)]
pub struct DiplomacyAdvisor {
    pub peace: PeaceMaking,
}

impl crate::ai::system::SystemAdvisor for DiplomacyAdvisor {
    fn start_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        let obs = input.observation;
        if obs.view.own_kind() != PlayerKind::Major {
            return Vec::new();
        }
        let answers = match self.peace {
            PeaceMaking::Alone => answer_offers(obs, input.rules),
            PeaceMaking::ByDeal => crate::ai::peace::answer_offers(input),
        };
        let mut out = crate::ai::combat::proposals(40, answers);
        let my_round = (obs.round + u32::from(obs.player.0)) % PROPOSAL_INTERVAL == 0;
        if my_round {
            let deals: Vec<Command> = proposals(obs, input.rules)
                .into_iter()
                .filter(|c| !proposes_peace(c))
                .collect();
            let priced = input.brief.prices.is_some_and(|p| p.model == crate::ai::value::PriceModel::Uses);
            let swaps = if priced { luxury_swaps(obs, input.rules, &deals) } else { Vec::new() };
            let asked: Vec<PlayerId> = deals.iter().chain(&swaps).filter_map(partner).collect();
            out.extend(crate::ai::combat::proposals(5, deals));
            out.extend(crate::ai::combat::proposals(5, swaps));
            if self.peace == PeaceMaking::ByDeal {
                out.extend(crate::ai::combat::proposals(5, crate::ai::peace::embassy_offers(input, &asked)));
                out.extend(crate::ai::combat::proposals(5, crate::ai::peace::peace_offers(input)));
            }
            out.extend(crate::ai::combat::proposals(5, denunciations(obs)));
        }
        out
    }
}

/// The receiver of a proposed deal.
fn partner(command: &Command) -> Option<PlayerId> {
    match command {
        Command::ProposeDeal { to, .. } => Some(*to),
        _ => None,
    }
}

fn proposes_peace(command: &Command) -> bool {
    match command {
        Command::ProposeDeal { items, .. } => items.iter().any(|(_, i)| *i == DealItem::Peace),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::ACCEPT_PEACE_AFTER_ROUNDS;
    use crate::ai::planner_strategy::MIN_WAR_ROUNDS;

    /// Two thresholds with two jobs (classic/docs/ai.md): the diplomacy advisor takes
    /// a peace offered after 5 rounds of war, the strategy asks for a peace of
    /// its own only after 15. A peace offered in between is a deal, not a
    /// flip; the strategy never gives up a war before the acceptance opens.
    #[test]
    fn peace_is_accepted_before_the_strategy_asks_for_it() {
        assert!(ACCEPT_PEACE_AFTER_ROUNDS < MIN_WAR_ROUNDS, "{ACCEPT_PEACE_AFTER_ROUNDS} against {MIN_WAR_ROUNDS}");
    }
}
