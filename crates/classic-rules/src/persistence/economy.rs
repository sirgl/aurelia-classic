//! The classic instance of `kernel::persistence::economy` (generic over the
//! rules module), with the classic player record (`classic::player`).

pub use kernel::persistence::economy::*;

pub use crate::classic::player::{SavedPlayerEconomy, FREE_TECH_ANY};

use crate::classic::Classic;

pub type SavedEconomy = kernel::persistence::economy::SavedEconomy<Classic>;
