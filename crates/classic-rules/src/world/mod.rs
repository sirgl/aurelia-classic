//! The classic instance of `kernel::world` (generic over the rules module).

pub use kernel::world::*;
pub mod barbarians;
pub mod city_states;
pub mod congress;
pub mod culture;
pub mod effects;
pub mod espionage;
pub mod facts;
pub mod military;
pub mod player_economy;
pub mod module;
pub mod player;
pub mod relations;
pub mod religion;
pub mod state;
pub mod tourism;
pub mod trade;
pub mod victory;

pub use state::AuthWorld;
pub use state::GameState;
pub use state::GameView;

/// Whether a puppet city is in resistance after its capture as an annexed one is (the owner's
/// decision on the rule; Civ V: a captured city resists whether puppeted or annexed). Off:
/// the Puppet decision ends the resistance at once. Priced by the AI's capture estimate
/// (`ai::operations::captured_parts_by`, `ai::conquest::priced_case`).
pub const PUPPET_RESISTS: bool = false;
