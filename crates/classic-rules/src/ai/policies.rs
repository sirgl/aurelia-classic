//! Minimal policy AI (PLAN M6 "AI for every mechanic"): what to do with the
//! culture pool, decided from the observation only (`AIObservation::
//! culture` lists what the core allows now). Deterministic.
//!
//! Strategy (own, simple):
//! - an ideology as soon as one may be chosen: the one the rules name for
//!   a wide empire, else for war, else for peace (in the shipped ruleset: Order,
//!   Autocracy, Freedom);
//! - spend a free tenet or policy, or the pool when it covers the cost;
//! - finish the branch with the most adopted policies first; otherwise
//!   unlock the branch the situation values most (`ai::policy_branches`).
//!
//! One command per call: every adoption changes the cost and the options,
//! the driver observes again before the next decision.

use crate::ai::policy_branches::next_branch;
use crate::ai::state::AiState;
use crate::command::Command;
use crate::ids::PolicyId;
use crate::knowledge::observation::AIObservation;
use crate::rules::content::{PolicyBranchId, PolicyRole};
use crate::rules::roles::IdeologyCase;
use crate::rules::CompiledRuleset;

/// Cities from which the AI plays a wide empire (the ideology it picks).
pub const WIDE_EMPIRE_CITIES: usize = 4;

/// The next culture decision of the observed player, if any.
pub fn policy_command(obs: &AIObservation<'_>, rules: &CompiledRuleset, memory: &AiState) -> Option<Command> {
    let player = obs.player;
    if let Some(ideology) = choose_ideology(obs, rules) {
        return Some(Command::ChooseIdeology { player, ideology });
    }
    if let Some(policy) = best_policy(obs, rules) {
        return Some(Command::AdoptPolicy { player, policy });
    }
    let branch = next_branch(obs, rules, memory)?;
    Some(Command::UnlockPolicyBranch { player, branch })
}

/// The next culture decision of a player that prices its policies by what
/// they do (`ScienceLevers::policies`): the ideology whose tenets bring the
/// most (`PolicyMarket::ideology`, the rules' role order on a tie), then
/// the started branches and the branches to unlock on one scale, a step of
/// culture (`policy_branches::priced`): a started branch by what is left of
/// it per policy left (`rest_step`; a tenet by its own effects), a new one
/// by what it brings per step (`branch_step`). The best started branch
/// adopts its policy worth most now; the best new one is unlocked. On a tie
/// an adoption before a branch, then `best_policy`'s order and the rules'
/// order of the branches.
pub fn priced_policy_command(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    memory: &AiState,
    prices: &crate::ai::value::Prices,
    levers: crate::ai::science_levers::ScienceLevers,
) -> Option<Command> {
    let player = obs.player;
    if obs.culture.ideologies.is_empty() && obs.culture.adoptable.is_empty() && obs.culture.unlockable.is_empty() {
        return None;
    }
    let market = crate::ai::policy_branches::priced::PolicyMarket::new(obs, rules, memory, prices, levers);
    if !obs.culture.ideologies.is_empty() {
        // The rules' choice first, so it wins a tie.
        let first = choose_ideology(obs, rules);
        let mut ideologies = obs.culture.ideologies.clone();
        ideologies.sort_by_key(|(b, _)| (Some(*b) != first, b.0));
        let mut best: Option<(crate::ai::value::Centi, PolicyBranchId)> = None;
        for (branch, free) in ideologies {
            let value = market.ideology(branch, free);
            if best.is_none_or(|(b, _)| value > b) {
                best = Some((value, branch));
            }
        }
        return best.map(|(_, ideology)| Command::ChooseIdeology { player, ideology });
    }
    let first = best_policy(obs, rules);
    let mut best: Option<(crate::ai::value::Centi, Command)> = None;
    // `best_policy`'s choice first, so it wins a tie among adoptions.
    let adoptable: Vec<PolicyId> = first.into_iter().chain(obs.culture.adoptable.iter().copied().filter(|p| Some(*p) != first)).collect();
    let mut started: Vec<PolicyBranchId> = Vec::new();
    for policy in &adoptable {
        let branch = rules.module.policies.get(policy.0 as usize).and_then(|d| d.branch);
        match branch {
            Some(branch) if !started.contains(&branch) => started.push(branch),
            _ => {}
        }
    }
    for branch in started {
        let ideology = rules.module.policy_branches.get(branch.0 as usize).is_some_and(|b| b.kind == crate::classic::content::PolicyBranchKind::Ideology);
        // The branch's best policy now, its value the branch's rest (a
        // normal branch) or its own (a tenet).
        let mut pick: Option<(crate::ai::value::Centi, PolicyId)> = None;
        for policy in adoptable.iter().copied().filter(|p| rules.module.policies.get(p.0 as usize).and_then(|d| d.branch) == Some(branch)) {
            let own = market.policy(policy);
            if pick.is_none_or(|(b, _)| own > b) {
                pick = Some((own, policy));
            }
        }
        let Some((own, policy)) = pick else { continue };
        let value = if ideology { own } else { market.rest_step(branch) };
        if best.as_ref().is_none_or(|(b, _)| value > *b) {
            best = Some((value, Command::AdoptPolicy { player, policy }));
        }
    }
    let order = rules.roles.module.ai_branches();
    let rank = |branch: &PolicyBranchId| order.iter().position(|b| b == branch).unwrap_or(order.len());
    let mut branches = obs.culture.unlockable.clone();
    branches.sort_by_key(|b| (rank(b), b.0));
    for branch in branches {
        let value = market.branch_step(branch);
        if best.as_ref().is_none_or(|(b, _)| value > *b) {
            best = Some((value, Command::UnlockPolicyBranch { player, branch }));
        }
    }
    best.map(|(_, command)| command)
}

