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
