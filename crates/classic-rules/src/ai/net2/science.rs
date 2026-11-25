//! The science flow's levers of `net2` (`science-flow` only, `TurnBrief::science_flow`): the share of the science
//! specialists' slots filled (`weights::SCIENTISTS`), the rivals a research agreement with pays our race and the cadence
//! of the ladder to it (`weights::AGREEMENT`), and the gold their agreements need until a partner lets it wait too long
//! (`r::CHEST_WAIT`, from `plan::ScienceMemory`) (the science-flow lane's notes). The executors read
//! `plan::Science`; nothing here moves a citizen or proposes a deal.

use crate::ai::strategy_net::{Intent, Milli};
use crate::ids::PlayerId;
use crate::knowledge::observation::AIObservation;
use crate::rules::CompiledRuleset;

use super::concepts::{fire, Frame};
use super::levers::{shares, Pass};
use super::plan::{ChestWait, Inputs, RivalInputs, Science, ScienceMemory, Tie, WarPlan};
use super::weights::{self as w, gate, r, y};

/// The levers of the turn: the scientists' level, the ties (none with the war order's target or a rival at war) and
/// their chest (the rules' cost of the agreement for every tie that is a friend without one while our technologies allow
/// agreements). Writes how long the chest has held each partner's gold into `memory`.
pub fn plan(obs: &AIObservation<'_>, rules: &CompiledRuleset, pass: &Pass, intent: &Intent, inputs: &Inputs, memory: &mut ScienceMemory) -> Science {
    let shares = shares(intent);
    let scientists = fire(&w::SCIENTISTS, &Frame { f: &inputs.situation, y: &shares, ..Frame::default() });
    let ties = ties(pass, &shares);
    let chest = chest(obs, rules, &ties, memory);
    Science { scientists, ties, chest }
}

/// The gold the chest keeps this turn (the rules' cost of every agreement the ties still lack), and the turn of the
/// waits in `memory` (`record_waits`).
pub fn chest(obs: &AIObservation<'_>, rules: &CompiledRuleset, ties: &[Tie], memory: &mut ScienceMemory) -> u32 {
    let held = held(obs, rules, ties);
    record_waits(memory, obs, &held);
    held.iter().map(|rival| agreement_cost(obs, rules, *rival)).sum()
}

/// The rivals whose `weights::AGREEMENT` level reaches `gate::AGREEMENT`, each with the cadence of its level.
fn ties(pass: &Pass, shares: &[Milli; y::N]) -> Vec<Tie> {
    let target = match pass.core.war {
        WarPlan::Order(order) => Some(order.rival),
        _ => None,
    };
    let mut ties = Vec::new();
    for rival in &pass.rivals {
        if Some(rival.rival) == target || rival.inputs[r::AT_WAR] > 0 {
            continue;
        }
        let level = fire(&w::AGREEMENT, &Frame { r: &rival.inputs, q: &rival.concepts, y: shares, ..Frame::default() });
        if level >= gate::AGREEMENT.0 {
            ties.push(Tie { rival: rival.rival, every: cadence(level) });
        }
    }
    ties
}

/// The rounds between two offers of a tie's ladder: `gate::LADDER_ROUNDS` at the gate, 1 at a level of 1000, linear
/// between (a level friend's 740: 3).
pub fn cadence(level: Milli) -> u32 {
    let (gate, slowest) = (gate::AGREEMENT.0, gate::LADDER_ROUNDS.0);
    let below_full = (1000 - level.clamp(gate, 1000)) as u32;
    1 + (slowest - 1) * below_full / (1000 - gate) as u32
}

/// The ties the chest holds gold for: friends without an agreement, while our technologies allow agreements.
fn held(obs: &AIObservation<'_>, rules: &CompiledRuleset, ties: &[Tie]) -> Vec<PlayerId> {
    let allowed = obs.economy.techs.iter().any(|t| rules.technologies.get(t.0 as usize).is_some_and(|d| d.allows_research_agreement));
    if !allowed {
        return Vec::new();
    }
    ties.iter().map(|t| t.rival).filter(|p| obs.relation(*p).is_some_and(|x| x.friends && !x.research_agreement)).collect()
}

/// The research agreement's cost with `rival` as the rules charge it (`deals::agreement_cost_of`): the higher era of
/// the two, ours from our technologies, its from the era the public screens show (ours while unknown).
pub fn agreement_cost(obs: &AIObservation<'_>, rules: &CompiledRuleset, rival: PlayerId) -> u32 {
    let ours = crate::combat::city::era_of_techs(rules, &obs.economy.techs);
    let theirs = match obs.public.eras.iter().find(|(p, _)| *p == rival) {
        Some((_, index)) => rules.eras.iter().filter(|e| e.index <= *index).max_by_key(|e| e.index).or(ours),
        None => ours,
    };
    crate::systems::diplomacy::deals::agreement_cost_of([ours, theirs], obs.speed.gold_percent)
}

