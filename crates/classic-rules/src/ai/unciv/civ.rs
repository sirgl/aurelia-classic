// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0:
// core/src/com/unciv/logic/automation/civilization/NextTurnAutomation.kt (`chooseTechToResearch`,
// `adoptPolicy`, `chooseGreatPerson`, `protectCityStates`, `bullyCityStates`,
// `valueCityStateAlliance`), logic/civilization/managers/PolicyManager.kt (`priorityMap`),
// logic/battle/Battle.kt (`automateCityConquer`, the AI's decision of a captured city) and the
// branch and policy priorities and AI weights of android/assets/jsons/Civ V - Gods & Kings/Policies.json.

//! The civilization's own decisions of Unciv's turn: research, policies, free
//! great people, captured cities, city-states.

use crate::ai::planner::Decision;
use crate::ai::state::AiState;
use crate::ai::trace::{Actor, Reason};
use crate::command::{CityDecision, Command};
use crate::ids::{PolicyId, TechId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::PolicyBranchId;
use crate::rules::CompiledRuleset;

use super::next_turn::draw;
use super::personality::{UncivPersonality, Value, Victory};

fn act(command: Command, reason: Reason) -> Decision {
    Decision::act(Actor::Player, reason, command)
}

/// `chooseTechToResearch`: a free technology from the most expensive group the
/// player may take; with nothing queued, a technology drawn among the cheapest
/// group and the next one (Unciv's AI weights are all 1 for technologies).
pub fn research(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Decision> {
    let mut out = Vec::new();
    let player = obs.player;
    let grouped = |techs: &[TechId]| -> Vec<Vec<TechId>> {
        let mut costs: Vec<u32> = techs.iter().map(|t| rules.technologies[t.0 as usize].cost).collect();
        costs.sort_unstable();
        costs.dedup();
        costs.iter().map(|c| techs.iter().copied().filter(|t| rules.technologies[t.0 as usize].cost == *c).collect()).collect()
    };
    if !obs.economy.free_technology_options.is_empty() {
        let groups = grouped(&obs.economy.free_technology_options);
        if let Some(most) = groups.last() {
            let tech = most[draw(obs.round, u64::from(player.0), 0x6674, most.len())];
            out.push(act(Command::ChooseFreeTechnology { player, tech }, Reason::Research));
        }
    }
    if obs.economy.research_queue.is_empty() {
        let researchable: Vec<TechId> = (0..rules.technologies.len())
            .map(|i| TechId(i as u16))
            .filter(|t| obs.view.accepts(&Command::ChooseTechnology { player, technology: *t }))
            .collect();
        let groups = grouped(&researchable);
        if let Some(cheapest) = groups.first() {
            let pool: Vec<TechId> = if cheapest.len() == 1 || groups.len() == 1 {
                cheapest.clone()
            } else {
                cheapest.iter().chain(groups[1].iter()).copied().collect()
            };
            let technology = pool[draw(obs.round, u64::from(player.0), 0x7274, pool.len())];
            out.push(act(Command::ChooseTechnology { player, technology }, Reason::Research));
        }
    }
    out
}

/// Unciv's branch priorities by its preferred victory (Policies.json), by
/// branch name: [Neutral, Cultural, Diplomatic, Domination, Scientific].
fn branch_victory_priority(branch: &str, victory: Victory) -> i32 {
    let row: [i32; 5] = match branch {
        "Tradition" | "Honor" | "Commerce" => [5, 5, 5, 5, 5],
        "Liberty" | "Rationalism" => [10, 10, 10, 10, 10],
        "Piety" => [0, 0, 0, 0, 0],
        "Patronage" => [5, 5, 10, 5, 5],
        "Freedom" => [5, 10, 5, 5, 10],
        "Autocracy" => [5, 5, 5, 10, 5],
        "Order" => [10, 5, 10, 5, 5],
        _ => [0, 0, 0, 0, 0],
    };
    row[match victory {
        Victory::Neutral => 0,
        Victory::Cultural => 1,
        Victory::Diplomatic => 2,
        Victory::Domination => 3,
        Victory::Scientific => 4,
    }]
}

/// Unciv's AI weight of a policy of Gods & Kings, by name.
fn policy_weight(name: &str) -> f64 {
    match name {
        "Oligarchy" | "Citizenship" | "Meritocracy" | "Scientific Revolution" | "Constitution" | "Universal Suffrage" | "United Front" => 0.0,
        "Humanism" => 0.5,
        "Populism" => 2.0,
        _ => 1.0,
    }
}

/// `adoptPolicy`: finish an incomplete branch of the highest priority, or
/// open a new branch of a higher one; within the branch the policy is drawn
/// by Unciv's AI weights; the branch closest to completion first. An
/// ideology (a Brave New World branch Unciv's AI does not know) is the
/// classic AI's choice (`ai::policies`).
pub fn adopt_policy(obs: &AIObservation<'_>, rules: &CompiledRuleset, state: &AiState, personality: &UncivPersonality) -> Option<Decision> {
    use crate::classic::content::PolicyBranchKind;
    let culture = &obs.culture;
    let player = obs.player;
    let branches = &rules.module.policy_branches;
    let policies = &rules.module.policies;
    let is_ideology = |b: PolicyBranchId| branches.get(b.0 as usize).is_some_and(|d| d.kind == PolicyBranchKind::Ideology);
    // Brave New World: ideologies and their tenets stay the classic AI's.
    let tenet_open = culture.adoptable.iter().any(|p| policies[p.0 as usize].branch.is_some_and(is_ideology));
    if !culture.ideologies.is_empty() || tenet_open {
        return crate::ai::policies::policy_command(obs, rules, state).map(|c| act(c, Reason::SystemAdvisor));
    }
    if culture.adoptable.is_empty() && culture.unlockable.is_empty() {
        return None;
    }
    let met_city_states = obs.city_states.len();
    let priority = |b: PolicyBranchId| -> i32 {
        let name = branches.get(b.0 as usize).map_or("", |d| d.name.as_str());
        let weight = if name == "Patronage" && met_city_states < 2 { 0 } else { 1 };
        (branch_victory_priority(name, personality.victory) + i32::from(personality.priority(name))) * weight
    };
    let adoptable_in = |b: PolicyBranchId| -> Vec<PolicyId> { culture.adoptable.iter().copied().filter(|p| policies[p.0 as usize].branch == Some(b) && Some(*p) != branches[b.0 as usize].finisher).collect() };
    let left_in = |b: PolicyBranchId| -> usize {
        policies.iter().enumerate().filter(|(i, p)| p.branch == Some(b) && !culture.policies.contains(&PolicyId(*i as u16))).count()
    };
    let incomplete: Vec<PolicyBranchId> = culture.branches.iter().copied().filter(|b| !is_ideology(*b) && !adoptable_in(*b).is_empty()).collect();
    let adoptable: Vec<PolicyBranchId> = culture.unlockable.iter().copied().filter(|b| !is_ideology(*b)).collect();
    if incomplete.is_empty() && adoptable.is_empty() {
        return None;
    }
    let max_incomplete = incomplete.iter().map(|b| priority(*b)).max();
    let max_adoptable = adoptable.iter().map(|b| priority(*b)).max();
    let (max_incomplete, max_adoptable) = match (max_incomplete, max_adoptable) {
        (Some(i), Some(a)) => (i, a),
        (Some(i), None) => (i, i - 1),
        (None, Some(a)) => (a - 1, a),
        (None, None) => return None,
    };
    let candidates: Vec<PolicyBranchId> = if max_adoptable <= max_incomplete {
        incomplete.iter().copied().filter(|b| priority(*b) == max_incomplete).collect()
    } else {
        adoptable.iter().copied().filter(|b| priority(*b) == max_adoptable).collect()
    };
    let least = candidates.iter().map(|b| left_in(*b)).min()?;
    let closest: Vec<PolicyBranchId> = candidates.into_iter().filter(|b| left_in(*b) == least).collect();
    let branch = closest[draw(obs.round, u64::from(player.0), 0x706f, closest.len())];
    if culture.unlockable.contains(&branch) {
        return Some(act(Command::UnlockPolicyBranch { player, branch }, Reason::SystemAdvisor));
    }
    let pool = adoptable_in(branch);
    let weights: Vec<f64> = pool.iter().map(|p| policy_weight(&policies[p.0 as usize].name)).collect();
    let policy = weighted(obs.round, u64::from(player.0), &pool, &weights)?;
    Some(act(Command::AdoptPolicy { player, policy }, Reason::SystemAdvisor))
}

/// `randomWeighted` with the deterministic draw; uniform when every weight is 0.
pub fn weighted<T: Copy>(round: u32, key: u64, pool: &[T], weights: &[f64]) -> Option<T> {
    if pool.is_empty() {
        return None;
    }
    let total: f64 = weights.iter().sum();
    if total <= 0.0 {
        return Some(pool[draw(round, key, 0x7577, pool.len())]);
    }
    let roll = draw(round, key, 0x7765, 1 << 20) as f64 / f64::from(1u32 << 20) * total;
    let mut acc = 0.0;
    for (item, w) in pool.iter().zip(weights) {
        acc += w;
        if roll < acc {
            return Some(*item);
        }
    }
    pool.last().copied()
}

/// `chooseGreatPerson`: a great scientist when the player may take one, else a
/// random one.
pub fn choose_great_person(obs: &AIObservation<'_>) -> Option<Decision> {
    use crate::classic::effects::GreatPersonKind;
    let options = &obs.economy.free_great_person_options;
    if options.is_empty() {
        return None;
    }
    let kind = options.iter().copied().find(|k| *k == GreatPersonKind::Scientist).unwrap_or(options[draw(obs.round, u64::from(obs.player.0), 0x6770, options.len())]);
    Some(act(Command::ChooseFreeGreatPerson { player: obs.player, kind }, Reason::SystemAdvisor))
}

/// `Battle.automateCityConquer` for the AI: a city it held before (Unciv: its
/// own original capital) is annexed back; otherwise a puppet, razed when it
/// keeps fewer than 4 citizens and was never ours. (Unciv also liberates a
/// city-state's city it values; the observation does not tell whose city it
/// was, so that case is not played.)
pub fn conquest_decisions(obs: &AIObservation<'_>, state: &AiState) -> Vec<Decision> {
    let founded = super::founded_tiles(state);
    let mut out = Vec::new();
    for city in obs.own_cities.iter().filter(|c| c.status == crate::world::city::CityStatus::AwaitingDecision) {
        let ours_before = founded.contains(&city.tile);
        let candidates: Vec<CityDecision> = if ours_before {
            vec![CityDecision::Annex, CityDecision::Puppet]
        } else if city.population < 4 {
            vec![CityDecision::Raze, CityDecision::Puppet]
        } else {
            vec![CityDecision::Puppet, CityDecision::Annex]
        };
        for decision in candidates {
            let command = Command::DecideCity { player: obs.player, city: city.handle, decision };
            if obs.view.accepts(&command) {
                out.push(act(command, Reason::SystemAdvisor));
                break;
            }
        }
    }
    out
}

/// `valueCityStateAlliance` from what the city-state screen shows: the
/// bonuses its kind gives an ally, our influence, its ally's.
pub fn value_city_state_alliance(obs: &AIObservation<'_>, info: &crate::knowledge::observation::CityStateInfo, personality: &UncivPersonality) -> i32 {
    use crate::rules::content::CityStateTrait;
    let p = |v: Value| personality.get(v) as i32 - 5;
    let mut value = 0;
    match info.minor_trait {
        Some(CityStateTrait::Cultured) => value += p(Value::Culture),
        Some(CityStateTrait::Religious) => value += p(Value::Faith),
        Some(CityStateTrait::Militaristic) => value += p(Value::Production),
        Some(CityStateTrait::Mercantile) => {
            let happiness = obs.economy.happiness.excess();
            if happiness < 10 {
                value += 10 - happiness;
            }
            value += p(Value::Happiness);
        }
        Some(CityStateTrait::Maritime) => value += 5 + p(Value::Food),
        _ => {}
    }
    let influence = info.influence;
    value += (influence / 10).min(10);
    if influence < 30 {
        value -= 5; // Consider bullying for cash.
    }
    if info.ally.is_some_and(|a| a != obs.player) {
        // A third civilization holds it: its influence is not shown; Unciv's 30 point margin is.
        value -= 0;
    }
    value
}

/// `protectCityStates` and `bullyCityStates`: pledge to protect every
/// city-state that accepts it; demand tribute from those below friendship at
/// peace whose alliance is worth nothing to us.
pub fn city_states(obs: &AIObservation<'_>, personality: &UncivPersonality) -> Vec<Decision> {
    let mut out = Vec::new();
    let player = obs.player;
    for info in &obs.city_states {
        let pledge = Command::PledgeProtection { player, city_state: info.player, pledge: true };
        if !info.pledged && obs.view.accepts(&pledge) {
            out.push(act(pledge, Reason::SystemAdvisor));
            continue;
        }
        if info.friends || info.at_war || value_city_state_alliance(obs, info, personality) > 0 || !info.can_bully_gold {
            continue;
        }
        let worker = Command::BullyWorker { player, city_state: info.player };
        let gold = Command::BullyGold { player, city_state: info.player };
        if obs.view.accepts(&worker) {
            out.push(act(worker, Reason::SystemAdvisor));
        } else if obs.view.accepts(&gold) {
            out.push(act(gold, Reason::SystemAdvisor));
        }
    }
    out
}
