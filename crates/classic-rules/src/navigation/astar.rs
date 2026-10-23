//! The classic instance of `kernel::navigation::astar` (generic over the rules module).

pub use kernel::navigation::astar::*;

use crate::classic::Classic;

pub type PathRequest = kernel::navigation::astar::PathRequest<Classic>;