/// One turn of the waits: a partner that signed or is no friend any more starts again; every partner the chest holds
/// gold for waits the rounds since the last turn written. A second pass in a round adds nothing.
fn record_waits(memory: &mut ScienceMemory, obs: &AIObservation<'_>, held: &[PlayerId]) {
    let first = *memory == ScienceMemory::default();
    if !first && obs.round <= memory.round {
        return;
    }
    let rounds = if first { 1 } else { obs.round - memory.round };
    memory.waits.retain(|wait| obs.relation(wait.rival).is_some_and(|x| x.friends && !x.research_agreement));
    for rival in held {
        match memory.waits.iter_mut().find(|wait| wait.rival == *rival) {
            Some(wait) => wait.rounds += rounds,
            None => memory.waits.push(ChestWait { rival: *rival, rounds }),
        }
    }
    memory.round = obs.round;
}

/// `r::CHEST_WAIT` of every rival read: its wait over `gate::CHEST_WAIT_ROUNDS` at the game's pace, 0…1000.
pub fn read_waits(memory: &ScienceMemory, obs: &AIObservation<'_>, rivals: &mut [RivalInputs]) {
    let full = obs.speed.pace_rounds(gate::CHEST_WAIT_ROUNDS.0).max(1);
    for rival in rivals {
        let rounds = memory.waits.iter().find(|wait| wait.rival == rival.rival).map_or(0, |wait| wait.rounds);
        rival.chest_wait = (i64::from(rounds.min(full)) * 1000 / i64::from(full)) as Milli;
    }
}

#[cfg(test)]
mod tests {
    use super::super::concepts::{fire, Frame};
    use super::super::weights::{self as w, gate, q, r, y};
    use crate::ai::strategy_net::weights::fe;
    use crate::ai::strategy_net::Milli;

    fn scientists(aim: Milli, progress: Milli, at_war: Milli) -> Milli {
        let mut f = [0; fe::N];
        f[fe::PROGRESS] = progress;
        f[fe::AT_WAR] = at_war;
        let mut s = [0; y::N];
        s[y::AIM_SCIENCE] = aim;
        fire(&w::SCIENTISTS, &Frame { f: &f, y: &s, ..Frame::default() })
    }

    fn agreement(aim: Milli, pace: Milli, distrust: Milli, runaway: Milli) -> Milli {
        agreement_waited(aim, pace, distrust, runaway, 0)
    }

    fn agreement_waited(aim: Milli, pace: Milli, distrust: Milli, runaway: Milli, wait: Milli) -> Milli {
        let mut ri = [0; r::N];
        ri[r::PACE] = pace;
        ri[r::CHEST_WAIT] = wait;
        let mut c = [0; q::N];
        c[q::DISTRUST] = distrust;
        c[q::RUNAWAY] = runaway;
        let mut s = [0; y::N];
        s[y::AIM_SCIENCE] = aim;
        fire(&w::AGREEMENT, &Frame { r: &ri, q: &c, y: &s, ..Frame::default() })
    }

    /// −900 + 2500·max(0, progress − 300) + 600·aim − 500·war, clamped to 0…1000 (each by hand).
    #[test]
    fn the_slots_fill_late_and_at_peace_more_with_the_science_aim() {
        assert_eq!(scientists(350, 600, 0), 60, "−900 + 750 + 210: mid-game, a seat between aims");
        assert_eq!(scientists(350, 750, 0), 435, "−900 + 1125 + 210: late");
        assert_eq!(scientists(1000, 750, 0), 825, "late with the science aim");
        assert_eq!(scientists(1000, 750, 1000), 325, "at war the hammers");
        assert_eq!(scientists(1000, 300, 0), 0, "early no slot is filled by hand");
    }

    /// 600 + 400·aim − 1200·max(0, 500 − pace) − 1000·distrust − 800·runaway + 800·runaway·aim − 600·wait; a tie
    /// from 500.
    #[test]
    fn a_friend_at_peace_is_a_tie_the_runaway_only_for_a_seat_that_races_it() {
        assert_eq!(agreement(350, 500, 0, 0), 740, "a level friend, any aim");
        assert!(agreement(350, 500, 0, 0) >= gate::AGREEMENT.0);
        assert_eq!(agreement(350, 375, 0, 0), 590, "a quarter less science: a smaller pool, still a tie");
        assert_eq!(agreement(350, 500, 500, 0), 240, "distrust");
        assert_eq!(agreement(350, 600, 0, 900), 272, "the runaway for a seat between aims: denied");
        assert_eq!(agreement(800, 600, 0, 900), 776, "the runaway for a seat that races it: a tie");
    }

    /// A partner that lets our gold wait frees the chest: a level friend after 0.4 of the wait (8 Standard rounds), a
    /// sure tie after 0.84.
    #[test]
    fn a_long_wait_unties() {
        assert_eq!(agreement_waited(350, 500, 0, 0, 400), 500, "a level friend at 0.4 of the wait: the gate");
        assert!(agreement_waited(350, 500, 0, 0, 450) < gate::AGREEMENT.0, "past it: no tie, no chest");
        assert!(agreement_waited(1000, 500, 0, 0, 830) >= gate::AGREEMENT.0 && agreement_waited(1000, 500, 0, 0, 840) < gate::AGREEMENT.0);
    }

    /// Every round at 1000, the diplomacy's interval at the gate, a level friend every third round.
    #[test]
    fn the_cadence_follows_the_level() {
        use super::cadence;
        assert_eq!((cadence(1000), cadence(740), cadence(500)), (1, 3, gate::LADDER_ROUNDS.0));
        assert_eq!(gate::LADDER_ROUNDS.0, crate::ai::diplomacy::PROPOSAL_INTERVAL, "at the gate a tie asks as often as any deal");
    }
}
