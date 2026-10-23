//! The running match without any window or GPU: `Session` owns the `GameClient` and the
//! `SimulationDriver`, turns UI requests into rules commands and builds the UI model.
mod air_paint;
mod city;
mod city_states;
mod civics;
mod combat;
mod congress;
pub(crate) mod cues;
mod culture;
mod diplomacy;
mod espionage;
pub(crate) mod names;
mod observed_city;
mod orders;
mod overview;
mod path;
pub(crate) mod reasons;
pub(crate) mod reasons_rules;
mod religion;
mod research;
mod resources;
mod ruins;
mod session;
mod trade;
mod turn;
mod victory;

#[cfg(test)]
mod presentation_tests;
#[cfg(test)]
mod reasons_rules_tests;
#[cfg(test)]
mod reasons_tests;
#[cfg(test)]
mod tests;

pub use client::shell::module::{AiProgress, MapButton};
pub use observed_city::ObservedCity;
pub use session::{decode_game, decode_save, Session, ai_turns_message};
