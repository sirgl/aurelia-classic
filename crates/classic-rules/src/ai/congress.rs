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
use crate::knowledge::observation::{AIObservation, ProjectInfo};
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
    vote_orders_for(obs, false)
}

/// `vote_orders`; with `guard` (`Variant::VoteGuard`) the host election goes to the strongest
/// nation but the front-runner when the front-runner could win the World Leader vote
/// (`victory_watch::host_choice`), and it takes all the delegates a second resolution on the table
/// would take (the plain AI splits them over the proposals, so a proposal beside the election halves
/// its vote).
pub fn vote_orders_for(obs: &AIObservation<'_>, guard: bool) -> Vec<Command> {
    let Some(info) = &obs.congress else {
        return Vec::new();
    };
    if !info.in_session || info.delegates_left == 0 || info.proposals.is_empty() {
        return Vec::new();
    }
    let host = guard.then(|| crate::ai::victory_watch::host_choice(obs)).flatten();
    if let (Some(host), Some(i)) = (host, info.proposals.iter().position(|p| p.kind == ResolutionKind::ChangeHost)) {
        if crate::ai::strategy_net::trace::enabled() {
            crate::ai::strategy_net::trace::push(format!("r{} p{} VOTE host denied: {} delegates to p{}", obs.round, obs.player.0, info.delegates_left, host.0));
        }
        // All the delegates on the host election, the rest abstain.
        return vec![Command::CastVotes { player: obs.player, proposal: i as u8, choice: u32::from(host.0), delegates: info.delegates_left }];
    }
    let shares = shares(&info.proposals, info.delegates_left);
    let mut out = Vec::new();
    for (i, (proposal, delegates)) in info.proposals.iter().zip(shares).enumerate() {
        if delegates == 0 {
            continue;
        }
        let choice = if proposal.kind.elects() {
            if proposal.kind == ResolutionKind::DiplomaticVictory {
                crate::ai::strategy_net::trace::leader_vote(obs, delegates, info.delegates_left, info.needed, info.proposals.len());
            }
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

/// Delegates per proposal, in the order of `proposals`: all of them on the World Leader vote when
/// it is on the table (it needs the whole seat, a share of it cannot win), else an even split in
/// which the remainder goes to the first proposals, so that no delegate is left uncast and, with
/// fewer delegates than proposals, the first ones get one each.
fn shares(proposals: &[Proposal], delegates_left: u16) -> Vec<u16> {
    if let Some(vote) = proposals.iter().position(|p| p.kind == ResolutionKind::DiplomaticVictory) {
        let mut shares = vec![0; proposals.len()];
        shares[vote] = delegates_left;
        return shares;
    }
    let count = proposals.len() as u16;
    let (each, rest) = (delegates_left / count, delegates_left % count);
    (0..count).map(|i| each + u16::from(i < rest)).collect()
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
        crate::ai::strategy_net::trace::watch(obs);
        let weights = Weights::of(input.state.strategy.current);
        let mut out = crate::ai::combat::proposals(8, vote_orders_for(obs, input.brief.vote_guard));
        out.extend(crate::ai::combat::proposals(6, proposal_orders(obs, input.rules, &weights)));
        out.extend(crate::ai::combat::proposals(5, project_orders(obs)));
        out
    }
}
