//! The version of the game is the version of its launchers (docs: CONTRIBUTING.md "Versions"). The
//! three launcher crates carry one number, the Android version code follows it, and the change log
//! has a section for it: a release that bumps one and forgets the others fails here.
use std::path::{Path, PathBuf};

fn classic_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    let full = classic_dir().join(path);
    std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{}: {e}", full.display()))
}

/// The value of the first `key = "value"` (or `key = value`) line of a manifest.
fn field(manifest: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = ");
    manifest.lines().find_map(|line| line.strip_prefix(&prefix)).map(|value| value.trim().trim_matches('"').to_owned())
}

fn launcher_version(launcher: &str) -> String {
    field(&read(&format!("launcher/{launcher}/Cargo.toml")), "version").unwrap_or_else(|| panic!("{launcher} has no version"))
}

#[test]
fn the_three_launchers_carry_one_version() {
    let desktop = launcher_version("desktop");
    assert_eq!(launcher_version("web"), desktop, "classic-web differs from classic-desktop");
    assert_eq!(launcher_version("android"), desktop, "classic-android differs from classic-desktop");
}

#[test]
fn the_android_version_code_follows_the_version() {
    let version = launcher_version("android");
    let parts: Vec<u32> = version.split('.').map(|part| part.parse().unwrap_or_else(|_| panic!("{version} is not major.minor.patch"))).collect();
    let [major, minor, patch] = parts[..] else { panic!("{version} is not major.minor.patch") };
    let code = field(&read("launcher/android/Cargo.toml"), "version_code").expect("the android manifest names its version_code");
    assert_eq!(code, (major * 10_000 + minor * 100 + patch).to_string(), "version_code is major*10000 + minor*100 + patch of {version}");
}

#[test]
fn the_change_log_has_a_section_for_the_version() {
    let version = launcher_version("desktop");
    let log = read("CHANGELOG.md");
    assert!(log.lines().any(|line| line.starts_with(&format!("## {version}"))), "CHANGELOG.md has no \"## {version}\" section");
}
