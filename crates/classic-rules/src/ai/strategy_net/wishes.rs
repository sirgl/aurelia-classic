//! The strategist's wishes to the scripted planners (`strategist-x`, classic/docs/ai-strategist-defects.md):
//! an output of the net that the planners read and no planner computes itself.
//!
//! | output | what | readers |
//! |---|---|---|
//! | `threat` | the rival running away and how far (`threat`) | the posture (`weights::threat::POSTURE`), the war's stakes (`Stakes::with_plan`), the denunciation and the deals (`diplomacy::contained`) |
//!
//! Every other variant's plan carries `Wishes::default()`: nothing is wished, nothing changes.

use crate::ids::PlayerId;

use super::Milli;

/// The rival running away: who, and the threat of its victory (0…1000).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Threat {
    pub rival: PlayerId,
    pub level: Milli,
}

/// What the strategist wishes of the planners this turn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wishes {
    pub threat: Option<Threat>,
}

/// The parts of `strategist-x` a variant plays (`Variant::strategist_parts`; the bench's ablations are
/// the variants `strategist-x-style` and `strategist-x-threat`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Parts {
    /// The rival running away (`threat::assess`): the posture term, the stakes, the denunciation, the deals.
    pub threat: bool,
    /// The wide style needs a strong reason (`weights::style::WIDE`).
    pub tall_first: bool,
}

impl Parts {
    /// Every variant but `strategist-x` and its ablations.
    pub const NONE: Parts = Parts { threat: false, tall_first: false };

    pub fn any(self) -> bool {
        self != Self::NONE
    }
}
