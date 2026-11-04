//! Classic developer tooling: offscreen captures and benchmarks of the real game loop
//! (`headless`), the scripted scenes they stage (`world_setup`, `showcase`), and the
//! HUD's overlap check over a match (`hud_check`).
pub mod art_bench;
pub mod headless;
pub mod hud_check;
pub mod scene_bench;
pub mod showcase;
pub mod world_setup;

/// The game loop the offscreen runs drive: the shell's frame with the classic module.
pub type GameApp = client::shell::app::App<crate::ClassicUi>;
