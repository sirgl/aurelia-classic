//! The site net (`ai::site_net`): the integer inference answers as the
//! trained f32 net did on the reference rows kept with the weights (within
//! 2 thousandths of a citizen: the rounding of the answer and the fixed
//! point), the references span the answers, every answer stays in a sane
//! range, and the same row gets the same answer.

use classic_rules::ai::site::{SITE_FEATURES, SITE_NAMES};
use classic_rules::ai::site_net::{planner_score, predict, references};

#[test]
fn integer_inference_matches_the_f32_net() {
    let refs = references();
    assert!(refs.len() >= 20, "the weights keep their reference rows");
    for (i, r) in refs.iter().enumerate() {
        let answer = i64::from(predict(&r.features));
        assert!((answer - r.output_milli).abs() <= 2, "row {i}: {answer} against the f32 net's {}", r.output_milli);
    }
    let spread = refs.iter().map(|r| r.output_milli).max().unwrap_or(0) - refs.iter().map(|r| r.output_milli).min().unwrap_or(0);
    assert!(spread >= 3_000, "the reference rows span three citizens or more ({spread})");
}

/// Rows from a fixed generator: every value within -20..=120 and the
/// extremes of the i32, so inputs outside the training range are clamped.
fn rows() -> Vec<[i32; SITE_FEATURES]> {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut out = Vec::new();
    for _ in 0..2000 {
        let mut row = [0i32; SITE_FEATURES];
        for v in row.iter_mut() {
            *v = (next() % 141) as i32 - 20;
        }
        out.push(row);
    }
    out.push([i32::MAX; SITE_FEATURES]);
    out.push([i32::MIN; SITE_FEATURES]);
    out.push([0; SITE_FEATURES]);
    out
}

#[test]
fn answers_are_in_range() {
    // Inputs are clamped to the training range, so any row, however wild
    // (these mix every feature at random, unlike any real site), gets a
    // bounded answer; the real rows get answers among the populations seen
    // (0..=12 citizens) with a margin.
    let answers: Vec<i32> = rows().iter().map(predict).collect();
    let (low, high) = (answers.iter().min().copied().unwrap_or(0), answers.iter().max().copied().unwrap_or(0));
    assert!(low >= -30_000 && high <= 60_000, "answers {low}..={high}");
    for r in references() {
        let answer = predict(&r.features);
        assert!((-1_000..=15_000).contains(&answer), "{:?}: {answer}", r.features);
    }
}

#[test]
fn the_same_row_gets_the_same_answer() {
    let first: Vec<i32> = rows().iter().map(predict).collect();
    let second: Vec<i32> = rows().iter().map(predict).collect();
    assert_eq!(first, second);
    assert!(first.windows(2).any(|w| w[0] != w[1]), "the answers depend on the row");
}

#[test]
fn a_better_site_nature_scores_higher() {
    // The reference row with the lowest answer, its nature yields raised to
    // those of the highest: the net reads the land (the nature group is
    // the one it needs most, docs/ai-site.md).
    let refs = references();
    let low = refs.iter().min_by_key(|r| r.output_milli).expect("references");
    let high = refs.iter().max_by_key(|r| r.output_milli).expect("references");
    let mut better = low.features;
    for (i, name) in SITE_NAMES.iter().enumerate() {
        if name.starts_with("nature_") {
            better[i] = better[i].max(high.features[i]);
        }
    }
    assert!(predict(&better) > predict(&low.features), "{} against {}", predict(&better), predict(&low.features));
}

#[test]
fn the_planner_score_grows_with_the_answer() {
    // A linear map with a positive slope: more citizens, more points; the
    // reference rows keep their order.
    assert!(planner_score(1_000) > planner_score(0));
    assert_eq!(planner_score(2_000) - planner_score(1_000), planner_score(1_000) - planner_score(0));
    let mut answers: Vec<i32> = references().iter().map(|r| predict(&r.features)).collect();
    answers.sort_unstable();
    for pair in answers.windows(2) {
        assert!(planner_score(pair[0]) <= planner_score(pair[1]));
    }
}
