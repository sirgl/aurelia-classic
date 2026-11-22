//! The strategist's wishes to the scripted planners (`strategist-x`, classic/docs/ai-strategist-defects.md):
//! three outputs of the net that the planners read and no planner computes itself.
//!
//! | output | what | readers |
//! |---|---|---|
//! | `supply` | the copies the goal's items lack, priced by the intent (`ai::supply`) | `Prices::supply`: tiles, workers, sites, buildings, deals, city-state gifts, the release of copies |
//! | `threat` | the rival running away and how far (`threat`) | the posture (`fe::THREAT`), the war's stakes, the denunciation and the deals |
//! | `city_states` | a stance toward each known city-state (`stance`) | the war machine's targets (prey), the pledges (client) |
//!
//! Every other variant's plan carries `Wishes::default()`: nothing is wished, nothing changes.

use crate::ai::supply::SupplyNeed;
use crate::ids::PlayerId;

use super::Milli;

/// The rival running away: who, and the threat of its victory (0…1000).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Threat {
    pub rival: PlayerId,
    pub level: Milli,
}

/// The strategist's stance toward one city-state (0…1000 each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stance {
    pub city_state: PlayerId,
    /// Its cities are targets of the war machine (`weights::stance::PREY`).
    pub prey: Milli,
    /// It is courted: pledged to at influence ≥ 0 (`weights::stance::CLIENT`).
    pub client: Milli,
}

/// What the strategist wishes of the planners this turn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wishes {
    pub supply: SupplyNeed,
    pub threat: Option<Threat>,
    pub city_states: Vec<Stance>,
}

impl Wishes {
    /// The stance toward `city_state`, if the strategist has one.
    pub fn stance(&self, city_state: PlayerId) -> Option<&Stance> {
        self.city_states.iter().find(|s| s.city_state == city_state)
    }
}

/// The bench's ablation of `strategist-x` (`AURELIA_X_PARTS=supply,threat,stance`, a measurement switch:
/// unset, every part): which outputs the strategist emits. Read once per process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parts {
    pub supply: bool,
    pub threat: bool,
    pub stance: bool,
    /// Within the stance: the prey and the client parts (`prey`, `client` in the list; `stance` = both).
    pub prey: bool,
    pub client: bool,
    /// The style's tilt to tall (`Net::tall_first`).
    pub style: bool,
}

impl Parts {
    pub const ENV: &'static str = "AURELIA_X_PARTS";

    pub fn of_env() -> Self {
        static PARTS: std::sync::OnceLock<Parts> = std::sync::OnceLock::new();
        *PARTS.get_or_init(|| match std::env::var(Self::ENV) {
            Ok(list) if !list.is_empty() => {
                let (prey, client) = (list.contains("prey") || list.contains("stance"), list.contains("client") || list.contains("stance"));
                Parts { supply: list.contains("supply"), threat: list.contains("threat"), stance: prey || client, prey, client, style: list.contains("style") }
            }
            _ => Parts { supply: true, threat: true, stance: true, prey: true, client: true, style: true },
        })
    }
}
