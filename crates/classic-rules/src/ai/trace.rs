//! The classic-ruleset instance of `kernel::ai::trace` (the AI's decision trace and
//! counters).

pub use kernel::ai::trace::*;

use crate::classic::Classic;

pub type TraceEntry = kernel::ai::trace::TraceEntry<Classic>;
pub type DecisionTrace = kernel::ai::trace::DecisionTrace<Classic>;
