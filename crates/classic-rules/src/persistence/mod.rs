//! Simulation save schema, not a filesystem API (§24).
//! GameState <-> Save representation; where to write is decided by the host/client.

pub use kernel::persistence::*;

pub mod culture;
pub mod economy;
pub mod city_states;
pub mod congress;
pub mod espionage;
pub mod relations;
pub mod victory;
pub mod religion;
pub mod schema;
pub mod tourism;
pub mod trade;

pub use schema::{SaveGameV1, SavedRelations, SavedWorldV1};
