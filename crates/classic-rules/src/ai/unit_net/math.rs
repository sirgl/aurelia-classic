//! The integer arithmetic of the unit net, in thousandths (`Milli`): the
//! sigmoid and tanh by the strategy net's table of `e^(−k/8)`, the chance
//! that summed blows reach a health from the previews' (min, expected, max).
//! No float anywhere: the same answer on every platform.

use super::Milli;
use crate::ai::strategy_net::layers::exp_neg;

pub use crate::ai::strategy_net::layers::{hold, ramp, unit, ONE};

/// σ(x) for `x` in thousandths, as thousandths.
pub fn sigmoid(x: Milli) -> Milli {
    if x > 0 {
        return ONE - sigmoid(-x);
    }
    // e^x in millionths (x ≤ 0): σ = e / (1 + e).
    let e = exp_neg(x);
    e * ONE / (e + 1_000_000)
}

/// tanh(x) = 2σ(2x) − 1.
pub fn tanh(x: Milli) -> Milli {
    2 * sigmoid(2 * x) - ONE
}

/// σ((x − at) · gain): a soft threshold.
pub fn step(x: Milli, at: Milli, gain: Milli) -> Milli {
    sigmoid((x - at) * gain / ONE)
}

/// σ((x − at) / width).
pub fn soft(x: Milli, at: Milli, width: Milli) -> Milli {
    sigmoid((x - at) * ONE / width.max(1))
}

/// x · y in thousandths.
pub fn mul(x: Milli, y: Milli) -> Milli {
    x * y / ONE
}

/// The chance (thousandths) that blows summing (min, expected, max) reach
/// `hp`: 1 when even the least does, 0 when even the most does not, else
/// linear from the least to the expected (0 … ½) and from the expected to
/// the most (½ … 1) — the previews' rolls, a triangle's median at the
/// expected damage.
pub fn reach_chance(min: i64, expected: i64, max: i64, hp: i64) -> Milli {
    if hp <= 0 || min >= hp {
        return ONE;
    }
    if max < hp {
        return 0;
    }
    if hp <= expected {
        // Between the least and the expected: ½ … 1.
        let span = (expected - min).max(1);
        ONE / 2 + (expected - hp) * (ONE / 2) / span
    } else {
        let span = (max - expected).max(1);
        (max - hp + 1) * (ONE / 2) / span
    }
    .clamp(0, ONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigmoid_is_symmetric_and_monotone() {
        assert_eq!(sigmoid(0), 500);
        assert!((sigmoid(2000) + sigmoid(-2000) - ONE).abs() <= 1);
        assert!((sigmoid(2000) - 881).abs() <= 3, "{}", sigmoid(2000));
        let mut last = -1;
        for x in (-9000..9000).step_by(97) {
            let s = sigmoid(x);
            assert!(s >= last);
            last = s;
        }
        assert!((tanh(1000) - 762).abs() <= 3, "{}", tanh(1000));
    }

    #[test]
    fn reach_chance_follows_the_rolls() {
        assert_eq!(reach_chance(30, 40, 50, 25), ONE);
        assert_eq!(reach_chance(30, 40, 50, 60), 0);
        assert_eq!(reach_chance(30, 40, 50, 40), 500);
        assert!(reach_chance(30, 40, 50, 35) > 500);
        assert!(reach_chance(30, 40, 50, 45) < 500);
    }
}
