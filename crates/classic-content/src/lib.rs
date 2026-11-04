//! classic-content: disk/HTTP/embed -> source files -> SourceRules (§28).
//! Knows files/paths/RON/manifests. classic-rules does not.
//! Semantic validation is in core (compile), parsing is here (§29).

pub mod classic;
pub mod nations;
pub mod core;
pub mod effects;
pub mod error;
#[cfg(feature = "importer")]
pub mod import;
pub mod legacy;
pub mod loader;
pub mod manifest;
pub mod ron;
mod natural_wonders;
mod ron_roles;
mod ron_types;
mod stats;

pub use loader::load_source_rules;
