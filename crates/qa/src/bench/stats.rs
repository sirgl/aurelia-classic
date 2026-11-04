//! The statistics of the bench: the seed-block bootstrap of a per-game
//! mean, and the mean of a sample with its 95 % interval.
//!
//! Games of one seed are not independent: they share the nations
//! (`kernel::scenario::generator::assign_nations` draws them from the seed
//! alone, on every map) and, head to head, the same world in mirrored
//! seatings. So the unit of resampling is a seed block — every game of one
//! seed, both seatings, kept together — and a binomial interval over games
//! (Wilson) is wrong in a direction nobody knows in advance. The design
//! effect says which way: the block variance over the binomial one.

use serde::{Deserialize, Serialize};

/// z of a two-sided 95 % interval.
pub const Z95: f64 = 1.96;

/// Resamples of every bootstrap, and the seed of their generator: a report
/// is the same on every run.
pub const RESAMPLES: usize = 10_000;
const BOOTSTRAP_SEED: u64 = 0x0b00_7572_a9b1_0c45;

/// One seed block: the sum of a per-game value over its games.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BlockSum {
    pub total: f64,
    pub games: u32,
}

/// A per-game mean over seed blocks with its 95 % percentile bootstrap
/// interval and the bootstrap variance of the mean.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockEstimate {
    pub mean: f64,
    pub low: f64,
    pub high: f64,
    pub variance: f64,
    pub blocks: u32,
}

impl BlockEstimate {
    /// Resamples whole blocks with replacement (`RESAMPLES` times, a fixed
    /// generator); each resample's mean is its games' total over its games.
    pub fn of(blocks: &[BlockSum]) -> Self {
        let Some(mean) = ratio(blocks.iter()) else { return Self::default() };
        let mut rng = SplitMix64(BOOTSTRAP_SEED);
        let mut means: Vec<f64> = (0..RESAMPLES)
            .map(|_| ratio((0..blocks.len()).map(|_| &blocks[rng.below(blocks.len())])).unwrap_or(mean))
            .collect();
        means.sort_by(f64::total_cmp);
        let average = means.iter().sum::<f64>() / RESAMPLES as f64;
        let variance = means.iter().map(|m| (m - average).powi(2)).sum::<f64>() / (RESAMPLES - 1) as f64;
        Self {
            mean,
            low: means[RESAMPLES * 25 / 1000],
            high: means[RESAMPLES * 975 / 1000 - 1],
            variance,
            blocks: blocks.len() as u32,
        }
    }

    /// The bootstrap variance over the variance of a share of `games`
    /// independent trials: above 1, a per-game interval is too narrow;
    /// below 1, too wide. None when the share is 0 or 1.
    pub fn design_effect(&self, games: u32) -> Option<f64> {
        let binomial = self.mean * (1.0 - self.mean) / f64::from(games.max(1));
        (binomial > 0.0).then(|| self.variance / binomial)
    }
}

fn ratio<'a>(blocks: impl Iterator<Item = &'a BlockSum>) -> Option<f64> {
    let (total, games) = blocks.fold((0.0, 0u32), |(t, g), b| (t + b.total, g + b.games));
    (games > 0).then(|| total / f64::from(games))
}

/// SplitMix64: the bench's own generator (no crate; the game's RNG is not
/// for tools).
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (multiply-shift; the bias is below 2⁻⁵⁰ here).
    fn below(&mut self, n: usize) -> usize {
        ((u128::from(self.next()) * n as u128) >> 64) as usize
    }
}

/// Mean, standard deviation and size of a sample.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    pub mean: f64,
    pub sd: f64,
    pub n: u32,
}

impl Estimate {
    pub fn of(values: impl IntoIterator<Item = f64>) -> Self {
        let values: Vec<f64> = values.into_iter().collect();
        let n = values.len();
        if n == 0 {
            return Self::default();
        }
        let mean = values.iter().sum::<f64>() / n as f64;
        let variance = if n > 1 {
            values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64
        } else {
            0.0
        };
        Self { mean, sd: variance.sqrt(), n: n as u32 }
    }

