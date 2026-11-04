//! The running match without any window or GPU: `Session` owns the `GameClient` and the
//! `SimulationDriver`, turns UI requests into rules commands and builds the UI model.
mod air_paint;
mod camps;
mod city;
pub(crate) mod city_states;
mod civics;
mod combat;
mod congress;
pub(crate) mod cues;
mod culture;
mod diplomacy;
mod end_gate;
mod espionage;
mod income;
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
mod save_header;
mod savings;
mod session;
mod stack_click;
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
mod stack_click_tests;
#[cfg(test)]
mod tests;

pub use client::shell::module::{AiProgress, MapButton};
pub use end_gate::Choice;
pub use observed_city::ObservedCity;
pub use save_header::describe as describe_save;
pub use session::{decode_game, decode_save, Session, ai_turns_message};
