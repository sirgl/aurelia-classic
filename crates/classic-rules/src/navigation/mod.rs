//! The classic instance of `kernel::navigation` (generic over the rules module).

pub use kernel::navigation::*;
pub mod astar;
pub mod profile;

pub use astar::PathRequest;
pub use profile::MoveProfile;