    /// Half the width of the 95 % interval of the mean.
    pub fn half_width(&self) -> f64 {
        if self.n == 0 {
            return 0.0;
        }
        Z95 * self.sd / f64::from(self.n).sqrt()
    }

    /// This mean minus `other`'s, with the half width of its 95 % interval
    /// (Welch: the samples are independent, their variances may differ).
    pub fn minus(&self, other: &Estimate) -> (f64, f64) {
        let variance = |e: &Estimate| if e.n == 0 { 0.0 } else { e.sd * e.sd / f64::from(e.n) };
        (self.mean - other.mean, Z95 * (variance(self) + variance(other)).sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Half the width of the normal interval of a share of independent
    /// games: what the bench used to report.
    fn binomial_half_width(share: f64, games: u32) -> f64 {
        Z95 * (share * (1.0 - share) / f64::from(games)).sqrt()
    }

    fn blocks(points: &[f64], games: u32) -> Vec<BlockSum> {
        points.iter().map(|&total| BlockSum { total, games }).collect()
    }

    /// Every game of a seed goes the same way (one nation lineup decides):
    /// 200 games are really 20 trials, and the interval is wider.
    #[test]
    fn correlated_blocks_widen_the_interval() {
        let points: Vec<f64> = (0..20).map(|i| if i % 2 == 0 { 10.0 } else { 0.0 }).collect();
        let e = BlockEstimate::of(&blocks(&points, 10));
        assert_eq!((e.mean, e.blocks), (0.5, 20));
        let deff = e.design_effect(200).unwrap();
        assert!((8.0..11.0).contains(&deff), "design effect {deff}");
        assert!(e.high - e.low > 2.5 * 2.0 * binomial_half_width(0.5, 200), "{e:?}");
    }

    /// Mirrored seatings: in almost every block each version wins once,
    /// so the share varies less than for independent games.
    #[test]
    fn mirrored_blocks_narrow_the_interval() {
        let mut points = vec![1.0; 36];
        points.extend([2.0, 0.0, 2.0, 0.0]);
        let e = BlockEstimate::of(&blocks(&points, 2));
        assert_eq!(e.mean, 0.5);
        let deff = e.design_effect(80).unwrap();
        assert!(deff < 0.3, "design effect {deff}");
        assert!(e.high - e.low < 0.6 * 2.0 * binomial_half_width(0.5, 80), "{e:?}");
    }

    /// Independent games in blocks: the design effect is about 1.
    #[test]
    fn independent_games_keep_the_binomial_variance() {
        let mut rng = SplitMix64(7);
        let points: Vec<f64> = (0..200).map(|_| (0..2).filter(|_| rng.below(2) == 1).count() as f64).collect();
        let e = BlockEstimate::of(&blocks(&points, 2));
        let deff = e.design_effect(400).unwrap();
        assert!((0.8..1.25).contains(&deff), "design effect {deff}");
    }

    #[test]
    fn the_bootstrap_is_reproducible() {
        let points = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];
        let e = BlockEstimate::of(&blocks(&points, 10));
        assert_eq!(e, BlockEstimate::of(&blocks(&points, 10)));
        assert!(e.low < e.mean && e.mean < e.high);
        assert_eq!(BlockEstimate::of(&[]), BlockEstimate::default());
        assert_eq!(BlockEstimate::of(&blocks(&[2.0; 5], 4)).design_effect(20), Some(0.0));
        assert_eq!(BlockEstimate::of(&blocks(&[4.0; 5], 4)).design_effect(20), None);
    }

    #[test]
    fn an_estimate_has_the_sample_deviation() {
        let e = Estimate::of([2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]);
        assert_eq!(e.mean, 5.0);
        assert!((e.sd - 2.138).abs() < 1e-3);
        let (difference, half) = e.minus(&Estimate { mean: 4.0, sd: 0.0, n: 1 });
        assert_eq!(difference, 1.0);
        assert!((half - Z95 * 2.138 / 8f64.sqrt()).abs() < 1e-3);
    }
}
