//! The classic ruleset's instance of `kernel::derived::yields` (generic over the rules module).

pub use kernel::derived::yields::*;

use crate::classic::Classic;

pub type CityBreakdown = kernel::derived::yields::CityBreakdown<Classic>;

#[cfg(test)]
#[path = "yields_tests.rs"]
mod tests;
