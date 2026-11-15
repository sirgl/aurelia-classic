//! Text snapshots of the classic screens (`client::shell::snapshot`): the main menu, and
//! after the opening of a small match (the capital founded, a Warrior queued, the first
//! technology chosen) the HUD over the map, the city, research and victory screens.
//! A changed screen fails with a line diff; `SNAPSHOT_UPDATE=1` accepts it.
//!
//! The snapshots are drawn WITH the art pack (`assets/visual/generated`, fetched, not in git: the
//! item cards take their art key and icon from it). `SNAPSHOT_UPDATE=1` refuses to write without
//! the pack (`.pack-version` missing): copy it from another clone (`cp -c -R`) first.
use classic_ui::ui::model::{Action, GameRequest, TechStatus};
use classic_ui::ui::screens::{prepare_match, MapSize, MatchRequest, Screen};
use classic_ui::ClassicUi;
use client::shell::module::{ModuleUi, Screen as ShellScreen};
use client::shell::snapshot::{assert_snapshot, snapshot_file, Stage, UPDATE_VARIABLE};
use std::path::Path;

const SIZE: egui::Vec2 = egui::vec2(1366.0, 768.0);

/// The reason an update must not write, when it is asked for (`update`) and the pack's
/// `.pack-version` file is not at `pack_version`: a snapshot written without the pack carries the
/// fallback art (no icons, `bnw:` keys), which the game with its pack never draws.
fn update_refusal(update: bool, pack_version: &Path) -> Option<String> {
    if !update || pack_version.exists() {
        return None;
    }
    Some(format!(
        "{UPDATE_VARIABLE}=1 refused: no art pack ({} is missing), so the snapshots would be written with the fallback art. Copy assets/visual/generated from a clone that has it (cp -c -R), then rerun.",
        pack_version.display()
    ))
}

fn check(stage: &mut Stage<ClassicUi>, name: &str) {
    let update = std::env::var(UPDATE_VARIABLE).as_deref() == Ok("1");
    if let Some(refusal) = update_refusal(update, &client::shell::assets::path("visual/generated/.pack-version")) {
        panic!("{refusal}");
    }
    assert_snapshot(snapshot_file(env!("CARGO_MANIFEST_DIR"), name), &stage.snapshot());
}

/// A small match after its opening, on the map centred on the capital.
fn opened_match() -> Stage<ClassicUi> {
    let mut stage = Stage::new(ClassicUi::game(), SIZE);
    let request = MatchRequest { nation: "core:nation/rome".into(), seed: 20260928, size: MapSize::Small, players: 3, water: 25 };
    stage.module.start(prepare_match(request).expect("the match is generated"));
    stage.apply_module_actions();
    stage.module.request(GameRequest::Unit(Action::FoundCity));
    let session = stage.module.session.as_mut().expect("the match runs");
    // The option is found by its English name whatever language the snapshot is drawn in.
    let warrior = i18n::with_language("en", || session.city_view()).expect("the capital").options.iter().find(|o| o.name == "Warrior").expect("Warrior").item;
    session.request(GameRequest::Enqueue(warrior));
    let first = session.research_view().techs.iter().find(|t| t.status == TechStatus::Available).expect("a technology").id;
    session.request(GameRequest::Research(first));
    stage.apply_module_actions();
    stage
}

#[test]
fn main_menu() {
    let mut stage = Stage::new(ClassicUi::game(), SIZE);
    check(&mut stage, "classic-menu");
}

#[test]
fn the_menu_and_the_settings_show_the_registered_build() {
    let mut stage = Stage::new(ClassicUi::game(), SIZE);
    assert!(stage.snapshot().contains("text \"Version 9.9.9 (abcdef0)\""), "the menu names the build");
    stage.open(ShellScreen::Settings);
    assert!(stage.snapshot().contains("text \"Version 9.9.9 (abcdef0)\""), "the settings screen names the build");
}

#[test]
fn hud_over_the_map() {
    let mut stage = opened_match();
    assert_eq!(stage.screens.screen, ShellScreen::Map);
    check(&mut stage, "classic-hud");
}

