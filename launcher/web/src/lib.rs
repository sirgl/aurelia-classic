//! Aurelia in a browser page. The page, the canvas and the art service are `launcher_core::web`; this
//! crate registers the module and writes the page API (`#[wasm_bindgen]` functions) with
//! `web_exports!`. See lands/docs/web.md.
#![cfg(target_arch = "wasm32")]

launcher_core::web_exports!(|| (classic_ui::ClassicUi::game(), classic_ui::terrain_layers()));
