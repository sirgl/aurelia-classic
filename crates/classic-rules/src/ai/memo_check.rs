//! The guard of the AI's remembered answers (`perf/ai-turn2`): while it is on, every answer taken from a memo
//! is also computed afresh and the two must be equal. A test turns it on, plays whole AI games and reads how
//! many answers it compared (and how many of those were not zero): a guard that compared nothing, or only
//! zeros, proves nothing. Off in play: one relaxed load per answer.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static ON: AtomicBool = AtomicBool::new(false);
static COMPARED: AtomicUsize = AtomicUsize::new(0);
static NOT_ZERO: AtomicUsize = AtomicUsize::new(0);

/// Turn the comparison of remembered answers with fresh ones on or off.
#[doc(hidden)]
pub fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether the comparison is on.
#[inline]
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Compare the remembered answer with the fresh one (`zero`: the answer is the empty one).
pub fn compare<T: PartialEq + std::fmt::Debug>(what: &str, remembered: &T, fresh: &T, zero: bool) {
    assert_eq!(remembered, fresh, "the remembered answer of {what} differs from the fresh one");
    COMPARED.fetch_add(1, Ordering::Relaxed);
    if !zero {
        NOT_ZERO.fetch_add(1, Ordering::Relaxed);
    }
}

/// How many answers were compared since the start, and how many of those were not zero.
#[doc(hidden)]
pub fn counts() -> (usize, usize) {
    (COMPARED.load(Ordering::Relaxed), NOT_ZERO.load(Ordering::Relaxed))
}
