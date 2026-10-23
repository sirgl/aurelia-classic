//! The classic instance of `kernel::world::state` (generic over the rules module).

pub use kernel::world::state::*;

use crate::classic::Classic;

pub type AuthWorld = kernel::world::state::AuthWorld<Classic>;
pub type GameState = kernel::world::state::GameState<Classic>;
pub type GameView<'a> = kernel::world::state::GameView<'a, Classic>;
