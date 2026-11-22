//! The screens draw in design units and never read or set the scale of their layer.
//!
//! A phone shrinks a module screen as one layer, and the shell lays every text of that layer out
//! again at the size it is drawn at (`client::shell::crisp`). A screen that scales its own text
//! for the layer (the policies screen once did) is laid out twice, at `size * scale * scale`, and a
//! screen that scales a layer of its own brings the faded glyphs back. Both are found here.

use std::path::{Path, PathBuf};

/// The calls that read or set a layer's scale.
const SCALE_CALLS: [&str; 2] = ["layer_transform_to_global(", "set_transform_layer("];

#[test]
fn no_screen_reads_or_sets_the_scale_of_its_layer() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut users: Vec<(PathBuf, String)> = Vec::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("the source directory reads").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(".rs") || name.ends_with("_tests.rs") || name == "tests.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a source file reads");
            let code = text.split("#[cfg(test)]").next().unwrap_or("");
            for line in code.lines() {
                let shipped = !line.trim_start().starts_with("//");
                if shipped && SCALE_CALLS.iter().any(|call| line.contains(call)) {
                    users.push((path.clone(), line.trim().to_owned()));
                }
            }
        }
    }
    assert_eq!(users, Vec::new(), "the shell alone scales a screen's layer and its text (client::shell::crisp)");
}
