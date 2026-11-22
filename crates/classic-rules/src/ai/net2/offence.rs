//! The offence of the addendum (work/new-net-offence-addendum-2026-11-22.md §3.3) whose inputs `net2` already has,
//! wired now (work/nets/tricks/CHAIN.md, Roman: «не только защищаться от этих техник, но и их использовать»).
//! The driver keeps these levers only for the variants that play the offence (`Variant::net2_offence`: `net2`,
//! `net2-tac`, the ablation's; not `net2-defence`, the control), and clears them otherwise (`clear`).
//!
//! | lever | rule (inputs) | executor |
//! |---|---|---|
//! | `WarOrder::raid` (MO6) | the order is at war with its rival and no operation of ours on it forms or marches (R `operating` 0): the war exists, no capture is under way, its free units pillage | `raid_war::sortie` (the units with no task of the empire's, priced by `field_craft::trip`); never a war for a raid |
//!
//! The rest of the addendum's levers wait for inputs `net2` does not compute yet (G13 spare copies, G14 vote reach,
//! G15 worker need, the pairs P1–P4, the aims T2–T6, R25–R31): CHAIN.md lists each with its size.

use super::levers::Pass;
use super::plan::{Net2Plan, OrderPhase, WarPlan};
use super::weights::r;

/// The switch of the raid (the stop rule of work/nets/tricks/GATES.md).
pub const RAID: bool = true;

/// The offence's levers on the turn's plan (`plan.war`'s order, `plan.offence`).
pub fn apply(plan: &mut Net2Plan, pass: &Pass) {
    if let WarPlan::Order(order) = &mut plan.war {
        let fighting = matches!(order.phase, OrderPhase::Fighting { .. });
        let operating = pass.rivals.iter().find(|x| x.rival == order.rival).is_some_and(|x| x.inputs[r::OPERATING] > 0);
        order.raid = RAID && fighting && !operating;
    }
}

/// The plan of a variant that does not play the offence (`net2-defence`): every offence lever at its default.
pub fn clear(plan: &mut Net2Plan) {
    plan.offence = Default::default();
    if let WarPlan::Order(order) = &mut plan.war {
        order.raid = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::net2::plan::{Opening, WarKind, WarOrder};
    use crate::ids::PlayerId;

    fn order(phase: OrderPhase) -> Net2Plan {
        let war = WarOrder { rival: PlayerId(1), kind: WarKind::Prey, since: 1, budget: 20, phase, value: 800, aim: None, opening: Opening::Plain, raid: false };
        Net2Plan { war: WarPlan::Order(war), ..Net2Plan::default() }
    }

    #[test]
    fn clear_takes_the_raid_back() {
        let mut plan = order(OrderPhase::Fighting { since: 1 });
        if let WarPlan::Order(o) = &mut plan.war {
            o.raid = true;
        }
        clear(&mut plan);
        assert!(plan.war.order().is_some_and(|o| !o.raid));
    }
}
