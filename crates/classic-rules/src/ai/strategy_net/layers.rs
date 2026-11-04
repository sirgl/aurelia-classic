//! The integer arithmetic of the strategy net: a neuron is a bias and a few
//! weighted inputs, all in thousandths (`Milli`: 1000 is 1.0), clamped to
//! 0…1 (a concept) or left linear (a logit); a softmax by a table of
//! `e^(−k/8)`; the hysteresis of a choice. No float anywhere: the same
//! answer on every platform.

use super::weights::{Act, Input, Neuron};
use super::Milli;

/// 1.0 in thousandths.
pub const ONE: Milli = 1000;

/// `x` clamped to 0…1.
pub fn unit(x: Milli) -> Milli {
    x.clamp(0, ONE)
}

/// `value` mapped linearly from `lo` (0) to `hi` (1), clamped.
pub fn ramp(value: i64, lo: i64, hi: i64) -> Milli {
    if hi == lo {
        return if value >= hi { ONE } else { 0 };
    }
    unit((value - lo) * ONE / (hi - lo))
}

/// One neuron over the features and the concepts computed so far.
pub fn fire(neuron: &Neuron, features: &[Milli], concepts: &[Milli]) -> Milli {
    let mut sum = neuron.bias;
    for term in neuron.terms {
        let x = match term.input {
            Input::F(i) => features[i],
            Input::K(i) => concepts[i],
        };
        sum += term.w * x / ONE;
    }
    match neuron.act {
        Act::Unit => unit(sum),
        Act::Linear => sum,
    }
}

/// `e^(−k/8)` in millionths for k = 0…64.
const EXP: [i64; 65] = [
    1000000, 882497, 778801, 687289, 606531, 535261, 472367, 416862, 367879, 324652, 286505, 252840, 223130, 196912, 173774, 153355, 135335, 119433, 105399, 93014, 82085,
    72440, 63928, 56416, 49787, 43937, 38774, 34218, 30197, 26649, 23518, 20754, 18316, 16163, 14264, 12588, 11109, 9804, 8652, 7635, 6738, 5946, 5248, 4631, 4087, 3607,
    3183, 2809, 2479, 2187, 1930, 1704, 1503, 1327, 1171, 1033, 912, 805, 710, 627, 553, 488, 431, 380, 335,
];

/// `e^(x/1000)` in millionths for `x` ≤ 0 (linear between the table's
/// steps of 1/8; 0 below −8).
pub fn exp_neg(x: Milli) -> i64 {
    let d = (-x).max(0);
    let step = d / 125;
    if step >= 64 {
        return 0;
    }
    let rest = d % 125;
    let (a, b) = (EXP[step as usize], EXP[step as usize + 1]);
    a - (a - b) * rest / 125
}

/// The softmax of logits (thousandths), as thousandths summing to about
/// 1000 (the largest share takes the rounding).
pub fn softmax<const N: usize>(logits: &[Milli; N]) -> [Milli; N] {
    let top = logits.iter().copied().max().unwrap_or(0);
    let weights: Vec<i64> = logits.iter().map(|l| exp_neg(l - top)).collect();
    let total: i64 = weights.iter().sum::<i64>().max(1);
    let mut out = [0; N];
    for (o, w) in out.iter_mut().zip(&weights) {
        *o = w * ONE / total;
    }
    let rest = ONE - out.iter().sum::<i64>();
    if let Some(best) = (0..N).max_by_key(|&i| (out[i], std::cmp::Reverse(i))) {
        out[best] += rest;
    }
    out
}

/// The index of the largest share (the lower index on a tie).
pub fn argmax(values: &[Milli]) -> usize {
    (0..values.len()).max_by_key(|&i| (values[i], std::cmp::Reverse(i))).unwrap_or(0)
}

/// A choice with hysteresis: the one held (`held`) stays unless the best
/// leads it by more than `margin` thousandths of its share (1150: 15 %).
pub fn hold(shares: &[Milli], held: Option<usize>, margin: Milli) -> usize {
    let best = argmax(shares);
    match held {
        Some(h) if h < shares.len() && best != h && shares[best] * ONE <= shares[h] * margin => h,
        _ => best,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn softmax_sums_to_one_and_orders() {
        let p = softmax(&[2000, 0, -1000]);
        assert_eq!(p.iter().sum::<i64>(), 1000);
        assert!(p[0] > p[1] && p[1] > p[2]);
        // e^2 : 1 : e^-1 = 7.389 : 1 : 0.368 → 0.844, 0.114, 0.042.
        assert!((p[0] - 844).abs() <= 3, "{p:?}");
    }

    #[test]
    fn hysteresis_keeps_a_close_choice() {
        assert_eq!(hold(&[520, 480], Some(1), 1150), 1, "52 against 48 is not 15 % more");
        assert_eq!(hold(&[600, 400], Some(1), 1150), 0);
        assert_eq!(hold(&[600, 400], None, 1150), 0);
    }

    #[test]
    fn exp_is_monotone() {
        let mut last = i64::MAX;
        for x in (-8000..=0).rev().step_by(37) {
            let e = exp_neg(x);
            assert!(e <= last);
            last = e;
        }
        assert_eq!(exp_neg(0), 1_000_000);
    }
}
