//! The record of what the economy nets (`econ-site`, `econ-choice`, `econ`;
//! classic/docs/ai-econ-nets.md) choose against what `standard` would have chosen in the same
//! state: off unless the shadow tool (`aurelia-qa` `ai-econ-shadow`) enables it on its thread, and
//! never read by a decision.

use std::cell::RefCell;

use crate::ids::{PlayerId, ProductionItem, TechId, TileId};

/// One site as a settling quality ranked it first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SiteCase {
    pub tile: TileId,
    /// Hex distance to the nearest own and remembered foreign city (None: no such city).
    pub own_distance: Option<i32>,
    pub foreign_distance: Option<i32>,
    /// The planner's land score of the site (`SiteContext::score`, distance terms included).
    pub heuristic: i32,
    /// Its value under the quality that ranked it (score minus travel).
    pub value: i32,
}

/// One decision of a net with the choice `standard` makes in its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// A settler's site: the site net's best and the standard quality's best.
    Site { player: PlayerId, round: u32, settler: u64, net: SiteCase, standard: SiteCase },
    /// A city's order: the strategist's (`standing`) and the one taken over it, with their ranks
    /// in the allocator's top (0: the allocator's best).
    Production { player: PlayerId, round: u32, city: u64, standing: ProductionItem, taken: ProductionItem, ranks: Ranks },
    /// The research: the strategist's technology and the one taken over it, with their ranks.
    Research { player: PlayerId, round: u32, standing: TechId, taken: TechId, ranks: Ranks },
}

/// The ranks of the standing and the taken choice in the allocator's top (0: its best).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ranks {
    pub standing: u8,
    pub taken: u8,
}

thread_local! {
    static LOG: RefCell<Option<Vec<Case>>> = const { RefCell::new(None) };
}

/// Start (or stop) recording on this thread.
pub fn enable(on: bool) {
    LOG.with(|log| *log.borrow_mut() = on.then(Vec::new));
}

pub fn enabled() -> bool {
    LOG.with(|log| log.borrow().is_some())
}

/// Record a case (nothing when recording is off).
pub fn push(case: Case) {
    LOG.with(|log| {
        if let Some(cases) = log.borrow_mut().as_mut() {
            cases.push(case);
        }
    });
}

/// The cases recorded since the last call.
pub fn take() -> Vec<Case> {
    LOG.with(|log| match log.borrow_mut().as_mut() {
        Some(cases) => std::mem::take(cases),
        None => Vec::new(),
    })
}
