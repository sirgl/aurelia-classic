//! The classic instance of `kernel::effects::unit` (generic over the rules module).

pub use kernel::effects::unit::*;

use crate::classic::Classic;

pub type CombatTerm = kernel::effects::unit::CombatTerm<Classic>;
pub type StatTerm = kernel::effects::unit::StatTerm<Classic>;
pub type TerrainRule = kernel::effects::unit::TerrainRule<Classic>;
pub type UnitModifiers = kernel::effects::unit::UnitModifiers<Classic>;
