//! Minimal World Congress decisions over `AIObservation` (M4 track C):
//! one proposal when the AI may propose, its delegates in an open session
//! — elections for itself, yes to what helps it and no to the rest — and
//! its share of the World Congress projects: its most productive city
//! works on the project (an empty queue with the project's process) until
//! the AI contributed a full share (reward tier 2). Reads
//! `AIObservation::congress` (the Congress screen).

use crate::ai::planner_value::{league_project_value, Weights};
use crate::command::Command;
use crate::ids::CityHandle;
use crate::knowledge::observation::{AIObservation, CongressInfo, ProjectInfo};
use crate::rules::CompiledRuleset;
use crate::systems::congress::{NO, YES};
use crate::world::city_economy::Process;
use crate::world::congress::{Proposal, ResolutionKind};

/// What the AI proposes: the most valuable World Congress project it may
/// start, else Scholars in Residence while behind in technologies, else
/// Cultural Heritage Sites, else Standing Army Tax against bigger armies
/// (first one not in force).
pub fn proposal_orders(obs: &AIObservation<'_>, rules: &CompiledRuleset, w: &Weights) -> Vec<Command> {
    let Some(info) = &obs.congress else {
        return Vec::new();
    };
    if !info.can_propose || info.in_session {
        return Vec::new();
    }
    let cities = obs.own_cities.len() as i32;
    let project = info
        .proposable_projects
        .iter()
        .filter(|p| !info.proposals.iter().any(|q| q.kind == ResolutionKind::InternationalProject && q.choice == u32::from(p.0)))
        .max_by_key(|p| (league_project_value(rules, **p, w, cities), std::cmp::Reverse(p.0)));
    if let Some(project) = project {
        return vec![Command::ProposeResolution {
            player: obs.player,
            kind: ResolutionKind::InternationalProject,
            choice: u32::from(project.0),
            repeal: false,
        }];
    }
    let preferred = [
        ResolutionKind::ScholarsInResidence,
        ResolutionKind::CulturalHeritageSites,
        ResolutionKind::StandingArmyTax,
    ];
    let taken = |k: ResolutionKind| {
        info.active.iter().any(|a| a.kind == k) || info.proposals.iter().any(|p| p.kind == k)
    };
    preferred
        .into_iter()
        .find(|k| !taken(*k))
        .map(|kind| Command::ProposeResolution {
            player: obs.player,
            kind,
            choice: 0,
            repeal: false,
        })
        .into_iter()
        .collect()
}

/// Would the resolution help the AI (yes) or hurt it (no)?
fn supports(obs: &AIObservation<'_>, proposal: &Proposal) -> bool {
    if proposal.proposer == Some(obs.player) {
        return true;
    }
    let favourable = match proposal.kind {
        ResolutionKind::ScholarsInResidence | ResolutionKind::CulturalHeritageSites => true,
        ResolutionKind::NuclearNonProliferation => true,
        ResolutionKind::PlayerEmbargo => proposal.choice != u32::from(obs.player.0),
        ResolutionKind::StandingArmyTax => obs.own_units.len() < 8,
        // Every member may earn the rewards by contributing.
        ResolutionKind::InternationalProject => true,
        _ => false,
    };
    favourable != proposal.repeal
}

/// All delegates on the open session: elections for itself, yes/no by
/// `supports`, split evenly over the proposals.
pub fn vote_orders(obs: &AIObservation<'_>) -> Vec<Command> {
    let Some(info) = &obs.congress else {
        return Vec::new();
    };
    if !info.in_session || info.delegates_left == 0 || info.proposals.is_empty() {
        return Vec::new();
    }
    let share = split(info);
    let mut left = info.delegates_left;
    let mut out = Vec::new();
    for (i, proposal) in info.proposals.iter().enumerate() {
        let delegates = share.min(left);
        if delegates == 0 {
            break;
        }
        left -= delegates;
        let choice = if proposal.kind.elects() {
            u32::from(obs.player.0)
        } else if supports(obs, proposal) {
            YES
        } else {
            NO
        };
        out.push(Command::CastVotes {
            player: obs.player,
            proposal: i as u8,
            choice,
            delegates,
        });
    }
    out
}

/// Delegates per proposal: all on an election when there is one (the
/// World Leader matters most), else an even split.
fn split(info: &CongressInfo) -> u16 {
    if info.proposals.iter().any(|p| p.kind.elects()) && info.proposals.len() == 1 {
        return info.delegates_left;
    }
    (info.delegates_left / info.proposals.len() as u16).max(1)
}

/// The project still short of our full share (the most advanced first).
pub fn project_to_contribute(obs: &AIObservation<'_>) -> Option<ProjectInfo> {
    let info = obs.congress.as_ref()?;
    info.projects
        .iter()
        .filter(|p| p.ours < p.share)
        .max_by_key(|p| (p.total, std::cmp::Reverse(p.project.0)))
        .copied()
}

/// The city that works on the project: the most productive one.
pub fn contributing_city(obs: &AIObservation<'_>) -> Option<CityHandle> {
    project_to_contribute(obs)?;
    obs.own_cities
        .iter()
        .max_by_key(|c| (c.income.production, std::cmp::Reverse(c.id)))
        .map(|c| c.handle)
}

/// The contributing city takes the project's process (its queue runs
/// empty: `planner_cities` adds nothing to it).
pub fn project_orders(obs: &AIObservation<'_>) -> Vec<Command> {
    let (Some(project), Some(city)) = (project_to_contribute(obs), contributing_city(obs)) else {
        return Vec::new();
    };
    let process = Some(Process::Contribute(project.project));
    let current = obs.own_cities.iter().find(|c| c.handle == city).and_then(|c| c.process);
    if current == process {
        return Vec::new();
    }
    vec![Command::SetProcess {
        player: obs.player,
        city,
        process,
    }]
}

/// The congress system's advisor (`ai::system`).
#[derive(Debug, Clone, Copy, Default)]
pub struct CongressAdvisor;

impl crate::ai::system::SystemAdvisor for CongressAdvisor {
    fn start_of_turn(
        &self,
        input: &crate::ai::system::AdvisorInput<'_, '_>,
    ) -> Vec<crate::ai::system::Proposal> {
        let obs = input.observation;
        let weights = Weights::of(input.state.strategy.current);
        let mut out = crate::ai::combat::proposals(8, vote_orders(obs));
        out.extend(crate::ai::combat::proposals(6, proposal_orders(obs, input.rules, &weights)));
        out.extend(crate::ai::combat::proposals(5, project_orders(obs)));
        out
    }
}
