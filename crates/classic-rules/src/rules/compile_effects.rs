//! The classic instance of `kernel::rules::compile_effects` (generic over the rules module).

pub use kernel::rules::compile_effects::*;

use crate::classic::Classic;

pub type SourceKind = kernel::rules::compile_effects::SourceKind<Classic>;
