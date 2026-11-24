//! The site head of `net2` (design §3.4): one bounded linear neuron per candidate tile, in place of the
//! guarded `planner_settle::net_score`. Part E calls `score` from the site search; the legal and
//! minimum-score gates stay in `planner_settle`.
//!
//! site = clamp(S1, S2 − 169, S2 + 169) + S3 − 30·S4·(1 + S5/1000) − 60·S6 − 40·S7 + 25·S8·S9/10⁶ − S11·S12/1000
//! (S11·S12: `early-growth`'s shared land, 0 for every other variant).

use crate::ai::strategy_net::layers::ONE;

use super::plan::SiteInputs;
use super::weights::site as w;

/// The site's score in the planner's points.
pub fn score(site: &SiteInputs) -> i64 {
    let bound = w::BOUND.0;
    let net = site.pop50.clamp(site.land - bound, site.land + bound);
    let gap = w::GAP.0 * site.rival_gap * (ONE + site.rival_menace) / ONE;
    let fill = w::FILL.0 * site.fill * site.tall / (ONE * ONE);
    let shared = site.shared * site.shared_weight / ONE;
    net + site.dist - gap - w::ARMY.0 * site.army - w::TRAVEL.0 * site.travel + fill - shared
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> SiteInputs {
        SiteInputs { pop50: 600, land: 300, dist: -20, ..SiteInputs::default() }
    }

    #[test]
    fn the_net_is_bounded_around_the_land_score() {
        assert_eq!(score(&site()), 300 + 169 - 20, "the net's answer at most one citizen above the land score");
        assert_eq!(score(&SiteInputs { pop50: 250, ..site() }), 250 - 20, "inside the bound the net's answer");
    }

    #[test]
    fn a_menacing_rival_doubles_the_gap_penalty() {
        let near = SiteInputs { rival_gap: 3, ..site() };
        assert_eq!(score(&site()) - score(&near), 90, "−30 a tile next to a weak friend");
        assert_eq!(score(&site()) - score(&SiteInputs { rival_menace: 1000, ..near }), 180, "twice next to a menacing one");
    }

    #[test]
    fn army_travel_and_gaps() {
        assert_eq!(score(&site()) - score(&SiteInputs { army: 2, travel: 3, ..site() }), 120 + 120);
        assert_eq!(score(&SiteInputs { fill: 1000, tall: 1000, ..site() }), score(&site()), "a gap weighs nothing: it stays ours (GAP.md)");
        assert_eq!(score(&SiteInputs { fill: 1000, tall: 0, ..site() }), score(&site()), "a wide seat does not");
    }

    #[test]
    fn shared_land_costs_its_weight() {
        assert_eq!(score(&SiteInputs { shared: 420, ..site() }), score(&site()), "no weight, no term (every variant but early-growth)");
        assert_eq!(score(&site()) - score(&SiteInputs { shared: 420, shared_weight: 333, ..site() }), 139, "a third of the shared land");
    }
}
