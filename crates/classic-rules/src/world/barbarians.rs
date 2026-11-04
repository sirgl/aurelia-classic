//! Barbarian state: the game option, the camps with
//! their spawn counters and the record of cleared camps (city-state quests
//! read it). Camps are also on the map as the barbarian-camp improvement
//! (the map is what players see); this store holds what the map does not.
//! Authoritative: persisted and hashed.

use crate::ids::{PlayerId, TileId};
use crate::rng::StreamRng;

/// The barbarian constants (defined by the barbarian system).
pub use crate::systems::barbarians::{
    CAMP_GOLD, EARLIEST_RELEASE_ROUND, MIN_DISTANCE_CAMP, MIN_DISTANCE_CITY,
};

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
    /// Camp sites and spawns draw from this stream only.
    pub(crate) rng: StreamRng,
}

impl BarbarianState {
    pub fn new(seed: u64) -> Self {
        Self {
            mode: BarbarianMode::None,
            camps: Vec::new(),
            released: false,
            cleared: Vec::new(),
            rng: StreamRng::new(seed ^ 0xBA4B),
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
    }

    pub fn rng(&self) -> &StreamRng {
        &self.rng
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
    ) -> Self {
        Self {
            mode,
            camps,
            released,
            cleared,
            rng,
        }
    }
}
