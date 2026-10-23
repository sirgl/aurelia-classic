//! The classic keyboard on a stage (`client::shell::snapshot`): simulated key presses reach
//! the right commands, a remapped key does what the player chose, the keys wait in the
//! menu, and the shortcuts screens are drawn. `SNAPSHOT_UPDATE=1` accepts a changed screen.
use classic_ui::ui::screens::{prepare_match, MapSize, MatchRequest, Screen};
use classic_ui::ClassicUi;
use client::shell::keymap::Chord;
use client::shell::module::{ModuleUi, Screen as ShellScreen, ShellAction};
use client::shell::settings::Settings;
use client::shell::shortcuts::Routed;
use client::shell::snapshot::{assert_snapshot, snapshot_file, Stage};

const SIZE: egui::Vec2 = egui::vec2(1366.0, 768.0);

fn check(stage: &mut Stage<ClassicUi>, name: &str) {
    assert_snapshot(snapshot_file(env!("CARGO_MANIFEST_DIR"), name), &stage.snapshot());
}

/// A small match at its start: the settler stands selected, no city yet.
fn fresh_match() -> Stage<ClassicUi> {
    let mut stage = Stage::new(ClassicUi::game(), SIZE);
    let request = MatchRequest { civilization: "core:nation/rome".into(), seed: 20260928, size: MapSize::Small, players: 3, water: 25 };
    stage.module.start(prepare_match(request).expect("the match is generated"));
    stage.apply_module_actions();
    stage
}

fn cities(stage: &Stage<ClassicUi>) -> usize {
    let world = stage.module.view().expect("the match runs");
    world.cities.iter().filter(|c| c.owner == world.observer).count()
}

#[test]
fn b_founds_the_city_with_the_settler_and_does_nothing_without_one() {
    let mut stage = fresh_match();
    assert_eq!(cities(&stage), 0);
    assert_eq!(stage.press("H"), Routed::None, "a settler cannot heal");
    assert_eq!(stage.press("B"), Routed::Module);
    assert_eq!(cities(&stage), 1);
    assert_eq!(stage.press("B"), Routed::None, "the settler is gone: no order has B now");
}

#[test]
fn function_keys_open_the_screens_of_the_match() {
    let mut stage = fresh_match();
    for (chord, screen) in [("F6", Screen::Research), ("F5", Screen::Civics), ("F10", Screen::Culture), ("F2", Screen::Overview), ("F1", Screen::Encyclopedia)] {
        stage.open(ShellScreen::Map);
        assert_eq!(stage.press(chord), Routed::Module, "{chord}");
        assert_eq!(stage.screens.screen, screen.into(), "{chord}");
    }
    // From a screen the next F key works at once; orders do not (they belong to the map).
    stage.open(Screen::Research.into());
    assert_eq!(stage.press("F5"), Routed::Module);
    assert_eq!(stage.press("B"), Routed::None);
}

#[test]
fn the_shell_keys_ask_the_shell() {
    let mut stage = fresh_match();
    stage.open(ShellScreen::Map);
    assert_eq!(stage.press("Enter"), Routed::Shell(ShellAction::EndTurn));
    stage.open(ShellScreen::Map);
    assert_eq!(stage.press("Ctrl+Space"), Routed::Shell(ShellAction::EndTurn));
    stage.open(ShellScreen::Map);
    assert_eq!(stage.press("G"), Routed::Shell(ShellAction::ToggleGrid));
    assert_eq!(stage.press("Ctrl+Y"), Routed::Shell(ShellAction::ToggleYields));
    assert_eq!(stage.press("Home"), Routed::Shell(ShellAction::CenterOnSelection));
    assert!(matches!(stage.press("Plus"), Routed::Shell(ShellAction::Zoom(f)) if f > 1.0));
    assert!(matches!(stage.press("Minus"), Routed::Shell(ShellAction::Zoom(f)) if f < 1.0));
    assert!(matches!(stage.press("PageUp"), Routed::Shell(ShellAction::Zoom(f)) if f > 1.0));
    assert_eq!(stage.press("F11"), Routed::Shell(ShellAction::Save));
    assert_eq!(stage.press("F12"), Routed::Shell(ShellAction::Load));
}

#[test]
fn the_camera_letters_are_orders_while_a_unit_is_selected() {
    let mut stage = fresh_match();
    stage.open(ShellScreen::Map);
    stage.frame(Vec::new());
    assert!(stage.module.unit_selected(), "the settler is selected");
    assert_eq!(stage.press("W"), Routed::None, "W moves no camera and gives no order with a unit selected");
    let keymap = stage.screens.keymap(&stage.module);
    let held = |k: egui::Key| k == egui::Key::D || k == egui::Key::W;
    assert_eq!(keymap.pan(held, true), (0.0, 0.0), "letters do not pan with a unit selected");
    assert_eq!(keymap.pan(held, false), (1.0, -1.0), "and pan without one");
    assert_eq!(keymap.pan(|k| k == egui::Key::ArrowLeft, true), (-1.0, 0.0), "arrows always pan");
}

