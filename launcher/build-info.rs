//! The build script of the three launchers (`build = "../build-info.rs"`): names the commit the build is made
//! from, so that the game can show it next to its version (`client::register_build!`).
//! `AURELIA_GIT_HASH` set by the caller wins (a package made from an archive, a CI job); else the
//! checkout's `HEAD`. Outside a checkout the variable stays unset and the game shows the version only.
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let dir = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_owned()).filter(|text| !text.is_empty())
}

fn main() {
    println!("cargo:rerun-if-env-changed=AURELIA_GIT_HASH");
    let given = std::env::var("AURELIA_GIT_HASH").ok().filter(|hash| !hash.trim().is_empty());
    let Some(hash) = given.or_else(|| git(&["rev-parse", "--short=7", "HEAD"])) else {
        return;
    };
    println!("cargo:rustc-env=AURELIA_GIT_HASH={}", hash.trim());
    // Every commit appends to the reflog of HEAD: the hash is read again after one.
    if let Some(log) = git(&["rev-parse", "--path-format=absolute", "--git-path", "logs/HEAD"]) {
        println!("cargo:rerun-if-changed={log}");
    }
}
