//! Scratch trace of the decisions the human tricks exercise (`TRICKS_TRACE=<file>`, work/nets/tricks/CHAIN.md):
//! one JSON line per answer to an offer, per gift and per peace decision, with the `net2` outputs it read. It
//! decides nothing: without the variable it writes nothing and costs one environment read a call.

use std::io::Write as _;

/// Appends the line `make` builds when `TRICKS_TRACE` names a file.
pub fn line(make: impl FnOnce() -> String) {
    let Some(path) = std::env::var_os("TRICKS_TRACE") else { return };
    let text = make();
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{text}");
    }
}

/// Whether the trace is on (to skip building a costly line).
pub fn enabled() -> bool {
    std::env::var_os("TRICKS_TRACE").is_some()
}
