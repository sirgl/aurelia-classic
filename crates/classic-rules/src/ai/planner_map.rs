//! The classic-ruleset instance of `kernel::ai::map` (the AI's map search over the
//! player's knowledge).

pub use kernel::ai::map::*;

use crate::classic::Classic;

pub type KnownMap<'v, 'a> = kernel::ai::map::KnownMap<'v, 'a, Classic>;
