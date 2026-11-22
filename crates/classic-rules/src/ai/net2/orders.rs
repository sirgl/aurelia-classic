//! What the war order promises the executors that price a war (work/nets/tricks/CHAIN.md, D8).
//!
//! The peace answer (`peace::Negotiation::answer`) refuses a white peace only while going on is worth more
//! (`WarProspect`), and going on was priced by a running operation alone: a seat at war with an order of `net2` on the
//! enemy whose army had not formed yet (or had formed nowhere) priced its war at 0 and took every white peace (HD8:
//! 1.00 of seats; the design's "already guarded (code): `Negotiation::answer`" held only with an operation running).
//! The order is the plan's decision that the war pays; its aim and the aim's ease are the plan's estimate of what it
//! takes. `aim_ease` carries the ease to the executors (`Net2Plan::aim_ease`).

use super::plan::{TargetInputs, WarPlan};
use crate::ai::strategy_net::Milli;

/// The ease of the target the order aims at (0 without an order or an aim, or when the target is not read).
pub fn aim_ease(war: &WarPlan, targets: &[TargetInputs], eases: &[Milli]) -> Milli {
    let Some(aim) = war.order().and_then(|o| o.aim) else { return 0 };
    targets.iter().zip(eases).find(|(t, _)| t.city == aim.city).map_or(0, |(_, e)| *e)
}
