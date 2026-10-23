//! Reproducible worlds: the kernel's generator (`kernel::scenario`) and the
//! classic setup of a new game (city-states, barbarians, victories).

pub use kernel::scenario::{generator, landscape, map_script};

pub mod city_states;
pub mod new_game;
pub mod ruins;

pub use kernel::scenario::{
    generate, generate_new_game, generate_player_game, generate_player_game_scripted, generate_scripted,
    settler_kind, MapScript, ScenarioConfig,
};
pub use new_game::{generate_new_game_with, NewGameOptions};

#[cfg(test)]
#[path = "generator_tests.rs"]
mod generator_tests;
