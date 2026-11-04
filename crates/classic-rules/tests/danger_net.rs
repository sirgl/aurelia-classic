//! The danger net (`ai::danger_net`): the integer inference answers as the
//! trained f32 net did on the reference rows kept with the weights (within
//! 1 hp of damage and 2 permille of death: the rounding of the answer and
//! the fixed point), every answer is in range, and the same row gets the
//! same answer.

use classic_rules::ai::danger::{FEATURES, NAMES};
use classic_rules::ai::danger_net::{predict, references};

#[test]
fn integer_inference_matches_the_f32_net() {
    let refs = references();
    assert!(refs.len() >= 20, "the weights keep their reference rows");
    for (i, r) in refs.iter().enumerate() {
        let answer = predict(&r.features);
        let damage_gap = (i32::from(answer.damage) * 1000 - r.damage_milli).abs();
        assert!(damage_gap <= 1000, "row {i}: damage {} hp against the f32 net's {} milli-hp", answer.damage, r.damage_milli);
        let death_gap = (i32::from(answer.death_permille) * 1000 - r.death_ppm).abs();
        assert!(death_gap <= 2000, "row {i}: death {} permille against the f32 net's {} ppm", answer.death_permille, r.death_ppm);
    }
    let spread = refs.iter().map(|r| r.damage_milli).max().unwrap_or(0) - refs.iter().map(|r| r.damage_milli).min().unwrap_or(0);
    assert!(spread >= 10_000, "the reference rows span the damage");
}

/// Rows from a fixed generator: every value within -50..=250 and the
/// extremes of the i32, so inputs outside the training range are clamped.
fn rows() -> Vec<[i32; FEATURES]> {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut out = Vec::new();
    for _ in 0..2000 {
        let mut row = [0i32; FEATURES];
        for v in row.iter_mut() {
            *v = (next() % 301) as i32 - 50;
        }
        out.push(row);
    }
    out.push([i32::MAX; FEATURES]);
    out.push([i32::MIN; FEATURES]);
    out.push([0; FEATURES]);
    out
}

#[test]
fn answers_are_in_range() {
    let hp = NAMES.iter().position(|n| *n == "hp").expect("an hp feature");
    for row in rows() {
        let answer = predict(&row);
        assert!(i32::from(answer.damage) <= row[hp].max(0), "{row:?}: damage {} over hp", answer.damage);
        assert!(answer.death_permille <= 1000, "{row:?}: death {}", answer.death_permille);
    }
    for r in references() {
        assert!(i32::from(predict(&r.features).damage) <= r.features[hp]);
    }
}

#[test]
fn the_same_row_gets_the_same_answer() {
    let first: Vec<_> = rows().iter().map(predict).collect();
    let second: Vec<_> = rows().iter().map(predict).collect();
    assert_eq!(first, second);
}
