//! classic-rules: the classic rules module (platform/PLATFORM.md): its world state,
//! systems, combat, derived mechanics, save sections, AI and effect words,
//! and the kernel instantiated with it.
//!
//! The kernel's modules appear here under their usual paths: the generic
//! types as aliases over `Classic` (`world::state::GameState` is
//! `kernel::world::state::GameState<Classic>`), everything else re-exported.
//! Forbidden in library code: render, classic-content, client, wgpu,
//! platform APIs. Tests and examples read content through dev-deps.

pub use kernel::{math, metrics, numeric, rng, turn, validation};

pub mod ai;
pub mod classic;
pub mod combat;
pub mod command;
pub mod derived;
pub mod effects;
pub mod game;
pub mod ids;
pub mod knowledge;
pub mod module;
pub mod navigation;
pub mod persistence;
pub mod replay;
pub mod rules;
pub mod save_codec;
pub mod scenario;
pub(crate) mod systems;
pub mod transaction;
pub mod world;

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod kernel_tests;

// Starting API (§53 platform/structure.md):
pub use command::Command;
pub use game::ClassicGame;
pub use knowledge::public::PlayerQueries;
pub use systems::research::{plan_research, technology_cost, ResearchPlanError};
pub use rules::{compile_rules, CompiledRuleset};
pub use systems::ruins;
pub use world::state::{GameConfig, GameState, GameView};
