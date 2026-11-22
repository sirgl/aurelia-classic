//! What the war order promises the executors that price a war (work/nets/tricks/CHAIN.md, D8).
//!
//! The peace answer (`peace::Negotiation::answer`) refuses a white peace only while going on is worth more
//! (`WarProspect`), and going on was priced by a running operation alone: a seat at war with an order of `net2` on the
//! enemy whose army had not formed yet (or had formed nowhere) priced its war at 0 and took every white peace (HD8:
//! 1.00 of seats; the design's "already guarded (code): `Negotiation::answer`" held only with an operation running).
//! The order is the plan's decision that the war pays; its aim and the aim's ease are the plan's estimate of what it
//! takes. `order_eases` carries them to the executors (`Net2Plan::order_eases`).

use super::plan::{TargetInputs, WarPlan};

/// The switch of this lever (the stop rule of work/nets/tricks/GATES.md): false gives the peace answer back its
/// pricing by a running operation alone.
pub const PRICES_PEACE: bool = true;
use crate::ai::strategy_net::Milli;

/// The order rival's known cities with their eases, the easiest first; empty without an order, and while the wars do
/// not pay (G4 `war_gain` at or below even). Why the second condition (the stop rule's hand edit, GATES.md): priced in
/// every ordered war, the lever made the wars of `net2` 15.5 → 21.7 rounds long and its real victories 99 → 90 in 400
/// games (offsets 1950–1951); D8 is the winner that gives its winning war away, not the war that has not paid, which
/// the order's budget ends by design.
pub fn order_eases(war: &WarPlan, targets: &[TargetInputs], eases: &[Milli], war_gain: Milli) -> Vec<(crate::ids::TileId, Milli)> {
    let Some(order) = war.order() else { return Vec::new() };
    if war_gain <= crate::ai::strategy_net::layers::ONE / 2 {
        return Vec::new();
    }
    let mut out: Vec<(crate::ids::TileId, Milli)> = targets.iter().zip(eases).filter(|(t, _)| t.rival == order.rival).map(|(t, e)| (t.city, *e)).collect();
    out.sort_by_key(|(t, e)| (std::cmp::Reverse(*e), t.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::net2::plan::{OrderPhase, Opening, WarKind, WarOrder};
    use crate::ids::{PlayerId, TileId};

    fn target(rival: u8, city: u32) -> TargetInputs {
        TargetInputs { rival: PlayerId(rival), city: TileId(city), size: 0, garrison: 0, near: 0, walls: 0 }
    }

    /// The order's cities price the war only while the wars pay (G4 above even), the easiest first, the order's rival only.
    #[test]
    fn the_order_prices_the_war_only_while_it_pays() {
        let order = WarOrder { rival: PlayerId(1), kind: WarKind::Prey, since: 1, budget: 20, phase: OrderPhase::Fighting { since: 1 }, value: 900, aim: None, opening: Opening::Plain, raid: false };
        let war = WarPlan::Order(order);
        let targets = [target(1, 10), target(2, 20), target(1, 30)];
        let eases = [300, 900, 700];
        assert_eq!(order_eases(&war, &targets, &eases, 750), vec![(TileId(30), 700), (TileId(10), 300)]);
        assert!(order_eases(&war, &targets, &eases, 500).is_empty(), "even: the order does not price peace");
        assert!(order_eases(&WarPlan::NoWar, &targets, &eases, 1000).is_empty());
    }
}
