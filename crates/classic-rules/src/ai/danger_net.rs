//! The danger net: a small learned model of the damage an own military land
//! unit takes between the end of its owner's turn and the start of the next,
//! and of the chance it dies, read from the unit's row of
//! `ai::danger::unit_features`. Trained offline on AI-only games and
//! synthetic battles (`aurelia_qa::battle`) by `ai-train-danger`
//! (aurelia-qa, its `nn` module); the weights are the
//! generated `danger_weights`. Consulted only where a threat exists (the
//! row's `threats` > 0): it was trained on such rows.
//!
//! Integer only (`ai::net`, the AI's one inference of its learned nets:
//! fixed point, i64 sums), so the game gets the same answer on every
//! platform. The net: 37 inputs → two ReLU layers → two outputs:
//!
//! - the damage head: Q16 of damage / 100; the damage in hit points is it
//!   × 100, rounded, clamped to `[0, hp]`;
//! - the death head: Q16 of the logit; the probability is
//!   `net::sigmoid_ppm` in permille, rounded.
//!
//! The trainer runs this same function on its quantised weights, so the
//! report's numbers for the integer net are the game's.
//!
//! The weights reproduce, bit for bit, from the games and the battles
//! (the data split by seed, `ai-train-danger`):
//!
//! ```text
//! cargo +1.98.1 run --release -p aurelia-qa --bin ai-danger -- --seeds 1..=200 --size small --rounds 200 --out games.csv
//! cargo +1.98.1 run --release -p aurelia-qa --bin ai-danger -- --battles --seeds 1..=200 --size small --rounds 30 --out battles.csv
//! cargo +1.98.1 run --release -p aurelia-qa --bin ai-train-danger -- --data games.csv,battles.csv --hidden 64,32 --seed 1 \
//!     --out-weights crates/classic-rules/src/ai/danger_weights.rs --report REPORT.md
//! ```

use super::danger::FEATURES;
use super::danger_weights;
use super::net::{self, Activation, Layer, Net, ACT};

/// The net's answer for one unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Danger {
    /// The expected damage before the owner's next turn, hit points,
    /// 0..=the unit's hp.
    pub damage: u16,
    /// The chance the unit is gone by then, 0..=1000.
    pub death_permille: u16,
}

/// The generated weights (`danger_weights`): the three layers of
/// `ai::net`, named as the file has them. Rows are by output:
/// `w1[j * inputs + i]` is the weight of input `i` in unit `j`.
#[derive(Debug, Clone, Copy)]
pub struct Weights<'w> {
    pub low: &'w [i32],
    pub high: &'w [i32],
    pub w1: &'w [i32],
    pub b1: &'w [i64],
    pub shift1: u32,
    pub w2: &'w [i32],
    pub b2: &'w [i64],
    pub shift2: u32,
    /// Two rows: the damage head, the death head.
    pub w3: &'w [i32],
    pub b3: &'w [i64],
    pub shift3: u32,
}

impl<'w> Weights<'w> {
    /// The layers as `ai::net` runs them: ReLU, ReLU, linear.
    pub fn layers(&self) -> [Layer<'w>; 3] {
        let layer = |w, b, shift, activation| Layer { w, b, shift, activation };
        [
            layer(self.w1, self.b1, self.shift1, Activation::Relu),
            layer(self.w2, self.b2, self.shift2, Activation::Relu),
            layer(self.w3, self.b3, self.shift3, Activation::Linear),
        ]
    }
}

/// The f32 net's answer on one row of the test games, kept beside the
/// weights: the integer inference is checked against it.
#[derive(Debug, Clone, Copy)]
pub struct Reference {
    pub features: [i32; FEATURES],
    /// The f32 net's damage, clamped to `[0, hp]`, in thousandths of a hit point.
    pub damage_milli: i32,
    /// The f32 net's death probability in millionths.
    pub death_ppm: i32,
}

/// The trained net's answer for the row of a unit (`danger::unit_features`).
pub fn predict(features: &[i32; FEATURES]) -> Danger {
    let weights = &danger_weights::WEIGHTS;
    let layers = weights.layers();
    infer(&Net { low: weights.low, high: weights.high, layers: &layers }, features)
}

/// The answer of a danger net for a row (`features[0]` is the hp).
pub fn infer(net: &Net<'_>, features: &[i32]) -> Danger {
    let mut out = [0i64; 2];
    net::forward(net, features, &mut out);
    let hp = i64::from(features[0].max(0));
    let damage = net::round_shift(out[0] * 100, ACT).clamp(0, hp);
    Danger { damage: damage as u16, death_permille: ((net::sigmoid_ppm(out[1]) + 500) / 1000) as u16 }
}

/// The f32 net's answers on a few test rows (`Reference`).
pub fn references() -> &'static [Reference] {
    &danger_weights::REFERENCE
}
