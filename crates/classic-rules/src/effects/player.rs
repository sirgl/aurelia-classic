//! The classic instance of `kernel::effects::player` (generic over the rules module).

pub use kernel::effects::player::*;

use crate::classic::Classic;

pub type PlayerModifiers = kernel::effects::player::PlayerModifiers<Classic>;
