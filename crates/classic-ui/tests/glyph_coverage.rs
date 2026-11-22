//! Every character of every shipped text has a glyph in the fonts the game installs for its
//! language: a character without one is drawn as a box (the attack line's arrow was).
//! The texts are the `.ftl` files of the platform and of classic; the fonts are the theme's with
//! the language's fallbacks (`client::shell::theme::install_fonts`), asked through egui itself so
//! that its own default fonts count too.
use client::shell::module::ModuleUi;
use classic_ui::ClassicUi;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const LANGUAGES: [&str; 7] = ["en", "de", "es", "fr", "ru", "ja", "zh-Hans"];

fn ftl_files(language: &str) -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for assets in [manifest.join("../../assets/lang"), manifest.join("../../../platform/assets/lang")] {
        let Ok(entries) = std::fs::read_dir(assets.join(language)) else { continue };
        files.extend(entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "ftl")));
    }
    files.sort();
    files
}

/// The characters of the message values of a Fluent file (keys and comments left out).
fn characters(file: &Path) -> BTreeSet<char> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut set = BTreeSet::new();
    for line in text.lines() {
        let value = if line.starts_with('#') {
            continue;
        } else if line.starts_with(char::is_whitespace) {
            line
        } else if let Some((_, value)) = line.split_once('=') {
            value
        } else {
            continue;
        };
        set.extend(value.chars().filter(|c| !c.is_control() && !c.is_whitespace()));
    }
    set
}

/// Characters the shipped CJK subsets still lack (they were cut from the platform's texts alone,
/// before the game's own were written: `platform/tools/i18n/subset_fonts.py` now reads every part).
/// A ratchet: the count may fall when the subsets are rebuilt from Noto Sans CJK, and never rise.
const CJK_BOXES_PINNED: [(&str, usize); 2] = [("ja", 79), ("zh-Hans", 134)];

#[test]
fn every_text_glyph_is_in_a_loaded_font() {
    let module = ClassicUi::game();
    let mut missing = Vec::new();
    for language in LANGUAGES {
        let ctx = egui::Context::default();
        let raw = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100., 100.))), ..Default::default() };
        // Fonts exist after the first frame; the install replaces them.
        let _ = ctx.run_ui(raw(), |_| {});
        assert!(client::shell::theme::install_fonts(&ctx, module.theme(), language), "the fonts of {language} are in the asset folder");
        let _ = ctx.run_ui(raw(), |_| {});
        let wanted: BTreeSet<char> = ftl_files(language).iter().flat_map(|f| characters(f)).collect();
        assert!(!wanted.is_empty(), "{language} has texts");
        let font = egui::FontId::proportional(14.);
        let lacking: Vec<char> = ctx.fonts_mut(|fonts| wanted.iter().copied().filter(|c| !fonts.has_glyph(&font, *c)).collect());
        let allowed = CJK_BOXES_PINNED.iter().find(|(l, _)| *l == language).map_or(0, |(_, n)| *n);
        if lacking.len() > allowed {
            missing.push(format!("{language}: {} lack a glyph (allowed {allowed}): {}", lacking.len(), lacking.iter().map(|c| format!("{c} U+{:04X}", *c as u32)).collect::<Vec<_>>().join(", ")));
        }
    }
    assert!(missing.is_empty(), "characters with no glyph in the installed fonts:\n{}", missing.join("\n"));
}
