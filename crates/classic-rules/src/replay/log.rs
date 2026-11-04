//! The classic instance of `kernel::replay::log` (generic over the rules module).


use crate::classic::Classic;

pub type ReplayEntry = kernel::replay::log::ReplayEntry<Classic>;
pub type Replay = kernel::replay::log::Replay<Classic>;
