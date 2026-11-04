//! The site net: a small learned model of how a city founded on a site
//! develops, read from the site's row of `ai::site::site_features` (what
//! the player knows when it chooses). It predicts the city's population 50
//! rounds after the founding. Trained offline on AI-only games, on every
//! city but the first (the capital is not the settling planner's choice;
//! a city lost before the 50 rounds counts as population 0), by
//! `ai-train-site` (aurelia-qa, its `nn` module); the weights are the
//! generated `site_weights`. The settling planner of `Variant::SiteNet`
//! values a site worth a city by it (`planner_settle::SiteQuality::Net`,
//! `planner_score`); classic/docs/ai-site.md has its quality against the
//! planner's own score and the games.
//!
//! Integer only (`ai::net`, the AI's one inference of its learned nets),
//! so the game gets the same answer on every platform; about 1 µs a site.
//!
//! The weights reproduce, bit for bit, from the games (the data split by
//! seed, `ai-train-site`):
//!
//! ```text
//! cargo +1.98.1 run --release -p aurelia-qa --bin ai-site -- --seeds 1..=800 --size small --rounds 300 --jobs 10 --out sites.csv
//! cargo +1.98.1 run --release -p aurelia-qa --bin ai-train-site -- --data sites.csv --heads pop:huber --hidden 32,16 --seed 1 \
//!     --out-weights crates/classic-rules/src/ai/site_weights.rs --report REPORT.md
//! ```

use super::net::{self, Net, ACT};
use super::site::SITE_FEATURES;
#[cfg(feature = "experimental-ai")]
use super::site_weights;

/// The f32 net's answer on one row of the test games, kept beside the
/// weights: the integer inference is checked against it.
#[derive(Debug, Clone, Copy)]
pub struct Reference {
    pub features: [i32; SITE_FEATURES],
    /// The f32 net's first head in thousandths (population: thousandths of
    /// a citizen).
    pub output_milli: i64,
}

/// The city's expected population 50 rounds after its founding on the
/// site of this row (`site::site_features`), in thousandths of a citizen.
/// Not clamped: a poor site may come out slightly below 0.
/// Compiled only with `experimental-ai`: the weights are `Variant::SiteNet`'s alone.
#[cfg(feature = "experimental-ai")]
pub fn predict(features: &[i32; SITE_FEATURES]) -> i32 {
    infer(&site_weights::NET, features)
}

/// The answer of a site net for a row: its first head in thousandths.
pub fn infer(net: &Net<'_>, features: &[i32]) -> i32 {
    let mut out = [0i64; net::MAX_WIDTH];
    let outputs = net.outputs();
    net::forward(net, features, &mut out[..outputs]);
    net::round_shift(out[0] * 1000, ACT) as i32
}

/// The settling planner's score points of an answer of `predict`: a
/// linear map fitted on the training games (`ai-train-site`: the spread
/// and mean of the planner's own land score), so the answer weighs against
/// the travel and the distance terms of `planner_settle` as its own score
/// did.
#[cfg(feature = "experimental-ai")]
pub fn planner_score(milli_citizens: i32) -> i32 {
    let points = i64::from(site_weights::SCORE_PER_CITIZEN) * i64::from(milli_citizens);
    site_weights::SCORE_AT_ZERO + (points + 500).div_euclid(1000) as i32
}

/// The f32 net's answers on a few test rows (`Reference`).
#[cfg(feature = "experimental-ai")]
pub fn references() -> &'static [Reference] {
    &site_weights::REFERENCE
}
