//! The classic screens and HUD panels. Widgets draw from read-only view models and return
//! typed requests; they never mutate the game (`game::Session` applies the requests).
pub mod shortcuts;
pub mod hud;
pub mod model;
pub mod overview_model;
pub mod pedia;
pub mod pedia_link;
pub mod screens;
pub mod unit_kinds;
pub mod world_model;
