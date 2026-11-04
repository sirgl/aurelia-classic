//! The classic instance of `kernel::effects::city` (generic over the rules module).

pub use kernel::effects::city::*;

use crate::classic::Classic;

pub type CityModifiers = kernel::effects::city::CityModifiers<Classic>;
pub type TileEffects = kernel::effects::city::TileEffects<Classic>;
