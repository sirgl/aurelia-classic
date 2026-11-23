//! The science flow's levers of `net2` (`science-flow` only, `TurnBrief::science_flow`): the share of the science
//! specialists' slots filled (`weights::SCIENTISTS`), the rivals a research agreement with pays our race
//! (`weights::AGREEMENT`) and the gold their agreements need (work/nets/science-flow/SCIENCE-FLOW.md). The executors
//! read `plan::Science`; nothing here moves a citizen or proposes a deal.

use crate::ai::strategy_net::{Intent, Milli};
use crate::knowledge::observation::AIObservation;
use crate::rules::CompiledRuleset;

use super::concepts::{fire, Frame};
use super::levers::{shares, Pass};
use super::plan::{Inputs, Science, WarPlan};
use super::weights::{self as w, gate, r};

/// The levers of the turn: the scientists' level, the ties (no tie with the war order's target or a rival at war) and
/// their chest (the agreement's cost, at the game speed, for every tie that is a friend without an agreement while our
/// technologies allow agreements).
pub fn plan(obs: &AIObservation<'_>, rules: &CompiledRuleset, pass: &Pass, intent: &Intent, inputs: &Inputs) -> Science {
    let y = shares(intent);
    let scientists = fire(&w::SCIENTISTS, &Frame { f: &inputs.situation, y: &y, ..Frame::default() });
    let target = match pass.core.war {
        WarPlan::Order(order) => Some(order.rival),
        _ => None,
    };
    let mut ties = Vec::new();
    for rival in &pass.rivals {
        if Some(rival.rival) == target || rival.inputs[r::AT_WAR] > 0 {
            continue;
        }
        let level = fire(&w::AGREEMENT, &Frame { r: &rival.inputs, q: &rival.concepts, y: &y, ..Frame::default() });
        if level >= gate::AGREEMENT.0 {
            ties.push(rival.rival);
        }
    }
    let allowed = obs.economy.techs.iter().any(|t| rules.technologies.get(t.0 as usize).is_some_and(|d| d.allows_research_agreement));
    let lacking = ties.iter().filter(|p| obs.relation(**p).is_some_and(|x| x.friends && !x.research_agreement)).count() as u32;
    let chest = if allowed { lacking * agreement_cost(obs, rules) } else { 0 };
    if let Some(path) = std::env::var_os("SCIENCE_FLOW_TRACE") {
        let levels: Vec<String> = pass.rivals.iter().map(|x| format!("[{},{},{}]", x.rival.0, fire(&w::AGREEMENT, &Frame { r: &x.inputs, q: &x.concepts, y: &y, ..Frame::default() }), x.inputs[r::PACE])).collect();
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            use std::io::Write as _;
            let _ = writeln!(file, "{{\"p\":{},\"r\":{},\"aim\":{},\"sci\":{scientists},\"ties\":{},\"chest\":{chest},\"levels\":[{}]}}", obs.player.0, obs.round, y[super::weights::y::AIM_SCIENCE], ties.len(), levels.join(","));
        }
    }
    Science { scientists, ties, chest, beaker_tilt: science_tilt(inputs) }
}

/// `weights::SCIENCE_TILT`: the science price's tilt in the production head (`price_tilt[pd::SCIENCE]`).
pub fn science_tilt(inputs: &Inputs) -> Milli {
    fire(&w::SCIENCE_TILT, &Frame { f: &inputs.situation, ..Frame::default() })
}

/// The research agreement's cost of our era at the game speed (the rules charge the higher era of the two:
/// `deals::research_agreement_cost`; ours is what we know).
fn agreement_cost(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> u32 {
    let era = obs
        .economy
        .techs
        .iter()
        .filter_map(|t| rules.technologies.get(t.0 as usize)?.content_era)
        .filter_map(|e| rules.eras.get(e.0 as usize))
        .max_by_key(|e| e.index);
    let cost = era.map_or(0, |e| e.research_agreement_cost);
    kernel::rules::speeds::SpeedValues::scale(cost, obs.speed.gold_percent)
}

/// The tie levels of every rival (diagnostics and tests): (rival, `weights::AGREEMENT`).
pub fn tie_levels(pass: &Pass, intent: &Intent) -> Vec<(crate::ids::PlayerId, Milli)> {
    let y = shares(intent);
    pass.rivals.iter().map(|rival| (rival.rival, fire(&w::AGREEMENT, &Frame { r: &rival.inputs, q: &rival.concepts, y: &y, ..Frame::default() }))).collect()
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
        let mut ri = [0; r::N];
        ri[r::PACE] = pace;
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

    /// 600 + 400·aim − 1200·max(0, 500 − pace) − 1000·distrust − 800·runaway + 800·runaway·aim; a tie from 500.
    #[test]
    fn a_friend_at_peace_is_a_tie_the_runaway_only_for_a_seat_that_races_it() {
        assert_eq!(agreement(350, 500, 0, 0), 740, "a level friend, any aim");
        assert!(agreement(350, 500, 0, 0) >= gate::AGREEMENT.0);
        assert_eq!(agreement(350, 375, 0, 0), 590, "a quarter less science: a smaller pool, still a tie");
        assert_eq!(agreement(350, 500, 500, 0), 240, "distrust");
        assert_eq!(agreement(350, 600, 0, 900), 272, "the runaway for a seat between aims: denied");
        assert_eq!(agreement(800, 600, 0, 900), 776, "the runaway for a seat that races it: a tie");
    }

    /// 3000·max(0, progress − 400) − 400·war, linear (by hand).
    #[test]
    fn the_beaker_tilt_grows_from_the_renaissance() {
        let tilt = |progress: Milli, war: Milli| {
            let mut f = [0; fe::N];
            f[fe::PROGRESS] = progress;
            f[fe::AT_WAR] = war;
            fire(&w::SCIENCE_TILT, &Frame { f: &f, ..Frame::default() })
        };
        assert_eq!(tilt(300, 0), 0, "early the carried rows alone");
        assert_eq!(tilt(600, 0), 600);
        assert_eq!(tilt(800, 1000), 800, "at war 400 less");
    }
}
