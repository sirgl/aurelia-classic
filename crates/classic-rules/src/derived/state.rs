//! The classic ruleset's instance of `kernel::derived::state` (generic over the rules module).


use crate::classic::Classic;

pub type DerivedWorld = kernel::derived::state::DerivedWorld<Classic>;
