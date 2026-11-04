//! The classic instance of `kernel::navigation::profile` (generic over the rules module).

pub use kernel::navigation::profile::*;

use crate::classic::Classic;

pub type MoveProfile = kernel::navigation::profile::MoveProfile<Classic>;