#[test]
fn city_screen() {
    let mut stage = opened_match();
    let capital = stage.module.session.as_ref().and_then(|session| session.selected_city_tile()).expect("the capital is selected");
    stage.module.request(GameRequest::SelectCity(capital));
    stage.apply_module_actions();
    check(&mut stage, "classic-city");
}

/// Captions of the buttons in a snapshot (`click "caption" x,y WxH`), without the whole-screen
/// groups ("a / b / c") and the item cards.
fn button_captions(snapshot: &str) -> Vec<&str> {
    snapshot
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("click \""))
        .filter_map(|rest| rest.split('"').next())
        .filter(|caption| !caption.contains(" / "))
        .collect()
}

#[test]
fn the_city_buttons_are_in_english_when_the_language_is_english() {
    let mut stage = opened_match();
    let capital = stage.module.session.as_ref().and_then(|session| session.selected_city_tile()).expect("the capital is selected");
    stage.module.request(GameRequest::SelectCity(capital));
    stage.apply_module_actions();
    let snapshot = stage.snapshot();
    let captions = button_captions(&snapshot);
    assert!(captions.len() > 10, "the city has its buttons: {captions:?}");
    // Every caption comes from the catalogue; the default language of the tests is English, so no
    // Cyrillic may be left over from the screen's old Russian literals.
    let cyrillic: Vec<_> = captions.into_iter().filter(|c| c.chars().any(|ch| ('\u{0400}'..='\u{04FF}').contains(&ch))).collect();
    assert!(cyrillic.is_empty(), "Russian captions on the English city screen: {cyrillic:?}");
}

#[test]
fn research_screen() {
    let mut stage = opened_match();
    stage.open(Screen::Research.into());
    check(&mut stage, "classic-research");
}

#[test]
fn victory_screen() {
    let mut stage = opened_match();
    stage.open(Screen::Victory.into());
    check(&mut stage, "classic-victory");
}

/// Smoke snapshots in other languages (platform/docs/localization.md): the same screens drawn with the
/// Russian, German and Japanese bundles. They are not about the exact words (a translator's
/// fix changes them with `SNAPSHOT_UPDATE=1`) but about the layout: a text that no longer fits
/// its button, a missing glyph or a lost placeholder shows up as a changed rectangle.
fn menu_in(tag: &str, name: &str) {
    i18n::with_language(tag, || {
        let mut stage = Stage::new(ClassicUi::game(), SIZE);
        client::shell::theme::install_fonts(&stage.ctx, stage.module.theme(), tag);
        check(&mut stage, name);
    });
}

fn city_in(tag: &str, name: &str) {
    i18n::with_language(tag, || {
        let mut stage = opened_match();
        client::shell::theme::install_fonts(&stage.ctx, stage.module.theme(), tag);
        let capital = stage.module.session.as_ref().and_then(|session| session.selected_city_tile()).expect("the capital is selected");
        stage.module.request(GameRequest::SelectCity(capital));
        stage.apply_module_actions();
        check(&mut stage, name);
    });
}

#[test]
fn main_menu_in_russian() {
    menu_in("ru", "classic-menu-ru");
}

#[test]
fn main_menu_in_german() {
    menu_in("de", "classic-menu-de");
}

#[test]
fn main_menu_in_japanese() {
    menu_in("ja", "classic-menu-ja");
}

#[test]
fn city_screen_in_russian() {
    city_in("ru", "classic-city-ru");
}

#[test]
fn city_screen_in_german() {
    city_in("de", "classic-city-de");
}

#[test]
fn city_screen_in_japanese() {
    city_in("ja", "classic-city-ja");
}

#[test]
fn an_update_without_the_art_pack_is_refused() {
    let refusal = update_refusal(true, Path::new("/nonexistent/visual/generated/.pack-version")).expect("refused");
    assert!(refusal.contains("SNAPSHOT_UPDATE=1 refused") && refusal.contains(".pack-version"), "{refusal}");
}

#[test]
fn an_update_with_the_art_pack_and_a_plain_run_are_not_refused() {
    let present = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    assert_eq!(update_refusal(true, &present), None);
    assert_eq!(update_refusal(false, Path::new("/nonexistent/.pack-version")), None);
}
