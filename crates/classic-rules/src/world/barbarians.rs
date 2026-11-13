//! Barbarian state: the game option, the camps with
//! their spawn counters, the record of cleared camps (city-state quests
//! read it) and the countdowns of rebellions (`systems::revolts`). Camps are also on the map as the barbarian-camp improvement
//! (the map is what players see); this store holds what the map does not.
//! Authoritative: persisted and hashed.

use crate::ids::{PlayerId, TileId};
use crate::rng::StreamRng;

/// The barbarian constants (defined by the barbarian system).
pub use crate::systems::barbarians::{
    CAMP_GOLD, EARLIEST_RELEASE_ROUND, MIN_DISTANCE_CAMP, MIN_DISTANCE_CITY,
};
/// The rebellion constants and the rebels' unit kinds (defined by the revolt system).
pub use crate::systems::revolts::{rebel_kinds, rebels_possible, REBEL_PERCENT_PER_CITY, REVOLT_DELAY_BASE, REVOLT_DELAY_RANDOM};

/// Game option of barbarians: no barbarians, normal, raging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BarbarianMode {
    #[default]
    None,
    Normal,
    Raging,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Camp {
    pub tile: TileId,
    /// Turns of the barbarian player until the camp spawns a unit.
    pub spawn_in: u8,
}

/// A camp cleared by a player in a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClearedCamp {
    pub tile: TileId,
    pub player: PlayerId,
    pub round: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarbarianState {
    pub mode: BarbarianMode,
    /// Camps in the order they appeared.
    pub(crate) camps: Vec<Camp>,
    /// The first wave of camps was placed.
    pub(crate) released: bool,
    /// Cleared camps, oldest first (kept for 30 rounds).
    pub(crate) cleared: Vec<ClearedCamp>,
    /// Camp sites and spawns draw from this stream only (rebellions too).
    pub(crate) rng: StreamRng,
    /// Players whose rebels rise in so many of their turns (ascending by
    /// player; a player at or above the rebellion threshold has none).
    pub(crate) revolts: Vec<(PlayerId, u8)>,
}

impl BarbarianState {
    pub fn new(seed: u64) -> Self {
        Self {
            mode: BarbarianMode::None,
            camps: Vec::new(),
            released: false,
            cleared: Vec::new(),
            rng: StreamRng::new(seed ^ 0xBA4B),
            revolts: Vec::new(),
        }
    }

    /// Turns of `player` left until its rebels rise (None: no rebellion
    /// is coming).
    pub fn revolt_countdown(&self, player: PlayerId) -> Option<u8> {
        self.revolts.iter().find(|(p, _)| *p == player).map(|(_, n)| *n)
    }

    /// Every running rebellion countdown, ascending by player.
    pub fn revolts(&self) -> &[(PlayerId, u8)] {
        &self.revolts
    }

    pub(crate) fn set_revolt(&mut self, player: PlayerId, turns: Option<u8>) {
        let at = self.revolts.binary_search_by_key(&player, |(p, _)| *p);
        match (at, turns) {
            (Ok(i), Some(n)) => self.revolts[i].1 = n,
            (Ok(i), None) => {
                self.revolts.remove(i);
            }
            (Err(i), Some(n)) => self.revolts.insert(i, (player, n)),
            (Err(_), None) => {}
        }
    }

    pub fn camps(&self) -> &[Camp] {
        &self.camps
    }

    pub fn cleared(&self) -> &[ClearedCamp] {
        &self.cleared
    }

    /// No option, camp or record yet (the stream does not count: it moves
    /// only once barbarians play).
    pub fn is_pristine(&self) -> bool {
        self.mode == BarbarianMode::None
            && self.camps.is_empty()
            && !self.released
            && self.cleared.is_empty()
            && self.revolts.is_empty()
    }

    pub fn rng(&self) -> &StreamRng {
        &self.rng
    }

    /// Harness-only: the stream restarted (`ClassicGame::reseed_streams`).
    #[cfg(any(test, feature = "harness"))]
    pub(crate) fn reseed(&mut self, rng: StreamRng) {
        self.rng = rng;
    }

    pub fn released(&self) -> bool {
        self.released
    }

    pub(crate) fn from_parts(
        mode: BarbarianMode,
        camps: Vec<Camp>,
        released: bool,
        cleared: Vec<ClearedCamp>,
        rng: StreamRng,
        revolts: Vec<(PlayerId, u8)>,
    ) -> Self {
        Self {
            mode,
            camps,
            released,
            cleared,
            rng,
            revolts,
        }
    }
}
