//! The classic instance of `kernel::rules` (generic over the rules module).

pub use kernel::rules::*;
pub mod compile_effects;
pub mod compiled;
pub mod content;
pub mod effects;
pub mod roles;
pub mod source;
pub mod stats;
pub mod vocabulary;

pub use compiled::CompiledRuleset;
pub use source::SourceRules;
