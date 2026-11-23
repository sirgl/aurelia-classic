//! The fixed belief preference lists of the AI before `net2`'s belief head (`strategy_net::beliefs`): the
//! seats without `net2`'s part `Beliefs` (`pre-net2` and every older variant, `net2-no-beliefs`) still choose
//! their founder, follower and enhancer beliefs by these lists, so that they play as they were measured.
//!
//! FROZEN. These lists name content, which the game's AI does not do anywhere else: a new choice values a
//! belief by its effects (the belief head). The content-names guard (`tests/ai_content_names.rs`) allows
//! this file for that reason only.

use crate::ids::BeliefId;
use crate::rules::CompiledRuleset;

pub const FOUNDERS: [&str; 3] = [
    "core:belief/founder/tithe",
    "core:belief/founder/church-property",
    "core:belief/founder/ceremonial-burial",
];
pub const FOLLOWERS: [&str; 4] = [
    "core:belief/follower/pagodas",
    "core:belief/follower/cathedrals",
    "core:belief/follower/mosques",
    "core:belief/follower/feed-the-world",
];
pub const ENHANCERS: [&str; 3] = [
    "core:belief/enhancer/religious-texts",
    "core:belief/enhancer/missionary-zeal",
    "core:belief/enhancer/itinerant-preachers",
];

/// The first free belief of the preference list, else the first free one.
pub fn pick(rules: &CompiledRuleset, free: &[BeliefId], preferred: &[&str]) -> Option<BeliefId> {
    for key in preferred {
        let found = free.iter().find(|b| rules.module.beliefs[b.0 as usize].key == *key);
        if let Some(b) = found {
            return Some(*b);
        }
    }
    free.first().copied()
}
