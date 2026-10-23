//! The classic module's user interface: the running match (`game::Session`), the screens of
//! its mechanics and their view models (`ui`), its HUD panels, keyboard and tooltips, and
//! the classic developer scenes (`dev`). `ClassicUi` is what the app shell talks to
//! (`client::shell::module::ModuleUi`, implemented in `module`).
#[cfg(any(test, feature = "dev-tools"))]
pub mod dev;
pub mod game;
mod module;
pub mod observer;
mod observer_cards;
pub mod ui;

pub use module::ClassicUi;

/// The terrain texture layers (width, height, RGBA), for the renderer: the platform's
/// ground (`render::ground`), shared with the other modules.
pub fn terrain_layers() -> Vec<(u32, u32, Vec<u8>)> {
    render::ground::layers()
}
