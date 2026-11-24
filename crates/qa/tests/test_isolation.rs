//! The guard of the tests' isolation: the tests of one binary run in parallel threads of one process, so a test
//! that changes the process (the environment, the current directory) changes the others' world, and what they see
//! depends on the order. A test sets what it needs for its own thread instead (`mind::defer_override`,
//! `seat::install_net`, a temp dir named by the test) and reads the environment only. The one exception is a
//! file that tests the environment itself and serializes its tests on a lock (`ai_experimental_off.rs`).

use std::path::Path;

const FORBIDDEN: [&str; 3] = ["set_var(", "remove_var(", "set_current_dir("];

fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("the directory reads").flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_test_changes_the_process_environment_or_directory() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for dir in ["qa/tests", "classic-rules/tests"] {
        let dir = root.join(dir);
        if dir.exists() {
            sources(&dir, &mut files);
        }
    }
    assert!(files.len() > 5, "the scan finds the test files");
    let mut found = Vec::new();
    for file in files.iter().filter(|f| !f.ends_with("test_isolation.rs")) {
        let text = std::fs::read_to_string(file).expect("the file reads");
        // A file whose tests must change the environment says so by serializing them on a lock (`VARIANT_ENV`).
        if text.contains("static VARIANT_ENV: std::sync::Mutex") {
            continue;
        }
        for (n, line) in text.lines().enumerate() {
            if !line.trim_start().starts_with("//") && FORBIDDEN.iter().any(|f| line.contains(f)) {
                found.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(found.is_empty(), "tests share one process; do not change it:\n{}", found.join("\n"));
}
