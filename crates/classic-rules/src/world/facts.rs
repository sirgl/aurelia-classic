//! The classic instance of `kernel::world::facts` (generic over the rules module).

pub use kernel::world::facts::*;

use crate::classic::Classic;

pub type TruthFacts<'a> = kernel::world::facts::TruthFacts<'a, Classic>;
