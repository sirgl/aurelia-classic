//! The only point of authoritative mutation (§13).
//!
//! After a successful transaction AuthWorld is valid; on reject it is unchanged.
//! Strategy: validate everything -> infallible mutation (without copying the world).

// Module names follow platform/structure.md (§2); module_inception is deliberate.
#![allow(clippy::module_inception)]

pub use journal::{ChangeJournal, CommandResult};
pub mod journal;
pub mod transaction;
