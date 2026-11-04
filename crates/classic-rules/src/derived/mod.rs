//! Derived state: restorable from auth + rules (§15).
//! Contract: rebuild(auth) == incremental(auth, prev, changes) logically.

pub use kernel::derived::*;

pub mod archaeology;
pub mod citizens;
pub mod culture;
pub mod grants;
pub mod great_people;
pub mod happiness;
pub mod income;
pub mod maintenance;
pub mod religion;
pub mod research;
pub mod resources;
pub mod ruins;
pub mod state;
pub mod tourism;
pub mod trade;
pub mod trade_path;
pub mod yields;

pub use state::DerivedWorld;
