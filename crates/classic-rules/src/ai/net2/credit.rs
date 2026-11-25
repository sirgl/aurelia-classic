//! `credit[r]`: the share of a flow paid for now that `net2` expects to receive beyond the rounds the rules
//! guarantee (the tricks lane's notes, D1/D2/D3/D6; design §4.6 "items we give first are paid against items
//! that flow later at trust[r]", completed).
//!
//! Why a separate lever and not `trust`: the swindler of the first sale is a friend with no past betrayal, so its
//! trust is 1000 (distrust's bias −500, friend −300) and `trust` cannot fire before the first betrayal: the human
//! defence exam's HD1 measured 0.97 of seats paying, under `net2` as under `nets-x-site`. A flow paid for now is a
//! credit to the payer: a declaration ends the flow at once (`systems/diplomacy/mod.rs`, `end_ties`) and keeps the
//! gold, and on these rules nothing makes the declarer pay for it (no backstab memory; `fix-rules` adds the betrayal
//! grievance on `main` after 22 Nov 14:50, the input that would earn a credit). So the credit is 0: `net2` pays
//! now only for the rounds a peace treaty guarantees (`peace::guaranteed_rounds`). Read by `peace::Negotiation`.

use super::plan::Net2Plan;
use crate::ai::strategy_net::Milli;
use crate::ids::PlayerId;

/// The credit of every rival on these rules, with its reason (a hand weight, `weights`' method).
pub const CREDIT: (Milli, &str) = (0, "nothing in these rules makes a nation that ends a flow by war pay for it: a flow paid for now is worth only the rounds a peace treaty guarantees");

impl Net2Plan {
    /// `credit[r]`, 0…1000: the share of the unguaranteed rounds of a flow paid for now that counts (`CREDIT`).
    pub fn credit(&self, _rival: PlayerId) -> Milli {
        CREDIT.0
    }
}
