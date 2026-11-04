//! Public write API: intent, not low-level mutation (§10).

// Module names follow platform/structure.md (§2: command/command.rs etc.);
// module_inception is deliberate here: structure beats the lint.
#![allow(clippy::module_inception)]

pub mod codec;
pub mod command;
pub mod resolved;
pub mod validate;

pub use command::{CityDecision, Command};
pub use kernel::command::*;
pub use resolved::ResolvedCommand;
pub use validate::{validate, NOT_YOUR_TURN};
