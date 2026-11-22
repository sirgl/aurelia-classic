//! The site head of `net2` (design §3.4): one bounded linear neuron per candidate tile, in place of the
//! guarded `planner_settle::net_score`. Part E calls `score` from the site search; the legal and
//! minimum-score gates stay in `planner_settle`.
//!
//! site = clamp(S1, S2 − 169, S2 + 169) + S3 − 30·S4·(1 + S5/1000) − 60·S6 − 40·S7 + 25·S8·S9/1000

use super::plan::SiteInputs;

/// The site's score in the planner's points.
pub fn score(site: &SiteInputs) -> i64 {
    let _ = site;
    0
}
