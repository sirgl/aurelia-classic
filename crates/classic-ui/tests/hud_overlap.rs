//! No two things of the map HUD overlap — panels, the shell's controls, the top bar's
//! items, city banners and unit badges — in any state of a classic match the check stages,
//! at 1280×720, 1600×900 and 2560×1440 (`classic_ui::dev::hud_check`).

#[test]
fn the_hud_never_overlaps_at_three_window_sizes() {
    let Some(overlaps) = classic_ui::dev::hud_check::hud_overlaps(&[(1280, 720), (1600, 900), (2560, 1440)]) else {
        return eprintln!("skipped: no GPU");
    };
    assert!(overlaps.is_empty(), "{} overlaps:\n{}", overlaps.len(), overlaps.join("\n"));
}
