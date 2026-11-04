//! The classic instance of `kernel::world::effects` (generic over the rules module).

pub use kernel::world::effects::*;

use crate::classic::Classic;

pub type EffectRef = kernel::world::effects::EffectRef<Classic>;
pub type ActiveEffect = kernel::world::effects::ActiveEffect<Classic>;
pub type ActiveEffectStore = kernel::world::effects::ActiveEffectStore<Classic>;
