//! Guard for the AI contract (classic/docs/ai.md, classic/docs/ai-truth-reads.md): the AI
//! sees the world only through its observation.
//!
//! What the TYPES enforce (not this test): the AI entry points of every
//! module (`kernel::ai::mind::AiMind`) receive an observation built by
//! `sense`/`sense_unit`, never the `GameState`. For the classic AI the observation is
//! `AIObservation`/`LocalObservation`, so the planners cannot reach the
//! authoritative world through their arguments.
//!
//! What THIS TEST holds by reading the sources:
//! 1. The classic AI (`classic-rules/src/ai`): no direct read of the authoritative
//!    world in non-test code, except the allow-list below (each with its reason).
//!    The remaining way round the types is `PlayerView::world()` and friends;
//!    they are forbidden here.
//! 2. Lands & Laws and the sandbox: their ratchet test sits in `aurelia-qa`
//!    (`tests/ai_truth_ratchet.rs`), because it reads the sources of two other parts.
//!
//! Test code (from the first `#[cfg(test)]` of a file) and comment lines are
//! not scanned.

use std::path::{Path, PathBuf};

/// What counts as a read of the authoritative world.
const PATTERNS: &[&str] = &[
    ".view().auth",
    ".auth.",
    ".auth)",
    ".auth;",
    "AuthWorld",
    ".world(",
    "GameState",
    "GameView",
    ".derived()",
    "player_view(",
    "ai_observation",
    "ai_local_observation",
    ".indexes()",
    ".module()",
];

/// Classic AI allow-list: (file below `classic-rules/src/ai`, pattern, reason).
const CLASSIC_ALLOWED: &[(&str, &str, &str)] = &[
    (
        "driver.rs",
        "GameState",
        "the sensors `ClassicMind::sense`/`sense_unit` and the type alias of the driver: the one place where the classic AI is handed the game, to build the observation",
    ),
    ("driver.rs", "ai_observation", "the sensor `sense`: builds `AIObservation` (the driver side of the boundary)"),
    ("driver.rs", "ai_local_observation", "the sensor `sense_unit`: builds `LocalObservation`"),
];


fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("crates dir").to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out.sort();
}

/// The scanned lines of a file: numbered, without comments and test code.
fn code_lines(path: &Path) -> Vec<(usize, String)> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[cfg(test)]") {
            break;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        out.push((i + 1, line.to_owned()));
    }
    out
}

fn hits(line: &str) -> Vec<&'static str> {
    PATTERNS.iter().copied().filter(|p| line.contains(p)).collect()
}

#[test]
fn the_classic_ai_reads_the_world_only_through_its_observation() {
    let root = crates_dir().join("classic-rules/src/ai");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    assert!(files.len() > 30, "found the AI sources: {}", files.len());
    let mut bad = Vec::new();
    let mut allowed_used = vec![false; CLASSIC_ALLOWED.len()];
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        for (number, line) in code_lines(file) {
            for pattern in hits(&line) {
                match CLASSIC_ALLOWED.iter().position(|(f, p, _)| *f == rel && *p == pattern) {
                    Some(at) => allowed_used[at] = true,
                    None => bad.push(format!("{rel}:{number}: `{pattern}` in `{}`", line.trim())),
                }
            }
        }
    }
    assert!(bad.is_empty(), "the classic AI reads the authoritative world (docs/ai-truth-reads.md); route it through AIObservation/PlayerQueries:\n{}", bad.join("\n"));
    for (used, (file, pattern, _)) in allowed_used.iter().zip(CLASSIC_ALLOWED) {
        assert!(*used, "stale allow-list entry ({file}, {pattern}): remove it");
    }
}
