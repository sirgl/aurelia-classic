//! The classic ruleset's instance of `kernel::derived::citizens` (generic over the rules module).

pub use kernel::derived::citizens::*;

use crate::classic::Classic;

pub type EmpireContext = kernel::derived::citizens::EmpireContext<Classic>;
