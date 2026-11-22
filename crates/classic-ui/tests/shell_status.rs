//! An error of the shell raised during play (a failed autosave, a full browser store) is drawn over
//! the map's HUD, not kept in the main menu's line: the toast is in the frame of the map and in the
//! frame of a module screen, and a click on it dismisses it.
use classic_ui::ui::screens::{prepare_match, MapSize, MatchRequest};
use classic_ui::ClassicUi;
use client::shell::module::Screen as ShellScreen;
use client::shell::snapshot::Stage;

const SIZE: egui::Vec2 = egui::vec2(1366.0, 768.0);

fn playing() -> Stage<ClassicUi> {
    let mut stage = Stage::new(ClassicUi::game(), SIZE);
    let request = MatchRequest { nation: "core:nation/rome".into(), seed: 20260928, size: MapSize::Small, players: 3, water: 25 };
    stage.module.start(prepare_match(request).expect("the match is generated"));
    stage.apply_module_actions();
    stage
}

#[test]
fn a_shell_error_during_play_is_on_the_map_and_on_a_screen_until_clicked() {
    let mut stage = playing();
    assert_eq!(stage.screens.screen, ShellScreen::Map);
    assert!(!stage.snapshot().contains("Cannot save: the storage is full"));
    stage.screens.error("Cannot save: the storage is full");
    assert!(stage.snapshot().contains("text \"Cannot save: the storage is full\""), "the HUD holds the toast:\n{}", stage.snapshot());
    // An error stays across frames.
    for _ in 0..5 {
        stage.frame(Vec::new());
    }
    assert!(stage.screens.status.is_some(), "an error is sticky");
    stage.open(ShellScreen::Settings);
    assert!(stage.snapshot().contains("text \"Cannot save: the storage is full\""), "a screen holds it too");
    stage.screens.status = None;
    assert!(!stage.snapshot().contains("the storage is full"));
}
