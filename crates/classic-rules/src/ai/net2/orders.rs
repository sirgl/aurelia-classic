//! What the war order promises the executors that price a war (work/nets/tricks/CHAIN.md, D8).
//!
//! The peace answer (`peace::Negotiation::answer`) refuses a white peace only while going on is worth more
//! (`WarProspect`), and going on was priced by a running operation alone: a seat at war with an order of `net2` on the
//! enemy whose army had not formed yet (or had formed nowhere) priced its war at 0 and took every white peace (HD8:
//! 1.00 of seats; the design's "already guarded (code): `Negotiation::answer`" held only with an operation running).
//! The order is the plan's decision that the war pays; its aim and the aim's ease are the plan's estimate of what it
//! takes. `order_eases` carries them to the executors (`Net2Plan::order_eases`).

use super::plan::{TargetInputs, WarPlan};
use crate::ai::strategy_net::Milli;

/// The order rival's known cities with their eases, the easiest first (empty without an order).
pub fn order_eases(war: &WarPlan, targets: &[TargetInputs], eases: &[Milli]) -> Vec<(crate::ids::TileId, Milli)> {
    let Some(order) = war.order() else { return Vec::new() };
    let mut out: Vec<(crate::ids::TileId, Milli)> = targets.iter().zip(eases).filter(|(t, _)| t.rival == order.rival).map(|(t, e)| (t.city, *e)).collect();
    out.sort_by_key(|(t, e)| (std::cmp::Reverse(*e), t.0));
    out
}