#[test]
fn nothing_works_before_a_match_but_the_help() {
    let mut stage = Stage::new(ClassicUi::game(), SIZE);
    for chord in ["B", "F6", "Enter", "G", "F11"] {
        assert_eq!(stage.press(chord), Routed::None, "{chord}");
    }
    assert_eq!(stage.press("Ctrl+H"), Routed::Used);
    assert!(stage.screens.shortcuts.open);
    assert_eq!(stage.press("Escape"), Routed::Used, "Esc closes the overlay first");
    assert!(!stage.screens.shortcuts.open);
}

#[test]
fn a_remapped_key_does_what_the_player_chose() {
    let mut stage = fresh_match();
    stage.screens.settings.keys.insert("classic.order-found-city".into(), "J".into());
    stage.screens.settings.keys.insert("shell.toggle-grid".into(), "Ctrl+G".into());
    assert_eq!(stage.press("B"), Routed::None, "the old key is free");
    assert_eq!(stage.press("G"), Routed::None);
    assert_eq!(stage.press("Ctrl+G"), Routed::Shell(ShellAction::ToggleGrid));
    assert_eq!(stage.press("J"), Routed::Module);
    assert_eq!(cities(&stage), 1);
    // The change survives a restart: settings.cfg text and back.
    let again = Settings::from_text(&stage.screens.settings.to_text());
    assert_eq!(again.keys, stage.screens.settings.keys);
    assert!(stage.screens.settings.to_text().contains("key.classic.order-found-city = J"));
}

#[test]
fn an_old_settings_file_without_keys_keeps_the_defaults() {
    let old = "window = windowed\nresolution = 1440x900\nquality = medium\n";
    let settings = Settings::from_text(old);
    assert!(settings.keys.is_empty());
    let mut stage = fresh_match();
    stage.screens.settings = settings;
    assert_eq!(stage.press("B"), Routed::Module);
    // A damaged key line is dropped, the others stay.
    let text = "key.classic.order-found-city = Ctrl+Nonsense\nkey.shell.toggle-grid = Ctrl+G\nkey.shell.zoom-in = none\n";
    let settings = Settings::from_text(text);
    assert_eq!(settings.keys.len(), 2);
    assert!(!settings.keys.contains_key("classic.order-found-city"));
}

#[test]
fn changing_a_key_in_the_screen_asks_before_taking_it_from_another_action() {
    let mut stage = fresh_match();
    stage.screens.shortcuts.listen("order-found-city");
    assert_eq!(stage.press("F"), Routed::Used, "the press is taken by the remapping");
    let (chord, with) = stage.screens.shortcuts.pending_replace().expect("F is used by Fortify");
    assert_eq!(chord, Chord::parse("F").unwrap());
    assert_eq!(with, vec!["order-fortify"]);
    assert!(stage.screens.settings.keys.is_empty(), "nothing changed yet");
    stage.screens.confirm_replace(&stage.module);
    assert_eq!(stage.screens.settings.keys["classic.order-found-city"], "F");
    assert_eq!(stage.screens.settings.keys["classic.order-fortify"], "none");
    assert_eq!(stage.press("F"), Routed::Module, "F founds the city now");
    assert_eq!(cities(&stage), 1);
}

#[test]
fn a_free_key_is_taken_at_once_and_esc_cancels() {
    let mut stage = fresh_match();
    stage.screens.shortcuts.listen("order-found-city");
    stage.press("Escape");
    assert!(!stage.screens.shortcuts.listening());
    stage.screens.shortcuts.listen("order-found-city");
    stage.press("J");
    assert!(!stage.screens.shortcuts.listening());
    assert_eq!(stage.screens.settings.keys["classic.order-found-city"], "J");
    assert!(stage.screens.shortcuts.pending_replace().is_none());
}

#[test]
fn the_overlay_and_the_controls_page_are_drawn() {
    let mut stage = fresh_match();
    stage.open(ShellScreen::Map);
    assert_eq!(stage.press("Ctrl+H"), Routed::Used);
    check(&mut stage, "classic-shortcuts-overlay");
    stage.press("Escape");
    stage.open(ShellScreen::Settings);
    stage.screens.capture_settings_tab(&stage.ctx, "controls");
    check(&mut stage, "classic-shortcuts-controls");
}

#[test]
fn the_controls_page_in_russian() {
    i18n::with_language("ru", || {
        let mut stage = fresh_match();
        client::shell::theme::install_fonts(&stage.ctx, stage.module.theme(), "ru");
        stage.open(ShellScreen::Settings);
        stage.screens.capture_settings_tab(&stage.ctx, "controls");
        check(&mut stage, "classic-shortcuts-controls-ru");
    });
}

#[test]
fn every_label_of_the_module_is_translated_into_russian() {
    i18n::with_language("ru", || {
        for def in classic_ui::ui::shortcuts::ROWS.iter().chain(client::shell::keymap::SHELL_ROWS) {
            let ru = def.label();
            assert!(ru.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)), "{}: {ru}", def.id);
        }
    });
}
