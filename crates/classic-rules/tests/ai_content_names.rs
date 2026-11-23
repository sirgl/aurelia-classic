//! Guard: the game's AI chooses content by what it does, never by its name (classic/docs/ai.md: "No content
//! is named"). A belief, building, unit, technology, policy, wonder, promotion or improvement is valued by
//! its declared effects (`ai::policy_branches`, `ai::pantheon`, `strategy_net::beliefs`, …); a content key
//! written in the AI's code is a choice by name, which a mod's table or a changed table silently breaks.
//!
//! What THIS TEST holds by reading the sources of `classic-rules/src/ai`: no string literal is a content key
//! (`namespace:kind/name`, or any key of the compiled core rules) or a content's display name (`Tithe`,
//! `Tradition`) in non-test code, except the allow-list
//! below, each with its reason. Test code (from the first `#[cfg(test)]` of a file) and comment lines are
//! not scanned. `CONTENT_NAMES_ROOT` points the scan at another copy of the AI's sources (the guard's own
//! proof on a scratch copy).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use classic_content::core::core_rules;
use classic_rules::compile_rules;

/// (path below `ai/`: a file, or a directory ending in `/`; the line must start with this, or anything;
/// reason).
const ALLOWED: &[(&str, &str, &str)] = &[
    ("books/", "", "scripted league opponents and probes: a book names the content of the human strategy it copies (ai/books/mod.rs)"),
    ("unciv/", "", "the ported AI of the `unciv` variants, which names content by its display name as the ported code does"),
    ("tall.rs", "pub const BOOK_", "the `TallBook` probe's scripted line (its branches and buildings)"),
    ("belief_lists.rs", "", "the frozen belief lists of the seats before net2's belief head (pre-net2 and older), kept for their play"),
];

fn ai_root() -> PathBuf {
    match std::env::var_os("CONTENT_NAMES_ROOT") {
        Some(root) => PathBuf::from(root),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ai"),
    }
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
fn code_lines(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[cfg(test)]") {
            break;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        out.push((i + 1, line));
    }
    out
}

/// The string literals of a line (escapes kept as written).
fn literals(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'"' {
                j += if bytes[j] == b'\\' { 2 } else { 1 };
            }
            out.push(&line[start..j.min(line.len())]);
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// A content key's shape: `namespace:kind/name` (lower case, digits, `-`, `_`; more `/` allowed).
fn looks_like_key(s: &str) -> bool {
    let Some((namespace, rest)) = s.split_once(':') else { return false };
    let Some((kind, name)) = rest.split_once('/') else { return false };
    let word = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    word(namespace) && word(kind) && name.split('/').all(word)
}

/// Every key and display name of the compiled core rules' content.
fn rule_keys() -> BTreeSet<String> {
    let rules = compile_rules(core_rules()).expect("core rules");
    let mut keys = BTreeSet::new();
    keys.extend(rules.units.iter().map(|d| d.key.clone()));
    keys.extend(rules.buildings.iter().map(|d| d.key.clone()));
    keys.extend(rules.technologies.iter().map(|d| d.key.clone()));
    keys.extend(rules.improvements.iter().map(|d| d.key.clone()));
    keys.extend(rules.resources.iter().map(|d| d.key.clone()));
    keys.extend(rules.promotions.iter().map(|d| d.key.clone()));
    keys.extend(rules.module.policies.iter().map(|d| d.key.clone()));
    keys.extend(rules.module.policy_branches.iter().map(|d| d.key.clone()));
    keys.extend(rules.module.beliefs.iter().map(|d| d.key.clone()));
    keys.extend(rules.module.projects.iter().map(|d| d.key.clone()));
    keys.extend(rules.units.iter().map(|d| d.name.clone()));
    keys.extend(rules.buildings.iter().map(|d| d.name.clone()));
    keys.extend(rules.technologies.iter().map(|d| d.name.clone()));
    keys.extend(rules.improvements.iter().map(|d| d.name.clone()));
    keys.extend(rules.promotions.iter().map(|d| d.name.clone()));
    keys.extend(rules.module.policies.iter().map(|d| d.name.clone()));
    keys.extend(rules.module.policy_branches.iter().map(|d| d.name.clone()));
    keys.extend(rules.module.beliefs.iter().map(|d| d.name.clone()));
    keys.retain(|k| !k.is_empty());
    keys
}

/// The content keys named in the AI's sources below `root`, outside `ALLOWED`: "file:line: literal".
fn named(root: &Path, keys: &BTreeSet<String>, used: &mut [bool]) -> Vec<String> {
    let mut files = Vec::new();
    rust_files(root, &mut files);
    let mut bad = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        for (number, line) in code_lines(&text) {
            for literal in literals(line).into_iter().filter(|l| looks_like_key(l) || keys.contains(*l)) {
                let allowed = ALLOWED.iter().position(|(path, start, _)| {
                    let here = if path.ends_with('/') { rel.starts_with(path) } else { rel == *path };
                    here && line.trim_start().starts_with(start)
                });
                match allowed {
                    Some(at) => used[at] = true,
                    None => bad.push(format!("{rel}:{number}: \"{literal}\" in `{}`", line.trim())),
                }
            }
        }
    }
    bad
}

#[test]
fn the_ai_names_no_content() {
    let root = ai_root();
    let keys = rule_keys();
    assert!(keys.len() > 300, "the core rules' keys: {}", keys.len());
    let mut used = vec![false; ALLOWED.len()];
    let bad = named(&root, &keys, &mut used);
    assert!(bad.is_empty(), "the AI chooses content by its name; value it by its declared effects instead (or, for a scripted probe, add it to ALLOWED with its reason):\n{}", bad.join("\n"));
    if std::env::var_os("CONTENT_NAMES_ROOT").is_none() {
        for (used, (path, _, _)) in used.iter().zip(ALLOWED) {
            assert!(*used, "stale allow-list entry {path}: remove it");
        }
    }
}

/// The scan itself: a key in an AI file is found (full key, by shape and by the table), an allowed file and
/// test code and comments are not.
#[test]
fn the_guard_finds_a_named_belief() {
    let dir = std::env::temp_dir().join(format!("ai-content-names-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("books")).unwrap();
    std::fs::write(dir.join("chooser.rs"), "fn f() -> &'static str {\n    \"core:belief/founder/tithe\"\n}\n// \"core:belief/founder/tithe\"\n#[cfg(test)]\nconst X: &str = \"core:unit/settler\";\n").unwrap();
    std::fs::write(dir.join("other.rs"), "const MOD: &str = \"mymod:building/hall\";\nconst WORD: &str = \"gold\";\n").unwrap();
    std::fs::write(dir.join("books/plan.rs"), "const S: &str = \"core:unit/settler\";\n").unwrap();
    let keys: BTreeSet<String> = ["core:belief/founder/tithe".to_owned()].into();
    let mut used = vec![false; ALLOWED.len()];
    let bad = named(&dir, &keys, &mut used);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(bad.len(), 2, "{bad:?}");
    assert!(bad[0].starts_with("chooser.rs:2:"), "{bad:?}");
    assert!(bad[1].starts_with("other.rs:1:"), "{bad:?}");
    assert!(used[0], "the books are allowed");
}