fn choose_ideology(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Option<PolicyBranchId> {
    if obs.culture.ideologies.is_empty() {
        return None;
    }
    let at_war = obs.relations.iter().any(|r| r.at_war);
    let case = if obs.own_cities.len() >= WIDE_EMPIRE_CITIES {
        IdeologyCase::Wide
    } else if at_war {
        IdeologyCase::AtWar
    } else {
        IdeologyCase::AtPeace
    };
    let wanted = rules.roles.module.ai_ideology(case);
    let preferred = obs.culture.ideologies.iter().find(|(b, _)| Some(*b) == wanted);
    // Most free tenets otherwise (first and second adopters).
    let fallback = obs
        .culture
        .ideologies
        .iter()
        .max_by_key(|(b, free)| (*free, std::cmp::Reverse(b.0)));
    preferred.or(fallback).map(|(b, _)| *b)
}

/// An adoptable policy of the branch with the most adopted policies (to
/// complete it), lowest grid row first; tenets lowest tier first.
fn best_policy(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Option<PolicyId> {
    let mut best: Option<(u32, u8, u8, PolicyId)> = None;
    for policy in &obs.culture.adoptable {
        let Some(def) = rules.module.policies.get(policy.0 as usize) else {
            continue;
        };
        let Some(branch) = def.branch else {
            continue;
        };
        let adopted = adopted_in(obs, rules, branch);
        let tier = match def.role {
            PolicyRole::Tenet(t) => t,
            _ => 0,
        };
        // Higher score first: more adopted in the branch, lower tier, lower row.
        let key = (adopted, u8::MAX - tier, u8::MAX - def.grid.1, *policy);
        let better = match best {
            None => true,
            Some(b) => (key.0, key.1, key.2) > (b.0, b.1, b.2),
        };
        if better {
            best = Some(key);
        }
    }
    best.map(|b| b.3)
}

fn adopted_in(obs: &AIObservation<'_>, rules: &CompiledRuleset, branch: PolicyBranchId) -> u32 {
    let mut count = 0;
    for policy in &obs.culture.policies {
        if rules.module.policies.get(policy.0 as usize).and_then(|d| d.branch) == Some(branch) {
            count += 1;
        }
    }
    count
}

/// The policies advisor of the AI driver (`ai::system`): the policy
/// command of the turn, after the units moved.
#[derive(Debug, Clone, Copy, Default)]
pub struct PolicyAdvisor;

impl crate::ai::system::SystemAdvisor for PolicyAdvisor {
    fn end_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        if let (Some(plan), Some(prices)) = (input.brief.strategy.as_deref(), input.brief.prices.as_ref()) {
            // The strategy net's policy head (`ai::strategy_net::heads::policy`).
            let command = crate::ai::strategy_net::heads::policy(input.observation, input.rules, prices, plan);
            return command.map(|command| crate::ai::system::Proposal { priority: 20, command }).into_iter().collect();
        }
        let priced = input.brief.prices.as_ref().filter(|_| input.brief.levers.policies);
        let tall = input.brief.prices.as_ref().filter(|_| input.brief.tall.plays() || input.brief.tall.book_policies());
        let command = match (tall, priced) {
            (Some(prices), _) if input.brief.tall.book_policies() => crate::ai::tall::book_policy_command(input.observation, input.rules, input.state, prices, input.brief.tall.plan()),
            (Some(prices), _) => crate::ai::tall::science_policy_command(input.observation, input.rules, input.state, prices),
            (None, Some(prices)) => priced_policy_command(input.observation, input.rules, input.state, prices, input.brief.levers),
            (None, None) => policy_command(input.observation, input.rules, input.state),
        };
        command
            .map(|command| crate::ai::system::Proposal {
                priority: 20,
                command,
            })
            .into_iter()
            .collect()
    }
}

/// The culture and faith requirements of a building as the player sees
/// them (`derived::culture::building_culture_status` over the observation):
/// its policy branch is unlocked, a holy-city building goes into a holy city.
pub fn culture_allows(
    obs: &AIObservation<'_>,
    rules: &CompiledRuleset,
    city: crate::ids::CityHandle,
    kind: crate::ids::BuildingKindId,
) -> bool {
    let Some(def) = rules.buildings.get(kind.0 as usize) else {
        return false;
    };
    let branch_ok = def
        .module.requires_policy_branch
        .is_none_or(|b| obs.culture.branches.contains(&b));
    let holy_ok = !def.module.requires_holy_city
        || obs
            .faith
            .overview
            .religions
            .iter()
            .any(|r| r.holy_city == city);
    branch_ok && holy_ok
}
