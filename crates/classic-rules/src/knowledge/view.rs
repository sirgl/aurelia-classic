//! The classic instance of `kernel::knowledge::view` (generic over the rules module).

pub use kernel::knowledge::view::*;

use crate::classic::Classic;

pub type PlayerView<'a> = kernel::knowledge::view::PlayerView<'a, Classic>;
