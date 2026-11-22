//! Minimal World Congress decisions over `AIObservation` (M4 track C):
//! one proposal when the AI may propose, its delegates in an open session
//! — elections for itself, yes to what helps it and no to the rest — and
//! its share of the World Congress projects: its most productive city
//! works on the project (an empty queue with the project's process) until
//! the AI contributed a full share (reward tier 2). Reads
//! `AIObservation::congress` (the Congress screen).

use crate::ai::net2::plan::{Ballot, Embargo, Net2Plan};
use crate::ai::planner_value::{league_project_value, Weights};
use crate::command::Command;
use crate::ids::{CityHandle, PlayerId};
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
    proposal_orders_with(obs, rules, w, None)
}

/// `proposal_orders`, read by a `net2` plan (design §4.6): the embargo on the rival the plan names
/// (`Net2Plan::embargo`, the runaway) comes before every other proposal, once, while it is neither in force nor
/// proposed and the rival is a nation we have met. None plays as today.
pub fn proposal_orders_with(obs: &AIObservation<'_>, rules: &CompiledRuleset, w: &Weights, net2: Option<&Net2Plan>) -> Vec<Command> {
    let Some(info) = &obs.congress else {
        return Vec::new();
    };
    if !info.can_propose || info.in_session {
        return Vec::new();
    }
    // The timing is the executor's: the rules let a seat propose only before a session (`can_propose`, `!in_session`).
    if let Some(Embargo::On(rival)) = net2.map(|p| p.embargo) {
        let known = obs.relations.iter().any(|r| r.other == rival && r.kind == crate::world::player::PlayerKind::Major);
        let named = |kind: ResolutionKind, choice: u32| kind == ResolutionKind::PlayerEmbargo && choice == u32::from(rival.0);
        let taken = info.active.iter().any(|a| named(a.kind, a.choice)) || info.proposals.iter().any(|p| named(p.kind, p.choice));
        if known && !taken && rival != obs.player {
            return vec![Command::ProposeResolution { player: obs.player, kind: ResolutionKind::PlayerEmbargo, choice: u32::from(rival.0), repeal: false }];
        }
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

/// What the seat does with a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stance {
    Yes,
    No,
    /// No delegates on it (a resolution that names nothing the seat gains or loses by).
    Abstain,
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

/// `supports`, read by a `net2` plan: what the resolution NAMES decides (A3 of the all-problems doc: the plain ballot
/// said yes to every embargo on another nation, even a trade partner, and no to every luxury ban):
/// - an embargo on the nation we are named for: no; on the nation the plan embargoes or denies help: yes; on any other: abstain
///   (we know nothing that it costs or pays us);
/// - a luxury ban: no while we hold a copy of the luxury; else abstain;
/// - the rest as `supports`.
/// A repeal is the reverse of its resolution. None plays as `supports`.
fn stance(obs: &AIObservation<'_>, proposal: &Proposal, net2: Option<&Net2Plan>) -> Stance {
    let yes_no = |yes: bool| if yes { Stance::Yes } else { Stance::No };
    let Some(plan) = net2 else {
        return yes_no(supports(obs, proposal));
    };
    if proposal.proposer == Some(obs.player) {
        return Stance::Yes;
    }
    let named = |favourable: bool| yes_no(favourable != proposal.repeal);
    match proposal.kind {
        ResolutionKind::PlayerEmbargo => {
            let target = PlayerId(proposal.choice as u8);
            if target == obs.player {
                named(false)
            } else if matches!(plan.embargo, Embargo::On(r) if r == target) || plan.denies_help(target) {
                named(true)
            } else {
                Stance::Abstain
            }
        }
        ResolutionKind::BanLuxury => {
            let held = obs.economy.resources.total(crate::ids::ResourceId(proposal.choice as u16)) > 0;
            if held {
                named(false)
            } else {
                Stance::Abstain
            }
        }
        _ => yes_no(supports(obs, proposal)),
    }
}

/// The ballot of the World Leader vote under a `net2` plan. Whether to block a rival is the net's decision
/// (`Net2Plan::ballot`, `weights::RIVAL` `block`: a rival whose own delegates reach the votes needed); this executes it
/// from the delegates every nation shows on the Congress screen:
/// - we hold the threshold ourselves: vote for ourselves;
/// - `Ballot::Block(threat)`: all delegates to the coalition candidate `c*`, the strongest candidate but the threat (ties
///   to the lowest id; we may be `c*`), so that a tie or a higher candidate stops it. Never so that our ballot elects
///   `c*`: when `c*` and our delegates together would reach the threshold and pass the threat, we vote for ourselves
///   (`pool_elects`);
/// - `Ballot::Own`: for ourselves, as today.
pub fn leader_choice(obs: &AIObservation<'_>, info: &crate::knowledge::observation::CongressInfo, ballot: Ballot) -> PlayerId {
    let me = obs.player;
    let needed = i32::from(info.needed);
    let delegates = |p: PlayerId| info.delegates.iter().find(|(q, _)| *q == p).map_or(0, |(_, n)| i32::from(*n));
    if needed == 0 || delegates(me) >= needed {
        return me;
    }
    let Ballot::Block(threat) = ballot else {
        return me;
    };
    let mut table: Vec<(PlayerId, i32)> = info.delegates.iter().map(|(p, n)| (*p, i32::from(*n))).collect();
    if !table.iter().any(|(p, _)| *p == me) {
        table.push((me, 0));
    }
    // The strongest first; the lowest id on a tie.
    table.sort_by_key(|(p, n)| (std::cmp::Reverse(*n), p.0));
    let coalition = table.iter().find(|(p, _)| *p != threat).map_or(me, |(p, _)| *p);
    if coalition != me && pool_elects(delegates(coalition) + delegates(me), delegates(threat), needed) {
        return me;
    }
    coalition
}

/// Our delegates on the coalition candidate would elect it: together they reach the threshold and pass the threat's
/// `threat` delegates (a tie at the threshold elects nobody, `systems::congress::leader_vote`). In a field where the
/// other seats vote for themselves, the pool gave the coalition candidate the World Leader: tuning offset 1690, the
/// `nets-x-site` seats won 42 diplomatic victories against net2 with the pool and 19 without net2's congress.
fn pool_elects(pooled: i32, threat: i32, needed: i32) -> bool {
    pooled >= needed && pooled > threat
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
    vote_orders_with(obs, guard, None)
}

/// `vote_orders_for`, read by a `net2` plan: the World Leader ballot is `leader_choice` and each resolution is voted
/// by what it names (`stance`); the delegates are split over the proposals the seat votes on, the leader vote
/// first and whole. None plays as today.
pub fn vote_orders_with(obs: &AIObservation<'_>, guard: bool, net2: Option<&Net2Plan>) -> Vec<Command> {
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
    let stances: Vec<Stance> = info.proposals.iter().map(|p| if p.kind.elects() { Stance::Yes } else { stance(obs, p, net2) }).collect();
    let voted: Vec<Proposal> = info.proposals.iter().zip(&stances).filter(|(_, s)| **s != Stance::Abstain).map(|(p, _)| *p).collect();
    let mut shares = shares(&voted, info.delegates_left).into_iter();
    let mut out = Vec::new();
    for (i, (proposal, stance)) in info.proposals.iter().zip(&stances).enumerate() {
        if *stance == Stance::Abstain {
            continue;
        }
        let delegates = shares.next().unwrap_or(0);
        if delegates == 0 {
            continue;
        }
        let choice = if proposal.kind.elects() {
            if proposal.kind == ResolutionKind::DiplomaticVictory {
                crate::ai::strategy_net::trace::leader_vote(obs, delegates, info.delegates_left, info.needed, info.proposals.len());
                if let Some(plan) = net2 {
                    out.push(Command::CastVotes { player: obs.player, proposal: i as u8, choice: u32::from(leader_choice(obs, info, plan.ballot).0), delegates });
                    continue;
                }
            }
            u32::from(obs.player.0)
        } else if *stance == Stance::Yes {
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
/// fewer delegates than proposals, the first ones get one each. No proposal (a `net2` seat abstains on
/// every one): no share.
fn shares(proposals: &[Proposal], delegates_left: u16) -> Vec<u16> {
    if proposals.is_empty() {
        return Vec::new();
    }
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
        let net2 = input.brief.net2_for(crate::ai::net2::plan::Part::Congress);
        let votes = vote_orders_with(obs, input.brief.vote_guard, net2);
        let proposed = proposal_orders_with(obs, input.rules, &weights, net2);
        vote_trace::seat(obs, input.brief.net2.is_some(), &votes, &proposed);
        let mut out = crate::ai::combat::proposals(8, votes);
        out.extend(crate::ai::combat::proposals(6, proposed));
        out.extend(crate::ai::combat::proposals(5, project_orders(obs)));
        out
    }
}

/// Scratch trace of the congress (`NET2_CONGRESS_TRACE=<file>`, net2/GAP.md): every seat's ballot on the World
/// Leader vote and every proposal it makes, one JSON line each, with the public delegates table. It decides nothing.
mod vote_trace {
    use super::*;
    use std::io::Write as _;

    fn thread() -> String {
        format!("{:?}", std::thread::current().id()).replace(|c: char| !c.is_ascii_digit(), "")
    }

    pub(super) fn seat(obs: &AIObservation<'_>, net2_seat: bool, votes: &[Command], proposed: &[Command]) {
        let Some(path) = std::env::var_os("NET2_CONGRESS_TRACE") else { return };
        let Some(info) = &obs.congress else { return };
        let table: Vec<String> = info.delegates.iter().map(|(p, n)| format!("[{},{n}]", p.0)).collect();
        let mut lines = Vec::new();
        for vote in votes {
            if let Command::CastVotes { proposal, choice, delegates, .. } = vote {
                let Some(p) = info.proposals.get(usize::from(*proposal)) else { continue };
                if p.kind == ResolutionKind::DiplomaticVictory {
                    lines.push(format!("{{\"ev\":\"leader\",\"th\":{},\"r\":{},\"p\":{},\"net2\":{net2_seat},\"needed\":{},\"choice\":{choice},\"cast\":{delegates},\"del\":[{}]}}", thread(), obs.round, obs.player.0, info.needed, table.join(",")));
                }
            }
        }
        for command in proposed {
            if let Command::ProposeResolution { kind, choice, repeal, .. } = command {
                lines.push(format!("{{\"ev\":\"propose\",\"th\":{},\"r\":{},\"p\":{},\"net2\":{net2_seat},\"kind\":\"{kind:?}\",\"choice\":{choice},\"repeal\":{repeal}}}", thread(), obs.round, obs.player.0));
            }
        }
        if lines.is_empty() {
            return;
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(format!("{}\n", lines.join("\n")).as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal(kind: ResolutionKind) -> Proposal {
        Proposal { kind, choice: 0, proposer: None, repeal: false }
    }

    /// The delegates are split over the proposals voted; a seat that abstains on every proposal (`net2`'s
    /// `stance`) votes on none, and the split of no proposal is empty, not a division by zero (G2 of the
    /// integration, 22 Nov: two games panicked here).
    #[test]
    fn the_split_of_no_proposal_is_empty() {
        assert_eq!(shares(&[], 7), Vec::<u16>::new());
        let two = [proposal(ResolutionKind::PlayerEmbargo), proposal(ResolutionKind::PlayerEmbargo)];
        assert_eq!(shares(&two, 7), vec![4, 3]);
        let with_vote = [proposal(ResolutionKind::PlayerEmbargo), proposal(ResolutionKind::DiplomaticVictory)];
        assert_eq!(shares(&with_vote, 7), vec![0, 7]);
    }
}
