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
        policy_command(input.observation, input.rules, input.state)
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
