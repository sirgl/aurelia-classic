//! The AI bench (platform/docs/ai-bench.md): versions of the classic AI play
//! a suite of games against each other on the kernel's AI driver, one mind
//! per seat; the report says which version is stronger and by how much, with
//! a bootstrap over seed blocks for the spread. The recording mind
//! (`record`) writes a game's decisions and observations as a dataset.

pub mod battle;
pub mod bench;
pub mod choice;
pub mod danger;
pub mod e2;
pub mod export;
pub mod frames;
pub mod grounds;
pub mod nn;
pub mod record;
pub mod search;
pub mod selfplay;
pub mod site;
pub mod spec;

pub use spec::{GameSpec, WorldSize};

/// The 50th and 95th percentiles and the maximum of `values` (0 for none).
pub fn percentiles(values: &[f64]) -> (f64, f64, f64) {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let at = |share: usize| {
        if sorted.is_empty() {
            return 0.0;
        }
        sorted[(sorted.len() * share / 100).min(sorted.len() - 1)]
    };
    (at(50), at(95), sorted.last().copied().unwrap_or(0.0))
}
