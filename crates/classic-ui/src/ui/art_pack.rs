//! The art pack (`assets/visual/generated`, fetched by `platform/tools/fetch-assets.sh`, not in git: README.md,
//! THIRD-PARTY.md) and the tests that need it.
//!
//! The group of pack-dependent tests: `ui::pedia::art` (every content key has its card), `ui::unit_actions` (the
//! action sheets), `ui::nation_catalog` (the art catalogue), `game::tests` (a great work's art) and the
//! snapshots of `tests/screen_snapshots.rs` (the screens with their icons). A test of the group starts with
//! `if art_pack::skip(name) { return; }`: on a clone without the pack it prints one line and passes
//! (`art pack missing: see README`), with the pack it runs as any test. The pack counts as missing only when
//! its `.pack-version` file (written when it is unpacked) is absent: a pack that is there but incomplete or
//! damaged is NOT skipped, its tests run and fail.

use std::path::Path;

/// Whether the pack is missing: the file `.pack-version` is not at `marker`.
pub fn missing_at(marker: &Path) -> bool {
    !marker.exists()
}

/// True, with one line on stderr, when the art pack is missing: the test of the group returns at once.
pub fn skip(test: &str) -> bool {
    let marker = client::shell::assets::path("visual/generated/.pack-version");
    let missing = missing_at(&marker);
    if missing {
        eprintln!("art pack missing: see README, \"Running the game\" ({test} skipped; {} is not there)", marker.display());
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pack_is_missing_only_when_its_marker_is_not_there() {
        assert!(missing_at(Path::new("/nonexistent/visual/generated/.pack-version")));
        // A pack that is there but empty or damaged is not "missing": its tests run and fail.
        let dir = std::env::temp_dir().join(format!("aurelia-broken-pack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".pack-version"), "v1").unwrap();
        assert!(!missing_at(&dir.join(".pack-version")), "a broken pack is not skipped");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
